//! Executor piece 3: build and send the HTLC transactions:
//! `lock`, `redeem`, and `refund`. Each is a direct translation from a
//! [`NextAction`] chosen by the state machine into a contract call. They do not
//! recalculate what the state machine already decided; they only map structs to
//! parameters and send.
//!
//! Native ETH locks use `newSwap(...).value(amount)`. ERC-20 locks first approve
//! exactly `amount` for the HTLC or Settlement spender, then call without ETH.
//!
//! Critical `contractId` equivalence: the `contractId` kept for refund is
//! computed in Rust and must match the contract exactly:
//! (`keccak256(abi.encode(sender, recipient, token, amount, hashlock, timelock))`).
//! The implementation uses Alloy's ABI encoder and proves equality against the
//! real contract in anvil tests. If this diverges, refund would point at a lock
//! that does not exist.

use crate::exec::signer::Signer;
use crate::verify::Address;
use alloy::primitives::{keccak256, Address as EvmAddress, Bytes, B256, U256};
use alloy::sol;
use alloy::sol_types::SolValue;
use orderbook::eip712;
use orderbook::order::Order;

// HTLC write interface: only the operations the executor sends. Reads live in
// `chain.rs`; this module owns the write path.
sol! {
    #[sol(rpc)]
    interface IHashedTimelockWrite {
        function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
            external payable returns (bytes32 contractId);
        function redeem(bytes32 contractId, bytes32 preimage) external;
        function refund(bytes32 contractId) external;
    }

    #[sol(rpc)]
    interface ISettlementWrite {
        struct Order {
            address maker;
            address sellToken;
            uint256 sellChainId;
            uint256 sellAmount;
            address buyToken;
            uint256 buyChainId;
            uint256 buyAmount;
            uint256 validUntil;
            uint256 nonce;
        }

        function settleLeg(Order order, bytes signature, address recipient, bytes32 hashlock, uint256 timelock)
            external payable returns (bytes32 contractId);
        function refundLeg(bytes32 contractId) external;
        function htlc() external view returns (address);
    }

    #[sol(rpc)]
    interface IERC20Write {
        function approve(address spender, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function balanceOf(address owner) external view returns (uint256);
    }
}

/// Errors while building or sending a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxError {
    /// Amount zero is rejected before any approval or lock attempt.
    ZeroAmount,
    /// Sending failed or the receipt could not be obtained.
    Send(String),
}

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxError::ZeroAmount => write!(f, "amount must be greater than zero"),
            TxError::Send(e) => write!(f, "failed to send/mine tx: {e}"),
        }
    }
}
impl std::error::Error for TxError {}

/// Lock result: `contract_id` for observation/refund and the tx hash for
/// confirmation-depth tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locked {
    pub contract_id: [u8; 32],
    pub tx_hash: [u8; 32],
}

#[derive(Clone)]
pub struct SettlementLockConfig {
    pub settlement: Address,
    pub maker_private_key: [u8; 32],
    pub sell_chain_id: u64,
    pub buy_token: Address,
    pub buy_chain_id: u64,
    pub buy_amount: u128,
    pub valid_until: u64,
    pub nonce: u64,
}

/// Computes `contractId` exactly like the contract:
/// `keccak256(abi.encode(sender, recipient, token, amount, hashlock, timelock))`.
///
/// This uses Alloy's ABI encoder instead of manually recreating the byte layout.
/// Equivalence with the contract is proven by an anvil test. Pure, no network.
pub fn compute_contract_id(
    sender: Address,
    recipient: Address,
    token: Address,
    amount: u128,
    hashlock: [u8; 32],
    timelock: u64,
) -> [u8; 32] {
    let encoded = (
        EvmAddress::from(sender),
        EvmAddress::from(recipient),
        EvmAddress::from(token),
        U256::from(amount),
        B256::from(hashlock),
        U256::from(timelock),
    )
        .abi_encode();
    keccak256(encoded).0
}

/// Locks my leg: `newSwap(...)` with `value = amount` (native ETH). Returns the
/// computed contractId and tx hash. `recipient` is the counterparty that can
/// redeem this leg.
pub async fn lock(
    signer: &Signer,
    htlc: Address,
    recipient: Address,
    token: Address,
    amount: u128,
    hashlock: [u8; 32],
    timelock: u64,
) -> Result<Locked, TxError> {
    if amount == 0 {
        return Err(TxError::ZeroAmount);
    }
    // On-chain sender is the signer address, which is what the contractId binds to.
    let contract_id = compute_contract_id(
        signer.address(),
        recipient,
        token,
        amount,
        hashlock,
        timelock,
    );

    let htlc_c = IHashedTimelockWrite::new(EvmAddress::from(htlc), signer.provider());
    let mut call = htlc_c.newSwap(
        EvmAddress::from(recipient),
        EvmAddress::from(token),
        U256::from(amount),
        B256::from(hashlock),
        U256::from(timelock),
    );
    if token == [0u8; 20] {
        call = call.value(U256::from(amount));
    } else {
        approve_exact(signer, token, htlc, amount).await?;
    }
    let receipt = call
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;

    Ok(Locked {
        contract_id,
        tx_hash: receipt.transaction_hash.0,
    })
}

/// Locks my leg through Settlement (Approach A): the signed order binds maker,
/// token, amount, and chain; Settlement calls the canonical HTLC as itself.
pub async fn lock_via_settlement(
    signer: &Signer,
    config: &SettlementLockConfig,
    recipient: Address,
    token: Address,
    amount: u128,
    hashlock: [u8; 32],
    timelock: u64,
) -> Result<Locked, TxError> {
    if amount == 0 {
        return Err(TxError::ZeroAmount);
    }

    let order = Order {
        maker: signer.address(),
        sell_token: token,
        sell_chain_id: config.sell_chain_id,
        sell_amount: amount,
        buy_token: config.buy_token,
        buy_chain_id: config.buy_chain_id,
        buy_amount: config.buy_amount,
        valid_until: config.valid_until,
        nonce: config.nonce,
        created_at: 0,
    };
    let signature = eip712::sign(&order, &config.maker_private_key);

    let contract_id = compute_contract_id(
        config.settlement,
        recipient,
        token,
        amount,
        hashlock,
        timelock,
    );

    let settlement = ISettlementWrite::new(EvmAddress::from(config.settlement), signer.provider());
    let sol_order = ISettlementWrite::Order {
        maker: EvmAddress::from(order.maker),
        sellToken: EvmAddress::from(order.sell_token),
        sellChainId: U256::from(order.sell_chain_id),
        sellAmount: U256::from(order.sell_amount),
        buyToken: EvmAddress::from(order.buy_token),
        buyChainId: U256::from(order.buy_chain_id),
        buyAmount: U256::from(order.buy_amount),
        validUntil: U256::from(order.valid_until),
        nonce: U256::from(order.nonce),
    };
    let mut call = settlement.settleLeg(
        sol_order,
        Bytes::copy_from_slice(&signature),
        EvmAddress::from(recipient),
        B256::from(hashlock),
        U256::from(timelock),
    );
    if token == [0u8; 20] {
        call = call.value(U256::from(amount));
    } else {
        approve_exact(signer, token, config.settlement, amount).await?;
    }
    let receipt = call
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;

    Ok(Locked {
        contract_id,
        tx_hash: receipt.transaction_hash.0,
    })
}

async fn approve_exact(
    signer: &Signer,
    token: Address,
    spender: Address,
    amount: u128,
) -> Result<(), TxError> {
    let token_c = IERC20Write::new(EvmAddress::from(token), signer.provider());
    token_c
        .approve(EvmAddress::from(spender), U256::from(amount))
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;
    let allowance = token_c
        .allowance(
            EvmAddress::from(signer.address()),
            EvmAddress::from(spender),
        )
        .call()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;
    if allowance != U256::from(amount) {
        return Err(TxError::Send(format!(
            "ERC-20 allowance mismatch after approve: expected {amount}, got {allowance}"
        )));
    }
    Ok(())
}

/// Redeems the counterparty leg by revealing `secret`: `redeem(contract_id, secret)`.
/// Returns the tx hash. `contract_id` belongs to the counterparty lock.
pub async fn redeem(
    signer: &Signer,
    htlc: Address,
    contract_id: [u8; 32],
    secret: [u8; 32],
) -> Result<[u8; 32], TxError> {
    let htlc_c = IHashedTimelockWrite::new(EvmAddress::from(htlc), signer.provider());
    let receipt = htlc_c
        .redeem(B256::from(contract_id), B256::from(secret))
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;
    Ok(receipt.transaction_hash.0)
}

/// Refunds my leg after timelock expiry: `refund(contract_id)`.
/// Returns the tx hash. `contract_id` belongs to my own lock.
pub async fn refund(
    signer: &Signer,
    htlc: Address,
    contract_id: [u8; 32],
) -> Result<[u8; 32], TxError> {
    let htlc_c = IHashedTimelockWrite::new(EvmAddress::from(htlc), signer.provider());
    let receipt = htlc_c
        .refund(B256::from(contract_id))
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;
    Ok(receipt.transaction_hash.0)
}

pub async fn refund_via_settlement(
    signer: &Signer,
    settlement: Address,
    contract_id: [u8; 32],
) -> Result<[u8; 32], TxError> {
    let settlement_c = ISettlementWrite::new(EvmAddress::from(settlement), signer.provider());
    let receipt = settlement_c
        .refundLeg(B256::from(contract_id))
        .send()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?
        .get_receipt()
        .await
        .map_err(|e| TxError::Send(format!("{e}")))?;
    Ok(receipt.transaction_hash.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{ChainVerifier, LockObservation, RpcVerifier};
    use alloy::network::EthereumWallet;
    use alloy::node_bindings::Anvil;
    use alloy::providers::ProviderBuilder;
    use alloy::signers::local::PrivateKeySigner;
    use alloy::sol;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::time::{SystemTime, UNIX_EPOCH};

    sol! {
        #[sol(rpc)]
        Settlement,
        "../contracts/out/Settlement.sol/Settlement.json"
    }

    sol! {
        #[sol(rpc)]
        MockERC20,
        "../contracts/out/MockERC20.sol/MockERC20.json"
    }

    // Anvil key #0 from the default mnemonic, same as the signer test.
    const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // Proves Rust contractId == contract contractId against a real anvil deploy,
    // then performs a lock->redeem round trip to prove the returned contractId
    // addresses the real lock.
    #[tokio::test]
    async fn contract_id_matches_contract_and_lock_redeem_roundtrip() {
        let anvil = Anvil::new().spawn(); // chain-id 31337, allowed by the signer guard.
        let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
        let sender = pk.address();
        let wallet = EthereumWallet::from(pk);
        let deploy_provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());

        let htlc = HashedTimelock::deploy(deploy_provider.clone())
            .await
            .unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();

        // Executor signer with the same key; the allowlist guard passes on 31337.
        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        assert_eq!(
            signer.address(),
            sender.into_array(),
            "sanity: same account"
        );

        let me = signer.address(); // recipient = self; anyone can redeem with the preimage.
        let token = [0u8; 20];
        let amount: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        // (1) Rust compute vs on-chain compute: the critical equivalence.
        let rust_cid = compute_contract_id(me, me, token, amount, hashlock, timelock);
        let onchain_cid: B256 = htlc
            .computeContractId(
                EvmAddress::from(me),
                EvmAddress::from(me),
                EvmAddress::ZERO,
                U256::from(amount),
                B256::from(hashlock),
                U256::from(timelock),
            )
            .call()
            .await
            .unwrap();
        assert_eq!(
            rust_cid, onchain_cid.0,
            "Rust computeContractId must match the contract byte-for-byte"
        );

        // (2) Real lock returns exactly that contractId.
        let locked = lock(&signer, htlc_addr, me, token, amount, hashlock, timelock)
            .await
            .unwrap();
        assert_eq!(
            locked.contract_id, rust_cid,
            "lock returns the computed contractId"
        );

        // (3) The returned contractId addresses the real lock (reads Confirmed).
        let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        assert!(
            matches!(
                v.observe_lock(htlc_addr, locked.contract_id, 1)
                    .await
                    .unwrap(),
                LockObservation::Confirmed(_)
            ),
            "the computed contractId finds the created lock"
        );

        // (4) Real redeem removes the lock (Absent), proving the redeem mapping.
        redeem(&signer, htlc_addr, locked.contract_id, preimage)
            .await
            .unwrap();
        assert_eq!(
            v.observe_lock(htlc_addr, locked.contract_id, 1)
                .await
                .unwrap(),
            LockObservation::Absent,
            "after redeem -> Absent"
        );
    }

    #[tokio::test]
    async fn settlement_lock_uses_settlement_as_htlc_sender() {
        let anvil = Anvil::new().spawn();
        let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
        let wallet = EthereumWallet::from(pk);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());

        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let settlement = Settlement::deploy(provider.clone(), *htlc.address())
            .await
            .unwrap();

        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let recipient = [0x7Au8; 20];
        let token = [0u8; 20];
        let amount: u128 = 500;
        let preimage = [0x51u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        let locked = lock_via_settlement(
            &signer,
            &SettlementLockConfig {
                settlement: (*settlement.address()).into_array(),
                maker_private_key: hex_key(ANVIL_KEY0),
                sell_chain_id: 31337,
                buy_token: token,
                buy_chain_id: 31338,
                buy_amount: amount,
                valid_until: timelock,
                nonce: 1,
            },
            recipient,
            token,
            amount,
            hashlock,
            timelock,
        )
        .await
        .unwrap();

        let onchain = htlc
            .getSwap(B256::from(locked.contract_id))
            .call()
            .await
            .unwrap();
        assert_eq!(onchain.sender, *settlement.address());
        assert_eq!(onchain.recipient, EvmAddress::from(recipient));
        assert_eq!(onchain.token, EvmAddress::ZERO);
        assert_eq!(onchain.amount, U256::from(amount));
        assert_eq!(onchain.hashlock, B256::from(hashlock));
        assert_eq!(onchain.timelock, U256::from(timelock));
    }

    #[tokio::test]
    async fn settlement_lock_supports_erc20_with_exact_allowance() {
        let anvil = Anvil::new().spawn();
        let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
        let wallet = EthereumWallet::from(pk);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());

        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let settlement = Settlement::deploy(provider.clone(), *htlc.address())
            .await
            .unwrap();
        let token = MockERC20::deploy(provider.clone()).await.unwrap();

        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let token_addr = (*token.address()).into_array();
        let amount: u128 = 750;
        token
            .mint(EvmAddress::from(signer.address()), U256::from(amount))
            .send()
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();

        let recipient = [0x7Au8; 20];
        let preimage = [0x61u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        let locked = lock_via_settlement(
            &signer,
            &SettlementLockConfig {
                settlement: (*settlement.address()).into_array(),
                maker_private_key: hex_key(ANVIL_KEY0),
                sell_chain_id: 31337,
                buy_token: [0u8; 20],
                buy_chain_id: 31338,
                buy_amount: amount,
                valid_until: timelock,
                nonce: 2,
            },
            recipient,
            token_addr,
            amount,
            hashlock,
            timelock,
        )
        .await
        .unwrap();

        let onchain = htlc
            .getSwap(B256::from(locked.contract_id))
            .call()
            .await
            .unwrap();
        assert_eq!(onchain.sender, *settlement.address());
        assert_eq!(onchain.recipient, EvmAddress::from(recipient));
        assert_eq!(onchain.token, *token.address());
        assert_eq!(onchain.amount, U256::from(amount));
        assert_eq!(
            token
                .allowance(EvmAddress::from(signer.address()), *settlement.address())
                .call()
                .await
                .unwrap(),
            U256::ZERO
        );
        assert_eq!(
            token.balanceOf(*htlc.address()).call().await.unwrap(),
            U256::from(amount)
        );
        assert_eq!(
            token.balanceOf(*settlement.address()).call().await.unwrap(),
            U256::ZERO
        );
    }

    // Real refund after expiry: advances anvil time past the timelock and proves
    // refund returns funds for the same contractId stored by lock.
    #[tokio::test]
    async fn refund_after_expiry_real() {
        use alloy::providers::Provider;

        let anvil = Anvil::new().spawn();
        let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
        let wallet = EthereumWallet::from(pk);
        let deploy_provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());
        let htlc = HashedTimelock::deploy(deploy_provider.clone())
            .await
            .unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();

        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let me = signer.address();
        let amount: u128 = 777;
        let hashlock = hashlock_from_preimage(&[0x01u8; 32]);
        let timelock = now_unix() + 100;

        let locked = lock(
            &signer, htlc_addr, me, [0u8; 20], amount, hashlock, timelock,
        )
        .await
        .unwrap();

        let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        assert!(matches!(
            v.observe_lock(htlc_addr, locked.contract_id, 1)
                .await
                .unwrap(),
            LockObservation::Confirmed(_)
        ));

        // Move past the timelock and mine a block to fix the new time.
        let p = signer.provider();
        let _: serde_json::Value = p
            .raw_request("evm_increaseTime".into(), (3601u64,))
            .await
            .unwrap();
        let _: serde_json::Value = p.raw_request("evm_mine".into(), ()).await.unwrap();

        // Real refund removes the lock (Absent). Same contractId as the lock.
        refund(&signer, htlc_addr, locked.contract_id)
            .await
            .unwrap();
        assert_eq!(
            v.observe_lock(htlc_addr, locked.contract_id, 1)
                .await
                .unwrap(),
            LockObservation::Absent,
            "after refund -> Absent"
        );
    }

    #[tokio::test]
    async fn lock_rejects_zero_amount_before_sending() {
        let anvil = Anvil::new().spawn();
        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let r = lock(
            &signer,
            [0x11; 20],
            [0x7A; 20],
            [0x22; 20],
            0,
            [0xAB; 32],
            now_unix() + 3600,
        )
        .await;
        assert_eq!(r, Err(TxError::ZeroAmount));
    }

    fn hex_key(s: &str) -> [u8; 32] {
        let bytes = hex::decode(s.trim_start_matches("0x")).unwrap();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }
}

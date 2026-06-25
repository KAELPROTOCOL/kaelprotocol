//! Peça 3 do executor: montar e enviar as três transações do HTLC —
//! `lock` / `redeem` / `refund`. Cada uma é TRADUÇÃO PURA de um [`NextAction`]
//! (decidido pela máquina de estados) numa chamada do contrato. NÃO recalculam
//! nada que a máquina já decidiu — só traduzem struct em parâmetros e enviam.
//!
//! MVP: **ETH nativo apenas** (`token = 0x0`); o lock é `newSwap(...).value(amount)`.
//! ERC-20 (approve + newSwap via `_safeTransferFrom`) é trabalho posterior — aqui
//! um `token != 0x0` é rejeitado explicitamente, não silenciosamente ignorado.
//!
//! EQUIVALÊNCIA DO contractId (crítica): o `contractId` que guardamos para o
//! refund é computado em Rust e DEVE bater EXATAMENTE com o do contrato
//! (`keccak256(abi.encode(sender, recipient, token, amount, hashlock, timelock))`).
//! Computamos com o MESMO encoder ABI do alloy (o que o solc usa) e provamos a
//! igualdade contra o contrato real no anvil (ver testes). Se divergir, o refund
//! aponta para uma trava que não existe — por isso é provado, não presumido.

use crate::exec::signer::Signer;
use crate::verify::Address;
use alloy::primitives::{keccak256, Address as EvmAddress, B256, U256};
use alloy::sol;
use alloy::sol_types::SolValue;

// Interface de ESCRITA do HTLC (só as três operações que o executor envia). A
// leitura (getSwap) vive em `chain.rs`; aqui é só o caminho de escrita.
sol! {
    #[sol(rpc)]
    interface IHashedTimelockWrite {
        function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
            external payable returns (bytes32 contractId);
        function redeem(bytes32 contractId, bytes32 preimage) external;
        function refund(bytes32 contractId) external;
    }
}

/// Erros ao montar/enviar uma transação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxError {
    /// MVP é ETH nativo (`token = 0x0`); ERC-20 ainda não é suportado.
    TokenNotSupportedYet,
    /// falha ao enviar a tx ou obter o recibo (rede, revert do contrato, etc.).
    Send(String),
}

impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxError::TokenNotSupportedYet => {
                write!(
                    f,
                    "MVP só suporta ETH nativo (token=0x0); ERC-20 é trabalho futuro"
                )
            }
            TxError::Send(e) => write!(f, "falha ao enviar/minerar a tx: {e}"),
        }
    }
}
impl std::error::Error for TxError {}

/// Resultado de um lock: o `contract_id` (guardado para observar/refundar a
/// minha perna) e o hash da tx (para a peça de confirmação medir profundidade).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locked {
    pub contract_id: [u8; 32],
    pub tx_hash: [u8; 32],
}

/// Computa o `contractId` EXATAMENTE como o contrato:
/// `keccak256(abi.encode(sender, recipient, token, amount, hashlock, timelock))`.
///
/// Usa o encoder ABI do alloy (idêntico ao do solc) — NÃO reimplementa o layout
/// de bytes na mão. A equivalência com o contrato é provada por teste contra o
/// anvil. Puro, sem rede.
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

/// Trava a minha perna: `newSwap(...)` com `value = amount` (ETH nativo). Devolve
/// o [`Locked`] (contractId computado + hash da tx). O `recipient` é quem pode
/// resgatar a minha perna = a contraparte.
pub async fn lock(
    signer: &Signer,
    htlc: Address,
    recipient: Address,
    token: Address,
    amount: u128,
    hashlock: [u8; 32],
    timelock: u64,
) -> Result<Locked, TxError> {
    if token != [0u8; 20] {
        return Err(TxError::TokenNotSupportedYet);
    }
    // o sender on-chain será o endereço do signer → é com ele que o contractId casa.
    let contract_id = compute_contract_id(
        signer.address(),
        recipient,
        token,
        amount,
        hashlock,
        timelock,
    );

    let htlc_c = IHashedTimelockWrite::new(EvmAddress::from(htlc), signer.provider());
    let receipt = htlc_c
        .newSwap(
            EvmAddress::from(recipient),
            EvmAddress::from(token),
            U256::from(amount),
            B256::from(hashlock),
            U256::from(timelock),
        )
        .value(U256::from(amount))
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

/// Resgata a perna OPOSTA revelando o `secret`: `redeem(contract_id, secret)`.
/// Devolve o hash da tx. (O `contract_id` é o da trava da contraparte.)
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

/// Reembolsa a MINHA perna após a expiração do timelock: `refund(contract_id)`.
/// Devolve o hash da tx. (O `contract_id` é o da minha própria trava.)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{ChainVerifier, LockObservation, RpcVerifier};
    use alloy::network::EthereumWallet;
    use alloy::node_bindings::Anvil;
    use alloy::providers::ProviderBuilder;
    use alloy::signers::local::PrivateKeySigner;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Chave #0 do anvil (mnemônico default) — a mesma do teste do signer.
    const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // ---- O TESTE QUE IMPORTA: contractId do Rust == contractId do contrato ----
    // Prova a equivalência contra o anvil real (deploy do contrato, lê o que ELE
    // computa, compara com o Rust), e faz o round-trip lock→redeem provando que o
    // contractId devolvido endereça a trava de verdade.
    #[tokio::test]
    async fn contract_id_matches_contract_and_lock_redeem_roundtrip() {
        let anvil = Anvil::new().spawn(); // chain-id 31337 (na allowlist do signer)
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

        // o signer do executor (mesma chave) — guard allowlist passa em 31337.
        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        assert_eq!(signer.address(), sender.into_array(), "sanity: mesma conta");

        let me = signer.address(); // recipient = eu (qualquer um resgata com o preimage)
        let token = [0u8; 20];
        let amount: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        // (1) Rust compute vs on-chain compute — a EQUIVALÊNCIA crítica.
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
            "computeContractId do Rust DEVE bater byte a byte com o do contrato"
        );

        // (2) lock real → devolve EXATAMENTE esse contractId.
        let locked = lock(&signer, htlc_addr, me, token, amount, hashlock, timelock)
            .await
            .unwrap();
        assert_eq!(
            locked.contract_id, rust_cid,
            "lock devolve o contractId computado"
        );

        // (3) o contractId devolvido endereça a trava REAL (lê Confirmed).
        let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        assert!(
            matches!(
                v.observe_lock(htlc_addr, locked.contract_id, 1)
                    .await
                    .unwrap(),
                LockObservation::Confirmed(_)
            ),
            "o contractId computado encontra a trava criada"
        );

        // (4) redeem real → a trava some (Absent). Prova a tradução de redeem.
        redeem(&signer, htlc_addr, locked.contract_id, preimage)
            .await
            .unwrap();
        assert_eq!(
            v.observe_lock(htlc_addr, locked.contract_id, 1)
                .await
                .unwrap(),
            LockObservation::Absent,
            "após redeem → Absent"
        );
    }

    // ---- refund REAL após expiração (o caminho de SEGURANÇA) ----
    // Viaja no tempo do anvil além do timelock e prova que refund devolve os
    // fundos — e que o contractId do refund é o MESMO que o lock guardou.
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

        // viaja além do timelock e minera um bloco para fixar o novo tempo.
        let p = signer.provider();
        let _: serde_json::Value = p
            .raw_request("evm_increaseTime".into(), (3601u64,))
            .await
            .unwrap();
        let _: serde_json::Value = p.raw_request("evm_mine".into(), ()).await.unwrap();

        // refund real → a trava some (Absent). Mesmo contractId do lock.
        refund(&signer, htlc_addr, locked.contract_id)
            .await
            .unwrap();
        assert_eq!(
            v.observe_lock(htlc_addr, locked.contract_id, 1)
                .await
                .unwrap(),
            LockObservation::Absent,
            "após refund → Absent"
        );
    }

    // ---- ETH-only: token != 0x0 é rejeitado explicitamente (sem rede) ----
    #[tokio::test]
    async fn lock_rejects_erc20_token_for_now() {
        let anvil = Anvil::new().spawn();
        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let r = lock(
            &signer,
            [0x11; 20],
            [0x7A; 20],
            [0x22; 20], // token != 0x0
            500,
            [0xAB; 32],
            now_unix() + 3600,
        )
        .await;
        assert_eq!(r, Err(TxError::TokenNotSupportedYet));
    }
}

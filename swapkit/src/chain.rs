//!
//! substituir o RPC depois, SEM reescrever a carteira.
//!

use crate::verify::{Address, ObservedLock};

/// Erros ao ler a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    BadUrl(String),
    Rpc(String),
    Decode(String),
    AmountOverflow,
    TimelockOverflow,
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ChainError {}

///
/// - [`Confirmed`](LockObservation::Confirmed): trava ativa E suficientemente
///
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockObservation {
    Confirmed(ObservedLock),
    Shallow,
    Absent,
}

pub fn confirmations(head: u64, block: u64) -> u64 {
    head.saturating_sub(block) + 1
}

impl LockObservation {
    pub fn for_gate(&self) -> Option<ObservedLock> {
        match self {
            LockObservation::Confirmed(o) => Some(*o),
            LockObservation::Shallow | LockObservation::Absent => None,
        }
    }
}

/// consome.
///
#[allow(async_fn_in_trait)]
pub trait ChainVerifier {
    /// exigindo `min_confirmations` de PROFUNDIDADE (anti-reorg).
    ///
    /// antigo (profundidade 0); `min_confirmations = 0` aceita qualquer coisa
    /// ativa. Erros de leitura viram [`ChainError`].
    async fn observe_lock(
        &self,
        htlc_address: Address,
        contract_id: [u8; 32],
        min_confirmations: u64,
    ) -> Result<LockObservation, ChainError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSwap {
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: u128,
    pub hashlock: [u8; 32],
    pub timelock: u64,
    pub withdrawn: bool,
    pub refunded: bool,
}

/// Pure Swap to ObservedLock mapping.
///
/// MODELAGEM DE `exists`: reportamos `exists = true` apenas para uma trava
pub fn observed_from_swap(s: &RawSwap) -> ObservedLock {
    let active = s.sender != [0u8; 20] && !s.withdrawn && !s.refunded;
    ObservedLock {
        hashlock: s.hashlock,
        token: s.token,
        amount: s.amount,
        recipient: s.recipient,
        timelock: s.timelock,
        sender: s.sender,
        exists: active,
    }
}

// ------------------------------------------------------------------------
// ------------------------------------------------------------------------

use alloy::primitives::{Address as EvmAddress, B256, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::types::Filter;
use alloy::sol;
use alloy::sol_types::SolEvent;

// que filtra o log certo.
sol! {
    #[sol(rpc)]
    interface IHashedTimelock {
        struct Swap {
            address sender;
            address recipient;
            address token;
            uint256 amount;
            bytes32 hashlock;
            uint256 timelock;
            bool withdrawn;
            bool refunded;
        }
        event LogNewSwap(
            bytes32 indexed contractId,
            address indexed sender,
            address indexed recipient,
            address token,
            uint256 amount,
            bytes32 hashlock,
            uint256 timelock
        );
        function getSwap(bytes32 contractId) external view returns (Swap memory);
    }
}

/// Leitor de chain via RPC EVM (alloy).
///
pub struct RpcVerifier {
    provider: DynProvider,
}

impl RpcVerifier {
    pub fn new(rpc_url: &str) -> Result<Self, ChainError> {
        let url = rpc_url
            .parse()
            .map_err(|e| ChainError::BadUrl(format!("{e}")))?;
        let provider = ProviderBuilder::new().connect_http(url).erased();
        Ok(Self { provider })
    }
}

fn u256_to_u128(v: U256) -> Result<u128, ChainError> {
    u128::try_from(v).map_err(|_| ChainError::AmountOverflow)
}
fn u256_to_u64(v: U256) -> Result<u64, ChainError> {
    u64::try_from(v).map_err(|_| ChainError::TimelockOverflow)
}

impl RpcVerifier {
    /// Bloco em que a trava `contract_id` foi CRIADA, lido do `LogNewSwap`
    /// (filtrado pelo `contractId` indexado). `None` se nenhum log for achado.
    ///
    /// rastrear a partir de um checkpoint evita a varredura completa.
    async fn creation_block(
        &self,
        htlc_address: Address,
        contract_id: [u8; 32],
    ) -> Result<Option<u64>, ChainError> {
        let filter = Filter::new()
            .address(EvmAddress::from(htlc_address))
            .event_signature(IHashedTimelock::LogNewSwap::SIGNATURE_HASH)
            .topic1(B256::from(contract_id))
            .from_block(0u64);
        let logs = self
            .provider
            .get_logs(&filter)
            .await
            .map_err(|e| ChainError::Rpc(format!("{e}")))?;
        Ok(logs.first().and_then(|l| l.block_number))
    }
}

impl ChainVerifier for RpcVerifier {
    async fn observe_lock(
        &self,
        htlc_address: Address,
        contract_id: [u8; 32],
        min_confirmations: u64,
    ) -> Result<LockObservation, ChainError> {
        let htlc = IHashedTimelock::new(EvmAddress::from(htlc_address), &self.provider);
        let swap = htlc
            .getSwap(B256::from(contract_id))
            .call()
            .await
            .map_err(|e| ChainError::Rpc(format!("{e}")))?;

        let raw = RawSwap {
            sender: swap.sender.into_array(),
            recipient: swap.recipient.into_array(),
            token: swap.token.into_array(),
            amount: u256_to_u128(swap.amount)?,
            hashlock: swap.hashlock.0,
            timelock: u256_to_u64(swap.timelock)?,
            withdrawn: swap.withdrawn,
            refunded: swap.refunded,
        };
        let observed = observed_from_swap(&raw);

        // No active lock: nonexistent, withdrawn, or refunded. Return Absent.
        if !observed.exists {
            return Ok(LockObservation::Absent);
        }

        let head = self
            .provider
            .get_block_number()
            .await
            .map_err(|e| ChainError::Rpc(format!("{e}")))?;
        let confs = match self.creation_block(htlc_address, contract_id).await? {
            Some(cb) => confirmations(head, cb),
            None => 0,
        };

        if confs >= min_confirmations {
            Ok(LockObservation::Confirmed(observed))
        } else {
            Ok(LockObservation::Shallow)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{
        verify_counterparty_leg, LegExpectation, Role, UnsafeReason, VerifyOutcome,
    };

    fn h(b: u8) -> [u8; 32] {
        [b; 32]
    }
    fn a(b: u8) -> [u8; 20] {
        [b; 20]
    }

    fn active_swap() -> RawSwap {
        RawSwap {
            sender: a(0x3A),    // maker locked
            recipient: a(0x7A), // paga o taker
            token: a(0x22),
            amount: 500,
            hashlock: h(0xAB),
            timelock: 1800,
            withdrawn: false,
            refunded: false,
        }
    }

    // --- mapeamento: campos certos para uma trava ativa ---
    #[test]
    fn maps_active_swap_fields() {
        let o = observed_from_swap(&active_swap());
        assert!(o.exists);
        assert_eq!(o.hashlock, h(0xAB));
        assert_eq!(o.token, a(0x22));
        assert_eq!(o.amount, 500);
        assert_eq!(o.recipient, a(0x7A));
        assert_eq!(o.timelock, 1800);
        assert_eq!(o.sender, a(0x3A));
    }

    // --- exists = false: trava inexistente (sender zero) ---
    #[test]
    fn nonexistent_swap_is_not_exists() {
        let mut s = active_swap();
        s.sender = [0u8; 20]; // getSwap de um id inexistente devolve struct zerada
        let o = observed_from_swap(&s);
        assert!(!o.exists);
    }

    #[test]
    fn withdrawn_swap_is_not_exists() {
        let mut s = active_swap();
        s.withdrawn = true;
        assert!(!observed_from_swap(&s).exists);
    }

    #[test]
    fn refunded_swap_is_not_exists() {
        let mut s = active_swap();
        s.refunded = true;
        assert!(!observed_from_swap(&s).exists);
    }

    //     RPC mockada alimenta verify_counterparty_leg corretamente. ---
    #[test]
    fn parsed_lock_feeds_verification_safe() {
        let observed = observed_from_swap(&active_swap());

        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            now: 0,
            role: Role::Taker,
        };
        assert_eq!(
            verify_counterparty_leg(&exp, &observed),
            VerifyOutcome::Safe
        );
    }

    #[test]
    fn parsed_lock_feeds_verification_unsafe() {
        let mut s = active_swap();
        s.hashlock = h(0x01);
        let observed = observed_from_swap(&s);

        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            now: 0,
            role: Role::Taker,
        };
        assert_eq!(
            verify_counterparty_leg(&exp, &observed),
            VerifyOutcome::Unsafe(UnsafeReason::HashlockMismatch)
        );
    }

    #[test]
    fn withdrawn_lock_verifies_as_lock_not_found() {
        let mut s = active_swap();
        s.withdrawn = true;
        let observed = observed_from_swap(&s); // exists = false
        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            now: 0,
            role: Role::Taker,
        };
        assert_eq!(
            verify_counterparty_leg(&exp, &observed),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }

    #[test]
    fn for_gate_only_confirmed_is_observed() {
        let o = observed_from_swap(&active_swap());
        assert_eq!(LockObservation::Confirmed(o).for_gate(), Some(o));
        assert_eq!(LockObservation::Shallow.for_gate(), None);
        assert_eq!(LockObservation::Absent.for_gate(), None);
    }

    // ----------------------------------------------------------------
    // Esta prova sobe anvil, faz deploy do HTLC, cria uma trava de verdade, e:
    // ----------------------------------------------------------------
    use crate::sm::{next_action, NextAction, SwapContext, SwapState};
    use alloy::network::EthereumWallet;
    use alloy::node_bindings::Anvil;
    use alloy::primitives::{Address as EvmAddr, B256, U256};
    use alloy::providers::ProviderBuilder;
    use alloy::signers::local::PrivateKeySigner;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[tokio::test]
    async fn rpc_verifier_reads_real_chain_and_drives_wallet() {
        // --- sobe anvil + carteira ---
        let anvil = Anvil::new().chain_id(10).spawn();
        let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
        let sender = signer.address();
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());

        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let me = [0x7Au8; 20]; // ME (taker): who can redeem the maker leg
        let amount_u128: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock_secs = now_unix() + 3600;

        htlc.newSwap(
            EvmAddr::from(me),
            EvmAddr::ZERO, // ETH nativo
            U256::from(amount_u128),
            B256::from(hashlock),
            U256::from(timelock_secs),
        )
        .value(U256::from(amount_u128))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

        let cid: B256 = htlc
            .computeContractId(
                sender,
                EvmAddr::from(me),
                EvmAddr::ZERO,
                U256::from(amount_u128),
                B256::from(hashlock),
                U256::from(timelock_secs),
            )
            .call()
            .await
            .unwrap();

        // === LEITURA REAL via RpcVerifier (o que o #[ignore] nunca provava) ===
        let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();
        // Confirmed (equivalente ao comportamento antigo de profundidade 0).
        let obs = match v.observe_lock(htlc_addr, cid.0, 1).await.unwrap() {
            LockObservation::Confirmed(o) => o,
            other => panic!("esperava Confirmed da chain real, veio {other:?}"),
        };

        assert!(obs.exists, "trava ativa lida da chain real");
        assert_eq!(obs.hashlock, hashlock);
        assert_eq!(obs.amount, amount_u128);
        assert_eq!(obs.recipient, me);
        assert_eq!(obs.token, [0u8; 20]);
        assert_eq!(obs.sender, sender.into_array());
        assert_eq!(obs.timelock, timelock_secs);

        let exp = LegExpectation {
            expected_hashlock: hashlock,
            expected_token: [0u8; 20],
            expected_amount: amount_u128,
            expected_recipient: me,
            my_timelock: obs.timelock + 100, // my long leg
            min_gap: 50,
            now: now_unix(), // well before expiry: safe window
            role: Role::Taker,
        };
        assert_eq!(verify_counterparty_leg(&exp, &obs), VerifyOutcome::Safe);

        let ctx = SwapContext {
            role: Role::Taker,
            my_token: [0x11; 20],
            my_amount: 1000,
            my_timelock: obs.timelock + 100,
            my_recipient: sender.into_array(),
            cp_token: [0u8; 20],
            cp_amount: amount_u128,
            me,
            min_gap: 50,
            hashlock: Some(hashlock),
            secret: Some(preimage),
            my_leg_locked: true,
            counterparty_lock: Some(obs),
            revealed_secret: None,
            now: now_unix(),
        };
        assert_eq!(
            next_action(&SwapState::MyLegLocked, &ctx),
            NextAction::RedeemCounterpartyLeg { secret: preimage },
            "a leitura real, verificada Safe, leva o taker a resgatar"
        );

        let absent = v.observe_lock(htlc_addr, [0xFFu8; 32], 1).await.unwrap();
        assert_eq!(
            absent,
            LockObservation::Absent,
            "nonexistent lock -> Absent"
        );
        // (que um ObservedLock inexistente verifica como LockNotFound continua
        assert_eq!(absent.for_gate(), None);
    }

    // ----------------------------------------------------------------
    // ----------------------------------------------------------------
    #[tokio::test]
    async fn observe_lock_depth_gate_shallow_then_confirmed() {
        use alloy::rpc::types::TransactionRequest;

        let anvil = Anvil::new().spawn(); // chain-id default 31337
        let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
        let sender = signer.address();
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url());

        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let me = [0x7Au8; 20];
        let amount: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        htlc.newSwap(
            EvmAddr::from(me),
            EvmAddr::ZERO,
            U256::from(amount),
            B256::from(hashlock),
            U256::from(timelock),
        )
        .value(U256::from(amount))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

        let cid: B256 = htlc
            .computeContractId(
                sender,
                EvmAddr::from(me),
                EvmAddr::ZERO,
                U256::from(amount),
                B256::from(hashlock),
                U256::from(timelock),
            )
            .call()
            .await
            .unwrap();

        let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();

        // min=0 and min=1 both mean Confirmed.
        assert!(matches!(
            v.observe_lock(htlc_addr, cid.0, 0).await.unwrap(),
            LockObservation::Confirmed(_)
        ));
        assert!(matches!(
            v.observe_lock(htlc_addr, cid.0, 1).await.unwrap(),
            LockObservation::Confirmed(_)
        ));
        assert_eq!(
            v.observe_lock(htlc_addr, cid.0, 2).await.unwrap(),
            LockObservation::Shallow,
            "1 confirmation < 2 required: Shallow"
        );

        let bump = TransactionRequest::default()
            .to(sender)
            .value(U256::from(0));
        provider
            .send_transaction(bump)
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();

        // The same lock now has depth 2, so min=2 becomes Confirmed.
        assert!(
            matches!(
                v.observe_lock(htlc_addr, cid.0, 2).await.unwrap(),
                LockObservation::Confirmed(_)
            ),
            "after advancing the chain, 2 confirmations >= 2 required: Confirmed"
        );
    }
}

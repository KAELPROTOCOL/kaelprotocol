//! O EXECUTOR — a camada que REALIZA no mundo o que a máquina de estados DECIDE.
//!
//! O núcleo puro (`verify`, `sm`, `handshake`) decide; este módulo assina e envia
//! transações, lê confirmações e dirige o laço. Tudo que tem efeito colateral
//! (chaves, rede, relógio) vive AQUI, isolado em submódulos — o núcleo continua
//! puro e testável sem mundo.
//!
//! Construído em peças testáveis, na ordem de dependência:
//! 1. [`signer`] — chave + guard allowlist (esta peça).
//! 2. interface `observe_lock` com `min_confirmations` (próxima).
//! 3. `tx` — lock/redeem/refund.
//! 4. `observe` + `confirm` — descoberta por hashlock (maestro) + profundidade N.
//! 5. `mod` — o laço + a re-verificação no último instante (anti-TOCTOU).

use crate::chain::{ChainError, RpcVerifier};
use crate::exec::observe::CounterpartyObserver;
use crate::exec::signer::Signer;
use crate::exec::tx::TxError;
use crate::sm::{advance, next_action, AbortReason, NextAction, SwapContext, SwapEvent, SwapState};
use crate::verify::Address;
use alloy::providers::{Provider, ProviderBuilder};
use rand::RngCore;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod confirm;
pub mod observe;
pub mod signer;
pub mod tx;

/// Relógio injetável para o executor. Testes podem avançar timelock sem sleep real.
pub trait Clock {
    fn now(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs()
    }
}

#[derive(Debug)]
pub enum ExecutorError {
    MissingHashlock,
    MissingOwnContractId,
    MissingCounterpartyContractId,
    Chain(ChainError),
    Tx(TxError),
    MaxStepsExceeded,
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::MissingHashlock => write!(f, "hashlock ausente no contexto do executor"),
            ExecutorError::MissingOwnContractId => write!(f, "contractId da minha perna ausente"),
            ExecutorError::MissingCounterpartyContractId => {
                write!(f, "contractId da perna oposta ausente")
            }
            ExecutorError::Chain(e) => write!(f, "erro de chain: {e}"),
            ExecutorError::Tx(e) => write!(f, "erro de tx: {e}"),
            ExecutorError::MaxStepsExceeded => write!(f, "executor excedeu o limite de passos"),
        }
    }
}

impl std::error::Error for ExecutorError {}

impl From<ChainError> for ExecutorError {
    fn from(value: ChainError) -> Self {
        ExecutorError::Chain(value)
    }
}

impl From<TxError> for ExecutorError {
    fn from(value: TxError) -> Self {
        ExecutorError::Tx(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    GeneratedSecret,
    LockedMyLeg { contract_id: [u8; 32] },
    RedeemedCounterpartyLeg,
    RefundedMyLeg,
    Waiting,
    Terminal,
    Aborted(AbortReason),
}

/// Executor concreto do MVP EVM/anvil.
///
/// Ele mantém a regra central: observa o mundo, chama `next_action`, re-observa
/// imediatamente antes de lock/redeem, e só transmite se a ação atual ainda for
/// exatamente a ação planejada pela state machine.
pub struct WalletExecutor<C: Clock> {
    pub state: SwapState,
    pub ctx: SwapContext,
    own_signer: Signer,
    counterparty_signer: Signer,
    own_htlc: Address,
    counterparty_htlc: Address,
    own_observer: CounterpartyObserver<RpcVerifier>,
    counterparty_observer: CounterpartyObserver<RpcVerifier>,
    clock: C,
    min_confirmations: u64,
    own_contract_id: Option<[u8; 32]>,
    counterparty_contract_id: Option<[u8; 32]>,
}

pub struct WalletExecutorConfig<C: Clock> {
    pub state: SwapState,
    pub ctx: SwapContext,
    pub own_signer: Signer,
    pub counterparty_signer: Signer,
    pub own_htlc: Address,
    pub counterparty_htlc: Address,
    pub own_observer: CounterpartyObserver<RpcVerifier>,
    pub counterparty_observer: CounterpartyObserver<RpcVerifier>,
    pub clock: C,
    pub min_confirmations: u64,
}

impl<C: Clock> WalletExecutor<C> {
    pub fn new(config: WalletExecutorConfig<C>) -> Self {
        Self {
            state: config.state,
            ctx: config.ctx,
            own_signer: config.own_signer,
            counterparty_signer: config.counterparty_signer,
            own_htlc: config.own_htlc,
            counterparty_htlc: config.counterparty_htlc,
            own_observer: config.own_observer,
            counterparty_observer: config.counterparty_observer,
            clock: config.clock,
            min_confirmations: config.min_confirmations,
            own_contract_id: None,
            counterparty_contract_id: None,
        }
    }

    pub fn own_contract_id(&self) -> Option<[u8; 32]> {
        self.own_contract_id
    }

    pub fn counterparty_contract_id(&self) -> Option<[u8; 32]> {
        self.counterparty_contract_id
    }

    async fn refresh_observations(&mut self) -> Result<(), ExecutorError> {
        self.ctx.now = self.clock.now();
        let hashlock = self.ctx.hashlock.ok_or(ExecutorError::MissingHashlock)?;

        let cp = self
            .counterparty_observer
            .observe(&hashlock, self.min_confirmations)
            .await?;
        self.ctx.counterparty_lock = cp.for_gate();
        self.counterparty_contract_id = self.counterparty_observer.discover_contract_id(&hashlock);

        self.own_observer.poll().await?;
        if let Some(secret) = self.own_observer.revealed_secret(&hashlock) {
            self.ctx.revealed_secret = Some(secret);
        }

        if self.own_contract_id.is_some() {
            self.ctx.my_leg_locked = true;
        }

        Ok(())
    }

    async fn reverified_action(&mut self) -> Result<NextAction, ExecutorError> {
        self.refresh_observations().await?;
        Ok(next_action(&self.state, &self.ctx))
    }

    pub async fn step(&mut self) -> Result<StepOutcome, ExecutorError> {
        self.refresh_observations().await?;
        let planned = next_action(&self.state, &self.ctx);

        match planned {
            NextAction::GenerateSecret => {
                if self.ctx.secret.is_none() {
                    let mut secret = [0u8; 32];
                    rand::rngs::OsRng.fill_bytes(&mut secret);
                    self.ctx.hashlock = Some(maestro::hashlock_from_preimage(&secret));
                    self.ctx.secret = Some(secret);
                }
                self.state = advance(self.state, SwapEvent::SecretGenerated);
                Ok(StepOutcome::GeneratedSecret)
            }
            NextAction::LockMyLeg { recipient, token, amount, hashlock, timelock } => {
                let current = self.reverified_action().await?;
                if !reverified_action_matches(&planned, &current) {
                    return Ok(StepOutcome::Waiting);
                }
                if self.own_contract_id.is_some() {
                    self.ctx.my_leg_locked = true;
                    self.state = advance(self.state, SwapEvent::MyLegConfirmed);
                    return Ok(StepOutcome::Waiting);
                }
                let locked = tx::lock(
                    &self.own_signer,
                    self.own_htlc,
                    recipient,
                    token,
                    amount,
                    hashlock,
                    timelock,
                )
                .await?;
                self.own_contract_id = Some(locked.contract_id);
                self.ctx.my_leg_locked = true;
                self.state = advance(self.state, SwapEvent::MyLegConfirmed);
                Ok(StepOutcome::LockedMyLeg { contract_id: locked.contract_id })
            }
            NextAction::RedeemCounterpartyLeg { secret } => {
                let current = self.reverified_action().await?;
                if !reverified_action_matches(&planned, &current) {
                    return Ok(StepOutcome::Waiting);
                }
                let cid = self
                    .counterparty_contract_id
                    .ok_or(ExecutorError::MissingCounterpartyContractId)?;
                tx::redeem(&self.counterparty_signer, self.counterparty_htlc, cid, secret).await?;
                if self.state == SwapState::WaitingForSecret {
                    self.state = advance(self.state, SwapEvent::SecretObserved);
                }
                self.state = advance(self.state, SwapEvent::RedeemConfirmed);
                Ok(StepOutcome::RedeemedCounterpartyLeg)
            }
            NextAction::Refund => {
                if self.ctx.now < self.ctx.my_timelock {
                    return Ok(StepOutcome::Waiting);
                }
                let cid = self.own_contract_id.ok_or(ExecutorError::MissingOwnContractId)?;
                tx::refund(&self.own_signer, self.own_htlc, cid).await?;
                self.state = advance(SwapState::Refunding, SwapEvent::RefundConfirmed);
                Ok(StepOutcome::RefundedMyLeg)
            }
            NextAction::WaitForCounterpartyLock
            | NextAction::WaitForSecretReveal
            | NextAction::VerifyCounterpartyLeg => Ok(StepOutcome::Waiting),
            NextAction::Done => Ok(StepOutcome::Terminal),
            NextAction::Abort { reason } => {
                self.state = SwapState::Aborted(reason);
                Ok(StepOutcome::Aborted(reason))
            }
        }
    }

    pub async fn run(
        &mut self,
        max_steps: usize,
        poll_interval: Duration,
    ) -> Result<SwapState, ExecutorError> {
        for _ in 0..max_steps {
            match self.step().await? {
                StepOutcome::Terminal | StepOutcome::Aborted(_) => return Ok(self.state),
                _ if is_terminal(&self.state) => return Ok(self.state),
                _ => {
                    if !poll_interval.is_zero() {
                        std::thread::sleep(poll_interval);
                    }
                }
            }
        }
        Err(ExecutorError::MaxStepsExceeded)
    }
}

pub fn rpc_observer(
    rpc_url: &str,
    htlc: Address,
    chain_id: u64,
) -> Result<CounterpartyObserver<RpcVerifier>, ChainError> {
    let verifier = RpcVerifier::new(rpc_url)?;
    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().map_err(|e| ChainError::BadUrl(format!("{e}")))?)
        .erased();
    Ok(CounterpartyObserver::new(verifier, provider, htlc, chain_id))
}

fn is_terminal(state: &SwapState) -> bool {
    matches!(
        state,
        SwapState::Done | SwapState::Refunded | SwapState::CounterpartyRedeemed | SwapState::Aborted(_)
    )
}

fn reverified_action_matches(planned: &NextAction, current: &NextAction) -> bool {
    match (planned, current) {
        (
            NextAction::LockMyLeg {
                recipient: a_recipient,
                token: a_token,
                amount: a_amount,
                hashlock: a_hashlock,
                timelock: a_timelock,
            },
            NextAction::LockMyLeg {
                recipient: b_recipient,
                token: b_token,
                amount: b_amount,
                hashlock: b_hashlock,
                timelock: b_timelock,
            },
        ) => {
            a_recipient == b_recipient
                && a_token == b_token
                && a_amount == b_amount
                && a_hashlock == b_hashlock
                && a_timelock == b_timelock
        }
        (
            NextAction::RedeemCounterpartyLeg { secret: a },
            NextAction::RedeemCounterpartyLeg { secret: b },
        ) => a == b,
        (NextAction::Refund, NextAction::Refund) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{ChainVerifier, LockObservation};
    use crate::sm::SwapState;
    use crate::verify::Role;
    use alloy::network::EthereumWallet;
    use alloy::node_bindings::Anvil;
    use alloy::providers::Provider;
    use alloy::signers::local::PrivateKeySigner;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::sync::{Arc, Mutex};

    const ANVIL_KEY0: &str =
        "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const ANVIL_KEY1: &str =
        "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<u64>>);

    impl FakeClock {
        fn new(now: u64) -> Self {
            Self(Arc::new(Mutex::new(now)))
        }
        fn set(&self, now: u64) {
            *self.0.lock().unwrap() = now;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> u64 {
            *self.0.lock().unwrap()
        }
    }

    fn zero() -> Address {
        [0u8; 20]
    }

    fn ctx(
        role: Role,
        me: Address,
        cp: Address,
        secret: Option<[u8; 32]>,
        hashlock: [u8; 32],
        now: u64,
    ) -> SwapContext {
        SwapContext {
            role,
            my_token: zero(),
            my_amount: if role == Role::Taker { 1_000 } else { 500 },
            my_timelock: now + if role == Role::Taker { 7_200 } else { 3_600 },
            my_recipient: cp,
            cp_token: zero(),
            cp_amount: if role == Role::Taker { 500 } else { 1_000 },
            me,
            min_gap: 1_800,
            hashlock: Some(hashlock),
            secret,
            my_leg_locked: false,
            counterparty_lock: None,
            revealed_secret: None,
            now,
        }
    }

    async fn deploy_htlc(rpc: &str, key: &str) -> (Address, u64) {
        let pk: PrivateKeySigner = format!("0x{key}").parse().unwrap();
        let wallet = EthereumWallet::from(pk);
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(rpc.parse().unwrap());
        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let chain_id = provider.get_chain_id().await.unwrap();
        ((*htlc.address()).into_array(), chain_id)
    }

    async fn executor(
        state: SwapState,
        ctx: SwapContext,
        own_key: &str,
        cp_key: &str,
        own_rpc: &str,
        cp_rpc: &str,
        own_htlc: Address,
        cp_htlc: Address,
        own_chain_id: u64,
        cp_chain_id: u64,
        clock: FakeClock,
    ) -> WalletExecutor<FakeClock> {
        let own_signer = Signer::from_key_str(own_key, own_rpc).await.unwrap();
        let counterparty_signer = Signer::from_key_str(own_key, cp_rpc).await.unwrap();
        let _cp_signer_sanity = Signer::from_key_str(cp_key, cp_rpc).await.unwrap();
        WalletExecutor::new(WalletExecutorConfig {
            state,
            ctx,
            own_signer,
            counterparty_signer,
            own_htlc,
            counterparty_htlc: cp_htlc,
            own_observer: rpc_observer(own_rpc, own_htlc, own_chain_id).unwrap(),
            counterparty_observer: rpc_observer(cp_rpc, cp_htlc, cp_chain_id).unwrap(),
            clock,
            min_confirmations: 1,
        })
    }

    #[test]
    fn anti_toctou_reverify_blocks_lock_when_current_action_changes_to_abort() {
        let planned = NextAction::LockMyLeg {
            recipient: [0xBB; 20],
            token: zero(),
            amount: 1000,
            hashlock: [0xAB; 32],
            timelock: 10_000,
        };
        let current = NextAction::Abort {
            reason: AbortReason::UnsafeCounterparty(crate::verify::UnsafeReason::HashlockMismatch),
        };
        assert!(!reverified_action_matches(&planned, &current));
    }

    #[test]
    fn anti_toctou_reverify_blocks_redeem_and_secret_leak_when_current_action_is_refund() {
        let planned = NextAction::RedeemCounterpartyLeg { secret: [0x5e; 32] };
        let current = NextAction::Refund;
        assert!(!reverified_action_matches(&planned, &current));
    }

    #[tokio::test]
    async fn local_two_party_htlc_swap_e2e_wallet_driven() {
        let anvil_a = Anvil::new().spawn();
        let anvil_b = Anvil::new().spawn();
        let rpc_a = anvil_a.endpoint();
        let rpc_b = anvil_b.endpoint();

        let (htlc_a, chain_a) = deploy_htlc(&rpc_a, ANVIL_KEY0).await;
        let (htlc_b, chain_b) = deploy_htlc(&rpc_b, ANVIL_KEY1).await;

        let taker_a = Signer::from_key_str(ANVIL_KEY0, &rpc_a).await.unwrap();
        let maker_b = Signer::from_key_str(ANVIL_KEY1, &rpc_b).await.unwrap();
        let taker = taker_a.address();
        let maker = maker_b.address();

        let now = SystemClock.now();
        let clock = FakeClock::new(now);
        let secret = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&secret);

        let taker_ctx = ctx(Role::Taker, taker, maker, Some(secret), hashlock, now);
        let maker_ctx = ctx(Role::Maker, maker, taker, None, hashlock, now);

        let mut taker_exec = executor(
            SwapState::SecretGenerated,
            taker_ctx,
            ANVIL_KEY0,
            ANVIL_KEY1,
            &rpc_a,
            &rpc_b,
            htlc_a,
            htlc_b,
            chain_a,
            chain_b,
            clock.clone(),
        )
        .await;
        let mut maker_exec = executor(
            SwapState::Start,
            maker_ctx,
            ANVIL_KEY1,
            ANVIL_KEY0,
            &rpc_b,
            &rpc_a,
            htlc_b,
            htlc_a,
            chain_b,
            chain_a,
            clock.clone(),
        )
        .await;

        assert!(matches!(
            taker_exec.step().await.unwrap(),
            StepOutcome::LockedMyLeg { .. }
        ));
        assert_eq!(taker_exec.state, SwapState::MyLegLocked);

        assert!(matches!(
            maker_exec.step().await.unwrap(),
            StepOutcome::LockedMyLeg { .. }
        ));
        assert_eq!(maker_exec.state, SwapState::WaitingForSecret);

        assert_eq!(
            taker_exec.step().await.unwrap(),
            StepOutcome::RedeemedCounterpartyLeg
        );
        assert_eq!(taker_exec.state, SwapState::CounterpartyRedeemed);

        assert_eq!(
            maker_exec.step().await.unwrap(),
            StepOutcome::RedeemedCounterpartyLeg
        );
        assert_eq!(maker_exec.state, SwapState::CounterpartyRedeemed);

        let verify_a = RpcVerifier::new(&rpc_a).unwrap();
        let verify_b = RpcVerifier::new(&rpc_b).unwrap();
        assert_eq!(
            verify_a
                .observe_lock(htlc_a, taker_exec.own_contract_id().unwrap(), 1)
                .await
                .unwrap(),
            LockObservation::Absent
        );
        assert_eq!(
            verify_b
                .observe_lock(htlc_b, maker_exec.own_contract_id().unwrap(), 1)
                .await
                .unwrap(),
            LockObservation::Absent
        );
        assert_eq!(maker_exec.ctx.revealed_secret, Some(secret));
    }

    #[tokio::test]
    async fn fake_clock_expiry_drives_refund_without_real_sleep() {
        let anvil_a = Anvil::new().spawn();
        let anvil_b = Anvil::new().spawn();
        let rpc_a = anvil_a.endpoint();
        let rpc_b = anvil_b.endpoint();

        let (htlc_a, chain_a) = deploy_htlc(&rpc_a, ANVIL_KEY0).await;
        let (htlc_b, chain_b) = deploy_htlc(&rpc_b, ANVIL_KEY1).await;

        let taker_a = Signer::from_key_str(ANVIL_KEY0, &rpc_a).await.unwrap();
        let maker_b = Signer::from_key_str(ANVIL_KEY1, &rpc_b).await.unwrap();
        let now = SystemClock.now();
        let clock = FakeClock::new(now);
        let secret = [0x24u8; 32];
        let hashlock = hashlock_from_preimage(&secret);
        let mut taker_ctx = ctx(
            Role::Taker,
            taker_a.address(),
            maker_b.address(),
            Some(secret),
            hashlock,
            now,
        );
        taker_ctx.my_timelock = now + 30;

        let mut taker_exec = executor(
            SwapState::SecretGenerated,
            taker_ctx,
            ANVIL_KEY0,
            ANVIL_KEY1,
            &rpc_a,
            &rpc_b,
            htlc_a,
            htlc_b,
            chain_a,
            chain_b,
            clock.clone(),
        )
        .await;

        assert!(matches!(
            taker_exec.step().await.unwrap(),
            StepOutcome::LockedMyLeg { .. }
        ));
        assert_eq!(taker_exec.step().await.unwrap(), StepOutcome::Waiting);

        clock.set(now + 31);
        let _: serde_json::Value = taker_exec
            .own_signer
            .provider()
            .raw_request("evm_setNextBlockTimestamp".into(), (now + 31,))
            .await
            .unwrap();
        let _: serde_json::Value = taker_exec
            .own_signer
            .provider()
            .raw_request("evm_mine".into(), ())
            .await
            .unwrap();

        assert_eq!(taker_exec.step().await.unwrap(), StepOutcome::RefundedMyLeg);
        assert_eq!(taker_exec.state, SwapState::Refunded);
    }
}

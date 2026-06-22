//! Máquina de estados do protocolo interativo de swap (pura).
//!
//! Ela NÃO executa ações — DECIDE a próxima ação ([`NextAction`]) dado o papel,
//! o estado e as observações das chains. Uma camada posterior executa. Tudo do
//! mundo entra por parâmetro (observações, tempo atual). Sem rede, sem chaves.
//!
//! PAPÉIS (fixos):
//! - `Taker` = detentor do segredo; trava PRIMEIRO, timelock LONGO.
//! - `Maker` = respondente; trava DEPOIS (mesmo hashlock que leu da perna do
//!   taker), timelock CURTO.
//!
//! PRINCÍPIO DE SEGURANÇA INVIOLÁVEL: a máquina NUNCA emite `LockMyLeg` nem
//! `RedeemCounterpartyLeg` se [`verify_counterparty_leg`] não devolver `Safe`.
//! - O Maker só trava DEPOIS de verificar a perna do taker como Safe (mesmo
//!   hashlock, gap seguro). Se Unsafe e ele ainda não travou → `Abort`.
//! - O Taker só revela o segredo (resgatando a perna do maker) DEPOIS de
//!   verificar essa perna como Safe. Se Unsafe e ele já travou → `Refund`
//!   (após expiração) — o segredo NUNCA vaza contra uma perna insegura.
//!
//! DECISÃO DE MODELAGEM: não há estado "Verificado" persistido. A verificação é
//! re-derivada das observações a CADA `next_action`. Um flag "já verifiquei"
//! gravado no estado poderia ser burlado se as condições da outra chain mudassem
//! (reorg, substituição). Verificar sempre, a partir do observado, é mais seguro.

use crate::verify::{verify_counterparty_leg, Address, LegExpectation, ObservedLock, Role, UnsafeReason, VerifyOutcome};
use serde::{Deserialize, Serialize};

/// Os estados pelos quais um swap passa, do ponto de vista DESTA parte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapState {
    /// início.
    Start,
    /// (taker) segredo + hashlock prontos; prestes a travar a própria perna.
    SecretGenerated,
    /// (taker) própria perna travada; vai observar+verificar a perna do maker e resgatar.
    MyLegLocked,
    /// (maker) própria perna travada; aguardando o taker revelar o segredo.
    WaitingForSecret,
    /// (maker) segredo aprendido; prestes a resgatar a perna do taker.
    SecretLearned,
    /// (ambos) resgatei a perna oposta (taker revela; maker reivindica) — sucesso.
    CounterpartyRedeemed,
    /// concluído.
    Done,
    /// decidi reembolsar a minha perna (após expiração).
    Refunding,
    /// reembolsado.
    Refunded,
    /// abortado, com a razão.
    Aborted(AbortReason),
}

/// Razão de um aborto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbortReason {
    /// a perna oposta foi considerada insegura pelo verificador.
    UnsafeCounterparty(UnsafeReason),
    /// faltava o segredo quando ele era necessário.
    MissingSecret,
    /// faltava o hashlock quando ele era necessário.
    MissingHashlock,
    /// uma verificação falhou (caminho de evento, sem a razão detalhada).
    VerificationFailed,
    /// o estado não faz sentido para este papel.
    InvalidState,
    /// uma transição inválida foi solicitada.
    InvalidTransition,
}

/// O que a máquina MANDA fazer a seguir. A camada de execução realiza.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NextAction {
    /// (taker) gere um segredo e seu hashlock H = SHA256(segredo).
    GenerateSecret,
    /// trave a minha perna no HTLC desta chain.
    LockMyLeg {
        recipient: Address, // quem pode resgatar a MINHA perna = a contraparte
        token: Address,
        amount: u128,
        hashlock: [u8; 32],
        timelock: u64,
    },
    /// reservada: a verificação é feita DENTRO de `next_action` (gate); exposta
    /// para executores que queiram dirigir a verificação explicitamente.
    VerifyCounterpartyLeg,
    /// resgate a perna oposta revelando/usando o segredo.
    RedeemCounterpartyLeg { secret: [u8; 32] },
    /// a trava oposta ainda não apareceu — continue observando.
    WaitForCounterpartyLock,
    /// minha perna travada; aguarde a contraparte revelar o segredo.
    WaitForSecretReveal,
    /// reembolse a minha perna (após expiração do timelock).
    Refund,
    /// nada mais a fazer.
    Done,
    /// pare com segurança; não prossiga.
    Abort { reason: AbortReason },
}

/// Eventos do mundo que fazem o estado avançar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapEvent {
    /// (taker) segredo + hashlock gerados.
    SecretGenerated,
    /// a minha trava foi confirmada on-chain.
    MyLegConfirmed,
    /// a trava da contraparte foi observada.
    CounterpartyLockObserved,
    /// a verificação da perna oposta falhou (insegura).
    VerificationFailed,
    /// o segredo foi revelado on-chain (por um resgate).
    SecretObserved,
    /// o meu timelock expirou.
    TimelockExpired,
    /// o meu resgate da perna oposta foi confirmado.
    RedeemConfirmed,
    /// o meu reembolso foi confirmado.
    RefundConfirmed,
}

/// Tudo que a máquina precisa saber do mundo (observações + parâmetros).
#[derive(Clone, Debug)]
pub struct SwapContext {
    pub role: Role,

    // --- a MINHA perna (o que eu vendo/travo) ---
    pub my_token: Address,
    pub my_amount: u128,
    pub my_timelock: u64,
    /// quem pode resgatar a MINHA perna = a contraparte.
    pub my_recipient: Address,

    // --- a perna OPOSTA que eu espero/observo (o que eu compro) ---
    pub cp_token: Address,
    pub cp_amount: u128,
    /// o MEU endereço — quem deve poder resgatar a perna oposta.
    pub me: Address,
    pub min_gap: u64,

    /// hashlock: taker = H(segredo) após gerar; maker = H acordado no handshake.
    pub hashlock: Option<[u8; 32]>,
    /// o segredo (só o taker tem desde o início).
    pub secret: Option<[u8; 32]>,

    // --- observações ---
    /// a minha trava já foi confirmada on-chain?
    pub my_leg_locked: bool,
    /// a perna oposta como observada (None = ainda não vista).
    pub counterparty_lock: Option<ObservedLock>,
    /// segredo revelado por um resgate (o maker aprende por aqui).
    pub revealed_secret: Option<[u8; 32]>,
    /// tempo atual (parâmetro — a máquina não lê relógio).
    pub now: u64,
}

impl SwapContext {
    /// Expectativa para verificar a perna OPOSTA, conforme o meu papel.
    fn expectation(&self) -> LegExpectation {
        LegExpectation {
            expected_hashlock: self.hashlock.unwrap_or([0u8; 32]),
            expected_token: self.cp_token,
            expected_amount: self.cp_amount,
            expected_recipient: self.me,
            my_timelock: self.my_timelock,
            min_gap: self.min_gap,
            role: self.role,
        }
    }

    fn lock_my_leg(&self, hashlock: [u8; 32]) -> NextAction {
        NextAction::LockMyLeg {
            recipient: self.my_recipient,
            token: self.my_token,
            amount: self.my_amount,
            hashlock,
            timelock: self.my_timelock,
        }
    }
}

/// Decide a próxima ação, dado o estado e o contexto (puro).
///
/// Os pontos de gate (`LockMyLeg` do maker, `RedeemCounterpartyLeg` do taker)
/// chamam [`verify_counterparty_leg`] internamente e SÓ liberam a ação se o
/// resultado for `Safe`. Caso contrário: `Abort` (se nada travado) ou `Refund`
/// (se já travei).
pub fn next_action(state: &SwapState, ctx: &SwapContext) -> NextAction {
    // estados terminais / independentes de papel
    match state {
        SwapState::Aborted(r) => return NextAction::Abort { reason: *r },
        SwapState::Done | SwapState::Refunded => return NextAction::Done,
        SwapState::Refunding => return NextAction::Refund,
        SwapState::CounterpartyRedeemed => return NextAction::Done,
        _ => {}
    }
    match ctx.role {
        Role::Taker => taker_next(state, ctx),
        Role::Maker => maker_next(state, ctx),
    }
}

fn taker_next(state: &SwapState, ctx: &SwapContext) -> NextAction {
    match state {
        SwapState::Start => NextAction::GenerateSecret,
        SwapState::SecretGenerated => match ctx.hashlock {
            Some(h) => ctx.lock_my_leg(h),
            None => NextAction::Abort { reason: AbortReason::MissingHashlock },
        },
        SwapState::MyLegLocked => match ctx.counterparty_lock {
            // a perna do maker ainda não apareceu
            None => {
                if ctx.now >= ctx.my_timelock {
                    NextAction::Refund // expirei esperando a contraparte
                } else {
                    NextAction::WaitForCounterpartyLock
                }
            }
            // observei a perna do maker: VERIFICO antes de revelar o segredo
            Some(obs) => match verify_counterparty_leg(&ctx.expectation(), &obs) {
                VerifyOutcome::Safe => match ctx.secret {
                    Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
                    None => NextAction::Abort { reason: AbortReason::MissingSecret },
                },
                // INSEGURA + já travei → reembolso. O SEGREDO NUNCA VAZA.
                VerifyOutcome::Unsafe(_) => NextAction::Refund,
            },
        },
        // estados de maker não fazem sentido para o taker
        _ => NextAction::Abort { reason: AbortReason::InvalidState },
    }
}

fn maker_next(state: &SwapState, ctx: &SwapContext) -> NextAction {
    match state {
        SwapState::Start => match ctx.counterparty_lock {
            // a perna do taker ainda não apareceu
            None => NextAction::WaitForCounterpartyLock,
            // observei a perna do taker: VERIFICO antes de travar a minha
            Some(obs) => match verify_counterparty_leg(&ctx.expectation(), &obs) {
                VerifyOutcome::Safe => match ctx.hashlock {
                    Some(h) => ctx.lock_my_leg(h),
                    None => NextAction::Abort { reason: AbortReason::MissingHashlock },
                },
                // INSEGURA + ainda NÃO travei → aborto. NUNCA travo contra perna insegura.
                VerifyOutcome::Unsafe(r) => NextAction::Abort { reason: AbortReason::UnsafeCounterparty(r) },
            },
        },
        SwapState::WaitingForSecret => match ctx.revealed_secret {
            Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
            None => {
                if ctx.now >= ctx.my_timelock {
                    NextAction::Refund // o taker nunca revelou; minha perna expirou
                } else {
                    NextAction::WaitForSecretReveal
                }
            }
        },
        SwapState::SecretLearned => match ctx.revealed_secret {
            Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
            None => NextAction::Abort { reason: AbortReason::MissingSecret },
        },
        // estados de taker não fazem sentido para o maker
        _ => NextAction::Abort { reason: AbortReason::InvalidState },
    }
}

/// Transição de estado a partir de um evento do mundo. Transições inválidas →
/// `Aborted(InvalidTransition)`, NUNCA panic.
pub fn advance(state: SwapState, event: SwapEvent) -> SwapState {
    use SwapEvent as E;
    use SwapState::*;

    // terminais absorvem qualquer evento.
    match state {
        Done | Refunded => return state,
        Aborted(r) => return Aborted(r),
        _ => {}
    }

    match (state, event) {
        // taker
        (Start, E::SecretGenerated) => SecretGenerated,
        (SecretGenerated, E::MyLegConfirmed) => MyLegLocked,
        (SecretGenerated, E::TimelockExpired) => Refunding,
        (MyLegLocked, E::CounterpartyLockObserved) => MyLegLocked, // obs disponível; next_action re-decide
        (MyLegLocked, E::RedeemConfirmed) => CounterpartyRedeemed,
        (MyLegLocked, E::VerificationFailed) => Refunding,
        (MyLegLocked, E::TimelockExpired) => Refunding,

        // maker
        (Start, E::CounterpartyLockObserved) => Start, // obs disponível; next_action re-decide
        (Start, E::MyLegConfirmed) => WaitingForSecret,
        (Start, E::VerificationFailed) => Aborted(AbortReason::VerificationFailed),
        (WaitingForSecret, E::SecretObserved) => SecretLearned,
        (WaitingForSecret, E::TimelockExpired) => Refunding,
        (SecretLearned, E::RedeemConfirmed) => CounterpartyRedeemed,

        // ambos
        (CounterpartyRedeemed, _) => Done,
        (Refunding, E::RefundConfirmed) => Refunded,

        // qualquer outra combinação é inválida — erro explícito, sem panic.
        _ => Aborted(AbortReason::InvalidTransition),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> [u8; 32] {
        [b; 32]
    }
    fn a(b: u8) -> [u8; 20] {
        [b; 20]
    }

    const SECRET: [u8; 32] = [0x5e; 32];
    const HASH: u8 = 0xAB; // "H" acordado (faz de conta SHA256(SECRET))
    const TAKER_ADDR: u8 = 0x7A;
    const MAKER_ADDR: u8 = 0x3A;
    const TOK_A: u8 = 0x11; // o que o taker vende (= maker compra)
    const TOK_B: u8 = 0x22; // o que o maker vende (= taker compra)

    // ---------------- TAKER ----------------

    fn taker_ctx() -> SwapContext {
        SwapContext {
            role: Role::Taker,
            my_token: a(TOK_A),
            my_amount: 1000,
            my_timelock: 2000, // LONGO
            my_recipient: a(MAKER_ADDR), // minha perna paga o maker
            cp_token: a(TOK_B),
            cp_amount: 500,
            me: a(TAKER_ADDR), // a perna do maker me paga
            min_gap: 100,
            hashlock: Some(h(HASH)),
            secret: Some(SECRET),
            my_leg_locked: false,
            counterparty_lock: None,
            revealed_secret: None,
            now: 1000,
        }
    }

    // perna do MAKER, observada pelo taker (segura: timelock CURTO, 1800+100<=2000)
    fn maker_leg_safe() -> ObservedLock {
        ObservedLock {
            hashlock: h(HASH),
            token: a(TOK_B),
            amount: 500,
            recipient: a(TAKER_ADDR), // me paga
            timelock: 1800,
            sender: a(MAKER_ADDR),
            exists: true,
        }
    }

    #[test]
    fn taker_happy_path_sequence() {
        let mut ctx = taker_ctx();
        let mut actions = Vec::new();

        // Start → gera segredo
        let mut st = SwapState::Start;
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::SecretGenerated);

        // SecretGenerated → trava a própria perna
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::MyLegConfirmed);

        // MyLegLocked, sem observar o maker → espera
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::CounterpartyLockObserved);

        // MyLegLocked, observou o maker (Safe) → resgata revelando o segredo
        ctx.counterparty_lock = Some(maker_leg_safe());
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::RedeemConfirmed);

        // CounterpartyRedeemed → Done
        actions.push(next_action(&st, &ctx));

        assert_eq!(
            actions,
            vec![
                NextAction::GenerateSecret,
                ctx.lock_my_leg(h(HASH)),
                NextAction::WaitForCounterpartyLock,
                NextAction::RedeemCounterpartyLeg { secret: SECRET },
                NextAction::Done,
            ]
        );
        assert_eq!(st, SwapState::CounterpartyRedeemed);
    }

    // TESTE CRÍTICO 1: o taker NÃO revela o segredo contra uma perna insegura.
    #[test]
    fn taker_never_reveals_secret_against_unsafe_leg() {
        let mut ctx = taker_ctx();
        // a perna do maker tem hashlock ERRADO
        let mut bad = maker_leg_safe();
        bad.hashlock = h(0x01);
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::MyLegLocked, &ctx);
        // a máquina manda REEMBOLSAR, jamais resgatar
        assert_eq!(action, NextAction::Refund);
        assert!(
            !matches!(action, NextAction::RedeemCounterpartyLeg { .. }),
            "o segredo NUNCA pode vazar contra uma perna insegura"
        );
    }

    // ---------------- MAKER ----------------

    fn maker_ctx() -> SwapContext {
        SwapContext {
            role: Role::Maker,
            my_token: a(TOK_B),
            my_amount: 500,
            my_timelock: 1000, // CURTO
            my_recipient: a(TAKER_ADDR), // minha perna paga o taker
            cp_token: a(TOK_A),
            cp_amount: 1000,
            me: a(MAKER_ADDR), // a perna do taker me paga
            min_gap: 100,
            hashlock: Some(h(HASH)), // H acordado no handshake
            secret: None,
            my_leg_locked: false,
            counterparty_lock: None,
            revealed_secret: None,
            now: 500,
        }
    }

    // perna do TAKER, observada pelo maker (segura: timelock LONGO >= 1000+100)
    fn taker_leg_safe() -> ObservedLock {
        ObservedLock {
            hashlock: h(HASH),
            token: a(TOK_A),
            amount: 1000,
            recipient: a(MAKER_ADDR), // me paga
            timelock: 1200,
            sender: a(TAKER_ADDR),
            exists: true,
        }
    }

    #[test]
    fn maker_happy_path_sequence() {
        let mut ctx = maker_ctx();
        let mut actions = Vec::new();

        // Start, sem observar o taker → espera
        let mut st = SwapState::Start;
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::CounterpartyLockObserved);

        // Start, observou o taker (Safe) → trava a própria perna (mesmo hashlock)
        ctx.counterparty_lock = Some(taker_leg_safe());
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::MyLegConfirmed);

        // WaitingForSecret, sem segredo → espera o segredo
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::SecretObserved);

        // SecretLearned → resgata a perna do taker com o segredo aprendido
        ctx.revealed_secret = Some(SECRET);
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::RedeemConfirmed);

        // CounterpartyRedeemed → Done
        actions.push(next_action(&st, &ctx));

        assert_eq!(
            actions,
            vec![
                NextAction::WaitForCounterpartyLock,
                ctx.lock_my_leg(h(HASH)),
                NextAction::WaitForSecretReveal,
                NextAction::RedeemCounterpartyLeg { secret: SECRET },
                NextAction::Done,
            ]
        );
        assert_eq!(st, SwapState::CounterpartyRedeemed);
    }

    // TESTE CRÍTICO 2: o maker NÃO trava contra uma perna com gap inseguro.
    #[test]
    fn maker_never_locks_against_unsafe_gap() {
        let mut ctx = maker_ctx();
        // perna do taker com timelock só um pouco maior (gap < min_gap)
        let mut bad = taker_leg_safe();
        bad.timelock = 1050; // 1050 < my(1000) + gap(100)
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::Start, &ctx);
        assert_eq!(
            action,
            NextAction::Abort { reason: AbortReason::UnsafeCounterparty(UnsafeReason::TimelockGapTooSmall) }
        );
        assert!(
            !matches!(action, NextAction::LockMyLeg { .. }),
            "o maker NUNCA trava contra uma perna insegura"
        );
    }

    // o maker também não trava se o hashlock do taker difere do H acordado.
    #[test]
    fn maker_never_locks_against_hashlock_mismatch() {
        let mut ctx = maker_ctx();
        let mut bad = taker_leg_safe();
        bad.hashlock = h(0x99); // != H acordado
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::Start, &ctx);
        assert_eq!(
            action,
            NextAction::Abort { reason: AbortReason::UnsafeCounterparty(UnsafeReason::HashlockMismatch) }
        );
        assert!(!matches!(action, NextAction::LockMyLeg { .. }));
    }

    // ---------------- caminhos de reembolso ----------------

    #[test]
    fn taker_refunds_when_counterparty_never_locks() {
        let mut ctx = taker_ctx();
        ctx.counterparty_lock = None;
        ctx.now = ctx.my_timelock + 1; // expirou esperando

        let st = SwapState::MyLegLocked;
        assert_eq!(next_action(&st, &ctx), NextAction::Refund);

        let st = advance(st, SwapEvent::TimelockExpired);
        assert_eq!(st, SwapState::Refunding);
        assert_eq!(next_action(&st, &ctx), NextAction::Refund);

        let st = advance(st, SwapEvent::RefundConfirmed);
        assert_eq!(st, SwapState::Refunded);
        assert_eq!(next_action(&st, &ctx), NextAction::Done);
    }

    #[test]
    fn maker_refunds_when_secret_never_revealed() {
        let mut ctx = maker_ctx();
        ctx.revealed_secret = None;
        ctx.now = ctx.my_timelock + 1; // expirou esperando o segredo

        let st = SwapState::WaitingForSecret;
        assert_eq!(next_action(&st, &ctx), NextAction::Refund);

        let st = advance(st, SwapEvent::TimelockExpired);
        assert_eq!(st, SwapState::Refunding);

        let st = advance(st, SwapEvent::RefundConfirmed);
        assert_eq!(st, SwapState::Refunded);
    }

    // ---------------- transição inválida ----------------

    #[test]
    fn invalid_transition_goes_to_aborted_no_panic() {
        // SecretObserved não faz sentido em SecretGenerated
        let st = advance(SwapState::SecretGenerated, SwapEvent::SecretObserved);
        assert_eq!(st, SwapState::Aborted(AbortReason::InvalidTransition));
        // e o estado abortado é absorvente
        assert_eq!(
            advance(st, SwapEvent::MyLegConfirmed),
            SwapState::Aborted(AbortReason::InvalidTransition)
        );
    }

    #[test]
    fn terminal_states_absorb_events() {
        assert_eq!(advance(SwapState::Done, SwapEvent::TimelockExpired), SwapState::Done);
        assert_eq!(advance(SwapState::Refunded, SwapEvent::MyLegConfirmed), SwapState::Refunded);
    }

    // papel errado para o estado → InvalidState (sem panic)
    #[test]
    fn wrong_role_for_state_is_invalid_state() {
        let ctx = taker_ctx(); // taker
        // WaitingForSecret é estado de maker
        assert_eq!(
            next_action(&SwapState::WaitingForSecret, &ctx),
            NextAction::Abort { reason: AbortReason::InvalidState }
        );
    }
}

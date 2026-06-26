//! Pure state machine for the interactive swap protocol.
//!
//! It does not execute actions. It decides the next action ([`NextAction`])
//! from the role, state, and chain observations. A later layer executes the
//! action. All external data is supplied as parameters: observations and
//! current time. No network access and no keys.
//!
//! Fixed roles:
//! - `Taker`: owns the secret, locks first, uses the long timelock.
//! - `Maker`: responds second, locks with the same hashlock observed on the
//!   taker leg, uses the short timelock.
//!
//! Inviolable safety principle: the machine never emits `LockMyLeg` or
//! `RedeemCounterpartyLeg` unless [`verify_counterparty_leg`] returns `Safe`.
//! - The maker locks only after verifying the taker leg as Safe: same hashlock
//!   and safe timelock gap. If Unsafe before locking, return `Abort`.
//! - The taker reveals the secret by redeeming the maker leg only after that
//!   leg verifies as Safe. If Unsafe after locking, return `Refund` after
//!   expiry. The secret must never leak against an unsafe leg.
//!
//! Modeling decision: there is no persisted "verified" state. Verification is
//! re-derived from observations on every `next_action`. A stored "already
//! verified" flag could become stale if the other chain changes due to a reorg
//! or replacement. Re-verifying from observations is safer.

use crate::verify::{
    verify_counterparty_leg, Address, LegExpectation, ObservedLock, Role, UnsafeReason,
    VerifyOutcome,
};
use serde::{Deserialize, Serialize};

/// States a swap can pass through from this party's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapState {
    /// Initial state.
    Start,
    /// Taker has generated the secret and hashlock and is ready to lock.
    SecretGenerated,
    /// Own leg is locked; observe and verify the maker leg before redeeming.
    MyLegLocked,
    /// Maker has locked and is waiting for the taker to reveal the secret.
    WaitingForSecret,
    /// Maker learned the secret and is ready to redeem the taker leg.
    SecretLearned,
    /// This party redeemed the opposite leg successfully.
    CounterpartyRedeemed,
    /// Completed.
    Done,
    /// Decided to refund this party's own leg after expiry.
    Refunding,
    /// Refunded.
    Refunded,
    /// Aborted with the reason.
    Aborted(AbortReason),
}

/// Reason for aborting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbortReason {
    /// The opposite leg was considered unsafe by the verifier.
    UnsafeCounterparty(UnsafeReason),
    /// The secret was missing when it was required.
    MissingSecret,
    /// The hashlock was missing when it was required.
    MissingHashlock,
    /// Verification failed on the event path without a detailed reason.
    VerificationFailed,
    /// The current state is invalid for this role.
    InvalidState,
    /// An invalid state transition was requested.
    InvalidTransition,
}

/// The next action requested by the state machine. The execution layer performs it.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NextAction {
    /// Taker should generate a secret and H = SHA256(secret).
    GenerateSecret,
    /// Lock this party's leg in this chain's HTLC.
    LockMyLeg {
        recipient: Address, // who can redeem this party's leg: the counterparty
        token: Address,
        amount: u128,
        hashlock: [u8; 32],
        timelock: u64,
    },
    /// Reserved: verification is gated inside `next_action`; exposed for
    /// executors that want to drive verification explicitly.
    VerifyCounterpartyLeg,
    /// Redeem the opposite leg by revealing or using the secret.
    RedeemCounterpartyLeg { secret: [u8; 32] },
    /// The opposite lock has not appeared yet; keep observing.
    WaitForCounterpartyLock,
    /// Own leg is locked; wait for the counterparty to reveal the secret.
    WaitForSecretReveal,
    /// Refund this party's own leg after timelock expiry.
    Refund,
    /// Nothing else to do.
    Done,
    /// Stop safely and do not proceed.
    Abort { reason: AbortReason },
}

impl std::fmt::Debug for NextAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NextAction::GenerateSecret => write!(f, "GenerateSecret"),
            NextAction::LockMyLeg {
                recipient,
                token,
                amount,
                hashlock,
                timelock,
            } => f
                .debug_struct("LockMyLeg")
                .field("recipient", recipient)
                .field("token", token)
                .field("amount", amount)
                .field("hashlock", hashlock)
                .field("timelock", timelock)
                .finish(),
            NextAction::VerifyCounterpartyLeg => write!(f, "VerifyCounterpartyLeg"),
            NextAction::RedeemCounterpartyLeg { .. } => f
                .debug_struct("RedeemCounterpartyLeg")
                .field("secret", &"<redacted>")
                .finish(),
            NextAction::WaitForCounterpartyLock => write!(f, "WaitForCounterpartyLock"),
            NextAction::WaitForSecretReveal => write!(f, "WaitForSecretReveal"),
            NextAction::Refund => write!(f, "Refund"),
            NextAction::Done => write!(f, "Done"),
            NextAction::Abort { reason } => {
                f.debug_struct("Abort").field("reason", reason).finish()
            }
        }
    }
}

/// Events from the outside world that advance state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapEvent {
    /// Taker generated the secret and hashlock.
    SecretGenerated,
    /// This party's lock was confirmed on-chain.
    MyLegConfirmed,
    /// The counterparty lock was observed.
    CounterpartyLockObserved,
    /// Verification of the opposite leg failed.
    VerificationFailed,
    /// The secret was revealed on-chain by a redeem.
    SecretObserved,
    /// This party's timelock expired.
    TimelockExpired,
    /// This party's redeem of the opposite leg was confirmed.
    RedeemConfirmed,
    /// This party's refund was confirmed.
    RefundConfirmed,
}

/// Everything the state machine needs to know from the world.
#[derive(Clone)]
pub struct SwapContext {
    pub role: Role,

    // --- my leg: what I sell/lock ---
    pub my_token: Address,
    pub my_amount: u128,
    pub my_timelock: u64,
    /// Who can redeem my leg: the counterparty.
    pub my_recipient: Address,

    // --- opposite leg: what I expect/observe ---
    pub cp_token: Address,
    pub cp_amount: u128,
    /// My address: who must be able to redeem the opposite leg.
    pub me: Address,
    pub min_gap: u64,

    /// Hashlock: taker uses H(secret) after generation; maker uses the agreed H.
    pub hashlock: Option<[u8; 32]>,
    /// Secret preimage. Sensitive: this type intentionally does not implement Debug.
    pub secret: Option<[u8; 32]>,

    // --- observations ---
    /// Whether my lock has been confirmed on-chain.
    pub my_leg_locked: bool,
    /// Observed opposite leg, or None when it has not been seen yet.
    pub counterparty_lock: Option<ObservedLock>,
    /// Secret revealed by an on-chain redeem.
    pub revealed_secret: Option<[u8; 32]>,
    /// Current time supplied by the caller. The state machine does not read a clock.
    pub now: u64,
}

impl SwapContext {
    /// Expectations for verifying the opposite leg according to this role.
    fn expectation(&self) -> LegExpectation {
        LegExpectation {
            expected_hashlock: self.hashlock.unwrap_or([0u8; 32]),
            expected_token: self.cp_token,
            expected_amount: self.cp_amount,
            expected_recipient: self.me,
            my_timelock: self.my_timelock,
            min_gap: self.min_gap,
            now: self.now, // absolute clock gate: opposite leg must not expire now
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

/// Decide the next action from state and context without side effects.
///
/// Gate points (`LockMyLeg` for the maker, `RedeemCounterpartyLeg` for the
/// taker) call [`verify_counterparty_leg`] internally and only release the
/// action when the result is `Safe`. Otherwise they return `Abort` when
/// nothing is locked or `Refund` when this party has already locked.
pub fn next_action(state: &SwapState, ctx: &SwapContext) -> NextAction {
    // Terminal states and role-independent states.
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
            None => NextAction::Abort {
                reason: AbortReason::MissingHashlock,
            },
        },
        SwapState::MyLegLocked => match ctx.counterparty_lock {
            // The maker leg has not appeared yet.
            None => {
                if ctx.now >= ctx.my_timelock {
                    NextAction::Refund // expired while waiting for the counterparty
                } else {
                    NextAction::WaitForCounterpartyLock
                }
            }
            // Observed maker leg: verify before revealing the secret.
            Some(obs) => match verify_counterparty_leg(&ctx.expectation(), &obs) {
                VerifyOutcome::Safe => match ctx.secret {
                    Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
                    None => NextAction::Abort {
                        reason: AbortReason::MissingSecret,
                    },
                },
                // Unsafe after locking: refund. The secret must never leak.
                VerifyOutcome::Unsafe(_) => NextAction::Refund,
            },
        },
        // Maker states are invalid for the taker.
        _ => NextAction::Abort {
            reason: AbortReason::InvalidState,
        },
    }
}

fn maker_next(state: &SwapState, ctx: &SwapContext) -> NextAction {
    match state {
        SwapState::Start => match ctx.counterparty_lock {
            // The taker leg has not appeared yet.
            None => NextAction::WaitForCounterpartyLock,
            // Observed taker leg: verify before locking this party's leg.
            Some(obs) => match verify_counterparty_leg(&ctx.expectation(), &obs) {
                VerifyOutcome::Safe => match ctx.hashlock {
                    Some(h) => ctx.lock_my_leg(h),
                    None => NextAction::Abort {
                        reason: AbortReason::MissingHashlock,
                    },
                },
                // Unsafe before locking: abort. Never lock against an unsafe leg.
                VerifyOutcome::Unsafe(r) => NextAction::Abort {
                    reason: AbortReason::UnsafeCounterparty(r),
                },
            },
        },
        SwapState::WaitingForSecret => match ctx.revealed_secret {
            Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
            None => {
                if ctx.now >= ctx.my_timelock {
                    NextAction::Refund // taker never revealed; this party's leg expired
                } else {
                    NextAction::WaitForSecretReveal
                }
            }
        },
        SwapState::SecretLearned => match ctx.revealed_secret {
            Some(s) => NextAction::RedeemCounterpartyLeg { secret: s },
            None => NextAction::Abort {
                reason: AbortReason::MissingSecret,
            },
        },
        // Taker states are invalid for the maker.
        _ => NextAction::Abort {
            reason: AbortReason::InvalidState,
        },
    }
}

/// Advance state from an external event. Invalid transitions return
/// `Aborted(InvalidTransition)` and never panic.
pub fn advance(state: SwapState, event: SwapEvent) -> SwapState {
    use SwapEvent as E;
    use SwapState::*;

    // Terminal states absorb every event.
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
        (MyLegLocked, E::CounterpartyLockObserved) => MyLegLocked, // observation available; next_action re-decides
        (MyLegLocked, E::RedeemConfirmed) => CounterpartyRedeemed,
        (MyLegLocked, E::VerificationFailed) => Refunding,
        (MyLegLocked, E::TimelockExpired) => Refunding,

        // maker
        (Start, E::CounterpartyLockObserved) => Start, // observation available; next_action re-decides
        (Start, E::MyLegConfirmed) => WaitingForSecret,
        (Start, E::VerificationFailed) => Aborted(AbortReason::VerificationFailed),
        (WaitingForSecret, E::SecretObserved) => SecretLearned,
        (WaitingForSecret, E::TimelockExpired) => Refunding,
        (SecretLearned, E::RedeemConfirmed) => CounterpartyRedeemed,

        // both
        (CounterpartyRedeemed, _) => Done,
        (Refunding, E::RefundConfirmed) => Refunded,

        // Every other combination is invalid: explicit error, no panic.
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
    const HASH: u8 = 0xAB; // agreed "H" in place of SHA256(SECRET)
    const TAKER_ADDR: u8 = 0x7A;
    const MAKER_ADDR: u8 = 0x3A;
    const TOK_A: u8 = 0x11; // what the taker sells and the maker buys
    const TOK_B: u8 = 0x22; // what the maker sells and the taker buys

    // ---------------- TAKER ----------------

    fn taker_ctx() -> SwapContext {
        SwapContext {
            role: Role::Taker,
            my_token: a(TOK_A),
            my_amount: 1000,
            my_timelock: 2000,           // long
            my_recipient: a(MAKER_ADDR), // this party's leg pays the maker
            cp_token: a(TOK_B),
            cp_amount: 500,
            me: a(TAKER_ADDR), // maker leg pays this party
            min_gap: 100,
            hashlock: Some(h(HASH)),
            secret: Some(SECRET),
            my_leg_locked: false,
            counterparty_lock: None,
            revealed_secret: None,
            now: 1000,
        }
    }

    // Maker leg observed by the taker. Safe: short timelock, 1800 + 100 <= 2000.
    fn maker_leg_safe() -> ObservedLock {
        ObservedLock {
            hashlock: h(HASH),
            token: a(TOK_B),
            amount: 500,
            recipient: a(TAKER_ADDR), // pays this party
            timelock: 1800,
            sender: a(MAKER_ADDR),
            exists: true,
        }
    }

    #[test]
    fn taker_happy_path_sequence() {
        let mut ctx = taker_ctx();
        let mut actions = Vec::new();

        // Start: generate secret.
        let mut st = SwapState::Start;
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::SecretGenerated);

        // SecretGenerated: lock own leg.
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::MyLegConfirmed);

        // MyLegLocked without observing the maker: wait.
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::CounterpartyLockObserved);

        // MyLegLocked after observing a Safe maker leg: redeem by revealing the secret.
        ctx.counterparty_lock = Some(maker_leg_safe());
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::RedeemConfirmed);

        // CounterpartyRedeemed: Done.
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

    // Critical test 1: the taker does not reveal the secret against an unsafe leg.
    #[test]
    fn taker_never_reveals_secret_against_unsafe_leg() {
        let mut ctx = taker_ctx();
        // Maker leg has the wrong hashlock.
        let mut bad = maker_leg_safe();
        bad.hashlock = h(0x01);
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::MyLegLocked, &ctx);
        // The machine refunds and never redeems.
        assert_eq!(action, NextAction::Refund);
        assert!(
            !matches!(action, NextAction::RedeemCounterpartyLeg { .. }),
            "the secret must never leak against an unsafe leg"
        );
    }

    // Critical test 1b: the taker does not reveal the secret against a leg that
    // is structurally correct but expires too close to now. Without the clock
    // gate, the machine would redeem and reveal the secret without a safe
    // mining window.
    #[test]
    fn taker_never_reveals_secret_against_leg_expiring_now() {
        let mut ctx = taker_ctx();
        // Maker leg is safe by inter-leg gap: 1800 + 100 <= 2000.
        ctx.counterparty_lock = Some(maker_leg_safe());
        // But the clock is already 1750: 1800 < 1750 + 100, so no safe window remains.
        ctx.now = 1750;

        let action = next_action(&SwapState::MyLegLocked, &ctx);
        assert_eq!(action, NextAction::Refund, "must refund and never redeem");
        assert!(
            !matches!(action, NextAction::RedeemCounterpartyLeg { .. }),
            "the secret must never leak against a leg about to expire"
        );
    }

    // ---------------- MAKER ----------------

    fn maker_ctx() -> SwapContext {
        SwapContext {
            role: Role::Maker,
            my_token: a(TOK_B),
            my_amount: 500,
            my_timelock: 1000,           // short
            my_recipient: a(TAKER_ADDR), // this party's leg pays the taker
            cp_token: a(TOK_A),
            cp_amount: 1000,
            me: a(MAKER_ADDR), // taker leg pays this party
            min_gap: 100,
            hashlock: Some(h(HASH)), // H agreed during the handshake
            secret: None,
            my_leg_locked: false,
            counterparty_lock: None,
            revealed_secret: None,
            now: 500,
        }
    }

    // Taker leg observed by the maker. Safe: long timelock >= 1000 + 100.
    fn taker_leg_safe() -> ObservedLock {
        ObservedLock {
            hashlock: h(HASH),
            token: a(TOK_A),
            amount: 1000,
            recipient: a(MAKER_ADDR), // pays this party
            timelock: 1200,
            sender: a(TAKER_ADDR),
            exists: true,
        }
    }

    #[test]
    fn maker_happy_path_sequence() {
        let mut ctx = maker_ctx();
        let mut actions = Vec::new();

        // Start without observing the taker: wait.
        let mut st = SwapState::Start;
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::CounterpartyLockObserved);

        // Start after observing a Safe taker leg: lock own leg with the same hashlock.
        ctx.counterparty_lock = Some(taker_leg_safe());
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::MyLegConfirmed);

        // WaitingForSecret without a secret: wait for the secret.
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::SecretObserved);

        // SecretLearned: redeem the taker leg with the learned secret.
        ctx.revealed_secret = Some(SECRET);
        actions.push(next_action(&st, &ctx));
        st = advance(st, SwapEvent::RedeemConfirmed);

        // CounterpartyRedeemed: Done.
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

    // Critical test 2: the maker does not lock against a leg with an unsafe gap.
    #[test]
    fn maker_never_locks_against_unsafe_gap() {
        let mut ctx = maker_ctx();
        // Taker leg with a timelock only slightly larger than maker leg.
        let mut bad = taker_leg_safe();
        bad.timelock = 1050; // 1050 < my(1000) + gap(100)
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::Start, &ctx);
        assert_eq!(
            action,
            NextAction::Abort {
                reason: AbortReason::UnsafeCounterparty(UnsafeReason::TimelockGapTooSmall)
            }
        );
        assert!(
            !matches!(action, NextAction::LockMyLeg { .. }),
            "the maker must never lock against an unsafe leg"
        );
    }

    // The maker also does not lock if the taker hashlock differs from agreed H.
    #[test]
    fn maker_never_locks_against_hashlock_mismatch() {
        let mut ctx = maker_ctx();
        let mut bad = taker_leg_safe();
        bad.hashlock = h(0x99); // differs from agreed H
        ctx.counterparty_lock = Some(bad);

        let action = next_action(&SwapState::Start, &ctx);
        assert_eq!(
            action,
            NextAction::Abort {
                reason: AbortReason::UnsafeCounterparty(UnsafeReason::HashlockMismatch)
            }
        );
        assert!(!matches!(action, NextAction::LockMyLeg { .. }));
    }

    // ---------------- refund paths ----------------

    #[test]
    fn taker_refunds_when_counterparty_never_locks() {
        let mut ctx = taker_ctx();
        ctx.counterparty_lock = None;
        ctx.now = ctx.my_timelock + 1; // expired while waiting

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
        ctx.now = ctx.my_timelock + 1; // expired while waiting for the secret

        let st = SwapState::WaitingForSecret;
        assert_eq!(next_action(&st, &ctx), NextAction::Refund);

        let st = advance(st, SwapEvent::TimelockExpired);
        assert_eq!(st, SwapState::Refunding);

        let st = advance(st, SwapEvent::RefundConfirmed);
        assert_eq!(st, SwapState::Refunded);
    }

    // ---------------- invalid transition ----------------

    #[test]
    fn invalid_transition_goes_to_aborted_no_panic() {
        // SecretObserved does not make sense in SecretGenerated.
        let st = advance(SwapState::SecretGenerated, SwapEvent::SecretObserved);
        assert_eq!(st, SwapState::Aborted(AbortReason::InvalidTransition));
        // Aborted state is absorbing.
        assert_eq!(
            advance(st, SwapEvent::MyLegConfirmed),
            SwapState::Aborted(AbortReason::InvalidTransition)
        );
    }

    #[test]
    fn terminal_states_absorb_events() {
        assert_eq!(
            advance(SwapState::Done, SwapEvent::TimelockExpired),
            SwapState::Done
        );
        assert_eq!(
            advance(SwapState::Refunded, SwapEvent::MyLegConfirmed),
            SwapState::Refunded
        );
    }

    // Wrong role for the state returns InvalidState without panicking.
    #[test]
    fn wrong_role_for_state_is_invalid_state() {
        let ctx = taker_ctx();
        // WaitingForSecret is a maker state.
        assert_eq!(
            next_action(&SwapState::WaitingForSecret, &ctx),
            NextAction::Abort {
                reason: AbortReason::InvalidState
            }
        );
    }
}

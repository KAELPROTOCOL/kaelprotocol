//! Counterparty-leg verification for the atomic swap wallet.

use serde::{Deserialize, Serialize};

pub type Address = [u8; 20];

/// Swap role used for asymmetric timelock checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Taker,
    Maker,
}

/// HTLC lock observed on the other chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedLock {
    pub hashlock: [u8; 32],
    pub token: Address,
    pub amount: u128,
    /// Recipient allowed to redeem with the preimage.
    pub recipient: Address,
    pub timelock: u64,
    pub sender: Address,
    /// Whether the lock was found on-chain.
    pub exists: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegExpectation {
    pub expected_hashlock: [u8; 32],
    pub expected_token: Address,
    pub expected_amount: u128,
    pub expected_recipient: Address,
    pub my_timelock: u64,
    pub min_gap: u64,
    pub now: u64,
    pub role: Role,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsafeReason {
    LockNotFound,
    HashlockMismatch,
    TokenMismatch,
    AmountMismatch,
    RecipientMismatch,
    TimelockGapTooSmall,
    TimelockInverted,
    CounterpartyExpiresTooSoon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyOutcome {
    Safe,
    Unsafe(UnsafeReason),
}

pub fn verify_counterparty_leg(
    expectation: &LegExpectation,
    observed: &ObservedLock,
) -> VerifyOutcome {
    use UnsafeReason::*;
    use VerifyOutcome::*;

    if !observed.exists {
        return Unsafe(LockNotFound);
    }
    if observed.hashlock != expectation.expected_hashlock {
        return Unsafe(HashlockMismatch);
    }
    if observed.token != expectation.expected_token {
        return Unsafe(TokenMismatch);
    }
    if observed.amount != expectation.expected_amount {
        return Unsafe(AmountMismatch);
    }
    if observed.recipient != expectation.expected_recipient {
        return Unsafe(RecipientMismatch);
    }
    if let Some(reason) = check_timelock_gap(expectation, observed) {
        return Unsafe(reason);
    }
    Safe
}

///
///
///   expirar DEPOIS, com margem:
///
///   ```text
///       observed.timelock >= my_timelock + min_gap
///   ```
///
///   - my_timelock < observed.timelock < my+gap   -> TimelockGapTooSmall
///
///   com margem:
///
///   ```text
///       my_timelock >= observed.timelock + min_gap
///   ```
///
///   - my-gap < observed.timelock < my_timelock   -> TimelockGapTooSmall
///
/// the short Maker, shorter if I am the long Taker) and with
/// pode ser roubado. Usamos somas saturadas para evitar overflow/underflow.
fn check_timelock_gap(
    expectation: &LegExpectation,
    observed: &ObservedLock,
) -> Option<UnsafeReason> {
    let my = expectation.my_timelock;
    let opp = observed.timelock;
    let gap = expectation.min_gap;
    let now = expectation.now;

    let structural = match expectation.role {
        Role::Maker => {
            if opp <= my {
                Some(UnsafeReason::TimelockInverted)
            } else if opp < my.saturating_add(gap) {
                Some(UnsafeReason::TimelockGapTooSmall)
            } else {
                None
            }
        }
        Role::Taker => {
            if opp >= my {
                Some(UnsafeReason::TimelockInverted)
            } else if opp.saturating_add(gap) > my {
                Some(UnsafeReason::TimelockGapTooSmall)
            } else {
                None
            }
        }
    };
    if structural.is_some() {
        return structural;
    }

    if opp < now.saturating_add(gap) {
        return Some(UnsafeReason::CounterpartyExpiresTooSoon);
    }
    None
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

    const ME: u8 = 0xEE;
    const HL: u8 = 0xAB;
    const TOK: u8 = 0x11;
    const AMT: u128 = 1000;

    fn maker_exp() -> LegExpectation {
        LegExpectation {
            expected_hashlock: h(HL),
            expected_token: a(TOK),
            expected_amount: AMT,
            expected_recipient: a(ME),
            my_timelock: 1000,
            min_gap: 100,
            now: 0, // far from expiry in base vectors
            role: Role::Maker,
        }
    }
    fn obs_for_maker() -> ObservedLock {
        ObservedLock {
            hashlock: h(HL),
            token: a(TOK),
            amount: AMT,
            recipient: a(ME),
            timelock: 1200,
            sender: a(0x01),
            exists: true,
        }
    }

    fn taker_exp() -> LegExpectation {
        LegExpectation {
            expected_hashlock: h(HL),
            expected_token: a(TOK),
            expected_amount: AMT,
            expected_recipient: a(ME),
            my_timelock: 2000,
            min_gap: 100,
            now: 0, // far from expiry in base vectors
            role: Role::Taker,
        }
    }
    fn obs_for_taker() -> ObservedLock {
        ObservedLock {
            hashlock: h(HL),
            token: a(TOK),
            amount: AMT,
            recipient: a(ME),
            timelock: 1800,
            sender: a(0x02),
            exists: true,
        }
    }

    #[test]
    fn safe_maker() {
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &obs_for_maker()),
            VerifyOutcome::Safe
        );
    }

    #[test]
    fn safe_taker() {
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &obs_for_taker()),
            VerifyOutcome::Safe
        );
    }

    #[test]
    fn safe_at_exact_gap_boundary() {
        let mut o = obs_for_maker();
        o.timelock = 1100; // == my(1000) + gap(100)
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Safe
        );

        let mut ot = obs_for_taker();
        ot.timelock = 1900; // 1900 + 100 == my(2000)
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &ot),
            VerifyOutcome::Safe
        );
    }

    #[test]
    fn lock_not_found() {
        let mut o = obs_for_maker();
        o.exists = false;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }

    #[test]
    fn hashlock_mismatch_is_detected() {
        // a checagem CENTRAL: hashlock diferente quebra a atomicidade.
        let mut o = obs_for_maker();
        o.hashlock = h(0x01);
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::HashlockMismatch)
        );
    }

    #[test]
    fn token_mismatch() {
        let mut o = obs_for_maker();
        o.token = a(0x99);
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TokenMismatch)
        );
    }

    #[test]
    fn amount_mismatch() {
        let mut o = obs_for_maker();
        o.amount = AMT + 1;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::AmountMismatch)
        );
    }

    #[test]
    fn recipient_mismatch_funds_would_go_to_another() {
        let mut o = obs_for_maker();
        o.recipient = a(0xBA); // not me (ME)
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::RecipientMismatch)
        );
    }

    // ---- gap de timelock: GapTooSmall por papel ----

    #[test]
    fn gap_too_small_maker() {
        let mut o = obs_for_maker();
        o.timelock = 1050;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockGapTooSmall)
        );
    }

    #[test]
    fn gap_too_small_taker() {
        let mut o = obs_for_taker();
        o.timelock = 1950;
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockGapTooSmall)
        );
    }

    // ---- gap de timelock: Inverted (lado errado) por papel ----

    #[test]
    fn inverted_maker() {
        let mut o = obs_for_maker();
        o.timelock = 900; // <= my(1000)
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_maker_equal_is_inverted() {
        let mut o = obs_for_maker();
        o.timelock = 1000; // == my, still on the wrong side
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_taker() {
        let mut o = obs_for_taker();
        o.timelock = 2100; // >= my(2000)
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_taker_equal_is_inverted() {
        let mut o = obs_for_taker();
        o.timelock = 2000; // == my, wrong side
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    // embora com gap inter-pernas correto, expira perto demais de `now`. Sem
    #[test]
    fn taker_unsafe_when_counterparty_expires_too_soon() {
        let mut e = taker_exp();
        e.now = 1750;
        let o = obs_for_taker(); // timelock 1800
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::CounterpartyExpiresTooSoon)
        );
    }

    #[test]
    fn safe_at_exact_now_window_boundary() {
        let mut e = taker_exp();
        e.now = 1700; // 1800 == 1700 + 100, exact safety window
        assert_eq!(
            verify_counterparty_leg(&e, &obs_for_taker()),
            VerifyOutcome::Safe
        );
    }

    #[test]
    fn maker_unsafe_when_counterparty_expires_too_soon() {
        let mut e = maker_exp();
        let mut o = obs_for_maker();
        o.timelock = 5000; // bem do lado certo (>> my=1000+gap)
        e.now = 4950; // but 5000 < 4950 + 100: no clock window
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::CounterpartyExpiresTooSoon)
        );
    }

    #[test]
    fn structural_failure_precedes_clock_failure() {
        let mut e = taker_exp();
        e.now = 5000; // tudo "expira" vs now
        let mut o = obs_for_taker();
        o.timelock = 2500; // >= my(2000), inverted wrong side
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn first_failure_wins_existence_before_hashlock() {
        let mut o = obs_for_maker();
        o.exists = false;
        o.hashlock = h(0x01); // also wrong, but LockNotFound comes first
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }
}

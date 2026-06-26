use orderbook::order::Order;
use swapkit::{
    assign_role, next_action, verify_counterparty_leg, LegExpectation, NextAction, ObservedLock,
    Role, SwapContext, SwapState, TimelockPolicy, UnsafeReason, VerifyOutcome,
};

fn a(byte: u8) -> [u8; 20] {
    [byte; 20]
}

fn h(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn safe_taker_expectation() -> LegExpectation {
    LegExpectation {
        expected_hashlock: h(0xAA),
        expected_token: a(0x11),
        expected_amount: 1_000,
        expected_recipient: a(0xEE),
        my_timelock: 2_000,
        min_gap: 100,
        now: 1_000,
        role: Role::Taker,
    }
}

fn safe_observed_for_taker() -> ObservedLock {
    ObservedLock {
        hashlock: h(0xAA),
        token: a(0x11),
        amount: 1_000,
        recipient: a(0xEE),
        timelock: 1_800,
        sender: a(0xBB),
        exists: true,
    }
}

fn taker_context_with(counterparty_lock: Option<ObservedLock>) -> SwapContext {
    SwapContext {
        role: Role::Taker,
        my_token: a(0x22),
        my_amount: 2_000,
        my_timelock: 2_000,
        my_recipient: a(0xBB),
        cp_token: a(0x11),
        cp_amount: 1_000,
        me: a(0xEE),
        min_gap: 100,
        hashlock: Some(h(0xAA)),
        secret: Some(h(0x55)),
        my_leg_locked: true,
        counterparty_lock,
        revealed_secret: None,
        now: 1_000,
    }
}

fn maker_context_with(counterparty_lock: Option<ObservedLock>) -> SwapContext {
    SwapContext {
        role: Role::Maker,
        my_token: a(0x11),
        my_amount: 1_000,
        my_timelock: 1_800,
        my_recipient: a(0xAA),
        cp_token: a(0x22),
        cp_amount: 2_000,
        me: a(0xEE),
        min_gap: 100,
        hashlock: Some(h(0xAA)),
        secret: None,
        my_leg_locked: false,
        counterparty_lock,
        revealed_secret: None,
        now: 1_000,
    }
}

fn mirrored_order(maker: u8, nonce: u64, created_at: u64, sells_x: bool) -> Order {
    let (sell_token, buy_token, sell_amount, buy_amount) = if sells_x {
        (a(0x11), a(0x22), 1_000, 500)
    } else {
        (a(0x22), a(0x11), 500, 1_000)
    };
    Order {
        maker: a(maker),
        sell_token,
        sell_chain_id: if sells_x { 1 } else { 10 },
        sell_amount,
        buy_token,
        buy_chain_id: if sells_x { 10 } else { 1 },
        buy_amount,
        valid_until: 4_000_000_000,
        nonce,
        created_at,
    }
}

#[test]
fn verifier_rejects_each_single_field_mutation_property() {
    let expectation = safe_taker_expectation();
    let base = safe_observed_for_taker();
    assert_eq!(
        verify_counterparty_leg(&expectation, &base),
        VerifyOutcome::Safe
    );

    let mut cases = Vec::new();

    let mut missing = base;
    missing.exists = false;
    cases.push((missing, UnsafeReason::LockNotFound));

    let mut hashlock = base;
    hashlock.hashlock = h(0xAB);
    cases.push((hashlock, UnsafeReason::HashlockMismatch));

    let mut token = base;
    token.token = a(0x12);
    cases.push((token, UnsafeReason::TokenMismatch));

    let mut amount = base;
    amount.amount += 1;
    cases.push((amount, UnsafeReason::AmountMismatch));

    let mut recipient = base;
    recipient.recipient = a(0xEF);
    cases.push((recipient, UnsafeReason::RecipientMismatch));

    for (observed, reason) in cases {
        assert_eq!(
            verify_counterparty_leg(&expectation, &observed),
            VerifyOutcome::Unsafe(reason)
        );
    }
}

#[test]
fn state_machine_never_redeems_secret_against_unsafe_counterparty_property() {
    let mut unsafe_locks = Vec::new();
    let mut wrong_hashlock = safe_observed_for_taker();
    wrong_hashlock.hashlock = h(0xAB);
    unsafe_locks.push(wrong_hashlock);

    let mut wrong_token = safe_observed_for_taker();
    wrong_token.token = a(0x12);
    unsafe_locks.push(wrong_token);

    let mut wrong_recipient = safe_observed_for_taker();
    wrong_recipient.recipient = a(0xEF);
    unsafe_locks.push(wrong_recipient);

    for observed in unsafe_locks {
        let action = next_action(&SwapState::MyLegLocked, &taker_context_with(Some(observed)));
        assert_ne!(
            action,
            NextAction::RedeemCounterpartyLeg { secret: h(0x55) }
        );
        assert_eq!(action, NextAction::Refund);
    }
}

#[test]
fn maker_never_locks_against_unsafe_counterparty_property() {
    let mut observed = ObservedLock {
        hashlock: h(0xAA),
        token: a(0x22),
        amount: 2_000,
        recipient: a(0xEE),
        timelock: 2_000,
        sender: a(0xAA),
        exists: true,
    };
    assert!(matches!(
        next_action(&SwapState::Start, &maker_context_with(Some(observed))),
        NextAction::LockMyLeg { .. }
    ));

    observed.timelock = 1_850;
    assert!(matches!(
        next_action(&SwapState::Start, &maker_context_with(Some(observed))),
        NextAction::Abort { .. }
    ));
}

#[test]
fn handshake_roles_are_complementary_for_arrival_and_digest_ties_property() {
    let policy = TimelockPolicy {
        taker_lock_secs: 7_200,
        maker_lock_secs: 3_600,
        min_gap: 1_800,
    };
    assert!(policy.taker_lock_secs >= policy.maker_lock_secs + policy.min_gap);

    for created_at in [1, 10, 42, 1_000] {
        let resting = mirrored_order(0xAA, created_at, created_at, true);
        let crossing = mirrored_order(0xBB, created_at + 1, created_at + 1, false);
        assert_eq!(assign_role(&resting, &crossing), Role::Maker);
        assert_eq!(assign_role(&crossing, &resting), Role::Taker);
    }

    for nonce in 1..8 {
        let left = mirrored_order(0xAA, nonce, 50, true);
        let right = mirrored_order(0xBB, nonce + 100, 50, false);
        assert_ne!(assign_role(&left, &right), assign_role(&right, &left));
    }
}

#[test]
fn redeem_action_debug_redacts_secret_property() {
    let secret = h(0x5E);
    let rendered = format!("{:?}", NextAction::RedeemCounterpartyLeg { secret });
    assert!(rendered.contains("<redacted>"));
    assert!(!rendered.contains("5e"));
    assert!(!rendered.contains("94"));
}

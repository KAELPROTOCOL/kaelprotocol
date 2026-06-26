//! Deterministic Taker/Maker role assignment and `SwapContext` derivation from
//! matched orders. Pure logic only: no network, clock, or randomness.
//!
//! The resting order is Maker and uses the short timelock. The crossing order is
//! Taker, generates the secret, locks first, uses the long timelock, and carries
//! the free-option. Time ties are broken by signed order digest.

use crate::sm::SwapContext;
use crate::verify::Role;
use orderbook::eip712::digest;
use orderbook::order::Order;

///
/// ```text
/// taker_lock_secs >= maker_lock_secs + min_gap
/// ```
///
#[derive(Clone, Copy, Debug)]
pub struct TimelockPolicy {
    pub taker_lock_secs: u64,
    pub maker_lock_secs: u64,
    pub min_gap: u64,
}

pub fn assign_role(order_self: &Order, order_cp: &Order) -> Role {
    use std::cmp::Ordering;
    match order_self.created_at.cmp(&order_cp.created_at) {
        Ordering::Less => Role::Maker,
        Ordering::Greater => Role::Taker,
        // Time tie: signed-data tie-breaker.
        Ordering::Equal => {
            if digest(order_self) < digest(order_cp) {
                Role::Maker
            } else {
                Role::Taker
            }
        }
    }
}

/// Derive this wallet's `SwapContext` from the matched orders, assigned role,
/// hashlock/secret material, timelock policy, and current time.
///
/// Role-specific hashlock material:
/// - Taker: `hashlock = Some(H)`, `secret = Some(s)` with `H = SHA256(s)`.
/// - Maker: `hashlock = Some(H)`, `secret = None`.
///
/// Observation fields start empty and are filled by the executor.
pub fn derive_context(
    order_self: &Order,
    order_cp: &Order,
    role: Role,
    hashlock: Option<[u8; 32]>,
    secret: Option<[u8; 32]>,
    policy: &TimelockPolicy,
    now: u64,
) -> SwapContext {
    let my_lock = match role {
        Role::Taker => policy.taker_lock_secs,
        Role::Maker => policy.maker_lock_secs,
    };
    SwapContext {
        role,
        my_token: order_self.sell_token,
        my_amount: order_self.sell_amount,
        my_timelock: now.saturating_add(my_lock),
        my_recipient: order_cp.maker,
        cp_token: order_cp.sell_token,
        // expected_amount = o que a contraparte realmente TRAVA (seu sell inteiro),
        cp_amount: order_cp.sell_amount,
        me: order_self.maker,
        min_gap: policy.min_gap,
        hashlock,
        secret,
        my_leg_locked: false,
        counterparty_lock: None,
        revealed_secret: None,
        now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sm::{next_action, NextAction, SwapState};
    use crate::verify::ObservedLock;

    const X: u8 = 0x11; // ativo na chain 1
    const Y: u8 = 0x22; // ativo na chain 10

    #[allow(clippy::too_many_arguments)]
    fn ord(
        maker: u8,
        nonce: u64,
        sell_tok: u8,
        sell_chain: u64,
        sell_amt: u128,
        buy_tok: u8,
        buy_chain: u64,
        buy_amt: u128,
        created_at: u64,
    ) -> Order {
        Order {
            maker: [maker; 20],
            sell_token: [sell_tok; 20],
            sell_chain_id: sell_chain,
            sell_amount: sell_amt,
            buy_token: [buy_tok; 20],
            buy_chain_id: buy_chain,
            buy_amount: buy_amt,
            valid_until: 4_000_000_000,
            nonce,
            created_at,
        }
    }

    // Exact mirrored pair: A sells X(1000) for Y(500); B sells Y(500) for X(1000).
    fn order_a(created_at: u64) -> Order {
        ord(0xAA, 1, X, 1, 1000, Y, 10, 500, created_at)
    }
    fn order_b(created_at: u64) -> Order {
        ord(0xBB, 2, Y, 10, 500, X, 1, 1000, created_at)
    }

    fn policy() -> TimelockPolicy {
        TimelockPolicy {
            taker_lock_secs: 7200,
            maker_lock_secs: 3600,
            min_gap: 1800,
        }
    }

    #[test]
    fn complementary_roles_by_arrival() {
        let a = order_a(10);
        let b = order_b(20); // B arrived later, so B crossed.
        assert_eq!(assign_role(&a, &b), Role::Maker, "A resting order = Maker");
        assert_eq!(assign_role(&b, &a), Role::Taker, "B (cruzou) = Taker");

        let a2 = order_a(20);
        let b2 = order_b(10);
        assert_eq!(assign_role(&a2, &b2), Role::Taker);
        assert_eq!(assign_role(&b2, &a2), Role::Maker);
    }

    // created_at tie uses signed digest tie-breaker; lower digest becomes Maker.
    #[test]
    fn complementary_roles_by_digest_tie() {
        let a = order_a(50);
        let b = order_b(50); // same created_at, so tie-breaker applies

        let ra = assign_role(&a, &b);
        let rb = assign_role(&b, &a);
        assert_ne!(ra, rb, "roles must be complementary even on ties");

        if digest(&a) < digest(&b) {
            assert_eq!(ra, Role::Maker);
            assert_eq!(rb, Role::Taker);
        } else {
            assert_eq!(ra, Role::Taker);
            assert_eq!(rb, Role::Maker);
        }
    }

    // mapeamento + as duas sutilezas: expected_amount = sell da cp (melhora de
    #[test]
    fn derives_context_with_price_improvement_and_recipients() {
        let a = order_a(10); // A = Maker (mais antigo)
        let b = ord(0xBB, 2, Y, 10, 600, X, 1, 1000, 20);
        let role = assign_role(&a, &b);
        assert_eq!(role, Role::Maker);

        let ctx = derive_context(&a, &b, role, Some([0xAB; 32]), None, &policy(), 1000);

        assert_eq!(ctx.role, Role::Maker);
        assert_eq!(ctx.my_token, a.sell_token);
        assert_eq!(ctx.my_amount, 1000);
        assert_eq!(ctx.cp_token, b.sell_token);
        assert_eq!(ctx.cp_amount, 600);
        assert_ne!(ctx.cp_amount, a.buy_amount, "not my buy_amount");
        assert_eq!(ctx.my_recipient, b.maker, "my leg pays the counterparty");
        assert_eq!(ctx.me, a.maker, "I redeem the opposite leg");
        // maker uses the short timelock
        assert_eq!(ctx.my_timelock, 1000 + 3600);
        assert_eq!(ctx.min_gap, 1800);
        assert!(ctx.secret.is_none(), "maker does not have the secret");
    }

    #[test]
    fn divergence_both_maker_swap_never_starts() {
        let p = policy();
        let a = order_a(10);
        let b_seen_by_a = order_b(20); // from A view: a<b so A is Maker
        let b = order_b(10);
        let a_seen_by_b = order_a(20); // from B view: b<a so B is Maker

        let ra = assign_role(&a, &b_seen_by_a);
        let rb = assign_role(&b, &a_seen_by_b);
        assert_eq!(ra, Role::Maker);
        assert_eq!(rb, Role::Maker);

        let ctx_a = derive_context(&a, &b_seen_by_a, ra, Some([0xAB; 32]), None, &p, 1000);
        let ctx_b = derive_context(&b, &a_seen_by_b, rb, Some([0xAB; 32]), None, &p, 1000);

        // Neither emits LockMyLeg, so no funds move.
        let act_a = next_action(&SwapState::Start, &ctx_a);
        let act_b = next_action(&SwapState::Start, &ctx_b);
        assert_eq!(act_a, NextAction::WaitForCounterpartyLock);
        assert_eq!(act_b, NextAction::WaitForCounterpartyLock);
        assert!(!matches!(act_a, NextAction::LockMyLeg { .. }));
        assert!(!matches!(act_b, NextAction::LockMyLeg { .. }));
    }

    #[test]
    fn divergence_both_taker_secret_never_revealed() {
        let p = policy();
        let a = order_a(20);
        let b_seen_by_a = order_b(10); // from A view: a>b so A is Taker
        let ra = assign_role(&a, &b_seen_by_a);
        assert_eq!(ra, Role::Taker);

        let secret = [0x5e; 32];
        let h = [0xAB; 32];
        let ctx = derive_context(&a, &b_seen_by_a, ra, Some(h), Some(secret), &p, 1000);

        let b_leg = ObservedLock {
            hashlock: h,
            token: ctx.cp_token,
            amount: ctx.cp_amount,
            recipient: ctx.me,         // pagaria A corretamente...
            timelock: ctx.my_timelock, // but without asymmetry (equal to mine)
            sender: [0xBB; 20],
            exists: true,
        };

        let mut ctx2 = ctx.clone();
        ctx2.my_leg_locked = true;
        ctx2.counterparty_lock = Some(b_leg);

        let action = next_action(&SwapState::MyLegLocked, &ctx2);
        assert_eq!(
            action,
            NextAction::Refund,
            "no asymmetry -> Unsafe -> refund"
        );
        assert!(
            !matches!(action, NextAction::RedeemCounterpartyLeg { .. }),
            "the secret is never revealed under role divergence"
        );
    }
}

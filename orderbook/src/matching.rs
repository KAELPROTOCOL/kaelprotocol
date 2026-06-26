//!
//!
//!    Cruza quando ambos recebem ao menos o que pediram:
//!
//!    ```text
//!    A.sell_amount >= B.buy_amount   (A entrega X suficiente para B)
//!    B.sell_amount >= A.buy_amount   (B entrega Y suficiente para A)
//!    ```

use crate::order::Order;

/// Espelhamento estrito de tokens E chains entre as duas pernas.
fn mirrors(a: &Order, b: &Order) -> bool {
    a.sell_token == b.buy_token
        && a.sell_chain_id == b.buy_chain_id
        && a.buy_token == b.sell_token
        && a.buy_chain_id == b.sell_chain_id
}

fn crosses(a: &Order, b: &Order) -> bool {
    a.sell_amount >= b.buy_amount && b.sell_amount >= a.buy_amount
}

pub fn compatible(a: &Order, b: &Order, now: u64) -> bool {
    !a.is_expired(now) && !b.is_expired(now) && mirrors(a, b) && crosses(a, b)
}

/// Entre os `makers`, escolhe o melhor para `taker` por price-time.
///
pub fn best_match_for(taker: &Order, makers: &[Order], now: u64) -> Option<usize> {
    if taker.is_expired(now) {
        return None;
    }
    let mut best: Option<usize> = None;
    for (i, m) in makers.iter().enumerate() {
        if !compatible(taker, m, now) {
            continue;
        }
        best = Some(match best {
            None => i,
            Some(b) => {
                if better_for_taker(m, &makers[b]) {
                    i
                } else {
                    b
                }
            }
        });
    }
    best
}

/// Total price-time ordering among maker candidates from the taker's view.
pub fn cmp_makers_for_taker(a: &Order, b: &Order) -> std::cmp::Ordering {
    b.sell_amount
        .cmp(&a.sell_amount) // larger sell_amount first
        .then_with(|| a.created_at.cmp(&b.created_at)) // older first
        .then_with(|| a.nonce.cmp(&b.nonce)) // lower nonce first
}

fn better_for_taker(cand: &Order, cur: &Order) -> bool {
    cmp_makers_for_taker(cand, cur) == std::cmp::Ordering::Less
}

pub fn find_match(orders: &[Order], now: u64) -> Option<(usize, usize)> {
    for (i, taker) in orders.iter().enumerate() {
        let mut best: Option<usize> = None;
        for (j, maker) in orders.iter().enumerate() {
            if i == j || !compatible(taker, maker, now) {
                continue;
            }
            best = Some(match best {
                None => j,
                Some(b) => {
                    if better_for_taker(maker, &orders[b]) {
                        j
                    } else {
                        b
                    }
                }
            });
        }
        if let Some(j) = best {
            return Some((i, j));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(b: u8) -> [u8; 20] {
        [b; 20]
    }

    const X: u8 = 0x11; // token na chain 1
    const Y: u8 = 0x22; // token na chain 10

    /// A vende `sell` de X (chain 1), quer `buy` de Y (chain 10).
    fn order_a(sell: u128, buy: u128, valid_until: u64, created_at: u64, nonce: u64) -> Order {
        Order {
            maker: addr(0xAA),
            sell_token: addr(X),
            sell_chain_id: 1,
            sell_amount: sell,
            buy_token: addr(Y),
            buy_chain_id: 10,
            buy_amount: buy,
            valid_until,
            nonce,
            created_at,
        }
    }

    /// B (espelho) vende `sell` de Y (chain 10), quer `buy` de X (chain 1).
    fn order_b(sell: u128, buy: u128, valid_until: u64, created_at: u64, nonce: u64) -> Order {
        Order {
            maker: addr(0xBB),
            sell_token: addr(Y),
            sell_chain_id: 10,
            sell_amount: sell,
            buy_token: addr(X),
            buy_chain_id: 1,
            buy_amount: buy,
            valid_until,
            nonce,
            created_at,
        }
    }

    #[test]
    fn mirrored_crossing_matches() {
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(compatible(&a, &b, 500));
        assert_eq!(find_match(&[a, b], 500), Some((0, 1)));
    }

    #[test]
    fn exact_amounts_match() {
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(crosses(&a, &b));
    }

    #[test]
    fn wrong_chain_no_match() {
        let a = order_a(100, 200, 1000, 1, 0);
        let mut b = order_b(200, 100, 1000, 1, 0);
        b.sell_chain_id = 137; // not A buy_chain (10)
        assert!(!compatible(&a, &b, 500));
        assert_eq!(find_match(&[a, b], 500), None);
    }

    #[test]
    fn same_direction_no_match() {
        let a1 = order_a(100, 200, 1000, 1, 0);
        let a2 = order_a(100, 200, 1000, 1, 1); // ambos vendem X
        assert!(!compatible(&a1, &a2, 500));
        assert_eq!(find_match(&[a1, a2], 500), None);
    }

    #[test]
    fn non_crossing_prices_no_match() {
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(150, 100, 1000, 1, 0); // sell_b (150) < buy_a (200)
        assert!(!crosses(&a, &b));
        assert!(!compatible(&a, &b, 500));
    }

    #[test]
    fn expired_out_of_match() {
        let a = order_a(100, 200, 400, 1, 0); // valid only until 400
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(!compatible(&a, &b, 500)); // now=500 > 400
        assert_eq!(find_match(&[a, b], 500), None);
    }

    #[test]
    fn best_price_wins() {
        let taker = order_a(100, 200, 1000, 1, 0);
        let m_meh = order_b(200, 100, 1000, 5, 1); // gives 200 Y
        let m_best = order_b(300, 100, 1000, 9, 2); // gives 300 Y (better for taker)
        let makers = vec![m_meh.clone(), m_best.clone()];
        assert_eq!(best_match_for(&taker, &makers, 500), Some(1));
    }

    #[test]
    fn price_tie_breaks_by_oldest() {
        let taker = order_a(100, 200, 1000, 1, 0);
        let m_new = order_b(200, 100, 1000, 50, 1); // arrived later
        let m_old = order_b(200, 100, 1000, 10, 2); // same price, older
        let makers = vec![m_new, m_old];
        assert_eq!(best_match_for(&taker, &makers, 500), Some(1)); // the older order
    }
}

//! Camada 3 (matching) — função PURA de descoberta de pares.
//!
//! Sem rede, sem estado, sem efeito colateral, sem relógio do sistema.
//! `now` entra como parâmetro. Decisões de fundação implementadas aqui:
//!
//! 1. ESPELHAMENTO: A e B casam só se o que A vende é o que B compra e
//!    vice-versa — incluindo a CHAIN, não só o token (é cross-chain).
//! 2. CRUZAMENTO DE PREÇO sob TRAVA INTEGRAL: cada lado tranca o `sell_amount`
//!    inteiro; o excedente conta como melhora de preço para a contraparte.
//!    Cruza quando ambos recebem ao menos o que pediram:
//!
//!    ```text
//!    A.sell_amount >= B.buy_amount   (A entrega X suficiente para B)
//!    B.sell_amount >= A.buy_amount   (B entrega Y suficiente para A)
//!    ```
//! 3. SEM PREENCHIMENTO PARCIAL: não dividimos ordens; cada lado compromete
//!    o sell_amount inteiro. (fill parcial é peça futura com fundação própria.)
//! 4. PRIORIDADE PRICE-TIME (neutra): melhor preço para o taker primeiro;
//!    empate de preço → o maker mais antigo (menor `created_at`); empate de
//!    tempo → menor `nonce` (determinismo total, sem arbítrio do operador).

use crate::order::Order;

/// Espelhamento estrito de tokens E chains entre as duas pernas.
fn mirrors(a: &Order, b: &Order) -> bool {
    a.sell_token == b.buy_token
        && a.sell_chain_id == b.buy_chain_id
        && a.buy_token == b.sell_token
        && a.buy_chain_id == b.sell_chain_id
}

/// Cruzamento de preço sob trava integral (ambas as desigualdades).
fn crosses(a: &Order, b: &Order) -> bool {
    a.sell_amount >= b.buy_amount && b.sell_amount >= a.buy_amount
}

/// Dois pedidos são compatíveis (espelham e cruzam) e ambos estão vigentes.
pub fn compatible(a: &Order, b: &Order, now: u64) -> bool {
    !a.is_expired(now) && !b.is_expired(now) && mirrors(a, b) && crosses(a, b)
}

/// Entre os `makers`, escolhe o melhor para `taker` por price-time.
///
/// Preço para o taker sob trava integral: o taker entrega seu `sell_amount`
/// (fixo) e recebe TODO o `sell_amount` do maker — logo, mais `sell_amount` do
/// maker = melhor preço para o taker. Empate de preço → maker mais antigo;
/// empate de tempo → menor nonce. Retorna o índice em `makers`.
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

/// Ordem total price-time entre candidatos a maker, do ponto de vista do taker,
/// MELHOR primeiro (`Ordering::Less` == melhor). Critérios: maior `sell_amount`
/// (preço para o taker) > menor `created_at` (mais antigo) > menor `nonce`
/// (desempate determinístico final). FONTE ÚNICA do ranking price-time — tanto o
/// matcher puro quanto o caminho servido pelo livro (ADR-004) usam esta função,
/// para que NÃO divirjam.
pub fn cmp_makers_for_taker(a: &Order, b: &Order) -> std::cmp::Ordering {
    b.sell_amount
        .cmp(&a.sell_amount) // maior sell_amount primeiro
        .then_with(|| a.created_at.cmp(&b.created_at)) // mais antigo primeiro
        .then_with(|| a.nonce.cmp(&b.nonce)) // menor nonce primeiro
}

/// `cand` é estritamente melhor que `cur` para o taker? Delega à ordem total
/// [`cmp_makers_for_taker`] — uma única definição de "melhor".
fn better_for_taker(cand: &Order, cur: &Order) -> bool {
    cmp_makers_for_taker(cand, cur) == std::cmp::Ordering::Less
}

/// Varre todas as ordens e devolve o primeiro par compatível por índices
/// `(i, j)`, com `i` sendo o taker na ordem de chegada (price-time aplicado ao
/// escolher o maker). Útil para o servidor informar pares. Determinístico.
pub fn find_match(orders: &[Order], now: u64) -> Option<(usize, usize)> {
    for (i, taker) in orders.iter().enumerate() {
        // candidatos = todas as outras ordens; mantemos os índices originais
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

    // 1) par espelhado com preços que cruzam → casa
    #[test]
    fn mirrored_crossing_matches() {
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(compatible(&a, &b, 500));
        assert_eq!(find_match(&[a, b], 500), Some((0, 1)));
    }

    // 2) exato (sell == buy dos dois lados) também cruza
    #[test]
    fn exact_amounts_match() {
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(crosses(&a, &b));
    }

    // 3) não-espelhado (chain errada) → não casa
    #[test]
    fn wrong_chain_no_match() {
        let a = order_a(100, 200, 1000, 1, 0);
        let mut b = order_b(200, 100, 1000, 1, 0);
        b.sell_chain_id = 137; // não é a buy_chain de A (10)
        assert!(!compatible(&a, &b, 500));
        assert_eq!(find_match(&[a, b], 500), None);
    }

    // 4) mesma direção (não-espelhado por token) → não casa
    #[test]
    fn same_direction_no_match() {
        let a1 = order_a(100, 200, 1000, 1, 0);
        let a2 = order_a(100, 200, 1000, 1, 1); // ambos vendem X
        assert!(!compatible(&a1, &a2, 500));
        assert_eq!(find_match(&[a1, a2], 500), None);
    }

    // 5) preços que NÃO cruzam → não casa
    #[test]
    fn non_crossing_prices_no_match() {
        // A quer 200 Y por 100 X; B só oferece 150 Y e quer 100 X.
        let a = order_a(100, 200, 1000, 1, 0);
        let b = order_b(150, 100, 1000, 1, 0); // sell_b (150) < buy_a (200)
        assert!(!crosses(&a, &b));
        assert!(!compatible(&a, &b, 500));
    }

    // 6) ordem expirada → fora do match
    #[test]
    fn expired_out_of_match() {
        let a = order_a(100, 200, 400, 1, 0); // valida só até 400
        let b = order_b(200, 100, 1000, 1, 0);
        assert!(!compatible(&a, &b, 500)); // now=500 > 400
        assert_eq!(find_match(&[a, b], 500), None);
    }

    // 7) melhor preço vence (entre vários makers, o que dá mais Y)
    #[test]
    fn best_price_wins() {
        let taker = order_a(100, 200, 1000, 1, 0);
        let m_meh = order_b(200, 100, 1000, 5, 1); // dá 200 Y
        let m_best = order_b(300, 100, 1000, 9, 2); // dá 300 Y (melhor p/ taker)
        let makers = vec![m_meh.clone(), m_best.clone()];
        assert_eq!(best_match_for(&taker, &makers, 500), Some(1));
    }

    // 8) empate de preço → o mais antigo (menor created_at) vence
    #[test]
    fn price_tie_breaks_by_oldest() {
        let taker = order_a(100, 200, 1000, 1, 0);
        let m_new = order_b(200, 100, 1000, 50, 1); // chegou depois
        let m_old = order_b(200, 100, 1000, 10, 2); // mesmo preço, mais antigo
        let makers = vec![m_new, m_old];
        assert_eq!(best_match_for(&taker, &makers, 500), Some(1)); // o antigo
    }
}

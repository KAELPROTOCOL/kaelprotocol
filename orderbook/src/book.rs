//! In-memory orderbook state with verified ingestion at the boundary.

use crate::eip712::{self, VerifyError};
use crate::matching;
use crate::order::{Address, Order};
use std::collections::HashSet;

/// Signature verification scheme used before accepting an order.
pub trait SignatureVerifier: Send + Sync {
    fn verify(&self, order: &Order, signature: &[u8], now: u64) -> Result<[u8; 32], VerifyError>;
}

/// EIP-712 verifier equivalent to `Order.sol`.
pub struct Eip712Verifier;

impl SignatureVerifier for Eip712Verifier {
    fn verify(&self, order: &Order, signature: &[u8], now: u64) -> Result<[u8; 32], VerifyError> {
        eip712::verify(order, signature, now)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    Verify(VerifyError),
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MatchPair {
    #[serde(serialize_with = "ser_hex")]
    pub taker_hash: [u8; 32],
    #[serde(serialize_with = "ser_hex")]
    pub maker_hash: [u8; 32],
}

fn ser_hex<S: serde::Serializer>(b: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("0x{}", hex::encode(b)))
}

pub struct Book<V: SignatureVerifier> {
    orders: Vec<Order>,
    hashes: Vec<[u8; 32]>, // canonical hash for each order in `orders` (parallel)
    seen: HashSet<[u8; 32]>, // order-hash anti-replay
    verifier: V,
}

impl<V: SignatureVerifier> Book<V> {
    pub fn new(verifier: V) -> Self {
        Self {
            orders: Vec::new(),
            hashes: Vec::new(),
            seen: HashSet::new(),
            verifier,
        }
    }

    pub fn len(&self) -> usize {
        self.orders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn submit(
        &mut self,
        order: Order,
        signature: &[u8],
        now: u64,
    ) -> Result<[u8; 32], SubmitError> {
        let hash = self
            .verifier
            .verify(&order, signature, now)
            .map_err(SubmitError::Verify)?;
        if self.seen.contains(&hash) {
            return Err(SubmitError::Duplicate);
        }
        self.seen.insert(hash);
        self.orders.push(order);
        self.hashes.push(hash);
        Ok(hash)
    }

    pub fn matches_for(&self, maker: &Address, now: u64) -> Vec<MatchPair> {
        let mut out = Vec::new();
        for (i, taker) in self.orders.iter().enumerate() {
            if &taker.maker != maker {
                continue;
            }
            let mut cands: Vec<usize> = (0..self.orders.len())
                .filter(|&j| j != i && matching::compatible(taker, &self.orders[j], now))
                .collect();
            cands
                .sort_by(|&x, &y| matching::cmp_makers_for_taker(&self.orders[x], &self.orders[y]));
            for j in cands {
                out.push(MatchPair {
                    taker_hash: self.hashes[i],
                    maker_hash: self.hashes[j],
                });
            }
        }
        out
    }

    pub fn orders(&self) -> &[Order] {
        &self.orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AcceptVerifier;
    impl SignatureVerifier for AcceptVerifier {
        fn verify(&self, o: &Order, sig: &[u8], now: u64) -> Result<[u8; 32], VerifyError> {
            if o.is_expired(now) {
                return Err(VerifyError::OrderExpired);
            }
            if sig != b"ok" {
                return Err(VerifyError::BadSignature);
            }
            let mut h = [0u8; 32];
            h[..20].copy_from_slice(&o.maker);
            h[24..].copy_from_slice(&o.nonce.to_be_bytes());
            Ok(h)
        }
    }

    fn order(maker: u8, nonce: u64, valid_until: u64) -> Order {
        Order {
            maker: [maker; 20],
            sell_token: [0x11; 20],
            sell_chain_id: 1,
            sell_amount: 100,
            buy_token: [0x22; 20],
            buy_chain_id: 10,
            buy_amount: 200,
            valid_until,
            nonce,
            created_at: 1,
        }
    }

    fn mirror(maker: u8, nonce: u64, valid_until: u64) -> Order {
        Order {
            maker: [maker; 20],
            sell_token: [0x22; 20],
            sell_chain_id: 10,
            sell_amount: 200,
            buy_token: [0x11; 20],
            buy_chain_id: 1,
            buy_amount: 100,
            valid_until,
            nonce,
            created_at: 1,
        }
    }

    #[test]
    fn valid_order_enters() {
        let mut b = Book::new(AcceptVerifier);
        assert!(b.submit(order(0xAA, 1, 1000), b"ok", 500).is_ok());
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn invalid_signature_rejected_at_edge() {
        let mut b = Book::new(AcceptVerifier);
        assert_eq!(
            b.submit(order(0xAA, 1, 1000), b"bad", 500),
            Err(SubmitError::Verify(VerifyError::BadSignature))
        );
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn expired_order_rejected() {
        let mut b = Book::new(AcceptVerifier);
        assert_eq!(
            b.submit(order(0xAA, 1, 400), b"ok", 500),
            Err(SubmitError::Verify(VerifyError::OrderExpired))
        );
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn duplicate_rejected() {
        let mut b = Book::new(AcceptVerifier);
        assert!(b.submit(order(0xAA, 1, 1000), b"ok", 500).is_ok());
        assert_eq!(
            b.submit(order(0xAA, 1, 1000), b"ok", 500),
            Err(SubmitError::Duplicate)
        );
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn compatible_pair_is_reported() {
        let mut b = Book::new(AcceptVerifier);
        b.submit(order(0xAA, 1, 1000), b"ok", 500).unwrap();
        b.submit(mirror(0xBB, 2, 1000), b"ok", 500).unwrap();
        let m = b.matches_for(&[0xAA; 20], 500);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn served_matches_are_price_time_ordered() {
        let mut b = Book::new(AcceptVerifier);
        b.submit(order(0xAA, 1, 1000), b"ok", 500).unwrap(); // taker

        let mut m_meh = mirror(0xBB, 2, 1000);
        m_meh.sell_amount = 200; // gives 200 Y
        let mut m_best = mirror(0xCC, 3, 1000);
        m_best.sell_amount = 300; // gives 300 Y: better taker price
        b.submit(m_meh, b"ok", 500).unwrap();
        b.submit(m_best, b"ok", 500).unwrap();

        let m = b.matches_for(&[0xAA; 20], 500);
        assert_eq!(m.len(), 2);

        let mut best_hash = [0u8; 32];
        best_hash[..20].copy_from_slice(&[0xCC; 20]);
        best_hash[24..].copy_from_slice(&3u64.to_be_bytes());
        assert_eq!(
            m[0].maker_hash, best_hash,
            "best price must be served first"
        );
    }
}

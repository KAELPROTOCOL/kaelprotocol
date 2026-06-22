//! Estado do livro em memória + ingestão verificada na borda.
//!
//! DECISÕES DE FUNDAÇÃO:
//! - Estado em MEMÓRIA (sem banco): ordens são efêmeras e assinadas; se o
//!   servidor reinicia, os makers reenviam e nada se perde (fundos nunca
//!   estiveram aqui).
//! - VERIFICAÇÃO NA BORDA: toda ordem tem a assinatura re-verificada antes de
//!   entrar. Inválida/expirada é rejeitada e NÃO entra.
//! - VERIFICADOR EXTENSÍVEL: o trait [`SignatureVerifier`] permite Bitcoin/Solana
//!   como novos esquemas sem refatorar o livro.
//! - O servidor SÓ INFORMA matches; nunca executa, altera nem prioriza.

use crate::eip712::{self, VerifyError};
use crate::matching;
use crate::order::{Address, Order};
use std::collections::HashSet;

/// Esquema de verificação de assinatura. EIP-712 é a primeira impl; Bitcoin e
/// Solana entram como novas impls sem mexer no resto do livro.
pub trait SignatureVerifier: Send + Sync {
    /// Verifica e retorna o hash canônico da ordem (chave anti-replay).
    fn verify(&self, order: &Order, signature: &[u8], now: u64) -> Result<[u8; 32], VerifyError>;
}

/// Verificador EIP-712 (equivalente ao `Order.sol`).
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

/// Um par compatível encontrado pelo matcher, por hashes canônicos das ordens.
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

/// O livro: ordens ativas + hashes já vistos (anti-replay) + verificador.
pub struct Book<V: SignatureVerifier> {
    orders: Vec<Order>,
    hashes: Vec<[u8; 32]>, // hash canônico de cada ordem em `orders` (paralelo)
    seen: HashSet<[u8; 32]>, // anti-replay por hash de ordem
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

    /// Ingestão verificada na borda. Re-verifica a assinatura; rejeita
    /// inválida/expirada/duplicada ANTES de inserir. Retorna o hash canônico.
    pub fn submit(&mut self, order: Order, signature: &[u8], now: u64) -> Result<[u8; 32], SubmitError> {
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

    /// Informa todos os pares compatíveis que envolvem `maker` (modelo pull).
    /// SÓ INFORMA — não executa nada. `now` é parâmetro.
    pub fn matches_for(&self, maker: &Address, now: u64) -> Vec<MatchPair> {
        let mut out = Vec::new();
        for (i, taker) in self.orders.iter().enumerate() {
            if &taker.maker != maker {
                continue;
            }
            for (j, cand) in self.orders.iter().enumerate() {
                if i == j {
                    continue;
                }
                if matching::compatible(taker, cand, now) {
                    out.push(MatchPair {
                        taker_hash: self.hashes[i],
                        maker_hash: self.hashes[j],
                    });
                }
            }
        }
        out
    }

    /// Acesso de leitura às ordens ativas (para o matcher global / depuração).
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verificador trivial para testar a mecânica do livro isoladamente:
    // aceita qualquer ordem não-expirada e usa um hash determinístico simples.
    struct AcceptVerifier;
    impl SignatureVerifier for AcceptVerifier {
        fn verify(&self, o: &Order, sig: &[u8], now: u64) -> Result<[u8; 32], VerifyError> {
            if o.is_expired(now) {
                return Err(VerifyError::OrderExpired);
            }
            if sig != b"ok" {
                return Err(VerifyError::BadSignature);
            }
            // hash determinístico a partir de maker+nonce, só para teste
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
}

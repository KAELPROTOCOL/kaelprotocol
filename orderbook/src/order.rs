//! A ordem do livro, espelhando os campos assinados de `Order.sol` mais
//! `created_at` (timestamp de chegada, usado só para desempate por tempo).

use serde::{Deserialize, Serialize};

pub type Address = [u8; 20];

/// Ordem de swap cross-chain. Os 9 primeiros campos são EXATAMENTE os campos
/// assinados em EIP-712 (ver `Order.sol`); `created_at` é metadado de chegada
/// do servidor e NÃO é assinado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub maker: Address,
    pub sell_token: Address,
    pub sell_chain_id: u64,
    pub sell_amount: u128,
    pub buy_token: Address,
    pub buy_chain_id: u64,
    pub buy_amount: u128,
    pub valid_until: u64,
    pub nonce: u64,
    /// Timestamp de chegada (desempate price-time). Não assinado.
    pub created_at: u64,
}

impl Order {
    /// Uma ordem está expirada quando `valid_until < now`.
    /// `now` é PARÂMETRO — a lógica nunca lê o relógio do sistema.
    pub fn is_expired(&self, now: u64) -> bool {
        self.valid_until < now
    }
}

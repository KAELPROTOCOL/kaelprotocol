//! Representação de fio (JSON) das ordens: endereços em hex `0x…`, montantes
//! como string decimal (u128 não cabe com segurança em number JSON), assinatura
//! em hex. Converte de/para o [`Order`] interno.

use crate::order::{Address, Order};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDto {
    pub maker: String,
    pub sell_token: String,
    pub sell_chain_id: u64,
    pub sell_amount: String,
    pub buy_token: String,
    pub buy_chain_id: u64,
    pub buy_amount: String,
    pub valid_until: u64,
    pub nonce: u64,
}

#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    pub order: OrderDto,
    /// assinatura `r‖s‖v` (65 bytes) em hex `0x…`.
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub accepted: bool,
    pub order_hash: String,
}

#[derive(Debug)]
pub enum DtoError {
    BadAddress(&'static str),
    BadAmount(&'static str),
    BadHex,
}

fn parse_addr(s: &str, field: &'static str) -> Result<Address, DtoError> {
    let bytes = hex::decode(s.trim_start_matches("0x")).map_err(|_| DtoError::BadAddress(field))?;
    if bytes.len() != 20 {
        return Err(DtoError::BadAddress(field));
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&bytes);
    Ok(a)
}

fn parse_u128(s: &str, field: &'static str) -> Result<u128, DtoError> {
    s.parse::<u128>().map_err(|_| DtoError::BadAmount(field))
}

impl OrderDto {
    /// Converte para o [`Order`] interno, carimbando `created_at` (chegada).
    pub fn into_order(self, created_at: u64) -> Result<Order, DtoError> {
        Ok(Order {
            maker: parse_addr(&self.maker, "maker")?,
            sell_token: parse_addr(&self.sell_token, "sell_token")?,
            sell_chain_id: self.sell_chain_id,
            sell_amount: parse_u128(&self.sell_amount, "sell_amount")?,
            buy_token: parse_addr(&self.buy_token, "buy_token")?,
            buy_chain_id: self.buy_chain_id,
            buy_amount: parse_u128(&self.buy_amount, "buy_amount")?,
            valid_until: self.valid_until,
            nonce: self.nonce,
            created_at,
        })
    }
}

pub fn parse_signature(s: &str) -> Result<Vec<u8>, DtoError> {
    hex::decode(s.trim_start_matches("0x")).map_err(|_| DtoError::BadHex)
}

pub fn parse_address(s: &str) -> Result<Address, DtoError> {
    parse_addr(s, "address")
}

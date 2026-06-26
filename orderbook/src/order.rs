use serde::{Deserialize, Serialize};

pub type Address = [u8; 20];

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
    pub created_at: u64,
}

impl Order {
    pub fn is_expired(&self, now: u64) -> bool {
        self.valid_until < now
    }
}

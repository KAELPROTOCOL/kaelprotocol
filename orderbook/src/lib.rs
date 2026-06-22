//! Kael — livro de ordens off-chain.
//!
//! Responsabilidades estritamente separadas:
//! - [`order`]    : a ordem do livro (espelha `Order.sol` + `created_at`).
//! - [`matching`] : função pura de descoberta de pares (price-time, neutra).
//! - [`eip712`]   : verificação de assinatura na borda (equivalente ao contrato).
//! - [`book`]     : estado em memória + ingestão verificada.
//!
//! INVARIANTE: nenhum componente aqui custodia, move ou prioriza fundos. No
//! pior caso o servidor para — nunca rouba.

pub mod book;
pub mod eip712;
pub mod matching;
pub mod order;
pub mod server;
pub mod wire;

pub use order::{Address, Order};

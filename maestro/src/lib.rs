//! Kael — Maestro (Camada 4): observador off-chain.
//!
//! Observa duas chains EVM, decodifica os eventos do HTLC, correlaciona as duas
//! pernas de um swap pelo MESMO hashlock, extrai o preimage revelado e detecta
//! timeouts.
//!
//! INVARIANTE: SEM chaves, SEM custódia, SEM assinar transações de usuário.
//! Estado em memória. No pior caso, deixa de observar — nunca move fundos.
//!
//! FUNDAÇÃO: a correlação entre as duas pernas é feita pelo hashlock, derivado
//! do preimage por SHA-256 (alinhado ao contrato da Camada 1). Se o maestro
//! usasse keccak aqui, indexaria os swaps pela chave errada e nunca
//! correlacionaria — bug silencioso. Por isso há UMA única função de derivação.

pub mod correlate;
pub mod hashlock;
pub mod watcher;

pub use correlate::{Leg, LegKind, SwapTracker};
pub use hashlock::hashlock_from_preimage;

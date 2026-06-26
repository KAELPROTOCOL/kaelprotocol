//!
//! Observa duas chains EVM, decodifica os eventos do HTLC, correlaciona as duas
//! pernas de um swap pelo MESMO hashlock, extrai o preimage revelado e detecta
//! timeouts.
//!
//!

pub mod correlate;
pub mod hashlock;
pub mod watcher;

pub use correlate::{Leg, LegKind, SwapTracker};
pub use hashlock::hashlock_from_preimage;

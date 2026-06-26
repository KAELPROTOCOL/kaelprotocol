//!
//! Observes two EVM chains, decodes HTLC events, correlates both swap legs by
//! the same hashlock, extracts the revealed preimage, and detects timeouts.
//!
//!

pub mod correlate;
pub mod hashlock;
pub mod watcher;

pub use correlate::{Leg, LegKind, SwapTracker};
pub use hashlock::hashlock_from_preimage;

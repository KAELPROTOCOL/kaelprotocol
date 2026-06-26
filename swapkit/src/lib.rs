//!
//! se observou na outra chain.
//!

pub mod chain;
pub mod exec;
pub mod handshake;
pub mod sm;
pub mod verify;

pub use chain::{observed_from_swap, ChainError, ChainVerifier, RawSwap, RpcVerifier};
pub use exec::signer::{Signer, SignerError};
pub use handshake::{assign_role, derive_context, TimelockPolicy};

pub use sm::{advance, next_action, AbortReason, NextAction, SwapContext, SwapEvent, SwapState};
pub use verify::{
    verify_counterparty_leg, Address, LegExpectation, ObservedLock, Role, UnsafeReason,
    VerifyOutcome,
};

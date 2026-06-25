//! Kael — swapkit (a futura carteira/SDK).
//!
//! Na Abordagem A, a segurança do swap cross-chain vive na CARTEIRA, não no
//! contrato: antes de travar ou resgatar, cada parte verifica a perna oposta
//! on-chain. Este crate começa pelo CORAÇÃO dessa verificação — uma função
//! PURA que decide "seguro / não-seguro + razão" dado o que se espera e o que
//! se observou na outra chain.
//!
//! Sem rede, sem relógio, sem estado: a leitura real da chain virá depois,
//! atrás de uma interface, e alimentará esta lógica.

pub mod chain;
pub mod handshake;
pub mod sm;
pub mod verify;

pub use chain::{observed_from_swap, ChainError, ChainVerifier, RawSwap, RpcVerifier};
pub use handshake::{assign_role, derive_context, TimelockPolicy};

pub use sm::{advance, next_action, AbortReason, NextAction, SwapContext, SwapEvent, SwapState};
pub use verify::{
    verify_counterparty_leg, Address, LegExpectation, ObservedLock, Role, UnsafeReason,
    VerifyOutcome,
};

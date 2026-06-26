# Kael Foundation Decisions (ADRs)

This file records architecture decisions that are already reflected in code or
explicitly marked as open. ADR-002 and ADR-003 were not assigned in the available
foundation material and are intentionally left unfilled.

Non-negotiable rule: no real funds before an independent professional audit. The
current repository is experimental and unaudited.

## ADR-001 - Hashlock Uses SHA-256, Not Keccak256

Decision: HTLC hashlocks use `sha256(preimage)`, not keccak256. The internal
HTLC `contractId` still uses keccak256 because it is cheap and native to the EVM;
these are distinct uses.

Reason: the same preimage must remain verifiable by Bitcoin Script
(`OP_SHA256`) in a future Bitcoin path.

Status: implemented.

## ADR-004 - Deterministic Price-Time Matching

Decision: matching is a pure deterministic function with price-time priority:
best price, then oldest `created_at`, then lowest nonce. `now` is an input.

Reason: operator neutrality and testable behavior.

Status: implemented.

## ADR-005 - Chain-Agnostic EIP-712 Domain

Decision: the EIP-712 domain omits `chainId` and `verifyingContract`. Chain
binding lives in the signed payload through `sellChainId` and `buyChainId`.

Reason: a Kael order is inherently cross-chain and carries its own chain IDs.

Status: implemented and covered by Rust/Solidity equivalence tests.

## ADR-006 - In-Memory Orderbook For MVP

Decision: active orders live in memory for the MVP.

Reason: orders are ephemeral and signed; if the server restarts, makers can
resubmit. Funds are never held by the server.

Status: implemented for the MVP. Production persistence remains a separate
design topic.

## ADR-007 - Signature Verification At The Edge

Decision: every order is re-verified before entering the orderbook. The verifier
is behind a trait so additional signature schemes can be added later.

Reason: no order is accepted by trust in an upstream component.

Status: implemented for EIP-712.

## ADR-008 - Server Only Reports Matches

Decision: the orderbook server only reports available matches. It never executes,
modifies, prioritizes, or takes custody of funds.

Reason: non-custody and operator neutrality.

Status: implemented.

## ADR-009 - Settlement Connects Signed Orders To HTLC

Decision: `Settlement` is the order-to-HTLC entrypoint. It verifies the signed
order, consumes the nonce, receives authorized funds, and locks into the
canonical `HashedTimelock`. `HashedTimelock` and `OrderLib` remain independent
primitives.

Reason: signed order authorization must be connected to the actual HTLC lock
without mutating the HTLC primitive.

Status: implemented.

## ADR-010 - Settlement Non-Custody

Decision: `Settlement` has no arbitrary fund exit. Funds leave through HTLC
redeem to the recipient or HTLC refund to the maker.

Reason: non-custody must be a verifiable invariant.

Status: implemented and tested.

## ADR-011 - Per-Maker Nonce With Chain Binding

Decision: nonces are consumed per maker in `Settlement`. The signed payload binds
the order to its source and destination chains, so a global cross-chain nonce is
not required for the current model.

Reason: each leg is independently locked on its own chain and the signed chain
IDs bind where the leg can execute.

Status: implemented.

## ADR-012 - Only Maker Settles Their Own Leg

Decision: `settleLeg` requires `msg.sender == order.maker`.

Reason: prevents a third party from using a visible order plus ERC-20 allowance
to lock or drain tokens on behalf of the maker.

Status: implemented and tested.

## ADR-013 - Approach A: Cross-Leg Validation In The Wallet

Decision: cross-leg validation remains in wallet-side `swapkit`, not in
`Settlement`. `Settlement` validates its local signed leg; the wallet validates
the opposite leg by reading the other chain before locking or redeeming.

Reason: cross-chain validation cannot be performed trustlessly inside a
single-chain contract without introducing bridge/oracle assumptions.

Status: implemented for the local/private flow. Bridge/oracle security is not a
base assumption.

## ADR-014 - Interactive Taker/Maker Protocol

Decision: the taker generates the secret, locks first, and uses the long
timelock. The maker responds after verifying the taker leg and uses the short
timelock.

Reason: this is the standard atomic-swap sequencing that prevents secret
release before the counterparty leg is safe.

Status: implemented. Economic mitigation of taker free-option risk remains open.

## ADR-015 - Wallet Verification Of The Opposite Leg

Decision: the wallet verifies existence, hashlock, token, amount, recipient,
timelock gap, and absolute time window before locking or redeeming.

Reason: this is where cross-chain safety can be enforced without custody.

Status: implemented and tested.

## ADR-016 - RPC-Based Chain Reads For MVP

Decision: the MVP uses RPC-based observation behind the `ChainVerifier`
interface. This is trust-minimized, not trustless.

Reason: it is the pragmatic MVP boundary while preserving an upgrade path to
quorum or light-client verification.

Status: implemented for local/private audit reproduction. Multi-RPC quorum and
public-chain confirmation calibration remain mainnet-blocking gaps.

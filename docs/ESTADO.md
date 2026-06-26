# Kael - Project State

> Honest state of what exists, what has been decided but not built, and what
> remains open. Test counts are real outputs from `forge test` and
> `cargo test --workspace`, not estimates.
>
> Non-negotiable rule: no real funds before an independent professional audit.
> Everything below is experimental, unaudited, and local/closed-testnet only.

---

## 1. Built And Tested

**Total: 139 passing tests, 0 ignored** (40 Foundry + 99 Rust).

### Contracts (Foundry) - 40 tests

| Suite | Tests | Coverage |
|---|---:|---|
| `HashedTimelock.t.sol` | 9 | HTLC lock/redeem/refund, wrong preimage, double redeem, redeem after expiry, creation guards |
| `Order.t.sol` | 10 | EIP-712 valid/invalid/expired signatures and ECDSA hardening |
| `Settlement.t.sol` | 20 | Approach A settlement: authorization+nonce, chain binding, maker-only ETH/ERC-20 paths, replay, maker refund, non-custody, HTLC contractId binding for recipient/hashlock/timelock, and invalid local leg rollback |
| `Vector.t.sol` | 1 | Emits the on-chain/off-chain EIP-712 equivalence vector |

### `orderbook` (Rust) - 26 tests

- `lib` (25): pure price-time matching, EIP-712 verification equivalence with the
  contract, in-memory book, verified edge ingestion, and served price-time ranking.
- `server_integration` (1): real HTTP server with verified ingestion and match
  queries.

### `maestro` (Rust) - 9 tests

- `lib` (6): SHA-256 hashlock source, hashlock correlation, and watchdog.
- `e2e` (2): two anvils, HTLC deployment, correlated swap, captured preimage, and
  expiry watchdog.
- `full_flow` (1): capstone orderbook to settlement to maestro flow.

### `swapkit` (Rust) - 64 tests, 0 ignored

- `verify` (19): counterparty-leg verification for hashlock, token, amount,
  recipient, asymmetric role timelock gap, and the absolute `now + min_gap`
  window.
- `sm` (11): interactive taker/maker state machine, critical safety tests,
  refunds, and invalid transitions.
- `chain` (8): `Swap` to `ObservedLock` mapping, verification join, and real
  anvil integration.
- `handshake` (5): deterministic Taker/Maker role assignment and pure
  `SwapContext` derivation.
- `exec` (19): signer allowlist, `lock`/`redeem`/`refund` sends, hashlock
  observation, confirmation depth, injected clock, and anti-TOCTOU re-checks.
  Includes a direct HTLC local e2e over two anvils; the closed local runner now
  locks/refunds through `Settlement` while observing and redeeming through the
  canonical HTLC.

---

## 2. Decided, Not Built

- **Settlement order transport is peer-to-peer, not through the orderbook.** The
  role rule and `SwapContext` are transport-agnostic and already exist in
  `swapkit/src/handshake.rs`. The wallet must obtain the full counterparty order
  through a p2p channel and independently re-verify the EIP-712 signature. That
  p2p channel is not built yet.
- **Known debt: EVM recipients currently assume the same key on both chains.**
  This is acceptable for EVM-to-EVM, but Bitcoin will require an explicit
  non-EVM recipient.
- **Per-chain timelock policy calibration.** `TimelockPolicy` exists as shared
  protocol logic, but final per-chain values are not calibrated.
- **Confirmation depth against reorgs.** `ChainVerifier::observe_lock` reads
  current state. A minimum confirmation depth is required before public use.
- **Multi-node quorum.** `RpcVerifier` trusts one node. Public use needs multiple
  nodes or trustless verification.
- **Transaction liveness near expiry.** The MVP uses node-default gas estimation
  and has no RBF or fee-bump policy. That is acceptable only on deterministic
  local anvil chains.
- **Cross-chain parameter coupling.** Timelock gap, confirmation depth, and block
  time must be calibrated together per chain.

---

## 3. Open Work

- ~~**Executor.**~~ Built for the local/anvil direct HTLC scope and now used by
  the closed local Settlement runner.
- **Complete product path.** Orderbook match to p2p handshake to executor to
  chains is not fully wired.
- ~~**Real anvil integration test.**~~ Built in
  `swapkit/src/chain.rs::rpc_verifier_reads_real_chain_and_drives_wallet`; the
  remaining gap is confirmation-depth hardening.
- **Native Bitcoin.** SHA-256 keeps the path open, but Bitcoin SPV/inclusion
  verification remains separate work.
- **Liquidity incentives and maker economics.**
- **Independent professional audit.** Required before any real value.

---

## Test Count Summary

```text
Foundry  : 40 tests  (HashedTimelock 9, Order 10, Settlement 20, Vector 1)
orderbook: 26 tests  (lib 25 + integration 1)
maestro  :  9 tests  (lib 6 + e2e 2 + full_flow 1)
swapkit  : 64 tests  (verify 19 + sm 11 + chain 10 + handshake 5 + exec 19, incl. real anvil)
---------------------------------------------------------------
TOTAL    : 139 passing, 0 ignored
```

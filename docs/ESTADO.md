# Kael - Project State

> Honest state of what exists, what has been decided but not built, and what
> remains open. Test counts are real outputs from `forge test` and
> `cargo test --workspace`, not estimates.
>
> Non-negotiable rule: no real funds before an independent professional audit.
> Everything below is experimental, unaudited, and local/closed-testnet only.
> `scripts/run_private_testnet_full.sh` adds a mainnet-like private validation
> gate, but it is still local/private testnet only.

---

## 1. Built And Tested

**Total: 146 passing tests, 0 ignored** (45 Foundry + 101 Rust).

### Contracts (Foundry) - 45 tests

| Suite | Tests | Coverage |
|---|---:|---|
| `HashedTimelock.t.sol` | 12 | HTLC lock/redeem/refund, wrong preimage, double redeem, double refund, redeem after refund, refund after redeem, redeem after expiry, creation guards |
| `Order.t.sol` | 10 | EIP-712 valid/invalid/expired signatures and ECDSA hardening |
| `Settlement.t.sol` | 22 | Approach A settlement: authorization+nonce, chain binding, maker-only ETH/ERC-20 paths, replay, maker refund, non-custody, HTLC contractId binding for recipient/hashlock/timelock, invalid local leg rollback, ERC-20 EOA rejection, and insufficient allowance rejection |
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

### `swapkit` (Rust) - 66 tests, 0 ignored

- `verify` (19): counterparty-leg verification for hashlock, token, amount,
  recipient, asymmetric role timelock gap, and the absolute `now + min_gap`
  window.
- `sm` (11): interactive taker/maker state machine, critical safety tests,
  refunds, and invalid transitions.
- `chain` (8): `Swap` to `ObservedLock` mapping, verification join, and real
  anvil integration.
- `handshake` (5): deterministic Taker/Maker role assignment and pure
  `SwapContext` derivation.
- `exec` (21): signer allowlist, `lock`/`redeem`/`refund` sends, hashlock
  observation, confirmation depth, injected clock, and anti-TOCTOU re-checks.
  Includes a direct HTLC local e2e over two anvils; the closed local runner now
  locks/refunds through `Settlement` while observing and redeeming through the
  canonical HTLC, including an ERC-20 Settlement lock with exact allowance.
- `scripts/run_private_testnet_full.sh`: local/private mainnet-like gate that
  deploys HTLC, Settlement, and ERC-20 test tokens; validates chain IDs,
  bytecode, gas, balances, allowances, and confirmations; runs direct HTLC
  primitive coverage; runs native and ERC-20 Settlement swaps; and checks
  expected operational failures for missing send confirmation, EOA HTLC, EOA
  Settlement, invalid ERC-20 token, and missing cross-chain gas on both signers.

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
Foundry  : 45 tests  (HashedTimelock 12, Order 10, Settlement 22, Vector 1)
orderbook: 26 tests  (lib 25 + integration 1)
maestro  :  9 tests  (lib 6 + e2e 2 + full_flow 1)
swapkit  : 66 tests  (verify 19 + sm 11 + chain 10 + handshake 5 + exec 21, incl. real anvil)
---------------------------------------------------------------
TOTAL    : 146 passing, 0 ignored
```

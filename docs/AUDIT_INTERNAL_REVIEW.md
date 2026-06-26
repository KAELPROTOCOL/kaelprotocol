# Kael Internal Technical Review

Last updated: 2026-06-25

## Executive Summary

This review audited the current Kael repository for the local development swap milestone only. The project remains experimental and unaudited; it is not ready for mainnet, public testnet funds, or real funds.

Baseline contract and Rust tests pass. The main blocker is not an existing broken primitive, but missing orchestration: the executor has signer, tx, observe, and confirm pieces, but no `step()`/`run()` loop that drives `next_action`, re-verifies before sending, and proves the two-party wallet-led local swap end to end.

## Repository State

- Path: `/home/dev/kael`
- User: `dev`
- Branch: `fix/wallet-timelock-htlc-pricetime-hardening`
- Worktree before audit edits: clean
- Recent commits:
  - `bdef5ac executor: pecas 1-4 (signer + guard, tx, observe, confirm)`
  - `ecafa36 handshake: regra de papeis Taker/Maker + derivacao do SwapContext`
  - `2f0ede6 Harden wallet safety, settlement, and served path`
  - `30897a2 docs: consolida ADRs (DECISIONS.md) e estado do projeto (ESTADO.md)`
  - `a0045d1 swapkit: ChainVerifier (trait) + RpcVerifier (alloy) — leitura da outra chain`

## Commands Run

| Command | Result | Notes |
|---|---:|---|
| `pwd && whoami && git branch --show-current && git status --short && git log --oneline -5` | Pass | Environment confirmed. |
| `rg --files` | Pass | Structure mapped. |
| `cargo test --workspace` | Pass | 99 Rust tests passed after executor/e2e implementation. |
| `cargo fmt --all -- --check` | Pass | `rustfmt` installed and formatting applied. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass | `clippy` installed; warnings fixed. |
| `cd contracts && forge test` | Pass | 36 Foundry tests passed. |

## Components Audited

- Contracts: `contracts/src/HashedTimelock.sol`, `contracts/src/Order.sol`, `contracts/src/Settlement.sol`
- Swapkit: `swapkit/src/verify.rs`, `swapkit/src/sm.rs`, `swapkit/src/handshake.rs`, `swapkit/src/chain.rs`, `swapkit/src/exec/*`
- Orderbook: matching, in-memory book, EIP-712 verifier, HTTP integration
- Maestro: hashlock, watcher, correlation, e2e tests
- Docs: `README.md`, `docs/DECISIONS.md`, `docs/ESTADO.md`

## Findings

### Critical

No Critical findings are open from the audited code paths. Existing gates prevent acting against `Unsafe` counterparty legs at the pure state-machine level.

### High

#### KAEL-H-001: Executor loop missing

- Severity: High
- Component: `swapkit/src/exec/mod.rs`
- Description: The executor exports signer, tx, observe, and confirm modules, but has no `step()`/`run()` orchestration.
- Impact: The project could not yet reach the development milestone "wallet-driven local swap"; users still had to manually compose primitives.
- Evidence: `swapkit/src/exec/mod.rs` only declares submodules and documents piece 5 as pending.
- Test: Absent before this review.
- Recommendation: Implement `Clock`, `step()`, `run()`, idempotent state advancement, and anti-TOCTOU re-verification before lock/redeem.
- Status: fixed

#### KAEL-H-002: No local two-party wallet-led HTLC e2e

- Severity: High
- Component: `swapkit` tests / repository scripts
- Description: Existing e2e tests cover maestro correlation and primitive tx paths, but not two executors coordinating the whole wallet flow.
- Impact: The required development test cannot be run by one command.
- Evidence: No `scripts/run_dev_swap_test.sh`; no test named around two-party executor flow.
- Test: Absent before this review.
- Recommendation: Add an anvil-only e2e that deploys two HTLCs, creates two executors, and asserts lock -> lock -> redeem -> redeem.
- Status: fixed

### Medium

#### KAEL-M-001: `cargo fmt` was unavailable in current environment

- Severity: Medium
- Component: Tooling
- Description: Baseline formatting validation was blocked because the Rust toolchain lacked `rustfmt`.
- Impact: Formatting validation was initially blocked in this environment.
- Evidence: `rustup component add rustfmt clippy` installed the missing component; `cargo fmt --all -- --check` now passes.
- Test: `cargo fmt --all -- --check`.
- Recommendation: Keep `rustfmt` installed in the active toolchain.
- Status: fixed

#### KAEL-M-002: `cargo clippy` was unavailable in current environment

- Severity: Medium
- Component: Tooling
- Description: Baseline lint validation was blocked because the Rust toolchain lacked `clippy`.
- Impact: Lint validation was initially blocked in this environment.
- Evidence: `rustup component add rustfmt clippy` installed the missing component; `cargo clippy --workspace --all-targets -- -D warnings` now passes.
- Test: `cargo clippy --workspace --all-targets -- -D warnings`.
- Recommendation: Keep `clippy` installed in the active toolchain.
- Status: fixed

#### KAEL-M-003: Public-network liveness policy is intentionally absent

- Severity: Medium
- Component: Executor / protocol operations
- Description: No fee bump/RBF/liveness strategy exists for public chains near expiry.
- Impact: Acceptable for deterministic anvil; not acceptable for public testnet or real funds.
- Evidence: Documented in `docs/ESTADO.md`.
- Test: Not applicable to local milestone.
- Recommendation: Keep local/anvil guardrails; do not declare public testnet or mainnet readiness.
- Status: deferred

### Low

#### KAEL-L-001: Dead helper warning in orderbook integration test

- Severity: Low
- Component: `orderbook/tests/server_integration.rs`
- Description: `addr_hex` is unused and produces a warning during `cargo test`.
- Impact: Non-blocking for the development test, but would be promoted under stricter warning policy.
- Evidence: `warning: function addr_hex is never used`.
- Test: `cargo clippy --workspace --all-targets -- -D warnings`.
- Recommendation: Remove the unused helper when touching that test area.
- Status: fixed

### Informational

#### KAEL-I-001: Settlement was not part of the first local executor e2e

- Severity: Informational
- Component: `contracts/src/Settlement.sol`, e2e design
- Description: The first development e2e used direct HTLC native ETH as requested. Settlement is now integrated into the closed developer testnet runner while the direct HTLC e2e remains as primitive coverage.
- Impact: The real closed-testnet path now locks and refunds through Settlement and continues to observe/redeem against the canonical HTLC.
- Evidence: `Settlement.t.sol` passes independently; the closed-testnet runner now requires `KAEL_SETTLEMENT_A/B`.
- Test: `./scripts/run_dev_swap_test.sh`, `./scripts/run_closed_testnet_local.sh`, and Foundry coverage.
- Recommendation: Keep direct HTLC coverage as primitive-level testing; use the Settlement-mediated runner for closed testnet.
- Status: fixed

## Risk Matrix

| Risk | Current Control | Residual Risk |
|---|---|---|
| Secret revealed against unsafe leg | `verify_counterparty_leg` and `next_action` gates; executor re-verification added | RPC trust remains for MVP |
| Lock/redeem against shallow counterparty leg | `LockObservation::for_gate()` only returns `Confirmed` | Single RPC node trust |
| Mainnet/funds touched accidentally | signer chain allowlist | Operator must not add mainnet chain IDs |
| Replay / wrong chain in Settlement | signed chain IDs and consumed nonce | Settlement not used in first e2e |
| Reorg after observation | min confirmations interface | local test uses anvil; public calibration deferred |
| Tx liveness near expiry | none beyond anvil determinism | deferred; not public-chain safe |

## Development Test Blockers

- Missing executor loop: fixed.
- Missing local two-party executor e2e: fixed.
- Missing one-command runner and runbook: fixed.
- Missing `rustfmt`/`clippy` components in environment: fixed; both validation commands now pass.

## Correction Plan

1. Implement executor `Clock`, `step()`, `run()`, and anti-TOCTOU re-verification.
2. Add unit tests for refund via fake clock, Unsafe anti-TOCTOU, and no secret leak against Unsafe.
3. Add local two-party anvil e2e using direct ETH HTLCs and two executors.
4. Add `scripts/run_dev_swap_test.sh`.
5. Add `docs/DEV_TEST_RUNBOOK.md` and update checklist/state docs.
6. Re-run validation and commit incrementally.

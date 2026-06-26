# Kael Findings Register

Last updated: 2026-06-26

Status values:

- `fixed`: remediated and covered by validation.
- `open`: unresolved and inside current scope.
- `deferred`: unresolved because it is outside local/private scope and blocks
  mainnet or real funds.

## Critical

No Critical findings are open.

## High

| ID | Severity | Component | Impact | Evidence | Status | Test | Residual Risk |
|---|---|---|---|---|---|---|---|
| KAEL-H-001 | High | Executor | Missing orchestration could leave primitives manually composed and unsafe to operate. | Prior executor lacked full `step()`/`run()` loop. | fixed | `cargo test --workspace`, `./scripts/run_dev_swap_test.sh` | None in local/private scope. |
| KAEL-H-002 | High | Local e2e | No wallet-led two-party swap test. | No one-command runner existed before remediation. | fixed | `./scripts/run_dev_swap_test.sh` | Direct HTLC remains primitive coverage only. |
| KAEL-H-003 | High | Preflight | Cross-chain gas could be missing and fail after locks. | Both signers now checked on both RPCs. | fixed | `./scripts/run_private_testnet_full.sh` expected gas failures | Public gas volatility still deferred. |
| KAEL-H-004 | High | Broadcast | Non-contract HTLC could receive plain value transfer. | Broadcast path validates HTLC bytecode. | fixed | private runner EOA HTLC failure | Single RPC trust deferred. |
| KAEL-H-005 | High | Settlement path | Closed flow could bypass Settlement. | Closed/local runners require Settlement and lock through it. | fixed | `./scripts/run_closed_testnet_local.sh`, private runner | Direct HTLC intentionally remains primitive test coverage. |
| KAEL-H-006 | High | Settlement security | Replay or wrong-chain Settlement orders could execute. | Order chain IDs and consumed nonces are tested. | fixed | `forge test` Settlement replay/wrong-chain tests | Public operations require external audit. |
| KAEL-H-007 | High | Secret safety | Secret could be revealed against Unsafe leg. | State machine and executor re-verify before redeem. | fixed | Rust state-machine and anti-TOCTOU tests | Single RPC trust deferred. |
| KAEL-H-008 | High | Sensitive logging | Debug output could expose redeem secret. | `NextAction` redacts secret and `SwapContext` has no `Debug`. | fixed | `redeem_action_debug_redacts_secret_property` | Operational logs must keep this invariant. |

## Medium

| ID | Severity | Component | Impact | Evidence | Status | Test | Residual Risk |
|---|---|---|---|---|---|---|---|
| KAEL-M-001 | Medium | Tooling | Formatting validation was unavailable. | `rustfmt` installed and check passes. | fixed | `cargo fmt --all -- --check` | Environment must keep component installed. |
| KAEL-M-002 | Medium | Tooling | Lint validation was unavailable. | `clippy` installed and check passes. | fixed | `cargo clippy --workspace --all-targets -- -D warnings` | Environment must keep component installed. |
| KAEL-M-003 | Medium | Contract validation | Invalid ERC-20 token could be configured. | Token bytecode and contract guards reject zero/EOA tokens. | fixed | `forge test`, private runner invalid token failure | Single RPC trust deferred. |
| KAEL-M-004 | Medium | ERC-20 approvals | Excess allowance could remain after lock. | Executor approves exact amount and checks post-swap allowance. | fixed | `settlement_lock_supports_erc20_with_exact_allowance`, private runner | Non-standard ERC-20 behavior needs external audit. |
| KAEL-M-005 | Medium | Operational scripts | Swap command could be documented without required confirmation. | Docs show explicit env confirmation. | fixed | documentation review, swap-without-confirmation failure | Operator must follow runbook. |
| KAEL-M-006 | Medium | Timelock safety | Redeem near expiry could leak secret. | Verifier enforces inter-leg and absolute `now + min_gap` windows. | fixed | state-machine and verifier tests | Public calibration deferred. |
| KAEL-M-007 | Medium | Restart behavior | Re-derived observations must drive state after restart. | State machine has no persisted verified state. | fixed | property and unit tests | Durable production journal deferred. |
| KAEL-M-008 | Medium | Panic hardening | System clock before Unix epoch could panic. | Clock now falls back to zero instead of panicking. | fixed | `cargo test --workspace` | Host clock sanity remains operational concern. |
| KAEL-M-101 | Medium | Public transaction liveness | Transactions near expiry may need fee bumping. | No RBF/fee-bump policy exists. | deferred | Not applicable to private Anvil | Blocks public chains, mainnet, and real funds. |
| KAEL-M-102 | Medium | RPC trust | A faulty single RPC can mislead observation. | Local/private runners use one RPC per chain. | deferred | Not applicable to local deterministic chains | Blocks public chains without quorum/light-client strategy. |
| KAEL-M-103 | Medium | Production persistence | Crash/restart service semantics are not production-grade. | Runners are process-local audit tools. | deferred | Local rederive/property tests only | Blocks production operations. |
| KAEL-M-104 | Medium | Chain calibration | Timelock and confirmation parameters are not calibrated for public chains. | Local chain IDs use deterministic Anvil assumptions. | deferred | Local/private runners | Blocks public chains, mainnet, and real funds. |
| KAEL-M-105 | Medium | Product transport | Public p2p order transport is not built. | Closed flow injects both sides locally. | deferred | Closed/private runners | Blocks complete product launch. |

## Low

| ID | Severity | Component | Impact | Evidence | Status | Test | Residual Risk |
|---|---|---|---|---|---|---|---|
| KAEL-L-001 | Low | Tests | Unused helper warning could fail strict linting. | Helper removed. | fixed | `cargo clippy --workspace --all-targets -- -D warnings` | None. |
| KAEL-L-002 | Low | Documentation | Checklist could lag actual confirmation behavior. | Checklist shows preflight, explicit swap confirmation, and local runner separately. | fixed | documentation review | Docs require maintenance. |
| KAEL-L-003 | Low | Script operations | Local process cleanup could leave ports occupied. | Runners use traps and isolated log directories. | fixed | local/private runners | Operator can still interrupt with external signals. |
| KAEL-L-004 | Low | Language consistency | Touched code/docs contained non-English comments. | Touched files were converted to English. | fixed | targeted `rg` scan | Legacy untouched files should be checked during final packaging. |

## Informational

| ID | Severity | Component | Impact | Evidence | Status | Test | Residual Risk |
|---|---|---|---|---|---|---|---|
| KAEL-I-001 | Informational | Architecture | Direct HTLC remains useful as primitive coverage. | Settlement is primary closed flow; direct HTLC runner stage remains primitive test. | fixed | private runner direct primitive stage | None. |
| KAEL-I-002 | Informational | Audit package | External audit is still mandatory. | README and docs prohibit real funds. | fixed | documentation review | External audit may find new issues. |
| KAEL-I-003 | Informational | Mainnet | Mainnet is intentionally not enabled. | signer allowlist rejects mainnet chain IDs. | fixed | signer tests | Operators must not modify allowlist for public value. |
| KAEL-I-004 | Informational | Funds | Real funds are prohibited. | Scripts and docs label local/private test funds only. | fixed | script review | Human misuse remains out of protocol scope. |
| KAEL-I-005 | Informational | Scope | Native Bitcoin is not implemented. | SHA-256 hashlock keeps path open. | fixed | hashlock tests | Future Bitcoin work requires separate audit. |

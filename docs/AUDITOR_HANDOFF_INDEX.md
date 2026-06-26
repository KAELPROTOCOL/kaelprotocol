# Kael Auditor Handoff Index

Last updated: 2026-06-26

## Purpose

This index tells an external auditor how to reproduce the local/private
mainnet-readiness release-candidate gate. A passing gate means the repository is
ready for professional audit review. It does not mean production readiness,
mainnet readiness, or real-fund readiness.

## Auditable Scope

- EVM-to-EVM local/private atomic swap flow.
- Settlement-mediated native and ERC-20 HTLC locks.
- Direct HTLC primitive coverage as a base test.
- Wallet-side verifier, state machine, handshake, executor, preflight, and
  guarded closed/private runners.
- Solidity contracts under `contracts/src`.
- Rust crates under `swapkit`, `orderbook`, and `maestro`.
- Operational scripts under `scripts`.
- Audit-readiness documentation under `docs`.

## Out Of Scope

- Mainnet launch.
- Real funds.
- Public user funds.
- Public p2p order transport.
- Production key management.
- Native Bitcoin or non-EVM execution.
- Bridge, oracle, or custody assumptions as base security.

## Primary Reproduction Command

```bash
./scripts/run_mainnet_readiness_gate.sh
```

The gate includes the private audit reproduction command:

```bash
./scripts/run_private_testnet_full.sh
```

Logs are written to:

```text
/tmp/kael-mainnet-readiness-gate
```

## Commands The Gate Runs

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd contracts && forge test
shellcheck scripts/*.sh
./scripts/run_dev_swap_test.sh
./scripts/run_closed_testnet_local.sh
./scripts/run_private_testnet_full.sh
```

## Documents To Read First

- `docs/AUDIT_PACKAGE.md`
- `docs/ARCHITECTURE.md`
- `docs/THREAT_MODEL.md`
- `docs/SECURITY_INVARIANTS.md`
- `docs/TRUST_ASSUMPTIONS.md`
- `docs/TEST_MATRIX.md`
- `docs/INTERNAL_AUDIT_REPORT.md`
- `docs/FINDINGS_REGISTER.md`
- `docs/RISK_REGISTER.md`
- `docs/MAINNET_READINESS_GAP.md`
- `docs/MAINNET_READINESS_GATE.md`

## Main Scripts

- `scripts/run_mainnet_readiness_gate.sh`
- `scripts/run_private_testnet_full.sh`
- `scripts/run_dev_swap_test.sh`
- `scripts/run_closed_testnet_local.sh`
- `scripts/run_closed_testnet_preflight.sh`
- `scripts/run_closed_testnet_swap.sh`

## Relevant Commits

- `b476fd8` Complete mainnet-ready Settlement flow
- `93896e5` Add mainnet-ready ERC20 swap flow
- `9e23162` Add mainnet-like private testnet runner
- `a98d2fe` Add adversarial mainnet-readiness suite
- `191a676` Add fuzz and invariant test coverage
- `ca00efd` Harden runtime and operational safety
- `db1839e` Add internal audit report and risk register
- `c3485fc` Add professional audit readiness package
- `80f433e` Finalize mainnet-readiness audit gate
- `c6721d1` Finalize mainnet-readiness audit gate
- `ea0db96` Add session work report

## Known Limits

- Mainnet and real funds are out of scope.
- External professional audit is still pending.
- Multi-RPC quorum or trust-minimized public observation is not implemented.
- Public-chain fee bumping and transaction liveness are not implemented.
- Production durable execution journal and restart recovery are not implemented.
- Public p2p transport is not implemented.

## Deferred Internal Findings

Deferred Medium findings are documented in:

- `docs/FINDINGS_REGISTER.md`
- `docs/RISK_REGISTER.md`
- `docs/MAINNET_READINESS_GAP.md`

They block mainnet and real funds. They do not block local/private audit
reproduction.

## Required Interpretation

If the gate passes:

- Professional Audit Ready: YES
- Production Ready: NO
- External Audit Pending: YES

Do not interpret a pass as permission to launch mainnet or use real funds.

# Kael Checklist

Last updated: 2026-06-25

## Last Completed

- Internal technical audit created in `docs/AUDIT_INTERNAL_REVIEW.md`.
- Audit-derived implementation plan created in `docs/IMPLEMENTATION_PLAN_FROM_AUDIT.md`.
- Executor piece 5 implemented for the local EVM/anvil MVP:
  - `Clock` and `SystemClock`;
  - `step()`;
  - `run()`;
  - fresh observation before decision;
  - anti-TOCTOU re-verification before lock/redeem;
  - fake-clock refund test without real sleep.
- Local two-party direct HTLC wallet e2e added.
- Development test runner added at `scripts/run_dev_swap_test.sh`.
- Development test runbook added at `docs/DEV_TEST_RUNBOOK.md`.
- Closed developer testnet preflight added at `scripts/run_closed_testnet_preflight.sh`.
- Closed developer testnet swap runner added at `scripts/run_closed_testnet_swap.sh`.
- Closed developer testnet runbook added at `docs/CLOSED_TESTNET_RUNBOOK.md`.
- Closed developer testnet UX improved:
  - preflight lists all missing required variables at once;
  - `.env.closed-testnet.example` documents safe local defaults;
  - `scripts/run_closed_testnet_local.sh` starts two local anvils, deploys HTLCs and Settlements, runs preflight, runs swap, and cleans up.
- Closed developer testnet runner now locks and refunds through `Settlement` while observing/redeeming the canonical HTLC.
- Settlement mainnet-readiness coverage added for HTLC contractId binding of recipient/hashlock/timelock and rollback on invalid zero amount/hashlock/timelock legs.
- ERC-20 hardening added for exact allowance in the executor plus EOA-token and insufficient-allowance rejection in contracts.
- Mainnet-like private testnet runner added at `scripts/run_private_testnet_full.sh`, with native Settlement, ERC-20 Settlement, direct HTLC primitive coverage, bytecode/gas/balance/allowance checks, and expected operational failures.
- Adversarial coverage added for direct HTLC terminal-state conflicts and private-testnet operational failures covering EOA HTLC, EOA Settlement, invalid ERC-20 token, missing send confirmation, and missing cross-chain gas for each signer.
- Fuzz and property coverage added in `contracts/test/MainnetReadinessFuzz.t.sol` and `swapkit/tests/mainnet_readiness_properties.rs`.
- Defensive hardening added to redact secrets from `NextAction` debug output and remove `Debug` from `SwapContext`.
- Internal audit report, findings register, and risk register added in `docs/INTERNAL_AUDIT_REPORT.md`, `docs/FINDINGS_REGISTER.md`, and `docs/RISK_REGISTER.md`.
- Professional audit readiness package added in `docs/AUDIT_PACKAGE.md`, with architecture, threat model, invariants, assumptions, runbooks, incident response, test matrix, known limitations, and mainnet gap documents.

## Current Milestone

- Piece 5: concluded for local/anvil development scope.
- Local direct HTLC e2e: concluded as primitive coverage.
- Development runner: concluded.
- Closed testnet preflight: concluded for configuration/environment validation.
- Closed testnet Settlement-mediated swap runner: concluded for developer-only native ETH HTLC scope.
- Closed local automatic runner: available for two local Anvil chains.
- Mainnet-like private testnet runner: available for local/private audit-gate validation; not mainnet and not real funds.
- Internal audit package: available for professional audit handoff; not production approval.
- Professional audit package: available for external auditor reproduction; not production approval.

Expected local command:

```bash
./scripts/run_dev_swap_test.sh
```

Expected success marker:

```text
Development milestone reached: wallet-driven local swap through Settlement.
```

Expected closed testnet preflight command:

```bash
./scripts/run_closed_testnet_preflight.sh
```

Expected preflight marker:

```text
CLOSED TESTNET PREFLIGHT OK
```

Expected closed testnet swap command:

```bash
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh
```

Expected closed testnet swap marker:

```text
CLOSED TESTNET SWAP OK
```

Expected automatic local closed testnet command:

```bash
./scripts/run_closed_testnet_local.sh
```

Expected automatic local closed testnet marker:

```text
Closed developer testnet swap completed.
```

Expected mainnet-like private testnet command:

```bash
./scripts/run_private_testnet_full.sh
```

Expected mainnet-like private testnet marker:

```text
PRIVATE TESTNET FULL PASS
```

## Audit Findings

- Critical: 0 open.
- High: 2 fixed.
  - `KAEL-H-001`: executor loop missing.
  - `KAEL-H-002`: local two-party wallet-led e2e missing.
- Medium: 2 fixed tooling issues, 1 deferred public-network liveness issue.
  - `KAEL-M-001`: `rustfmt` installed and `cargo fmt --all -- --check` passing.
  - `KAEL-M-002`: `clippy` installed and `cargo clippy --workspace --all-targets -- -D warnings` passing.
- `KAEL-M-003`: fee/RBF/liveness policy deferred; not needed for local anvil.
- Low: 1 fixed.
  - `KAEL-L-001`: unused orderbook test helper removed.
- Informational: 1 fixed.
  - `KAEL-I-001`: Settlement integrated into the closed-testnet runner; direct HTLC remains as primitive coverage.

## Next Exact Step

Run:

```bash
./scripts/run_dev_swap_test.sh
```

For the closed developer testnet local path, run:

```bash
./scripts/run_closed_testnet_local.sh
```

For the broader local/private audit gate, run:

```bash
./scripts/run_private_testnet_full.sh
```

For broader validation, run:

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd contracts && forge test
```

## Remaining Before Any Public Funds

- Professional independent audit.
- Public-chain fee and replacement policy.
- Public-chain timelock/min-confirmation calibration per chain.
- Multi-RPC quorum or trustless light-client verification.
- Persistent crash/restart recovery beyond the simple local executor state.
- Explicit non-EVM recipient handling before Bitcoin support.
- Productized p2p/orderbook flow around the Settlement-mediated closed-testnet runner.

Rule: no mainnet, no real funds, and no weakening of the testnet/local allowlist.

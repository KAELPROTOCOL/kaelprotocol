# Kael Mainnet Readiness Gate

Last updated: 2026-06-26

## What It Is

`scripts/run_mainnet_readiness_gate.sh` is the official single-command gate for
answering whether the current Kael checkout is ready to be sent to professional
audit as a release candidate for the documented local/private EVM-to-EVM scope.

It does not launch mainnet, touch mainnet, use real funds, or declare production
readiness.

## How To Run

From the repository root:

```bash
./scripts/run_mainnet_readiness_gate.sh
```

Logs are written to:

```text
/tmp/kael-mainnet-readiness-gate
```

## What It Validates

- Git state and recent commits.
- Required dependencies.
- Rust formatting.
- Rust linting with warnings denied.
- Rust workspace tests.
- Foundry contract tests.
- Shell script static analysis.
- Development swap flow.
- Closed local/private Settlement swap flow.
- Full private mainnet-like testnet flow.
- Quick deterministic 30-node market simulation profile.
- Required audit documentation exists.
- Documentation does not claim production or real-fund readiness.
- Critical security strings and unsafe script patterns are absent.
- Auditor handoff index is present and reproducible.

## What PASS Means

A passing result means:

- Professional Audit Ready: YES
- Production Ready: NO
- External Audit Pending: YES

It means the repository can be handed to an external audit firm for review of the
documented local/private scope.

## What PASS Does Not Mean

A passing result does not mean:

- mainnet can launch;
- real funds are safe;
- public users can use the system;
- production operations are ready;
- external audit has been completed.

## Reading Logs

Each step writes a log file under:

```text
/tmp/kael-mainnet-readiness-gate
```

On failure, the script prints the failing step and the tail of the step log.

## Common Failures

- Dirty worktree: commit or intentionally discard local changes before running
  the committed gate.
- Missing dependency: install the named tool and rerun.
- Port conflict: stop local Anvil processes using the private testnet ports.
- Rust formatting failure: run `cargo fmt --all` and review the diff.
- Clippy failure: fix the warning instead of suppressing it.
- Private testnet failure: inspect `/tmp/kael-private-testnet-full`.
- Market simulation failure: inspect
  `/tmp/kael-30node-market-testnet-simulation/summary.md` and `metrics.json`.
- Documentation sanity failure: qualify any readiness statement as not
  production/mainnet ready unless external audit has completed.

## Auditor Usage

Auditors should start with:

```bash
./scripts/run_mainnet_readiness_gate.sh
```

Then read:

- `docs/AUDITOR_HANDOFF_INDEX.md`
- `docs/AUDIT_PACKAGE.md`
- `docs/ARCHITECTURE.md`
- `docs/THREAT_MODEL.md`
- `docs/SECURITY_INVARIANTS.md`
- `docs/FINDINGS_REGISTER.md`
- `docs/RISK_REGISTER.md`
- `docs/MAINNET_READINESS_GAP.md`

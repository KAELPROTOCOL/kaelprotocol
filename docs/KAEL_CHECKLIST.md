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

## Current Milestone

- Peça 5: concluded for local/anvil development scope.
- Local e2e: concluded for direct HTLC native ETH scope.
- Development runner: concluded.

Expected local command:

```bash
./scripts/run_dev_swap_test.sh
```

Expected success marker:

```text
Marco de desenvolvimento atingido: swap local rodando pela carteira.
```

## Audit Findings

- Critical: 0 open.
- High: 2 fixed.
  - `KAEL-H-001`: executor loop missing.
  - `KAEL-H-002`: local two-party wallet-led e2e missing.
- Medium: 2 open tooling issues, 1 deferred public-network liveness issue.
  - `KAEL-M-001`: `rustfmt` missing in current environment.
  - `KAEL-M-002`: `clippy` missing in current environment.
  - `KAEL-M-003`: fee/RBF/liveness policy deferred; not needed for local anvil.
- Low: 1 fixed.
  - `KAEL-L-001`: unused orderbook test helper removed.
- Informational: 1 fixed.
  - `KAEL-I-001`: Settlement intentionally out of first local executor e2e.

## Next Exact Step

Run:

```bash
./scripts/run_dev_swap_test.sh
```

Then, for broader validation in an environment with full Rust components installed, run:

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

Rule: no mainnet, no real funds, and no weakening of the testnet/local allowlist.

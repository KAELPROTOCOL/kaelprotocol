# Implementation Plan From Internal Audit

Last updated: 2026-06-26

Scope: local development swap only. Do not touch mainnet, real funds, safety allowlists, or production-readiness claims.

## Ordered Tasks

1. Executor loop
   - Add `Clock`, `SystemClock`, and fake-test support.
   - Add one-iteration `step()` that refreshes observations, calls `next_action`, and dispatches only that action.
   - Add `run()` bounded by a maximum step count and terminal states.
   - Add anti-TOCTOU re-verification immediately before lock/redeem.
   - Make repeated calls idempotent when an on-chain action already happened.

2. Executor regression tests
   - Prove lock decision is not sent if re-verification turns Unsafe.
   - Prove redeem does not transmit and secret is not revealed against Unsafe.
   - Prove fake clock expiry drives refund without real sleep.

3. Local wallet-led e2e
   - Use two local anvils.
   - Deploy `HashedTimelock` on both.
   - Use ETH native direct HTLC, not Settlement.
   - Instantiate one executor per party.
   - Drive the flow: Taker lock, Maker lock, Taker redeem, Maker learns secret, Maker redeem.
   - Assert both legs are redeemed and no refund path was taken.

4. Development runner
   - Create executable `scripts/run_dev_swap_test.sh`.
   - Check `cargo`, `forge`, and `anvil`.
   - Run the focused local development e2e.
   - Print success/failure clearly and never require real keys.

5. Documentation/checklist
   - Add `docs/DEV_TEST_RUNBOOK.md`.
   - Add or update `docs/KAEL_CHECKLIST.md`.
   - Update current project state to reflect the local development milestone only.

6. Final validation
   - Run `cargo fmt --all` if available.
   - Run `cargo test --workspace`.
   - Run `cargo clippy --workspace --all-targets -- -D warnings` if available.
   - Run `cd contracts && forge test`.
   - Run `./scripts/run_dev_swap_test.sh`.
   - Record any environment blockers exactly.

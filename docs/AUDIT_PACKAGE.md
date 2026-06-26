# Kael Professional Audit Package

Last updated: 2026-06-26

## Purpose

This package is the handoff entrypoint for an independent professional audit of
Kael's local/private EVM-to-EVM mainnet-readiness scope. It is not a production
approval, mainnet launch plan, or permission to use real funds.

## Safety Boundary

- No mainnet.
- No real funds.
- No public user funds.
- No allowlist weakening.
- No broadcast without `KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS`.
- No production or mainnet claim before external audit and remediation.

## Audit Scope

In scope:

- `contracts/src/HashedTimelock.sol`
- `contracts/src/Order.sol`
- `contracts/src/Settlement.sol`
- `contracts/test/*.t.sol`
- `swapkit/src/verify.rs`
- `swapkit/src/sm.rs`
- `swapkit/src/handshake.rs`
- `swapkit/src/chain.rs`
- `swapkit/src/exec/*`
- `swapkit/src/bin/closed-testnet-preflight.rs`
- `swapkit/src/bin/closed-testnet-swap.rs`
- `swapkit/src/bin/market-testnet-sim.rs`
- `scripts/run_dev_swap_test.sh`
- `scripts/run_closed_testnet_preflight.sh`
- `scripts/run_closed_testnet_swap.sh`
- `scripts/run_closed_testnet_local.sh`
- `scripts/run_private_testnet_full.sh`
- `scripts/run_30node_market_testnet_simulation.sh`
- documentation in `docs/`

Out of scope:

- Mainnet deployment.
- Real funds.
- Native Bitcoin execution.
- Production p2p transport.
- Bridge, oracle, or custody security assumptions.
- Token lists or public liquidity operations.

## Reproduction

From the repository root:

```bash
./scripts/run_private_testnet_full.sh
./scripts/run_30node_market_testnet_simulation.sh --quick
./scripts/run_30node_market_testnet_simulation.sh --extended
./scripts/run_dev_swap_test.sh
./scripts/run_closed_testnet_local.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd contracts && forge test && cd ..
shellcheck scripts/*.sh
```

The main private audit gate writes logs to:

```text
/tmp/kael-private-testnet-full/
/tmp/kael-30node-market-testnet-simulation/
```

Expected success marker:

```text
PRIVATE TESTNET FULL PASS
```

## Document Map

- Architecture: `docs/ARCHITECTURE.md`
- Threat model: `docs/THREAT_MODEL.md`
- Security invariants: `docs/SECURITY_INVARIANTS.md`
- Trust assumptions: `docs/TRUST_ASSUMPTIONS.md`
- Private testnet runbook: `docs/PRIVATE_TESTNET_RUNBOOK.md`
- Mainnet runbook draft: `docs/MAINNET_RUNBOOK_DRAFT.md`
- Incident response: `docs/INCIDENT_RESPONSE.md`
- Test matrix: `docs/TEST_MATRIX.md`
- Known limitations: `docs/KNOWN_LIMITATIONS.md`
- Mainnet readiness gap: `docs/MAINNET_READINESS_GAP.md`
- Internal audit report: `docs/INTERNAL_AUDIT_REPORT.md`
- Findings register: `docs/FINDINGS_REGISTER.md`
- Risk register: `docs/RISK_REGISTER.md`
- 30-node market simulation: `docs/THIRTY_NODE_MARKET_TESTNET_SIMULATION.md`

## Protected Assets

- User funds locked in HTLC contracts.
- Secret preimages.
- Signed orders and nonces.
- Wallet private keys.
- Chain observations used to decide lock, redeem, or refund.

## Review Questions

Auditors should specifically assess:

- Whether Settlement order binding is complete for maker, chain, recipient,
  token, amount, hashlock, timelock, expiry, and nonce.
- Whether ERC-20 approval handling is safe for realistic token behavior.
- Whether any path can redeem or reveal a secret while the counterparty leg is
  Unsafe.
- Whether preflight and broadcast checks can be bypassed.
- Whether scripts can accidentally touch mainnet or real funds.
- Whether local/private assumptions are clearly separated from production gaps.
- Whether the deterministic market simulation evidence covers realistic
  multi-node orderbook and Settlement behavior within the private scope.

## Current Conclusion

The repository is ready for professional audit of the stated local/private
scope. It is not ready for production or mainnet without external audit,
remediation, and the gap closures listed in `docs/MAINNET_READINESS_GAP.md`.

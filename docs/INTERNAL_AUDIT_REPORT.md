# Kael Internal Audit Report

Last updated: 2026-06-26

## Scope

This internal audit covers the repository state after the mainnet-readiness
phases for the closed/local EVM-to-EVM swap flow:

- `HashedTimelock` as the primitive lock/redeem/refund contract.
- `Settlement` as the primary closed-testnet swap path.
- Native and ERC-20 Settlement-mediated locks.
- Wallet-side state machine, verifier, handshake, executor, signer allowlist,
  preflight, and local/private runners.
- Operational scripts and documentation for local and private testnet use.

Out of scope:

- Mainnet launch.
- Real funds.
- Public p2p transport.
- Production operations.
- Native Bitcoin and non-EVM recipient support.
- External professional audit.

## Executive Summary

No open Critical findings were identified in the audited local/private
mainnet-readiness path. The current repository is ready to be handed to a
professional audit firm for review of the closed/private EVM-to-EVM scope.

The project is not production-ready and must not be used on mainnet or with real
funds. The main remaining risks are public-network operations: multi-RPC quorum,
fee bumping, calibrated timelocks, production persistence, and external audit.

## Security Posture

| Area | Status | Evidence |
|---|---:|---|
| Mainnet guardrails | Pass | signer allowlist rejects mainnet and unknown chain IDs |
| Explicit send confirmation | Pass | closed swap requires `KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS` |
| Settlement as closed flow | Pass | local and private runners lock through `Settlement` |
| Direct HTLC as primitive | Pass | direct HTLC remains covered by tests and runner primitive stage |
| ERC-20 flow | Pass | exact allowance, balance, token bytecode, and negative tests |
| Unsafe-leg handling | Pass | state machine and executor re-verify before lock/redeem |
| Secret handling | Pass | no redeem against Unsafe; debug output redacts redeem secrets |
| Cross-chain gas preflight | Pass | both signers checked on both chains |
| Invalid contract rejection | Pass | HTLC, Settlement, and ERC-20 bytecode checks before broadcast |
| Professional audit | Open | required before production or real funds |

## Validation Evidence

The following validation commands passed during the phased readiness work:

```bash
./scripts/run_dev_swap_test.sh
./scripts/run_closed_testnet_local.sh
./scripts/run_private_testnet_full.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd contracts && forge test
shellcheck scripts/*.sh
```

Current test inventory:

- Foundry: 49 tests.
- Rust workspace: 106 tests.
- Total: 155 passing tests, 0 ignored.

## Findings Summary

| Severity | Fixed | Open | Deferred |
|---|---:|---:|---:|
| Critical | 0 | 0 | 0 |
| High | 8 | 0 | 0 |
| Medium | 8 | 0 | 5 |
| Low | 4 | 0 | 0 |
| Informational | 5 | 0 | 0 |

All deferred Medium findings are public-network or production-readiness items.
They are explicitly outside the allowed local/private scope and block mainnet or
real funds until resolved and externally audited.

## Key Fixed Findings

- Settlement is now the primary closed-testnet path.
- ERC-20 Settlement locks use exact approval and reject invalid token contracts.
- Preflight validates cross-chain gas for both signers on both chains.
- Broadcast paths reject zero-address or non-contract HTLC and Settlement
  addresses before sending transactions.
- State machine and executor re-verify before lock and redeem.
- Secrets do not flow to debug output in `NextAction::RedeemCounterpartyLeg`.
- `SwapContext` does not implement `Debug` because it can contain a secret.
- Private testnet runner exercises native, Settlement, ERC-20, and expected
  operational failures.

## Open Deferred Medium Findings

The following Medium findings remain deferred with explicit justification:

- `KAEL-M-101`: public-chain fee bumping and transaction liveness are not built.
  Justification: local/private Anvil chains are deterministic; public use is
  prohibited.
- `KAEL-M-102`: multi-RPC quorum or trustless light-client verification is not
  built. Justification: single local RPC is acceptable only for private audit
  reproduction; public use is prohibited.
- `KAEL-M-103`: production crash recovery and durable execution journal are not
  built. Justification: current runners are local/private validation tools, not
  production services.
- `KAEL-M-104`: chain-specific timelock and confirmation calibration is not
  complete. Justification: local/private chain IDs and deterministic block times
  are used; public use is prohibited.
- `KAEL-M-105`: public p2p order transport is not built. Justification: closed
  testnet scripts inject both sides locally; production product flow is out of
  scope for this package.

## Auditor Handoff

An external auditor should review:

- Solidity contracts in `contracts/src`.
- Rust executor and state-machine paths in `swapkit/src`.
- Broadcast and preflight binaries in `swapkit/src/bin`.
- Operational scripts in `scripts`.
- Safety documentation and known limitations in `docs`.

The expected reproduction entrypoint is:

```bash
./scripts/run_private_testnet_full.sh
```

## Conclusion

Kael is ready for professional audit of the local/private EVM-to-EVM
mainnet-readiness package. It is not ready for production, mainnet, public
testnet value, or real funds without resolving the deferred Medium findings and
passing an independent external audit.

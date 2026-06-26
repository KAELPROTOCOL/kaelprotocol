# Kael Test Matrix

Last updated: 2026-06-26

## Required Commands

| Command | Purpose | Expected Result |
|---|---|---|
| `./scripts/run_private_testnet_full.sh` | Full local/private mainnet-like gate | `PRIVATE TESTNET FULL PASS` |
| `./scripts/run_30node_market_testnet_simulation.sh --quick` | Deterministic logical multi-node market simulation for gate/CI | `VERDICT: PASS` |
| `./scripts/run_30node_market_testnet_simulation.sh --extended` | Pre-audit logical multi-node market simulation | `VERDICT: PASS` |
| `./scripts/run_dev_swap_test.sh` | Development contract, primitive, and closed Settlement flow | success marker printed |
| `./scripts/run_closed_testnet_local.sh` | Local closed Settlement flow | closed swap completed |
| `cargo fmt --all -- --check` | Rust formatting | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lint gate | pass |
| `cargo test --workspace` | Rust unit/integration/property tests | 108 Rust tests pass |
| `cd contracts && forge test && cd ..` | Solidity unit/fuzz tests | 49 Foundry tests pass |
| `shellcheck scripts/*.sh` | shell static analysis | pass |

## Coverage Map

| Area | Covered By |
|---|---|
| HTLC native lock/redeem/refund | `HashedTimelock.t.sol`, direct HTLC local e2e |
| HTLC ERC-20 lock/redeem/refund | `HashedTimelock.t.sol` |
| Settlement native lock | `Settlement.t.sol`, closed/private runners |
| Settlement ERC-20 lock | `Settlement.t.sol`, private runner |
| Order signature validation | `Order.t.sol`, `orderbook` tests |
| Replay protection | `Settlement.t.sol` |
| Chain binding | `Settlement.t.sol`, `MainnetReadinessFuzz.t.sol` |
| Recipient/token/amount binding | Foundry tests, Rust verifier tests |
| Unsafe-leg rejection | Rust verifier, state machine, executor tests |
| Secret non-leak | state machine, executor, property tests |
| Cross-chain gas validation | private runner expected failures |
| Invalid HTLC/Settlement/token | preflight, broadcast, private runner failures |
| Confirmation depth | `exec::confirm`, `chain` tests |
| Script safety | `shellcheck`, private runner expected failures |
| 30 logical-node market simulation | `market-testnet-sim`, quick gate profile, extended/full auditor profiles |
| Price-time orderbook matching | `orderbook` tests, `market-testnet-sim` same-price and best-price checks |
| Reorg/rollback no-secret-leak | `swapkit` executor test, `market-testnet-sim` reorg metrics |
| Preflight zero transactions | `swapkit/tests/preflight_no_transactions.rs`, `market-testnet-sim` metrics |

## Test Counts

- Foundry: 49 tests.
- Rust workspace: includes the market simulation integration test.
- Total count may change as audit simulation coverage is extended; the gate is
  authoritative.

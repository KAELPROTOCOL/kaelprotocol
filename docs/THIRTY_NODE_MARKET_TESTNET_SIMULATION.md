# Kael 30-Node Market Testnet Simulation

Last updated: 2026-06-26

## Scope

This command runs a deterministic private logical testnet simulation. It does
not touch mainnet, use real funds, sign real transactions, or claim production
readiness.

```bash
./scripts/run_30node_market_testnet_simulation.sh
```

The full profile models 30 logical nodes, 60 wallets, 30 accelerated days, 100
orders per day, price-time matching, simultaneous Settlement-mediated native and
ERC-20 swap outcomes, refunds, expected failures, rollback/reorg handling, and
preflight zero-transaction evidence.

## Profiles

| Profile | Purpose | Parameters |
|---|---|---|
| `--quick` | gate and CI smoke coverage | 5 nodes, 1 wallet/node, 1 day, 10 orders/day, concurrency 2 |
| `--extended` | pre-audit evidence | 10 nodes, 2 wallets/node, 2 days, 20 orders/day, concurrency 4 |
| `--full` | prolonged auditor evidence | 30 nodes, 2 wallets/node, 30 days, 100 orders/day, concurrency 10 |

Environment variables override defaults unless a profile is selected:

```text
KAEL_SIM_NODES=30
KAEL_SIM_WALLETS_PER_NODE=2
KAEL_SIM_DAYS=30
KAEL_SIM_ORDERS_PER_DAY=100
KAEL_SIM_CONCURRENCY=10
KAEL_SIM_NATIVE_RATIO=0.5
KAEL_SIM_ERC20_RATIO=0.5
KAEL_SIM_FAILURE_RATE=0.05
KAEL_SIM_REFUND_RATE=0.02
KAEL_SIM_REORG_RATE=0.01
KAEL_SIM_SEED=1
```

The same seed produces the same nodes, wallets, orders, matches, swap outcomes,
failure records, and metrics.

## Outputs

All artifacts are written to:

```text
/tmp/kael-30node-market-testnet-simulation/
```

Generated files:

- `summary.md`
- `metrics.json`
- `nodes.jsonl`
- `wallets.jsonl`
- `orders.jsonl`
- `matches.jsonl`
- `swaps.jsonl`
- `failures.jsonl`
- `reorgs.jsonl`
- `simulation.log`

## Coverage

The simulator exercises:

- 30 logical nodes in full mode, with distinct node IDs and wallet IDs.
- Deterministic maker, taker, and mixed behavior.
- Multiple orders per simulated day.
- Crossing and non-crossing orders.
- Same-price orders and deterministic price-time tie breaks.
- Unique matches and duplicate match blocking metrics.
- Concurrent swap execution bounded by `KAEL_SIM_CONCURRENCY`.
- Native and ERC-20 swap attempts through the Settlement market flow model.
- Refund and expected-failure paths.
- Reorg/rollback evidence where a confirmed lock disappears, the executor
  reobserves, no redeem is sent, and no secret is leaked.
- Preflight zero-transaction evidence by comparing before/after counters.

## Required Verdict

The command returns success only when these status fields are `PASS`:

- `final_accounting_status`
- `orderbook_price_time_status`
- `settlement_status`
- `erc20_status`
- `preflight_zero_tx`
- `reorg_simulation_status`
- `verdict`

It also requires:

- `unexpected_failures = 0`
- `secret_leaks_detected = 0`
- `unsafe_broadcasts_detected = 0`
- `stuck_swaps = 0`

## Known Gaps

The current orderbook does not implement cancellation or partial fill. The
simulator does not invent those behaviors; they remain documented mainnet
readiness gaps.

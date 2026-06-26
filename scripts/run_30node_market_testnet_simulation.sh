#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

profile_args=()
case "${1:-}" in
  "")
    ;;
  --quick|--extended|--full)
    profile_args+=("$1")
    shift
    ;;
  --help|-h)
    cat <<'USAGE'
usage: ./scripts/run_30node_market_testnet_simulation.sh [--quick|--extended|--full]

Environment defaults:
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

Profiles:
  --quick     5 nodes, 1 wallet/node, 1 day, 10 orders/day, concurrency 2
  --extended 10 nodes, 2 wallets/node, 2 days, 20 orders/day, concurrency 4
  --full      30 nodes, 2 wallets/node, 30 days, 100 orders/day, concurrency 10

Outputs:
  /tmp/kael-30node-market-testnet-simulation/
USAGE
    exit 0
    ;;
  *)
    echo "unknown argument: $1" >&2
    exit 2
    ;;
esac

if (($# > 0)); then
  echo "unexpected extra arguments: $*" >&2
  exit 2
fi

cat <<'BANNER'
==========================================
KAEL 30-NODE MARKET TESTNET SIMULATION
==========================================
Scope: private logical testnet simulation only.
No mainnet. No real funds. No production claim.
No transaction is signed or broadcast by this command.
BANNER

cargo run -p swapkit --bin market-testnet-sim -- "${profile_args[@]}"

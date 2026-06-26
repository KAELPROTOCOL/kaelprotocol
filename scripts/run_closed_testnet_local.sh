#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="/tmp/kael-closed-testnet"
ANVIL_A_PID=""
ANVIL_B_PID=""

cleanup() {
  local status=$?
  for pid in "$ANVIL_A_PID" "$ANVIL_B_PID"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  done
  exit "$status"
}
trap cleanup EXIT INT TERM

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "FAIL missing dependency: $1" >&2
    exit 127
  fi
  echo "PASS dependency found: $1"
}

stop_old_anvil_on_port() {
  local port="$1"
  local line pid cmd
  while IFS= read -r line; do
    pid="${line%% *}"
    cmd="${line#* }"
    case "$cmd" in
      *anvil*"--port $port"*|*anvil*"--port=$port"*)
        echo "Stopping old anvil on port $port: pid=$pid"
        kill "$pid" >/dev/null 2>&1 || true
        ;;
    esac
  done < <(pgrep -af anvil || true)
}

wait_rpc() {
  local rpc="$1"
  local label="$2"
  for _ in $(seq 1 60); do
    if cast chain-id --rpc-url "$rpc" >/dev/null 2>&1; then
      echo "PASS $label RPC ready at $rpc"
      return 0
    fi
    sleep 1
  done
  echo "FAIL $label RPC did not become ready: $rpc" >&2
  return 1
}

deploy_htlc() {
  local rpc="$1"
  local key="$2"
  local label="$3"
  local log_file="$LOG_DIR/deploy-$label.log"
  local address
  if ! forge create src/HashedTimelock.sol:HashedTimelock \
    --root contracts \
    --rpc-url "$rpc" \
    --private-key "$key" \
    --broadcast \
    >"$log_file" 2>&1; then
    cat "$log_file" >&2
    return 1
  fi
  address="$(awk '/Deployed to:/ {print $3}' "$log_file" | tail -n 1)"
  if [[ -z "$address" ]]; then
    echo "FAIL could not read deployed address from $log_file" >&2
    cat "$log_file" >&2
    return 1
  fi
  echo "$address"
}

deploy_settlement() {
  local rpc="$1"
  local key="$2"
  local htlc="$3"
  local label="$4"
  local log_file="$LOG_DIR/deploy-settlement-$label.log"
  local address
  if ! forge create src/Settlement.sol:Settlement \
    --root contracts \
    --rpc-url "$rpc" \
    --private-key "$key" \
    --broadcast \
    --constructor-args "$htlc" \
    >"$log_file" 2>&1; then
    cat "$log_file" >&2
    return 1
  fi
  address="$(awk '/Deployed to:/ {print $3}' "$log_file" | tail -n 1)"
  if [[ -z "$address" ]]; then
    echo "FAIL could not read deployed address from $log_file" >&2
    cat "$log_file" >&2
    return 1
  fi
  echo "$address"
}

need cargo
need forge
need anvil
need cast

mkdir -p "$LOG_DIR"
: >"$LOG_DIR/anvil-a.log"
: >"$LOG_DIR/anvil-b.log"

stop_old_anvil_on_port 8545
stop_old_anvil_on_port 8546

anvil --host 127.0.0.1 --port 8545 --chain-id 31337 >"$LOG_DIR/anvil-a.log" 2>&1 &
ANVIL_A_PID=$!
anvil --host 127.0.0.1 --port 8546 --chain-id 31338 >"$LOG_DIR/anvil-b.log" 2>&1 &
ANVIL_B_PID=$!

export KAEL_RPC_A="http://127.0.0.1:8545"
export KAEL_RPC_B="http://127.0.0.1:8546"
export KAEL_CHAIN_A="31337"
export KAEL_CHAIN_B="31338"
export KAEL_SIGNER_KEY_A="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
export KAEL_SIGNER_KEY_B="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
export KAEL_AMOUNT_A_WEI="1000000000000000"
export KAEL_AMOUNT_B_WEI="1000000000000000"
export KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="1"
export KAEL_MIN_GAS_BALANCE_WEI="10000000000000000"
export KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI="10000000000000000"
export KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI="10000000000000000"
export KAEL_TAKER_LOCK_SECS="7200"
export KAEL_MAKER_LOCK_SECS="3600"
export KAEL_MIN_GAP_SECS="1800"
export KAEL_CLOSED_TESTNET_MAX_STEPS="120"
export KAEL_CLOSED_TESTNET_POLL_SECS="1"

wait_rpc "$KAEL_RPC_A" "chain A"
wait_rpc "$KAEL_RPC_B" "chain B"

KAEL_HTLC_A="$(deploy_htlc "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "a")"
KAEL_HTLC_B="$(deploy_htlc "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "b")"
KAEL_SETTLEMENT_A="$(deploy_settlement "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "$KAEL_HTLC_A" "a")"
KAEL_SETTLEMENT_B="$(deploy_settlement "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "$KAEL_HTLC_B" "b")"
export KAEL_HTLC_A
export KAEL_HTLC_B
export KAEL_SETTLEMENT_A
export KAEL_SETTLEMENT_B

echo "HTLC A: $KAEL_HTLC_A"
echo "HTLC B: $KAEL_HTLC_B"
echo "Settlement A: $KAEL_SETTLEMENT_A"
echo "Settlement B: $KAEL_SETTLEMENT_B"
echo "Logs: $LOG_DIR"

./scripts/run_closed_testnet_preflight.sh
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh

echo "Closed developer testnet swap completed."

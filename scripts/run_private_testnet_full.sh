#!/usr/bin/env bash
set -euo pipefail
shopt -s extglob

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="${KAEL_PRIVATE_TESTNET_LOG_DIR:-/tmp/kael-private-testnet-full}"
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

stage() {
  echo
  echo "==> $*"
}

pass() {
  echo "PASS $*"
}

fail() {
  echo "FAIL $*" >&2
  exit 1
}

uint_ge() {
  local lhs="${1##+(0)}"
  local rhs="${2##+(0)}"
  [[ -n "$lhs" ]] || lhs="0"
  [[ -n "$rhs" ]] || rhs="0"
  if ((${#lhs} > ${#rhs})); then
    return 0
  fi
  if ((${#lhs} < ${#rhs})); then
    return 1
  fi
  [[ "$lhs" > "$rhs" || "$lhs" == "$rhs" ]]
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
      pass "$label RPC ready at $rpc"
      return 0
    fi
    sleep 1
  done
  fail "$label RPC did not become ready: $rpc"
}

reject_mainnet_chain_id() {
  local chain_id="$1"
  case "$chain_id" in
    1|10|56|137|8453|42161|43114)
      fail "refusing mainnet chain id $chain_id"
      ;;
  esac
}

assert_chain_id() {
  local rpc="$1"
  local expected="$2"
  local label="$3"
  local actual
  actual="$(cast chain-id --rpc-url "$rpc")"
  reject_mainnet_chain_id "$actual"
  if [[ "$actual" != "$expected" ]]; then
    fail "$label chain id mismatch: expected $expected, got $actual"
  fi
  pass "$label chain id $actual"
}

assert_code() {
  local rpc="$1"
  local address="$2"
  local label="$3"
  local code
  code="$(cast code --rpc-url "$rpc" "$address")"
  if [[ "$code" == "0x" || -z "$code" ]]; then
    fail "$label has no bytecode at $address"
  fi
  pass "$label bytecode present at $address"
}

assert_native_balance() {
  local rpc="$1"
  local address="$2"
  local min_balance="$3"
  local label="$4"
  local balance
  balance="$(cast balance --rpc-url "$rpc" "$address")"
  balance="${balance%% *}"
  if ! uint_ge "$balance" "$min_balance"; then
    fail "$label native balance too low: balance=$balance required=$min_balance"
  fi
  pass "$label native balance $balance >= $min_balance"
}

assert_token_balance() {
  local rpc="$1"
  local token="$2"
  local owner="$3"
  local min_balance="$4"
  local label="$5"
  local balance
  balance="$(cast call --rpc-url "$rpc" "$token" "balanceOf(address)(uint256)" "$owner")"
  balance="${balance%% *}"
  if ! uint_ge "$balance" "$min_balance"; then
    fail "$label token balance too low: balance=$balance required=$min_balance"
  fi
  pass "$label token balance $balance >= $min_balance"
}

assert_allowance() {
  local rpc="$1"
  local token="$2"
  local owner="$3"
  local spender="$4"
  local expected="$5"
  local label="$6"
  local allowance
  allowance="$(cast call --rpc-url "$rpc" "$token" "allowance(address,address)(uint256)" "$owner" "$spender")"
  allowance="${allowance%% *}"
  if [[ "$allowance" != "$expected" ]]; then
    fail "$label allowance mismatch: expected=$expected actual=$allowance"
  fi
  pass "$label allowance is $expected"
}

deploy_contract() {
  local rpc="$1"
  local key="$2"
  local label="$3"
  shift 3
  local log_file="$LOG_DIR/deploy-$label.log"
  local address
  if ! forge create \
    --root contracts \
    --rpc-url "$rpc" \
    --private-key "$key" \
    --broadcast \
    "$@" \
    >"$log_file" 2>&1; then
    cat "$log_file" >&2
    return 1
  fi
  address="$(awk '/Deployed to:/ {print $3}' "$log_file" | tail -n 1)"
  if [[ -z "$address" ]]; then
    cat "$log_file" >&2
    fail "could not read deployed address from $log_file"
  fi
  echo "$address"
}

mint_token() {
  local rpc="$1"
  local key="$2"
  local token="$3"
  local to="$4"
  local amount="$5"
  local label="$6"
  local log_file="$LOG_DIR/mint-$label.log"
  if ! cast send --rpc-url "$rpc" --private-key "$key" "$token" "mint(address,uint256)" "$to" "$amount" \
    >"$log_file" 2>&1; then
    cat "$log_file" >&2
    return 1
  fi
  pass "$label minted $amount test tokens"
}

set_native_balance() {
  local rpc="$1"
  local address="$2"
  local amount="$3"
  local label="$4"
  local hex_amount
  hex_amount="$(cast to-hex "$amount")"
  cast rpc --rpc-url "$rpc" anvil_setBalance "$address" "$hex_amount" >/dev/null
  pass "$label native balance set to $amount"
}

expect_failure() {
  local label="$1"
  shift
  local log_file="$LOG_DIR/fail-${label// /-}.log"
  set +e
  "$@" >"$log_file" 2>&1
  local status=$?
  set -e
  if ((status == 0)); then
    cat "$log_file" >&2
    fail "$label unexpectedly succeeded"
  fi
  pass "$label failed as expected"
}

need cargo
need forge
need anvil
need cast

mkdir -p "$LOG_DIR"
: >"$LOG_DIR/anvil-a.log"
: >"$LOG_DIR/anvil-b.log"

export KAEL_RPC_A="${KAEL_RPC_A:-http://127.0.0.1:8545}"
export KAEL_RPC_B="${KAEL_RPC_B:-http://127.0.0.1:8546}"
export KAEL_CHAIN_A="${KAEL_CHAIN_A:-31337}"
export KAEL_CHAIN_B="${KAEL_CHAIN_B:-31338}"
export KAEL_SIGNER_KEY_A="${KAEL_SIGNER_KEY_A:-0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"
export KAEL_SIGNER_KEY_B="${KAEL_SIGNER_KEY_B:-0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d}"
export KAEL_AMOUNT_A_WEI="${KAEL_AMOUNT_A_WEI:-1000000000000000}"
export KAEL_AMOUNT_B_WEI="${KAEL_AMOUNT_B_WEI:-1000000000000000}"
export KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="${KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS:-1}"
export KAEL_MIN_GAS_BALANCE_WEI="${KAEL_MIN_GAS_BALANCE_WEI:-10000000000000000}"
export KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI="${KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI:-10000000000000000}"
export KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI="${KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI:-10000000000000000}"
export KAEL_TAKER_LOCK_SECS="${KAEL_TAKER_LOCK_SECS:-7200}"
export KAEL_MAKER_LOCK_SECS="${KAEL_MAKER_LOCK_SECS:-3600}"
export KAEL_MIN_GAP_SECS="${KAEL_MIN_GAP_SECS:-1800}"
export KAEL_CLOSED_TESTNET_MAX_STEPS="${KAEL_CLOSED_TESTNET_MAX_STEPS:-120}"
export KAEL_CLOSED_TESTNET_POLL_SECS="${KAEL_CLOSED_TESTNET_POLL_SECS:-1}"

if [[ "$KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS" == "0" ]]; then
  fail "KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS must be greater than zero"
fi

case "$KAEL_RPC_A $KAEL_RPC_B" in
  *mainnet*|*Mainnet*)
    fail "refusing RPC URL containing mainnet"
    ;;
esac

stage "Start private testnet chains"
stop_old_anvil_on_port 8545
stop_old_anvil_on_port 8546
anvil --host 127.0.0.1 --port 8545 --chain-id "$KAEL_CHAIN_A" >"$LOG_DIR/anvil-a.log" 2>&1 &
ANVIL_A_PID=$!
anvil --host 127.0.0.1 --port 8546 --chain-id "$KAEL_CHAIN_B" >"$LOG_DIR/anvil-b.log" 2>&1 &
ANVIL_B_PID=$!
wait_rpc "$KAEL_RPC_A" "chain A"
wait_rpc "$KAEL_RPC_B" "chain B"
assert_chain_id "$KAEL_RPC_A" "$KAEL_CHAIN_A" "chain A"
assert_chain_id "$KAEL_RPC_B" "$KAEL_CHAIN_B" "chain B"

SIGNER_A_ADDRESS="$(cast wallet address --private-key "$KAEL_SIGNER_KEY_A")"
SIGNER_B_ADDRESS="$(cast wallet address --private-key "$KAEL_SIGNER_KEY_B")"

stage "Deploy HTLC, Settlement, and ERC-20 test tokens"
KAEL_HTLC_A="$(deploy_contract "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "htlc-a" src/HashedTimelock.sol:HashedTimelock)"
KAEL_HTLC_B="$(deploy_contract "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "htlc-b" src/HashedTimelock.sol:HashedTimelock)"
KAEL_SETTLEMENT_A="$(deploy_contract "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "settlement-a" src/Settlement.sol:Settlement --constructor-args "$KAEL_HTLC_A")"
KAEL_SETTLEMENT_B="$(deploy_contract "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "settlement-b" src/Settlement.sol:Settlement --constructor-args "$KAEL_HTLC_B")"
TOKEN_A="$(deploy_contract "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "token-a" test/MockERC20.sol:MockERC20)"
TOKEN_B="$(deploy_contract "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "token-b" test/MockERC20.sol:MockERC20)"
export KAEL_HTLC_A KAEL_HTLC_B KAEL_SETTLEMENT_A KAEL_SETTLEMENT_B
pass "HTLC A: $KAEL_HTLC_A"
pass "HTLC B: $KAEL_HTLC_B"
pass "Settlement A: $KAEL_SETTLEMENT_A"
pass "Settlement B: $KAEL_SETTLEMENT_B"
pass "Token A: $TOKEN_A"
pass "Token B: $TOKEN_B"

assert_code "$KAEL_RPC_A" "$KAEL_HTLC_A" "HTLC A"
assert_code "$KAEL_RPC_B" "$KAEL_HTLC_B" "HTLC B"
assert_code "$KAEL_RPC_A" "$KAEL_SETTLEMENT_A" "Settlement A"
assert_code "$KAEL_RPC_B" "$KAEL_SETTLEMENT_B" "Settlement B"
assert_code "$KAEL_RPC_A" "$TOKEN_A" "Token A"
assert_code "$KAEL_RPC_B" "$TOKEN_B" "Token B"

stage "Validate gas and native balances"
assert_native_balance "$KAEL_RPC_A" "$SIGNER_A_ADDRESS" "$KAEL_MIN_GAS_BALANCE_WEI" "signer A on chain A"
assert_native_balance "$KAEL_RPC_B" "$SIGNER_A_ADDRESS" "$KAEL_MIN_GAS_BALANCE_WEI" "signer A on chain B"
assert_native_balance "$KAEL_RPC_A" "$SIGNER_B_ADDRESS" "$KAEL_MIN_GAS_BALANCE_WEI" "signer B on chain A"
assert_native_balance "$KAEL_RPC_B" "$SIGNER_B_ADDRESS" "$KAEL_MIN_GAS_BALANCE_WEI" "signer B on chain B"

stage "Run preflight without broadcasting"
export KAEL_TOKEN_A="0x0000000000000000000000000000000000000000"
export KAEL_TOKEN_B="0x0000000000000000000000000000000000000000"
./scripts/run_closed_testnet_preflight.sh

stage "Run direct HTLC native primitive test"
cargo test -p swapkit exec::tests::local_two_party_htlc_swap_e2e_wallet_driven

stage "Run Settlement native closed swap"
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh

stage "Mint ERC-20 test balances"
mint_token "$KAEL_RPC_A" "$KAEL_SIGNER_KEY_A" "$TOKEN_A" "$SIGNER_A_ADDRESS" "100000000000000000000" "token-a"
mint_token "$KAEL_RPC_B" "$KAEL_SIGNER_KEY_B" "$TOKEN_B" "$SIGNER_B_ADDRESS" "100000000000000000000" "token-b"
assert_token_balance "$KAEL_RPC_A" "$TOKEN_A" "$SIGNER_A_ADDRESS" "$KAEL_AMOUNT_A_WEI" "signer A on chain A"
assert_token_balance "$KAEL_RPC_B" "$TOKEN_B" "$SIGNER_B_ADDRESS" "$KAEL_AMOUNT_B_WEI" "signer B on chain B"
assert_allowance "$KAEL_RPC_A" "$TOKEN_A" "$SIGNER_A_ADDRESS" "$KAEL_SETTLEMENT_A" "0" "signer A to Settlement A"
assert_allowance "$KAEL_RPC_B" "$TOKEN_B" "$SIGNER_B_ADDRESS" "$KAEL_SETTLEMENT_B" "0" "signer B to Settlement B"

stage "Run ERC-20 preflight and Settlement swap"
export KAEL_TOKEN_A="$TOKEN_A"
export KAEL_TOKEN_B="$TOKEN_B"
./scripts/run_closed_testnet_preflight.sh
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh
assert_allowance "$KAEL_RPC_A" "$TOKEN_A" "$SIGNER_A_ADDRESS" "$KAEL_SETTLEMENT_A" "0" "signer A to Settlement A after swap"
assert_allowance "$KAEL_RPC_B" "$TOKEN_B" "$SIGNER_B_ADDRESS" "$KAEL_SETTLEMENT_B" "0" "signer B to Settlement B after swap"

stage "Run expected operational failures"
expect_failure "swap without explicit confirmation" ./scripts/run_closed_testnet_swap.sh
KAEL_HTLC_A="$SIGNER_A_ADDRESS" \
  expect_failure "preflight rejects EOA HTLC" ./scripts/run_closed_testnet_preflight.sh
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS \
  KAEL_SETTLEMENT_A="$SIGNER_A_ADDRESS" \
  expect_failure "swap rejects EOA Settlement" ./scripts/run_closed_testnet_swap.sh
KAEL_TOKEN_A="0x000000000000000000000000000000000000dEaD" \
  expect_failure "preflight rejects invalid token" ./scripts/run_closed_testnet_preflight.sh
set_native_balance "$KAEL_RPC_B" "$SIGNER_A_ADDRESS" "0" "signer A on chain B"
expect_failure "preflight rejects signer A missing cross-chain gas" ./scripts/run_closed_testnet_preflight.sh
set_native_balance "$KAEL_RPC_B" "$SIGNER_A_ADDRESS" "10000000000000000000000" "signer A on chain B"
set_native_balance "$KAEL_RPC_A" "$SIGNER_B_ADDRESS" "0" "signer B on chain A"
expect_failure "preflight rejects signer B missing cross-chain gas" ./scripts/run_closed_testnet_preflight.sh

echo
echo "PRIVATE TESTNET FULL PASS"
echo "Logs: $LOG_DIR"

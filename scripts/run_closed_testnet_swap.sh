#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "FAIL missing dependency: $1" >&2
    exit 127
  fi
  echo "PASS dependency found: $1"
}

need cargo

echo "Kael closed developer testnet swap"
echo "Scope: closed testnet; no mainnet; test funds only."
echo "This command can broadcast lock/redeem/refund if KAEL_CLOSED_TESTNET_SEND_TX is confirmed."
echo

expected_confirmation="I_UNDERSTAND_THIS_USES_TEST_FUNDS"
if [[ "${KAEL_CLOSED_TESTNET_SEND_TX:-}" != "$expected_confirmation" ]]; then
  echo "FAIL send refused." >&2
  echo "Set exactly:" >&2
  echo "KAEL_CLOSED_TESTNET_SEND_TX=$expected_confirmation ./scripts/run_closed_testnet_swap.sh" >&2
  echo "No transaction was sent." >&2
  exit 2
fi

required_vars=(
  KAEL_RPC_A
  KAEL_CHAIN_A
  KAEL_HTLC_A
  KAEL_SETTLEMENT_A
  KAEL_SIGNER_KEY_A
  KAEL_AMOUNT_A_WEI
  KAEL_RPC_B
  KAEL_CHAIN_B
  KAEL_HTLC_B
  KAEL_SETTLEMENT_B
  KAEL_SIGNER_KEY_B
  KAEL_AMOUNT_B_WEI
)

missing_vars=()
for var_name in "${required_vars[@]}"; do
  if [[ -z "${!var_name:-}" ]]; then
    missing_vars+=("$var_name")
  else
    echo "PASS variable set: $var_name"
  fi
done

if ((${#missing_vars[@]} > 0)); then
  echo "FAIL missing required variables:" >&2
  for var_name in "${missing_vars[@]}"; do
    echo "  - $var_name" >&2
  done
  echo "No transaction was sent." >&2
  exit 2
fi

cargo run -p swapkit --bin closed-testnet-swap

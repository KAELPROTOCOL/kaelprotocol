#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "FAIL missing dependency: $1" >&2
    return 1
  fi
  echo "PASS dependency found: $1"
}

print_example() {
  cat <<'EOF'

Safe example for two local anvils:
export KAEL_RPC_A="http://127.0.0.1:8545"
export KAEL_RPC_B="http://127.0.0.1:8546"
export KAEL_CHAIN_A="31337"
export KAEL_CHAIN_B="31338"
export KAEL_HTLC_A="0x..."
export KAEL_HTLC_B="0x..."
export KAEL_SETTLEMENT_A="0x..."
export KAEL_SETTLEMENT_B="0x..."
export KAEL_TOKEN_A="0x0000000000000000000000000000000000000000"
export KAEL_TOKEN_B="0x0000000000000000000000000000000000000000"
export KAEL_SIGNER_KEY_A="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
export KAEL_SIGNER_KEY_B="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
export KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="1"
export KAEL_MIN_GAS_BALANCE_WEI="10000000000000000"

Or run the automatic local path:
./scripts/run_closed_testnet_local.sh
EOF
}

missing=0
need cargo || missing=1

echo "Kael closed developer testnet preflight"
echo "Scope: closed testnet; no mainnet; no real funds; no transaction broadcast."
echo

required_vars=(
  KAEL_RPC_A
  KAEL_CHAIN_A
  KAEL_HTLC_A
  KAEL_SETTLEMENT_A
  KAEL_SIGNER_KEY_A
  KAEL_RPC_B
  KAEL_CHAIN_B
  KAEL_HTLC_B
  KAEL_SETTLEMENT_B
  KAEL_SIGNER_KEY_B
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
  print_example >&2
  exit 2
fi

if ((missing != 0)); then
  print_example >&2
  exit 127
fi

cargo run -p swapkit --bin closed-testnet-preflight

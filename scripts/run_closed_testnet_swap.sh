#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "FAIL dependencia ausente: $1" >&2
    exit 127
  fi
  echo "PASS dependencia encontrada: $1"
}

need cargo

echo "Kael closed developer testnet swap"
echo "Escopo: testnet fechada; sem mainnet; somente test funds."
echo "Este comando pode transmitir lock/redeem/refund se KAEL_CLOSED_TESTNET_SEND_TX estiver confirmado."
echo

expected_confirmation="I_UNDERSTAND_THIS_USES_TEST_FUNDS"
if [[ "${KAEL_CLOSED_TESTNET_SEND_TX:-}" != "$expected_confirmation" ]]; then
  echo "FAIL envio recusado." >&2
  echo "Defina exatamente:" >&2
  echo "KAEL_CLOSED_TESTNET_SEND_TX=$expected_confirmation ./scripts/run_closed_testnet_swap.sh" >&2
  echo "Nenhuma transacao foi enviada." >&2
  exit 2
fi

required_vars=(
  KAEL_RPC_A
  KAEL_CHAIN_A
  KAEL_HTLC_A
  KAEL_SIGNER_KEY_A
  KAEL_AMOUNT_A_WEI
  KAEL_RPC_B
  KAEL_CHAIN_B
  KAEL_HTLC_B
  KAEL_SIGNER_KEY_B
  KAEL_AMOUNT_B_WEI
)

missing_vars=()
for var_name in "${required_vars[@]}"; do
  if [[ -z "${!var_name:-}" ]]; then
    missing_vars+=("$var_name")
  else
    echo "PASS variavel definida: $var_name"
  fi
done

if ((${#missing_vars[@]} > 0)); then
  echo "FAIL variaveis obrigatorias ausentes:" >&2
  for var_name in "${missing_vars[@]}"; do
    echo "  - $var_name" >&2
  done
  echo "Nenhuma transacao foi enviada." >&2
  exit 2
fi

cargo run -p swapkit --bin closed-testnet-swap

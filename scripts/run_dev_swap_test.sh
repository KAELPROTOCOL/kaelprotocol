#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "erro: dependencia ausente: $1" >&2
    echo "instale $1 e rode novamente: ./scripts/run_dev_swap_test.sh" >&2
    exit 127
  fi
}

need cargo
need forge
need anvil

echo "Kael development swap test"
echo "Escopo: local/anvil apenas; sem mainnet, sem chaves reais, sem fundos reais."
echo

cargo test -p swapkit exec::tests::local_two_party_htlc_swap_e2e_wallet_driven -- --nocapture

echo
echo "Marco de desenvolvimento atingido: swap local rodando pela carteira."

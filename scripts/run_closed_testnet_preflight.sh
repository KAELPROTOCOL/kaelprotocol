#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "erro: dependencia ausente: $1" >&2
    exit 127
  fi
}

need cargo

echo "Kael closed developer testnet preflight"
echo "Escopo: testnet fechada; sem mainnet; sem fundos reais; sem envio de transacoes."
echo

cargo run -p swapkit --bin closed-testnet-preflight

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: missing dependency: $1" >&2
    echo "install $1 and run again: ./scripts/run_dev_swap_test.sh" >&2
    exit 127
  fi
}

need cargo
need forge
need anvil
need cast

echo "Kael development swap test"
echo "Scope: local/anvil only; no mainnet, no real keys, no real funds."
echo

echo "1/3 Foundry contracts"
(cd contracts && forge test)

echo
echo "2/3 Wallet-driven local direct HTLC swap"
cargo test -p swapkit exec::tests::local_two_party_htlc_swap_e2e_wallet_driven -- --nocapture

echo
echo "3/3 Local closed swap through Settlement"
./scripts/run_closed_testnet_local.sh

echo
echo "Development milestone reached: wallet-driven local swap through Settlement."

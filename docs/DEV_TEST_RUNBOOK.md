# Kael Development Swap Test Runbook

Last updated: 2026-06-25

## Scope

This runbook is only for the local development milestone. It uses local Anvil chains, deterministic test keys, direct native ETH HTLCs, and no real funds.

It does not make Kael ready for mainnet, public testnet funds, or production use.

## Prerequisites

- Rust and `cargo`
- Foundry tools: `forge` and `anvil`
- Git checkout at the Kael repository root

## Command

From the repository root:

```bash
./scripts/run_dev_swap_test.sh
```

## What The Test Does

The script first runs the Solidity contract tests:

```bash
cd contracts && forge test
```

Then it runs the focused wallet-led development e2e:

```bash
cargo test -p swapkit exec::tests::local_two_party_htlc_swap_e2e_wallet_driven -- --nocapture
```

The test:

- starts two local Anvil chains;
- deploys `HashedTimelock` on both chains;
- uses two deterministic Anvil test wallets;
- uses direct native ETH HTLCs, not `Settlement`;
- drives two wallet executors through the local flow;
- verifies Taker lock, Maker lock, Taker redeem, Maker secret extraction, and Maker redeem;
- asserts both HTLC legs are no longer active after redeem.

## Success

Success means the command exits with code `0` and prints:

```text
Marco de desenvolvimento atingido: swap local rodando pela carteira.
```

## Failure

If dependency checks fail, install the missing local tool and rerun the script.

If the Rust test fails, inspect the failing assertion printed by `cargo test`. The test is deterministic and should not require real keys, balances, RPC endpoints, or external services.

## Logs

The script prints `cargo test -- --nocapture` output directly to the terminal. Anvil instances are spawned by the Rust test harness and are local only.

## Safety Guarantee

This development test does not touch mainnet. It does not require or use real private keys. It uses only local Anvil chains and deterministic test accounts.

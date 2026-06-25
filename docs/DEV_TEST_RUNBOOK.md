# Kael Development And Closed Testnet Runbook

Last updated: 2026-06-25

## Scope

These commands are for local development and closed developer testnet only. They are not production readiness, mainnet readiness, or approval to use real funds.

Safety rules:

- no mainnet;
- no real funds;
- only local Anvil or closed developer testnet RPCs allowed by the signer allowlist;
- no removal of the `KAEL_CLOSED_TESTNET_SEND_TX` confirmation gate;
- direct HTLC/native ETH only for the closed-testnet runner;
- no `Settlement`, ERC-20, p2p, bridge, oracle, or custody in this runner.

## Prerequisites

- Rust and `cargo`
- Foundry tools: `forge`, `anvil`, and `cast`
- Git checkout at the Kael repository root

## 1. Local Development Swap Test

Command:

```bash
./scripts/run_dev_swap_test.sh
```

This is the original development milestone. It runs Foundry contract tests and the focused wallet-led Rust e2e with two local Anvil chains.

Success means the command exits with code `0` and prints:

```text
Marco de desenvolvimento atingido: swap local rodando pela carteira.
```

## 2. Closed Testnet With Developer RPCs

Use this path when two private/closed testnet RPCs and deployed HTLC addresses already exist.

Start from the example file:

```bash
cp .env.closed-testnet.example .env.closed-testnet
```

Edit the values for the closed developer testnet, then export the required variables. At minimum:

```bash
export KAEL_RPC_A="..."
export KAEL_RPC_B="..."
export KAEL_CHAIN_A="..."
export KAEL_CHAIN_B="..."
export KAEL_HTLC_A="..."
export KAEL_HTLC_B="..."
export KAEL_SIGNER_KEY_A="..."
export KAEL_SIGNER_KEY_B="..."
export KAEL_AMOUNT_A_WEI="1000000000000000"
export KAEL_AMOUNT_B_WEI="1000000000000000"
```

Run preflight first. It does not sign or broadcast transactions:

```bash
./scripts/run_closed_testnet_preflight.sh
```

Only after preflight passes, run the swap with explicit test-funds confirmation:

```bash
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh
```

The swap script refuses to run without the exact confirmation value above.

## 3. Automatic Local Closed Testnet

This is the simplest closed-testnet developer path:

```bash
./scripts/run_closed_testnet_local.sh
```

The script:

- checks `cargo`, `forge`, `anvil`, and `cast`;
- starts two local Anvil chains:
  - chain A: `127.0.0.1:8545`, chain id `31337`;
  - chain B: `127.0.0.1:8546`, chain id `31338`;
- deploys `HashedTimelock` on both chains;
- exports safe local `KAEL_*` variables with deterministic Anvil test keys;
- runs `./scripts/run_closed_testnet_preflight.sh`;
- runs the closed-testnet swap with the explicit test-funds confirmation;
- cleans up the Anvil processes on exit.

Success means it exits with code `0` and prints:

```text
Closed developer testnet swap completed.
```

Logs are written to:

```text
/tmp/kael-closed-testnet/anvil-a.log
/tmp/kael-closed-testnet/anvil-b.log
/tmp/kael-closed-testnet/deploy-a.log
/tmp/kael-closed-testnet/deploy-b.log
```

If cleanup is interrupted, stop only Anvil processes for the local ports:

```bash
pgrep -af anvil
kill <pid-for-8545-or-8546>
```

## Interpreting Failures

- Missing variables: the preflight lists all missing required variables and prints a copy/paste example.
- RPC failure: verify the RPC URL and that the private node is running.
- Chain ID failure: use only local/closed testnet chain IDs allowed by the signer guard; mainnet is refused.
- HTLC bytecode failure: deploy `HashedTimelock` on that chain and set the resulting `KAEL_HTLC_*`.
- Balance failure: fund the configured test signer with faucet/local test ETH.
- Swap confirmation failure: set `KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS` exactly.

## Current Limits

The closed-testnet runner is a developer-only direct HTLC/native ETH flow. It assumes both developer keys are available to this process and does not include `Settlement`, ERC-20 support, public p2p coordination, fee/RBF policy, multi-RPC quorum, or production restart hardening.

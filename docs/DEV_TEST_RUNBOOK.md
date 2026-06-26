# Kael Development And Closed Testnet Runbook

Last updated: 2026-06-26

## Scope

These commands are for local development and closed developer testnet only. They are not production readiness, mainnet readiness, or approval to use real funds.

Safety rules:

- no mainnet;
- no real funds;
- only local Anvil or closed developer testnet RPCs allowed by the signer allowlist;
- no removal of the `KAEL_CLOSED_TESTNET_SEND_TX` confirmation gate;
- Settlement-mediated native ETH by default for the closed-testnet runner;
- ERC-20 is supported by the transaction executor and private-testnet paths, but
  this developer runner defaults to native ETH unless `KAEL_TOKEN_A/B` are set;
- no p2p, bridge, oracle, or custody in this runner.

## Prerequisites

- Rust and `cargo`
- Foundry tools: `forge`, `anvil`, and `cast`
- Git checkout at the Kael repository root

## 1. Local Development Swap Test

Command:

```bash
./scripts/run_dev_swap_test.sh
```

This runs Foundry contract tests, the focused wallet-led direct HTLC Rust e2e, and the local closed-testnet Settlement flow.

Success means the command exits with code `0` and prints:

```text
Development milestone reached: wallet-driven local swap through Settlement.
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
export KAEL_SETTLEMENT_A="..."
export KAEL_SETTLEMENT_B="..."
export KAEL_TOKEN_A="0x0000000000000000000000000000000000000000"
export KAEL_TOKEN_B="0x0000000000000000000000000000000000000000"
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
- deploys `HashedTimelock` and `Settlement` on both chains;
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
/tmp/kael-closed-testnet/deploy-settlement-a.log
/tmp/kael-closed-testnet/deploy-settlement-b.log
```

If cleanup is interrupted, stop only Anvil processes for the local ports:

```bash
pgrep -af anvil
kill <pid-for-8545-or-8546>
```

## 4. Mainnet-Like Private Testnet Gate

Use this before audit handoff or when validating a larger local/private flow:

```bash
./scripts/run_private_testnet_full.sh
```

The script writes logs to `/tmp/kael-private-testnet-full/` and:

- starts two local private Anvil chains;
- deploys `HashedTimelock`, `Settlement`, and test `MockERC20` tokens on both chains;
- validates chain IDs, bytecode, cross-chain gas, native balances, ERC-20 balances, allowances, and confirmation settings;
- runs preflight without broadcasting;
- runs the direct HTLC native primitive test as base coverage;
- runs the guarded Settlement native swap;
- mints local test ERC-20 balances and runs the guarded Settlement ERC-20 swap;
- verifies post-swap ERC-20 allowances are zero;
- proves expected operational failures for missing send confirmation, EOA HTLC,
  EOA Settlement, invalid ERC-20 token bytecode, and missing cross-chain gas on
  both signers.

Passing output includes:

```text
PRIVATE TESTNET FULL PASS
```

## Interpreting Failures

- Missing variables: the preflight lists all missing required variables and prints a copy/paste example.
- RPC failure: verify the RPC URL and that the private node is running.
- Chain ID failure: use only local/closed testnet chain IDs allowed by the signer guard; mainnet is refused.
- HTLC bytecode failure: deploy `HashedTimelock` on that chain and set the resulting `KAEL_HTLC_*`.
- Settlement bytecode/binding failure: deploy `Settlement` with the canonical HTLC for that chain and set `KAEL_SETTLEMENT_*`.
- Balance failure: fund the configured test signer with faucet/local test ETH.
- Cross-chain gas failure: fund both configured signers on both closed-testnet chains; the preflight checks `KAEL_SIGNER_KEY_A` and `KAEL_SIGNER_KEY_B` on both RPCs before any lock.
- Gas threshold: set `KAEL_MIN_GAS_BALANCE_WEI` when the default local/testnet gas minimum is not appropriate.
- Swap confirmation failure: set `KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS` exactly.

## Current Limits

The closed-testnet runner is a developer-only Settlement-mediated HTLC flow. It defaults to native ETH and can use ERC-20 token legs when `KAEL_TOKEN_A/B` point to token contracts with sufficient test balances. The private-testnet full runner exercises both native and ERC-20 Settlement paths on local/private chains. These runners assume both developer keys are available to this process and do not include public p2p coordination, fee/RBF policy, multi-RPC quorum, or production restart hardening.

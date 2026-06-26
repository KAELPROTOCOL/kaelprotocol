# Closed Developer Testnet Runbook

Last updated: 2026-06-26

This runbook is for a closed developer testnet only. It is not production readiness, mainnet readiness, or approval to use real funds.

## Safety Scope

- Use only faucet/test funds.
- Use only chain IDs already allowed by `swapkit::exec::signer::ALLOWED_TEST_CHAINS`.
- Do not add mainnet chain IDs to the allowlist.
- Do not remove the local/testnet signer guard.
- Do not use this for public users or real value.

## Preflight Command

```bash
./scripts/run_closed_testnet_preflight.sh
```

The preflight validates configuration and chain safety. It lists all missing required variables at once and prints a local copy/paste example. It does not sign or broadcast `lock`, `redeem`, or `refund`.

## Closed Testnet Swap Command

After preflight passes, developers can run the Settlement-mediated native ETH closed-testnet swap:

```bash
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh
```

This command can broadcast `lock`, `redeem`, and `refund` transactions. It refuses to run without the exact explicit confirmation above.

## Required Environment

For local Anvil defaults, start from:

```bash
cp .env.closed-testnet.example .env.closed-testnet
```

```bash
export KAEL_RPC_A=https://...
export KAEL_CHAIN_A=11155111
export KAEL_HTLC_A=0x...
export KAEL_SETTLEMENT_A=0x...
export KAEL_SIGNER_KEY_A=...

export KAEL_RPC_B=https://...
export KAEL_CHAIN_B=11155420
export KAEL_HTLC_B=0x...
export KAEL_SETTLEMENT_B=0x...
export KAEL_SIGNER_KEY_B=...

export KAEL_AMOUNT_A_WEI=1000000000000000
export KAEL_AMOUNT_B_WEI=1000000000000000
```

Optional:

```bash
export KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS=3
export KAEL_MIN_GAS_BALANCE_WEI=10000000000000000
export KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI=10000000000000000
export KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI=10000000000000000
export KAEL_TAKER_LOCK_SECS=7200
export KAEL_MAKER_LOCK_SECS=3600
export KAEL_MIN_GAP_SECS=1800
export KAEL_CLOSED_TESTNET_MAX_STEPS=120
export KAEL_CLOSED_TESTNET_POLL_SECS=12
```

## Automatic Local Closed Testnet

For the complete local closed-testnet developer path:

```bash
./scripts/run_closed_testnet_local.sh
```

This starts two local Anvil chains (`31337` and `31338`), deploys `HashedTimelock` and `Settlement` on both, runs preflight, runs the guarded swap with test funds, writes logs to `/tmp/kael-closed-testnet/`, and cleans up Anvil processes on exit.

Passing output includes:

```text
Closed developer testnet swap completed.
```

## Mainnet-Like Private Testnet

For the broader local/private audit gate:

```bash
./scripts/run_private_testnet_full.sh
```

This starts two private local chains, deploys `HashedTimelock`, `Settlement`,
and test `MockERC20` tokens, validates bytecode, chain IDs, gas, native balances,
ERC-20 balances, allowances, and confirmations, runs preflight, runs the direct
HTLC native primitive test, runs Settlement native and Settlement ERC-20 swaps,
and verifies expected operational failures for missing send confirmation, EOA
HTLC, EOA Settlement, invalid ERC-20 token, and missing cross-chain gas on both
signers. Logs are written to `/tmp/kael-private-testnet-full/`.

Passing output includes:

```text
PRIVATE TESTNET FULL PASS
```

## What Preflight Checks

- required tools exist;
- RPC URLs are reachable;
- RPC chain IDs match `KAEL_CHAIN_A` and `KAEL_CHAIN_B`;
- both chain IDs are in the test/local allowlist;
- the two legs are on distinct chains;
- configured HTLC addresses have bytecode;
- configured Settlement addresses have bytecode and point to the configured HTLC;
- configured ERC-20 token addresses have bytecode when `KAEL_TOKEN_A/B` are nonzero;
- configured signer keys are valid;
- both configured signers have native gas on both chains before any lock is sent;
- the signer locking value on each chain also has enough balance for the configured amount when `KAEL_AMOUNT_A_WEI` / `KAEL_AMOUNT_B_WEI` are set.
- the signer locking an ERC-20 leg has enough test token balance for the configured amount.

## Passing Output

The preflight command exits with code `0` and prints:

```text
CLOSED TESTNET PREFLIGHT OK
```

The swap command exits with code `0` and prints:

```text
CLOSED TESTNET SWAP OK
```

## Remaining Limits

Closed testnet is still a developer-only milestone. The runner is Settlement-mediated HTLC, defaults to native ETH, and can use ERC-20 token legs when `KAEL_TOKEN_A/B` point to token contracts with sufficient test balances. The private-testnet full runner provides mainnet-like local/private validation but is not production readiness. It assumes both developer keys are available to this process. Before public testnet or any real funds, Kael still needs fee/RBF policy, per-chain timelock and confirmation calibration, multi-RPC quorum or trustless verification, persistence/restart hardening, and professional independent audit.

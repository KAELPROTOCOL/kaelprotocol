# Closed Developer Testnet Runbook

Last updated: 2026-06-25

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

The preflight validates configuration and chain safety. It does not sign or broadcast `lock`, `redeem`, or `refund`.

## Closed Testnet Swap Command

After preflight passes, developers can run the direct HTLC closed-testnet swap:

```bash
./scripts/run_closed_testnet_swap.sh
```

This command can broadcast `lock`, `redeem`, and `refund` transactions. It refuses to run unless this explicit confirmation is present:

```bash
export KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS
```

## Required Environment

```bash
export KAEL_RPC_A=https://...
export KAEL_CHAIN_A=11155111
export KAEL_HTLC_A=0x...
export KAEL_SIGNER_KEY_A=...

export KAEL_RPC_B=https://...
export KAEL_CHAIN_B=11155420
export KAEL_HTLC_B=0x...
export KAEL_SIGNER_KEY_B=...

export KAEL_AMOUNT_A_WEI=1000000000000000
export KAEL_AMOUNT_B_WEI=1000000000000000
```

Optional:

```bash
export KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS=3
export KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI=10000000000000000
export KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI=10000000000000000
export KAEL_TAKER_LOCK_SECS=7200
export KAEL_MAKER_LOCK_SECS=3600
export KAEL_MIN_GAP_SECS=1800
export KAEL_CLOSED_TESTNET_MAX_STEPS=120
export KAEL_CLOSED_TESTNET_POLL_SECS=12
```

## What Preflight Checks

- required tools exist;
- RPC URLs are reachable;
- RPC chain IDs match `KAEL_CHAIN_A` and `KAEL_CHAIN_B`;
- both chain IDs are in the test/local allowlist;
- the two legs are on distinct chains;
- configured HTLC addresses have bytecode;
- configured signer keys are valid;
- signer balances meet the minimum faucet/test balance threshold.

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

Closed testnet is still a developer-only milestone. The runner is direct HTLC/native ETH only and assumes both developer keys are available to this process. Before public testnet or any real funds, Kael still needs fee/RBF policy, per-chain timelock and confirmation calibration, multi-RPC quorum or trustless verification, persistence/restart hardening, and professional independent audit.

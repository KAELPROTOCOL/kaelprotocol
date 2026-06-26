# Kael Private Testnet Runbook

Last updated: 2026-06-26

## Scope

This runbook reproduces the full local/private mainnet-like audit gate. It does
not touch mainnet and must not use real funds.

## Command

From the repository root:

```bash
./scripts/run_private_testnet_full.sh
```

## What It Does

- Starts two private local Anvil chains.
- Deploys `HashedTimelock` on both chains.
- Deploys `Settlement` on both chains, each bound to its local HTLC.
- Deploys `MockERC20` test tokens on both chains.
- Validates RPC readiness and chain IDs.
- Rejects mainnet by signer allowlist.
- Validates HTLC, Settlement, and token bytecode.
- Validates cross-chain gas for both signers.
- Validates native and ERC-20 balances.
- Validates allowance behavior.
- Runs preflight without broadcasting.
- Runs direct HTLC native primitive coverage.
- Runs native Settlement swap with explicit confirmation.
- Runs ERC-20 Settlement swap with explicit confirmation.
- Verifies expected operational failures.
- Writes logs to `/tmp/kael-private-testnet-full/`.
- Cleans up local Anvil processes on exit.

## Expected Result

```text
PRIVATE TESTNET FULL PASS
```

## Logs

```text
/tmp/kael-private-testnet-full/
```

## Failure Handling

- Dependency failure: install the missing tool and rerun.
- Port collision: stop only the Anvil process on the reported local port.
- Chain ID failure: verify that the runner started both local chains.
- Bytecode failure: inspect deployment logs in the log directory.
- Gas or balance failure: inspect the balance validation section.
- Expected failure did not fail: treat as a security regression.

## Safety Notes

- Do not replace private local RPCs with mainnet RPCs.
- Do not add mainnet chain IDs to the signer allowlist.
- Do not use real private keys.
- Do not use real funds.

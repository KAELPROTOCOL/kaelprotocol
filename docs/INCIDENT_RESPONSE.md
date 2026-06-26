# Kael Incident Response

Last updated: 2026-06-26

## Scope

This document covers local/private audit and future production planning. It is
not evidence that production operations are ready.

## Severity Levels

| Severity | Description | Immediate Action |
|---|---|---|
| Critical | Real or possible loss of funds, secret leak, mainnet broadcast, or key compromise | Stop all runners, preserve logs, notify owners, start root cause analysis |
| High | Guard bypass, invalid contract accepted, unsafe redeem path, replay path | Stop affected workflow, reproduce, patch, retest |
| Medium | Operational failure that could block refund or liveness | Pause workflow, diagnose, document workaround |
| Low | Documentation, tooling, or local-only regression | Fix in normal cycle |

## Immediate Local/Private Actions

1. Stop the current script.
2. Preserve `/tmp/kael-private-testnet-full/` or `/tmp/kael-closed-testnet/`.
3. Record command, git commit, environment, and timestamps.
4. Do not retry with real funds.
5. Do not modify guards to work around the incident.
6. Open an internal finding with severity and evidence.

## Mainnet Accident Response

If any command appears to touch mainnet:

1. Stop all processes immediately.
2. Preserve logs and terminal output.
3. Identify chain ID, RPC URL, signer address, and transaction hash.
4. Rotate any potentially exposed keys.
5. Do not hide or rewrite history.
6. Publish a factual incident record before further work.

## Secret Or Key Exposure

- Treat exposed private keys as compromised.
- Treat exposed HTLC preimages as public.
- Stop any swap depending on the exposed secret.
- If safe and possible, refund or redeem according to HTLC state.
- Add regression tests for the exposure path.

## Post-Incident Requirements

- Root cause.
- Impact.
- Timeline.
- Fixed commit.
- Tests added.
- Residual risk.
- Decision on whether external audit scope must expand.

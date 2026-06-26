# Kael Mainnet Readiness Gap

Last updated: 2026-06-26

## Current Position

Kael is ready for professional audit of the local/private EVM-to-EVM
mainnet-readiness package. It is not ready for production or mainnet.

## Blocking Gaps Before Real Mainnet

| Gap | Severity | Required Work |
|---|---|---|
| External professional audit | Critical | Complete audit, remediate findings, publish final report. |
| Public chain observation | High | Add multi-RPC quorum or trust-minimized verification. |
| Transaction liveness | High | Add fee bumping, stuck transaction handling, and expiry playbooks. |
| Timelock calibration | High | Calibrate per chain for block time, finality, confirmations, and gas volatility. |
| Production persistence | High | Add durable swap journal, restart recovery, and idempotent resend policy. |
| Production p2p transport | High | Build and audit authenticated public order exchange. |
| Production key management | High | Define storage, rotation, signing, and operator procedures. |
| Dependency review | Medium | Audit Rust, Solidity, Foundry, and script dependencies. |
| Non-standard ERC-20 behavior | Medium | Test and document supported token behavior. |
| Incident drills | Medium | Run production-like incident response exercises. |
| Native Bitcoin | Deferred | Separate design, implementation, and audit required. |

## Non-Negotiable Pre-Mainnet Rule

Do not add mainnet chain IDs to the signer allowlist until all High gaps are
closed, an external audit is complete, and project owners explicitly approve a
separate launch decision.

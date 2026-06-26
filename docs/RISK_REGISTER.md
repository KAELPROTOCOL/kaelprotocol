# Kael Risk Register

Last updated: 2026-06-26

## Scope

This register covers the local/private EVM-to-EVM audit-readiness package. It
does not approve mainnet, public testnet value, or real funds.

## Risk Summary

| ID | Risk | Likelihood | Impact | Current Controls | Status | Residual Risk |
|---|---|---:|---:|---|---|---|
| R-001 | Accidental mainnet signing | Low | Critical | signer allowlist, local/private scripts, docs prohibition | controlled | Requires preserving allowlist and no mainnet configuration. |
| R-002 | Real funds used before audit | Medium | Critical | explicit docs, local/private env examples, confirmation string | controlled | Human operator risk remains; production launch must require external audit. |
| R-003 | Secret leak against Unsafe leg | Low | Critical | verifier gates, state-machine safety tests, executor re-verification | controlled | Single RPC trust can still mislead observations. |
| R-004 | Invalid HTLC address accepted | Low | High | preflight and broadcast bytecode checks | controlled | Single RPC trust. |
| R-005 | Invalid Settlement address accepted | Low | High | preflight and broadcast bytecode checks | controlled | Single RPC trust. |
| R-006 | Invalid ERC-20 token accepted | Low | High | token bytecode checks and contract-level token-code guards | controlled | Non-standard token behavior requires audit before public use. |
| R-007 | Excess ERC-20 allowance | Low | Medium | exact approval and allowance checks | controlled | Non-standard approval semantics need external audit. |
| R-008 | Missing cross-chain gas | Low | High | preflight checks both signers on both chains | controlled | Public fee volatility deferred. |
| R-009 | Replay or wrong-chain Settlement order | Low | High | signed chain IDs and consumed nonce | controlled | External audit required. |
| R-010 | Wrong recipient/token/amount lock | Low | High | signed order binding and HTLC contractId binding tests | controlled | External audit required. |
| R-011 | Reorg after observation | Medium | High | minimum confirmation parameter and local confirmation checks | deferred | Needs public-chain calibration and multi-RPC strategy. |
| R-012 | Transaction stuck near expiry | Medium | High | local deterministic chains only | deferred | Needs fee bumping and liveness policy. |
| R-013 | Single RPC lies or malfunctions | Medium | High | local/private deterministic RPC only | deferred | Needs quorum or light-client verification. |
| R-014 | Process crash during live swap | Medium | High | state can be re-derived from chain observations in tests | deferred | Needs durable production journal and recovery runbook validation. |
| R-015 | Documentation diverges from scripts | Medium | Medium | checklist and runbooks updated per phase | controlled | Final packaging must keep docs synchronized. |
| R-016 | Logs expose sensitive data | Low | High | `NextAction` redacts secrets and `SwapContext` has no `Debug` | controlled | Future logging must preserve redaction. |
| R-017 | Unsafe direct HTLC bypass | Low | High | direct HTLC documented as primitive coverage; closed flow uses Settlement | controlled | Future product code must not use primitive as primary swap path. |
| R-018 | Unreviewed dependency or toolchain drift | Medium | Medium | reproducible commands documented | open-for-audit | Professional audit should include dependency review. |
| R-019 | Mainnet readiness overstated | Medium | High | docs state not production/mainnet-ready without audit | controlled | Marketing/product docs must retain this boundary. |
| R-020 | Native Bitcoin assumptions incomplete | Medium | Medium | SHA-256 hashlock invariant only | deferred | Native Bitcoin requires separate design and audit. |

## Launch Gates

The following gates must remain closed until resolved:

- No mainnet deployment.
- No real funds.
- No public value testnet.
- No allowlist weakening.
- No send path without explicit confirmation.
- No production claim before independent professional audit.

## Required Before Real Mainnet

- Independent professional audit with remediation.
- Multi-RPC quorum or trust-minimized chain observation.
- Fee bumping and liveness policy.
- Chain-specific timelock and confirmation calibration.
- Durable execution journal and restart recovery.
- Production incident response exercise.
- Dependency and supply-chain review.
- Separate audit for any native Bitcoin or non-EVM expansion.

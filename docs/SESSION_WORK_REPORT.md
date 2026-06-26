# Kael Session Work Report

Date: 2026-06-26

## Verdict

The mainnet-readiness audit package was completed for professional audit
handoff of the local/private EVM-to-EVM scope.

Kael is ready for professional audit of the documented local/private scope.
Kael is not ready for production, mainnet, public funds, or real funds without
external audit and the documented gap closures.

## Non-Negotiable Constraints Preserved

- No mainnet was touched.
- No real funds were used.
- Mainnet chain IDs remain rejected by the signer allowlist.
- The explicit send confirmation remains required:
  `KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS`.
- No force push was performed.
- No merge to `main` was performed.
- No prohibited commit trailers, external noreply author markers, or generated-assistant signatures were added.
- Commit author and committer remained `kael <>`.
- Code and documentation were brought to English-only content based on the final repository scan.

## Phase Summary

| Phase | Status | Commit |
|---|---:|---|
| Phase 0 - Baseline | PASS | no commit |
| Phase 1 - Settlement mainnet-ready | PASS | `b476fd8` |
| Phase 2 - ERC-20 mainnet-ready | PASS | `93896e5` |
| Phase 3 - Private testnet mainnet-like | PASS | `9e23162` |
| Phase 4 - Adversarial suite | PASS | `a98d2fe` |
| Phase 5 - Fuzz, property, and invariant tests | PASS | `191a676` |
| Phase 6 - Defensive hardening | PASS | `ca00efd` |
| Phase 7 - Internal audit report and risk register | PASS | `db1839e` |
| Phase 8 - Professional audit readiness package | PASS | `c3485fc` |
| Phase 9 - Final mainnet-readiness audit gate | PASS | `80f433e`, `c6721d1` |

## Commit List

- `b476fd8` Complete mainnet-ready Settlement flow
- `93896e5` Add mainnet-ready ERC20 swap flow
- `9e23162` Add mainnet-like private testnet runner
- `a98d2fe` Add adversarial mainnet-readiness suite
- `191a676` Add fuzz and invariant test coverage
- `ca00efd` Harden runtime and operational safety
- `db1839e` Add internal audit report and risk register
- `c3485fc` Add professional audit readiness package
- `80f433e` Finalize mainnet-readiness audit gate
- `c6721d1` Finalize mainnet-readiness audit gate

## Main Work Completed

### Settlement Flow

- Made Settlement the primary closed/private swap path.
- Kept direct HTLC as primitive/base coverage.
- Preserved Approach A: cross-leg validation remains in the wallet, not in a bridge/oracle/custodial mechanism.
- Added/confirmed maker-only Settlement behavior.
- Added/confirmed replay protection through consumed nonces.
- Added/confirmed chain binding in signed orders.
- Added/confirmed recipient, token, amount, hashlock, and timelock binding.
- Added validation that Settlement and HTLC addresses are contracts before broadcast.

### ERC-20 Flow

- Added ERC-20 support in the real Settlement-mediated path.
- Added local/mock ERC-20 token deployment for private testnet.
- Added test minting for private/local flows.
- Added exact approval handling.
- Added post-swap allowance checks.
- Added positive and negative tests around ERC-20 Settlement locks.
- Added invalid token rejection for zero/EOA/non-contract token addresses.
- Added insufficient allowance and invalid amount coverage.

### Private Testnet Mainnet-Like Runner

- Added `scripts/run_private_testnet_full.sh`.
- Added `.env.private-testnet.example`.
- Runner starts two local/private chains.
- Runner deploys HTLC, Settlement, and mock ERC-20 tokens.
- Runner validates:
  - dependencies;
  - RPC readiness;
  - chain IDs;
  - no mainnet;
  - bytecode for HTLC/Settlement/tokens;
  - Settlement-to-HTLC binding;
  - native gas balances;
  - cross-chain gas for both signers;
  - ERC-20 balances;
  - ERC-20 allowances;
  - confirmation settings.
- Runner executes:
  - preflight without broadcasting;
  - direct HTLC primitive coverage;
  - native Settlement swap;
  - ERC-20 Settlement swap;
  - expected operational failures.
- Logs are written to `/tmp/kael-private-testnet-full/`.

### Adversarial Coverage

Added/confirmed coverage for:

- no secret reveal against Unsafe legs;
- redeem blocked when re-verification changes current action;
- refund before timelock fails;
- refund after timelock passes;
- redeem after refund fails;
- refund after redeem fails;
- double redeem fails;
- double refund fails;
- shallow confirmations do not pass the gate;
- configured minimum confirmations are respected;
- restart/rederive assumptions are documented and partially covered by state re-derivation tests;
- signer A without gas on chain B fails preflight;
- signer B without gas on chain A fails preflight;
- zero/EOA HTLC rejected;
- zero/EOA Settlement rejected;
- zero/EOA ERC-20 token rejected;
- wrong signer fails;
- invalid signature fails;
- wrong chain fails;
- wrong token fails;
- wrong amount fails;
- wrong recipient fails;
- wrong hashlock fails;
- unsafe timelock gaps fail;
- swap without explicit confirmation fails.

### Fuzz, Property, And Invariant Coverage

Added Foundry fuzz coverage:

- HTLC contract ID binds fields.
- HTLC rejects wrong preimage.
- Order hash binds amounts, tokens, chains, and nonce.
- Settlement native leg locks exact signed amount.

Added Rust property-style tests:

- verifier rejects single-field mutations;
- state machine never redeems the secret against Unsafe counterparty;
- maker never locks against Unsafe counterparty;
- handshake roles remain complementary for arrival/digest ties;
- debug output redacts redeem secrets.

### Defensive Hardening

- Removed `Debug` from `SwapContext` because it can contain a secret.
- Implemented redacted `Debug` for `NextAction::RedeemCounterpartyLeg`.
- Added a property test proving secret redaction in debug output.
- Avoided a panic path in `SystemClock::now`.
- Reviewed for mainnet accidental paths, real-fund use, unsafe broadcasts, missing confirmation, and secret logging.
- Cleaned code/docs language to English.

### Audit Documentation

Created:

- `docs/INTERNAL_AUDIT_REPORT.md`
- `docs/FINDINGS_REGISTER.md`
- `docs/RISK_REGISTER.md`
- `docs/AUDIT_PACKAGE.md`
- `docs/ARCHITECTURE.md`
- `docs/THREAT_MODEL.md`
- `docs/SECURITY_INVARIANTS.md`
- `docs/TRUST_ASSUMPTIONS.md`
- `docs/PRIVATE_TESTNET_RUNBOOK.md`
- `docs/MAINNET_RUNBOOK_DRAFT.md`
- `docs/INCIDENT_RESPONSE.md`
- `docs/TEST_MATRIX.md`
- `docs/KNOWN_LIMITATIONS.md`
- `docs/MAINNET_READINESS_GAP.md`
- `docs/SESSION_WORK_REPORT.md`

Updated:

- `README.md`
- `docs/KAEL_CHECKLIST.md`
- `docs/ESTADO.md`
- `docs/DEV_TEST_RUNBOOK.md`
- `docs/CLOSED_TESTNET_RUNBOOK.md`
- `docs/DECISIONS.md`
- `docs/AUDIT_INTERNAL_REVIEW.md`
- `docs/IMPLEMENTATION_PLAN_FROM_AUDIT.md`

## Final Validation Commands

The following commands passed:

```bash
./scripts/run_private_testnet_full.sh
./scripts/run_dev_swap_test.sh
./scripts/run_closed_testnet_local.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd contracts && forge test && cd ..
shellcheck scripts/*.sh
git status --short
git log --oneline -15
```

## Test Results

- `./scripts/run_private_testnet_full.sh`: PASS
- `./scripts/run_dev_swap_test.sh`: PASS
- `./scripts/run_closed_testnet_local.sh`: PASS
- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS
- `cargo test --workspace`: PASS, 106 Rust tests
- `cd contracts && forge test && cd ..`: PASS, 49 Foundry tests
- `shellcheck scripts/*.sh`: PASS
- `git status --short`: clean

Total reported test inventory:

- Foundry: 49 tests.
- Rust workspace: 106 tests.
- Total: 155 passing tests, 0 ignored.

## Final Local Review Result

Reviewed for:

- accidental mainnet path;
- real-fund usage;
- secret leakage;
- lock/redeem against Unsafe;
- dangerous ERC-20 approval behavior;
- Settlement bypass in closed/private flow;
- missing preflight;
- documentation claiming production readiness;
- dangerous scripts;
- private key or secret logging;
- open Critical/High/Medium findings.

Result:

- No Critical findings open.
- No High findings open.
- No Medium findings open inside the local/private audit scope.
- Deferred Medium findings are documented as mainnet blockers, not local/private blockers.

## Open Deferred Medium Findings

The following remain intentionally deferred and block real mainnet:

- Public-chain transaction liveness and fee bumping.
- Multi-RPC quorum or trust-minimized chain observation.
- Production durable execution journal and restart recovery.
- Chain-specific timelock and confirmation calibration.
- Public p2p order transport.

## Remaining Gaps Before Real Mainnet

- Independent professional audit.
- Audit remediation.
- Multi-RPC quorum or trust-minimized verification.
- Fee bumping and transaction liveness policy.
- Chain-specific timelock and confirmation calibration.
- Durable production execution journal.
- Production key management.
- Public p2p transport and review.
- Dependency and supply-chain review.
- Non-standard ERC-20 behavior review.
- Incident response exercise.
- Separate audit for any native Bitcoin/non-EVM expansion.

## Final State

- Professional audit package: ready for external auditor reproduction.
- Local/private EVM-to-EVM mainnet-readiness scope: complete.
- Production/mainnet readiness: not complete.
- Mainnet: not launched and not touched.
- Real funds: not used.
- Worktree at final check: clean.

## Copy Command

```bash
xclip -selection clipboard < docs/SESSION_WORK_REPORT.md
```

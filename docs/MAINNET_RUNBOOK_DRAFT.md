# Kael Mainnet Runbook Draft

Last updated: 2026-06-26

## Status

Draft only. This is not a launch runbook and must not be used to deploy or run
mainnet. It documents the gates that must exist before a real mainnet launch can
be considered.

## Mandatory Pre-Launch Gates

- Independent professional audit completed.
- All Critical, High, and Medium audit findings fixed or explicitly accepted by
  project owners with documented rationale.
- Multi-RPC quorum or trust-minimized observation implemented.
- Chain-specific timelock and confirmation values calibrated.
- Fee bumping and transaction liveness policy implemented.
- Durable execution journal and restart recovery tested.
- Production key management defined and reviewed.
- Incident response exercise completed.
- Dependency and supply-chain review completed.
- Public p2p transport reviewed and tested.

## Prohibited Until Gates Pass

- Mainnet deployment.
- Mainnet signer allowlist entries.
- Real funds.
- Public user funds.
- Production-ready claims.

## Draft Launch Sequence

1. Freeze audited code.
2. Verify reproducible builds and dependency lockfiles.
3. Run full local/private test matrix.
4. Run staging/private testnet dry run.
5. Review all open findings and risk acceptances.
6. Prepare rollback and pause procedures.
7. Obtain external sign-off.
8. Only then consider a separate mainnet launch decision.

## Abort Criteria

- Any Critical or High finding unresolved.
- Any Medium finding unresolved without signed risk acceptance.
- Any failed private testnet gate.
- Any unexplained secret/key exposure.
- Any mainnet allowlist change before formal launch approval.

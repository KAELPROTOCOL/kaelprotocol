# Kael Known Limitations

Last updated: 2026-06-26

## Current Limits

- Not audited by an independent professional auditor.
- Not production-ready.
- Not mainnet-ready for real funds.
- No public p2p order transport.
- No multi-RPC quorum or trustless chain observation.
- No public-chain fee bumping or RBF policy.
- No production durable execution journal.
- No production key-management system.
- Chain-specific timelock and confirmation parameters are not calibrated for
  public networks.
- Native Bitcoin is not implemented.
- Non-EVM recipient handling is not implemented.
- Non-standard public ERC-20 behavior requires further review.

## Intentional Local/Private Constraints

- Signer allowlist is restricted to local/closed test chains.
- Broadcast requires explicit test-funds confirmation.
- Runners use local/private Anvil chains and test keys.
- Direct HTLC remains primitive coverage, not the primary closed swap path.
- Settlement is the primary closed/private swap path.

## Not A Bug

These are expected boundaries, not defects:

- Refusing mainnet chain IDs.
- Refusing to broadcast without explicit confirmation.
- Refusing zero-address or EOA HTLC/Settlement/token addresses.
- Failing preflight when either signer lacks cross-chain gas.
- Blocking public funds until external audit and production gaps are closed.

# Kael Trust Assumptions

Last updated: 2026-06-26

## Local/Private Audit Scope Assumptions

- Chains are local/private Anvil-style networks.
- Chain IDs are non-mainnet and allowed by the signer guard.
- Test keys and test funds are used.
- RPC nodes are controlled by the operator for reproduction.
- Both developer signers are available to the local runner.
- `forge`, `anvil`, `cast`, `cargo`, and `shellcheck` are installed.
- Environment variables are supplied by the operator and not treated as secrets
  safe for production.

## Protocol Assumptions

- SHA-256 preimage resistance holds.
- ECDSA signatures are valid only for the expected maker and payload.
- The EVM execution environment follows standard contract semantics.
- ERC-20 tokens in the private runner are controlled test mocks.
- Non-standard public ERC-20 behavior requires separate review.

## Not Assumed

- No bridge honesty.
- No oracle honesty.
- No custodial server honesty.
- No orderbook authority over funds.
- No public RPC honesty for mainnet.
- No production crash tolerance beyond documented local/private tests.

## Mainnet-Blocking Assumptions

The following assumptions are acceptable for local/private audit reproduction but
not for mainnet:

- single RPC per chain;
- deterministic local gas conditions;
- local process state;
- private test keys;
- no public p2p adversarial network;
- no production key-management system.

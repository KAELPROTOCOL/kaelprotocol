# Kael Threat Model

Last updated: 2026-06-26

## Scope

This model covers the local/private EVM-to-EVM Settlement-mediated HTLC flow. It
does not cover mainnet production, public liquidity, native Bitcoin, or p2p
transport.

## Assets

- Locked native or ERC-20 funds.
- Secret preimage.
- Private keys.
- Signed orders and nonces.
- Chain observations used for safety decisions.

## Trust Boundaries

- Wallet process to RPC node.
- Wallet process to local environment variables.
- Wallet process to contracts.
- Settlement contract to canonical HTLC.
- Scripts to local Anvil processes and generated logs.

## Adversaries

- Counterparty submitting malformed or stale legs.
- Counterparty attempting replay, wrong chain, wrong recipient, wrong token, or
  wrong amount execution.
- Counterparty trying to learn the secret while their leg is Unsafe.
- Misconfigured operator using wrong contracts, wrong RPCs, or missing gas.
- Malfunctioning or dishonest RPC node.
- Future developer accidentally weakening guards.

## Primary Threats And Controls

| Threat | Control |
|---|---|
| Mainnet accidental use | signer allowlist rejects mainnet and unknown chain IDs |
| Broadcast without consent | exact `KAEL_CLOSED_TESTNET_SEND_TX` gate |
| Invalid HTLC or Settlement address | preflight and broadcast bytecode validation |
| Invalid ERC-20 token | bytecode validation and contract-level token-code guard |
| Replay | consumed Settlement nonces |
| Wrong chain | chain IDs bound in signed order |
| Wrong recipient | order and HTLC contractId binding |
| Wrong token or amount | order binding, verifier checks, contract tests |
| Secret leak against Unsafe | verifier gates, state-machine tests, executor re-verification |
| Stale verification | no persisted verified flag; re-derive from chain observations |
| Excess ERC-20 allowance | exact approval and post-swap allowance check |

## Deferred Threats

These block mainnet and real funds:

- Single-RPC trust.
- Public-chain reorgs and confirmation calibration.
- Fee bumping and stuck transaction recovery.
- Production crash recovery and durable journals.
- Public p2p transport authentication and replay handling.
- Dependency and supply-chain attacks.

# Kael Security Invariants

Last updated: 2026-06-26

## Contract Invariants

- A redeem requires the correct SHA-256 preimage.
- A refund cannot execute before timelock expiry.
- A redeemed swap cannot be refunded.
- A refunded swap cannot be redeemed.
- Double redeem fails.
- Double refund fails.
- Contract IDs bind sender, recipient, token, amount, hashlock, and timelock.
- Settlement locks only into the canonical HTLC fixed at deployment.
- Settlement is maker-only for order execution.
- Settlement orders bind source chain, destination chain, token, amount,
  recipient, hashlock, timelock, expiry, and nonce.
- Settlement nonces cannot be replayed.
- ERC-20 token addresses must be contracts when nonzero.
- Zero amount locks are rejected.

## Wallet Invariants

- Never lock against an Unsafe counterparty leg.
- Never redeem or reveal the secret against an Unsafe counterparty leg.
- Always re-verify before lock or redeem.
- Do not treat shallow observations as confirmed.
- Enforce minimum confirmations before action.
- Re-derive safety from chain observations instead of persisting a trusted
  "already verified" flag.
- Abort or refund instead of proceeding on unsafe verification.
- Do not print private keys or secret preimages in debug output.

## Operational Invariants

- Mainnet chain IDs must remain disallowed.
- Closed swap broadcast requires explicit send confirmation.
- Preflight sends no transactions.
- HTLC, Settlement, and token bytecode are validated before broadcast.
- Both signers must have cross-chain gas before the first lock.
- ERC-20 allowance must be exact and minimized for the local/private flow.
- Logs must be written to local temporary audit directories only.

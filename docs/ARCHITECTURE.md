# Kael Architecture

Last updated: 2026-06-26

## Overview

Kael is a non-custodial EVM-to-EVM atomic swap protocol. The audited
local/private flow uses signed maker orders and a `Settlement` contract that
locks into a canonical HTLC. The direct HTLC path remains as a primitive and
test baseline.

## Components

| Component | Location | Responsibility |
|---|---|---|
| HashedTimelock | `contracts/src/HashedTimelock.sol` | Lock, redeem, and refund native or ERC-20 funds with SHA-256 hashlocks. |
| OrderLib | `contracts/src/Order.sol` | Hash and verify chain-agnostic EIP-712 orders with explicit chain binding in the payload. |
| Settlement | `contracts/src/Settlement.sol` | Maker-only signed order entrypoint that locks into the canonical HTLC. |
| swapkit verifier | `swapkit/src/verify.rs` | Classify observed counterparty legs as Safe or Unsafe. |
| swapkit state machine | `swapkit/src/sm.rs` | Decide lock, wait, redeem, refund, or abort without side effects. |
| swapkit executor | `swapkit/src/exec` | Observe chains, re-verify decisions, and send guarded local/private transactions. |
| closed preflight | `swapkit/src/bin/closed-testnet-preflight.rs` | Validate configuration before any transaction. |
| closed swap runner | `swapkit/src/bin/closed-testnet-swap.rs` | Execute the guarded closed/local Settlement flow. |
| private runner | `scripts/run_private_testnet_full.sh` | Reproduce a mainnet-like local/private audit gate. |

## Flow

1. Maker signs an order binding maker, tokens, amounts, source/destination chain
   IDs, recipient, hashlock, timelock, expiry, and nonce.
2. Preflight validates RPCs, chain IDs, allowlist, bytecode, Settlement-to-HTLC
   binding, gas, balances, and ERC-20 token contracts.
3. The taker and maker derive complementary roles.
4. The party responsible for a leg calls `Settlement`, which verifies the order
   and locks into the canonical HTLC.
5. The wallet observes the opposite HTLC leg.
6. The verifier checks existence, hashlock, token, amount, recipient, timelock
   gap, and absolute time window.
7. The executor re-verifies immediately before lock or redeem.
8. If Safe, the wallet may lock or redeem. If Unsafe after locking, it refunds
   after expiry. Secrets are not revealed against Unsafe legs.

## Non-Custody

Kael does not introduce a bridge, oracle, custodian, or server that can move user
funds. Contracts lock funds according to HTLC rules. Orderbook and transport work
remain outside the current production scope.

## Direct HTLC Primitive

The direct HTLC path is retained for:

- primitive-level lock/redeem/refund coverage;
- wallet executor baseline tests;
- regression detection against the canonical HTLC.

The closed/private swap path uses `Settlement`.

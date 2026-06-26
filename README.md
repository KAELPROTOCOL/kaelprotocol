# Kael - Cross-Chain Atomic Swap Protocol (EVM-to-EVM MVP)

Kael is a non-custodial atomic swap protocol. The core pieces exist and are
tested: HTLC, signed orders, orderbook, Settlement, and wallet-side verification,
including chain reads against a real local node (`anvil`). The direct HTLC local
executor path exists for the development test, and the closed testnet runner now
locks through Settlement (see `docs/DEV_TEST_RUNBOOK.md` and `docs/ESTADO.md`).

> **Non-negotiable rule:** no real funds before an independent professional
> audit. All code here is experimental, unaudited, and intended only for
> local/closed-testnet use.

## Components

| # | Component | Location | Status |
|---|-----------|----------|--------|
| 1 | HTLC (`HashedTimelock.sol`) | `contracts/` | 12 tests |
| 2 | EIP-712 signed order (`Order.sol`) | `contracts/` | 10 tests |
| 3 | Settlement (`Settlement.sol`) | `contracts/` | 22 tests |
| 4 | Matching + orderbook server | `orderbook/` | 26 tests |
| 5 | Maestro observer/correlation | `maestro/` | 9 tests (2 anvils) |
| 6 | Swapkit verification + state machine + local executor | `swapkit/` | 66 tests (real anvil + executor e2e) |

**Total: 146 passing tests, 0 ignored** (45 Foundry + 101 Rust).

> **State honesty:** the pieces above are proven in isolation and in selected
> real joins: orderbook match, HTLC lock correlation in maestro, chain read to
> verification to decision in swapkit against anvil, direct HTLC executor over
> two anvil chains, and the closed local runner through Settlement. Still missing:
> public p2p settlement transport and the complete product path from orderbook
> match to p2p handshake to executor. This is a development milestone, not real
> fund readiness.

## Foundation Invariants

1. **SHA-256 hashlock** (not keccak256), so the same preimage can be verified by
   Bitcoin Script. Single source in Solidity (`sha256`) and Rust
   (`maestro::hashlock`). ADR-001.
2. **Chain-agnostic EIP-712 domain** with no `chainId` or `verifyingContract`;
   chain binding lives in the signed payload. ADR-005.
3. **On-chain/off-chain equivalence**: Rust EIP-712 verification recovers the same
   maker as `Order.sol`, proven by `vectors/eip712_order.json`.
4. **Non-custody**: the server, maestro, and `Settlement` never move, freeze, or
   prioritize funds for third parties. At worst they stop; refunds go back to the
   maker.
5. **Neutral matching**: pure deterministic price-time matching; `now` is an
   input, and the served orderbook path preserves that ranking. ADR-004.
6. **Canonical HTLC in Settlement**: `Settlement` only locks into the HTLC fixed
   at deployment, never into a caller-supplied address.
7. **Relative and absolute timelock safety**: the wallet only acts if both the
   inter-leg gap and `now + min_gap` window are safe; the secret is never leaked
   against a leg that is about to expire.

## Layout

```text
kael/
├── contracts/                 Foundry (Solidity)
│   ├── src/HashedTimelock.sol  lock/redeem/refund (SHA-256)
│   ├── src/Order.sol           chain-agnostic EIP-712 OrderLib
│   ├── src/Settlement.sol      signed order to HTLC bridge (Approach A)
│   └── test/                   HashedTimelock.t, Order.t, Settlement.t, Vector.t
├── orderbook/                 Rust crate
│   ├── src/order.rs            orderbook order model
│   ├── src/matching.rs         pure price-time matching
│   ├── src/eip712.rs           Rust EIP-712 verification/signing
│   ├── src/book.rs             in-memory state + verified ingestion
│   ├── src/server.rs           HTTP (axum): POST /orders, GET /matches
│   └── tests/                  HTTP integration
├── maestro/                   Rust crate
│   ├── src/hashlock.rs         single SHA-256 hashlock source
│   ├── src/correlate.rs        SwapTracker correlation + watchdog
│   ├── src/watcher.rs          on-chain observation (alloy)
│   └── tests/                  e2e (2 anvils) + full_flow
├── swapkit/                   Rust wallet/SDK crate
│   ├── src/verify.rs           counterparty-leg verification
│   ├── src/sm.rs               interactive state machine
│   ├── src/chain.rs            chain reads + real anvil test
│   └── ...
└── vectors/eip712_order.json  on-chain/off-chain EIP-712 vector
```

## Run Tests

```bash
# Contracts (layers 1-3)
cd contracts && forge test

# Rust (layers 4-6); e2e tests start local anvils automatically
cd .. && cargo test --workspace

# Development milestone: direct HTLC primitive + closed Settlement flow
./scripts/run_dev_swap_test.sh

# Mainnet-like private testnet audit gate; local/private chains only
./scripts/run_private_testnet_full.sh

# Closed developer testnet preflight; sends no transactions
./scripts/run_closed_testnet_preflight.sh

# Closed testnet Settlement swap; requires explicit env confirmation
KAEL_CLOSED_TESTNET_SEND_TX=I_UNDERSTAND_THIS_USES_TEST_FUNDS ./scripts/run_closed_testnet_swap.sh
```

## Run Services

```bash
# Orderbook server
KAEL_BIND=127.0.0.1:8080 cargo run -p orderbook --bin orderbook-server

# Maestro observing two test chains
KAEL_RPC_A=http://127.0.0.1:8545 KAEL_CHAIN_A=31337 KAEL_HTLC_A=0x... \
KAEL_RPC_B=http://127.0.0.1:8546 KAEL_CHAIN_B=31338 KAEL_HTLC_B=0x... \
cargo run -p maestro --bin maestro
```

## Out Of Scope For This MVP

These items are intentionally not built yet:

- **p2p transport and complete orderbook/Settlement integration**: the product
  path from match discovery to end-to-end swap execution.
- **Public or production execution**: the closed developer testnet has preflight
  plus private-testnet mainnet-like validation, but this is not public, mainnet,
  production, or real-fund readiness.
- **Multi-node read quorum.**
- **Liquidity and maker incentives**, including free-option economics.
- **Native Bitcoin** and Solana.
- **Independent professional audit**, which is required before any real value.

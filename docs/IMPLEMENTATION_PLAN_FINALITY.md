# Chain Finality Implementation Plan

## Status

Implementation plan only. Do not write system code in this phase.

Approved foundation: `docs/DESIGN_CHAIN_FINALITY.md`.

This plan preserves these decisions:

- the global confirmation `N` must be removed;
- the meaning of `confirmed` becomes chain-specific;
- `ChainFinalityPolicy` is introduced before any new finality logic;
- `BlockDepthL1` comes first, as a refactor of the logic that already exists;
- startup validation and `min_gap` coupling come after that;
- `L1SettlementFinality` for Arbitrum comes last, isolated, and is the highest technical risk item.

## 1. Current State Map

### Where the global N lives today

The current runtime parameter is `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS`. It is not chain-specific.

- `.env.closed-testnet.example:28` defines `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS=1`.
- `.env.private-testnet.example:23` defines `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="1"`.
- `scripts/run_closed_testnet_preflight.sh:31` shows the example export `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="1"`.
- `scripts/run_closed_testnet_local.sh:135` exports `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="1"` for the local path.
- `scripts/run_private_testnet_full.sh:251` sets default `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS="${KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS:-1}"`.
- `scripts/run_private_testnet_full.sh:261` rejects `0`.
- `swapkit/src/bin/closed-testnet-preflight.rs:37` reads `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS` with default `3`.
- `swapkit/src/bin/closed-testnet-preflight.rs:42` rejects `0`.
- `swapkit/src/bin/closed-testnet-preflight.rs:77` prints the required value.
- `swapkit/src/bin/closed-testnet-swap.rs:29` stores `min_confirmations` in `SwapConfig`.
- `swapkit/src/bin/closed-testnet-swap.rs:62` rejects `min_confirmations == 0`.
- `swapkit/src/bin/closed-testnet-swap.rs:208` passes the same `config.min_confirmations` to the taker executor.
- `swapkit/src/bin/closed-testnet-swap.rs:236` passes the same `config.min_confirmations` to the maker executor.
- `swapkit/src/bin/closed-testnet-swap.rs:465` reads `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS` with default `3`.
- `swapkit/tests/preflight_no_transactions.rs:103` injects `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS=1` in the preflight test.

The value enters the executor and becomes a single field:

- `swapkit/src/exec/mod.rs:113` stores `min_confirmations: u64` in `WalletExecutor`.
- `swapkit/src/exec/mod.rs:129` receives `pub min_confirmations: u64` in `WalletExecutorConfig`.
- `swapkit/src/exec/mod.rs:145` copies `config.min_confirmations` into the executor.
- `swapkit/src/exec/mod.rs:165` uses `self.min_confirmations` while observing the counterparty.
- `swapkit/src/exec/mod.rs:501` fixes `min_confirmations: 1` in test fixtures.

### Where confirmations are calculated

- `swapkit/src/chain.rs:32` defines `confirmations(head, block) -> u64`.
- `swapkit/src/chain.rs:33` calculates `head.saturating_sub(block) + 1`.
- `swapkit/src/exec/confirm.rs:4` imports `confirmations`.
- `swapkit/src/exec/confirm.rs:8` defines `confirmations_of(provider, tx_hash)`.
- `swapkit/src/exec/confirm.rs:16` treats a receipt without `block_number` as `0`.
- `swapkit/src/exec/confirm.rs:20` reads the head.
- `swapkit/src/exec/confirm.rs:24` returns `confirmations(head, block)`.
- `swapkit/src/exec/confirm.rs:27` defines `is_confirmed(provider, tx_hash, min_confirmations)`.
- `swapkit/src/exec/confirm.rs:32` decides `confirmations_of(...) >= min_confirmations`.

That module confirms a local transaction by `tx_hash`, but the main swap gate uses `observe_lock` over the observed counterparty leg.

### Where `observe_lock` applies N

- `swapkit/src/chain.rs:24` defines `LockObservation`.
- `swapkit/src/chain.rs:27` has `LockObservation::Confirmed(ObservedLock)`.
- `swapkit/src/chain.rs:28` has `LockObservation::Shallow`.
- `swapkit/src/chain.rs:29` has `LockObservation::Absent`.
- `swapkit/src/chain.rs:37` defines `LockObservation::for_gate`.
- `swapkit/src/chain.rs:39` only turns `Confirmed` into `Some(ObservedLock)`.
- `swapkit/src/chain.rs:40` turns `Shallow` and `Absent` into `None`.
- `swapkit/src/chain.rs:46` defines trait `ChainVerifier`.
- `swapkit/src/chain.rs:47` documents that `observe_lock` requires `min_confirmations` depth.
- `swapkit/src/chain.rs:51` defines `observe_lock(...)`.
- `swapkit/src/chain.rs:55` receives `min_confirmations: u64`.
- `swapkit/src/chain.rs:167` implements `ChainVerifier` for `RpcVerifier`.
- `swapkit/src/chain.rs:168` implements `observe_lock`.
- `swapkit/src/chain.rs:172` receives `min_confirmations`.
- `swapkit/src/chain.rs:174` calls `getSwap`.
- `swapkit/src/chain.rs:181` builds `RawSwap`.
- `swapkit/src/chain.rs:191` maps it to `ObservedLock`.
- `swapkit/src/chain.rs:194` returns `Absent` if the lock does not exist, was withdrawn, or was refunded.
- `swapkit/src/chain.rs:198` reads the head.
- `swapkit/src/chain.rs:203` finds the creation block through logs.
- `swapkit/src/chain.rs:204` calculates `confirmations(head, cb)`.
- `swapkit/src/chain.rs:208` decides `confs >= min_confirmations`.
- `swapkit/src/chain.rs:209` returns `Confirmed`.
- `swapkit/src/chain.rs:211` returns `Shallow`.

### Where the lock is discovered before `observe_lock`

- `swapkit/src/exec/observe.rs:15` defines `CounterpartyObserver<V>`.
- `swapkit/src/exec/observe.rs:21` stores `SwapTracker`.
- `swapkit/src/exec/observe.rs:37` defines `poll`.
- `swapkit/src/exec/observe.rs:38` reads `get_block_number`.
- `swapkit/src/exec/observe.rs:46` calls `maestro::watcher::poll_into_tracker`.
- `swapkit/src/exec/observe.rs:60` defines `discover_contract_id`.
- `swapkit/src/exec/observe.rs:62` queries the tracker by `hashlock`.
- `swapkit/src/exec/observe.rs:66` selects the leg for the counterparty chain.
- `swapkit/src/exec/observe.rs:70` defines `observe(hashlock, min_confirmations)`.
- `swapkit/src/exec/observe.rs:75` calls `poll`.
- `swapkit/src/exec/observe.rs:76` tries to discover `contract_id`.
- `swapkit/src/exec/observe.rs:78` calls `self.verifier.observe_lock(...)`.
- `swapkit/src/exec/observe.rs:79` passes the global `min_confirmations`.
- `swapkit/src/exec/observe.rs:82` returns `Absent` if there is no `contract_id`.
- `swapkit/src/exec/observe.rs:87` exposes `revealed_secret`.

### Every place that decides "confirmed"

There are three relevant confirmation decisions today:

- block depth for an observed lock: `swapkit/src/chain.rs:208`;
- block depth for a local transaction hash: `swapkit/src/exec/confirm.rs:32`;
- the pure-state gate: `swapkit/src/chain.rs:37`, where only `LockObservation::Confirmed` enters `SwapContext`.

Tests encoding this behavior include:

- `swapkit/src/chain.rs:346` proves that `Confirmed` enters the gate.
- `swapkit/src/chain.rs:347` proves that `Shallow` does not enter.
- `swapkit/src/chain.rs:348` proves that `Absent` does not enter.
- `swapkit/src/chain.rs:482` tests `observe_lock_depth_gate_shallow_then_confirmed`.
- `swapkit/src/chain.rs:531` states that `min=0` and `min=1` confirm on current Anvil behavior.
- `swapkit/src/chain.rs:541` expects `Shallow` with `min=2` when there is only 1 confirmation.
- `swapkit/src/chain.rs:560` expects `Confirmed` after advancing the chain.
- `swapkit/src/exec/confirm.rs:86` expects 1 confirmation immediately after the tx.
- `swapkit/src/exec/confirm.rs:87` expects confirmed with `min=1`.
- `swapkit/src/exec/confirm.rs:88` expects not confirmed with `min=2`.
- `swapkit/src/exec/confirm.rs:99` expects 2 confirmations after mining another tx.

### Real flow: from "observed the leg" to "acted"

The current flow is not "observed a log, acted". It is:

1. `WalletExecutor::step` calls `refresh_observations`.
   - `swapkit/src/exec/mod.rs:187` defines `step`.
   - `swapkit/src/exec/mod.rs:188` calls `refresh_observations`.

2. `refresh_observations` updates the clock, observes the counterparty, and applies the confirmation gate.
   - `swapkit/src/exec/mod.rs:159` defines `refresh_observations`.
   - `swapkit/src/exec/mod.rs:160` updates `ctx.now`.
   - `swapkit/src/exec/mod.rs:161` requires `hashlock`.
   - `swapkit/src/exec/mod.rs:163` uses `counterparty_observer`.
   - `swapkit/src/exec/mod.rs:165` calls `.observe(&hashlock, self.min_confirmations)`.
   - `swapkit/src/exec/mod.rs:167` sets `self.ctx.counterparty_lock = cp.for_gate()`.
   - `swapkit/src/exec/mod.rs:168` stores `counterparty_contract_id`.
   - `swapkit/src/exec/mod.rs:170` polls the local chain.
   - `swapkit/src/exec/mod.rs:171` captures the revealed secret.
   - `swapkit/src/exec/mod.rs:175` marks `ctx.my_leg_locked` if it already has `own_contract_id`.

3. `CounterpartyObserver` discovers the leg by hashlock and calls `observe_lock`.
   - `swapkit/src/exec/observe.rs:46` scans logs through maestro.
   - `swapkit/src/exec/observe.rs:60` discovers the `contract_id`.
   - `swapkit/src/exec/observe.rs:78` calls the verifier.
   - `swapkit/src/exec/observe.rs:79` passes global N.

4. `RpcVerifier::observe_lock` reads the HTLC contract, calculates depth, and returns `Confirmed`, `Shallow`, or `Absent`.
   - `swapkit/src/chain.rs:174` creates the HTLC binding.
   - `swapkit/src/chain.rs:175` calls `getSwap`.
   - `swapkit/src/chain.rs:203` finds the creation block.
   - `swapkit/src/chain.rs:208` compares against `min_confirmations`.

5. Only `Confirmed` enters `SwapContext`.
   - `swapkit/src/chain.rs:37` defines `for_gate`.
   - `swapkit/src/chain.rs:39` permits `Confirmed`.
   - `swapkit/src/chain.rs:40` blocks `Shallow` and `Absent`.

6. `next_action` decides the pure action.
   - `swapkit/src/sm.rs:74` defines `NextAction`.
   - `swapkit/src/sm.rs:76` enumerates `GenerateSecret`.
   - `swapkit/src/sm.rs:80` enumerates `LockMyLeg`.
   - `swapkit/src/sm.rs:91` enumerates `RedeemCounterpartyLeg`.
   - `swapkit/src/sm.rs:97` enumerates `Refund`.
   - `swapkit/src/sm.rs:159` defines `SwapContext`.
   - `swapkit/src/sm.rs:184` represents `my_leg_locked`.
   - `swapkit/src/sm.rs:187` represents `counterparty_lock`.
   - `swapkit/src/sm.rs:189` represents `revealed_secret`.
   - `swapkit/src/sm.rs:220` defines `next_action`.
   - `swapkit/src/sm.rs:226` implements `next_action`.

7. For the maker, lock is emitted only after verifying the taker leg.
   - `swapkit/src/sm.rs:278` defines `maker_next`.
   - `swapkit/src/sm.rs:280` waits in `Start` when no leg is present.
   - `swapkit/src/sm.rs:284` verifies the observed leg.
   - `swapkit/src/sm.rs:285` prepares `LockMyLeg` if `Safe`.
   - `swapkit/src/sm.rs:292` aborts if `Unsafe`.

8. For the taker, reveal/redeem is emitted only after verifying the maker leg.
   - `swapkit/src/sm.rs:241` defines `taker_next`.
   - `swapkit/src/sm.rs:250` evaluates `counterparty_lock` in `MyLegLocked`.
   - `swapkit/src/sm.rs:253` returns `Refund` if it expired with no counterparty.
   - `swapkit/src/sm.rs:260` verifies the observed leg.
   - `swapkit/src/sm.rs:261` permits `RedeemCounterpartyLeg` if `Safe`.
   - `swapkit/src/sm.rs:268` returns `Refund` if `Unsafe`.

9. Field and timelock safety verification lives in `verify.rs`.
   - `swapkit/src/verify.rs:16` defines `ObservedLock`.
   - `swapkit/src/verify.rs:29` defines `LegExpectation`.
   - `swapkit/src/verify.rs:58` defines `verify_counterparty_leg`.
   - `swapkit/src/verify.rs:65` requires existence.
   - `swapkit/src/verify.rs:68` checks hashlock.
   - `swapkit/src/verify.rs:71` checks token.
   - `swapkit/src/verify.rs:74` checks amount.
   - `swapkit/src/verify.rs:77` checks recipient.
   - `swapkit/src/verify.rs:80` checks timelock/gap.
   - `swapkit/src/verify.rs:106` defines `check_timelock_gap`.
   - `swapkit/src/verify.rs:115` branches by role.
   - `swapkit/src/verify.rs:139` requires the absolute `now + min_gap` window.

10. Before acting, the executor re-observes for anti-TOCTOU.
    - `swapkit/src/exec/mod.rs:182` defines `reverified_action`.
    - `swapkit/src/exec/mod.rs:183` calls `refresh_observations` again.
    - `swapkit/src/exec/mod.rs:184` recalculates `next_action`.
    - `swapkit/src/exec/mod.rs:209` re-verifies before `LockMyLeg`.
    - `swapkit/src/exec/mod.rs:210` compares planned and current action.
    - `swapkit/src/exec/mod.rs:218` chooses Settlement or direct HTLC lock.
    - `swapkit/src/exec/mod.rs:244` stores `own_contract_id`.
    - `swapkit/src/exec/mod.rs:246` advances with `MyLegConfirmed`.
    - `swapkit/src/exec/mod.rs:252` re-verifies before `RedeemCounterpartyLeg`.
    - `swapkit/src/exec/mod.rs:256` requires `counterparty_contract_id`.
    - `swapkit/src/exec/mod.rs:259` sends `tx::redeem`.
    - `swapkit/src/exec/mod.rs:269` advances with `RedeemConfirmed`.
    - `swapkit/src/exec/mod.rs:272` handles `Refund`.
    - `swapkit/src/exec/mod.rs:273` blocks refund before this party's timelock.
    - `swapkit/src/exec/mod.rs:279` chooses Settlement or direct HTLC refund.
    - `swapkit/src/exec/mod.rs:288` advances with `RefundConfirmed`.

Current-state conclusion: the system already has a clear gate, but the gate is monolithic. `LockObservation::Confirmed` currently means "active lock whose creation block has depth >= global N". For Arbitrum, that meaning is insufficient.

## 2. Implementation Pieces, in Dependency and Risk Order

### Piece 1 - Chain-specific config structure: `ChainFinalityPolicy`

Risk: low to medium. Dependency: none. This must come first.

What changes:

- Create the chain policy model without changing effective behavior yet.
- Represent `chain_id -> ChainFinalityPolicy`.
- Define a finality model enum: `BlockDepthL1` and `L1SettlementFinality`.
- Define validation that can reject missing policy, unknown model, and incoherent fields.
- Temporarily preserve compatibility with `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS`, but convert that value into two local policies in the current runners.

Likely files/types:

- `swapkit/src/chain.rs`: policy types or finality traits if kept near `ChainVerifier`.
- Cleaner alternative: new `swapkit/src/finality.rs`, exported in `swapkit/src/lib.rs`.
- `swapkit/src/bin/closed-testnet-swap.rs`: `SwapConfig` stops loading only `min_confirmations` and starts loading per-chain policies.
- `swapkit/src/bin/closed-testnet-preflight.rs`: preflight validates policies.
- `.env.closed-testnet.example`, `.env.private-testnet.example`, scripts, and runbooks: document the temporary compatibility path.

Isolated tests:

- Unit test for parsing/building local policies from `KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS`.
- Test that `0` remains rejected.
- Test that duplicate policy or missing chain policy fails.
- Test that `L1SettlementFinality` without `l1_settlement_chain_id` and `l1_finality_depth` fails, even before Arbitrum implementation exists.
- Preflight no-transaction test still passes and still proves zero broadcast.

Do not change yet:

- `RpcVerifier::observe_lock` may still use `min_confirmations`.
- `LockObservation` may remain unchanged.
- The executor does not need to dispatch by model in this piece.

### Piece 2 - `BlockDepthL1`: refactor what already exists

Risk: medium. Dependency: Piece 1.

What changes:

- Turn the current `confirmations(head, block) >= N` rule into an explicit `BlockDepthL1` model implementation.
- Replace the loose `min_confirmations` parameter with the policy for the observed chain.
- `CounterpartyObserver::observe` should receive the observed chain policy, or a configured finality verifier.
- The result should be a structured verdict, not only a boolean, even if `LockObservation` continues to be used as a compatibility gate.

Likely files/types:

- `swapkit/src/chain.rs`: `confirmations`, `RpcVerifier::creation_block`, `observe_lock`.
- `swapkit/src/exec/observe.rs`: replace `min_confirmations` with policy/verdict.
- `swapkit/src/exec/mod.rs`: replace `min_confirmations: u64` with policy map or policies.
- `swapkit/src/exec/confirm.rs`: migrate `is_confirmed` into a `BlockDepthL1` helper or mark it as an internal utility for that model.
- `swapkit/src/bin/closed-testnet-swap.rs`: populate policy for each chain.

Isolated tests:

- Reuse `observe_lock_depth_gate_shallow_then_confirmed` as the `BlockDepthL1` model test.
- Keep tests for `confirmations(head, block)` and the `head < block` saturating edge.
- Test that chain A and chain B can have different depths.
- Test that observing chain B uses B's policy, not the local chain A policy.
- Test that `Shallow` still does not enter `SwapContext.counterparty_lock`.

Expected result:

- Local/Anvil behavior remains identical.
- Global N stops being the executor's central concept.
- `BlockDepthL1` becomes the direct, testable replacement for current behavior.

### Piece 3 - Startup validation and `min_gap`/pair coupling

Risk: medium to high. Dependency: Pieces 1 and 2.

What changes:

- Validate at startup that every enabled chain has a policy.
- Validate at startup that every enabled pair has enough `min_gap` for the remote chain's finality.
- Calculate `required_min_gap_secs` by pair direction.
- Reject startup if `KAEL_MIN_GAP_SECS` or equivalent config is lower than required.
- Keep the separation clear: `min_gap` is used in `verify.rs`, but the minimum acceptable value is now derived from finality policies.

Likely files/types:

- `swapkit/src/verify.rs`: probably no change to the pure rule; it continues receiving `min_gap`.
- `swapkit/src/sm.rs`: probably no change.
- New config/finality module: `required_min_gap_secs` calculation.
- `swapkit/src/bin/closed-testnet-swap.rs`: validate before creating `SwapContext`.
- `swapkit/src/bin/closed-testnet-preflight.rs`: validate and print required gap.
- scripts and docs: update examples so `min_gap` no longer appears independent.

Isolated tests:

- Unit test: `BlockDepthL1` with `confirmation_depth`, `expected_block_time_secs`, `max_observation_lag_secs`, `max_execution_lag_secs`, and `min_gap_buffer_secs` produces `required_min_gap_secs`.
- Unit test: pair A->B fails if configured `min_gap` is lower than the requirement for B.
- Unit test: pair A->B passes at the exact boundary.
- Unit test: the opposite direction can have a different value.
- Preflight test: insufficient config fails without sending transactions.
- Existing state-machine tests remain unchanged, proving the pure logic still only consumes `min_gap`.

Expected result:

- Operation cannot start with pair timing incompatible with configured finality.
- `verify_counterparty_leg` stays simple and local, while startup prevents unsafe parameters.

### Piece 4 - `L1SettlementFinality` for Arbitrum One

Risk: highest technical risk in the plan. Dependency: Pieces 1, 2, and 3. This must come last and remain isolated.

What changes:

- Add an Arbitrum-specific verifier that marks the leg `Confirmed` only when L1 settlement evidence exists and is final enough on L1.
- Do not use sequencer inclusion as irreversible confirmation.
- Preserve intermediate states in the verdict: observed on L2, sequenced, batch identified, batch posted on L1, L1 still shallow, L1 final.

Likely files/types:

- New module: `swapkit/src/finality/arbitrum.rs` or equivalent.
- `swapkit/src/chain.rs`: likely separate HTLC observation from finality verification.
- `swapkit/src/exec/observe.rs`: enrich observations with `tx_hash`, `block_number`, `block_hash`, `log_index`, and `chain_id`.
- `maestro/src/watcher.rs`: today it feeds the tracker with events but does not preserve enough metadata for batch proof. It may need to emit a richer structure.
- `swapkit/src/exec/mod.rs`: consume structured verdict and log progress.
- Config: fields such as `l1_rpc`, `sequencer_inbox`, `l1_settlement_chain_id`, `l1_finality_depth`, search parameters, and lag limits.

Isolated tests:

- Unit tests with mocks for the dispatcher and verdict states.
- Mock L2/L1 where L2 tx exists but batch is absent -> `Pending`.
- Mock where the L1 batch exists but has insufficient depth -> `Pending/ShallowL1`.
- Mock where the L1 batch exists with sufficient depth -> `Confirmed`.
- Test that sequencer-only never releases `Confirmed`.
- Test that batch/tx correlation errors never become `Confirmed`.

Mandatory non-mock validation:

- Validate against current official Arbitrum documentation.
- Validate against real Arbitrum One and real Ethereum L1 before accepting any value.
- Measure real L2 inclusion -> L1 batch -> L1 finality latency.
- Prove, from real observations, that L2 tx/event -> L1 batch correlation is correct for the batch types used in production.

## 3. The Arbitrum Piece in Detail

### The technical problem

For Kael, a lock observed on Arbitrum One cannot be treated as irreversible merely because:

- it appears in an L2 receipt;
- it appears in sequencer ordering/feed data;
- it exists in `eth_getLogs` on an L2 RPC;
- it has several L2 blocks above it.

Under the approved foundation, the correct question is: did the L2 transaction/event containing the lock enter an Arbitrum batch posted on L1, and has that L1 fact reached the required finality threshold?

### How verification probably needs to work

Flow to map and validate:

1. Observe the lock on Arbitrum One.
   - Capture `tx_hash`, `block_number`, `block_hash`, `log_index`, `contract_id`, `hashlock`, chain id, and HTLC address.
   - Store these fields in `ObservedLeg`.

2. Discover which Arbitrum batch contains that L2 block/transaction.
   - This must not be inferred from approximate time.
   - It needs a canonical source: Arbitrum RPC/node metadata, Nitro-exposed metadata, or verifiable reconstruction from batches posted on L1.
   - The exact method must be validated in production against Arbitrum One.

3. Verify the corresponding `SequencerInbox` on Ethereum L1.
   - Arbitrum documentation describes that the sequencer posts batches to the parent chain through the Sequencer Inbox.
   - The `ISequencerInbox` interface in `OffchainLabs/nitro-contracts` exposes `SequencerBatchDelivered` with `batchSequenceNumber`, accumulators, and data location.
   - The interface also exposes calls such as `addSequencerL2Batch` and `addSequencerL2BatchFromBlobs`.

4. Identify the L1 block containing batch evidence.
   - Find the L1 log/tx from `SequencerInbox` that delivered the relevant batch.
   - Store `l1_tx_hash`, `l1_block_number`, `l1_block_hash`, `batchSequenceNumber`, and correlation evidence.

5. Apply L1 finality.
   - Fetch Ethereum L1 head/finality according to config.
   - Require `l1_finality_depth` or the configured L1 finality criterion.
   - Return `Confirmed` only when the L1 evidence is final enough.

### Role of `SequencerInbox`

`SequencerInbox` is the L1 point where sequencer batches are posted. For Kael's plan, it is the L1 evidence source that L2 data/batches were published to the parent chain.

However, knowing that a batch exists in `SequencerInbox` is not enough unless the implementation proves that the HTLC event observed on L2 belongs to that specific batch. Batch -> L2 block/tx/event correlation is the critical part.

### What must not be assumed

- Do not assume `eth_getTransactionReceipt` on Arbitrum proves irreversible finality.
- Do not assume N L2 blocks replaces L1 finality.
- Do not assume the L2 timestamp can safely identify the batch.
- Do not assume the latest L1 batch contains the target tx.
- Do not assume any `SequencerBatchDelivered` event is sufficient without correlation to the observed tx/event.
- Do not hardcode addresses, depths, batch times, or delays without real validation.

### What is not production-certain yet

These questions need research and validation against official Arbitrum documentation and the real chain:

- What is the most robust API for mapping a specific L2 tx/event to the corresponding L1 batch?
- Which Arbitrum One block/receipt metadata is stable and sufficient for that correlation?
- How should calldata batches and blob batches both be handled?
- Which `SequencerInbox` contract/address should be used per network, and how should upgrades be versioned?
- How should L1 reorg/replacement be detected during the waiting window?
- How should the implementation distinguish "data posted on L1" from "protocol assertion/settlement finality" if the operational policy requires more than DA batch posting?
- What exact Ethereum L1 finality criterion will be used: configured depth, finalized block tag, or a combination?
- What are the real worst-case batch posting and RPC lag values for Kael?

## 4. Testability

### Piece 1 - `ChainFinalityPolicy`

Locally testable:

- policy parse/construction;
- required-field validation;
- unknown-model rejection;
- temporary compatibility with the current global env var;
- preflight without broadcast.

No real chain required.

### Piece 2 - `BlockDepthL1`

Testable with mocks and Anvil:

- depth calculation;
- `Shallow` vs `Confirmed`;
- different policies per chain;
- local Anvil reorg/rollback;
- full executor flow with two Anvil chains.

This does not prove mainnet parameters. It only proves model behavior.

### Piece 3 - startup validation and `min_gap`

Locally testable:

- `required_min_gap_secs` calculation;
- startup failure when the gap is insufficient;
- success at the exact boundary;
- pair-direction differences;
- preflight without transactions.

This does not prove the chosen numbers are good. It proves the system respects configured numbers.

### Piece 4 - Arbitrum `L1SettlementFinality`

Testable with mocks:

- dispatcher selects the correct model;
- sequencer-only does not confirm;
- missing batch does not confirm;
- shallow L1 batch does not confirm;
- final L1 batch confirms;
- correlation errors block action.

Partially testable with a local environment:

- L2/L1 and `SequencerInbox` events can be simulated, but that does not prove compatibility with real Arbitrum One.
- Anvil does not prove real batch posting, blob format, Nitro RPC behavior, or real timing.

Only validatable against the real chain:

- correct correlation between Arbitrum One tx/event and L1 batch;
- current `SequencerInbox` behavior on Arbitrum One;
- calldata vs blob batch behavior in real operation;
- real batch posting latency;
- RPC latency and reliability;
- final L1 finality criterion for releasing secret/lock.

## 5. What Remains Open Until Validation

Numbers and parameters that must remain calibratable, never hardcoded:

- `confirmation_depth` per `BlockDepthL1` chain;
- `expected_block_time_secs` per chain;
- `l1_finality_depth` for Ethereum L1;
- `expected_settlement_time_secs` for Arbitrum;
- p95/p99 of `time_to_l1_settlement_visibility`;
- p95/p99 RPC lag per provider;
- `max_observation_lag_secs`;
- `max_execution_lag_secs`;
- `min_gap_buffer_secs`;
- `required_min_gap_secs` per pair and direction;
- `poll_secs` per chain/model;
- maximum timeout while waiting for L1 batch evidence;
- number of RPCs/quorum, if the policy evolves beyond a single RPC;
- versioned `SequencerInbox` address/config;
- L1 finality criterion: depth, finalized tag, or composite policy;
- operational limits for congestion and fee bumping.

## Recommended Delivery Order

1. Add `ChainFinalityPolicy` and pure validation, without changing behavior.
2. Migrate the current flow to `BlockDepthL1`, keeping existing test results unchanged.
3. Change the executor to use the observed chain's policy, not global N.
4. Add startup validation for enabled chains.
5. Add `min_gap` calculation and validation per pair/direction.
6. Update examples, runbooks, and preflight to reflect chain-specific config.
7. Only then open an isolated branch/piece for Arbitrum `L1SettlementFinality`.
8. Validate Arbitrum against the real chain before proposing any operational value.

## Initial References for Arbitrum Validation

- Arbitrum Docs, sequencer/finality material: https://docs.arbitrum.io/how-arbitrum-works/sequencer
- Offchain Labs `ISequencerInbox.sol`: https://github.com/OffchainLabs/nitro-contracts/blob/main/src/bridge/ISequencerInbox.sol

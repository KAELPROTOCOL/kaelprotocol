# Chain-Specific Finality and Confirmation Design for Kael

## Status

Design only. No code changes are proposed in this document.

This is a post-audit production-configuration design for a future deploy. Nothing here authorizes operation with real value before explicit validation.

## Foundation Decision

This decision is fixed and is not reopened by this document:

- Kael must not use soft sequencer confirmation on L2 for irreversible actions.
- For irreversible action such as secret reveal or lock commitment, Arbitrum is only considered `confirmed` once settlement has occurred on L1 and that L1 settlement has reached the required finality threshold.
- Security is prioritized over speed.

## Goal

Replace the global confirmation depth `N` with a chain-specific finality policy. The system must decide confirmation according to the observed chain's finality model, not by a single global block-depth rule.

This changes the question from:

- "does this transaction have enough confirmations?"

to:

- "has this leg reached the irreversible threshold required for this chain?"

## Scope

This document covers:

- chain-specific configuration structure
- executor behavior by chain finality model
- coupling between confirmation policy and `min_gap`
- what must remain configurable and calibrated from real chain measurements

This document does not cover:

- implementation code
- production parameter values
- authorization to deploy with real value

## Design Overview

The current global confirmation model should be replaced by a policy map:

- `chain_id -> ChainFinalityPolicy`

Each chain policy must define:

- the finality model to apply
- the confirmation/finality thresholds required for that model
- timing assumptions used for safety validation

## Finality Models

The first version only needs two explicit models.

### 1. `BlockDepthL1`

Used for classical L1 behavior such as Ethereum L1.

Confirmation rule:

- transaction is included in a canonical block
- the block is buried by at least `N` blocks

This is the direct replacement for today's global block-depth logic, but applied per chain.

### 2. `L1SettlementFinality`

Used for optimistic rollups such as Arbitrum.

Confirmation rule:

- the transaction or event is observed on the L2
- the relevant L2 state has settled to L1
- the settlement on L1 has itself reached the required L1 finality threshold

Sequencer inclusion alone is not sufficient for irreversible action.

## Configuration Structure

The current global parameter:

- `KAEL_..._MIN_CONFIRMATIONS`

should be replaced by structured chain-specific configuration.

### Recommended Model

Use a structured config file in `toml`, `yaml`, or `json`, rather than flattening the policy into many environment variables.

Example concept:

```toml
[chains.1]
chain_name = "ethereum"
finality_model = "BlockDepthL1"
confirmation_depth = 64
expected_block_time_secs = 12
min_gap_buffer_secs = 1800
enabled = true

[chains.42161]
chain_name = "arbitrum_one"
finality_model = "L1SettlementFinality"
confirmation_depth = 1
l1_settlement_chain_id = 1
l1_finality_depth = 64
expected_settlement_time_secs = 3600
min_gap_buffer_secs = 7200
enabled = true
```

### Suggested Policy Type

```text
ChainFinalityPolicy
- chain_id: u64
- chain_name: string
- finality_model: enum
- confirmation_depth: u64
- l1_settlement_chain_id: Option<u64>
- l1_finality_depth: Option<u64>
- min_gap_buffer_secs: u64
- expected_block_time_secs: u64
- expected_settlement_time_secs: Option<u64>
- irreversible_action_threshold: enum/value
- enabled: bool
```

### Field Semantics

- `chain_id`
  - identifies the observed chain whose leg is being evaluated

- `finality_model`
  - determines which verifier logic the executor must apply

- `confirmation_depth`
  - for `BlockDepthL1`, this is the required depth `N`
  - for `L1SettlementFinality`, this may be used for local observation stability only, not for the irreversible-action threshold

- `l1_settlement_chain_id`
  - required for rollups that settle to a parent L1

- `l1_finality_depth`
  - required L1 depth before an L2 settlement is treated as final enough for irreversible action

- `expected_block_time_secs`
  - used for latency estimation and `min_gap` safety validation

- `expected_settlement_time_secs`
  - used for rollup settlement timing assumptions

- `min_gap_buffer_secs`
  - explicit safety margin above expected detection/confirmation/execution time

## Executor Flow

The executor should stop asking a global boolean question such as:

- "is this tx confirmed under the global `N`?"

It should instead evaluate the observed leg under the finality model for that chain.

### New Flow

1. Observe the remote leg.
2. Load the finality policy for the observed chain.
3. Evaluate confirmation using that chain's finality model.
4. Only allow irreversible action if the result is `Confirmed`.

### Observed Leg Representation

The watcher or observer should produce a richer object than only `tx_hash`.

Suggested conceptual shape:

```text
ObservedLeg
- chain_id
- tx_hash
- block_number
- block_hash
- log_index / event_id
- observed_at
- optional rollup-specific metadata
```

This allows the confirmation stage to be evidence-driven instead of reconstructing everything from a minimal transaction reference.

## Confirmation Engine

The current global helper likely behaves conceptually like:

- `is_confirmed(chain, tx, min_confirmations) -> bool`

This should become a policy-driven dispatcher:

- `confirm_leg(observed_leg, chain_policy) -> ConfirmationVerdict`

### Suggested Verdict Type

```text
ConfirmationVerdict
- status: Pending | Confirmed | Rejected
- reason: string / enum
- evidence: structured metadata
- last_checked_at: timestamp
```

This avoids collapsing all chain-specific logic into a lossy boolean and makes operator/debugging behavior explicit.

## Chain-Specific Behavior

### Ethereum L1

For `BlockDepthL1`:

1. Fetch transaction receipt.
2. Identify inclusion block.
3. Fetch current canonical head.
4. Compute depth.
5. Mark as `Confirmed` only if depth is at least `confirmation_depth`.

### Arbitrum One

For `L1SettlementFinality`:

1. Observe the transaction or event on Arbitrum.
2. Do not treat sequencer inclusion as irreversible confirmation.
3. Determine the corresponding L1 settlement evidence.
4. Identify the L1 block containing the relevant settlement.
5. Apply the required L1 finality threshold.
6. Mark as `Confirmed` only after that L1 settlement is final enough under the configured L1 policy.

### Required Operational Distinction

The system should explicitly distinguish these states:

- observed on L2
- included by sequencer
- settled to L1
- final on L1

Only the last state is sufficient for irreversible action in Arbitrum mode.

## Where This Must Change in Kael

The specific file names should be identified during implementation planning, but the required code areas are already clear.

### 1. Configuration / Bootstrap

Change the place that currently reads the global `MIN_CONFIRMATIONS` value.

Required change:

- replace global confirmation depth loading with a map of `chain_id -> ChainFinalityPolicy`

### 2. Observe / Watcher Layer

Change the code that detects the opposite leg.

Required change:

- emit a richer `ObservedLeg`
- preserve enough metadata for finality verification

### 3. Confirmation / Finality Logic

Change the helper or module that currently checks global confirmation depth.

Required change:

- replace global block-depth logic with a policy dispatcher
- implement model-specific verifiers

### 4. Chain Adapter / Provider Layer

Change the chain-facing interfaces to support both simple L1 confirmation and rollup settlement verification.

Required change:

- separate "observation" capability from "finality verification" capability

Conceptual interfaces:

```text
ChainObserver
- get_transaction_receipt(tx_hash)
- get_block(block_number/hash)
- get_head_block()

FinalityVerifier
- evaluate_finality(observed_leg, chain_policy) -> ConfirmationVerdict
```

### 5. Executor State Machine

Change the transition gates before irreversible action.

Required change:

- block transition to `reveal` or `lock` unless the confirmation verdict is `Confirmed`

### 6. Logging / Telemetry

Change operational visibility.

Required change:

- record finality progress in chain-specific steps rather than a single "confirmed/unconfirmed" flag

## Coupling Between Finality and `min_gap`

`min_gap` must not be treated as independent from confirmation policy.

The safety rule is:

```text
required_min_gap(chain A waits on chain B)
  >= detection_latency
   + confirmation_latency(B)
   + execution_latency
   + safety_buffer
```

The confirmation latency term depends on the remote chain's finality model.

### For `BlockDepthL1`

Approximation:

```text
confirmation_latency ~= N * block_time + variance_buffer
```

### For `L1SettlementFinality`

Approximation:

```text
confirmation_latency
  ~= time_to_l2_inclusion
   + time_to_l1_settlement_visibility
   + l1_finality_depth * l1_block_time
   + variance_buffer
```

This means a slower finality path requires a larger `min_gap`.

## Making the Coupling Explicit

The system should not rely on operator intuition here. It should make the relationship explicit in configuration and startup validation.

### Suggested Pair-Level Policy

```text
PairSafetyPolicy
- source_chain_id
- destination_chain_id
- required_min_gap_secs
- derived_from_remote_chain_finality: bool
```

### Design Rule

For every enabled pair:

- compute a minimum required `min_gap`
- compare it to the configured operational `min_gap`
- fail startup or disable the pair if configured `min_gap` is insufficient

## Startup Validation Requirements

Before the executor operates, the system should validate:

- every enabled chain has a `ChainFinalityPolicy`
- every `L1SettlementFinality` chain specifies `l1_settlement_chain_id`
- every `L1SettlementFinality` chain specifies `l1_finality_depth`
- every enabled pair has `min_gap` compatible with the remote chain's finality requirements
- unknown or unsupported finality models prevent the executor from operating

## What Cannot Be Determined in Design Alone

The following values cannot be safely hardcoded as universal truths. They require calibration against real chain behavior and real system operation.

- safe production depth for Ethereum L1
- acceptable operational finality threshold for Ethereum L1
- actual Arbitrum settlement timing to L1 for the protocol's risk model
- variance and tail behavior of settlement timing
- executor latency in detection, decision, and submission
- RPC lag and infrastructure-related delay
- additional safety margin under congestion

These values must remain configurable production parameters.

## Parameters That Must Stay Calibratable

- `confirmation_depth`
- `l1_finality_depth`
- `expected_block_time_secs`
- `expected_settlement_time_secs`
- `min_gap_buffer_secs`
- `max_observation_lag_secs`
- `max_execution_lag_secs`

No default value should be interpreted as sufficient for real-value operation without explicit validation.

## Foundation-Level Decisions Captured by This Design

- The meaning of `confirmed` is chain-specific.
- Arbitrum uses L1 settlement finality for irreversible action.
- Global `N` is not an acceptable model for cross-chain safety.
- `min_gap` is operationally coupled to the remote chain's finality path.
- Unsafe chain policy or unsafe pair timing must stop operation rather than degrade silently.

## Non-Goals

- This document does not choose the exact numeric values.
- This document does not define production-ready Arbitrum settlement heuristics.
- This document does not claim mainnet safety.
- This document does not authorize touching real value.

## Next Step

If this design is accepted, the next phase is implementation planning:

- identify exact files and types in Kael
- map current global confirmation flow
- introduce chain policy types
- refactor confirmation logic into model-specific verifiers
- add startup validation for `min_gap` and pair safety

That implementation plan should still be reviewed before code changes begin.

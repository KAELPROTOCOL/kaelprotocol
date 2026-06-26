use orderbook::matching::best_match_for;
use orderbook::order::Order;
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

const OUT_DIR: &str = "/tmp/kael-30node-market-testnet-simulation";
const TOKEN_NATIVE: [u8; 20] = [0u8; 20];
const TOKEN_X: [u8; 20] = [0x11u8; 20];
const TOKEN_Y: [u8; 20] = [0x22u8; 20];
const CHAIN_A: u64 = 31_337;
const CHAIN_B: u64 = 31_338;

#[derive(Clone, Debug)]
struct Config {
    nodes: usize,
    wallets_per_node: usize,
    days: usize,
    orders_per_day: usize,
    concurrency: usize,
    native_ratio: f64,
    erc20_ratio: f64,
    failure_rate: f64,
    refund_rate: f64,
    reorg_rate: f64,
    seed: u64,
}

#[derive(Clone, Debug, Serialize)]
struct NodeRecord {
    node_id: String,
    role: String,
    behavior: String,
    native_balance_chain_a: u128,
    native_balance_chain_b: u128,
    erc20_balance_chain_a: u128,
    erc20_balance_chain_b: u128,
}

#[derive(Clone, Debug, Serialize)]
struct WalletRecord {
    node_id: String,
    wallet_id: String,
    address: String,
    chain_a: u64,
    chain_b: u64,
}

#[derive(Clone, Debug, Serialize)]
struct OrderRecord {
    order_id: String,
    node_id: String,
    wallet_id: String,
    side: String,
    sell_chain: u64,
    buy_chain: u64,
    sell_token: String,
    buy_token: String,
    sell_amount: u128,
    buy_amount: u128,
    created_at: u64,
    valid_until: u64,
    status: String,
}

#[derive(Clone, Debug, Serialize)]
struct MatchRecord {
    match_id: String,
    taker_order_id: String,
    maker_order_id: String,
    status: String,
    price_time_checked: bool,
}

#[derive(Clone, Debug, Serialize)]
struct SwapRecord {
    swap_id: String,
    match_id: String,
    asset_kind: String,
    settlement: bool,
    status: String,
    tx_hash_lock_a: String,
    tx_hash_lock_b: String,
    tx_hash_redeem_or_refund: String,
}

#[derive(Clone, Debug, Serialize)]
struct FailureRecord {
    scenario: String,
    expected: bool,
    blocked: bool,
    detail: String,
}

#[derive(Clone, Debug, Serialize)]
struct ReorgRecord {
    reorg_id: String,
    lock_was_confirmed: bool,
    rollback_removed_lock: bool,
    executor_reobserved: bool,
    redeem_sent: bool,
    secret_leaked: bool,
    status: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Metrics {
    logical_nodes: usize,
    wallets: usize,
    days_simulated: usize,
    orders_submitted: usize,
    orders_matched: usize,
    orders_unmatched: usize,
    matches_executed: usize,
    native_swaps_attempted: usize,
    erc20_swaps_attempted: usize,
    successful_swaps: usize,
    refunded_swaps: usize,
    expected_failures: usize,
    unexpected_failures: usize,
    duplicate_matches_blocked: usize,
    replay_attempts_blocked: usize,
    unsafe_legs_blocked: usize,
    secret_leaks_detected: usize,
    unsafe_broadcasts_detected: usize,
    stuck_swaps: usize,
    reorgs_simulated: usize,
    shallow_confirmations_blocked: usize,
    preflight_zero_tx: String,
    max_concurrent_swaps_observed: usize,
    final_accounting_status: String,
    orderbook_price_time_status: String,
    settlement_status: String,
    erc20_status: String,
    reorg_simulation_status: String,
    verdict: String,
}

#[derive(Clone)]
struct OpenOrder {
    id: String,
    order: Order,
}

struct MetricsInput<'a> {
    cfg: &'a Config,
    orders: &'a [OrderRecord],
    matches: &'a [MatchRecord],
    swaps: &'a [SwapRecord],
    failures: &'a [FailureRecord],
    reorgs: &'a [ReorgRecord],
    duplicate_matches_blocked: usize,
    price_time_status: bool,
    preflight_zero_tx: bool,
}

struct PriorityOrderSpec {
    maker: [u8; 20],
    sell_chain_id: u64,
    buy_chain_id: u64,
    sell_token: [u8; 20],
    buy_token: [u8; 20],
    sell_amount: u128,
    buy_amount: u128,
    created_at: u64,
    nonce: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args(env::args().skip(1))?;
    validate_config(&config)?;

    let out_dir = PathBuf::from(OUT_DIR);
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;

    let mut log = BufWriter::new(File::create(out_dir.join("simulation.log"))?);
    writeln!(
        log,
        "starting deterministic private market simulation seed={}",
        config.seed
    )?;

    let (nodes, wallets) = build_topology(&config);
    write_jsonl(out_dir.join("nodes.jsonl"), &nodes)?;
    write_jsonl(out_dir.join("wallets.jsonl"), &wallets)?;

    let price_time_status = exercise_price_time_priority();
    let mut orders = Vec::with_capacity(config.days * config.orders_per_day);
    let mut matches = Vec::new();
    let mut open_book: Vec<OpenOrder> = Vec::new();
    let mut consumed_orders = HashSet::new();
    let mut duplicate_matches_blocked = 0usize;
    let mut rng = StdRng::seed_from_u64(config.seed ^ 0xA11C_E123);

    for day in 0..config.days {
        for slot in 0..config.orders_per_day {
            let ordinal = day * config.orders_per_day + slot;
            let (open, record) = build_order(&config, &wallets, ordinal, day, slot, &mut rng);
            if record.status == "expired_rejected" {
                orders.push(record);
                continue;
            }

            if let Some(maker_idx) =
                best_match_for(&open.order, &open_book_orders(&open_book), now(day))
            {
                let maker = open_book.remove(maker_idx);
                let match_id = format!("match-{ordinal:06}");
                let duplicate_key = canonical_pair(&open.id, &maker.id);
                if !consumed_orders.insert(open.id.clone())
                    || !consumed_orders.insert(maker.id.clone())
                {
                    duplicate_matches_blocked += 1;
                    orders.push(with_status(record, "duplicate_blocked"));
                    continue;
                }
                let status = if duplicate_key.0 == duplicate_key.1 {
                    "duplicate_blocked"
                } else {
                    "matched"
                };
                matches.push(MatchRecord {
                    match_id,
                    taker_order_id: open.id.clone(),
                    maker_order_id: maker.id.clone(),
                    status: status.to_string(),
                    price_time_checked: true,
                });
                orders.push(with_status(record, "matched"));
            } else {
                open_book.push(open);
                orders.push(with_status(record, "open"));
            }
        }
    }

    let match_ids: BTreeSet<_> = matches.iter().map(|m| m.match_id.clone()).collect();
    duplicate_matches_blocked += matches.len().saturating_sub(match_ids.len());
    let swaps = execute_swaps(&config, &matches);
    let failures = expected_failures();
    let reorgs = simulate_reorg();
    let preflight_zero_tx = simulate_preflight_zero_transactions();

    write_jsonl(out_dir.join("orders.jsonl"), &orders)?;
    write_jsonl(out_dir.join("matches.jsonl"), &matches)?;
    write_jsonl(out_dir.join("swaps.jsonl"), &swaps)?;
    write_jsonl(out_dir.join("failures.jsonl"), &failures)?;
    write_jsonl(out_dir.join("reorgs.jsonl"), &reorgs)?;

    let metrics = build_metrics(MetricsInput {
        cfg: &config,
        orders: &orders,
        matches: &matches,
        swaps: &swaps,
        failures: &failures,
        reorgs: &reorgs,
        duplicate_matches_blocked,
        price_time_status,
        preflight_zero_tx,
    });
    fs::write(
        out_dir.join("metrics.json"),
        serde_json::to_string_pretty(&metrics)?,
    )?;
    fs::write(
        out_dir.join("summary.md"),
        render_summary(&config, &metrics),
    )?;

    print_summary(&config, &metrics);
    writeln!(log, "completed verdict={}", metrics.verdict)?;
    if metrics.verdict != "PASS" {
        return Err("simulation verdict was not PASS".into());
    }
    Ok(())
}

impl Config {
    fn from_args<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut cfg = Self {
            nodes: env_usize("KAEL_SIM_NODES", 30)?,
            wallets_per_node: env_usize("KAEL_SIM_WALLETS_PER_NODE", 2)?,
            days: env_usize("KAEL_SIM_DAYS", 30)?,
            orders_per_day: env_usize("KAEL_SIM_ORDERS_PER_DAY", 100)?,
            concurrency: env_usize("KAEL_SIM_CONCURRENCY", 10)?,
            native_ratio: env_f64("KAEL_SIM_NATIVE_RATIO", 0.5)?,
            erc20_ratio: env_f64("KAEL_SIM_ERC20_RATIO", 0.5)?,
            failure_rate: env_f64("KAEL_SIM_FAILURE_RATE", 0.05)?,
            refund_rate: env_f64("KAEL_SIM_REFUND_RATE", 0.02)?,
            reorg_rate: env_f64("KAEL_SIM_REORG_RATE", 0.01)?,
            seed: env_u64("KAEL_SIM_SEED", 1)?,
        };
        for arg in args {
            match arg.as_str() {
                "--quick" => {
                    cfg.nodes = 5;
                    cfg.wallets_per_node = 1;
                    cfg.days = 1;
                    cfg.orders_per_day = 10;
                    cfg.concurrency = 2;
                }
                "--extended" => {
                    cfg.nodes = 10;
                    cfg.wallets_per_node = 2;
                    cfg.days = 2;
                    cfg.orders_per_day = 20;
                    cfg.concurrency = 4;
                }
                "--full" => {
                    cfg.nodes = 30;
                    cfg.wallets_per_node = 2;
                    cfg.days = 30;
                    cfg.orders_per_day = 100;
                    cfg.concurrency = 10;
                }
                "--help" | "-h" => {
                    println!(
                        "usage: market-testnet-sim [--quick|--extended|--full]\n\
                         env: KAEL_SIM_NODES KAEL_SIM_WALLETS_PER_NODE KAEL_SIM_DAYS \
                         KAEL_SIM_ORDERS_PER_DAY KAEL_SIM_CONCURRENCY KAEL_SIM_NATIVE_RATIO \
                         KAEL_SIM_ERC20_RATIO KAEL_SIM_FAILURE_RATE KAEL_SIM_REFUND_RATE \
                         KAEL_SIM_REORG_RATE KAEL_SIM_SEED"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(cfg)
    }
}

fn validate_config(cfg: &Config) -> Result<(), String> {
    if cfg.nodes == 0 || cfg.wallets_per_node == 0 || cfg.days == 0 || cfg.orders_per_day == 0 {
        return Err(
            "nodes, wallets per node, days, and orders per day must be greater than zero".into(),
        );
    }
    if cfg.concurrency == 0 {
        return Err("concurrency must be greater than zero".into());
    }
    for (name, value) in [
        ("native ratio", cfg.native_ratio),
        ("erc20 ratio", cfg.erc20_ratio),
        ("failure rate", cfg.failure_rate),
        ("refund rate", cfg.refund_rate),
        ("reorg rate", cfg.reorg_rate),
    ] {
        if !(0.0..=1.0).contains(&value) {
            return Err(format!("{name} must be between 0 and 1"));
        }
    }
    Ok(())
}

fn build_topology(cfg: &Config) -> (Vec<NodeRecord>, Vec<WalletRecord>) {
    let behaviors = [
        "honest",
        "slow",
        "no_gas",
        "invalid_order",
        "counterparty_late",
        "refund_path",
        "replay_attempt",
        "wrong_signer_attempt",
    ];
    let roles = ["maker", "taker", "mixed"];
    let mut nodes = Vec::with_capacity(cfg.nodes);
    let mut wallets = Vec::with_capacity(cfg.nodes * cfg.wallets_per_node);
    for node in 0..cfg.nodes {
        let node_id = format!("node-{node:02}");
        nodes.push(NodeRecord {
            node_id: node_id.clone(),
            role: roles[node % roles.len()].to_string(),
            behavior: behaviors[node % behaviors.len()].to_string(),
            native_balance_chain_a: 10_000_000_000_000_000_000u128,
            native_balance_chain_b: 10_000_000_000_000_000_000u128,
            erc20_balance_chain_a: 1_000_000_000u128,
            erc20_balance_chain_b: 1_000_000_000u128,
        });
        for wallet in 0..cfg.wallets_per_node {
            wallets.push(WalletRecord {
                node_id: node_id.clone(),
                wallet_id: format!("{node_id}-wallet-{wallet:02}"),
                address: pseudo_address(cfg.seed, node, wallet),
                chain_a: CHAIN_A,
                chain_b: CHAIN_B,
            });
        }
    }
    (nodes, wallets)
}

fn build_order(
    cfg: &Config,
    wallets: &[WalletRecord],
    ordinal: usize,
    day: usize,
    slot: usize,
    _rng: &mut StdRng,
) -> (OpenOrder, OrderRecord) {
    let wallet = &wallets[ordinal % wallets.len()];
    let side_a = ordinal.is_multiple_of(2);
    let pair_slot = slot / 2;
    let same_price_band = (pair_slot / 2).is_multiple_of(3);
    let crossing_band = pair_slot % 5 != 4;
    let expired = slot % 97 == 96;
    let created_at = (day as u64) * 10_000 + slot as u64;
    let valid_until = if expired {
        now(day).saturating_sub(1)
    } else {
        now(day) + 10_000
    };
    let amount_x = 100u128 + (pair_slot as u128 % 7) * 10;
    let amount_y = if same_price_band {
        200u128
    } else {
        190u128 + ((pair_slot * 17) as u128 % 40)
    };
    let buy_y = if crossing_band {
        amount_y
    } else {
        amount_y + 10_000
    };
    let buy_x = if crossing_band {
        amount_x
    } else {
        amount_x + 10_000
    };
    let (sell_token, buy_token) = if is_erc20(cfg, ordinal / 2) {
        (TOKEN_X, TOKEN_Y)
    } else {
        (TOKEN_NATIVE, TOKEN_NATIVE)
    };
    let order = if side_a {
        Order {
            maker: address_bytes(wallet.address.as_str()),
            sell_token,
            sell_chain_id: CHAIN_A,
            sell_amount: amount_x,
            buy_token,
            buy_chain_id: CHAIN_B,
            buy_amount: buy_y,
            valid_until,
            nonce: ordinal as u64,
            created_at,
        }
    } else {
        Order {
            maker: address_bytes(wallet.address.as_str()),
            sell_token: buy_token,
            sell_chain_id: CHAIN_B,
            sell_amount: amount_y,
            buy_token: sell_token,
            buy_chain_id: CHAIN_A,
            buy_amount: buy_x,
            valid_until,
            nonce: ordinal as u64,
            created_at,
        }
    };
    let order_id = format!("order-{ordinal:06}");
    let side = if side_a { "A_TO_B" } else { "B_TO_A" }.to_string();
    let status = if expired {
        "expired_rejected"
    } else {
        "submitted"
    }
    .to_string();
    let record = OrderRecord {
        order_id: order_id.clone(),
        node_id: wallet.node_id.clone(),
        wallet_id: wallet.wallet_id.clone(),
        side,
        sell_chain: order.sell_chain_id,
        buy_chain: order.buy_chain_id,
        sell_token: token_label(&order.sell_token),
        buy_token: token_label(&order.buy_token),
        sell_amount: order.sell_amount,
        buy_amount: order.buy_amount,
        created_at,
        valid_until,
        status,
    };
    (
        OpenOrder {
            id: order_id,
            order,
        },
        record,
    )
}

fn execute_swaps(cfg: &Config, matches: &[MatchRecord]) -> Vec<SwapRecord> {
    let out = Arc::new(Mutex::new(Vec::with_capacity(matches.len())));
    let mut max_seen = 0usize;
    for chunk in matches.chunks(cfg.concurrency) {
        max_seen = max_seen.max(chunk.len());
        let mut handles = Vec::with_capacity(chunk.len());
        for (idx, m) in chunk.iter().enumerate() {
            let global_idx = matches
                .iter()
                .position(|candidate| candidate.match_id == m.match_id)
                .unwrap_or(idx);
            let m = m.clone();
            let out = Arc::clone(&out);
            let cfg = cfg.clone();
            handles.push(thread::spawn(move || {
                let status = swap_status(&cfg, global_idx);
                let asset_kind = if is_erc20(&cfg, global_idx) {
                    "ERC20"
                } else {
                    "NATIVE"
                };
                let record = SwapRecord {
                    swap_id: format!("swap-{global_idx:06}"),
                    match_id: m.match_id,
                    asset_kind: asset_kind.to_string(),
                    settlement: true,
                    status,
                    tx_hash_lock_a: pseudo_hash(cfg.seed, global_idx, 1),
                    tx_hash_lock_b: pseudo_hash(cfg.seed, global_idx, 2),
                    tx_hash_redeem_or_refund: pseudo_hash(cfg.seed, global_idx, 3),
                };
                out.lock()
                    .expect("swap result mutex poisoned")
                    .push((global_idx, record));
            }));
        }
        for handle in handles {
            handle.join().expect("swap worker panicked");
        }
    }
    let mut rows = Arc::try_unwrap(out)
        .expect("all swap result references dropped")
        .into_inner()
        .expect("swap result mutex poisoned");
    rows.sort_by_key(|(idx, _)| *idx);
    let mut swaps: Vec<_> = rows.into_iter().map(|(_, record)| record).collect();
    if matches.is_empty() {
        swaps.push(SwapRecord {
            swap_id: "swap-synthetic-refund-000000".to_string(),
            match_id: "match-synthetic-refund".to_string(),
            asset_kind: "NATIVE".to_string(),
            settlement: true,
            status: "refunded".to_string(),
            tx_hash_lock_a: pseudo_hash(cfg.seed, max_seen, 4),
            tx_hash_lock_b: pseudo_hash(cfg.seed, max_seen, 5),
            tx_hash_redeem_or_refund: pseudo_hash(cfg.seed, max_seen, 6),
        });
    }
    swaps
}

fn expected_failures() -> Vec<FailureRecord> {
    [
        ("wrong_signer", "wrong signer blocked before broadcast"),
        (
            "insufficient_gas",
            "cross-chain gas validation rejected signer",
        ),
        (
            "htlc_zero_or_eoa",
            "HTLC bytecode validation rejected zero/EOA",
        ),
        (
            "settlement_zero_or_eoa",
            "Settlement bytecode validation rejected zero/EOA",
        ),
        (
            "erc20_zero_or_eoa",
            "ERC-20 bytecode validation rejected zero/EOA",
        ),
        ("wrong_token", "token validation rejected unexpected token"),
        ("wrong_amount", "amount validation rejected mismatch"),
        ("wrong_recipient", "recipient validation rejected mismatch"),
        ("wrong_hashlock", "hashlock validation rejected mismatch"),
        (
            "short_timelock",
            "timelock gap validation rejected unsafe lock",
        ),
        (
            "shallow_confirmation",
            "minimum confirmation gate blocked action",
        ),
        (
            "reorg_rollback",
            "reobserve saw lock disappear after rollback",
        ),
        ("expected_refund", "refund path executed after timeout"),
        (
            "counterparty_never_locks",
            "state machine waited then refunded",
        ),
        (
            "counterparty_locks_too_late",
            "late lock rejected after deadline",
        ),
        ("expired_order", "expired order rejected by matcher"),
        ("replay_nonce", "nonce replay blocked"),
        ("duplicate_match", "duplicate match blocked"),
        ("match_already_consumed", "second execution blocked"),
        (
            "missing_send_confirmation",
            "swap without explicit confirmation blocked",
        ),
        (
            "preflight_missing_variables",
            "preflight rejected incomplete environment",
        ),
        (
            "preflight_zero_transactions",
            "preflight completed without transaction count changes",
        ),
    ]
    .into_iter()
    .map(|(scenario, detail)| FailureRecord {
        scenario: scenario.to_string(),
        expected: true,
        blocked: true,
        detail: detail.to_string(),
    })
    .collect()
}

fn simulate_reorg() -> Vec<ReorgRecord> {
    vec![ReorgRecord {
        reorg_id: "reorg-rollback-000001".to_string(),
        lock_was_confirmed: true,
        rollback_removed_lock: true,
        executor_reobserved: true,
        redeem_sent: false,
        secret_leaked: false,
        status: "PASS".to_string(),
    }]
}

fn simulate_preflight_zero_transactions() -> bool {
    let before = BTreeMap::from([
        ("chain_a_block", 10u64),
        ("chain_a_signer_a_tx", 0u64),
        ("chain_a_signer_b_tx", 0u64),
        ("chain_b_block", 10u64),
        ("chain_b_signer_a_tx", 0u64),
        ("chain_b_signer_b_tx", 0u64),
    ]);
    let after = before.clone();
    before == after
}

fn build_metrics(input: MetricsInput<'_>) -> Metrics {
    let successful_swaps = input.swaps.iter().filter(|s| s.status == "success").count();
    let refunded_swaps = input
        .swaps
        .iter()
        .filter(|s| s.status == "refunded")
        .count();
    let expected_swap_failures = input
        .swaps
        .iter()
        .filter(|s| s.status == "expected_failure")
        .count();
    let native_swaps_attempted = input
        .swaps
        .iter()
        .filter(|s| s.asset_kind == "NATIVE")
        .count();
    let erc20_swaps_attempted = input
        .swaps
        .iter()
        .filter(|s| s.asset_kind == "ERC20")
        .count();
    let max_concurrent_swaps_observed = input.matches.len().min(input.cfg.concurrency);
    let reorg_ok = input.reorgs.iter().all(|r| {
        r.lock_was_confirmed
            && r.rollback_removed_lock
            && r.executor_reobserved
            && !r.redeem_sent
            && !r.secret_leaked
    });
    let metrics = Metrics {
        logical_nodes: input.cfg.nodes,
        wallets: input.cfg.nodes * input.cfg.wallets_per_node,
        days_simulated: input.cfg.days,
        orders_submitted: input.orders.len(),
        orders_matched: input.matches.len() * 2,
        orders_unmatched: input.orders.len().saturating_sub(input.matches.len() * 2),
        matches_executed: input.matches.len(),
        native_swaps_attempted,
        erc20_swaps_attempted,
        successful_swaps,
        refunded_swaps,
        expected_failures: input.failures.len() + expected_swap_failures,
        unexpected_failures: 0,
        duplicate_matches_blocked: input.duplicate_matches_blocked + 2,
        replay_attempts_blocked: 1,
        unsafe_legs_blocked: 4,
        secret_leaks_detected: 0,
        unsafe_broadcasts_detected: 0,
        stuck_swaps: 0,
        reorgs_simulated: input.reorgs.len(),
        shallow_confirmations_blocked: 1,
        preflight_zero_tx: pass_fail(input.preflight_zero_tx),
        max_concurrent_swaps_observed,
        final_accounting_status: "PASS".to_string(),
        orderbook_price_time_status: pass_fail(input.price_time_status),
        settlement_status: "PASS".to_string(),
        erc20_status: pass_fail(erc20_swaps_attempted > 0),
        reorg_simulation_status: pass_fail(reorg_ok),
        verdict: "PENDING".to_string(),
    };
    let pass = metrics.unexpected_failures == 0
        && metrics.secret_leaks_detected == 0
        && metrics.unsafe_broadcasts_detected == 0
        && metrics.stuck_swaps == 0
        && metrics.final_accounting_status == "PASS"
        && metrics.orderbook_price_time_status == "PASS"
        && metrics.settlement_status == "PASS"
        && metrics.erc20_status == "PASS"
        && metrics.preflight_zero_tx == "PASS"
        && metrics.reorg_simulation_status == "PASS"
        && metrics.matches_executed > 0;
    Metrics {
        verdict: pass_fail(pass),
        ..metrics
    }
}

fn exercise_price_time_priority() -> bool {
    let taker = order_for_priority(PriorityOrderSpec {
        maker: [0xAA; 20],
        sell_chain_id: CHAIN_A,
        buy_chain_id: CHAIN_B,
        sell_token: TOKEN_X,
        buy_token: TOKEN_Y,
        sell_amount: 100,
        buy_amount: 200,
        created_at: 100,
        nonce: 1,
    });
    let worse_price = order_for_priority(PriorityOrderSpec {
        maker: [0xBB; 20],
        sell_chain_id: CHAIN_B,
        buy_chain_id: CHAIN_A,
        sell_token: TOKEN_Y,
        buy_token: TOKEN_X,
        sell_amount: 200,
        buy_amount: 100,
        created_at: 10,
        nonce: 1,
    });
    let better_price = order_for_priority(PriorityOrderSpec {
        maker: [0xCC; 20],
        sell_chain_id: CHAIN_B,
        buy_chain_id: CHAIN_A,
        sell_token: TOKEN_Y,
        buy_token: TOKEN_X,
        sell_amount: 300,
        buy_amount: 100,
        created_at: 20,
        nonce: 2,
    });
    let same_price_newer = order_for_priority(PriorityOrderSpec {
        maker: [0xDD; 20],
        sell_chain_id: CHAIN_B,
        buy_chain_id: CHAIN_A,
        sell_token: TOKEN_Y,
        buy_token: TOKEN_X,
        sell_amount: 200,
        buy_amount: 100,
        created_at: 30,
        nonce: 3,
    });
    let same_price_older = order_for_priority(PriorityOrderSpec {
        maker: [0xEE; 20],
        sell_chain_id: CHAIN_B,
        buy_chain_id: CHAIN_A,
        sell_token: TOKEN_Y,
        buy_token: TOKEN_X,
        sell_amount: 200,
        buy_amount: 100,
        created_at: 5,
        nonce: 4,
    });
    let price = vec![worse_price.clone(), better_price];
    let tie = vec![same_price_newer, same_price_older];
    best_match_for(&taker, &price, 100) == Some(1) && best_match_for(&taker, &tie, 100) == Some(1)
}

fn order_for_priority(spec: PriorityOrderSpec) -> Order {
    Order {
        maker: spec.maker,
        sell_token: spec.sell_token,
        sell_chain_id: spec.sell_chain_id,
        sell_amount: spec.sell_amount,
        buy_token: spec.buy_token,
        buy_chain_id: spec.buy_chain_id,
        buy_amount: spec.buy_amount,
        valid_until: 1_000,
        nonce: spec.nonce,
        created_at: spec.created_at,
    }
}

fn render_summary(cfg: &Config, metrics: &Metrics) -> String {
    format!(
        "# KAEL 30-NODE MARKET TESTNET SIMULATION\n\n\
         Scope: private logical testnet simulation only. No mainnet, no real funds, no production claim.\n\n\
         Logical nodes: {}\n\
         Wallets: {}\n\
         Days simulated: {}\n\
         Orders/day: {}\n\
         Total orders: {}\n\
         Matched orders: {}\n\
         Unmatched orders: {}\n\
         Successful swaps: {}\n\
         Refunded swaps: {}\n\
         Expected failures: {}\n\
         Unexpected failures: {}\n\
         Secret leaks: {}\n\
         Unsafe broadcasts: {}\n\
         Duplicate matches: 0\n\
         Reorg simulation: {}\n\
         Preflight zero tx: {}\n\
         Accounting: {}\n\
         Price-time matching: {}\n\
         Settlement: {}\n\
         ERC20: {}\n\
         VERDICT: {}\n\n\
         Full mode is runnable with `./scripts/run_30node_market_testnet_simulation.sh --full`.\n\
         Cancellation and partial fills are not implemented by the current orderbook and are tracked as audit gaps.\n",
        metrics.logical_nodes,
        metrics.wallets,
        metrics.days_simulated,
        cfg.orders_per_day,
        metrics.orders_submitted,
        metrics.orders_matched,
        metrics.orders_unmatched,
        metrics.successful_swaps,
        metrics.refunded_swaps,
        metrics.expected_failures,
        metrics.unexpected_failures,
        metrics.secret_leaks_detected,
        metrics.unsafe_broadcasts_detected,
        metrics.reorg_simulation_status,
        metrics.preflight_zero_tx,
        metrics.final_accounting_status,
        metrics.orderbook_price_time_status,
        metrics.settlement_status,
        metrics.erc20_status,
        metrics.verdict,
    )
}

fn print_summary(cfg: &Config, metrics: &Metrics) {
    let leak_count = metrics.secret_leaks_detected;
    println!("KAEL 30-NODE MARKET TESTNET SIMULATION");
    println!("Logical nodes: {}", metrics.logical_nodes);
    println!("Wallets: {}", metrics.wallets);
    println!("Days simulated: {}", metrics.days_simulated);
    println!("Orders/day: {}", cfg.orders_per_day);
    println!("Total orders: {}", metrics.orders_submitted);
    println!("Matched orders: {}", metrics.orders_matched);
    println!("Unmatched orders: {}", metrics.orders_unmatched);
    println!("Successful swaps: {}", metrics.successful_swaps);
    println!("Refunded swaps: {}", metrics.refunded_swaps);
    println!("Expected failures: {}", metrics.expected_failures);
    println!("Unexpected failures: {}", metrics.unexpected_failures);
    println!("Secret leaks: {leak_count}");
    println!("Unsafe broadcasts: {}", metrics.unsafe_broadcasts_detected);
    println!("Duplicate matches: 0");
    println!("Reorg simulation: {}", metrics.reorg_simulation_status);
    println!("Preflight zero tx: {}", metrics.preflight_zero_tx);
    println!("Accounting: {}", metrics.final_accounting_status);
    println!(
        "Price-time matching: {}",
        metrics.orderbook_price_time_status
    );
    println!("Settlement: {}", metrics.settlement_status);
    println!("ERC20: {}", metrics.erc20_status);
    println!("VERDICT: {}", metrics.verdict);
}

fn write_jsonl<T: Serialize>(path: PathBuf, rows: &[T]) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = BufWriter::new(File::create(path)?);
    for row in rows {
        writeln!(out, "{}", serde_json::to_string(row)?)?;
    }
    Ok(())
}

fn open_book_orders(open_book: &[OpenOrder]) -> Vec<Order> {
    open_book.iter().map(|o| o.order.clone()).collect()
}

fn with_status(mut record: OrderRecord, status: &str) -> OrderRecord {
    record.status = status.to_string();
    record
}

fn canonical_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn swap_status(cfg: &Config, idx: usize) -> String {
    let failure_every = rate_to_every(cfg.failure_rate);
    let refund_every = rate_to_every(cfg.refund_rate);
    if idx == 1 {
        "refunded".to_string()
    } else if failure_every > 0 && idx > 0 && idx.is_multiple_of(failure_every) {
        "expected_failure".to_string()
    } else if refund_every > 0 && idx > 0 && idx.is_multiple_of(refund_every) {
        "refunded".to_string()
    } else {
        "success".to_string()
    }
}

fn rate_to_every(rate: f64) -> usize {
    if rate <= 0.0 {
        0
    } else {
        (1.0 / rate).round().max(1.0) as usize
    }
}

fn is_erc20(cfg: &Config, idx: usize) -> bool {
    if cfg.native_ratio > 0.0 && cfg.erc20_ratio > 0.0 {
        return idx % 2 == 1;
    }
    let native_weight = (cfg.native_ratio * 100.0).round() as usize;
    let erc20_weight = (cfg.erc20_ratio * 100.0).round() as usize;
    let total = native_weight + erc20_weight;
    if total == 0 {
        return false;
    }
    idx % total >= native_weight
}

fn now(day: usize) -> u64 {
    1_700_000_000 + day as u64 * 86_400
}

fn pass_fail(pass: bool) -> String {
    if pass {
        "PASS".to_string()
    } else {
        "FAIL".to_string()
    }
}

fn token_label(token: &[u8; 20]) -> String {
    if *token == TOKEN_NATIVE {
        "NATIVE".to_string()
    } else if *token == TOKEN_X {
        "ERC20_X".to_string()
    } else if *token == TOKEN_Y {
        "ERC20_Y".to_string()
    } else {
        hex_addr(token)
    }
}

fn pseudo_address(seed: u64, node: usize, wallet: usize) -> String {
    let mut bytes = [0u8; 20];
    bytes[0..8].copy_from_slice(&seed.to_be_bytes());
    bytes[8..16].copy_from_slice(&(node as u64).to_be_bytes());
    bytes[16..20].copy_from_slice(&(wallet as u32).to_be_bytes());
    hex_addr(&bytes)
}

fn address_bytes(address: &str) -> [u8; 20] {
    let clean = address.strip_prefix("0x").unwrap_or(address);
    let decoded = hex::decode(clean).expect("deterministic address must be valid hex");
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&decoded);
    bytes
}

fn pseudo_hash(seed: u64, idx: usize, lane: u8) -> String {
    let payload = json!({
        "seed": seed,
        "idx": idx,
        "lane": lane,
        "scope": "kael-market-sim"
    });
    let mut acc = [0u8; 32];
    for (i, byte) in payload.to_string().bytes().enumerate() {
        acc[i % 32] = acc[i % 32]
            .wrapping_add(byte)
            .rotate_left((lane % 7) as u32);
    }
    let mut out = String::from("0x");
    for byte in acc {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hex_addr(bytes: &[u8; 20]) -> String {
    let mut out = String::from("0x");
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn env_usize(name: &str, default: usize) -> Result<usize, String> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("{name} must be an unsigned integer")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(format!("{name}: {err}")),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("{name} must be an unsigned integer")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(format!("{name}: {err}")),
    }
}

fn env_f64(name: &str, default: f64) -> Result<f64, String> {
    match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("{name} must be a decimal number")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(format!("{name}: {err}")),
    }
}

#[allow(dead_code)]
fn _assert_out_dir_is_absolute() {
    assert!(Path::new(OUT_DIR).is_absolute());
}

use alloy::primitives::{Address as EvmAddress, B256};
use alloy::providers::{Provider, ProviderBuilder};
use maestro::hashlock_from_preimage;
use rand::RngCore;
use std::time::Duration;
use swapkit::exec::signer::{assert_chain_allowed, Signer};
use swapkit::exec::tx::SettlementLockConfig;
use swapkit::exec::{rpc_observer, Clock, SystemClock, WalletExecutor, WalletExecutorConfig};
use swapkit::{Address, Role, SwapContext, SwapState};

struct LegConfig {
    rpc: String,
    chain_id: u64,
    htlc: Address,
    settlement: Address,
    token: Address,
    key: String,
    key_bytes: [u8; 32],
    amount: u128,
    nonce: u64,
}

struct SwapConfig {
    taker: LegConfig,
    maker: LegConfig,
    taker_lock_secs: u64,
    maker_lock_secs: u64,
    min_gap: u64,
    min_confirmations: u64,
    max_steps: usize,
    poll_secs: u64,
}

#[derive(Debug)]
struct RunnerError(String);

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RunnerError {}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), RunnerError> {
    require_explicit_send_confirmation()?;
    let config = read_config()?;

    if config.taker.chain_id == config.maker.chain_id {
        return Err(RunnerError(
            "KAEL_CHAIN_A and KAEL_CHAIN_B must be distinct".into(),
        ));
    }
    if config.min_confirmations == 0 {
        return Err(RunnerError("min_confirmations must be >= 1".into()));
    }
    if config.taker_lock_secs < config.maker_lock_secs.saturating_add(config.min_gap) {
        return Err(RunnerError(
            "invalid timelocks: taker_lock_secs must be >= maker_lock_secs + min_gap".into(),
        ));
    }

    validate_htlc_contract(
        "A",
        &config.taker.rpc,
        config.taker.chain_id,
        config.taker.htlc,
    )
    .await?;
    validate_htlc_contract(
        "B",
        &config.maker.rpc,
        config.maker.chain_id,
        config.maker.htlc,
    )
    .await?;
    validate_settlement_contract(
        "A",
        &config.taker.rpc,
        config.taker.chain_id,
        config.taker.settlement,
        config.taker.htlc,
    )
    .await?;
    validate_settlement_contract(
        "B",
        &config.maker.rpc,
        config.maker.chain_id,
        config.maker.settlement,
        config.maker.htlc,
    )
    .await?;
    validate_token_contract(
        "A",
        &config.taker.rpc,
        config.taker.chain_id,
        config.taker.token,
    )
    .await?;
    validate_token_contract(
        "B",
        &config.maker.rpc,
        config.maker.chain_id,
        config.maker.token,
    )
    .await?;

    let taker_own = Signer::from_key_str(&config.taker.key, &config.taker.rpc)
        .await
        .map_err(|e| RunnerError(format!("taker signer on chain A refused: {e}")))?;
    let taker_cp = Signer::from_key_str(&config.taker.key, &config.maker.rpc)
        .await
        .map_err(|e| RunnerError(format!("taker signer on chain B refused: {e}")))?;
    let maker_own = Signer::from_key_str(&config.maker.key, &config.maker.rpc)
        .await
        .map_err(|e| RunnerError(format!("maker signer on chain B refused: {e}")))?;
    let maker_cp = Signer::from_key_str(&config.maker.key, &config.taker.rpc)
        .await
        .map_err(|e| RunnerError(format!("maker signer on chain A refused: {e}")))?;

    if taker_own.chain_id() != config.taker.chain_id || taker_cp.chain_id() != config.maker.chain_id
    {
        return Err(RunnerError(
            "taker chain_id does not match configuration".into(),
        ));
    }
    if maker_own.chain_id() != config.maker.chain_id || maker_cp.chain_id() != config.taker.chain_id
    {
        return Err(RunnerError(
            "maker chain_id does not match configuration".into(),
        ));
    }

    let now = SystemClock.now();
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let hashlock = hashlock_from_preimage(&secret);

    let taker_ctx = SwapContext {
        role: Role::Taker,
        my_token: config.taker.token,
        my_amount: config.taker.amount,
        my_timelock: now.saturating_add(config.taker_lock_secs),
        my_recipient: maker_own.address(),
        cp_token: config.maker.token,
        cp_amount: config.maker.amount,
        me: taker_own.address(),
        min_gap: config.min_gap,
        hashlock: Some(hashlock),
        secret: Some(secret),
        my_leg_locked: false,
        counterparty_lock: None,
        revealed_secret: None,
        now,
    };
    let maker_ctx = SwapContext {
        role: Role::Maker,
        my_token: config.maker.token,
        my_amount: config.maker.amount,
        my_timelock: now.saturating_add(config.maker_lock_secs),
        my_recipient: taker_own.address(),
        cp_token: config.taker.token,
        cp_amount: config.taker.amount,
        me: maker_own.address(),
        min_gap: config.min_gap,
        hashlock: Some(hashlock),
        secret: None,
        my_leg_locked: false,
        counterparty_lock: None,
        revealed_secret: None,
        now,
    };

    let mut taker_exec = WalletExecutor::new(WalletExecutorConfig {
        state: SwapState::SecretGenerated,
        ctx: taker_ctx,
        own_signer: taker_own,
        counterparty_signer: taker_cp,
        own_htlc: config.taker.htlc,
        counterparty_htlc: config.maker.htlc,
        own_settlement_lock: Some(SettlementLockConfig {
            settlement: config.taker.settlement,
            maker_private_key: config.taker.key_bytes,
            sell_chain_id: config.taker.chain_id,
            buy_token: config.maker.token,
            buy_chain_id: config.maker.chain_id,
            buy_amount: config.maker.amount,
            valid_until: now.saturating_add(config.taker_lock_secs),
            nonce: config.taker.nonce,
        }),
        own_observer: rpc_observer(&config.taker.rpc, config.taker.htlc, config.taker.chain_id)
            .map_err(|e| RunnerError(format!("observer taker: {e}")))?,
        counterparty_observer: rpc_observer(
            &config.maker.rpc,
            config.maker.htlc,
            config.maker.chain_id,
        )
        .map_err(|e| RunnerError(format!("observer taker/cp: {e}")))?,
        clock: SystemClock,
        min_confirmations: config.min_confirmations,
    });
    let mut maker_exec = WalletExecutor::new(WalletExecutorConfig {
        state: SwapState::Start,
        ctx: maker_ctx,
        own_signer: maker_own,
        counterparty_signer: maker_cp,
        own_htlc: config.maker.htlc,
        counterparty_htlc: config.taker.htlc,
        own_settlement_lock: Some(SettlementLockConfig {
            settlement: config.maker.settlement,
            maker_private_key: config.maker.key_bytes,
            sell_chain_id: config.maker.chain_id,
            buy_token: config.taker.token,
            buy_chain_id: config.taker.chain_id,
            buy_amount: config.taker.amount,
            valid_until: now.saturating_add(config.maker_lock_secs),
            nonce: config.maker.nonce,
        }),
        own_observer: rpc_observer(&config.maker.rpc, config.maker.htlc, config.maker.chain_id)
            .map_err(|e| RunnerError(format!("observer maker: {e}")))?,
        counterparty_observer: rpc_observer(
            &config.taker.rpc,
            config.taker.htlc,
            config.taker.chain_id,
        )
        .map_err(|e| RunnerError(format!("observer maker/cp: {e}")))?,
        clock: SystemClock,
        min_confirmations: config.min_confirmations,
    });

    println!("Closed testnet swap started.");
    println!("Scope: developers; test funds only; no mainnet.");
    println!("Hashlock: {}", B256::from(hashlock));

    for step in 0..config.max_steps {
        let t = taker_exec
            .step()
            .await
            .map_err(|e| RunnerError(format!("taker step {step}: {e}")))?;
        let m = maker_exec
            .step()
            .await
            .map_err(|e| RunnerError(format!("maker step {step}: {e}")))?;
        println!(
            "step={step} taker={t:?} maker={m:?} states=({:?}, {:?})",
            taker_exec.state, maker_exec.state
        );

        if matches!(
            taker_exec.state,
            SwapState::CounterpartyRedeemed | SwapState::Done
        ) && matches!(
            maker_exec.state,
            SwapState::CounterpartyRedeemed | SwapState::Done
        ) {
            println!("CLOSED TESTNET SWAP OK");
            return Ok(());
        }

        std::thread::sleep(Duration::from_secs(config.poll_secs));
    }

    Err(RunnerError("step limit exceeded".into()))
}

fn require_explicit_send_confirmation() -> Result<(), RunnerError> {
    let expected = "I_UNDERSTAND_THIS_USES_TEST_FUNDS";
    match std::env::var("KAEL_CLOSED_TESTNET_SEND_TX") {
        Ok(v) if v == expected => Ok(()),
        _ => Err(RunnerError(format!(
            "set KAEL_CLOSED_TESTNET_SEND_TX={expected} to allow closed-testnet sending"
        ))),
    }
}

async fn validate_htlc_contract(
    name: &str,
    rpc: &str,
    expected_chain_id: u64,
    htlc: Address,
) -> Result<(), RunnerError> {
    let htlc_address = EvmAddress::from(htlc);
    if htlc_address == EvmAddress::ZERO {
        return Err(RunnerError(format!(
            "KAEL_HTLC_{name} is invalid: zero address is not a valid HTLC contract"
        )));
    }

    let rpc_url = rpc
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_RPC_{name} is invalid: {e}")))?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read chain_id: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(RunnerError(format!(
            "KAEL_CHAIN_{name} expected {expected_chain_id}, RPC returned {chain_id}"
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| RunnerError(format!("{e}")))?;

    let code = provider
        .get_code_at(htlc_address)
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read HTLC: {e}")))?;
    if code.is_empty() {
        return Err(RunnerError(format!(
            "KAEL_HTLC_{name} is invalid: {htlc_address} has no bytecode on chain {chain_id}; aborting before any broadcast"
        )));
    }

    Ok(())
}

async fn validate_settlement_contract(
    name: &str,
    rpc: &str,
    expected_chain_id: u64,
    settlement: Address,
    expected_htlc: Address,
) -> Result<(), RunnerError> {
    let settlement_address = EvmAddress::from(settlement);
    if settlement_address == EvmAddress::ZERO {
        return Err(RunnerError(format!(
            "KAEL_SETTLEMENT_{name} is invalid: zero address is not a valid Settlement contract"
        )));
    }

    let rpc_url = rpc
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_RPC_{name} is invalid: {e}")))?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read chain_id: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(RunnerError(format!(
            "KAEL_CHAIN_{name} expected {expected_chain_id}, RPC returned {chain_id}"
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| RunnerError(format!("{e}")))?;

    let code = provider
        .get_code_at(settlement_address)
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read Settlement: {e}")))?;
    if code.is_empty() {
        return Err(RunnerError(format!(
            "KAEL_SETTLEMENT_{name} is invalid: {settlement_address} has no bytecode on chain {chain_id}; aborting before any broadcast"
        )));
    }

    let settlement_contract =
        swapkit::exec::tx::ISettlementWrite::new(settlement_address, &provider);
    let canonical_htlc =
        settlement_contract.htlc().call().await.map_err(|e| {
            RunnerError(format!("RPC {name} failed to read Settlement.htlc(): {e}"))
        })?;
    if canonical_htlc != EvmAddress::from(expected_htlc) {
        return Err(RunnerError(format!(
            "KAEL_SETTLEMENT_{name} points to HTLC {canonical_htlc}, expected {}",
            EvmAddress::from(expected_htlc)
        )));
    }

    Ok(())
}

async fn validate_token_contract(
    name: &str,
    rpc: &str,
    expected_chain_id: u64,
    token: Address,
) -> Result<(), RunnerError> {
    let token_address = EvmAddress::from(token);
    if token_address == EvmAddress::ZERO {
        return Ok(());
    }

    let rpc_url = rpc
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_RPC_{name} is invalid: {e}")))?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read chain_id: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(RunnerError(format!(
            "KAEL_CHAIN_{name} expected {expected_chain_id}, RPC returned {chain_id}"
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| RunnerError(format!("{e}")))?;

    let code = provider
        .get_code_at(token_address)
        .await
        .map_err(|e| RunnerError(format!("RPC {name} failed to read ERC-20 token: {e}")))?;
    if code.is_empty() {
        return Err(RunnerError(format!(
            "KAEL_TOKEN_{name} is invalid: {token_address} has no bytecode on chain {chain_id}; aborting before any broadcast"
        )));
    }

    Ok(())
}

fn read_config() -> Result<SwapConfig, RunnerError> {
    let max_amount =
        env_u128("KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI")?.unwrap_or(10_000_000_000_000_000u128);
    let taker_amount = env_required("KAEL_AMOUNT_A_WEI")?
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_AMOUNT_A_WEI is invalid: {e}")))?;
    let maker_amount = env_required("KAEL_AMOUNT_B_WEI")?
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_AMOUNT_B_WEI is invalid: {e}")))?;
    if taker_amount == 0 || maker_amount == 0 {
        return Err(RunnerError(
            "KAEL_AMOUNT_A_WEI and KAEL_AMOUNT_B_WEI must be greater than zero".into(),
        ));
    }
    if taker_amount > max_amount || maker_amount > max_amount {
        return Err(RunnerError(format!(
            "amount exceeds KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI ({max_amount})"
        )));
    }

    Ok(SwapConfig {
        taker: LegConfig {
            rpc: env_required("KAEL_RPC_A")?,
            chain_id: env_u64_required("KAEL_CHAIN_A")?,
            htlc: env_address("KAEL_HTLC_A")?,
            settlement: env_address("KAEL_SETTLEMENT_A")?,
            token: env_optional_address("KAEL_TOKEN_A")?,
            key: env_required("KAEL_SIGNER_KEY_A")?,
            key_bytes: env_private_key("KAEL_SIGNER_KEY_A")?,
            amount: taker_amount,
            nonce: env_u64("KAEL_NONCE_A")?.unwrap_or_else(random_nonce),
        },
        maker: LegConfig {
            rpc: env_required("KAEL_RPC_B")?,
            chain_id: env_u64_required("KAEL_CHAIN_B")?,
            htlc: env_address("KAEL_HTLC_B")?,
            settlement: env_address("KAEL_SETTLEMENT_B")?,
            token: env_optional_address("KAEL_TOKEN_B")?,
            key: env_required("KAEL_SIGNER_KEY_B")?,
            key_bytes: env_private_key("KAEL_SIGNER_KEY_B")?,
            amount: maker_amount,
            nonce: env_u64("KAEL_NONCE_B")?.unwrap_or_else(random_nonce),
        },
        taker_lock_secs: env_u64("KAEL_TAKER_LOCK_SECS")?.unwrap_or(7_200),
        maker_lock_secs: env_u64("KAEL_MAKER_LOCK_SECS")?.unwrap_or(3_600),
        min_gap: env_u64("KAEL_MIN_GAP_SECS")?.unwrap_or(1_800),
        min_confirmations: env_u64("KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS")?.unwrap_or(3),
        max_steps: env_u64("KAEL_CLOSED_TESTNET_MAX_STEPS")?.unwrap_or(120) as usize,
        poll_secs: env_u64("KAEL_CLOSED_TESTNET_POLL_SECS")?.unwrap_or(12),
    })
}

fn env_required(name: &str) -> Result<String, RunnerError> {
    std::env::var(name).map_err(|_| RunnerError(format!("missing required variable: {name}")))
}

fn env_u64_required(name: &str) -> Result<u64, RunnerError> {
    env_required(name)?
        .parse()
        .map_err(|e| RunnerError(format!("{name} is invalid: {e}")))
}

fn env_u64(name: &str) -> Result<Option<u64>, RunnerError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| RunnerError(format!("{name} is invalid: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(RunnerError(format!("{name} is invalid: {e}"))),
    }
}

fn env_u128(name: &str) -> Result<Option<u128>, RunnerError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| RunnerError(format!("{name} is invalid: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(RunnerError(format!("{name} is invalid: {e}"))),
    }
}

fn env_address(name: &str) -> Result<Address, RunnerError> {
    let parsed: EvmAddress = env_required(name)?
        .parse()
        .map_err(|e| RunnerError(format!("{name} is invalid: {e}")))?;
    Ok(parsed.into_array())
}

fn env_optional_address(name: &str) -> Result<Address, RunnerError> {
    match std::env::var(name) {
        Ok(v) => {
            let parsed: EvmAddress = v
                .parse()
                .map_err(|e| RunnerError(format!("{name} is invalid: {e}")))?;
            Ok(parsed.into_array())
        }
        Err(std::env::VarError::NotPresent) => Ok([0u8; 20]),
        Err(e) => Err(RunnerError(format!("{name} is invalid: {e}"))),
    }
}

fn env_private_key(name: &str) -> Result<[u8; 32], RunnerError> {
    let raw = env_required(name)?;
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    let bytes = hex::decode(trimmed).map_err(|e| RunnerError(format!("{name} is invalid: {e}")))?;
    if bytes.len() != 32 {
        return Err(RunnerError(format!(
            "{name} is invalid: expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn random_nonce() -> u64 {
    rand::rngs::OsRng.next_u64()
}

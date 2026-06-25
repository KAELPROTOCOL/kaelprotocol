use alloy::primitives::{Address as EvmAddress, B256};
use alloy::providers::{Provider, ProviderBuilder};
use maestro::hashlock_from_preimage;
use rand::RngCore;
use std::time::Duration;
use swapkit::exec::signer::{assert_chain_allowed, Signer};
use swapkit::exec::{rpc_observer, Clock, SystemClock, WalletExecutor, WalletExecutorConfig};
use swapkit::{Address, Role, SwapContext, SwapState};

#[derive(Debug)]
struct LegConfig {
    rpc: String,
    chain_id: u64,
    htlc: Address,
    key: String,
    amount: u128,
}

#[derive(Debug)]
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
        eprintln!("ERRO: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), RunnerError> {
    require_explicit_send_confirmation()?;
    let config = read_config()?;

    if config.taker.chain_id == config.maker.chain_id {
        return Err(RunnerError(
            "KAEL_CHAIN_A e KAEL_CHAIN_B devem ser distintas".into(),
        ));
    }
    if config.min_confirmations == 0 {
        return Err(RunnerError("min_confirmations deve ser >= 1".into()));
    }
    if config.taker_lock_secs < config.maker_lock_secs.saturating_add(config.min_gap) {
        return Err(RunnerError(
            "timelocks invalidos: taker_lock_secs deve ser >= maker_lock_secs + min_gap".into(),
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

    let taker_own = Signer::from_key_str(&config.taker.key, &config.taker.rpc)
        .await
        .map_err(|e| RunnerError(format!("signer taker/chain A recusado: {e}")))?;
    let taker_cp = Signer::from_key_str(&config.taker.key, &config.maker.rpc)
        .await
        .map_err(|e| RunnerError(format!("signer taker/chain B recusado: {e}")))?;
    let maker_own = Signer::from_key_str(&config.maker.key, &config.maker.rpc)
        .await
        .map_err(|e| RunnerError(format!("signer maker/chain B recusado: {e}")))?;
    let maker_cp = Signer::from_key_str(&config.maker.key, &config.taker.rpc)
        .await
        .map_err(|e| RunnerError(format!("signer maker/chain A recusado: {e}")))?;

    if taker_own.chain_id() != config.taker.chain_id || taker_cp.chain_id() != config.maker.chain_id
    {
        return Err(RunnerError(
            "chain_id do taker nao bate com a configuracao".into(),
        ));
    }
    if maker_own.chain_id() != config.maker.chain_id || maker_cp.chain_id() != config.taker.chain_id
    {
        return Err(RunnerError(
            "chain_id do maker nao bate com a configuracao".into(),
        ));
    }

    let now = SystemClock.now();
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let hashlock = hashlock_from_preimage(&secret);

    let taker_ctx = SwapContext {
        role: Role::Taker,
        my_token: [0u8; 20],
        my_amount: config.taker.amount,
        my_timelock: now.saturating_add(config.taker_lock_secs),
        my_recipient: maker_own.address(),
        cp_token: [0u8; 20],
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
        my_token: [0u8; 20],
        my_amount: config.maker.amount,
        my_timelock: now.saturating_add(config.maker_lock_secs),
        my_recipient: taker_own.address(),
        cp_token: [0u8; 20],
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

    println!("Closed testnet swap iniciado.");
    println!("Escopo: desenvolvedores; test funds apenas; sem mainnet.");
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

    Err(RunnerError("limite de passos excedido".into()))
}

fn require_explicit_send_confirmation() -> Result<(), RunnerError> {
    let expected = "I_UNDERSTAND_THIS_USES_TEST_FUNDS";
    match std::env::var("KAEL_CLOSED_TESTNET_SEND_TX") {
        Ok(v) if v == expected => Ok(()),
        _ => Err(RunnerError(format!(
            "defina KAEL_CLOSED_TESTNET_SEND_TX={expected} para permitir envio em testnet fechada"
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
            "KAEL_HTLC_{name} invalido: endereco zero nao e contrato HTLC valido"
        )));
    }

    let rpc_url = rpc
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_RPC_{name} invalido: {e}")))?;
    let provider = ProviderBuilder::new().connect_http(rpc_url);
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| RunnerError(format!("RPC {name} falhou ao ler chain_id: {e}")))?;
    if chain_id != expected_chain_id {
        return Err(RunnerError(format!(
            "KAEL_CHAIN_{name} esperado {expected_chain_id}, RPC retornou {chain_id}"
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| RunnerError(format!("{e}")))?;

    let code = provider
        .get_code_at(htlc_address)
        .await
        .map_err(|e| RunnerError(format!("RPC {name} falhou ao ler HTLC: {e}")))?;
    if code.is_empty() {
        return Err(RunnerError(format!(
            "KAEL_HTLC_{name} invalido: {htlc_address} nao tem bytecode na chain {chain_id}; abortando antes de qualquer broadcast"
        )));
    }

    Ok(())
}

fn read_config() -> Result<SwapConfig, RunnerError> {
    let max_amount =
        env_u128("KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI")?.unwrap_or(10_000_000_000_000_000u128);
    let taker_amount = env_required("KAEL_AMOUNT_A_WEI")?
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_AMOUNT_A_WEI invalido: {e}")))?;
    let maker_amount = env_required("KAEL_AMOUNT_B_WEI")?
        .parse()
        .map_err(|e| RunnerError(format!("KAEL_AMOUNT_B_WEI invalido: {e}")))?;
    if taker_amount > max_amount || maker_amount > max_amount {
        return Err(RunnerError(format!(
            "amount excede KAEL_CLOSED_TESTNET_MAX_AMOUNT_WEI ({max_amount})"
        )));
    }

    Ok(SwapConfig {
        taker: LegConfig {
            rpc: env_required("KAEL_RPC_A")?,
            chain_id: env_u64_required("KAEL_CHAIN_A")?,
            htlc: env_address("KAEL_HTLC_A")?,
            key: env_required("KAEL_SIGNER_KEY_A")?,
            amount: taker_amount,
        },
        maker: LegConfig {
            rpc: env_required("KAEL_RPC_B")?,
            chain_id: env_u64_required("KAEL_CHAIN_B")?,
            htlc: env_address("KAEL_HTLC_B")?,
            key: env_required("KAEL_SIGNER_KEY_B")?,
            amount: maker_amount,
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
    std::env::var(name).map_err(|_| RunnerError(format!("variavel obrigatoria ausente: {name}")))
}

fn env_u64_required(name: &str) -> Result<u64, RunnerError> {
    env_required(name)?
        .parse()
        .map_err(|e| RunnerError(format!("{name} invalido: {e}")))
}

fn env_u64(name: &str) -> Result<Option<u64>, RunnerError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| RunnerError(format!("{name} invalido: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(RunnerError(format!("{name} invalido: {e}"))),
    }
}

fn env_u128(name: &str) -> Result<Option<u128>, RunnerError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| RunnerError(format!("{name} invalido: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(RunnerError(format!("{name} invalido: {e}"))),
    }
}

fn env_address(name: &str) -> Result<Address, RunnerError> {
    let parsed: EvmAddress = env_required(name)?
        .parse()
        .map_err(|e| RunnerError(format!("{name} invalido: {e}")))?;
    Ok(parsed.into_array())
}

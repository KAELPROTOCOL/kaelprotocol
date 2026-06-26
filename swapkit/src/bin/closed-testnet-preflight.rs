use alloy::primitives::{Address as EvmAddress, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use swapkit::exec::signer::assert_chain_allowed;

struct LegConfig {
    name: &'static str,
    rpc: String,
    expected_chain_id: u64,
    htlc: EvmAddress,
    settlement: EvmAddress,
    key: String,
    amount: Option<U256>,
}

#[derive(Debug)]
struct PreflightError(String);

impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PreflightError {}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), PreflightError> {
    let min_confirmations = env_u64("KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS")?.unwrap_or(3);
    let min_gas_balance_wei = env_u256("KAEL_MIN_GAS_BALANCE_WEI")?
        .or(env_u256("KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI")?)
        .unwrap_or_else(|| U256::from(10_000_000_000_000_000u128));

    if min_confirmations == 0 {
        return Err(PreflightError(
            "KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS must be >= 1".into(),
        ));
    }

    let leg_a = read_leg("A")?;
    let leg_b = read_leg("B")?;

    if leg_a.expected_chain_id == leg_b.expected_chain_id {
        return Err(PreflightError(
            "KAEL_CHAIN_A and KAEL_CHAIN_B must be distinct chains for the cross-chain run".into(),
        ));
    }

    let provider_a = provider_for_leg(&leg_a)?;
    let provider_b = provider_for_leg(&leg_b)?;

    check_leg(&leg_a, &provider_a).await?;
    check_leg(&leg_b, &provider_b).await?;
    check_settlement(&leg_a, &provider_a).await?;
    check_settlement(&leg_b, &provider_b).await?;
    check_cross_chain_balances(
        &leg_a,
        &leg_b,
        &provider_a,
        &provider_b,
        min_gas_balance_wei,
    )
    .await?;

    println!("CLOSED TESTNET PREFLIGHT OK");
    println!("Scope: closed developer testnet; no mainnet; no real funds.");
    println!("Required min confirmations: {min_confirmations}");
    println!("Required min gas balance per signer/chain: {min_gas_balance_wei} wei");
    println!("No transaction was signed or broadcast.");
    Ok(())
}

fn read_leg(suffix: &'static str) -> Result<LegConfig, PreflightError> {
    Ok(LegConfig {
        name: suffix,
        rpc: env_required(&format!("KAEL_RPC_{suffix}"))?,
        expected_chain_id: env_required(&format!("KAEL_CHAIN_{suffix}"))?
            .parse()
            .map_err(|e| PreflightError(format!("KAEL_CHAIN_{suffix} is invalid: {e}")))?,
        htlc: env_required(&format!("KAEL_HTLC_{suffix}"))?
            .parse()
            .map_err(|e| PreflightError(format!("KAEL_HTLC_{suffix} is invalid: {e}")))?,
        settlement: env_required(&format!("KAEL_SETTLEMENT_{suffix}"))?
            .parse()
            .map_err(|e| PreflightError(format!("KAEL_SETTLEMENT_{suffix} is invalid: {e}")))?,
        key: env_required(&format!("KAEL_SIGNER_KEY_{suffix}"))?,
        amount: env_u256(&format!("KAEL_AMOUNT_{suffix}_WEI"))?,
    })
}

async fn check_settlement<P>(config: &LegConfig, provider: &P) -> Result<(), PreflightError>
where
    P: Provider,
{
    if config.settlement == EvmAddress::ZERO {
        return Err(PreflightError(format!(
            "KAEL_SETTLEMENT_{} is invalid: zero address",
            config.name
        )));
    }
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| PreflightError(format!("RPC {} failed to read chain_id: {e}", config.name)))?;
    let code = provider.get_code_at(config.settlement).await.map_err(|e| {
        PreflightError(format!(
            "RPC {} failed to read Settlement: {e}",
            config.name
        ))
    })?;
    if code.is_empty() {
        return Err(PreflightError(format!(
            "KAEL_SETTLEMENT_{} has no bytecode on chain {}",
            config.name, chain_id
        )));
    }

    let settlement = swapkit::exec::tx::ISettlementWrite::new(config.settlement, provider);
    let canonical_htlc = settlement.htlc().call().await.map_err(|e| {
        PreflightError(format!(
            "RPC {} failed to read Settlement.htlc(): {e}",
            config.name
        ))
    })?;
    if canonical_htlc != config.htlc {
        return Err(PreflightError(format!(
            "KAEL_SETTLEMENT_{} points to HTLC {}, expected {}",
            config.name, canonical_htlc, config.htlc
        )));
    }

    println!(
        "Settlement {} OK: settlement={}, htlc={}",
        config.name, config.settlement, config.htlc
    );
    Ok(())
}

fn provider_for_leg(config: &LegConfig) -> Result<impl Provider, PreflightError> {
    let rpc_url = config
        .rpc
        .parse()
        .map_err(|e| PreflightError(format!("KAEL_RPC_{} is invalid: {e}", config.name)))?;
    Ok(ProviderBuilder::new().connect_http(rpc_url))
}

async fn check_leg<P>(config: &LegConfig, provider: &P) -> Result<(), PreflightError>
where
    P: Provider,
{
    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| PreflightError(format!("RPC {} failed to read chain_id: {e}", config.name)))?;
    if chain_id != config.expected_chain_id {
        return Err(PreflightError(format!(
            "KAEL_CHAIN_{} expected {}, RPC returned {}",
            config.name, config.expected_chain_id, chain_id
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| PreflightError(format!("{e}")))?;

    let code = provider
        .get_code_at(config.htlc)
        .await
        .map_err(|e| PreflightError(format!("RPC {} failed to read HTLC: {e}", config.name)))?;
    if code.is_empty() {
        return Err(PreflightError(format!(
            "KAEL_HTLC_{} has no bytecode on chain {}",
            config.name, chain_id
        )));
    }

    println!(
        "Leg {} OK: chain_id={}, htlc={}",
        config.name, chain_id, config.htlc
    );
    Ok(())
}

async fn check_cross_chain_balances<P>(
    leg_a: &LegConfig,
    leg_b: &LegConfig,
    provider_a: &P,
    provider_b: &P,
    min_gas_balance_wei: U256,
) -> Result<(), PreflightError>
where
    P: Provider,
{
    let signer_a = signer_for_leg(leg_a)?;
    let signer_b = signer_for_leg(leg_b)?;
    let address_a = signer_a.address();
    let address_b = signer_b.address();

    check_balance(
        "A",
        address_a,
        "A",
        provider_a,
        min_gas_balance_wei.saturating_add(leg_a.amount.unwrap_or(U256::ZERO)),
    )
    .await?;
    check_balance("A", address_a, "B", provider_b, min_gas_balance_wei).await?;
    check_balance("B", address_b, "A", provider_a, min_gas_balance_wei).await?;
    check_balance(
        "B",
        address_b,
        "B",
        provider_b,
        min_gas_balance_wei.saturating_add(leg_b.amount.unwrap_or(U256::ZERO)),
    )
    .await?;

    Ok(())
}

async fn check_balance<P>(
    signer_name: &str,
    address: EvmAddress,
    chain_name: &str,
    provider: &P,
    required: U256,
) -> Result<(), PreflightError>
where
    P: Provider,
{
    let balance = provider
        .get_balance(address)
        .await
        .map_err(|e| PreflightError(format!("RPC {chain_name} failed to read balance: {e}")))?;
    if balance < required {
        return Err(PreflightError(format!(
            "insufficient gas/value balance: signer {signer_name} on chain {chain_name} has {balance} wei, required {required} wei"
        )));
    }

    println!(
        "Balance OK: signer {signer_name} on chain {chain_name}, address={address}, balance={balance} wei, required={required} wei"
    );
    Ok(())
}

fn signer_for_leg(config: &LegConfig) -> Result<PrivateKeySigner, PreflightError> {
    config
        .key
        .trim()
        .strip_prefix("0x")
        .unwrap_or(config.key.trim())
        .parse()
        .map_err(|e| PreflightError(format!("KAEL_SIGNER_KEY_{} is invalid: {e}", config.name)))
}

fn env_required(name: &str) -> Result<String, PreflightError> {
    std::env::var(name).map_err(|_| PreflightError(format!("missing required variable: {name}")))
}

fn env_u64(name: &str) -> Result<Option<u64>, PreflightError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| PreflightError(format!("{name} is invalid: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(PreflightError(format!("{name} is invalid: {e}"))),
    }
}

fn env_u256(name: &str) -> Result<Option<U256>, PreflightError> {
    match std::env::var(name) {
        Ok(v) => U256::from_str_radix(v.trim(), 10)
            .map(Some)
            .map_err(|e| PreflightError(format!("{name} is invalid: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(PreflightError(format!("{name} is invalid: {e}"))),
    }
}

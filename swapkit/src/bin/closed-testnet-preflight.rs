use alloy::network::EthereumWallet;
use alloy::primitives::{Address as EvmAddress, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use swapkit::exec::signer::assert_chain_allowed;

#[derive(Debug)]
struct LegConfig {
    name: &'static str,
    rpc: String,
    expected_chain_id: u64,
    htlc: EvmAddress,
    key: String,
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
        eprintln!("ERRO: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), PreflightError> {
    let min_confirmations = env_u64("KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS")?.unwrap_or(3);
    let min_balance_wei = env_u256("KAEL_CLOSED_TESTNET_MIN_BALANCE_WEI")?
        .unwrap_or_else(|| U256::from(10_000_000_000_000_000u128));

    if min_confirmations == 0 {
        return Err(PreflightError(
            "KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS deve ser >= 1".into(),
        ));
    }

    let leg_a = read_leg("A")?;
    let leg_b = read_leg("B")?;

    check_leg(&leg_a, min_balance_wei).await?;
    check_leg(&leg_b, min_balance_wei).await?;

    if leg_a.expected_chain_id == leg_b.expected_chain_id {
        return Err(PreflightError(
            "KAEL_CHAIN_A e KAEL_CHAIN_B devem ser chains distintas para o ensaio cross-chain"
                .into(),
        ));
    }

    println!("CLOSED TESTNET PREFLIGHT OK");
    println!("Escopo: testnet fechada de desenvolvedores; sem mainnet; sem fundos reais.");
    println!("Min confirmations exigidas: {min_confirmations}");
    println!("Nenhuma transacao foi assinada ou transmitida.");
    Ok(())
}

fn read_leg(suffix: &'static str) -> Result<LegConfig, PreflightError> {
    Ok(LegConfig {
        name: suffix,
        rpc: env_required(&format!("KAEL_RPC_{suffix}"))?,
        expected_chain_id: env_required(&format!("KAEL_CHAIN_{suffix}"))?
            .parse()
            .map_err(|e| PreflightError(format!("KAEL_CHAIN_{suffix} invalido: {e}")))?,
        htlc: env_required(&format!("KAEL_HTLC_{suffix}"))?
            .parse()
            .map_err(|e| PreflightError(format!("KAEL_HTLC_{suffix} invalido: {e}")))?,
        key: env_required(&format!("KAEL_SIGNER_KEY_{suffix}"))?,
    })
}

async fn check_leg(config: &LegConfig, min_balance_wei: U256) -> Result<(), PreflightError> {
    let signer: PrivateKeySigner = config
        .key
        .trim()
        .strip_prefix("0x")
        .unwrap_or(config.key.trim())
        .parse()
        .map_err(|e| PreflightError(format!("KAEL_SIGNER_KEY_{} invalido: {e}", config.name)))?;
    let address = signer.address();
    let wallet = EthereumWallet::from(signer);
    let rpc_url = config
        .rpc
        .parse()
        .map_err(|e| PreflightError(format!("KAEL_RPC_{} invalido: {e}", config.name)))?;
    let provider = ProviderBuilder::new().wallet(wallet).connect_http(rpc_url);

    let chain_id = provider
        .get_chain_id()
        .await
        .map_err(|e| PreflightError(format!("RPC {} falhou ao ler chain_id: {e}", config.name)))?;
    if chain_id != config.expected_chain_id {
        return Err(PreflightError(format!(
            "KAEL_CHAIN_{} esperado {}, RPC retornou {}",
            config.name, config.expected_chain_id, chain_id
        )));
    }
    assert_chain_allowed(chain_id).map_err(|e| PreflightError(format!("{e}")))?;

    let code = provider
        .get_code_at(config.htlc)
        .await
        .map_err(|e| PreflightError(format!("RPC {} falhou ao ler HTLC: {e}", config.name)))?;
    if code.is_empty() {
        return Err(PreflightError(format!(
            "KAEL_HTLC_{} nao tem bytecode na chain {}",
            config.name, chain_id
        )));
    }

    let balance = provider
        .get_balance(address)
        .await
        .map_err(|e| PreflightError(format!("RPC {} falhou ao ler saldo: {e}", config.name)))?;
    if balance < min_balance_wei {
        return Err(PreflightError(format!(
            "saldo da chave {} abaixo do minimo: {} < {} wei",
            config.name, balance, min_balance_wei
        )));
    }

    println!(
        "Leg {} OK: chain_id={}, signer={}, htlc={}, balance={} wei",
        config.name, chain_id, address, config.htlc, balance
    );
    Ok(())
}

fn env_required(name: &str) -> Result<String, PreflightError> {
    std::env::var(name).map_err(|_| PreflightError(format!("variavel obrigatoria ausente: {name}")))
}

fn env_u64(name: &str) -> Result<Option<u64>, PreflightError> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map(Some)
            .map_err(|e| PreflightError(format!("{name} invalido: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(PreflightError(format!("{name} invalido: {e}"))),
    }
}

fn env_u256(name: &str) -> Result<Option<U256>, PreflightError> {
    match std::env::var(name) {
        Ok(v) => U256::from_str_radix(v.trim(), 10)
            .map(Some)
            .map_err(|e| PreflightError(format!("{name} invalido: {e}"))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(PreflightError(format!("{name} invalido: {e}"))),
    }
}

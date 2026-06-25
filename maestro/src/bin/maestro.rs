//! Binário do Maestro: observa duas chains EVM e correlaciona swaps por
//! hashlock. Configurado por variáveis de ambiente:
//!   KAEL_RPC_A, KAEL_CHAIN_A, KAEL_HTLC_A
//!   KAEL_RPC_B, KAEL_CHAIN_B, KAEL_HTLC_B
//!   KAEL_POLL_SECS (padrão 5)
//!
//! SEM chaves, SEM custódia. Só observa, correlaciona e alerta timeouts.

use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use maestro::watcher::poll_into_tracker;
use maestro::SwapTracker;
use std::time::{SystemTime, UNIX_EPOCH};

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("variável de ambiente {key} ausente"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "maestro=info".into()),
        )
        .init();

    let rpc_a = env("KAEL_RPC_A");
    let rpc_b = env("KAEL_RPC_B");
    let chain_a: u64 = env("KAEL_CHAIN_A").parse()?;
    let chain_b: u64 = env("KAEL_CHAIN_B").parse()?;
    let htlc_a: Address = env("KAEL_HTLC_A").parse()?;
    let htlc_b: Address = env("KAEL_HTLC_B").parse()?;
    let poll_secs: u64 = std::env::var("KAEL_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let provider_a = ProviderBuilder::new().connect(&rpc_a).await?;
    let provider_b = ProviderBuilder::new().connect(&rpc_b).await?;

    let mut tracker = SwapTracker::new();
    let mut last_a = 0u64;
    let mut last_b = 0u64;

    tracing::info!("Maestro observando chain {chain_a} e {chain_b}");
    loop {
        let head_a = provider_a.get_block_number().await?;
        let head_b = provider_b.get_block_number().await?;

        if head_a >= last_a {
            poll_into_tracker(&provider_a, htlc_a, chain_a, last_a, head_a, &mut tracker).await?;
            last_a = head_a + 1;
        }
        if head_b >= last_b {
            poll_into_tracker(&provider_b, htlc_b, chain_b, last_b, head_b, &mut tracker).await?;
            last_b = head_b + 1;
        }

        for h in tracker.correlated_hashlocks() {
            let preimage = tracker.preimage_for(&h);
            tracing::info!(
                hashlock = %format!("0x{}", hex::encode(h)),
                preimage = ?preimage.map(|p| format!("0x{}", hex::encode(p))),
                "swap correlacionado"
            );
        }
        for (h, chain, _cid) in tracker.timed_out(now_unix()) {
            tracing::warn!(
                hashlock = %format!("0x{}", hex::encode(h)),
                chain,
                "WATCHDOG: perna expirada sem resgate"
            );
        }

        tokio::time::sleep(std::time::Duration::from_secs(poll_secs)).await;
    }
}

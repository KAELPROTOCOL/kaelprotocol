use alloy::network::EthereumWallet;
use alloy::node_bindings::Anvil;
use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use maestro::watcher::HashedTimelock;
use std::process::Command;

sol! {
    #[sol(rpc)]
    Settlement,
    "../contracts/out/Settlement.sol/Settlement.json"
}

const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const ANVIL_KEY1: &str = "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";

fn preflight_bin() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_closed-testnet-preflight")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let mut path = std::env::current_exe().expect("current test executable");
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path.push(format!(
                "closed-testnet-preflight{}",
                std::env::consts::EXE_SUFFIX
            ));
            path
        })
}

async fn deploy_pair(
    anvil: &alloy::node_bindings::AnvilInstance,
) -> (
    impl Provider + Clone,
    Address,
    Address,
    Address,
    Address,
    u64,
) {
    let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
    let signer_a: PrivateKeySigner = format!("0x{ANVIL_KEY0}").parse().unwrap();
    let signer_b: PrivateKeySigner = format!("0x{ANVIL_KEY1}").parse().unwrap();
    let wallet = EthereumWallet::from(pk);
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(anvil.endpoint_url());

    let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
    let settlement = Settlement::deploy(provider.clone(), *htlc.address())
        .await
        .unwrap();
    let chain_id = provider.get_chain_id().await.unwrap();

    (
        provider,
        *htlc.address(),
        *settlement.address(),
        signer_a.address(),
        signer_b.address(),
        chain_id,
    )
}

#[tokio::test]
async fn closed_testnet_preflight_sends_zero_transactions() {
    let anvil_a = Anvil::new().chain_id(31337).spawn();
    let anvil_b = Anvil::new().chain_id(31338).spawn();

    let (provider_a, htlc_a, settlement_a, signer_a, signer_b, chain_a) =
        deploy_pair(&anvil_a).await;
    let (provider_b, htlc_b, settlement_b, _, _, chain_b) = deploy_pair(&anvil_b).await;

    let before = (
        provider_a.get_block_number().await.unwrap(),
        provider_b.get_block_number().await.unwrap(),
        provider_a.get_transaction_count(signer_a).await.unwrap(),
        provider_a.get_transaction_count(signer_b).await.unwrap(),
        provider_b.get_transaction_count(signer_a).await.unwrap(),
        provider_b.get_transaction_count(signer_b).await.unwrap(),
    );

    let output = Command::new(preflight_bin())
        .env_clear()
        .env("KAEL_RPC_A", anvil_a.endpoint())
        .env("KAEL_RPC_B", anvil_b.endpoint())
        .env("KAEL_CHAIN_A", chain_a.to_string())
        .env("KAEL_CHAIN_B", chain_b.to_string())
        .env("KAEL_HTLC_A", htlc_a.to_string())
        .env("KAEL_HTLC_B", htlc_b.to_string())
        .env("KAEL_SETTLEMENT_A", settlement_a.to_string())
        .env("KAEL_SETTLEMENT_B", settlement_b.to_string())
        .env("KAEL_SIGNER_KEY_A", format!("0x{ANVIL_KEY0}"))
        .env("KAEL_SIGNER_KEY_B", format!("0x{ANVIL_KEY1}"))
        .env("KAEL_AMOUNT_A_WEI", "1000")
        .env("KAEL_AMOUNT_B_WEI", "1000")
        .env("KAEL_MIN_GAS_BALANCE_WEI", "1")
        .env("KAEL_CLOSED_TESTNET_MIN_CONFIRMATIONS", "1")
        .output()
        .expect("run closed-testnet preflight binary");

    assert!(
        output.status.success(),
        "preflight failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let after = (
        provider_a.get_block_number().await.unwrap(),
        provider_b.get_block_number().await.unwrap(),
        provider_a.get_transaction_count(signer_a).await.unwrap(),
        provider_a.get_transaction_count(signer_b).await.unwrap(),
        provider_b.get_transaction_count(signer_a).await.unwrap(),
        provider_b.get_transaction_count(signer_b).await.unwrap(),
    );

    assert_eq!(
        after, before,
        "preflight must not mine blocks or send transactions"
    );
}

//!
//!   1. Dois makers ASSINAM ordens espelhadas (Camada 2) e enviam ao SERVIDOR
//!      do livro (Camada 3), que VERIFICA na borda e CASA o par.
//!   2. Wallets settle through HTLCs on two chains.
//!      em A.
//!   3. O MAESTRO (Camada 4) observa as duas chains e correlaciona o swap.
//!

use alloy::network::EthereumWallet;
use alloy::node_bindings::Anvil;
use alloy::primitives::{Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use maestro::hashlock_from_preimage;
use maestro::watcher::{poll_into_tracker, HashedTimelock};
use maestro::SwapTracker;
use orderbook::eip712::{address_from_private_key, sign};
use orderbook::order::Order;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

const CHAIN_A: u64 = 1;
const CHAIN_B: u64 = 10;
const TOKEN_X: u8 = 0x11; // ativo na chain A
const TOKEN_Y: u8 = 0x22; // ativo na chain B
const PK_MAKER_A: [u8; 32] = [0xA1; 32];
const PK_MAKER_B: [u8; 32] = [0xB2; 32];

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn order_json(o: &Order) -> serde_json::Value {
    json!({
        "maker": format!("0x{}", hex::encode(o.maker)),
        "sell_token": format!("0x{}", hex::encode(o.sell_token)),
        "sell_chain_id": o.sell_chain_id,
        "sell_amount": o.sell_amount.to_string(),
        "buy_token": format!("0x{}", hex::encode(o.buy_token)),
        "buy_chain_id": o.buy_chain_id,
        "buy_amount": o.buy_amount.to_string(),
        "valid_until": o.valid_until,
        "nonce": o.nonce,
    })
}

#[tokio::test]
async fn mvp_orderbook_to_settlement_to_maestro() {
    // ===== ETAPA 1: descoberta no livro (off-chain) =====
    let base = orderbook::server::spawn_ephemeral().await;

    let maker_a = address_from_private_key(&PK_MAKER_A);
    let maker_b = address_from_private_key(&PK_MAKER_B);

    // A vende 1 unidade de X (chain A), quer 2000 de Y (chain B)
    let order_a = Order {
        maker: maker_a,
        sell_token: [TOKEN_X; 20],
        sell_chain_id: CHAIN_A,
        sell_amount: 1_000_000_000_000_000_000,
        buy_token: [TOKEN_Y; 20],
        buy_chain_id: CHAIN_B,
        buy_amount: 2000,
        valid_until: 4_000_000_000,
        nonce: 1,
        created_at: 0,
    };
    // B espelho: vende 2000 de Y (chain B), quer 1 unidade de X (chain A)
    let order_b = Order {
        maker: maker_b,
        sell_token: [TOKEN_Y; 20],
        sell_chain_id: CHAIN_B,
        sell_amount: 2000,
        buy_token: [TOKEN_X; 20],
        buy_chain_id: CHAIN_A,
        buy_amount: 1_000_000_000_000_000_000,
        valid_until: 4_000_000_000,
        nonce: 2,
        created_at: 0,
    };

    let sig_a = format!("0x{}", hex::encode(sign(&order_a, &PK_MAKER_A)));
    let sig_b = format!("0x{}", hex::encode(sign(&order_b, &PK_MAKER_B)));

    let r = http_post(
        &format!("{base}/orders"),
        &json!({"order": order_json(&order_a), "signature": sig_a}),
    )
    .await;
    assert_eq!(r.0, 200, "A aceita pela borda: {}", r.1);
    let r = http_post(
        &format!("{base}/orders"),
        &json!({"order": order_json(&order_b), "signature": sig_b}),
    )
    .await;
    assert_eq!(r.0, 200, "B aceita pela borda: {}", r.1);

    let m = http_get(&format!("{base}/matches?maker=0x{}", hex::encode(maker_a))).await;
    let pairs: serde_json::Value = serde_json::from_str(&m.1).unwrap();
    assert_eq!(
        pairs.as_array().unwrap().len(),
        1,
        "o livro deveria informar 1 par"
    );

    let (_anvil_a, prov_a, wallet_a) = spawn_chain(CHAIN_A).await;
    let (_anvil_b, prov_b, wallet_b) = spawn_chain(CHAIN_B).await;
    let htlc_a = HashedTimelock::deploy(prov_a.clone()).await.unwrap();
    let htlc_b = HashedTimelock::deploy(prov_b.clone()).await.unwrap();

    let preimage = [0x42u8; 32];
    let hashlock = hashlock_from_preimage(&preimage);
    let hl = B256::from(hashlock);
    let amount = U256::from(1_000_000_000_000_000_000u128);
    let tl_a = U256::from(now_unix() + 7200); // longo em A
    let tl_b = U256::from(now_unix() + 3600); // curto em B

    // trava em A (recipiente = parte B)
    htlc_a
        .newSwap(wallet_b, Address::ZERO, amount, hl, tl_a)
        .value(amount)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();
    // trava em B (recipiente = parte A), mesmo hashlock
    htlc_b
        .newSwap(wallet_a, Address::ZERO, amount, hl, tl_b)
        .value(amount)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    let cid_b: B256 = htlc_b
        .computeContractId(wallet_b, wallet_a, Address::ZERO, amount, hl, tl_b)
        .call()
        .await
        .unwrap();
    htlc_b
        .redeem(cid_b, B256::from(preimage))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();
    let cid_a: B256 = htlc_a
        .computeContractId(wallet_a, wallet_b, Address::ZERO, amount, hl, tl_a)
        .call()
        .await
        .unwrap();
    htlc_a
        .redeem(cid_a, B256::from(preimage))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    // ===== ETAPA 3: o maestro correlaciona =====
    let mut tracker = SwapTracker::new();
    let head_a = prov_a.get_block_number().await.unwrap();
    let head_b = prov_b.get_block_number().await.unwrap();
    poll_into_tracker(&prov_a, *htlc_a.address(), CHAIN_A, 0, head_a, &mut tracker)
        .await
        .unwrap();
    poll_into_tracker(&prov_b, *htlc_b.address(), CHAIN_B, 0, head_b, &mut tracker)
        .await
        .unwrap();

    assert_eq!(
        tracker.correlated_hashlocks(),
        vec![hashlock],
        "swap correlacionado"
    );
    assert_eq!(
        tracker.preimage_for(&hashlock),
        Some(preimage),
        "preimage capturado"
    );

    let s = tracker.get(&hashlock).unwrap();
    assert!(
        s.legs.iter().all(|l| l.redeemed),
        "ambas as pernas resgatadas"
    );
}

async fn spawn_chain(
    chain_id: u64,
) -> (
    alloy::node_bindings::AnvilInstance,
    impl Provider + Clone,
    Address,
) {
    let anvil = Anvil::new().chain_id(chain_id).spawn();
    let signer: PrivateKeySigner = anvil.keys()[0].clone().into();
    let sender = signer.address();
    let wallet = EthereumWallet::from(signer);
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(anvil.endpoint_url());
    (anvil, provider, sender)
}

async fn http_post(url: &str, body: &serde_json::Value) -> (u16, String) {
    http_request("POST", url, Some(body)).await
}
async fn http_get(url: &str) -> (u16, String) {
    http_request("GET", url, None).await
}
async fn http_request(method: &str, url: &str, body: Option<&serde_json::Value>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").unwrap();
    let (host_port, path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    };
    let mut stream = tokio::net::TcpStream::connect(&host_port).await.unwrap();
    let body_str = body.map(|b| b.to_string()).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

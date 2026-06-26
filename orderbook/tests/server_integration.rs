//! consulta de pares de ponta a ponta.

use orderbook::eip712::sign;
use orderbook::order::Order;
use serde_json::json;

const PK_A: [u8; 32] = [0x11; 32];
const PK_B: [u8; 32] = [0x22; 32];

fn maker_address(pk: &[u8; 32]) -> [u8; 20] {
    use k256::ecdsa::SigningKey;
    use sha3::{Digest, Keccak256};
    let sk = SigningKey::from_slice(pk).unwrap();
    let vk = sk.verifying_key();
    let pt = vk.to_encoded_point(false);
    let h = Keccak256::digest(&pt.as_bytes()[1..]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    a
}

struct OrderSpec {
    maker: [u8; 20],
    sell_tok: u8,
    sell_chain: u64,
    sell_amt: u128,
    buy_tok: u8,
    buy_chain: u64,
    buy_amt: u128,
    nonce: u64,
}

fn build_order(spec: OrderSpec) -> Order {
    Order {
        maker: spec.maker,
        sell_token: [spec.sell_tok; 20],
        sell_chain_id: spec.sell_chain,
        sell_amount: spec.sell_amt,
        buy_token: [spec.buy_tok; 20],
        buy_chain_id: spec.buy_chain,
        buy_amount: spec.buy_amt,
        valid_until: 4_000_000_000,
        nonce: spec.nonce,
        created_at: 0, // ignorado no payload assinado
    }
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

async fn spawn_server() -> String {
    // Porta 0 = SO escolhe uma livre; descobrimos qual e devolvemos a base URL.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = orderbook::server::build_router(orderbook::server::AppState::new());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn submit_verifies_and_matches_are_reported() {
    let base = spawn_server().await;
    let client = SimpleClient;

    let maker_a = maker_address(&PK_A);
    let maker_b = maker_address(&PK_B);

    // A vende 100 de tok 0x11 (chain 1), quer 200 de tok 0x22 (chain 10)
    let a = build_order(OrderSpec {
        maker: maker_a,
        sell_tok: 0x11,
        sell_chain: 1,
        sell_amt: 100,
        buy_tok: 0x22,
        buy_chain: 10,
        buy_amt: 200,
        nonce: 1,
    });
    // B espelho: vende 200 de 0x22 (chain 10), quer 100 de 0x11 (chain 1)
    let b = build_order(OrderSpec {
        maker: maker_b,
        sell_tok: 0x22,
        sell_chain: 10,
        sell_amt: 200,
        buy_tok: 0x11,
        buy_chain: 1,
        buy_amt: 100,
        nonce: 2,
    });

    let sig_a = format!("0x{}", hex::encode(sign(&a, &PK_A)));
    let sig_b = format!("0x{}", hex::encode(sign(&b, &PK_B)));

    let r = client
        .post(
            &format!("{base}/orders"),
            &json!({"order": order_json(&a), "signature": sig_a}),
        )
        .await;
    assert_eq!(r.0, 200, "A deveria ser aceita: {}", r.1);

    let bad = client
        .post(
            &format!("{base}/orders"),
            &json!({"order": order_json(&b), "signature": sig_a}),
        )
        .await;
    assert_eq!(bad.0, 422, "wrong signature should be rejected: {}", bad.1);

    let r = client
        .post(
            &format!("{base}/orders"),
            &json!({"order": order_json(&b), "signature": sig_b}),
        )
        .await;
    assert_eq!(r.0, 200, "B deveria ser aceita: {}", r.1);

    let m = client
        .get(&format!("{base}/matches?maker=0x{}", hex::encode(maker_a)))
        .await;
    assert_eq!(m.0, 200);
    let pairs: serde_json::Value = serde_json::from_str(&m.1).unwrap();
    assert_eq!(
        pairs.as_array().unwrap().len(),
        1,
        "esperava 1 par, veio: {}",
        m.1
    );
}

struct SimpleClient;
impl SimpleClient {
    async fn post(&self, url: &str, body: &serde_json::Value) -> (u16, String) {
        self.request("POST", url, Some(body)).await
    }
    async fn get(&self, url: &str) -> (u16, String) {
        self.request("GET", url, None).await
    }
    async fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> (u16, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (host_port, path) = {
            let rest = url.strip_prefix("http://").unwrap();
            match rest.find('/') {
                Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
                None => (rest.to_string(), "/".to_string()),
            }
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
}

//! Teste end-to-end (Parte 3 + base da Parte 6).
//!
//! Sobe DUAS chains anvil locais, faz deploy do HTLC em cada, executa um swap
//! cross-chain completo (trava em A → trava em B com o MESMO hashlock → resgate
//! em B revela o preimage) e prova que o maestro:
//!   1. detecta as duas travas e as correlaciona pelo hashlock SHA-256;
//!   2. captura o preimage revelado.
//! Segundo teste: uma perna que expira é apanhada pelo watchdog.

use alloy::network::EthereumWallet;
use alloy::node_bindings::Anvil;
use alloy::primitives::{Address, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use maestro::hashlock_from_preimage;
use maestro::watcher::{poll_into_tracker, HashedTimelock};
use maestro::SwapTracker;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// Sobe um anvil com um dado chain_id e devolve (instância, provider-com-wallet,
/// endereço da carteira). Mantém a instância viva enquanto retornada.
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

#[tokio::test]
async fn cross_chain_swap_is_correlated_and_preimage_captured() {
    // --- duas chains independentes ---
    let (_anvil_a, prov_a, sender_a) = spawn_chain(1).await;
    let (_anvil_b, prov_b, sender_b) = spawn_chain(10).await;

    // --- deploy do HTLC em cada chain ---
    let htlc_a = HashedTimelock::deploy(prov_a.clone()).await.unwrap();
    let htlc_b = HashedTimelock::deploy(prov_b.clone()).await.unwrap();

    // --- termos do swap ---
    let preimage = [0x7u8; 32];
    let hashlock = hashlock_from_preimage(&preimage);
    let hashlock_b = B256::from(hashlock);
    let preimage_b = B256::from(preimage);
    let amount = U256::from(1_000_000_000_000_000_000u128); // 1 ETH
    // timelock longo em A, curto em B (assimetria correta do HTLC)
    let timelock_a = U256::from(now_unix() + 7200);
    let timelock_b = U256::from(now_unix() + 3600);

    // recipients (cross): em A o destinatário é a parte de B e vice-versa
    let recipient_a = sender_b;
    let recipient_b = sender_a;

    // --- perna A: trava ETH ---
    htlc_a
        .newSwap(recipient_a, Address::ZERO, amount, hashlock_b, timelock_a)
        .value(amount)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    // --- perna B: trava ETH com o MESMO hashlock ---
    htlc_b
        .newSwap(recipient_b, Address::ZERO, amount, hashlock_b, timelock_b)
        .value(amount)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    // contractId da perna B (para resgatar)
    let cid_b: B256 = htlc_b
        .computeContractId(sender_b, recipient_b, Address::ZERO, amount, hashlock_b, timelock_b)
        .call()
        .await
        .unwrap();

    // --- resgate em B revela o preimage no evento ---
    htlc_b
        .redeem(cid_b, preimage_b)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    // --- o maestro observa as duas chains ---
    let mut tracker = SwapTracker::new();
    let head_a = prov_a.get_block_number().await.unwrap();
    let head_b = prov_b.get_block_number().await.unwrap();
    let n_a = poll_into_tracker(&prov_a, *htlc_a.address(), 1, 0, head_a, &mut tracker)
        .await
        .unwrap();
    let n_b = poll_into_tracker(&prov_b, *htlc_b.address(), 10, 0, head_b, &mut tracker)
        .await
        .unwrap();

    // A teve 1 evento (newSwap); B teve 2 (newSwap + redeem)
    assert_eq!(n_a, 1, "esperava 1 evento na chain A");
    assert_eq!(n_b, 2, "esperava 2 eventos na chain B");

    // 1) as duas pernas correlacionadas pelo hashlock
    assert_eq!(
        tracker.correlated_hashlocks(),
        vec![hashlock],
        "o maestro deveria correlacionar as duas pernas pelo hashlock"
    );

    // 2) o preimage revelado em B foi capturado
    assert_eq!(
        tracker.preimage_for(&hashlock),
        Some(preimage),
        "o maestro deveria capturar o preimage revelado no resgate"
    );
}

#[tokio::test]
async fn watchdog_detects_expired_swap() {
    let (_anvil_a, prov_a, sender_a) = spawn_chain(1).await;
    let htlc_a = HashedTimelock::deploy(prov_a.clone()).await.unwrap();

    let preimage = [0x3u8; 32];
    let hashlock = hashlock_from_preimage(&preimage);
    let hashlock_b = B256::from(hashlock);
    let amount = U256::from(500_000_000_000_000_000u128);
    let timelock = now_unix() + 30; // curto

    // trava uma perna e NUNCA resgata
    htlc_a
        .newSwap(sender_a, Address::ZERO, amount, hashlock_b, U256::from(timelock))
        .value(amount)
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

    let mut tracker = SwapTracker::new();
    let head = prov_a.get_block_number().await.unwrap();
    poll_into_tracker(&prov_a, *htlc_a.address(), 1, 0, head, &mut tracker)
        .await
        .unwrap();

    // antes do prazo: nada expirado (now é parâmetro do watchdog)
    assert!(tracker.timed_out(timelock - 1).is_empty());
    // depois do prazo, sem resgate: o watchdog apanha
    let to = tracker.timed_out(timelock + 1);
    assert_eq!(to.len(), 1, "watchdog deveria detectar a perna expirada");
    assert_eq!(to[0].0, hashlock);
}

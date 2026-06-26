//! enviei (lock/redeem/refund), identificada pelo `tx_hash`.
//!

use crate::chain::{confirmations, ChainError};
use alloy::primitives::B256;
use alloy::providers::Provider;

pub async fn confirmations_of<P: Provider>(
    provider: &P,
    tx_hash: [u8; 32],
) -> Result<u64, ChainError> {
    let receipt = provider
        .get_transaction_receipt(B256::from(tx_hash))
        .await
        .map_err(|e| ChainError::Rpc(format!("{e}")))?;
    let block = match receipt.and_then(|r| r.block_number) {
        Some(b) => b,
        None => return Ok(0), // still in mempool or not mined
    };
    let head = provider
        .get_block_number()
        .await
        .map_err(|e| ChainError::Rpc(format!("{e}")))?;
    Ok(confirmations(head, block))
}

pub async fn is_confirmed<P: Provider>(
    provider: &P,
    tx_hash: [u8; 32],
    min_confirmations: u64,
) -> Result<bool, ChainError> {
    Ok(confirmations_of(provider, tx_hash).await? >= min_confirmations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::signer::Signer;
    use crate::exec::tx;
    use crate::verify::Address;
    use alloy::node_bindings::Anvil;
    use alloy::primitives::U256;
    use alloy::providers::ProviderBuilder;
    use alloy::rpc::types::TransactionRequest;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::time::{SystemTime, UNIX_EPOCH};

    const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[tokio::test]
    async fn depth_of_my_tx_matches_piece2_convention() {
        let anvil = Anvil::new().spawn();
        let signer = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .unwrap();
        let htlc = HashedTimelock::deploy(signer.provider().clone())
            .await
            .unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();

        // envio um lock real e pego o tx_hash.
        let me = signer.address();
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let locked = tx::lock(
            &signer,
            htlc_addr,
            me,
            [0u8; 20],
            500,
            hashlock,
            now_unix() + 3600,
        )
        .await
        .unwrap();

        let p = signer.provider();
        assert_eq!(confirmations_of(p, locked.tx_hash).await.unwrap(), 1);
        assert!(is_confirmed(p, locked.tx_hash, 1).await.unwrap());
        assert!(!is_confirmed(p, locked.tx_hash, 2).await.unwrap());

        let bump = TransactionRequest::default()
            .to(me.into())
            .value(U256::from(0));
        p.send_transaction(bump)
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();
        assert_eq!(confirmations_of(p, locked.tx_hash).await.unwrap(), 2);
        assert!(is_confirmed(p, locked.tx_hash, 2).await.unwrap());
    }

    #[tokio::test]
    async fn unknown_tx_has_zero_confirmations() {
        let anvil = Anvil::new().spawn();
        let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());
        assert_eq!(confirmations_of(&provider, [0xFFu8; 32]).await.unwrap(), 0);
        assert!(!is_confirmed(&provider, [0xFFu8; 32], 1).await.unwrap());
    }
}

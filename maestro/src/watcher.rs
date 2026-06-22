//! Observação on-chain (alloy): lê os logs do HTLC numa faixa de blocos,
//! decodifica os eventos e alimenta o [`SwapTracker`]. SEM chaves, SEM custódia.

use crate::correlate::SwapTracker;
use alloy::primitives::{Address, B256};
use alloy::providers::Provider;
use alloy::rpc::types::Filter;
use alloy::sol;
use alloy::sol_types::SolEvent;

// Interface do HTLC lida do artefato do Foundry (ABI + bytecode para deploy).
// O caminho é relativo à raiz do crate (CARGO_MANIFEST_DIR).
sol! {
    #[sol(rpc)]
    HashedTimelock,
    "../contracts/out/HashedTimelock.sol/HashedTimelock.json"
}

fn to32(b: B256) -> [u8; 32] {
    b.0
}

/// Lê os logs do contrato `address` na faixa `[from, to]`, decodifica e
/// alimenta o tracker. Retorna quantos eventos relevantes foram processados.
pub async fn poll_into_tracker<P: Provider>(
    provider: &P,
    address: Address,
    chain_id: u64,
    from_block: u64,
    to_block: u64,
    tracker: &mut SwapTracker,
) -> Result<usize, Box<dyn std::error::Error>> {
    let filter = Filter::new()
        .address(address)
        .from_block(from_block)
        .to_block(to_block);
    let logs = provider.get_logs(&filter).await?;

    let mut count = 0usize;
    for log in logs {
        let primitive = log.inner; // alloy_primitives::Log
        if let Ok(ev) = HashedTimelock::LogNewSwap::decode_log(&primitive) {
            tracker.on_new_swap(
                chain_id,
                to32(ev.contractId),
                to32(ev.hashlock),
                ev.timelock.to::<u64>(),
                ev.amount.to::<u128>(),
            );
            count += 1;
        } else if let Ok(ev) = HashedTimelock::LogRedeem::decode_log(&primitive) {
            tracker.on_redeem(chain_id, to32(ev.contractId), to32(ev.preimage));
            count += 1;
        } else if let Ok(ev) = HashedTimelock::LogRefund::decode_log(&primitive) {
            // o hashlock não vem no LogRefund; resolvemos pelo contractId já
            // conhecido no tracker (estado em memória, pequeno).
            if let Some(hashlock) = tracker.hashlock_of(chain_id, to32(ev.contractId)) {
                tracker.on_refund(chain_id, to32(ev.contractId), hashlock);
            }
            count += 1;
        }
    }
    Ok(count)
}

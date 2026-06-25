//! Peça 4 (F3): DESCOBERTA da perna oposta por HASHLOCK + leitura com profundidade.
//!
//! A trava da contraparte é descoberta LENDO A CHAIN (logs `LogNewSwap`) e
//! correlacionando por hashlock — a escolha trustless: não confiamos no que a
//! contraparte anuncia, lemos a verdade da chain. REUSO direto do maestro:
//! [`maestro::watcher::poll_into_tracker`] + [`maestro::SwapTracker`] já fazem a
//! correlação por hashlock E a extração do preimage revelado. Não reimplementamos
//! nada disso.
//!
//! JUNÇÃO CRÍTICA: o `contractId` descoberto pelo tracker é EXATAMENTE o que vai
//! para [`ChainVerifier::observe_lock`]. "Descobrir qual é a trava" e "verificar
//! se ela é segura" operam sobre o MESMO id — nunca verificamos uma trava e
//! agimos sobre outra.

use crate::chain::{ChainError, ChainVerifier, LockObservation};
use crate::verify::Address;
use alloy::primitives::Address as EvmAddress;
use alloy::providers::{DynProvider, Provider};
use maestro::SwapTracker;

/// Observador da perna oposta: descobre a trava por hashlock (via tracker do
/// maestro, alimentado por polling de logs) e a lê com profundidade pela
/// [`ChainVerifier`] (depth-gated).
///
/// CONFIANÇA: a descoberta por logs é, no MVP, específica de RPC (lê os logs do
/// nó). A LEITURA verificada passa pela `ChainVerifier` (substituível por SPV).
pub struct CounterpartyObserver<V> {
    verifier: V,
    /// provider da chain da CONTRAPARTE, para varrer os logs (read-only).
    provider: DynProvider,
    cp_htlc: Address,
    cp_chain_id: u64,
    tracker: SwapTracker,
    /// checkpoint do polling incremental (próximo bloco ainda não lido).
    next_block: u64,
}

impl<V: ChainVerifier> CounterpartyObserver<V> {
    pub fn new(verifier: V, provider: DynProvider, cp_htlc: Address, cp_chain_id: u64) -> Self {
        Self {
            verifier,
            provider,
            cp_htlc,
            cp_chain_id,
            tracker: SwapTracker::new(),
            next_block: 0,
        }
    }

    /// Lê os logs novos (do checkpoint até `head`) e os correlaciona no tracker
    /// do maestro — tal-e-qual, sem reimplementar a correlação por hashlock.
    pub async fn poll(&mut self) -> Result<(), ChainError> {
        let head = self
            .provider
            .get_block_number()
            .await
            .map_err(|e| ChainError::Rpc(format!("{e}")))?;
        if head < self.next_block {
            return Ok(()); // nada novo
        }
        maestro::watcher::poll_into_tracker(
            &self.provider,
            EvmAddress::from(self.cp_htlc),
            self.cp_chain_id,
            self.next_block,
            head,
            &mut self.tracker,
        )
        .await
        .map_err(|e| ChainError::Rpc(format!("{e}")))?;
        self.next_block = head + 1;
        Ok(())
    }

    /// O `contractId` da perna oposta descoberto por `hashlock` na chain da
    /// contraparte (`None` se ainda não vista). É o id que [`observe`](Self::observe)
    /// passa para `observe_lock`.
    pub fn discover_contract_id(&self, hashlock: &[u8; 32]) -> Option<[u8; 32]> {
        self.tracker
            .get(hashlock)?
            .legs
            .iter()
            .find(|l| l.chain_id == self.cp_chain_id)
            .map(|l| l.contract_id)
    }

    /// Descobre a trava por `hashlock` e a lê com profundidade `min_confirmations`.
    /// Se ainda não há trava descoberta → [`LockObservation::Absent`] (for_gate
    /// `None` → a máquina ESPERA). O `contractId` lido é o MESMO que o tracker
    /// descobriu (a junção descoberta→verificação).
    pub async fn observe(
        &mut self,
        hashlock: &[u8; 32],
        min_confirmations: u64,
    ) -> Result<LockObservation, ChainError> {
        self.poll().await?;
        match self.discover_contract_id(hashlock) {
            Some(cid) => self.verifier.observe_lock(self.cp_htlc, cid, min_confirmations).await,
            None => Ok(LockObservation::Absent),
        }
    }

    /// O segredo revelado on-chain por um resgate da contraparte (o MAKER aprende
    /// o preimage por aqui). Reuso direto do tracker do maestro.
    pub fn revealed_secret(&self, hashlock: &[u8; 32]) -> Option<[u8; 32]> {
        self.tracker.preimage_for(hashlock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::RpcVerifier;
    use alloy::network::EthereumWallet;
    use alloy::node_bindings::Anvil;
    use alloy::primitives::{Address as EvmAddr, B256, U256};
    use alloy::providers::ProviderBuilder;
    use alloy::signers::local::PrivateKeySigner;
    use maestro::hashlock_from_preimage;
    use maestro::watcher::HashedTimelock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_unix() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    // Sobe anvil + carteira, faz deploy do HTLC, e devolve o que os testes precisam.
    // NÃO devolve o binding (o tipo do instance gerado pelo sol! é interno); cada
    // teste reconstrói via `HashedTimelock::new(addr, provider)` (tipo inferido).
    async fn setup() -> (alloy::node_bindings::AnvilInstance, DynProvider, Address, u64, EvmAddr) {
        let anvil = Anvil::new().spawn();
        let pk: PrivateKeySigner = anvil.keys()[0].clone().into();
        let sender = pk.address();
        let wallet = EthereumWallet::from(pk);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(anvil.endpoint_url())
            .erased();
        let htlc = HashedTimelock::deploy(provider.clone()).await.unwrap();
        let htlc_addr: Address = (*htlc.address()).into_array();
        let chain_id = provider.get_chain_id().await.unwrap();
        (anvil, provider, htlc_addr, chain_id, sender)
    }

    // F3 + JUNÇÃO: o contractId descoberto por hashlock é o da trava REAL, e é o
    // mesmo que observe_lock lê. "qual trava é" == "a trava verificada".
    #[tokio::test]
    async fn discovers_lock_by_hashlock_and_observes_the_same_one() {
        let (anvil, provider, htlc_addr, chain_id, sender) = setup().await;
        let htlc = HashedTimelock::new(EvmAddr::from(htlc_addr), provider.clone());

        let me = [0x7Au8; 20];
        let amount: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        htlc.newSwap(
            EvmAddr::from(me),
            EvmAddr::ZERO,
            U256::from(amount),
            B256::from(hashlock),
            U256::from(timelock),
        )
        .value(U256::from(amount))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

        // o contractId que o CONTRATO gera p/ essa trava.
        let expected_cid: B256 = htlc
            .computeContractId(
                sender,
                EvmAddr::from(me),
                EvmAddr::ZERO,
                U256::from(amount),
                B256::from(hashlock),
                U256::from(timelock),
            )
            .call()
            .await
            .unwrap();

        let verifier = RpcVerifier::new(&anvil.endpoint()).unwrap();
        let poll_provider = ProviderBuilder::new()
            .connect_http(anvil.endpoint_url())
            .erased();
        let mut obs = CounterpartyObserver::new(verifier, poll_provider, htlc_addr, chain_id);

        // observe: descobre por hashlock + lê com profundidade 1 → Confirmed.
        let observed = match obs.observe(&hashlock, 1).await.unwrap() {
            LockObservation::Confirmed(o) => o,
            other => panic!("esperava Confirmed, veio {other:?}"),
        };
        assert_eq!(observed.hashlock, hashlock);
        assert_eq!(observed.amount, amount);
        assert_eq!(observed.recipient, me);

        // A JUNÇÃO: o contractId descoberto == o do contrato == o que foi lido.
        let discovered = obs.discover_contract_id(&hashlock).expect("descoberto");
        assert_eq!(
            discovered, expected_cid.0,
            "o contractId descoberto por hashlock é o da trava REAL (descoberta == verificação)"
        );

        // hashlock inexistente → Absent (for_gate None → a máquina espera).
        let absent = obs.observe(&[0x00u8; 32], 1).await.unwrap();
        assert_eq!(absent, LockObservation::Absent);
        assert_eq!(absent.for_gate(), None);
    }

    // O MAKER aprende o segredo revelado por um resgate — reuso do tracker.
    #[tokio::test]
    async fn learns_revealed_secret_after_redeem() {
        let (anvil, provider, htlc_addr, chain_id, sender) = setup().await;
        let htlc = HashedTimelock::new(EvmAddr::from(htlc_addr), provider.clone());

        let me = [0x7Au8; 20];
        let amount: u128 = 500;
        let preimage = [0x42u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);
        let timelock = now_unix() + 3600;

        htlc.newSwap(
            EvmAddr::from(me),
            EvmAddr::ZERO,
            U256::from(amount),
            B256::from(hashlock),
            U256::from(timelock),
        )
        .value(U256::from(amount))
        .send()
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();

        let cid: B256 = htlc
            .computeContractId(
                sender,
                EvmAddr::from(me),
                EvmAddr::ZERO,
                U256::from(amount),
                B256::from(hashlock),
                U256::from(timelock),
            )
            .call()
            .await
            .unwrap();

        let verifier = RpcVerifier::new(&anvil.endpoint()).unwrap();
        let poll_provider = ProviderBuilder::new()
            .connect_http(anvil.endpoint_url())
            .erased();
        let mut obs = CounterpartyObserver::new(verifier, poll_provider, htlc_addr, chain_id);

        // antes do resgate: nenhum segredo revelado.
        obs.poll().await.unwrap();
        assert!(obs.revealed_secret(&hashlock).is_none());

        // resgate (revela o preimage no LogRedeem).
        htlc.redeem(cid, B256::from(preimage))
            .send()
            .await
            .unwrap()
            .get_receipt()
            .await
            .unwrap();

        // novo poll → o tracker captura o preimage revelado.
        obs.poll().await.unwrap();
        assert_eq!(
            obs.revealed_secret(&hashlock),
            Some(preimage),
            "o maker aprende o segredo revelado on-chain"
        );
    }
}

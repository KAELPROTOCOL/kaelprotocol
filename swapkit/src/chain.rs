//! Leitura da outra chain — a fronteira de confiança do MVP.
//!
//! `verify_counterparty_leg` consome um [`ObservedLock`]; ESTE módulo é quem o
//! produz, lendo a trava HTLC na chain alvo. A leitura fica atrás da interface
//! [`ChainVerifier`] para que uma implementação SPV/light-client possa
//! substituir o RPC depois, SEM reescrever a carteira.
//!
//! HONESTIDADE SOBRE CONFIANÇA:
//! - [`RpcVerifier`] (a impl do MVP) é **trust-minimized, NÃO trustless**: ela
//!   confia que o nó RPC reporta o estado real da chain. Um nó malicioso poderia
//!   mentir sobre a existência/parâmetros de uma trava.
//! - Uma futura impl SPV/light-client (verificando provas de inclusão contra
//!   cabeçalhos de bloco) seria trustless. Essa é a evolução prevista da
//!   fronteira de confiança — a interface existe justamente para permiti-la.

use crate::verify::{Address, ObservedLock};

/// Erros ao ler a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// URL de RPC inválida.
    BadUrl(String),
    /// falha de rede / RPC (timeout, conexão, erro do nó).
    Rpc(String),
    /// resposta malformada / não-decodificável.
    Decode(String),
    /// o amount on-chain (uint256) não cabe em u128.
    AmountOverflow,
    /// o timelock on-chain (uint256) não cabe em u64.
    TimelockOverflow,
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ChainError {}

/// Interface de leitura de chain. Produz o [`ObservedLock`] que a verificação
/// consome.
///
/// CONFIANÇA: implementações via RPC (como [`RpcVerifier`]) são
/// **trust-minimized** — confiam que o nó diz a verdade. Uma impl SPV/light-client
/// seria **trustless**. Esta interface é a costura que permite trocar uma pela
/// outra sem mexer no resto da carteira. É a fronteira de confiança do MVP.
#[allow(async_fn_in_trait)]
pub trait ChainVerifier {
    /// Lê a trava HTLC identificada por `contract_id` no contrato `htlc_address`
    /// da chain alvo. Retorna o [`ObservedLock`] (`exists=false` se não houver
    /// trava ativa). Erros de leitura viram [`ChainError`].
    async fn observe_lock(
        &self,
        htlc_address: Address,
        contract_id: [u8; 32],
    ) -> Result<ObservedLock, ChainError>;
}

/// Espelho da struct `Swap` do HashedTimelock, já decodificada para tipos Rust.
/// Existir como tipo próprio permite testar o MAPEAMENTO sem rede.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSwap {
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: u128,
    pub hashlock: [u8; 32],
    pub timelock: u64,
    pub withdrawn: bool,
    pub refunded: bool,
}

/// Mapeamento PURO Swap → ObservedLock.
///
/// MODELAGEM DE `exists`: reportamos `exists = true` apenas para uma trava
/// **ATIVA e resgatável** — presente on-chain (`sender != 0`) E ainda não
/// `withdrawn` nem `refunded`. Justificativa: a verificação serve para decidir
/// se vale agir contra a perna oposta; uma trava já resgatada ou reembolsada
/// não é algo em que a contraparte possa confiar para resgate, então tratá-la
/// como "inexistente" (`exists=false → LockNotFound`) é o comportamento seguro.
pub fn observed_from_swap(s: &RawSwap) -> ObservedLock {
    let active = s.sender != [0u8; 20] && !s.withdrawn && !s.refunded;
    ObservedLock {
        hashlock: s.hashlock,
        token: s.token,
        amount: s.amount,
        recipient: s.recipient,
        timelock: s.timelock,
        sender: s.sender,
        exists: active,
    }
}

// ------------------------------------------------------------------------
// Implementação RPC (alloy). Trust-minimized: confia no nó.
// ------------------------------------------------------------------------

use alloy::primitives::{Address as EvmAddress, B256, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::sol;

// Interface mínima do HashedTimelock para LER a trava (só a view getSwap).
sol! {
    #[sol(rpc)]
    interface IHashedTimelock {
        struct Swap {
            address sender;
            address recipient;
            address token;
            uint256 amount;
            bytes32 hashlock;
            uint256 timelock;
            bool withdrawn;
            bool refunded;
        }
        function getSwap(bytes32 contractId) external view returns (Swap memory);
    }
}

/// Leitor de chain via RPC EVM (alloy).
///
/// TRUST-MINIMIZED, NÃO TRUSTLESS: confia que o endpoint RPC reporta o estado
/// real da chain. Substituível por uma impl SPV/light-client trustless.
pub struct RpcVerifier {
    provider: DynProvider,
}

impl RpcVerifier {
    /// Conecta a um endpoint HTTP RPC. Não faz I/O ainda (HTTP é preguiçoso).
    pub fn new(rpc_url: &str) -> Result<Self, ChainError> {
        let url = rpc_url
            .parse()
            .map_err(|e| ChainError::BadUrl(format!("{e}")))?;
        let provider = ProviderBuilder::new().connect_http(url).erased();
        Ok(Self { provider })
    }
}

fn u256_to_u128(v: U256) -> Result<u128, ChainError> {
    u128::try_from(v).map_err(|_| ChainError::AmountOverflow)
}
fn u256_to_u64(v: U256) -> Result<u64, ChainError> {
    u64::try_from(v).map_err(|_| ChainError::TimelockOverflow)
}

impl ChainVerifier for RpcVerifier {
    async fn observe_lock(
        &self,
        htlc_address: Address,
        contract_id: [u8; 32],
    ) -> Result<ObservedLock, ChainError> {
        let htlc = IHashedTimelock::new(EvmAddress::from(htlc_address), &self.provider);
        let swap = htlc
            .getSwap(B256::from(contract_id))
            .call()
            .await
            .map_err(|e| ChainError::Rpc(format!("{e}")))?;

        let raw = RawSwap {
            sender: swap.sender.into_array(),
            recipient: swap.recipient.into_array(),
            token: swap.token.into_array(),
            amount: u256_to_u128(swap.amount)?,
            hashlock: swap.hashlock.0,
            timelock: u256_to_u64(swap.timelock)?,
            withdrawn: swap.withdrawn,
            refunded: swap.refunded,
        };
        Ok(observed_from_swap(&raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{verify_counterparty_leg, LegExpectation, Role, UnsafeReason, VerifyOutcome};

    fn h(b: u8) -> [u8; 32] {
        [b; 32]
    }
    fn a(b: u8) -> [u8; 20] {
        [b; 20]
    }

    fn active_swap() -> RawSwap {
        RawSwap {
            sender: a(0x3A), // maker travou
            recipient: a(0x7A), // paga o taker
            token: a(0x22),
            amount: 500,
            hashlock: h(0xAB),
            timelock: 1800,
            withdrawn: false,
            refunded: false,
        }
    }

    // --- mapeamento: campos certos para uma trava ativa ---
    #[test]
    fn maps_active_swap_fields() {
        let o = observed_from_swap(&active_swap());
        assert!(o.exists);
        assert_eq!(o.hashlock, h(0xAB));
        assert_eq!(o.token, a(0x22));
        assert_eq!(o.amount, 500);
        assert_eq!(o.recipient, a(0x7A));
        assert_eq!(o.timelock, 1800);
        assert_eq!(o.sender, a(0x3A));
    }

    // --- exists = false: trava inexistente (sender zero) ---
    #[test]
    fn nonexistent_swap_is_not_exists() {
        let mut s = active_swap();
        s.sender = [0u8; 20]; // getSwap de um id inexistente devolve struct zerada
        let o = observed_from_swap(&s);
        assert!(!o.exists);
    }

    // --- exists = false: já resgatada ---
    #[test]
    fn withdrawn_swap_is_not_exists() {
        let mut s = active_swap();
        s.withdrawn = true;
        assert!(!observed_from_swap(&s).exists);
    }

    // --- exists = false: já reembolsada ---
    #[test]
    fn refunded_swap_is_not_exists() {
        let mut s = active_swap();
        s.refunded = true;
        assert!(!observed_from_swap(&s).exists);
    }

    // --- JUNÇÃO parsing → verificação: ObservedLock montado de uma resposta
    //     RPC mockada alimenta verify_counterparty_leg corretamente. ---
    #[test]
    fn parsed_lock_feeds_verification_safe() {
        // resposta RPC mockada (a perna do maker, vista pelo taker)
        let observed = observed_from_swap(&active_swap());

        // o taker espera: mesmo H, token/amount da compra, ele como recipient,
        // sua perna longa (2000) vs a do maker (1800), gap 100 → Safe.
        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            role: Role::Taker,
        };
        assert_eq!(verify_counterparty_leg(&exp, &observed), VerifyOutcome::Safe);
    }

    #[test]
    fn parsed_lock_feeds_verification_unsafe() {
        // mesma trava, mas o hashlock observado difere do esperado → Unsafe.
        let mut s = active_swap();
        s.hashlock = h(0x01);
        let observed = observed_from_swap(&s);

        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            role: Role::Taker,
        };
        assert_eq!(
            verify_counterparty_leg(&exp, &observed),
            VerifyOutcome::Unsafe(UnsafeReason::HashlockMismatch)
        );
    }

    // --- montagem de uma trava resgatada que vira LockNotFound na verificação ---
    #[test]
    fn withdrawn_lock_verifies_as_lock_not_found() {
        let mut s = active_swap();
        s.withdrawn = true;
        let observed = observed_from_swap(&s); // exists = false
        let exp = LegExpectation {
            expected_hashlock: h(0xAB),
            expected_token: a(0x22),
            expected_amount: 500,
            expected_recipient: a(0x7A),
            my_timelock: 2000,
            min_gap: 100,
            role: Role::Taker,
        };
        assert_eq!(
            verify_counterparty_leg(&exp, &observed),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }

    // ----------------------------------------------------------------
    // PENDENTE: integração REAL contra uma chain (anvil). Os testes acima
    // provam o PARSING/MONTAGEM e a junção com a verificação — NÃO provam a
    // leitura real de um nó. Esta prova final exige uma chain de verdade
    // (subir anvil, fazer deploy do HTLC, criar um swap, e ler via RpcVerifier).
    // Deixado como stub explícito, ignorado, a ser implementado depois.
    // ----------------------------------------------------------------
    #[tokio::test]
    #[ignore = "integração real: requer anvil + HTLC com deploy + swap on-chain"]
    async fn rpc_verifier_against_real_chain_pending() {
        // Esboço do que esta prova faria (NÃO implementado agora):
        //   1. subir anvil; deploy do HashedTimelock; newSwap(...) → contractId.
        //   2. let v = RpcVerifier::new(&anvil.endpoint()).unwrap();
        //   3. let obs = v.observe_lock(htlc_addr, contract_id).await.unwrap();
        //   4. assert!(obs.exists && obs.hashlock == ... && obs.amount == ...);
        // Até isso existir, NÃO afirmamos que a leitura real de chain funciona.
        let _ = RpcVerifier::new("http://127.0.0.1:8545").unwrap();
    }
}

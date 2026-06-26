//!
//! Sem keystore cifrado, sem senha, sem HSM/KMS, sem hardware wallet, sem
//! fora do MVP.
//!

use crate::verify::Address;
use alloy::network::EthereumWallet;
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;

pub const ENV_KEY: &str = "KAEL_SIGNER_KEY";

/// Chain-ids de TESTE explicitamente liberados (allowlist, safe-by-default).
///
pub const ALLOWED_TEST_CHAINS: &[u64] = &[
    31337,    // anvil / hardhat (default)
    31338,    // second local anvil for closed developer testnet
    1337,     // geth/ganache --dev
    11155111, // Sepolia
    17000,    // Holesky
    11155420, // OP Sepolia
    84532,    // Base Sepolia
    421614,   // Arbitrum Sepolia
    80002,    // Polygon Amoy
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerError {
    MissingKey,
    BadKey(String),
    BadUrl(String),
    Rpc(String),
    MainnetForbidden { chain_id: u64 },
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignerError::MissingKey => write!(f, "variable {ENV_KEY} is not set"),
            SignerError::BadKey(e) => write!(f, "invalid key: {e}"),
            SignerError::BadUrl(e) => write!(f, "invalid RPC URL: {e}"),
            SignerError::Rpc(e) => write!(f, "failed to query chain-id: {e}"),
            SignerError::MainnetForbidden { chain_id } => write!(
                f,
                "REFUSED: chain-id {chain_id} is outside the test allowlist: \
                 the executor does not sign where real value could be touched"
            ),
        }
    }
}
impl std::error::Error for SignerError {}

pub struct Signer {
    provider: DynProvider,
    address: Address,
    chain_id: u64,
}

impl Signer {
    pub async fn from_env(rpc_url: &str) -> Result<Self, SignerError> {
        let raw = std::env::var(ENV_KEY).map_err(|_| SignerError::MissingKey)?;
        Self::from_key_str(&raw, rpc_url).await
    }

    pub async fn from_key_str(key_hex: &str, rpc_url: &str) -> Result<Self, SignerError> {
        let signer: PrivateKeySigner = key_hex
            .trim()
            .strip_prefix("0x")
            .unwrap_or(key_hex.trim())
            .parse()
            .map_err(|e| SignerError::BadKey(format!("{e}")))?;
        let address = signer.address().into_array();

        let url = rpc_url
            .parse()
            .map_err(|e| SignerError::BadUrl(format!("{e}")))?;
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(url)
            .erased();

        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|e| SignerError::Rpc(format!("{e}")))?;

        assert_chain_allowed(chain_id)?;

        Ok(Self {
            provider,
            address,
            chain_id,
        })
    }

    pub fn address(&self) -> Address {
        self.address
    }

    /// A chain liberada onde este assinante opera.
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn provider(&self) -> &DynProvider {
        &self.provider
    }
}

/// sem rede.
pub fn assert_chain_allowed(chain_id: u64) -> Result<(), SignerError> {
    if ALLOWED_TEST_CHAINS.contains(&chain_id) {
        Ok(())
    } else {
        Err(SignerError::MainnetForbidden { chain_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::node_bindings::Anvil;
    use tokio::sync::Mutex;

    const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const ANVIL_ADDR0: [u8; 20] = [
        0xf3, 0x9F, 0xd6, 0xe5, 0x1a, 0xad, 0x88, 0xF6, 0xF4, 0xce, 0x6a, 0xB8, 0x82, 0x72, 0x79,
        0xcf, 0xff, 0xb9, 0x22, 0x66,
    ];

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    // ---------------- o GUARD, puro (sem rede) ----------------

    #[test]
    fn allowlist_admits_known_test_chains() {
        assert!(assert_chain_allowed(31337).is_ok()); // anvil
        assert!(assert_chain_allowed(31338).is_ok()); // second local anvil
        assert!(assert_chain_allowed(11155111).is_ok()); // sepolia
    }

    #[test]
    fn allowlist_rejects_mainnets_and_unknowns() {
        for id in [1u64, 10, 56, 137, 8453, 42161, 43114] {
            assert_eq!(
                assert_chain_allowed(id),
                Err(SignerError::MainnetForbidden { chain_id: id }),
                "chain-id {id} jamais pode passar"
            );
        }
        assert_eq!(
            assert_chain_allowed(999_999),
            Err(SignerError::MainnetForbidden { chain_id: 999_999 })
        );
    }

    #[tokio::test]
    async fn bad_key_is_rejected_before_any_network() {
        let r = Signer::from_key_str("not-hex", "http://127.0.0.1:1").await;
        assert!(
            matches!(r.as_ref().err(), Some(SignerError::BadKey(_))),
            "esperava BadKey, veio outra coisa"
        );
    }

    #[tokio::test]
    async fn from_key_str_ok_on_allowed_chain() {
        let anvil = Anvil::new().spawn(); // chain-id default = 31337 (na allowlist)
        let s = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .expect("31337 is in the allowlist: should build");
        assert_eq!(s.chain_id(), 31337);
        assert_eq!(
            s.address(),
            ANVIL_ADDR0,
            "address derived from anvil key #0"
        );
    }

    #[tokio::test]
    async fn accepts_0x_prefixed_key() {
        let anvil = Anvil::new().spawn();
        let prefixed = format!("0x{ANVIL_KEY0}");
        let s = Signer::from_key_str(&prefixed, &anvil.endpoint())
            .await
            .unwrap();
        assert_eq!(s.address(), ANVIL_ADDR0);
    }

    // ---------------- forbidden chain: Err, never Signer ----------------

    #[tokio::test]
    async fn forbidden_chain_yields_err_never_signer() {
        // Local Anvil pretending to be chain-id 1 (Ethereum L1 mainnet).
        let anvil = Anvil::new().chain_id(1).spawn();
        let r = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint()).await;
        assert_eq!(
            r.err(),
            Some(SignerError::MainnetForbidden { chain_id: 1 }),
            "chain-id 1 must abort construction: never a Signer"
        );
    }

    #[tokio::test]
    async fn from_env_missing_then_present() {
        let _g = ENV_LOCK.lock().await;

        // missing: MissingKey
        std::env::remove_var(ENV_KEY);
        let r = Signer::from_env("http://127.0.0.1:1").await;
        assert_eq!(r.err(), Some(SignerError::MissingKey));

        // present plus allowed chain: Ok
        let anvil = Anvil::new().spawn();
        std::env::set_var(ENV_KEY, ANVIL_KEY0);
        let s = Signer::from_env(&anvil.endpoint()).await.unwrap();
        assert_eq!(s.address(), ANVIL_ADDR0);

        std::env::remove_var(ENV_KEY);
    }

    #[tokio::test]
    async fn from_env_forbidden_chain_yields_err() {
        let _g = ENV_LOCK.lock().await;

        let anvil = Anvil::new().chain_id(1).spawn();
        std::env::set_var(ENV_KEY, ANVIL_KEY0);
        let r = Signer::from_env(&anvil.endpoint()).await;
        std::env::remove_var(ENV_KEY);

        assert_eq!(r.err(), Some(SignerError::MainnetForbidden { chain_id: 1 }));
    }
}

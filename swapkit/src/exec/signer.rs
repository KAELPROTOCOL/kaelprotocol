//! Peça 1 do executor: a CHAVE que assina + o GUARD anti-mainnet.
//!
//! NÍVEL DE SEGURANÇA (honesto): a chave entra por variável de ambiente em
//! TEXTO PURO (`KAEL_SIGNER_KEY`). Quem lê a env, o arquivo ou a memória do
//! processo é dono dos fundos. Isso só é aceitável porque o código se RECUSA
//! FISICAMENTE a rodar onde tocaria valor real — ver [`assert_chain_allowed`].
//! Sem keystore cifrado, sem senha, sem HSM/KMS, sem hardware wallet, sem
//! rotação, sem chave por chain. Tudo isso é trabalho futuro, explicitamente
//! fora do MVP.
//!
//! O GUARD é INEGOCIÁVEL e usa **allowlist** (safe-by-default): só chain-ids de
//! TESTE explicitamente liberados passam. Uma chain desconhecida é RECUSADA por
//! padrão — o guard não precisa "conhecer" os mainnets, só os ambientes onde
//! tocar valor é seguro. Construir um [`Signer`] numa chain fora da allowlist é
//! IMPOSSÍVEL: o guard roda dentro da construção, antes de devolver o `Signer`.

use crate::verify::Address;
use alloy::network::EthereumWallet;
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;

/// Variável de ambiente de onde a chave privada (hex) é lida.
pub const ENV_KEY: &str = "KAEL_SIGNER_KEY";

/// Chain-ids de TESTE explicitamente liberados (allowlist, safe-by-default).
///
/// Qualquer chain-id FORA desta lista é recusado com [`SignerError::MainnetForbidden`].
/// É deliberadamente uma allowlist, não uma denylist: não dependemos de prever
/// todo mainnet — o desconhecido já é barrado. Para liberar um novo ambiente de
/// teste, adicione o id AQUI, conscientemente.
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

/// Erros ao carregar a chave / inicializar o assinante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerError {
    /// a variável de ambiente da chave não está definida.
    MissingKey,
    /// a chave não é um hex secp256k1 válido.
    BadKey(String),
    /// URL de RPC inválida.
    BadUrl(String),
    /// falha de rede ao consultar o chain-id do nó.
    Rpc(String),
    /// O GUARD disparou: a chain não está na allowlist de teste. NUNCA assinamos
    /// aqui — pode ser um mainnet (valor real).
    MainnetForbidden { chain_id: u64 },
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignerError::MissingKey => write!(f, "variável {ENV_KEY} não definida"),
            SignerError::BadKey(e) => write!(f, "chave inválida: {e}"),
            SignerError::BadUrl(e) => write!(f, "URL de RPC inválida: {e}"),
            SignerError::Rpc(e) => write!(f, "falha ao consultar chain-id: {e}"),
            SignerError::MainnetForbidden { chain_id } => write!(
                f,
                "RECUSADO: chain-id {chain_id} fora da allowlist de teste — \
                 o executor não assina onde poderia tocar valor real"
            ),
        }
    }
}
impl std::error::Error for SignerError {}

/// O assinante do executor: a chave (via `EthereumWallet`) + o provider já
/// conectado à chain liberada. Construído SÓ por caminhos que passam pelo guard.
pub struct Signer {
    provider: DynProvider,
    address: Address,
    chain_id: u64,
}

impl Signer {
    /// Carrega a chave da env [`ENV_KEY`], conecta em `rpc_url`, e SÓ devolve um
    /// `Signer` se a chain estiver na allowlist. Caso contrário, `Err`.
    pub async fn from_env(rpc_url: &str) -> Result<Self, SignerError> {
        let raw = std::env::var(ENV_KEY).map_err(|_| SignerError::MissingKey)?;
        Self::from_key_str(&raw, rpc_url).await
    }

    /// Como [`from_env`](Self::from_env), mas recebe a chave direto (testável sem
    /// mexer no ambiente do processo). É o caminho que faz o trabalho de verdade.
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

        // I/O: pergunta ao nó em que chain estamos.
        let chain_id = provider
            .get_chain_id()
            .await
            .map_err(|e| SignerError::Rpc(format!("{e}")))?;

        // GUARD — antes de QUALQUER assinatura, antes de devolver o Signer.
        assert_chain_allowed(chain_id)?;

        Ok(Self {
            provider,
            address,
            chain_id,
        })
    }

    /// O endereço que esta chave controla (= recipient da minha perna p/ a contraparte).
    pub fn address(&self) -> Address {
        self.address
    }

    /// A chain liberada onde este assinante opera.
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// O provider já com a carteira — usado pelas peças de tx (lock/redeem/refund).
    pub fn provider(&self) -> &DynProvider {
        &self.provider
    }
}

/// O GUARD INEGOCIÁVEL. `Ok` só para chain-ids na [`ALLOWED_TEST_CHAINS`];
/// qualquer outro → [`SignerError::MainnetForbidden`]. Puro e síncrono — testável
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

    // Chave #0 determinística do anvil (mnemônico default) e seu endereço.
    // A chave só precisa ser VÁLIDA p/ os testes — não precisa de saldo.
    const ANVIL_KEY0: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    const ANVIL_ADDR0: [u8; 20] = [
        0xf3, 0x9F, 0xd6, 0xe5, 0x1a, 0xad, 0x88, 0xF6, 0xF4, 0xce, 0x6a, 0xB8, 0x82, 0x72, 0x79,
        0xcf, 0xff, 0xb9, 0x22, 0x66,
    ];

    // set_var/remove_var é global ao processo; serializa os testes que tocam a env.
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
        // mainnets conhecidos — todos RECUSADOS por NÃO estarem na allowlist
        for id in [1u64, 10, 56, 137, 8453, 42161, 43114] {
            assert_eq!(
                assert_chain_allowed(id),
                Err(SignerError::MainnetForbidden { chain_id: id }),
                "chain-id {id} jamais pode passar"
            );
        }
        // e um id desconhecido qualquer também — é o ponto do safe-by-default
        assert_eq!(
            assert_chain_allowed(999_999),
            Err(SignerError::MainnetForbidden { chain_id: 999_999 })
        );
    }

    // ---------------- chave inválida (sem rede) ----------------

    #[tokio::test]
    async fn bad_key_is_rejected_before_any_network() {
        // URL impossível de conectar: se a chave fosse lida depois da rede, isso
        // viraria BadUrl/Rpc. Vem BadKey → a validação da chave é a primeira coisa.
        let r = Signer::from_key_str("não-é-hex", "http://127.0.0.1:1").await;
        assert!(
            matches!(r.as_ref().err(), Some(SignerError::BadKey(_))),
            "esperava BadKey, veio outra coisa"
        );
    }

    // ---------------- caminho feliz (anvil na allowlist) ----------------

    #[tokio::test]
    async fn from_key_str_ok_on_allowed_chain() {
        let anvil = Anvil::new().spawn(); // chain-id default = 31337 (na allowlist)
        let s = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint())
            .await
            .expect("31337 está na allowlist → deve construir");
        assert_eq!(s.chain_id(), 31337);
        assert_eq!(
            s.address(),
            ANVIL_ADDR0,
            "endereço derivado da chave #0 do anvil"
        );
    }

    // aceita a chave com prefixo 0x também.
    #[tokio::test]
    async fn accepts_0x_prefixed_key() {
        let anvil = Anvil::new().spawn();
        let prefixed = format!("0x{ANVIL_KEY0}");
        let s = Signer::from_key_str(&prefixed, &anvil.endpoint())
            .await
            .unwrap();
        assert_eq!(s.address(), ANVIL_ADDR0);
    }

    // ---------------- O TESTE QUE IMPORTA: chain proibida → Err, NUNCA Signer ----------------

    #[tokio::test]
    async fn forbidden_chain_yields_err_never_signer() {
        // anvil LOCAL, mas se passando pelo chain-id 1 (Ethereum L1 mainnet).
        let anvil = Anvil::new().chain_id(1).spawn();
        let r = Signer::from_key_str(ANVIL_KEY0, &anvil.endpoint()).await;
        assert_eq!(
            r.err(),
            Some(SignerError::MainnetForbidden { chain_id: 1 }),
            "chain-id 1 deve ABORTAR a construção — jamais um Signer"
        );
    }

    // ---------------- from_env: lê a env e aplica o mesmo guard ----------------

    #[tokio::test]
    async fn from_env_missing_then_present() {
        let _g = ENV_LOCK.lock().await;

        // ausente → MissingKey
        std::env::remove_var(ENV_KEY);
        let r = Signer::from_env("http://127.0.0.1:1").await;
        assert_eq!(r.err(), Some(SignerError::MissingKey));

        // presente + chain liberada → Ok
        let anvil = Anvil::new().spawn();
        std::env::set_var(ENV_KEY, ANVIL_KEY0);
        let s = Signer::from_env(&anvil.endpoint()).await.unwrap();
        assert_eq!(s.address(), ANVIL_ADDR0);

        std::env::remove_var(ENV_KEY);
    }

    // from_env numa chain proibida também aborta (o guard não depende da fonte da chave).
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

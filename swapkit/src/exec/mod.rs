//! O EXECUTOR — a camada que REALIZA no mundo o que a máquina de estados DECIDE.
//!
//! O núcleo puro (`verify`, `sm`, `handshake`) decide; este módulo assina e envia
//! transações, lê confirmações e dirige o laço. Tudo que tem efeito colateral
//! (chaves, rede, relógio) vive AQUI, isolado em submódulos — o núcleo continua
//! puro e testável sem mundo.
//!
//! Construído em peças testáveis, na ordem de dependência:
//! 1. [`signer`] — chave + guard allowlist (esta peça).
//! 2. interface `observe_lock` com `min_confirmations` (próxima).
//! 3. `tx` — lock/redeem/refund.
//! 4. `observe` + `confirm` — descoberta por hashlock (maestro) + profundidade N.
//! 5. `mod` — o laço + a re-verificação no último instante (anti-TOCTOU).

pub mod confirm;
pub mod observe;
pub mod signer;
pub mod tx;

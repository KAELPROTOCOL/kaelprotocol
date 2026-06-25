//! handshake.rs — atribuição DETERMINÍSTICA de papéis (Taker/Maker) e derivação
//! do [`SwapContext`] a partir de um par de ordens casadas. PURO: sem rede, sem
//! relógio, sem rng. Tudo do mundo entra por parâmetro.
//!
//! ## Regra de papéis (decidida)
//! A ordem em REPOUSO (menor `created_at`) é o **MAKER** — provedor de liquidez,
//! papel protegido, timelock CURTO, responde. A que CRUZOU (maior `created_at`)
//! é o **TAKER** — gera o segredo, trava PRIMEIRO, timelock LONGO, e carrega o
//! free-option. Empate de tempo → desempate por DADO ASSINADO (imune ao
//! servidor): **menor digest EIP-712 = MAKER**. Rodando a MESMA função com os
//! MESMOS dados, os dois lados derivam papéis COMPLEMENTARES, sem trocar mensagem.
//!
//! ## Segurança sob divergência (sem perda de fundos)
//! Se um servidor reportar `created_at` inconsistente e os lados divergirem, NÃO
//! há perda de fundos:
//! - **ambos-Maker** → ninguém trava primeiro → o swap não inicia (nada se move);
//! - **ambos-Taker** → ao verificar a perna oposta (simétrica, sem assimetria de
//!   timelock), a checagem acusa `TimelockInverted` (Unsafe) → o segredo NUNCA é
//!   revelado → refund após expiração.
//! O gate de [`crate::verify`] absorve a divergência. (provado nos testes)
//!
//! ## Modelo B (transporte do hashlock)
//! O Taker manda `H = SHA256(segredo)` pelo mesmo canal das ordens. `H` é PÚBLICO
//! (não é o segredo): isso só faz `H` chegar cedo. A FONTE DE VERDADE continua
//! sendo a trava REAL do Taker on-chain — a máquina de estados verifica que o `H`
//! recebido bate com o `hashlock` da trava OBSERVADA **antes** de o Maker travar
//! a própria perna (gate em [`crate::verify::verify_counterparty_leg`]). Este
//! módulo só COLOCA o `H` no contexto; a verificação on-chain não é enfraquecida.
//!
//! ## Dívida conhecida (EVM↔EVM) — documentada, não silenciosa
//! Os recipients assumem a **MESMA CHAVE nas duas chains** (o recipient de uma
//! perna = o endereço do maker da ordem oposta). Isso vale em EVM↔EVM, mas
//! **QUEBRA no Bitcoin**: a perna Bitcoin exigirá um recipient EXPLÍCITO (não
//! derivável do endereço EVM). Registrado também em `docs/ESTADO.md`.

use crate::sm::SwapContext;
use crate::verify::Role;
use orderbook::eip712::digest;
use orderbook::order::Order;

/// Política de timelock COMPARTILHADA — constante de protocolo, NÃO negociada por
/// swap (os dois lados usam a mesma, senão a assimetria não fecha). Invariante
/// necessária para um swap seguro:
///
/// ```text
/// taker_lock_secs >= maker_lock_secs + min_gap
/// ```
///
/// Se violada, a verificação de gap acusa Unsafe e o swap não procede (falha-segura).
/// Os VALORES concretos por chain seguem em aberto (calibração — ver ESTADO.md).
#[derive(Clone, Copy, Debug)]
pub struct TimelockPolicy {
    /// duração do timelock do Taker (LONGO), a partir de `now`.
    pub taker_lock_secs: u64,
    /// duração do timelock do Maker (CURTO), a partir de `now`.
    pub maker_lock_secs: u64,
    /// margem mínima — inter-pernas E janela de relógio (ver [`crate::verify`]).
    pub min_gap: u64,
}

/// Atribui o papel DESTA carteira (`order_self`) frente à contraparte
/// (`order_cp`). Determinística e simétrica: `assign_role(a, b)` e
/// `assign_role(b, a)` são SEMPRE complementares (um Maker, um Taker).
///
/// PRÉ-CONDIÇÃO (responsabilidade do chamador): `order_self`/`order_cp` formam um
/// match válido (espelham + cruzam) e a assinatura de `order_cp` foi RE-VERIFICADA
/// pela carteira — nunca confiar que o livro verificou.
pub fn assign_role(order_self: &Order, order_cp: &Order) -> Role {
    use std::cmp::Ordering;
    match order_self.created_at.cmp(&order_cp.created_at) {
        Ordering::Less => Role::Maker,    // EU em repouso (mais antigo) → Maker
        Ordering::Greater => Role::Taker, // EU cruzei (mais recente) → Taker
        // empate de tempo → desempate por DADO ASSINADO (imune ao servidor):
        // menor digest = Maker. Ordem lexicográfica big-endian dos 32 bytes.
        Ordering::Equal => {
            if digest(order_self) < digest(order_cp) {
                Role::Maker
            } else {
                Role::Taker
            }
        }
    }
}

/// Deriva o [`SwapContext`] desta carteira a partir do par casado, do papel já
/// atribuído ([`assign_role`]), do material de hashlock/segredo (Modelo B) e da
/// política/`now`.
///
/// MATERIAL DE HASHLOCK por papel:
/// - **Taker**: `hashlock = Some(H)`, `secret = Some(s)` com `H = SHA256(s)` (o
///   segredo é gerado pelo executor — geração de aleatoriedade NÃO é pura e não
///   mora aqui).
/// - **Maker**: `hashlock = Some(H)` (recebido via Modelo B), `secret = None`.
///
/// Os campos observacionais (`my_leg_locked`, `counterparty_lock`,
/// `revealed_secret`) nascem vazios — o executor os preenche conforme o mundo.
pub fn derive_context(
    order_self: &Order,
    order_cp: &Order,
    role: Role,
    hashlock: Option<[u8; 32]>,
    secret: Option<[u8; 32]>,
    policy: &TimelockPolicy,
    now: u64,
) -> SwapContext {
    let my_lock = match role {
        Role::Taker => policy.taker_lock_secs, // longo
        Role::Maker => policy.maker_lock_secs, // curto
    };
    SwapContext {
        role,
        // --- a MINHA perna (o que eu vendo/travo) ---
        my_token: order_self.sell_token,
        my_amount: order_self.sell_amount,
        my_timelock: now.saturating_add(my_lock),
        // recipient da MINHA perna = a contraparte. DÍVIDA EVM↔EVM: assume mesma
        // chave nas duas chains → o endereço da cp na minha chain == order_cp.maker.
        my_recipient: order_cp.maker,
        // --- a perna OPOSTA que eu espero/observo (o que eu compro) ---
        cp_token: order_cp.sell_token,
        // expected_amount = o que a contraparte realmente TRAVA (seu sell inteiro),
        // NÃO meu buy_amount: respeita a melhora de preço (cp.sell >= meu buy) e
        // verifico contra o que de fato aparece on-chain.
        cp_amount: order_cp.sell_amount,
        // EU na chain oposta — quem resgata a perna da cp. DÍVIDA EVM↔EVM (idem).
        me: order_self.maker,
        min_gap: policy.min_gap,
        hashlock,
        secret,
        // --- observações (vazias na origem) ---
        my_leg_locked: false,
        counterparty_lock: None,
        revealed_secret: None,
        now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sm::{next_action, NextAction, SwapState};
    use crate::verify::ObservedLock;

    const X: u8 = 0x11; // ativo na chain 1
    const Y: u8 = 0x22; // ativo na chain 10

    #[allow(clippy::too_many_arguments)]
    fn ord(
        maker: u8,
        nonce: u64,
        sell_tok: u8,
        sell_chain: u64,
        sell_amt: u128,
        buy_tok: u8,
        buy_chain: u64,
        buy_amt: u128,
        created_at: u64,
    ) -> Order {
        Order {
            maker: [maker; 20],
            sell_token: [sell_tok; 20],
            sell_chain_id: sell_chain,
            sell_amount: sell_amt,
            buy_token: [buy_tok; 20],
            buy_chain_id: buy_chain,
            buy_amount: buy_amt,
            valid_until: 4_000_000_000,
            nonce,
            created_at,
        }
    }

    // par espelhado e exato: A vende X(1000)→quer Y(500); B vende Y(500)→quer X(1000).
    fn order_a(created_at: u64) -> Order {
        ord(0xAA, 1, X, 1, 1000, Y, 10, 500, created_at)
    }
    fn order_b(created_at: u64) -> Order {
        ord(0xBB, 2, Y, 10, 500, X, 1, 1000, created_at)
    }

    fn policy() -> TimelockPolicy {
        // 7200 >= 3600 + 1800 → assimetria válida
        TimelockPolicy { taker_lock_secs: 7200, maker_lock_secs: 3600, min_gap: 1800 }
    }

    // ===================== PROVA 1: papéis COMPLEMENTARES =====================

    // por chegada: o mais antigo (repouso) é Maker; o mais recente (cruzou) é Taker.
    #[test]
    fn complementary_roles_by_arrival() {
        let a = order_a(10);
        let b = order_b(20); // B chegou depois → B cruzou
        assert_eq!(assign_role(&a, &b), Role::Maker, "A (repouso) = Maker");
        assert_eq!(assign_role(&b, &a), Role::Taker, "B (cruzou) = Taker");

        // simétrico no tempo invertido
        let a2 = order_a(20);
        let b2 = order_b(10);
        assert_eq!(assign_role(&a2, &b2), Role::Taker);
        assert_eq!(assign_role(&b2, &a2), Role::Maker);
    }

    // empate de created_at → desempate por digest assinado (menor = Maker), e os
    // dois lados ainda assim chegam a papéis COMPLEMENTARES.
    #[test]
    fn complementary_roles_by_digest_tie() {
        let a = order_a(50);
        let b = order_b(50); // MESMO created_at → cai no desempate

        let ra = assign_role(&a, &b);
        let rb = assign_role(&b, &a);
        assert_ne!(ra, rb, "papéis devem ser complementares mesmo no empate");

        // a convenção é determinística: menor digest = Maker
        if digest(&a) < digest(&b) {
            assert_eq!(ra, Role::Maker);
            assert_eq!(rb, Role::Taker);
        } else {
            assert_eq!(ra, Role::Taker);
            assert_eq!(rb, Role::Maker);
        }
    }

    // ===================== derivação do SwapContext (§5) =====================

    // mapeamento + as duas sutilezas: expected_amount = sell da cp (melhora de
    // preço) e recipients pela suposição EVM↔EVM.
    #[test]
    fn derives_context_with_price_improvement_and_recipients() {
        let a = order_a(10); // A = Maker (mais antigo)
        // B vende 600 (A pediu só 500) → melhora de preço
        let b = ord(0xBB, 2, Y, 10, 600, X, 1, 1000, 20);
        let role = assign_role(&a, &b);
        assert_eq!(role, Role::Maker);

        let ctx = derive_context(&a, &b, role, Some([0xAB; 32]), None, &policy(), 1000);

        assert_eq!(ctx.role, Role::Maker);
        assert_eq!(ctx.my_token, a.sell_token);
        assert_eq!(ctx.my_amount, 1000);
        assert_eq!(ctx.cp_token, b.sell_token);
        // a prova do §5.2: verifico contra o sell REAL da cp (600), não meu buy (500)
        assert_eq!(ctx.cp_amount, 600);
        assert_ne!(ctx.cp_amount, a.buy_amount, "NÃO é o meu buy_amount");
        // recipients (suposição EVM↔EVM)
        assert_eq!(ctx.my_recipient, b.maker, "minha perna paga a contraparte");
        assert_eq!(ctx.me, a.maker, "eu resgato a perna oposta");
        // maker = timelock curto
        assert_eq!(ctx.my_timelock, 1000 + 3600);
        assert_eq!(ctx.min_gap, 1800);
        assert!(ctx.secret.is_none(), "maker não tem o segredo");
    }

    // ===================== PROVA 2: divergência NÃO perde fundos =============

    // ambos-Maker (servidor diz a CADA lado que a SUA ordem chegou antes):
    // ninguém trava → o swap não inicia → nenhum fundo se move.
    #[test]
    fn divergence_both_maker_swap_never_starts() {
        let p = policy();
        let a = order_a(10);
        let b_seen_by_a = order_b(20); // na visão de A: a<b → A é Maker
        let b = order_b(10);
        let a_seen_by_b = order_a(20); // na visão de B: b<a → B é Maker

        let ra = assign_role(&a, &b_seen_by_a);
        let rb = assign_role(&b, &a_seen_by_b);
        assert_eq!(ra, Role::Maker);
        assert_eq!(rb, Role::Maker);

        let ctx_a = derive_context(&a, &b_seen_by_a, ra, Some([0xAB; 32]), None, &p, 1000);
        let ctx_b = derive_context(&b, &a_seen_by_b, rb, Some([0xAB; 32]), None, &p, 1000);

        // os dois Makers, no Start, sem perna oposta observada → só ESPERAM.
        // Nenhum emite LockMyLeg → nenhum fundo se move.
        let act_a = next_action(&SwapState::Start, &ctx_a);
        let act_b = next_action(&SwapState::Start, &ctx_b);
        assert_eq!(act_a, NextAction::WaitForCounterpartyLock);
        assert_eq!(act_b, NextAction::WaitForCounterpartyLock);
        assert!(!matches!(act_a, NextAction::LockMyLeg { .. }));
        assert!(!matches!(act_b, NextAction::LockMyLeg { .. }));
    }

    // ambos-Taker (servidor diz a CADA lado que a SUA ordem chegou depois):
    // ambos travam com timelock LONGO IGUAL (sem assimetria). Ao verificar a
    // perna oposta, cada Taker espera uma perna mais CURTA; vê uma igual →
    // TimelockInverted → Unsafe → o segredo NUNCA é revelado → refund.
    #[test]
    fn divergence_both_taker_secret_never_revealed() {
        let p = policy();
        let a = order_a(20);
        let b_seen_by_a = order_b(10); // na visão de A: a>b → A é Taker
        let ra = assign_role(&a, &b_seen_by_a);
        assert_eq!(ra, Role::Taker);

        let secret = [0x5e; 32];
        let h = [0xAB; 32];
        let ctx = derive_context(&a, &b_seen_by_a, ra, Some(h), Some(secret), &p, 1000);

        // A travou sua perna (taker, longo). Observa a perna de B — que, sob a
        // mesma divergência, TAMBÉM é taker: timelock LONGO IGUAL ao de A.
        let b_leg = ObservedLock {
            hashlock: h,
            token: ctx.cp_token,
            amount: ctx.cp_amount,
            recipient: ctx.me,         // pagaria A corretamente...
            timelock: ctx.my_timelock, // ...MAS sem assimetria (igual à minha)
            sender: [0xBB; 20],
            exists: true,
        };

        let mut ctx2 = ctx.clone();
        ctx2.my_leg_locked = true;
        ctx2.counterparty_lock = Some(b_leg);

        let action = next_action(&SwapState::MyLegLocked, &ctx2);
        assert_eq!(action, NextAction::Refund, "sem assimetria → Unsafe → refund");
        assert!(
            !matches!(action, NextAction::RedeemCounterpartyLeg { .. }),
            "o segredo NUNCA é revelado sob divergência de papel"
        );
    }
}

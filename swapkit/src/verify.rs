//! Verificador de perna oposta (puro, sem I/O).
//!
//! É a versão "carteira" do que o contrato NÃO pode fazer: olhar a outra perna.
//! Recebe (o que se espera) + (o que se observou na outra chain) e devolve
//! `Safe` ou `Unsafe(razão)`. Tudo entra por parâmetro — sem rede, sem relógio,
//! sem estado.

use serde::{Deserialize, Serialize};

/// Endereço de 20 bytes (EVM). `[0u8;20]` = nativo, quando usado como token.
pub type Address = [u8; 20];

/// Papel desta parte no swap. Determina a DIREÇÃO da checagem de gap.
///
/// Convenção do Kael (clássica do atomic swap):
/// - `Taker`  = detentor do segredo; trava PRIMEIRO e com timelock MAIOR (longo).
/// - `Maker`  = respondente; trava DEPOIS e com timelock MENOR (curto).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Taker,
    Maker,
}

/// Uma trava HTLC lida da outra chain (o que um leitor de chain reportaria).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedLock {
    pub hashlock: [u8; 32],
    pub token: Address,
    pub amount: u128,
    /// quem pode resgatar com o preimage.
    pub recipient: Address,
    pub timelock: u64,
    /// quem travou.
    pub sender: Address,
    /// a trava foi encontrada on-chain?
    pub exists: bool,
}

/// O que ESTA parte espera da perna oposta para considerá-la segura.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegExpectation {
    /// o H que deve ser idêntico nas duas pernas.
    pub expected_hashlock: [u8; 32],
    pub expected_token: Address,
    pub expected_amount: u128,
    /// EU — quem deve poder resgatar a perna oposta.
    pub expected_recipient: Address,
    /// o timelock da MINHA perna.
    pub my_timelock: u64,
    /// gap mínimo seguro entre os timelocks das duas pernas. Também é a janela
    /// MÍNIMA de relógio exigida da perna oposta a partir de `now` (ver abaixo).
    pub min_gap: u64,
    /// tempo atual (parâmetro — sem relógio do sistema). Usado para garantir que
    /// a perna oposta não está prestes a expirar AGORA: o gap entre as pernas é
    /// condição necessária mas NÃO suficiente — se a perna oposta expira em menos
    /// de `min_gap` a partir de `now`, agir contra ela (revelar o segredo / travar
    /// a perna do respondente) é inseguro mesmo com o gap inter-pernas correto.
    pub now: u64,
    pub role: Role,
}

/// Cada checagem que pode falhar vira uma razão de `Unsafe`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnsafeReason {
    /// a trava não foi encontrada on-chain.
    LockNotFound,
    /// hashlock diferente — quebra a atomicidade (o segredo não destravaria as duas pernas).
    HashlockMismatch,
    TokenMismatch,
    AmountMismatch,
    /// a perna oposta paga OUTRO endereço — eu não poderia resgatar.
    RecipientMismatch,
    /// a perna oposta está do lado certo, mas sem a margem `min_gap` segura.
    TimelockGapTooSmall,
    /// a perna oposta está do lado ERRADO (degenerado): não me dá janela alguma.
    TimelockInverted,
    /// a perna oposta está estruturalmente correta (lado certo, gap inter-pernas
    /// ok), mas expira perto demais de AGORA (`< now + min_gap`): não há janela de
    /// relógio para agir contra ela. Agir mesmo assim vazaria o segredo (taker) ou
    /// comprometeria fundos sem poder resgatar a tempo. ESTE é o furo do
    /// "segredo revelado contra perna prestes a expirar".
    CounterpartyExpiresTooSoon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyOutcome {
    Safe,
    Unsafe(UnsafeReason),
}

/// Decide se é seguro prosseguir, dada a expectativa e a trava observada.
///
/// Ordem das checagens: existência → hashlock → token → amount → recipient →
/// gap de timelock. A primeira que falhar é a razão devolvida.
pub fn verify_counterparty_leg(
    expectation: &LegExpectation,
    observed: &ObservedLock,
) -> VerifyOutcome {
    use UnsafeReason::*;
    use VerifyOutcome::*;

    if !observed.exists {
        return Unsafe(LockNotFound);
    }
    if observed.hashlock != expectation.expected_hashlock {
        // checagem CENTRAL da atomicidade: o mesmo segredo tem de destravar as duas pernas.
        return Unsafe(HashlockMismatch);
    }
    if observed.token != expectation.expected_token {
        return Unsafe(TokenMismatch);
    }
    if observed.amount != expectation.expected_amount {
        return Unsafe(AmountMismatch);
    }
    if observed.recipient != expectation.expected_recipient {
        return Unsafe(RecipientMismatch);
    }
    if let Some(reason) = check_timelock_gap(expectation, observed) {
        return Unsafe(reason);
    }
    Safe
}

/// A checagem de segurança mais sutil — a assimetria de timelock do atomic swap.
///
/// PRINCÍPIO: quem revela/age por ÚLTIMO precisa de janela. A direção da
/// desigualdade difere por papel porque os dois lados estão em pontas opostas
/// dessa janela:
///
/// - `Maker` (respondente, trava por ÚLTIMO, timelock CURTO `my_timelock`):
///   ele verifica a perna do Taker, que já travou. Sequência futura: o Taker
///   revela o segredo na chain do Maker (resgatando a perna do Maker) antes do
///   `my_timelock`; aí o Maker usa o segredo para resgatar a perna do Taker e
///   precisa de tempo até `observed.timelock`. Logo a perna do Taker tem de
///   expirar DEPOIS, com margem:
///       observed.timelock >= my_timelock + min_gap
///   - observed.timelock <= my_timelock           → TimelockInverted (não é nem maior)
///   - my_timelock < observed.timelock < my+gap   → TimelockGapTooSmall
///
/// - `Taker` (detentor do segredo, trava PRIMEIRO, timelock LONGO `my_timelock`):
///   ele verifica a perna do Maker antes de resgatá-la (revelando o segredo).
///   Depois que ele revela, o Maker precisa de tempo para resgatar a perna do
///   Taker antes do `my_timelock`. Logo a perna do Maker tem de expirar ANTES,
///   com margem:
///       my_timelock >= observed.timelock + min_gap
///   - observed.timelock >= my_timelock           → TimelockInverted (não é nem menor)
///   - my-gap < observed.timelock < my_timelock   → TimelockGapTooSmall
///
/// Em ambos os casos: a perna OPOSTA precisa estar do lado certo (mais longa que
/// a minha se eu sou o Maker curto; mais curta se eu sou o Taker longo) e com
/// pelo menos `min_gap` de folga, senão quem age por último fica sem janela e
/// pode ser roubado. Usamos somas saturadas para evitar overflow/underflow.
fn check_timelock_gap(expectation: &LegExpectation, observed: &ObservedLock) -> Option<UnsafeReason> {
    let my = expectation.my_timelock;
    let opp = observed.timelock;
    let gap = expectation.min_gap;
    let now = expectation.now;

    // (1) ESTRUTURAL: a perna oposta está do lado certo, com gap inter-pernas.
    let structural = match expectation.role {
        Role::Maker => {
            // a perna oposta (Taker) deve ser MAIOR que a minha, com margem.
            if opp <= my {
                Some(UnsafeReason::TimelockInverted)
            } else if opp < my.saturating_add(gap) {
                Some(UnsafeReason::TimelockGapTooSmall)
            } else {
                None
            }
        }
        Role::Taker => {
            // a perna oposta (Maker) deve ser MENOR que a minha, com margem.
            if opp >= my {
                Some(UnsafeReason::TimelockInverted)
            } else if opp.saturating_add(gap) > my {
                Some(UnsafeReason::TimelockGapTooSmall)
            } else {
                None
            }
        }
    };
    if structural.is_some() {
        return structural;
    }

    // (2) RELÓGIO ABSOLUTO: o gap inter-pernas é necessário mas NÃO suficiente.
    //     Quem age (o taker revelando o segredo; o maker que vai depender de
    //     resgatar) precisa de janela REAL a partir de `now` para minerar a tx
    //     contra a perna oposta antes que ela expire. Exigimos pelo menos
    //     `min_gap` de folga: opp deve estar a >= now + gap. Caso contrário, a
    //     perna está estruturalmente certa mas temporalmente inviável — e agir
    //     (revelar o segredo) seria inseguro. Convenção de fronteira idêntica à
    //     do gap inter-pernas: opp == now + gap é seguro (folga exata).
    if opp < now.saturating_add(gap) {
        return Some(UnsafeReason::CounterpartyExpiresTooSoon);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> [u8; 32] {
        [b; 32]
    }
    fn a(b: u8) -> [u8; 20] {
        [b; 20]
    }

    const ME: u8 = 0xEE;
    const HL: u8 = 0xAB;
    const TOK: u8 = 0x11;
    const AMT: u128 = 1000;

    // --- Maker: minha perna é a CURTA; a oposta (Taker) deve ser MAIOR. ---
    fn maker_exp() -> LegExpectation {
        LegExpectation {
            expected_hashlock: h(HL),
            expected_token: a(TOK),
            expected_amount: AMT,
            expected_recipient: a(ME),
            my_timelock: 1000,
            min_gap: 100,
            now: 0, // longe da expiração nos vetores base
            role: Role::Maker,
        }
    }
    // perna oposta segura para o Maker: timelock 1200 (>= 1000 + 100).
    fn obs_for_maker() -> ObservedLock {
        ObservedLock {
            hashlock: h(HL),
            token: a(TOK),
            amount: AMT,
            recipient: a(ME),
            timelock: 1200,
            sender: a(0x01),
            exists: true,
        }
    }

    // --- Taker: minha perna é a LONGA; a oposta (Maker) deve ser MENOR. ---
    fn taker_exp() -> LegExpectation {
        LegExpectation {
            expected_hashlock: h(HL),
            expected_token: a(TOK),
            expected_amount: AMT,
            expected_recipient: a(ME),
            my_timelock: 2000,
            min_gap: 100,
            now: 0, // longe da expiração nos vetores base
            role: Role::Taker,
        }
    }
    // perna oposta segura para o Taker: timelock 1800 (1800 + 100 <= 2000).
    fn obs_for_taker() -> ObservedLock {
        ObservedLock {
            hashlock: h(HL),
            token: a(TOK),
            amount: AMT,
            recipient: a(ME),
            timelock: 1800,
            sender: a(0x02),
            exists: true,
        }
    }

    // ---- caminho Safe para ambos os papéis ----

    #[test]
    fn safe_maker() {
        assert_eq!(verify_counterparty_leg(&maker_exp(), &obs_for_maker()), VerifyOutcome::Safe);
    }

    #[test]
    fn safe_taker() {
        assert_eq!(verify_counterparty_leg(&taker_exp(), &obs_for_taker()), VerifyOutcome::Safe);
    }

    // boundary: gap exato é seguro (>= / soma exata).
    #[test]
    fn safe_at_exact_gap_boundary() {
        let mut o = obs_for_maker();
        o.timelock = 1100; // == my(1000) + gap(100)
        assert_eq!(verify_counterparty_leg(&maker_exp(), &o), VerifyOutcome::Safe);

        let mut ot = obs_for_taker();
        ot.timelock = 1900; // 1900 + 100 == my(2000)
        assert_eq!(verify_counterparty_leg(&taker_exp(), &ot), VerifyOutcome::Safe);
    }

    // ---- razões de Unsafe (campos) ----

    #[test]
    fn lock_not_found() {
        let mut o = obs_for_maker();
        o.exists = false;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }

    #[test]
    fn hashlock_mismatch_is_detected() {
        // a checagem CENTRAL: hashlock diferente quebra a atomicidade.
        let mut o = obs_for_maker();
        o.hashlock = h(0x01);
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::HashlockMismatch)
        );
    }

    #[test]
    fn token_mismatch() {
        let mut o = obs_for_maker();
        o.token = a(0x99);
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TokenMismatch)
        );
    }

    #[test]
    fn amount_mismatch() {
        let mut o = obs_for_maker();
        o.amount = AMT + 1;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::AmountMismatch)
        );
    }

    #[test]
    fn recipient_mismatch_funds_would_go_to_another() {
        // a perna oposta pagaria OUTRO endereço — eu não poderia resgatar.
        let mut o = obs_for_maker();
        o.recipient = a(0xBA); // não sou eu (ME)
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::RecipientMismatch)
        );
    }

    // ---- gap de timelock: GapTooSmall por papel ----

    #[test]
    fn gap_too_small_maker() {
        // oposta só um pouco maior que a minha (1050), abaixo de my+gap(1100).
        let mut o = obs_for_maker();
        o.timelock = 1050;
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockGapTooSmall)
        );
    }

    #[test]
    fn gap_too_small_taker() {
        // oposta só um pouco menor que a minha (1950): 1950+100=2050 > my(2000).
        let mut o = obs_for_taker();
        o.timelock = 1950;
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockGapTooSmall)
        );
    }

    // ---- gap de timelock: Inverted (lado errado) por papel ----

    #[test]
    fn inverted_maker() {
        // perna oposta MENOR que a minha: deveria ser maior.
        let mut o = obs_for_maker();
        o.timelock = 900; // <= my(1000)
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_maker_equal_is_inverted() {
        let mut o = obs_for_maker();
        o.timelock = 1000; // == my → ainda do lado errado
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_taker() {
        // perna oposta MAIOR que a minha: deveria ser menor.
        let mut o = obs_for_taker();
        o.timelock = 2100; // >= my(2000)
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    #[test]
    fn inverted_taker_equal_is_inverted() {
        let mut o = obs_for_taker();
        o.timelock = 2000; // == my → lado errado
        assert_eq!(
            verify_counterparty_leg(&taker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    // ---- relógio absoluto: perna estruturalmente OK mas expirando AGORA ----

    // FURO FECHADO: o taker NÃO deve considerar Safe uma perna do maker que,
    // embora com gap inter-pernas correto, expira perto demais de `now`. Sem
    // isto, o taker revelaria o segredo sem janela para resgatar → segredo vaza.
    #[test]
    fn taker_unsafe_when_counterparty_expires_too_soon() {
        let mut e = taker_exp();
        // perna do maker em 1800 (gap inter-pernas ok: 1800+100 <= 2000), MAS
        // now=1750 → 1800 < 1750+100=1850 → janela de relógio insuficiente.
        e.now = 1750;
        let o = obs_for_taker(); // timelock 1800
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::CounterpartyExpiresTooSoon)
        );
    }

    // fronteira: opp == now + gap é seguro (folga de relógio exata).
    #[test]
    fn safe_at_exact_now_window_boundary() {
        let mut e = taker_exp();
        e.now = 1700; // 1800 == 1700 + 100 → folga exata, Safe
        assert_eq!(verify_counterparty_leg(&e, &obs_for_taker()), VerifyOutcome::Safe);
    }

    // vale também para o maker: perna do taker do lado certo mas expirando já.
    #[test]
    fn maker_unsafe_when_counterparty_expires_too_soon() {
        let mut e = maker_exp();
        let mut o = obs_for_maker();
        o.timelock = 5000; // bem do lado certo (>> my=1000+gap)
        e.now = 4950; // mas 5000 < 4950 + 100 → sem janela de relógio
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::CounterpartyExpiresTooSoon)
        );
    }

    // ordem: a falha ESTRUTURAL (lado errado/gap) tem prioridade sobre a de
    // relógio — um leg invertido reporta TimelockInverted, não ExpiresTooSoon.
    #[test]
    fn structural_failure_precedes_clock_failure() {
        let mut e = taker_exp();
        e.now = 5000; // tudo "expira" vs now
        let mut o = obs_for_taker();
        o.timelock = 2500; // >= my(2000) → invertido (lado errado)
        assert_eq!(
            verify_counterparty_leg(&e, &o),
            VerifyOutcome::Unsafe(UnsafeReason::TimelockInverted)
        );
    }

    // a primeira checagem que falha é a razão: existência vem antes do resto.
    #[test]
    fn first_failure_wins_existence_before_hashlock() {
        let mut o = obs_for_maker();
        o.exists = false;
        o.hashlock = h(0x01); // também errado, mas LockNotFound vem primeiro
        assert_eq!(
            verify_counterparty_leg(&maker_exp(), &o),
            VerifyOutcome::Unsafe(UnsafeReason::LockNotFound)
        );
    }
}

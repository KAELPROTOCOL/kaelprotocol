# Kael — Decisões de Fundação (ADRs)

> Registro consolidado das decisões de arquitetura do protocolo Kael. Cada ADR
> documenta uma decisão **já refletida no código** ou explicitamente marcada como
> **ABERTA**. A numeração segue o blueprint de fundação do projeto; **ADR-002 e
> ADR-003 não foram atribuídos** no material de fundação disponível (lacuna
> registrada por honestidade — não inventada).
>
> Regra inviolável do projeto: nada toca fundos reais antes de auditoria
> profissional independente. Todo o estado é experimental e não-auditado.

---

## ADR-001 — Hashlock usa SHA-256, não keccak256
**Decisão:** o hashlock do HTLC é `sha256(preimage)`, não keccak256. O `contractId`
interno do HTLC continua keccak256 (barato e nativo no EVM) — são usos distintos.
**Porquê:** o mesmo preimage precisa ser verificável em Bitcoin Script (`OP_SHA256`)
no futuro; fechar essa porta cedo evitaria o caminho do Bitcoin. A regra propaga
para todas as peças (contrato, maestro).
**Onde:** `contracts/src/HashedTimelock.sol`, `maestro/src/hashlock.rs`.
**Estado:** Cravado.

## ADR-004 — Matching neutro por price-time
**Decisão:** o matcher é uma função pura, determinística, com prioridade
**price-time** (melhor preço; empate → mais antigo → menor nonce). `now` entra como
parâmetro (sem relógio do sistema).
**Porquê:** neutralidade do operador — sem espaço para favorecer ninguém;
pureza/testabilidade.
**Onde:** `orderbook/src/matching.rs`.
**Estado:** Cravado.

## ADR-005 — Domínio EIP-712 chain-agnóstico
**Decisão:** o domínio EIP-712 omite `chainId` e `verifyingContract`
(`EIP712Domain(string name,string version)`). O binding de chain vive no payload
assinado (`sellChainId`/`buyChainId`), não no domínio.
**Porquê:** uma ordem Kael é inerentemente cross-chain e carrega seus próprios
chainIds; um domínio amarrado a uma chain não caberia. A verificação em Solidity e
em Rust concordam (provado por vetor fixo).
**Onde:** `contracts/src/Order.sol`, `orderbook/src/eip712.rs`, `vectors/eip712_order.json`.
**Estado:** Cravado.

## ADR-006 — Estado do livro em memória (sem banco no MVP)
**Decisão:** o servidor do livro guarda ordens ativas em memória; sem persistência.
**Porquê:** ordens são efêmeras e assinadas; se o servidor reinicia, os makers
reenviam e nada se perde (fundos nunca estiveram no servidor). Persistência é
evolução futura.
**Onde:** `orderbook/src/book.rs`, `orderbook/src/server.rs`.
**Estado:** Cravado (MVP).

## ADR-007 — Verificação de assinatura na borda + verificador extensível
**Decisão:** toda ordem tem a assinatura re-verificada **antes** de entrar no livro
(inválida/expirada/duplicada é rejeitada). A verificação fica atrás de um trait
`SignatureVerifier` (EIP-712 é a 1ª impl; Bitcoin/Solana entram depois).
**Porquê:** nada é aceito por confiança; nascer extensível evita refazer.
**Onde:** `orderbook/src/book.rs` (`submit`, `SignatureVerifier`).
**Estado:** Cravado.

## ADR-008 — O servidor só INFORMA matches; nunca executa
**Decisão:** ao achar um par, o servidor apenas o disponibiliza (modelo pull,
`GET /matches`). Nunca executa, altera, prioriza arbitrariamente nem custodia.
**Porquê:** neutralidade e não-custódia; remover o servidor só pararia a descoberta
de pares — ninguém ganha poder sobre fundos.
**Onde:** `orderbook/src/server.rs`, `orderbook/src/book.rs` (`matches_for`).
**Estado:** Cravado.

---

## ADR-009 — Liquidador (Settlement) liga ordem↔HTLC; HTLC intocado
**Decisão:** o contrato `Settlement` é a porta de entrada da liquidação: verifica a
ordem assinada (`OrderLib.verify`), consome o nonce, recebe os fundos autorizados e
trava no `HashedTimelock` em nome próprio. O `HashedTimelock` e a `OrderLib` **não
são alterados**.
**Porquê:** fecha o **Furo 1** (a ordem assinada estava desligada do HTLC) de forma
**LOCAL** — a trava usa `order.sellToken`/`order.sellAmount` autorizados, sem
precisar ver a outra perna.
**Onde:** `contracts/src/Settlement.sol`.
**Estado:** Cravado.

## ADR-010 — Não-custódia do Settlement: duas saídas de fundos, sem terceira porta
**Decisão:** o `Settlement` tem só dois caminhos de saída de fundos — (1) para o
HTLC via `newSwap`; (2) de volta ao maker via `refundLeg`. Sem dono, sem saque, sem
`selfdestruct`, sem chamada arbitrária. O reembolso vai **sempre** a `leg.maker` (o
dono real), mesmo que outro chame `refundLeg`.
**Porquê:** não-custódia como invariante verificável; ninguém pode desviar fundos.
**Onde:** `contracts/src/Settlement.sol` (`refundLeg`, `receive`); provado por
`test_NonCustody_NoRetainedBalance`.
**Estado:** Cravado.

## ADR-011 — Anti-replay de nonce per-chain + binding de chain
**Decisão:** o nonce é consumido na liquidação (`consumedNonce[maker][nonce]`),
fechando o **Furo 2**. O binding `order.sellChainId == block.chainid` confina cada
ordem à sua chain, tornando o anti-replay **per-chain suficiente** — um nonce global
entre chains **NÃO é necessário**.
**Porquê:** cada perna é uma trava independente confinada à sua chain; uma ordem só
liquida na chain para a qual foi assinada.
**Onde:** `contracts/src/Settlement.sol`; provado por `test_SettleLeg_Replay_Reverts`
e `test_SettleLeg_WrongChain_Reverts`.
**Estado:** Cravado.

## ADR-012 — Só o maker liquida a própria perna (fecha frontrun ERC-20)
**Decisão:** `settleLeg` exige `msg.sender == order.maker`.
**Porquê:** fecha o vetor onde um terceiro, vendo a ordem assinada + a aprovação
ERC-20 do maker, liquidaria em nome dele com `recipient`/`hashlock` do atacante e
drenaria os tokens. Alinhado à Abordagem A (cada parte trava os próprios fundos).
**Onde:** `contracts/src/Settlement.sol`; provado por
`test_SettleLeg_NotMaker_ERC20_ApprovalAloneCannotSteal`.
**Estado:** Cravado.

## ADR-013 — Abordagem A: validação cross-leg na carteira, não no contrato
**Decisão:** a validação cruzada on-chain entre as duas pernas (mesmo hashlock, gap
de timelock) foi **REMOVIDA** do `Settlement`. O contrato valida só a própria perna;
a verificação cross-leg vive na carteira (`swapkit`).
**Porquê:** a validação cruzada on-chain é (a) **inoperante cross-chain** — cada
`Settlement` é per-chain e só vê a própria perna (`legCount` sempre 0) — e (b)
**desnecessária** — a segurança vem da falha-segura do HTLC + cada parte verificar a
perna oposta on-chain antes de travar/resgatar (modelo clássico de atomic swap).
Mensageria cross-chain (ponte) foi **REJEITADA** por reintroduzir confiança e não
estender ao Bitcoin.
**Onde:** `contracts/src/Settlement.sol` (sem `swaps[swapId]`/`legCount`);
`swapkit/src/verify.rs`.
**Estado:** Cravado.

## ADR-014 — Protocolo interativo: papéis taker/maker e geração do segredo
**Decisão:** a liquidação é interativa (o livro descobre, as carteiras liquidam). O
**TAKER** gera o segredo, trava primeiro, timelock **longo**. O **MAKER** é
respondente, verifica a perna do taker, trava depois com o **mesmo hashlock**,
timelock **curto**.
**Porquê:** é a estrutura segura do atomic swap (commitment sequencial). O
free-option fica com o taker, mitigado por janela curta e compensado pelo modelo de
incentivo de liquidez (futuro — ABERTO).
**Onde:** `swapkit/src/sm.rs`.
**Estado:** Cravado (a mitigação econômica do free-option é ABERTA).

## ADR-015 — Verificação de perna oposta na carteira (swapkit)
**Decisão:** a carteira verifica a perna oposta antes de travar/resgatar:
existência, mesmo hashlock, token/amount, recipient, e gap de timelock seguro
(assimétrico por papel: `T_longo ≥ T_curto + min_gap`). A máquina de estados
**NUNCA** emite travar/resgatar contra uma perna `Unsafe` — o taker nunca revela o
segredo, o maker nunca trava, contra perna insegura.
**Porquê:** é onde a verificação cross-leg pode rodar de forma trustless (a carteira
lê a outra chain); o contrato não pode.
**Onde:** `swapkit/src/verify.rs`, `swapkit/src/sm.rs`.
**Estado:** Cravado.

## ADR-016 — Leitura de chain trust-minimized (RpcVerifier), evoluível para trustless
**Decisão:** a leitura da outra chain é, no MVP, via RPC: **trust-minimized, não
trustless** (confia no nó). Fica atrás da interface `ChainVerifier` para que
SPV/light-client (trustless) a substitua depois.
**Porquê:** é a fronteira de confiança pragmática do MVP; a interface preserva o
caminho para o trustless sem reescrever a carteira.
**Onde:** `swapkit/src/chain.rs`.
**Estado:** Cravado como MVP. Decisões de segurança **ABERTAS**: profundidade de
confirmação (anti-reorg) e eventual quórum de nós (ver `ESTADO.md`).

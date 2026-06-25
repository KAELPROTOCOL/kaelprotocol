# Kael — Estado do Projeto

> Estado honesto do que existe, do que foi decidido mas não construído, e do que
> está em aberto. Números de teste são **reais** (de `forge test` e
> `cargo test --workspace`), não estimados.
>
> Regra inviolável: nada toca fundos reais antes de auditoria profissional
> independente. Tudo abaixo é experimental, não-auditado, testnet/local.

---

## 1. CONSTRUÍDO E TESTADO

**Total: 135 testes passando, 0 ignorados** (36 Foundry + 99 Rust).

### Contratos (Foundry) — 36 testes
| Suíte | Testes | Cobre |
|---|---|---|
| `HashedTimelock.t.sol` | 9 | HTLC: trava/resgate/reembolso, preimage errado, duplo resgate, resgate após expiração, guardas de criação |
| `Order.t.sol` | 10 | EIP-712: assinatura válida/inválida/expirada, endurecimento ECDSA (s alto, v inválido, comprimento, signer zero) |
| `Settlement.t.sol` | 16 | liquidação Abordagem A: autorização+nonce, binding de chain, maker-only (ETH+ERC-20), replay, reembolso ao maker, não-custódia |
| `Vector.t.sol` | 1 | gera o vetor de equivalência EIP-712 on-chain/off-chain |

### `orderbook` (Rust) — 26 testes
- `lib` (25): matching puro price-time, verificação EIP-712 (equivalência com o contrato, provada por vetor), livro em memória + ingestão verificada na borda, e **price-time no caminho SERVIDO** (`matches_for` ordena por melhor preço; fonte única de ranking em `matching::cmp_makers_for_taker`).
- `server_integration` (1): servidor HTTP real — ingestão verificada + consulta de matches.

### `maestro` (Rust) — 9 testes
- `lib` (6): hashlock SHA-256 (fonte única) + correlação por hashlock e watchdog.
- `e2e` (2): dois anvils, deploy do HTLC, swap correlacionado + preimage capturado; watchdog de expiração.
- `full_flow` (1): capstone livro→liquidação→maestro.

### `swapkit` (Rust) — 64 testes, 0 ignorados
- `verify` (19): verificador de perna oposta (hashlock/token/amount/recipient + gap de timelock assimétrico por papel) **+ janela de relógio absoluta**: a perna oposta não pode expirar em menos de `now + min_gap` — fecha o vetor de "segredo revelado contra perna prestes a expirar".
- `sm` (11): máquina de estados interativa (jornadas taker/maker, testes críticos de segurança incl. o do relógio absoluto, reembolsos, transições inválidas).
- `chain` (8): mapeamento Swap→ObservedLock (`exists` = trava ativa) + junção com a verificação + **integração REAL contra anvil** (sobe nó, deploy do HTLC, `newSwap`, leitura via `RpcVerifier`, e a junção leitura→verificação→decisão da máquina de estados). O antigo stub `#[ignore]` foi **implementado**.
- `handshake` (5): **atribuição DETERMINÍSTICA de papéis** (Taker/Maker) + derivação do `SwapContext`, PURA (recebe as duas ordens como input, agnóstica ao transporte). Regra: repouso (menor `created_at`) = Maker; quem cruza = Taker; empate → menor digest EIP-712 = Maker. Provas: papéis complementares dos dois lados (incl. desempate por digest) e **divergência de papel NÃO perde fundos** (ambos-Maker → swap não inicia; ambos-Taker → Unsafe/`TimelockInverted` → segredo nunca revelado → refund).
- `exec` (19): signer com allowlist local/testnet, envio de `lock`/`redeem`/`refund`, observação por hashlock, confirmação por profundidade, executor `step()`/`run()` com relógio injetável e re-verificação anti-TOCTOU. Inclui e2e local com dois anvils e dois executores: Taker lock → Maker lock → Taker redeem → Maker aprende segredo → Maker redeem.

---

## 2. DECIDIDO, NÃO CONSTRUÍDO

Decisões já tomadas, mas ainda sem implementação:

- **Transporte das ordens na liquidação = PEER-TO-PEER (não via livro).** A regra
  de papéis e o `SwapContext` (em `swapkit/src/handshake.rs`) já estão prontos e são
  **agnósticos ao transporte** — recebem as duas ordens completas como input. A
  decisão de COMO a carteira obtém a ordem completa da contraparte: **p2p, as
  carteiras trocam direto entre si**, NÃO um `GET /orders/{hash}` no livro. Razão:
  o livro fica só para **descoberta** (anunciar/casar); a partir do match, a
  liquidação é interativa e **trustless**, sem o servidor no caminho crítico. Em
  ambos os casos a carteira **re-verifica a assinatura EIP-712 da contraparte por
  conta própria** (inegociável — nunca confiar no que recebeu). O **canal p2p em si
  não está construído** (é a próxima peça; mais trabalho que o GET, adiado de
  propósito). `GET /orders/{hash}` foi **rejeitado** como acoplamento ao servidor.
- **Dívida conhecida: recipients assumem MESMA CHAVE nas duas chains (EVM↔EVM).**
  Em `handshake::derive_context`, o recipient de uma perna = o endereço do maker da
  ordem oposta (mesma chave EVM nas duas chains). Vale em EVM↔EVM, mas **QUEBRA no
  Bitcoin**: a perna Bitcoin exigirá um recipient **EXPLÍCITO** (não derivável do
  endereço EVM). Registrado no código e aqui — não é suposição silenciosa.
- **Política de timelock por chain (calibração).** `TimelockPolicy`
  (taker longo / maker curto / `min_gap`) é uma constante de protocolo compartilhada;
  os VALORES por chain ainda não estão calibrados (ver o acoplamento abaixo).
- **Profundidade de confirmação (anti-reorg).** `ChainVerifier::observe_lock` hoje lê
  o estado "agora", sem exigir N confirmações. Para a perna oposta, observar uma
  trava que depois é revertida por reorg é um risco real. Decidido que precisa de uma
  profundidade mínima de confirmação antes de considerar uma trava "observada";
  **não implementado** (provavelmente um parâmetro da verificação/`ChainVerifier`).
- **Quórum de nós.** O `RpcVerifier` confia num único nó. Reforço barato antes do
  SPV completo: consultar múltiplos nós e exigir concordância. Decidido como direção;
  **não implementado**.
- **⚠️ LIVENESS DE TX PERTO DA EXPIRAÇÃO (taxa/RBF) — fora do MVP, OBRIGATÓRIO antes
  de testnet pública.** Uma transação **subprecificada** (resgate ou reembolso) pode
  ficar **presa na mempool** e não minerar a tempo. Se isso acontece **perto da
  expiração do timelock**, a parte **perde a janela de resgate** — risco de liveness
  REAL, com perda de fundos como consequência (a contraparte reembolsa; eu fico sem o
  resgate). O executor do MVP usa estimativa de gás default do nó, **sem
  replace-by-fee**, **sem retentativa com bump de taxa** — aceitável **só em anvil**
  (mineração determinística, sem mempool competitiva). Para qualquer chain pública,
  uma **política de taxa + RBF** (bump progressivo até minerar, com margem antes da
  expiração) é **necessária, não opcional**. Decidido como direção; **não
  implementado** e **não pode ir a testnet pública sem isso**.
- **Acoplamento de fundação a calibrar por chain.** `min_gap` (gap de timelock),
  **profundidade de confirmação** e **tempo de bloco da chain** são **acoplados**: o
  gap seguro depende de quantos blocos/quanto tempo a parte precisa para agir após a
  revelação, que por sua vez depende do tempo de bloco e da profundidade exigida.
  Esses três devem ser **calibrados juntos, por chain** — ainda não há tabela de
  valores nem método de calibração.

---

## 3. ABERTO / A CONSTRUIR

- ~~**O executor.**~~ **FEITO para o escopo local/anvil direto HTLC.** A camada pega
  a `NextAction` da máquina de estados, re-observa, re-verifica e só então envia
  `lock`/`redeem`/`refund`. Continua fora do escopo: produção, mainnet, fundos reais,
  fee bump/RBF e recuperação persistente completa.
- **Integração ponta a ponta completa.** Livro → match → handshake p2p → executor →
  chains ainda não está todo costurado. O marco local atual cobre dois executores
  diretos sobre HTLC nativo em duas chains anvil.
- ~~**Teste de integração real contra anvil.**~~ **FEITO.**
  `swapkit/src/chain.rs::rpc_verifier_reads_real_chain_and_drives_wallet` sobe
  anvil, faz deploy do HTLC, cria a trava, lê via `RpcVerifier` e ainda junta
  leitura→verificação→decisão da máquina de estados. Não é mais `#[ignore]`. O que
  permanece aberto aqui é a **profundidade de confirmação** (anti-reorg) na leitura.
- **A perna Bitcoin.** O diferencial central e a parte mais difícil. A leitura
  trustless do Bitcoin (SPV/prova de inclusão) é um projeto próprio ("Keystone"). A
  escolha de SHA-256 (ADR-001) mantém essa porta aberta, mas a fundação do Bitcoin
  ainda está por fechar.
- **Modelo econômico de incentivo de liquidez.** "Por que prover liquidez?" e a
  mitigação econômica do free-option (que recai sobre o taker — ADR-014). Em direção,
  não calibrado nem implementado.
- **Auditoria profissional independente.** Inviolável antes de qualquer valor real.
  Nada aqui foi auditado.

---

## Resumo dos números (reais)

```
Foundry  : 36 testes  (HashedTimelock 9, Order 10, Settlement 16, Vector 1)
orderbook: 26 testes  (lib 25 + integração 1)
maestro  :  9 testes  (lib 6 + e2e 2 + full_flow 1)
swapkit  : 64 testes  (verify 19 + sm 11 + chain 10 + handshake 5 + exec 19, incl. anvil real)
---------------------------------------------------------------
TOTAL    : 135 passando, 0 ignorados
```

# Kael — Estado do Projeto

> Estado honesto do que existe, do que foi decidido mas não construído, e do que
> está em aberto. Números de teste são **reais** (de `forge test` e
> `cargo test --workspace`), não estimados.
>
> Regra inviolável: nada toca fundos reais antes de auditoria profissional
> independente. Tudo abaixo é experimental, não-auditado, testnet/local.

---

## 1. CONSTRUÍDO E TESTADO

**Total: 102 testes passando + 1 ignorado** (36 Foundry + 66 Rust).

### Contratos (Foundry) — 36 testes
| Suíte | Testes | Cobre |
|---|---|---|
| `HashedTimelock.t.sol` | 9 | HTLC: trava/resgate/reembolso, preimage errado, duplo resgate, resgate após expiração, guardas de criação |
| `Order.t.sol` | 10 | EIP-712: assinatura válida/inválida/expirada, endurecimento ECDSA (s alto, v inválido, comprimento, signer zero) |
| `Settlement.t.sol` | 16 | liquidação Abordagem A: autorização+nonce, binding de chain, maker-only (ETH+ERC-20), replay, reembolso ao maker, não-custódia |
| `Vector.t.sol` | 1 | gera o vetor de equivalência EIP-712 on-chain/off-chain |

### `orderbook` (Rust) — 25 testes
- `lib` (24): matching puro price-time, verificação EIP-712 (equivalência com o contrato, provada por vetor), livro em memória + ingestão verificada na borda.
- `server_integration` (1): servidor HTTP real — ingestão verificada + consulta de matches.

### `maestro` (Rust) — 9 testes
- `lib` (6): hashlock SHA-256 (fonte única) + correlação por hashlock e watchdog.
- `e2e` (2): dois anvils, deploy do HTLC, swap correlacionado + preimage capturado; watchdog de expiração.
- `full_flow` (1): capstone livro→liquidação→maestro.

### `swapkit` (Rust) — 32 testes + 1 ignorado
- `verify` (15): verificador de perna oposta (hashlock/token/amount/recipient + gap de timelock assimétrico por papel).
- `sm` (10): máquina de estados interativa (jornadas taker/maker, dois testes críticos de segurança, reembolsos, transições inválidas).
- `chain` (7 + **1 ignorado**): mapeamento Swap→ObservedLock (`exists` = trava ativa) + junção com a verificação. O teste de **integração real contra anvil** está `#[ignore]` (ver §3).

---

## 2. DECIDIDO, NÃO CONSTRUÍDO

Decisões já tomadas, mas ainda sem implementação:

- **Profundidade de confirmação (anti-reorg).** `ChainVerifier::observe_lock` hoje lê
  o estado "agora", sem exigir N confirmações. Para a perna oposta, observar uma
  trava que depois é revertida por reorg é um risco real. Decidido que precisa de uma
  profundidade mínima de confirmação antes de considerar uma trava "observada";
  **não implementado** (provavelmente um parâmetro da verificação/`ChainVerifier`).
- **Quórum de nós.** O `RpcVerifier` confia num único nó. Reforço barato antes do
  SPV completo: consultar múltiplos nós e exigir concordância. Decidido como direção;
  **não implementado**.
- **Acoplamento de fundação a calibrar por chain.** `min_gap` (gap de timelock),
  **profundidade de confirmação** e **tempo de bloco da chain** são **acoplados**: o
  gap seguro depende de quantos blocos/quanto tempo a parte precisa para agir após a
  revelação, que por sua vez depende do tempo de bloco e da profundidade exigida.
  Esses três devem ser **calibrados juntos, por chain** — ainda não há tabela de
  valores nem método de calibração.

---

## 3. ABERTO / A CONSTRUIR

- **O executor.** A camada que pega a `NextAction` da máquina de estados e a executa
  no mundo: assinar e enviar transações reais (travar, resgatar, reembolsar), gerar o
  segredo, observar confirmações. Hoje a máquina **decide**; nada **executa**.
- **Integração ponta a ponta.** Ligar livro → match → máquina de estados → executor →
  chains num fluxo real. As peças existem isoladas; a costura completa não.
- **Teste de integração real contra anvil.** `swapkit/src/chain.rs` tem o stub
  `rpc_verifier_against_real_chain_pending` marcado `#[ignore]`. Os testes atuais
  provam o parsing/montagem e a junção com a verificação — **não** a leitura real de
  um nó. Falta subir anvil, fazer deploy do HTLC, criar um swap e ler via
  `RpcVerifier`.
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
orderbook: 25 testes  (lib 24 + integração 1)
maestro  :  9 testes  (lib 6 + e2e 2 + full_flow 1)
swapkit  : 32 testes  (verify 15 + sm 10 + chain 7) + 1 ignorado (integração real)
---------------------------------------------------------------
TOTAL    : 102 passando + 1 ignorado
```

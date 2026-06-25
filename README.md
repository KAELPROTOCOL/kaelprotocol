# Kael — Protocolo de Swap Atômico Cross-Chain (MVP EVM↔EVM)

Protocolo de swap atômico **não-custodial**. As peças centrais (HTLC, ordem
assinada, livro, liquidador, verificação de carteira) existem e são testadas —
inclusive a leitura de chain **contra um nó real (anvil)**. O fluxo ponta a ponta
conduzido por executor local direto HTLC já existe para o teste de desenvolvimento
(ver `docs/DEV_TEST_RUNBOOK.md` e `docs/ESTADO.md`).

> **Regra inviolável:** nada toca fundos reais antes de auditoria profissional
> independente. Todo este código é experimental, não-auditado, para testnet/local.

## Componentes

| # | Componente | Onde | Estado |
|---|-----------|------|--------|
| 1 | HTLC (`HashedTimelock.sol`) | `contracts/` | ✅ 9 testes |
| 2 | Ordem assinada EIP-712 (`Order.sol`) | `contracts/` | ✅ 10 testes |
| 3 | Liquidador (`Settlement.sol`) | `contracts/` | ✅ 16 testes |
| 4 | Matching + servidor do livro | `orderbook/` | ✅ 26 testes |
| 5 | Maestro (observador/correlação) | `maestro/` | ✅ 9 testes (2 anvils) |
| 6 | Swapkit (verificação + máquina de estados + executor local) | `swapkit/` | ✅ 64 testes (incl. anvil real + e2e executor) |

**Total: 135 testes verdes, 0 ignorados** (36 Foundry + 99 Rust).

> **Honestidade sobre o estado (leia `docs/ESTADO.md`):** os componentes acima
> são provados *isoladamente* e em algumas junções reais (livro→match;
> trava→correlação no maestro; leitura-de-chain→verificação→decisão no swapkit
> contra anvil; executor local direto HTLC em duas chains anvil). O que **ainda NÃO
> existe**: (a) transporte p2p de liquidação; (b) costura completa livro→match→p2p
> handshake→executor; (c) `Settlement` ↔ carteira num fluxo único. O executor local
> é um marco de desenvolvimento, não prontidão para fundos reais.

## Invariantes de fundação

1. **SHA-256 no hashlock** (não keccak256) — verificável também em Bitcoin Script.
   Fonte única em Solidity (`sha256`) e Rust (`maestro::hashlock`). ADR-001.
2. **Domínio EIP-712 chain-agnóstico** — sem `chainId`/`verifyingContract`; o
   binding de chain vive no payload assinado. ADR-005.
3. **Equivalência on-chain/off-chain** — a verificação EIP-712 em Rust recupera
   exatamente o mesmo maker que o `Order.sol`, provado por vetor fixo
   (`vectors/eip712_order.json`).
4. **Não-custódia** — servidor, maestro e `Settlement` nunca movem, congelam ou
   priorizam fundos a favor de terceiros. No pior caso, param; o reembolso vai
   sempre ao maker.
5. **Matching neutro** — função pura, price-time determinístico, `now` é
   parâmetro — e o caminho **servido** pelo livro respeita essa ordem (ADR-004).
6. **HTLC canônico no liquidador** — o `Settlement` trava só no HTLC fixado no
   deploy (imutável), nunca num endereço fornecido na chamada.
7. **Segurança de timelock relativa E absoluta** — a carteira só age contra a
   perna oposta se o gap inter-pernas E a janela de relógio (`now + min_gap`)
   forem seguros; o segredo nunca vaza contra uma perna prestes a expirar.

## Estrutura

```
kael/
├── contracts/                 Foundry (Solidity)
│   ├── src/HashedTimelock.sol  trava/resgata/reembolsa (SHA-256)
│   ├── src/Order.sol           OrderLib EIP-712 chain-agnóstico
│   ├── src/Settlement.sol      liga ordem assinada ↔ HTLC (Abordagem A)
│   └── test/                   HashedTimelock.t, Order.t, Settlement.t, Vector.t
├── orderbook/                 Crate Rust
│   ├── src/order.rs            ordem do livro (espelha Order.sol + created_at)
│   ├── src/matching.rs         matching puro price-time (fonte única do ranking)
│   ├── src/eip712.rs           verificação EIP-712 em Rust (== Order.sol) + sign
│   ├── src/book.rs             estado em memória + ingestão verificada na borda
│   ├── src/server.rs           HTTP (axum): POST /orders, GET /matches
│   └── tests/                  integração HTTP
├── maestro/                   Crate Rust
│   ├── src/hashlock.rs         fonte única do hashlock SHA-256
│   ├── src/correlate.rs        SwapTracker — correlação + watchdog
│   ├── src/watcher.rs          observação on-chain (alloy)
│   └── tests/                  e2e (2 anvils) + full_flow
├── swapkit/                   Crate Rust (a carteira/SDK)
│   ├── src/verify.rs           verificação da perna oposta (gap + janela de relógio)
│   ├── src/sm.rs               máquina de estados interativa (decide, não executa)
│   ├── src/chain.rs            leitura de chain (RpcVerifier) + teste real anvil
│   └── ...
└── vectors/eip712_order.json  vetor de equivalência on-chain/off-chain
```

## Como rodar os testes

```bash
# Contratos (camadas 1–3)
cd contracts && forge test

# Rust (camadas 4–6) — e2e/anvil sobem nós locais automaticamente
cd .. && cargo test --workspace

# Marco local de desenvolvimento: dois executores, dois anvils, HTLC direto
./scripts/run_dev_swap_test.sh
```

## Como rodar os serviços

```bash
# Servidor do livro
KAEL_BIND=127.0.0.1:8080 cargo run -p orderbook --bin orderbook-server

# Maestro (observa duas chains)
KAEL_RPC_A=http://127.0.0.1:8545 KAEL_CHAIN_A=1 KAEL_HTLC_A=0x... \
KAEL_RPC_B=http://127.0.0.1:8546 KAEL_CHAIN_B=10 KAEL_HTLC_B=0x... \
cargo run -p maestro --bin maestro
```

## Fora do escopo deste MVP (fundação em aberto)

Honestamente não construídos — têm decisões de fundação a fechar antes de codar
(detalhe em `docs/ESTADO.md`):

- **Transporte p2p + integração completa com livro/Settlement** — a costura de
  produto que transforma descoberta de match em swap conduzido ponta a ponta.
- **Profundidade de confirmação (anti-reorg)** e quórum de nós na leitura de chain.
- **Liquidez/makers** — o *free-option problem* e o incentivo de liquidez.
- **Bitcoin nativo** (a SHA-256 mantém essa porta aberta) e Solana.
- **Auditoria profissional independente** — inviolável antes de qualquer valor real.
```

# Kael — Protocolo de Swap Atômico Cross-Chain (MVP EVM↔EVM)

Protocolo de swap atômico **não-custodial**, construído seguindo o mapa do guia
(Partes 0–6). Núcleo demonstrável: um swap cross-chain EVM↔EVM funciona de ponta
a ponta em ambiente local, sem nenhum componente tocar fundos fora dos HTLCs.

> **Regra inviolável:** nada toca fundos reais antes de auditoria profissional
> independente. Todo este código é experimental, não-auditado, para testnet/local.

## Arquitetura (4 camadas)

| Camada | Componente | Onde | Estado |
|--------|-----------|------|--------|
| 1 | HTLC (`HashedTimelock.sol`) | `contracts/` | ✅ 8 testes |
| 2 | Ordem assinada EIP-712 (`Order.sol`) | `contracts/` | ✅ 7 testes |
| 3 | Matching + servidor do livro | `orderbook/` | ✅ 21 testes |
| 4 | Maestro (observador) | `maestro/` | ✅ 9 testes |

**Total: 46 testes verdes** (16 Foundry + 30 Rust).

## Invariantes de fundação

1. **SHA-256 no hashlock** (não keccak256) — verificável também em Bitcoin Script.
   Fonte única em Solidity (`sha256`) e Rust (`maestro::hashlock`). ADR-001.
2. **Domínio EIP-712 chain-agnóstico** — sem `chainId`/`verifyingContract`; o
   binding de chain vive no payload assinado. ADR-005.
3. **Equivalência on-chain/off-chain** — a verificação EIP-712 em Rust recupera
   exatamente o mesmo maker que o `Order.sol`, provado por vetor fixo
   (`vectors/eip712_order.json`).
4. **Não-custódia** — servidor e maestro nunca movem, congelam ou priorizam
   fundos. No pior caso, param.
5. **Matching neutro** — função pura, price-time determinístico, `now` é parâmetro.

## Estrutura

```
kael/
├── contracts/                 Foundry (Solidity)
│   ├── src/HashedTimelock.sol  Camada 1 — trava/resgata/reembolsa (SHA-256)
│   ├── src/Order.sol           Camada 2 — OrderLib EIP-712 chain-agnóstico
│   └── test/                   HashedTimelock.t, Order.t, Vector.t
├── orderbook/                 Crate Rust
│   ├── src/order.rs            ordem do livro (espelha Order.sol + created_at)
│   ├── src/matching.rs         Camada 3 — matching puro price-time
│   ├── src/eip712.rs           verificação EIP-712 em Rust (== Order.sol) + sign
│   ├── src/book.rs             estado em memória + ingestão verificada na borda
│   ├── src/server.rs           HTTP (axum): POST /orders, GET /matches
│   └── tests/                  integração HTTP
├── maestro/                   Crate Rust
│   ├── src/hashlock.rs         fonte única do hashlock SHA-256
│   ├── src/correlate.rs        SwapTracker — correlação + watchdog
│   ├── src/watcher.rs          observação on-chain (alloy)
│   └── tests/                  e2e (2 anvils) + full_flow (Parte 6)
└── vectors/eip712_order.json  vetor de equivalência on-chain/off-chain
```

## Como rodar os testes

```bash
# Camadas 1 e 2 (contratos)
cd contracts && forge test

# Camadas 3 e 4 (Rust) — o e2e sobe anvils locais automaticamente
cd .. && cargo test --workspace
```

O teste capstone (`maestro/tests/full_flow.rs`) demonstra o MVP inteiro:
livro casa ordens assinadas → carteiras liquidam via HTLC em duas chains →
maestro correlaciona pelo hashlock e captura o preimage.

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

Honestamente não construídos — têm decisões de fundação a fechar antes de codar:

- **Parte 7** — liquidez/makers: o *free-option problem* e o incentivo de
  liquidez não estão resolvidos.
- **Parte 8** — Bitcoin nativo (a SHA-256 mantém essa porta aberta) e Solana.
- **Parte 9** — auditoria, modelo de fee, descentralização do livro.

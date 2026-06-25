//! Correlação das duas pernas de um swap pelo hashlock, captura do preimage
//! revelado e detecção de timeout. Lógica PURA (sem chain): recebe eventos já
//! decodificados e mantém estado em memória. `now` entra como parâmetro.

use crate::hashlock::hashlock_from_preimage;
use std::collections::HashMap;

/// Qual perna do swap: a trava de origem ou a de destino. O maestro não precisa
/// saber qual é qual a priori — distingue pelas chains observadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegKind {
    /// Perna observada na chain A (primeira a aparecer para um dado hashlock).
    First,
    /// Perna observada na chain B (segunda a aparecer).
    Second,
}

/// Uma trava HTLC observada numa chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leg {
    pub chain_id: u64,
    pub contract_id: [u8; 32],
    pub hashlock: [u8; 32],
    pub timelock: u64,
    pub amount: u128,
    pub redeemed: bool,
    pub refunded: bool,
}

/// Estado de um swap cross-chain: as pernas observadas + o preimage revelado.
#[derive(Default, Clone, Debug)]
pub struct SwapState {
    pub legs: Vec<Leg>,
    pub preimage: Option<[u8; 32]>,
}

impl SwapState {
    /// Ambas as pernas observadas, em chains distintas → swap correlacionado.
    pub fn correlated(&self) -> bool {
        self.legs.len() >= 2 && {
            let mut chains: Vec<u64> = self.legs.iter().map(|l| l.chain_id).collect();
            chains.sort_unstable();
            chains.dedup();
            chains.len() >= 2
        }
    }
}

/// Rastreador de swaps, indexado por hashlock.
#[derive(Default)]
pub struct SwapTracker {
    swaps: HashMap<[u8; 32], SwapState>,
}

impl SwapTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processa um `LogNewSwap`: registra a perna sob seu hashlock.
    pub fn on_new_swap(
        &mut self,
        chain_id: u64,
        contract_id: [u8; 32],
        hashlock: [u8; 32],
        timelock: u64,
        amount: u128,
    ) -> LegKind {
        let state = self.swaps.entry(hashlock).or_default();
        let kind = if state.legs.is_empty() {
            LegKind::First
        } else {
            LegKind::Second
        };
        // idempotência: não duplica a mesma perna (mesmo chain+contract_id)
        if !state
            .legs
            .iter()
            .any(|l| l.chain_id == chain_id && l.contract_id == contract_id)
        {
            state.legs.push(Leg {
                chain_id,
                contract_id,
                hashlock,
                timelock,
                amount,
                redeemed: false,
                refunded: false,
            });
        }
        kind
    }

    /// Processa um `LogRedeem`: o preimage é revelado no evento. Derivamos o
    /// hashlock por SHA-256 (correlação!) e o guardamos — é ele que destrava a
    /// outra perna. Retorna o hashlock correlato se o preimage for consistente.
    pub fn on_redeem(
        &mut self,
        chain_id: u64,
        contract_id: [u8; 32],
        preimage: [u8; 32],
    ) -> Option<[u8; 32]> {
        let hashlock = hashlock_from_preimage(&preimage);
        let state = self.swaps.get_mut(&hashlock)?;
        state.preimage = Some(preimage);
        for l in state.legs.iter_mut() {
            if l.chain_id == chain_id && l.contract_id == contract_id {
                l.redeemed = true;
            }
        }
        Some(hashlock)
    }

    /// Processa um `LogRefund`: marca a perna reembolsada.
    pub fn on_refund(&mut self, chain_id: u64, contract_id: [u8; 32], hashlock: [u8; 32]) {
        if let Some(state) = self.swaps.get_mut(&hashlock) {
            for l in state.legs.iter_mut() {
                if l.chain_id == chain_id && l.contract_id == contract_id {
                    l.refunded = true;
                }
            }
        }
    }

    pub fn get(&self, hashlock: &[u8; 32]) -> Option<&SwapState> {
        self.swaps.get(hashlock)
    }

    /// Encontra o hashlock de uma perna a partir de (chain_id, contract_id).
    /// Usado para resolver `LogRefund` (que não carrega o hashlock).
    pub fn hashlock_of(&self, chain_id: u64, contract_id: [u8; 32]) -> Option<[u8; 32]> {
        for (h, s) in &self.swaps {
            if s.legs
                .iter()
                .any(|l| l.chain_id == chain_id && l.contract_id == contract_id)
            {
                return Some(*h);
            }
        }
        None
    }

    /// O preimage capturado para um swap (se já revelado em alguma perna).
    pub fn preimage_for(&self, hashlock: &[u8; 32]) -> Option<[u8; 32]> {
        self.swaps.get(hashlock).and_then(|s| s.preimage)
    }

    /// Swaps correlacionados (as duas pernas observadas em chains distintas).
    pub fn correlated_hashlocks(&self) -> Vec<[u8; 32]> {
        self.swaps
            .iter()
            .filter(|(_, s)| s.correlated())
            .map(|(h, _)| *h)
            .collect()
    }

    /// Watchdog: pernas expiradas (passou o timelock) ainda sem resgate nem
    /// reembolso. Devolve (hashlock, chain_id, contract_id). `now` é parâmetro.
    pub fn timed_out(&self, now: u64) -> Vec<([u8; 32], u64, [u8; 32])> {
        let mut out = Vec::new();
        for (h, s) in &self.swaps {
            for l in &s.legs {
                if now >= l.timelock && !l.redeemed && !l.refunded {
                    out.push((*h, l.chain_id, l.contract_id));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashlock::hashlock_from_preimage;

    fn cid(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn correlates_two_legs_and_captures_preimage() {
        let preimage = [7u8; 32];
        let hashlock = hashlock_from_preimage(&preimage);

        let mut t = SwapTracker::new();
        // trava na chain A
        assert_eq!(
            t.on_new_swap(1, cid(0xA1), hashlock, 1000, 100),
            LegKind::First
        );
        // trava na chain B, mesmo hashlock
        assert_eq!(
            t.on_new_swap(10, cid(0xB1), hashlock, 900, 200),
            LegKind::Second
        );

        // ainda não correlacionado? sim — duas pernas em chains distintas
        assert!(t.get(&hashlock).unwrap().correlated());
        assert_eq!(t.correlated_hashlocks(), vec![hashlock]);

        // resgate na chain B revela o preimage; o maestro o captura
        let h = t.on_redeem(10, cid(0xB1), preimage);
        assert_eq!(h, Some(hashlock));
        assert_eq!(t.preimage_for(&hashlock), Some(preimage));

        // a perna de B está marcada como resgatada
        let s = t.get(&hashlock).unwrap();
        assert!(s.legs.iter().find(|l| l.chain_id == 10).unwrap().redeemed);
    }

    #[test]
    fn redeem_with_unknown_preimage_is_ignored() {
        let mut t = SwapTracker::new();
        let hashlock = hashlock_from_preimage(&[7u8; 32]);
        t.on_new_swap(1, cid(0xA1), hashlock, 1000, 100);
        // preimage que não corresponde a nenhum swap conhecido
        assert_eq!(t.on_redeem(1, cid(0xA1), [9u8; 32]), None);
        assert_eq!(t.preimage_for(&hashlock), None);
    }

    #[test]
    fn watchdog_detects_expired_unredeemed_leg() {
        let mut t = SwapTracker::new();
        let hashlock = hashlock_from_preimage(&[3u8; 32]);
        t.on_new_swap(1, cid(0xA1), hashlock, 1000, 100);

        // antes do prazo: nada expirado
        assert!(t.timed_out(999).is_empty());
        // após o prazo, sem resgate: expirado
        let to = t.timed_out(1000);
        assert_eq!(to.len(), 1);
        assert_eq!(to[0].0, hashlock);

        // se foi reembolsado, sai do watchdog
        t.on_refund(1, cid(0xA1), hashlock);
        assert!(t.timed_out(2000).is_empty());
    }

    #[test]
    fn single_leg_not_correlated() {
        let mut t = SwapTracker::new();
        let hashlock = hashlock_from_preimage(&[5u8; 32]);
        t.on_new_swap(1, cid(0xA1), hashlock, 1000, 100);
        assert!(!t.get(&hashlock).unwrap().correlated());
        assert!(t.correlated_hashlocks().is_empty());
    }
}

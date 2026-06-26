//!
//! todo o maestro e seus testes. Centralizar evita o bug silencioso de indexar

use sha2::{Digest, Sha256};

/// Deriva o hashlock (32 bytes) de um preimage (32 bytes) via SHA-256.
pub fn hashlock_from_preimage(preimage: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(preimage);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // SHA-256 de 32 bytes 0x00 (vetor conhecido).
        let pre = [0u8; 32];
        let hl = hashlock_from_preimage(&pre);
        assert_eq!(
            hex::encode(hl),
            "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
        );
    }

    #[test]
    fn distinct_preimages_distinct_hashlocks() {
        let a = hashlock_from_preimage(&[1u8; 32]);
        let b = hashlock_from_preimage(&[2u8; 32]);
        assert_ne!(a, b);
    }
}

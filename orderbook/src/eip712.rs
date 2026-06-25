//! Verificação EIP-712 na borda — equivalente em Rust ao `Order.sol`.
//!
//! PONTO DE FUNDAÇÃO CRÍTICO: esta verificação DEVE recuperar o mesmo `maker`
//! que o contrato recuperaria para a mesma ordem + assinatura. Divergência aqui
//! é um furo silencioso (ordem aceita off-chain falharia on-chain). A
//! equivalência é provada por um vetor de teste fixo (ver testes).
//!
//! Domínio CHAIN-AGNÓSTICO (ADR-005): `EIP712Domain(string name,string version)`
//! — sem chainId, sem verifyingContract.

use crate::order::Order;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

/// metade da ordem da curva secp256k1 (EIP-2). s acima disso é maleável.
/// n/2 = 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0
const SECP256K1_HALF_N: [u8; 32] = [
    0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D, 0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
];

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    BadSignatureLength,
    MalleableS,
    InvalidV,
    BadSignature,
    ZeroSigner,
    SignerNotMaker,
    OrderExpired,
}

fn keccak(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    h.finalize().into()
}

/// abi.encode de um address: 12 bytes zero + 20 bytes do endereço.
fn enc_addr(a: &[u8; 20], out: &mut Vec<u8>) {
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(a);
}

/// abi.encode de um uint256 a partir de um u128 (left-pad para 32 bytes).
fn enc_u128(v: u128, out: &mut Vec<u8>) {
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&v.to_be_bytes());
}

/// abi.encode de um uint256 a partir de um u64 (left-pad para 32 bytes).
fn enc_u64(v: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(&v.to_be_bytes());
}

/// keccak256("EIP712Domain(string name,string version)")
fn domain_typehash() -> [u8; 32] {
    keccak(b"EIP712Domain(string name,string version)")
}

fn order_typehash() -> [u8; 32] {
    keccak(
        b"Order(address maker,address sellToken,uint256 sellChainId,uint256 sellAmount,address buyToken,uint256 buyChainId,uint256 buyAmount,uint256 validUntil,uint256 nonce)",
    )
}

/// domainSeparator = keccak(abi.encode(DOMAIN_TYPEHASH, keccak("Kael"), keccak("1")))
pub fn domain_separator() -> [u8; 32] {
    let mut buf = Vec::with_capacity(96);
    buf.extend_from_slice(&domain_typehash());
    buf.extend_from_slice(&keccak(b"Kael"));
    buf.extend_from_slice(&keccak(b"1"));
    keccak(&buf)
}

/// keccak(abi.encode(ORDER_TYPEHASH, ...campos...))
pub fn hash_struct(o: &Order) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 10);
    buf.extend_from_slice(&order_typehash());
    enc_addr(&o.maker, &mut buf);
    enc_addr(&o.sell_token, &mut buf);
    enc_u64(o.sell_chain_id, &mut buf);
    enc_u128(o.sell_amount, &mut buf);
    enc_addr(&o.buy_token, &mut buf);
    enc_u64(o.buy_chain_id, &mut buf);
    enc_u128(o.buy_amount, &mut buf);
    enc_u64(o.valid_until, &mut buf);
    enc_u64(o.nonce, &mut buf);
    keccak(&buf)
}

/// digest EIP-712 = keccak(0x1901 ‖ domainSeparator ‖ hashStruct)
pub fn digest(o: &Order) -> [u8; 32] {
    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(&domain_separator());
    buf.extend_from_slice(&hash_struct(o));
    keccak(&buf)
}

/// Deriva o endereço Ethereum (20 bytes) de uma VerifyingKey.
fn address_of(vk: &VerifyingKey) -> [u8; 20] {
    let point = vk.to_encoded_point(false); // descomprimido: 0x04 ‖ X ‖ Y
    let bytes = point.as_bytes();
    let hash = keccak(&bytes[1..]); // ignora o prefixo 0x04
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

/// Verifica a assinatura do maker e a validade temporal.
/// `signature` é `r ‖ s ‖ v` (65 bytes), idêntico ao formato do contrato.
/// Retorna o digest EIP-712 (chave canônica para rastrear nonce).
pub fn verify(o: &Order, signature: &[u8], now: u64) -> Result<[u8; 32], VerifyError> {
    if signature.len() != 65 {
        return Err(VerifyError::BadSignatureLength);
    }
    let r = &signature[0..32];
    let s = &signature[32..64];
    let v = signature[64];

    // Endurecimento ECDSA: rejeita s alto (maleabilidade, EIP-2).
    if is_high_s(s) {
        return Err(VerifyError::MalleableS);
    }
    if v != 27 && v != 28 {
        return Err(VerifyError::InvalidV);
    }

    let sig =
        Signature::from_slice(signature[0..64].as_ref()).map_err(|_| VerifyError::BadSignature)?;
    let _ = r; // r/s já estão dentro de `sig`; mantidos acima para clareza
    let recid = RecoveryId::from_byte(v - 27).ok_or(VerifyError::InvalidV)?;

    let dig = digest(o);
    let vk = VerifyingKey::recover_from_prehash(&dig, &sig, recid)
        .map_err(|_| VerifyError::BadSignature)?;
    let signer = address_of(&vk);

    // PARIDADE Rust↔Solidity: uma assinatura com r=0 é REJEITADA em ambos (propriedade de segurança intacta).
    // Os rótulos diferem: Solidity recupera address(0) e reverte ZeroSigner; Rust rejeita antes em
    // Signature::from_slice e retorna BadSignature. A variante ZeroSigner abaixo é defensiva/inalcançável
    // com k256 atual. Divergência cosmética, sem efeito de consenso (o que importa: ambos rejeitam).
    // Ver teste eip712::tests::zero_r_signature_is_rejected.
    if signer == [0u8; 20] {
        return Err(VerifyError::ZeroSigner);
    }
    if signer != o.maker {
        return Err(VerifyError::SignerNotMaker);
    }
    if now > o.valid_until {
        return Err(VerifyError::OrderExpired);
    }
    Ok(dig)
}

/// s é maior que n/2? (comparação big-endian byte a byte)
fn is_high_s(s: &[u8]) -> bool {
    s > &SECP256K1_HALF_N[..]
}

/// Deriva o endereço Ethereum (20 bytes) de uma chave privada (32 bytes).
pub fn address_from_private_key(private_key: &[u8; 32]) -> [u8; 20] {
    let sk = SigningKey::from_slice(private_key).expect("chave privada inválida");
    address_of(sk.verifying_key())
}

/// Assina a ordem com `private_key` (32 bytes), produzindo `r‖s‖v` (65 bytes)
/// no mesmo formato que o contrato consome. k256 normaliza para low-s, então a
/// assinatura sempre passa o endurecimento ECDSA. Útil para testes e clientes.
pub fn sign(o: &Order, private_key: &[u8; 32]) -> [u8; 65] {
    let sk = SigningKey::from_slice(private_key).expect("chave privada inválida");
    let dig = digest(o);
    let (sig, recid) = sk.sign_prehash_recoverable(&dig).expect("falha ao assinar");
    let mut out = [0u8; 65];
    out[..64].copy_from_slice(&sig.to_bytes());
    out[64] = 27 + recid.to_byte();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(hex_str: &str) -> [u8; 20] {
        let bytes = hex::decode(hex_str.trim_start_matches("0x")).unwrap();
        let mut a = [0u8; 20];
        a.copy_from_slice(&bytes);
        a
    }

    fn b32(hex_str: &str) -> [u8; 32] {
        let bytes = hex::decode(hex_str.trim_start_matches("0x")).unwrap();
        let mut a = [0u8; 32];
        a.copy_from_slice(&bytes);
        a
    }

    /// Ordem do vetor fixo (vectors/eip712_order.json), idêntica à de Vector.t.sol.
    fn vector_order() -> Order {
        Order {
            maker: addr("0x06884b215DE85bE18f99a04d05108524Edc88d82"),
            sell_token: addr("0x1111111111111111111111111111111111111111"),
            sell_chain_id: 1,
            sell_amount: 1_000_000_000_000_000_000,
            buy_token: addr("0x2222222222222222222222222222222222222222"),
            buy_chain_id: 10,
            buy_amount: 2_000_000_000,
            valid_until: 2_000_000_000,
            nonce: 42,
            created_at: 0,
        }
    }

    fn vector_signature() -> Vec<u8> {
        let mut sig = Vec::with_capacity(65);
        sig.extend_from_slice(&b32(
            "0x30ce4ef4a9e0ffa123e5bf6e588550416fe77bd947cf2bd8203453591f2ca6ce",
        ));
        sig.extend_from_slice(&b32(
            "0x22be7efbadb7f464cdaeb3ad68c2ad60d750e7dfe1572f9ea6f4d9ba5520d42e",
        ));
        sig.push(28);
        sig
    }

    // EQUIVALÊNCIA on-chain/off-chain: os hashes intermediários batem com Solidity.
    #[test]
    fn domain_separator_matches_solidity() {
        assert_eq!(
            domain_separator(),
            b32("0xc420abb1f32a367b5e624c22fe23edff18215146e3e5e59114624452b0296f41")
        );
    }

    #[test]
    fn struct_hash_matches_solidity() {
        assert_eq!(
            hash_struct(&vector_order()),
            b32("0xbeab77fa79633dfb42b2ea8c42ca94f87a408b3302d76d0418b3adce01bade9d")
        );
    }

    #[test]
    fn digest_matches_solidity() {
        assert_eq!(
            digest(&vector_order()),
            b32("0x6b63f6d4e04665700cac5a401bd965d8ff95b2ee26e0ba8924cd79d57f50e3a1")
        );
    }

    // CONSISTÊNCIA: a verificação recupera o MESMO maker do vetor.
    #[test]
    fn verify_recovers_vector_maker() {
        let o = vector_order();
        let r = verify(&o, &vector_signature(), 1_000_000_000);
        assert_eq!(r, Ok(digest(&o)));
    }

    #[test]
    fn tampered_order_rejected() {
        let mut o = vector_order();
        o.buy_amount = 9999; // adultera após assinar
        assert_eq!(
            verify(&o, &vector_signature(), 1_000_000_000),
            Err(VerifyError::SignerNotMaker)
        );
    }

    #[test]
    fn expired_order_rejected() {
        let o = vector_order();
        assert_eq!(
            verify(&o, &vector_signature(), o.valid_until + 1),
            Err(VerifyError::OrderExpired)
        );
    }

    // O assinador Rust produz uma assinatura que verifica e recupera o maker
    // da chave privada do vetor — fecha o ciclo sign→verify.
    #[test]
    fn sign_then_verify_roundtrip() {
        let pk = b32("0x00000000000000000000000000000000000000000000000000000000c0ffee01");
        let o = vector_order();
        let sig = sign(&o, &pk);
        assert_eq!(verify(&o, &sig, 1_000_000_000), Ok(digest(&o)));
    }

    // ===== GRUPO 1 — paridade do endurecimento ECDSA (lado Rust) =====

    // ordem completa da curva secp256k1 (n).
    const SECP256K1_N: [u8; 32] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B, 0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36,
        0x41, 0x41,
    ];

    // subtração big-endian de 32 bytes (a >= b), usada para s -> n - s.
    fn sub_be(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut borrow: i16 = 0;
        for i in (0..32).rev() {
            let mut diff = a[i] as i16 - b[i] as i16 - borrow;
            if diff < 0 {
                diff += 256;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out[i] = diff as u8;
        }
        out
    }

    // s alto (maleabilidade): troca s por (n - s) e inverte v; deve dar MalleableS.
    #[test]
    fn high_s_rejected() {
        let o = vector_order();
        let base = vector_signature(); // r ‖ s(low) ‖ v(28)
        let mut s = [0u8; 32];
        s.copy_from_slice(&base[32..64]);
        let high_s = sub_be(&SECP256K1_N, &s);

        let mut sig = Vec::with_capacity(65);
        sig.extend_from_slice(&base[0..32]); // r
        sig.extend_from_slice(&high_s); // s alto
        sig.push(if base[64] == 27 { 28 } else { 27 }); // v invertido

        assert_eq!(
            verify(&o, &sig, 1_000_000_000),
            Err(VerifyError::MalleableS)
        );
    }

    // v inválido (fora de {27,28}); deve dar InvalidV.
    #[test]
    fn invalid_v_rejected() {
        let o = vector_order();
        let mut sig = vector_signature();
        sig[64] = 29; // v inválido
        assert_eq!(verify(&o, &sig, 1_000_000_000), Err(VerifyError::InvalidV));
    }

    // comprimento errado (≠ 65); deve dar BadSignatureLength.
    #[test]
    fn bad_length_rejected() {
        let o = vector_order();
        let short = vec![0u8; 64];
        assert_eq!(
            verify(&o, &short, 1_000_000_000),
            Err(VerifyError::BadSignatureLength)
        );
    }

    // r = 0: o ponto do teste é provar que essa assinatura é REJEITADA, não
    // qual rótulo de erro sai.
    //
    // PARIDADE Rust↔Solidity: uma assinatura com r=0 é REJEITADA em ambos (propriedade de segurança intacta).
    // Os rótulos diferem: Solidity recupera address(0) e reverte ZeroSigner; Rust rejeita antes em
    // Signature::from_slice e retorna BadSignature. A variante ZeroSigner no Rust é defensiva/inalcançável
    // com k256 atual. Divergência cosmética, sem efeito de consenso (o que importa: ambos rejeitam).
    #[test]
    fn zero_r_signature_is_rejected() {
        let o = vector_order();
        let base = vector_signature();
        let mut sig = Vec::with_capacity(65);
        sig.extend_from_slice(&[0u8; 32]); // r = 0
        sig.extend_from_slice(&base[32..64]); // s (low, válido)
        sig.push(27);

        let result = verify(&o, &sig, 1_000_000_000);
        // 1) propriedade que importa: r=0 é rejeitado (qualquer Err serve).
        assert!(result.is_err(), "r=0 deve ser rejeitado, veio: {result:?}");
        // 2) divergência documentada: no Rust o rótulo é BadSignature (k256 erra
        //    em Signature::from_slice antes da recuperação), NÃO ZeroSigner.
        assert_eq!(result, Err(VerifyError::BadSignature));
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title OrderLib — Camada 2 do protocolo Kael
/// @notice Primitiva de autorização: o maker ASSINA os termos de um swap
///         (EIP-712) sem mover fundos. A assinatura tranca os termos e previne
///         replay. Biblioteca pura e STATELESS — o consumidor controla nonce.
/// @dev    FUNDAÇÃO (ADR-005): o domínio EIP-712 é CHAIN-AGNÓSTICO. Omite
///         chainId e verifyingContract porque a ordem é inerentemente
///         cross-chain e já carrega seus próprios sellChainId/buyChainId
///         assinados. O binding de chain vive no payload, não no domínio.
library OrderLib {
    struct Order {
        address maker;
        address sellToken;
        uint256 sellChainId;
        uint256 sellAmount;
        address buyToken;
        uint256 buyChainId;
        uint256 buyAmount;
        uint256 validUntil; // expiração (timestamp)
        uint256 nonce; // único por maker (anti-replay)
    }

    /// EIP712Domain SEM chainId e SEM verifyingContract (chain-agnóstico).
    bytes32 internal constant DOMAIN_TYPEHASH = keccak256("EIP712Domain(string name,string version)");

    bytes32 internal constant ORDER_TYPEHASH = keccak256(
        "Order(address maker,address sellToken,uint256 sellChainId,uint256 sellAmount,address buyToken,uint256 buyChainId,uint256 buyAmount,uint256 validUntil,uint256 nonce)"
    );

    // metade da ordem da curva secp256k1 (EIP-2): s acima disso é maleável.
    uint256 internal constant SECP256K1_HALF_N =
        0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0;

    error InvalidSignatureLength();
    error MalleableS();
    error InvalidV();
    error ZeroSigner();
    error SignerNotMaker();
    error OrderExpired();

    function domainSeparator() internal pure returns (bytes32) {
        return keccak256(abi.encode(DOMAIN_TYPEHASH, keccak256("Kael"), keccak256("1")));
    }

    function hashStruct(Order memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                ORDER_TYPEHASH,
                order.maker,
                order.sellToken,
                order.sellChainId,
                order.sellAmount,
                order.buyToken,
                order.buyChainId,
                order.buyAmount,
                order.validUntil,
                order.nonce
            )
        );
    }

    /// @notice Digest EIP-712: \x19\x01 ‖ domainSeparator ‖ hashStruct(order).
    function hash(Order memory order) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19\x01", domainSeparator(), hashStruct(order)));
    }

    /// @notice Verifica a assinatura do maker e a validade temporal.
    /// @param nowTs timestamp de referência (parâmetro, para pureza/testabilidade)
    /// @return orderHash o digest EIP-712 — chave canônica para rastrear nonce usado
    function verify(Order memory order, bytes memory signature, uint256 nowTs)
        internal
        pure
        returns (bytes32 orderHash)
    {
        if (signature.length != 65) revert InvalidSignatureLength();

        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := mload(add(signature, 0x20))
            s := mload(add(signature, 0x40))
            v := byte(0, mload(add(signature, 0x60)))
        }

        // Endurecimento ECDSA (sem isto, há caminho de replay mesmo com nonce):
        if (uint256(s) > SECP256K1_HALF_N) revert MalleableS();
        if (v != 27 && v != 28) revert InvalidV();

        orderHash = hash(order);
        address signer = ecrecover(orderHash, v, r, s);

        if (signer == address(0)) revert ZeroSigner();
        if (signer != order.maker) revert SignerNotMaker();
        if (nowTs > order.validUntil) revert OrderExpired();
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

library OrderLib {
    struct Order {
        address maker;
        address sellToken;
        uint256 sellChainId;
        uint256 sellAmount;
        address buyToken;
        uint256 buyChainId;
        uint256 buyAmount;
        uint256 validUntil; // expiry timestamp
        uint256 nonce; // unique per maker (anti-replay)
    }

        bytes32 internal constant DOMAIN_TYPEHASH = keccak256("EIP712Domain(string name,string version)");

    bytes32 internal constant ORDER_TYPEHASH = keccak256(
        "Order(address maker,address sellToken,uint256 sellChainId,uint256 sellAmount,address buyToken,uint256 buyChainId,uint256 buyAmount,uint256 validUntil,uint256 nonce)"
    );

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

        function hash(Order memory order) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked("\x19\x01", domainSeparator(), hashStruct(order)));
    }

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

        if (uint256(s) > SECP256K1_HALF_N) revert MalleableS();
        if (v != 27 && v != 28) revert InvalidV();

        orderHash = hash(order);
        address signer = ecrecover(orderHash, v, r, s);

        if (signer == address(0)) revert ZeroSigner();
        if (signer != order.maker) revert SignerNotMaker();
        if (nowTs > order.validUntil) revert OrderExpired();
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console2} from "forge-std/Test.sol";
import {OrderLib} from "../src/Order.sol";

/// Gera o vetor de equivalência on-chain/off-chain. Roda com:
///   forge test --match-test test_EmitVector -vv
/// Os valores impressos são a verdade-base para o verificador em Rust (Parte 5).
contract VectorTest is Test {
    function test_EmitVector() public {
        // Chave privada fixa e conhecida (NÃO usar com fundos reais).
        uint256 pk = 0x00000000000000000000000000000000000000000000000000000000c0ffee01;
        address maker = vm.addr(pk);

        OrderLib.Order memory o = OrderLib.Order({
            maker: maker,
            sellToken: 0x1111111111111111111111111111111111111111,
            sellChainId: 1,
            sellAmount: 1000000000000000000, // 1e18
            buyToken: 0x2222222222222222222222222222222222222222,
            buyChainId: 10,
            buyAmount: 2000000000, // 2000e6
            validUntil: 2000000000,
            nonce: 42
        });

        bytes32 domain = OrderLib.domainSeparator();
        bytes32 structHash = OrderLib.hashStruct(o);
        bytes32 digest = OrderLib.hash(o);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);

        console2.log("maker");
        console2.logAddress(maker);
        console2.log("domainSeparator");
        console2.logBytes32(domain);
        console2.log("structHash");
        console2.logBytes32(structHash);
        console2.log("digest");
        console2.logBytes32(digest);
        console2.log("r");
        console2.logBytes32(r);
        console2.log("s");
        console2.logBytes32(s);
        console2.log("v");
        console2.logUint(v);

        // sanity: o vetor verifica com a própria biblioteca
        bytes memory sig = abi.encodePacked(r, s, v);
        assertEq(OrderLib.verify(o, sig, o.validUntil - 1), digest);
    }
}

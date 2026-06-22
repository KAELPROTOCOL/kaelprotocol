// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {OrderLib} from "../src/Order.sol";

/// Wrapper para expor as funções internal da biblioteca aos testes.
contract OrderHarness {
    function hash(OrderLib.Order memory o) external pure returns (bytes32) {
        return OrderLib.hash(o);
    }

    function verify(OrderLib.Order memory o, bytes memory sig, uint256 nowTs) external pure returns (bytes32) {
        return OrderLib.verify(o, sig, nowTs);
    }
}

contract OrderTest is Test {
    OrderHarness h;

    uint256 makerPk = 0xA11CE;
    address maker;

    function setUp() public {
        h = new OrderHarness();
        maker = vm.addr(makerPk);
    }

    function _order() internal view returns (OrderLib.Order memory o) {
        o = OrderLib.Order({
            maker: maker,
            sellToken: address(0x1111),
            sellChainId: 1,
            sellAmount: 1 ether,
            buyToken: address(0x2222),
            buyChainId: 10,
            buyAmount: 2000e6,
            validUntil: 1_000_000,
            nonce: 7
        });
    }

    function _sign(OrderLib.Order memory o, uint256 pk) internal view returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, h.hash(o));
        return abi.encodePacked(r, s, v);
    }

    // 1) assinatura válida recupera o maker
    function test_ValidSignature_RecoversMaker() public {
        OrderLib.Order memory o = _order();
        bytes memory sig = _sign(o, makerPk);
        bytes32 oh = h.verify(o, sig, o.validUntil - 1);
        assertEq(oh, h.hash(o));
    }

    // 2) ordem adulterada não recupera o maker
    function test_TamperedOrder_Reverts() public {
        OrderLib.Order memory o = _order();
        bytes memory sig = _sign(o, makerPk);
        o.buyAmount = 9999e6; // adultera após assinar
        vm.expectRevert(OrderLib.SignerNotMaker.selector);
        h.verify(o, sig, o.validUntil - 1);
    }

    // 3) signatário errado é rejeitado
    function test_WrongSigner_Reverts() public {
        OrderLib.Order memory o = _order();
        bytes memory sig = _sign(o, 0xB0B); // chave diferente do maker
        vm.expectRevert(OrderLib.SignerNotMaker.selector);
        h.verify(o, sig, o.validUntil - 1);
    }

    // 4) ordem expirada é rejeitada
    function test_ExpiredOrder_Reverts() public {
        OrderLib.Order memory o = _order();
        bytes memory sig = _sign(o, makerPk);
        vm.expectRevert(OrderLib.OrderExpired.selector);
        h.verify(o, sig, o.validUntil + 1);
    }

    // 5) limite exato de expiração ainda é válido
    function test_ExpiryBoundary_StillValid() public {
        OrderLib.Order memory o = _order();
        bytes memory sig = _sign(o, makerPk);
        bytes32 oh = h.verify(o, sig, o.validUntil); // nowTs == validUntil
        assertEq(oh, h.hash(o));
    }

    // 6) nonces diferentes geram hashes diferentes
    function test_DifferentNonces_DifferentHashes() public view {
        OrderLib.Order memory a = _order();
        OrderLib.Order memory b = _order();
        b.nonce = a.nonce + 1;
        assertTrue(h.hash(a) != h.hash(b));
    }

    // extra: endurecimento ECDSA — s alto é rejeitado
    function test_MalleableS_Reverts() public {
        OrderLib.Order memory o = _order();
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(makerPk, h.hash(o));
        // flip para a metade alta da curva => maleável
        uint256 n = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;
        bytes32 sHigh = bytes32(n - uint256(s));
        uint8 vFlip = v == 27 ? 28 : 27;
        bytes memory sig = abi.encodePacked(r, sHigh, vFlip);
        vm.expectRevert(OrderLib.MalleableS.selector);
        h.verify(o, sig, o.validUntil - 1);
    }
}

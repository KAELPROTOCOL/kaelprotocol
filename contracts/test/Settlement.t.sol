// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Settlement} from "../src/Settlement.sol";
import {OrderLib} from "../src/Order.sol";
import {HashedTimelock} from "../src/HashedTimelock.sol";
import {MockERC20} from "./MockERC20.sol";

// NOTA DE ARQUITETURA (Abordagem A): a validação cross-leg on-chain foi REMOVIDA.
// Os testes que a exercitavam não existem mais aqui, porque a lógica que testavam
// não existe mais no contrato. Cada um migrou para a CARTEIRA:
//   - test_SettleLeg_NotMirrored_Reverts: o espelhamento contra uma counterOrder
//     alegada não era garantia real (counterOrder não-assinada, não-vinculada à
//     outra chain). A carteira é que confere a perna oposta on-chain.
//   - test_SettleLeg_HashlockMismatch_Reverts: "mesmo hashlock nas duas pernas" é
//     inoperante cross-chain (legCount sempre 0 por chain). A carteira copia o
//     hashlock da trava oposta que ela leu on-chain.
//   - test_SettleLeg_TimelockGapTooSmall_Reverts: o gap seguro depende de ver as
//     duas pernas. A carteira do respondente verifica o timelock da perna oposta
//     (já visível on-chain) ANTES de travar a sua.
//   - test_SettleLeg_SameSwapSameLeg_Reverts: dependia de swaps[swapId]/legCount,
//     removidos. O re-travar a mesma perna é barrado agora pelo nonce (mesma chain)
//     e, em última instância, pelo HTLC (SwapAlreadyExists).
contract SettlementTest is Test {
    Settlement settlement;
    HashedTimelock htlc;
    MockERC20 token; // o ativo "Y" da perna ERC-20

    uint256 makerPk = 0xA11CE;
    address maker; // autorização pura

    uint256 alicePk = 0xA11CE;
    uint256 bobPk = 0xB0B;
    address alice; // vende ETH
    address bob; // vende ERC-20

    bytes32 preimage = keccak256("kael-secret");
    bytes32 HL;

    event LegAuthorized(bytes32 indexed orderHash, address indexed maker, uint256 nonce);

    uint256 constant ETH_AMT = 1 ether;
    uint256 constant TOK_AMT = 5 ether;

    function setUp() public {
        settlement = new Settlement();
        htlc = new HashedTimelock();
        token = new MockERC20();
        maker = vm.addr(makerPk);
        alice = vm.addr(alicePk);
        bob = vm.addr(bobPk);
        HL = sha256(abi.encodePacked(preimage));

        vm.warp(1_000_000);
        vm.deal(alice, 10 ether);
        token.mint(bob, 100 ether);
    }

    function _sign(OrderLib.Order memory o, uint256 pk) internal pure returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, OrderLib.hash(o));
        return abi.encodePacked(r, s, v);
    }

    // ===================== autorização pura (mantida) =====================

    function _order(uint256 nonce) internal view returns (OrderLib.Order memory o) {
        o = OrderLib.Order({
            maker: maker,
            sellToken: address(0x1111),
            sellChainId: 1,
            sellAmount: 1 ether,
            buyToken: address(0x2222),
            buyChainId: 10,
            buyAmount: 2000e6,
            validUntil: 2_000_000,
            nonce: nonce
        });
    }

    function test_AuthorizeLeg_Valid_EmitsAndMarks() public {
        OrderLib.Order memory o = _order(7);
        vm.expectEmit(true, true, true, true);
        emit LegAuthorized(OrderLib.hash(o), maker, 7);
        settlement.authorizeLeg(o, _sign(o, makerPk), o.validUntil - 1);
        assertTrue(settlement.consumedNonce(maker, 7));
    }

    function test_AuthorizeLeg_Replay_Reverts() public {
        OrderLib.Order memory o = _order(7);
        bytes memory sig = _sign(o, makerPk);
        settlement.authorizeLeg(o, sig, o.validUntil - 1);
        vm.expectRevert(Settlement.NonceAlreadyUsed.selector);
        settlement.authorizeLeg(o, sig, o.validUntil - 1);
    }

    function test_AuthorizeLeg_BadSignature_Reverts() public {
        OrderLib.Order memory o = _order(7);
        vm.expectRevert(OrderLib.SignerNotMaker.selector);
        settlement.authorizeLeg(o, _sign(o, 0xBEEF), o.validUntil - 1);
    }

    function test_AuthorizeLeg_Expired_Reverts() public {
        OrderLib.Order memory o = _order(7);
        vm.expectRevert(OrderLib.OrderExpired.selector);
        settlement.authorizeLeg(o, _sign(o, makerPk), o.validUntil + 1);
    }

    function test_AuthorizeLeg_DifferentNonces_BothPass() public {
        OrderLib.Order memory o1 = _order(7);
        OrderLib.Order memory o2 = _order(8);
        settlement.authorizeLeg(o1, _sign(o1, makerPk), o1.validUntil - 1);
        settlement.authorizeLeg(o2, _sign(o2, makerPk), o2.validUntil - 1);
        assertTrue(settlement.consumedNonce(maker, 7));
        assertTrue(settlement.consumedNonce(maker, 8));
    }

    // ===================== liquidação de UMA perna (Abordagem A) =====================

    // Alice vende ETH NESTA chain (sellChainId == block.chainid).
    function _orderA(uint256 nonce) internal view returns (OrderLib.Order memory o) {
        o = OrderLib.Order({
            maker: alice,
            sellToken: address(0),
            sellChainId: block.chainid,
            sellAmount: ETH_AMT,
            buyToken: address(token),
            buyChainId: 10,
            buyAmount: TOK_AMT,
            validUntil: 2_000_000,
            nonce: nonce
        });
    }

    // Bob vende ERC-20 NESTA chain (sellChainId == block.chainid).
    function _orderB(uint256 nonce) internal view returns (OrderLib.Order memory o) {
        o = OrderLib.Order({
            maker: bob,
            sellToken: address(token),
            sellChainId: block.chainid,
            sellAmount: TOK_AMT,
            buyToken: address(0),
            buyChainId: 10,
            buyAmount: ETH_AMT,
            validUntil: 2_000_000,
            nonce: nonce
        });
    }

    function _settleEthLeg(OrderLib.Order memory a, address recipient, bytes32 hl, uint256 tl)
        internal
        returns (bytes32 cid)
    {
        vm.prank(alice);
        cid = settlement.settleLeg{value: ETH_AMT}(a, _sign(a, alicePk), recipient, hl, tl, address(htlc));
    }

    function _settleTokenLeg(OrderLib.Order memory b, address recipient, bytes32 hl, uint256 tl)
        internal
        returns (bytes32 cid)
    {
        bytes memory sigB = _sign(b, bobPk);
        vm.startPrank(bob);
        token.approve(address(settlement), TOK_AMT);
        cid = settlement.settleLeg(b, sigB, recipient, hl, tl, address(htlc));
        vm.stopPrank();
    }

    // caminho feliz: as duas pernas travam no HTLC com recipient explícito;
    // fundos saem dos makers; o Settlement não retém nada.
    function test_SettleLeg_HappyPath_BothLegsLocked() public {
        OrderLib.Order memory a = _orderA(1);
        OrderLib.Order memory b = _orderB(2);
        uint256 tlA = block.timestamp + 2 hours;
        uint256 tlB = block.timestamp + 1 hours;

        uint256 aliceEthBefore = alice.balance;
        uint256 bobTokBefore = token.balanceOf(bob);

        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA); // recipient = bob
        bytes32 cidB = _settleTokenLeg(b, alice, HL, tlB); // recipient = alice

        HashedTimelock.Swap memory sa = htlc.getSwap(cidA);
        assertEq(sa.sender, address(settlement));
        assertEq(sa.recipient, bob);
        assertEq(sa.token, address(0));
        assertEq(sa.amount, ETH_AMT);
        assertEq(sa.hashlock, HL);
        assertEq(sa.timelock, tlA);

        HashedTimelock.Swap memory sb = htlc.getSwap(cidB);
        assertEq(sb.sender, address(settlement));
        assertEq(sb.recipient, alice);
        assertEq(sb.token, address(token));
        assertEq(sb.amount, TOK_AMT);

        assertEq(alice.balance, aliceEthBefore - ETH_AMT);
        assertEq(token.balanceOf(bob), bobTokBefore - TOK_AMT);
        assertEq(address(htlc).balance, ETH_AMT);
        assertEq(token.balanceOf(address(htlc)), TOK_AMT);
        assertEq(address(settlement).balance, 0, "Settlement nao retem ETH");
        assertEq(token.balanceOf(address(settlement)), 0, "Settlement nao retem TOK");
    }

    // BINDING DE CHAIN (novo): ordem cujo sellChainId != block.chainid reverte.
    function test_SettleLeg_WrongChain_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        a.sellChainId = block.chainid + 1; // ordem para OUTRA chain
        bytes memory sigA = _sign(a, alicePk);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(Settlement.WrongChain.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, block.timestamp + 2 hours, address(htlc));

        assertEq(alice.balance, before, "nada saiu de Alice");
    }

    // SÓ O MAKER LIQUIDA: um terceiro não pode liquidar a ordem de outro (ETH).
    function test_SettleLeg_NotMaker_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory sigA = _sign(a, alicePk); // ordem ASSINADA por Alice
        address attacker = address(0xBAD);
        vm.deal(attacker, 10 ether);
        uint256 aliceBefore = alice.balance;

        // mesmo com a ordem+assinatura válidas de Alice, o atacante não é o maker
        vm.prank(attacker);
        vm.expectRevert(Settlement.NotOrderMaker.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, attacker, HL, block.timestamp + 2 hours, address(htlc));

        assertEq(alice.balance, aliceBefore, "nada de Alice se moveu");
        assertEq(address(htlc).balance, 0, "nada travado");
        assertFalse(settlement.consumedNonce(alice, 1), "nonce de Alice intacto");
    }

    // VETOR DE ROUBO ERC-20 FECHADO: Bob deu approve ao Settlement; um atacante
    // tenta liquidar a ordem do Bob com recipient/hashlock próprios para drenar
    // os tokens. Prova que a APROVAÇÃO SOZINHA não basta — só o maker liquida.
    function test_SettleLeg_NotMaker_ERC20_ApprovalAloneCannotSteal() public {
        OrderLib.Order memory b = _orderB(2);
        bytes memory sigB = _sign(b, bobPk); // ordem ASSINADA por Bob

        // pré-condição do "roubo": Bob aprova o Settlement
        vm.prank(bob);
        token.approve(address(settlement), TOK_AMT);

        uint256 bobBefore = token.balanceOf(bob);
        address attacker = address(0xBAD);
        bytes32 attackerHL = sha256(abi.encodePacked(keccak256("attacker-secret")));

        // atacante tenta liquidar a perna do Bob com recipient = atacante
        vm.prank(attacker);
        vm.expectRevert(Settlement.NotOrderMaker.selector);
        settlement.settleLeg(b, sigB, attacker, attackerHL, block.timestamp + 2 hours, address(htlc));

        // a aprovação sozinha NÃO permitiu roubo: tokens do Bob intactos
        assertEq(token.balanceOf(bob), bobBefore, "tokens do Bob intactos");
        assertEq(token.balanceOf(address(htlc)), 0, "nada travado no HTLC");
        assertEq(token.balanceOf(address(settlement)), 0, "Settlement nao reteve token");
        assertFalse(settlement.consumedNonce(bob, 2), "nonce do Bob nao consumido");
    }

    // anti-replay per-chain: a MESMA ordem/nonce duas vezes → 2ª reverte, sem mover fundos.
    function test_SettleLeg_Replay_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        _settleEthLeg(a, bob, HL, tlA); // 1ª: ok

        uint256 aliceAfterFirst = alice.balance;
        bytes memory sigA = _sign(a, alicePk);
        vm.prank(alice);
        vm.expectRevert(Settlement.NonceAlreadyUsed.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, tlA, address(htlc));

        assertEq(alice.balance, aliceAfterFirst, "nenhum ETH a mais saiu de Alice");
        assertEq(address(htlc).balance, ETH_AMT, "HTLC so tem a 1a trava");
        assertEq(address(settlement).balance, 0);
    }

    // assinatura inválida → reverte via OrderLib, fundos não movidos.
    function test_SettleLeg_BadSignature_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory badSig = _sign(a, 0xBEEF);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(OrderLib.SignerNotMaker.selector);
        settlement.settleLeg{value: ETH_AMT}(a, badSig, bob, HL, block.timestamp + 2 hours, address(htlc));

        assertEq(alice.balance, before);
        assertEq(address(settlement).balance, 0);
    }

    // ordem expirada → reverte via OrderLib, fundos não movidos.
    function test_SettleLeg_Expired_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        a.validUntil = block.timestamp - 1;
        bytes memory sigA = _sign(a, alicePk);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(OrderLib.OrderExpired.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, block.timestamp + 2 hours, address(htlc));

        assertEq(alice.balance, before);
    }

    // reembolso: após expirar, refundLeg devolve AO MAKER (não ao caller).
    function test_RefundLeg_ReturnsToMaker() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);

        vm.warp(tlA + 1);
        uint256 aliceBefore = alice.balance;
        address carol = address(0xCA401);
        uint256 carolBefore = carol.balance;

        vm.prank(carol);
        settlement.refundLeg(cidA);

        assertEq(alice.balance, aliceBefore + ETH_AMT, "ETH volta ao maker");
        assertEq(carol.balance, carolBefore, "caller nao recebe nada");
        assertEq(address(settlement).balance, 0, "Settlement zera");
        assertEq(address(htlc).balance, 0, "HTLC liberou a trava");
    }

    function test_RefundLeg_DoubleRefund_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);
        vm.warp(tlA + 1);
        settlement.refundLeg(cidA);
        vm.expectRevert(Settlement.AlreadyRefunded.selector);
        settlement.refundLeg(cidA);
    }

    // perna JÁ RESGATADA não pode ser reembolsada (o HTLC reverte).
    function test_RefundLeg_AfterRedeem_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);

        htlc.redeem(cidA, preimage); // bob (recipient) resgata antes do prazo

        vm.expectRevert(HashedTimelock.AlreadyWithdrawn.selector);
        settlement.refundLeg(cidA);
    }

    // NÃO-CUSTÓDIA: nenhum saldo retido fora de trânsito; só newSwap e refundLeg→maker.
    function test_NonCustody_NoRetainedBalance() public {
        OrderLib.Order memory a = _orderA(1);
        OrderLib.Order memory b = _orderB(2);
        uint256 tlA = block.timestamp + 2 hours;
        uint256 tlB = block.timestamp + 1 hours;

        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);
        _settleTokenLeg(b, alice, HL, tlB);
        assertEq(address(settlement).balance, 0);
        assertEq(token.balanceOf(address(settlement)), 0);

        vm.warp(tlA + 1);
        uint256 aliceBefore = alice.balance;
        settlement.refundLeg(cidA);
        assertEq(alice.balance, aliceBefore + ETH_AMT);
        assertEq(address(settlement).balance, 0, "sem saldo retido");
        assertEq(token.balanceOf(address(settlement)), 0);
    }
}

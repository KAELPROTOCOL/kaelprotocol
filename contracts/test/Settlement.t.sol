// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {Settlement} from "../src/Settlement.sol";
import {OrderLib} from "../src/Order.sol";
import {HashedTimelock} from "../src/HashedTimelock.sol";
import {MockERC20} from "./MockERC20.sol";

// Architecture note (Approach A): cross-leg validation is wallet-side, not
// on-chain. Each Settlement is per-chain and validates only its local leg. The
// wallet verifies the observed counterparty leg before broadcasting, while the
// HTLC contractId binds sender, recipient, token, amount, hashlock, and timelock.
contract SettlementTest is Test {
    Settlement settlement;
    HashedTimelock htlc;
    MockERC20 token; // ERC-20 asset for the token leg.

    uint256 makerPk = 0xA11CE;
    address maker; // pure authorization tests.

    uint256 alicePk = 0xA11CE;
    uint256 bobPk = 0xB0B;
    address alice; // sells ETH.
    address bob; // sells ERC-20.

    bytes32 preimage = keccak256("kael-secret");
    bytes32 HL;

    event LegAuthorized(bytes32 indexed orderHash, address indexed maker, uint256 nonce);

    uint256 constant ETH_AMT = 1 ether;
    uint256 constant TOK_AMT = 5 ether;

    function setUp() public {
        htlc = new HashedTimelock();
        settlement = new Settlement(address(htlc)); // Canonical HTLC fixed at deploy.
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

    // ===================== Pure authorization =====================

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

    // ===================== Single-leg settlement (Approach A) =====================

    // Alice sells ETH on this chain (sellChainId == block.chainid).
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

    // Bob sells ERC-20 on this chain (sellChainId == block.chainid).
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
        cid = settlement.settleLeg{value: ETH_AMT}(a, _sign(a, alicePk), recipient, hl, tl);
    }

    function _settleTokenLeg(OrderLib.Order memory b, address recipient, bytes32 hl, uint256 tl)
        internal
        returns (bytes32 cid)
    {
        bytes memory sigB = _sign(b, bobPk);
        vm.startPrank(bob);
        token.approve(address(settlement), TOK_AMT);
        cid = settlement.settleLeg(b, sigB, recipient, hl, tl);
        vm.stopPrank();
    }

    // Happy path: both legs lock in the HTLC with explicit recipients; funds
    // leave makers and Settlement retains nothing.
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
        assertEq(address(settlement).balance, 0, "Settlement retains no ETH");
        assertEq(token.balanceOf(address(settlement)), 0, "Settlement retains no token");
    }

    function test_SettleLeg_ContractIdBindsRecipientHashlockAndTimelock() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);

        bytes32 wrongRecipient = htlc.computeContractId(address(settlement), alice, address(0), ETH_AMT, HL, tlA);
        bytes32 wrongHashlock = htlc.computeContractId(
            address(settlement), bob, address(0), ETH_AMT, sha256(abi.encodePacked("wrong")), tlA
        );
        bytes32 wrongTimelock = htlc.computeContractId(address(settlement), bob, address(0), ETH_AMT, HL, tlA + 1);

        assertEq(cidA, htlc.computeContractId(address(settlement), bob, address(0), ETH_AMT, HL, tlA));
        assertEq(htlc.getSwap(cidA).recipient, bob);
        assertEq(htlc.getSwap(cidA).hashlock, HL);
        assertEq(htlc.getSwap(cidA).timelock, tlA);
        assertEq(htlc.getSwap(wrongRecipient).sender, address(0));
        assertEq(htlc.getSwap(wrongHashlock).sender, address(0));
        assertEq(htlc.getSwap(wrongTimelock).sender, address(0));
    }

    function test_SettleLeg_ZeroAmount_RevertsAndDoesNotConsumeNonce() public {
        OrderLib.Order memory a = _orderA(1);
        a.sellAmount = 0;
        bytes memory sigA = _sign(a, alicePk);

        vm.prank(alice);
        vm.expectRevert(HashedTimelock.ZeroAmount.selector);
        settlement.settleLeg{value: 0}(a, sigA, bob, HL, block.timestamp + 2 hours);

        assertFalse(settlement.consumedNonce(alice, 1), "nonce remains available after reverted zero-amount settle");
        assertEq(address(settlement).balance, 0);
        assertEq(address(htlc).balance, 0);
    }

    function test_SettleLeg_ZeroHashlock_RevertsAndDoesNotConsumeNonce() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory sigA = _sign(a, alicePk);

        vm.prank(alice);
        vm.expectRevert(HashedTimelock.ZeroHashlock.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, bytes32(0), block.timestamp + 2 hours);

        assertFalse(settlement.consumedNonce(alice, 1), "nonce remains available after reverted zero-hashlock settle");
        assertEq(address(settlement).balance, 0);
        assertEq(address(htlc).balance, 0);
    }

    function test_SettleLeg_TimelockInPast_RevertsAndDoesNotConsumeNonce() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory sigA = _sign(a, alicePk);

        vm.prank(alice);
        vm.expectRevert(HashedTimelock.TimelockInPast.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, block.timestamp);

        assertFalse(settlement.consumedNonce(alice, 1), "nonce remains available after reverted timelock settle");
        assertEq(address(settlement).balance, 0);
        assertEq(address(htlc).balance, 0);
    }

    // Chain binding: an order with sellChainId != block.chainid reverts.
    function test_SettleLeg_WrongChain_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        a.sellChainId = block.chainid + 1; // Order for another chain.
        bytes memory sigA = _sign(a, alicePk);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(Settlement.WrongChain.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, block.timestamp + 2 hours);

        assertEq(alice.balance, before, "nothing left Alice");
    }

    // Only the maker settles: a third party cannot settle another maker's order.
    function test_SettleLeg_NotMaker_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory sigA = _sign(a, alicePk); // Order signed by Alice.
        address attacker = address(0xBAD);
        vm.deal(attacker, 10 ether);
        uint256 aliceBefore = alice.balance;

        // Even with Alice's valid order+signature, attacker is not the maker.
        vm.prank(attacker);
        vm.expectRevert(Settlement.NotOrderMaker.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, attacker, HL, block.timestamp + 2 hours);

        assertEq(alice.balance, aliceBefore, "Alice funds did not move");
        assertEq(address(htlc).balance, 0, "nothing locked");
        assertFalse(settlement.consumedNonce(alice, 1), "Alice nonce intact");
    }

    // ERC-20 theft vector closed: approval alone is not enough; only the maker
    // can settle the maker's own order.
    function test_SettleLeg_NotMaker_ERC20_ApprovalAloneCannotSteal() public {
        OrderLib.Order memory b = _orderB(2);
        bytes memory sigB = _sign(b, bobPk); // Order signed by Bob.

        // Theft precondition: Bob approved Settlement.
        vm.prank(bob);
        token.approve(address(settlement), TOK_AMT);

        uint256 bobBefore = token.balanceOf(bob);
        address attacker = address(0xBAD);
        bytes32 attackerHL = sha256(abi.encodePacked(keccak256("attacker-secret")));

        // Attacker tries to settle Bob's leg with attacker as recipient.
        vm.prank(attacker);
        vm.expectRevert(Settlement.NotOrderMaker.selector);
        settlement.settleLeg(b, sigB, attacker, attackerHL, block.timestamp + 2 hours);

        assertEq(token.balanceOf(bob), bobBefore, "Bob tokens intact");
        assertEq(token.balanceOf(address(htlc)), 0, "nothing locked in HTLC");
        assertEq(token.balanceOf(address(settlement)), 0, "Settlement retained no token");
        assertFalse(settlement.consumedNonce(bob, 2), "Bob nonce not consumed");
    }

    function test_SettleLeg_ERC20_InsufficientAllowance_Reverts() public {
        OrderLib.Order memory b = _orderB(2);
        bytes memory sigB = _sign(b, bobPk);

        vm.prank(bob);
        vm.expectRevert(Settlement.TokenTransferFailed.selector);
        settlement.settleLeg(b, sigB, alice, HL, block.timestamp + 2 hours);

        assertFalse(settlement.consumedNonce(bob, 2), "Bob nonce not consumed");
        assertEq(token.balanceOf(address(settlement)), 0);
        assertEq(token.balanceOf(address(htlc)), 0);
    }

    function test_SettleLeg_ERC20_EoaToken_Reverts() public {
        OrderLib.Order memory b = _orderB(2);
        b.sellToken = address(0xBEEF);
        bytes memory sigB = _sign(b, bobPk);

        vm.prank(bob);
        vm.expectRevert(Settlement.InvalidToken.selector);
        settlement.settleLeg(b, sigB, alice, HL, block.timestamp + 2 hours);

        assertFalse(settlement.consumedNonce(bob, 2), "Bob nonce not consumed");
    }

    // Per-chain anti-replay: the same order/nonce twice reverts on the second attempt.
    function test_SettleLeg_Replay_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        _settleEthLeg(a, bob, HL, tlA);

        uint256 aliceAfterFirst = alice.balance;
        bytes memory sigA = _sign(a, alicePk);
        vm.prank(alice);
        vm.expectRevert(Settlement.NonceAlreadyUsed.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, tlA);

        assertEq(alice.balance, aliceAfterFirst, "no extra ETH left Alice");
        assertEq(address(htlc).balance, ETH_AMT, "HTLC only has the first lock");
        assertEq(address(settlement).balance, 0);
    }

    // Invalid signature reverts through OrderLib and moves no funds.
    function test_SettleLeg_BadSignature_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        bytes memory badSig = _sign(a, 0xBEEF);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(OrderLib.SignerNotMaker.selector);
        settlement.settleLeg{value: ETH_AMT}(a, badSig, bob, HL, block.timestamp + 2 hours);

        assertEq(alice.balance, before);
        assertEq(address(settlement).balance, 0);
    }

    // Expired order reverts through OrderLib and moves no funds.
    function test_SettleLeg_Expired_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        a.validUntil = block.timestamp - 1;
        bytes memory sigA = _sign(a, alicePk);
        uint256 before = alice.balance;

        vm.prank(alice);
        vm.expectRevert(OrderLib.OrderExpired.selector);
        settlement.settleLeg{value: ETH_AMT}(a, sigA, bob, HL, block.timestamp + 2 hours);

        assertEq(alice.balance, before);
    }

    // Refund: after expiry, refundLeg returns funds to the maker, not the caller.
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

        assertEq(alice.balance, aliceBefore + ETH_AMT, "ETH returns to maker");
        assertEq(carol.balance, carolBefore, "caller receives nothing");
        assertEq(address(settlement).balance, 0, "Settlement balance is zero");
        assertEq(address(htlc).balance, 0, "HTLC released the lock");
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

    // An already redeemed leg cannot be refunded; the HTLC reverts.
    function test_RefundLeg_AfterRedeem_Reverts() public {
        OrderLib.Order memory a = _orderA(1);
        uint256 tlA = block.timestamp + 2 hours;
        bytes32 cidA = _settleEthLeg(a, bob, HL, tlA);

        htlc.redeem(cidA, preimage); // Bob, the recipient, redeems before expiry.

        vm.expectRevert(HashedTimelock.AlreadyWithdrawn.selector);
        settlement.refundLeg(cidA);
    }

    // Non-custody: no retained balance outside in-flight calls.
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
        assertEq(address(settlement).balance, 0, "no retained balance");
        assertEq(token.balanceOf(address(settlement)), 0);
    }
}

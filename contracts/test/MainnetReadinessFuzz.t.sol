// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {HashedTimelock} from "../src/HashedTimelock.sol";
import {OrderLib} from "../src/Order.sol";
import {Settlement} from "../src/Settlement.sol";

contract OrderFuzzHarness {
    function hash(OrderLib.Order memory order) external pure returns (bytes32) {
        return OrderLib.hash(order);
    }
}

contract MainnetReadinessFuzzTest is Test {
    HashedTimelock htlc;
    Settlement settlement;
    OrderFuzzHarness orderHarness;

    uint256 makerPk = 0xA11CE;
    address maker;
    address recipient = address(0xB0B);

    function setUp() public {
        htlc = new HashedTimelock();
        settlement = new Settlement(address(htlc));
        orderHarness = new OrderFuzzHarness();
        maker = vm.addr(makerPk);
    }

    function testFuzz_HtlcContractIdBindsAllFields(
        address sender,
        address to,
        address token,
        uint96 rawAmount,
        bytes32 hashlock,
        uint64 rawTimelock
    ) public view {
        uint256 amount = bound(uint256(rawAmount), 1, type(uint96).max);
        uint256 timelock = bound(uint256(rawTimelock), 1, type(uint64).max - 1);
        vm.assume(hashlock != bytes32(0));

        bytes32 base = htlc.computeContractId(sender, to, token, amount, hashlock, timelock);

        assertTrue(base != htlc.computeContractId(address(uint160(sender) ^ 1), to, token, amount, hashlock, timelock));
        assertTrue(base != htlc.computeContractId(sender, address(uint160(to) ^ 1), token, amount, hashlock, timelock));
        assertTrue(base != htlc.computeContractId(sender, to, address(uint160(token) ^ 1), amount, hashlock, timelock));
        assertTrue(base != htlc.computeContractId(sender, to, token, amount + 1, hashlock, timelock));
        assertTrue(base != htlc.computeContractId(sender, to, token, amount, bytes32(uint256(hashlock) ^ 1), timelock));
        assertTrue(base != htlc.computeContractId(sender, to, token, amount, hashlock, timelock + 1));
    }

    function testFuzz_HtlcRejectsWrongPreimage(bytes32 preimage, bytes32 wrong, uint96 rawAmount, uint64 rawDelta)
        public
    {
        vm.assume(preimage != wrong);
        uint256 amount = bound(uint256(rawAmount), 1 wei, 10 ether);
        uint256 timelock = block.timestamp + bound(uint256(rawDelta), 1, 30 days);
        bytes32 hashlock = sha256(abi.encodePacked(preimage));

        vm.deal(maker, amount);
        vm.prank(maker);
        bytes32 id = htlc.newSwap{value: amount}(recipient, address(0), amount, hashlock, timelock);

        vm.expectRevert(HashedTimelock.InvalidPreimage.selector);
        htlc.redeem(id, wrong);
    }

    function testFuzz_OrderHashBindsAmountsTokensChainsAndNonce(
        uint96 rawSellAmount,
        uint96 rawBuyAmount,
        uint64 sellChain,
        uint64 buyChain,
        uint64 nonce
    ) public view {
        uint256 sellAmount = bound(uint256(rawSellAmount), 1, type(uint96).max);
        uint256 buyAmount = bound(uint256(rawBuyAmount), 1, type(uint96).max);

        OrderLib.Order memory order = OrderLib.Order({
            maker: maker,
            sellToken: address(0x1111),
            sellChainId: sellChain,
            sellAmount: sellAmount,
            buyToken: address(0x2222),
            buyChainId: buyChain,
            buyAmount: buyAmount,
            validUntil: 2_000_000,
            nonce: nonce
        });
        bytes32 base = orderHarness.hash(order);

        order.sellAmount = sellAmount + 1;
        assertTrue(base != orderHarness.hash(order));
        order.sellAmount = sellAmount;

        order.buyAmount = buyAmount + 1;
        assertTrue(base != orderHarness.hash(order));
        order.buyAmount = buyAmount;

        order.sellToken = address(0x3333);
        assertTrue(base != orderHarness.hash(order));
        order.sellToken = address(0x1111);

        order.buyChainId = uint256(buyChain) + 1;
        assertTrue(base != orderHarness.hash(order));
        order.buyChainId = buyChain;

        order.nonce = uint256(nonce) + 1;
        assertTrue(base != orderHarness.hash(order));
    }

    function testFuzz_SettlementEthLegLocksExactSignedAmount(uint96 rawAmount, uint64 rawDelta, uint64 nonce)
        public
    {
        uint256 amount = bound(uint256(rawAmount), 1 wei, 10 ether);
        uint256 timelock = block.timestamp + bound(uint256(rawDelta), 1, 30 days);
        bytes32 hashlock = sha256(abi.encodePacked(bytes32(uint256(0xCAFE))));

        OrderLib.Order memory order = OrderLib.Order({
            maker: maker,
            sellToken: address(0),
            sellChainId: block.chainid,
            sellAmount: amount,
            buyToken: address(0x2222),
            buyChainId: 31338,
            buyAmount: amount,
            validUntil: block.timestamp + 1 days,
            nonce: nonce
        });

        (uint8 v, bytes32 r, bytes32 s) = vm.sign(makerPk, OrderLib.hash(order));
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(maker, amount);
        vm.prank(maker);
        bytes32 id = settlement.settleLeg{value: amount}(order, signature, recipient, hashlock, timelock);

        HashedTimelock.Swap memory locked = htlc.getSwap(id);
        assertEq(locked.sender, address(settlement));
        assertEq(locked.recipient, recipient);
        assertEq(locked.token, address(0));
        assertEq(locked.amount, amount);
        assertEq(locked.hashlock, hashlock);
        assertEq(locked.timelock, timelock);
        assertEq(address(htlc).balance, amount);
        assertEq(address(settlement).balance, 0);
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {HashedTimelock} from "../src/HashedTimelock.sol";
import {MockERC20} from "./MockERC20.sol";

contract HashedTimelockTest is Test {
    HashedTimelock htlc;
    MockERC20 token;

    address sender = address(0xA11CE);
    address recipient = address(0xB0B);

    bytes32 preimage = keccak256("o-segredo-do-kael");
    bytes32 hashlock; // SHA-256 do preimage (fonte de verdade do contrato)
    uint256 timelock;

    function setUp() public {
        htlc = new HashedTimelock();
        token = new MockERC20();
        hashlock = sha256(abi.encodePacked(preimage));
        timelock = block.timestamp + 1 hours;
        vm.deal(sender, 100 ether);
    }

    // 1) resgate ETH com preimage certo
    function test_RedeemEth_Success() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        uint256 balBefore = recipient.balance;
        htlc.redeem(id, preimage);
        assertEq(recipient.balance, balBefore + 1 ether);
        assertTrue(htlc.getSwap(id).withdrawn);
    }

    // 2) resgate ERC-20 com preimage certo
    function test_RedeemErc20_Success() public {
        token.mint(sender, 10 ether);
        vm.startPrank(sender);
        token.approve(address(htlc), 5 ether);
        bytes32 id = htlc.newSwap(recipient, address(token), 5 ether, hashlock, timelock);
        vm.stopPrank();

        htlc.redeem(id, preimage);
        assertEq(token.balanceOf(recipient), 5 ether);
        assertEq(token.balanceOf(address(htlc)), 0);
    }

    // 3) resgate com preimage errado => revert
    function test_RedeemWrongPreimage_Reverts() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        vm.expectRevert(HashedTimelock.InvalidPreimage.selector);
        htlc.redeem(id, keccak256("errado"));
    }

    // 4) duplo resgate => revert
    function test_DoubleRedeem_Reverts() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        htlc.redeem(id, preimage);
        vm.expectRevert(HashedTimelock.AlreadyWithdrawn.selector);
        htlc.redeem(id, preimage);
    }

    // 5) refund antes do prazo => revert
    function test_RefundBeforeTimelock_Reverts() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        vm.prank(sender);
        vm.expectRevert(HashedTimelock.TimelockNotExpired.selector);
        htlc.refund(id);
    }

    // 6) refund após o prazo => sucesso
    function test_RefundAfterTimelock_Success() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        vm.warp(timelock + 1);
        uint256 balBefore = sender.balance;
        vm.prank(sender);
        htlc.refund(id);
        assertEq(sender.balance, balBefore + 1 ether);
        assertTrue(htlc.getSwap(id).refunded);
    }

    // 7) refund por quem não é o sender => revert
    function test_RefundByNonSender_Reverts() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        vm.warp(timelock + 1);
        vm.prank(recipient);
        vm.expectRevert(HashedTimelock.NotSender.selector);
        htlc.refund(id);
    }

    // extra: rejeições de criação (timelock no passado, amount zero, hashlock zero, duplicado)
    function test_NewSwapGuards() public {
        vm.startPrank(sender);
        vm.expectRevert(HashedTimelock.TimelockInPast.selector);
        htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, block.timestamp);

        vm.expectRevert(HashedTimelock.ZeroAmount.selector);
        htlc.newSwap{value: 0}(recipient, address(0), 0, hashlock, timelock);

        vm.expectRevert(HashedTimelock.ZeroHashlock.selector);
        htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, bytes32(0), timelock);

        htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);
        vm.expectRevert(HashedTimelock.SwapAlreadyExists.selector);
        htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);
        vm.stopPrank();
    }

    // GRUPO 2 — resgate APÓS o timelock expirar deve reverter (janela fechou).
    // redeem exige block.timestamp < s.timelock (HashedTimelock.sol:110),
    // senão reverte TimelockExpired — mesmo com o preimage CORRETO.
    function test_RedeemAfterTimelock_Reverts() public {
        vm.prank(sender);
        bytes32 id = htlc.newSwap{value: 1 ether}(recipient, address(0), 1 ether, hashlock, timelock);

        vm.warp(timelock + 1); // janela de resgate fechou
        vm.expectRevert(HashedTimelock.TimelockExpired.selector);
        htlc.redeem(id, preimage); // preimage correto, mas tarde demais
    }
}

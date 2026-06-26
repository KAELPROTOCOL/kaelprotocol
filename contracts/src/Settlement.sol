// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {OrderLib} from "./Order.sol";

/// @notice Minimal HashedTimelock interface used for calls.
interface IHashedTimelock {
    function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
        external
        payable
        returns (bytes32 contractId);
    function refund(bytes32 contractId) external;
}

/// @notice Minimal ERC-20 interface.
interface IERC20 {
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function transfer(address to, uint256 amount) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
}

/// @title Settlement - Kael settlement contract (Approach A)
/// @notice Binds a signed Order (OrderLib) to a HashedTimelock fund lock,
///         atomically and non-custodially, validating only the local leg.
/// @dev    ARCHITECTURE (chosen Approach A): cross-leg validation (same
///         hashlock across legs, safe timelock gap) is not done on-chain. It is
///         (a) not workable cross-chain because each Settlement is per-chain and
///         sees only its local leg, and (b) unnecessary for safety: security
///         comes from HTLC fail-safety plus the wallet verifying the opposite
///         leg on-chain before locking its own. This contract only performs
///         local, real checks:
///           - binds the signed order to the lock (authorized token/amount);
///           - confines the order to its chain (sellChainId == block.chainid);
///           - provides per-chain anti-replay by (maker, nonce);
///           - remains non-custodial: funds can exit only to HTLC or refundLeg.
///         No owner, no privileged withdrawal, no selfdestruct, no arbitrary call.
contract Settlement {
    // ---- immutables ----

    /// The only HashedTimelock this Settlement uses, fixed at deployment.
    /// The HTLC is no longer a call parameter, closing the footgun where a
    /// caller could pass an arbitrary or malicious `htlc`. Before that, the
    /// contract pulled maker funds into itself and then approved/newSwap'd an
    /// untrusted caller-provided address. Now the lock can only go to this
    /// chain's canonical HTLC.
    address public immutable htlc;

    constructor(address htlc_) {
        if (htlc_ == address(0)) revert ZeroHtlc();
        htlc = htlc_;
    }

    // ---- state ----

    /// maker -> nonce -> consumed. Per-chain anti-replay by (maker, nonce).
    mapping(address => mapping(uint256 => bool)) public consumedNonce;

    /// Refund record keyed by HTLC contractId.
    struct Leg {
        address maker; // true fund owner; refund ALWAYS goes here
        address token; // address(0) = ETH
        uint256 amount;
        bool refunded; // Settlement-level refund guard
    }

    /// contractId -> leg.
    mapping(bytes32 => Leg) public legs;

    // ---- events ----

    event LegAuthorized(bytes32 indexed orderHash, address indexed maker, uint256 nonce);
    event SwapLegSettled(
        bytes32 indexed contractId,
        address indexed maker,
        address recipient,
        bytes32 hashlock,
        uint256 timelock
    );
    event LegRefunded(bytes32 indexed contractId, address indexed maker, uint256 amount);

    // ---- errors ----

    error ZeroHtlc();
    error NonceAlreadyUsed();
    error WrongChain();
    error NotOrderMaker();
    error EthValueMismatch();
    error EthNotAllowedForToken();
    error ContractIdMismatch();
    error UnknownLeg();
    error AlreadyRefunded();
    error InvalidToken();
    error TokenTransferFailed();
    error TokenApproveFailed();
    error EthTransferFailed();

    // ---- pure authorization without locking ----

    /// @notice Verifies the signed order and consumes the maker nonce without locking.
    function authorizeLeg(OrderLib.Order calldata order, bytes calldata signature, uint256 nowTs) external {
        bytes32 orderHash = OrderLib.verify(order, signature, nowTs);
        if (consumedNonce[order.maker][order.nonce]) revert NonceAlreadyUsed();
        consumedNonce[order.maker][order.nonce] = true;
        emit LegAuthorized(orderHash, order.maker, order.nonce);
    }

    // ---- atomic settlement of one leg ----

    /// @notice Verifies the order, confines it to this chain, consumes the nonce,
    ///         receives authorized funds, and locks them in the HTLC for `recipient`.
    /// @dev VALIDATES ONLY THE LOCAL LEG. Matching both legs (same hashlock, safe
    ///      timelock gap, counterparty identity) is the wallet's responsibility.
    ///      The wallet reads the opposite leg on-chain before locking its own
    ///      funds (Approach A). `recipient` is supplied by the locker because the
    ///      wallet knows the match counterparty. The order-to-lock binding is
    ///      local: the lock uses `order.sellToken` and `order.sellAmount`
    ///      authorized by the signature, never ad hoc values.
    /// @param order this leg's sell order, signed by the maker
    /// @param signature EIP-712 signature over `order`
    /// @param recipient party that can redeem with the preimage
    /// @param hashlock SHA-256 hashlock agreed off-chain
    /// @param timelock this leg's timelock
    /// @dev The HTLC is immutable and canonical (`htlc`), fixed at deployment.
    ///      The lock can never go to a caller-provided address.
    function settleLeg(
        OrderLib.Order calldata order,
        bytes calldata signature,
        address recipient,
        bytes32 hashlock,
        uint256 timelock
    ) external payable returns (bytes32 contractId) {
        // a) Verify this order's signature and validity.
        OrderLib.verify(order, signature, block.timestamp);

        // b) CHAIN BINDING: the order is for this chain.
        if (order.sellChainId != block.chainid) revert WrongChain();

        // b2) Only the maker settles their own leg. This closes the vector where
        //     a third party observes a signed order plus maker ERC-20 approval
        //     and settles on the maker's behalf with attacker-controlled
        //     recipient/hashlock. This check is before receiving any funds.
        if (msg.sender != order.maker) revert NotOrderMaker();

        // c) Anti-replay: consume this leg's nonce.
        if (consumedNonce[order.maker][order.nonce]) revert NonceAlreadyUsed();
        consumedNonce[order.maker][order.nonce] = true;

        // d) Local order-to-lock binding: token/amount come from the signed order.
        contractId =
            keccak256(abi.encode(address(this), recipient, order.sellToken, order.sellAmount, hashlock, timelock));

        // e) Refund record before external interactions (CEI).
        legs[contractId] =
            Leg({maker: order.maker, token: order.sellToken, amount: order.sellAmount, refunded: false});

        // f-g) Receive authorized funds and lock them in the canonical HTLC.
        _receiveAndLock(order, recipient, hashlock, timelock, contractId);

        // h) Event.
        emit SwapLegSettled(contractId, order.maker, recipient, hashlock, timelock);
    }

    /// @dev Receives this leg's funds and locks them in the HTLC under this
    ///      contract. Requires the resulting contractId to match the expected one
    ///      defensively. This is one of only two fund exit paths; the other is
    ///      refundLeg.
    function _receiveAndLock(
        OrderLib.Order calldata order,
        address recipient,
        bytes32 hashlock,
        uint256 timelock,
        bytes32 contractId
    ) private {
        bytes32 got;
        if (order.sellToken == address(0)) {
            // ETH: the received value becomes the lock.
            if (msg.value != order.sellAmount) revert EthValueMismatch();
            got = IHashedTimelock(htlc).newSwap{value: order.sellAmount}(
                recipient, order.sellToken, order.sellAmount, hashlock, timelock
            );
        } else {
            // ERC-20: pull from the maker and approve the HTLC for exactly amount.
            if (msg.value != 0) revert EthNotAllowedForToken();
            if (order.sellToken.code.length == 0) revert InvalidToken();
            _safeTransferFrom(order.sellToken, order.maker, address(this), order.sellAmount);
            _safeApprove(order.sellToken, htlc, order.sellAmount);
            got = IHashedTimelock(htlc).newSwap(recipient, order.sellToken, order.sellAmount, hashlock, timelock);
        }
        if (got != contractId) revert ContractIdMismatch();
    }

    /// @notice Refunds a leg after the HTLC timelock expires.
    /// @dev Anyone may call, but funds ALWAYS go to `leg.maker`, the true owner.
    ///      The destination cannot be redirected, and the maker is not stuck if
    ///      they cannot or do not want to send the transaction themselves.
    ///      Reverts if the leg was already redeemed or already refunded.
    function refundLeg(bytes32 contractId) external {
        Leg storage leg = legs[contractId];
        if (leg.maker == address(0)) revert UnknownLeg();
        if (leg.refunded) revert AlreadyRefunded();

        // CEI: mark before external interactions.
        leg.refunded = true;
        address maker = leg.maker;
        address token = leg.token;
        uint256 amount = leg.amount;

        // Funds return to Settlement, the sender of the canonical immutable HTLC
        // lock. If the leg was already redeemed or has not expired, the HTLC
        // reverts here.
        IHashedTimelock(htlc).refund(contractId);

        // Forward to the true maker.
        if (token == address(0)) {
            (bool ok,) = payable(maker).call{value: amount}("");
            if (!ok) revert EthTransferFailed();
        } else {
            _safeTransfer(token, maker, amount);
        }

        emit LegRefunded(contractId, maker, amount);
    }

    /// @dev Accepts ETH returned by the HTLC during `refund`. There is no other
    ///      way to extract ETH from this contract besides `settleLeg` (to HTLC)
    ///      and `refundLeg` (to maker). ETH sent here directly would remain
    ///      stuck; it is not swap funding and there is no privileged withdrawal.
    receive() external payable {}

    // ---- safe ERC-20 helpers tolerant of no-return tokens ----

    function _safeTransferFrom(address token, address from, address to, uint256 amount) private {
        (bool ok, bytes memory data) = token.call(abi.encodeWithSelector(IERC20.transferFrom.selector, from, to, amount));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TokenTransferFailed();
    }

    function _safeTransfer(address token, address to, uint256 amount) private {
        (bool ok, bytes memory data) = token.call(abi.encodeWithSelector(IERC20.transfer.selector, to, amount));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TokenTransferFailed();
    }

    function _safeApprove(address token, address spender, uint256 amount) private {
        (bool ok, bytes memory data) = token.call(abi.encodeWithSelector(IERC20.approve.selector, spender, amount));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TokenApproveFailed();
    }
}

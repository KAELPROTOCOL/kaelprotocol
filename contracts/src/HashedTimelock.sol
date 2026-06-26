// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title HashedTimelock - Kael protocol layer 1
/// @notice Locks an asset (native ETH or ERC-20) behind a SHA-256 hashlock
///         and a timelock. No owner, no custody, no arbitrary withdrawal.
/// @dev    FOUNDATION (ADR-001): the hashlock uses SHA-256 (not keccak256)
///         so it can also be verified in Bitcoin Script (OP_SHA256). The
///         contractId uses keccak256 only as a cheap internal EVM identifier.
///         Do not confuse these two hash uses.
contract HashedTimelock {
    struct Swap {
        address sender; // locker; can refund after the deadline
        address recipient; // can redeem with the preimage
        address token; // address(0) = native ETH; otherwise ERC-20
        uint256 amount;
        bytes32 hashlock; // SHA-256 of the preimage
        uint256 timelock; // timestamp after which refund is allowed
        bool withdrawn;
        bool refunded;
    }

    mapping(bytes32 => Swap) public swaps;

    event LogNewSwap(
        bytes32 indexed contractId,
        address indexed sender,
        address indexed recipient,
        address token,
        uint256 amount,
        bytes32 hashlock,
        uint256 timelock
    );
    event LogRedeem(bytes32 indexed contractId, bytes32 preimage);
    event LogRefund(bytes32 indexed contractId);

    error ZeroAmount();
    error ZeroHashlock();
    error TimelockInPast();
    error SwapAlreadyExists();
    error SwapNotFound();
    error AlreadyWithdrawn();
    error AlreadyRefunded();
    error InvalidPreimage();
    error TimelockNotExpired();
    error TimelockExpired();
    error NotSender();
    error EthValueMismatch();
    error EthNotAllowedForToken();
    error InvalidToken();
    error TokenTransferFailed();

    /// @notice Derives the deterministic swap identifier.
    function computeContractId(
        address sender,
        address recipient,
        address token,
        uint256 amount,
        bytes32 hashlock,
        uint256 timelock
    ) public pure returns (bytes32) {
        return keccak256(abi.encode(sender, recipient, token, amount, hashlock, timelock));
    }

    /// @notice Locks `amount` behind (hashlock, timelock) for `recipient`.
    /// @dev For native ETH, pass token=address(0) and msg.value == amount.
    ///      For ERC-20, pass msg.value == 0 and approve this contract first.
    function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
        external
        payable
        returns (bytes32 contractId)
    {
        if (amount == 0) revert ZeroAmount();
        if (hashlock == bytes32(0)) revert ZeroHashlock();
        if (timelock <= block.timestamp) revert TimelockInPast();

        contractId = computeContractId(msg.sender, recipient, token, amount, hashlock, timelock);

        // Duplicate contractId means the swap already exists.
        if (swaps[contractId].sender != address(0)) revert SwapAlreadyExists();

        if (token == address(0)) {
            if (msg.value != amount) revert EthValueMismatch();
        } else {
            if (msg.value != 0) revert EthNotAllowedForToken();
            if (token.code.length == 0) revert InvalidToken();
            _safeTransferFrom(token, msg.sender, address(this), amount);
        }

        swaps[contractId] = Swap({
            sender: msg.sender,
            recipient: recipient,
            token: token,
            amount: amount,
            hashlock: hashlock,
            timelock: timelock,
            withdrawn: false,
            refunded: false
        });

        emit LogNewSwap(contractId, msg.sender, recipient, token, amount, hashlock, timelock);
    }

    /// @notice Redeems the asset by revealing the preimage before the timelock.
    /// @dev The preimage is PUBLISHED in the event, enabling the opposite
    ///      cross-chain swap leg to be redeemed.
    function redeem(bytes32 contractId, bytes32 preimage) external {
        Swap storage s = swaps[contractId];
        if (s.sender == address(0)) revert SwapNotFound();
        if (s.withdrawn) revert AlreadyWithdrawn();
        if (s.refunded) revert AlreadyRefunded();
        if (block.timestamp >= s.timelock) revert TimelockExpired();
        // FOUNDATION: SHA-256, not keccak256.
        if (sha256(abi.encodePacked(preimage)) != s.hashlock) revert InvalidPreimage();

        s.withdrawn = true;
        _payout(s.token, s.recipient, s.amount);

        emit LogRedeem(contractId, preimage);
    }

    /// @notice Refunds the sender after the timelock expires.
    function refund(bytes32 contractId) external {
        Swap storage s = swaps[contractId];
        if (s.sender == address(0)) revert SwapNotFound();
        if (s.withdrawn) revert AlreadyWithdrawn();
        if (s.refunded) revert AlreadyRefunded();
        if (block.timestamp < s.timelock) revert TimelockNotExpired();
        if (msg.sender != s.sender) revert NotSender();

        s.refunded = true;
        _payout(s.token, s.sender, s.amount);

        emit LogRefund(contractId);
    }

    function getSwap(bytes32 contractId) external view returns (Swap memory) {
        return swaps[contractId];
    }

    // --- internals ---

    function _payout(address token, address to, uint256 amount) private {
        if (token == address(0)) {
            (bool ok,) = payable(to).call{value: amount}("");
            if (!ok) revert TokenTransferFailed();
        } else {
            _safeTransfer(token, to, amount);
        }
    }

    function _safeTransfer(address token, address to, uint256 amount) private {
        (bool ok, bytes memory data) = token.call(abi.encodeWithSelector(0xa9059cbb, to, amount));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TokenTransferFailed();
    }

    function _safeTransferFrom(address token, address from, address to, uint256 amount) private {
        (bool ok, bytes memory data) = token.call(abi.encodeWithSelector(0x23b872dd, from, to, amount));
        if (!ok || (data.length != 0 && !abi.decode(data, (bool)))) revert TokenTransferFailed();
    }
}

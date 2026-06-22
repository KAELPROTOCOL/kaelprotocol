// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title HashedTimelock — Camada 1 do protocolo Kael
/// @notice Trava um ativo (ETH nativo ou ERC-20) atrás de um hashlock (SHA-256)
///         e um timelock. Sem dono, sem custódia, sem saque arbitrário.
/// @dev    FUNDAÇÃO (ADR-001): o hashlock usa SHA-256 (não keccak256) para ser
///         verificável também em Bitcoin Script (OP_SHA256). O contractId usa
///         keccak256 por ser apenas um identificador interno barato do EVM.
///         Não confundir os dois usos de hash.
contract HashedTimelock {
    struct Swap {
        address sender; // quem travou (pode reembolsar após o prazo)
        address recipient; // quem pode resgatar com o preimage
        address token; // address(0) = ETH nativo; senão, ERC-20
        uint256 amount;
        bytes32 hashlock; // SHA-256 do preimage
        uint256 timelock; // timestamp; após ele, refund liberado
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
    error TokenTransferFailed();

    /// @notice Deriva o identificador determinístico de um swap.
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

    /// @notice Trava `amount` atrás de (hashlock, timelock) a favor de `recipient`.
    /// @dev Para ETH nativo, passe token=address(0) e msg.value == amount.
    ///      Para ERC-20, passe msg.value == 0 e tenha aprovado este contrato.
    function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
        external
        payable
        returns (bytes32 contractId)
    {
        if (amount == 0) revert ZeroAmount();
        if (hashlock == bytes32(0)) revert ZeroHashlock();
        if (timelock <= block.timestamp) revert TimelockInPast();

        contractId = computeContractId(msg.sender, recipient, token, amount, hashlock, timelock);

        // contractId duplicado => swap já existe (sender != 0 só após criação)
        if (swaps[contractId].sender != address(0)) revert SwapAlreadyExists();

        if (token == address(0)) {
            if (msg.value != amount) revert EthValueMismatch();
        } else {
            if (msg.value != 0) revert EthNotAllowedForToken();
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

    /// @notice Resgata o ativo revelando o preimage, antes do timelock.
    /// @dev O preimage é PUBLICADO no evento — é o que permite a outra perna
    ///      do swap cross-chain ser resgatada.
    function redeem(bytes32 contractId, bytes32 preimage) external {
        Swap storage s = swaps[contractId];
        if (s.sender == address(0)) revert SwapNotFound();
        if (s.withdrawn) revert AlreadyWithdrawn();
        if (s.refunded) revert AlreadyRefunded();
        if (block.timestamp >= s.timelock) revert TimelockExpired();
        // FUNDAÇÃO: SHA-256, não keccak256.
        if (sha256(abi.encodePacked(preimage)) != s.hashlock) revert InvalidPreimage();

        s.withdrawn = true;
        _payout(s.token, s.recipient, s.amount);

        emit LogRedeem(contractId, preimage);
    }

    /// @notice Reembolsa o sender após o timelock expirar.
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

    // --- internos ---

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

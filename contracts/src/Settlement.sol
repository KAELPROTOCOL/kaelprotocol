// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {OrderLib} from "./Order.sol";

/// @notice Interface mínima do HashedTimelock — declarada para CHAMÁ-LO.
///         O HashedTimelock.sol NÃO é alterado em nenhuma linha.
interface IHashedTimelock {
    function newSwap(address recipient, address token, uint256 amount, bytes32 hashlock, uint256 timelock)
        external
        payable
        returns (bytes32 contractId);
    function refund(bytes32 contractId) external;
}

/// @notice Interface ERC-20 mínima.
interface IERC20 {
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function transfer(address to, uint256 amount) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
}

/// @title Settlement — liquidador do Kael (Abordagem A)
/// @notice Liga a Order assinada (OrderLib) à trava de fundos no HashedTimelock,
///         de forma atômica e NÃO-CUSTODIAL, validando APENAS a própria perna.
/// @dev    ARQUITETURA (Abordagem A — decidida): a validação cross-leg (mesmo
///         hashlock entre as pernas, gap de timelock seguro) NÃO é feita on-chain.
///         Ela é (a) inoperante cross-chain — cada Settlement é per-chain e só vê
///         a própria perna — e (b) desnecessária: a segurança vem da falha-segura
///         do HTLC + a CARTEIRA verificar a perna oposta on-chain ANTES de travar
///         a sua. Este contrato faz só o que é LOCAL e real:
///           - liga a ordem assinada à trava (token/amount autorizados) — Furo 1;
///           - confina a ordem à sua chain (sellChainId == block.chainid);
///           - anti-replay per-chain por (maker, nonce) — Furo 2;
///           - não-custódia: duas saídas de fundos só, HTLC ou refundLeg→maker.
///         Sem dono, sem saque, sem selfdestruct, sem chamada arbitrária.
contract Settlement {
    // ---- estado ----

    /// maker → nonce → consumido. Anti-replay per-chain por (maker, nonce).
    mapping(address => mapping(uint256 => bool)) public consumedNonce;

    /// Registro por trava (contractId no HTLC) para o reembolso.
    struct Leg {
        address maker; // dono real dos fundos — para quem o reembolso SEMPRE vai
        address htlc; // qual HTLC guarda a trava
        address token; // address(0) = ETH
        uint256 amount;
        bool refunded; // guarda de reembolso no nível do Settlement
    }

    /// contractId → perna.
    mapping(bytes32 => Leg) public legs;

    // ---- eventos ----

    event LegAuthorized(bytes32 indexed orderHash, address indexed maker, uint256 nonce);
    event SwapLegSettled(
        bytes32 indexed contractId,
        address indexed maker,
        address recipient,
        bytes32 hashlock,
        uint256 timelock
    );
    event LegRefunded(bytes32 indexed contractId, address indexed maker, uint256 amount);

    // ---- erros ----

    error NonceAlreadyUsed();
    error WrongChain();
    error NotOrderMaker();
    error EthValueMismatch();
    error EthNotAllowedForToken();
    error ContractIdMismatch();
    error UnknownLeg();
    error AlreadyRefunded();
    error TokenTransferFailed();
    error TokenApproveFailed();
    error EthTransferFailed();

    // ---- autorização pura (sem travar) ----

    /// @notice Verifica a ordem assinada e consome o nonce do maker (sem travar).
    function authorizeLeg(OrderLib.Order calldata order, bytes calldata signature, uint256 nowTs) external {
        bytes32 orderHash = OrderLib.verify(order, signature, nowTs);
        if (consumedNonce[order.maker][order.nonce]) revert NonceAlreadyUsed();
        consumedNonce[order.maker][order.nonce] = true;
        emit LegAuthorized(orderHash, order.maker, order.nonce);
    }

    // ---- liquidação atômica de UMA perna ----

    /// @notice Verifica a ordem, confina-a à chain, consome o nonce, recebe os
    ///         fundos autorizados e os trava no HTLC a favor de `recipient`.
    /// @dev VALIDA APENAS A PRÓPRIA PERNA. A correspondência entre as duas pernas
    ///      (mesmo hashlock, gap de timelock seguro, identidade da contraparte) é
    ///      responsabilidade da CARTEIRA, que lê a perna oposta on-chain antes de
    ///      travar a sua (Abordagem A). O `recipient` (quem poderá resgatar) é
    ///      fornecido por quem trava — a carteira sabe quem é a contraparte do match.
    ///      O binding ordem↔trava é LOCAL: a trava usa `order.sellToken` e
    ///      `order.sellAmount` (valores AUTORIZADOS pela assinatura), nunca valores
    ///      avulsos.
    /// @param order a ordem desta perna (perna de venda), assinada pelo maker
    /// @param signature assinatura EIP-712 do `order`
    /// @param recipient quem poderá resgatar com o preimage (a contraparte do match)
    /// @param hashlock hashlock SHA-256 acordado off-chain (a carteira garante o mesmo nas duas pernas)
    /// @param timelock timelock desta perna (a carteira garante a assimetria segura)
    /// @param htlc endereço do HashedTimelock desta chain
    function settleLeg(
        OrderLib.Order calldata order,
        bytes calldata signature,
        address recipient,
        bytes32 hashlock,
        uint256 timelock,
        address htlc
    ) external payable returns (bytes32 contractId) {
        // a) verifica assinatura + validade desta ordem (reverte se inválida/expirada)
        OrderLib.verify(order, signature, block.timestamp);

        // b) BINDING DE CHAIN: a ordem é para ESTA chain. Confina cada ordem à sua
        //    chain e torna o anti-replay per-chain suficiente.
        if (order.sellChainId != block.chainid) revert WrongChain();

        // b2) SÓ O MAKER LIQUIDA A PRÓPRIA PERNA (Abordagem A: cada parte trava os
        //     PRÓPRIOS fundos). Fecha o vetor onde um terceiro, vendo a ordem
        //     assinada + a aprovação ERC-20 do maker, liquidaria em nome dele com
        //     recipient/hashlock do atacante e drenaria os tokens do maker.
        //     Posicionado ANTES de receber qualquer fundo.
        if (msg.sender != order.maker) revert NotOrderMaker();

        // c) anti-replay: consome o nonce desta perna
        if (consumedNonce[order.maker][order.nonce]) revert NonceAlreadyUsed();
        consumedNonce[order.maker][order.nonce] = true;

        // d) BINDING ORDEM↔TRAVA (local): token/amount vêm da ordem ASSINADA.
        //    O contractId previsto usa o sender (este contrato) e os valores autorizados.
        contractId =
            keccak256(abi.encode(address(this), recipient, order.sellToken, order.sellAmount, hashlock, timelock));

        // e) REGISTRO para reembolso (antes de qualquer interação externa — CEI).
        legs[contractId] =
            Leg({maker: order.maker, htlc: htlc, token: order.sellToken, amount: order.sellAmount, refunded: false});

        // f-g) recebe os fundos autorizados e trava no HTLC, em nome próprio
        _receiveAndLock(order, recipient, hashlock, timelock, htlc, contractId);

        // h) evento
        emit SwapLegSettled(contractId, order.maker, recipient, hashlock, timelock);
    }

    /// @dev Recebe os fundos desta perna e os trava no HTLC, em nome próprio.
    ///      Exige que o contractId resultante seja o previsto (defensivo). Esta é
    ///      UMA das duas únicas saídas de fundos do contrato (a outra é refundLeg).
    function _receiveAndLock(
        OrderLib.Order calldata order,
        address recipient,
        bytes32 hashlock,
        uint256 timelock,
        address htlc,
        bytes32 contractId
    ) private {
        bytes32 got;
        if (order.sellToken == address(0)) {
            // ETH: o value recebido vira a trava.
            if (msg.value != order.sellAmount) revert EthValueMismatch();
            got = IHashedTimelock(htlc).newSwap{value: order.sellAmount}(
                recipient, order.sellToken, order.sellAmount, hashlock, timelock
            );
        } else {
            // ERC-20: puxa do maker e aprova o HTLC a sacar exatamente o amount.
            if (msg.value != 0) revert EthNotAllowedForToken();
            _safeTransferFrom(order.sellToken, order.maker, address(this), order.sellAmount);
            _safeApprove(order.sellToken, htlc, order.sellAmount);
            got = IHashedTimelock(htlc).newSwap(recipient, order.sellToken, order.sellAmount, hashlock, timelock);
        }
        if (got != contractId) revert ContractIdMismatch();
    }

    /// @notice Reembolsa uma perna após a expiração do timelock no HTLC.
    /// @dev Qualquer um pode chamar, mas os fundos vão SEMPRE para `leg.maker`
    ///      (o dono real). Não há como desviar o destino, e o maker nunca fica
    ///      preso se não puder/quiser enviar a tx ele mesmo. Reverte se a perna já
    ///      foi resgatada (o próprio HTLC reverte) ou já reembolsada.
    function refundLeg(bytes32 contractId) external {
        Leg storage leg = legs[contractId];
        if (leg.maker == address(0)) revert UnknownLeg();
        if (leg.refunded) revert AlreadyRefunded();

        // CEI: marca antes de qualquer interação externa.
        leg.refunded = true;
        address maker = leg.maker;
        address token = leg.token;
        uint256 amount = leg.amount;
        address htlc = leg.htlc;

        // os fundos voltam para o Settlement (que é o sender da trava no HTLC).
        // se a perna já foi resgatada ou ainda não expirou, o HTLC reverte aqui.
        IHashedTimelock(htlc).refund(contractId);

        // repassa ao maker real.
        if (token == address(0)) {
            (bool ok,) = payable(maker).call{value: amount}("");
            if (!ok) revert EthTransferFailed();
        } else {
            _safeTransfer(token, maker, amount);
        }

        emit LegRefunded(contractId, maker, amount);
    }

    /// @dev Aceita o ETH devolvido pelo HTLC durante `refund`. Não há nenhuma
    ///      outra forma de extrair ETH do contrato além de `settleLeg` (→HTLC) e
    ///      `refundLeg` (→maker). ETH enviado diretamente aqui ficaria preso —
    ///      não é fundo de swap e não há saque privilegiado.
    receive() external payable {}

    // ---- helpers ERC-20 seguros (tolerantes a tokens sem retorno) ----

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

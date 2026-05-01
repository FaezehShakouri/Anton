// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {L2Registrar} from "durin/src/examples/L2Registrar.sol";
import {IL2Registry} from "durin/src/interfaces/IL2Registry.sol";

import {Hex} from "./libraries/Hex.sol";

/// @title ChatRegistrar
/// @notice Durin L2 registrar controller for `*.chat.eth` that registers a subname and,
///         in one transaction, sets `addr` for the deployment chain + coin type 60,
///         plus text records `axl_peer_id` (hex 32-byte ed25519 pubkey) and `axl_pubkey`
///         (PEM). Mirrors Durin's example `L2Registrar.register` ordering (pre-seed resolver
///         fields on the future node, then `createSubnode`).
contract ChatRegistrar is L2Registrar {
    error PeerIdInvalidLength();
    error PubKeyEmpty();

    event ChatNameRegistered(string label, address indexed owner, bytes peerId, string pubkeyPem);

    constructor(address registry_) L2Registrar(registry_) {}

    /// @param label Plain label under `chat.eth` (e.g. `"alice"` for `alice.chat.eth`).
    /// @param owner_ Address that will own the ERC721 subname and receive `addr` records.
    /// @param peerId Raw 32-byte ed25519 public key bytes (encoded as `0x…` hex in `axl_peer_id`).
    /// @param pubkeyPem PEM-encoded public key string stored under `axl_pubkey`.
    function registerWithRecords(
        string calldata label,
        address owner_,
        bytes calldata peerId,
        string calldata pubkeyPem
    ) external {
        if (peerId.length != 32) revert PeerIdInvalidLength();
        if (bytes(pubkeyPem).length == 0) revert PubKeyEmpty();

        IL2Registry reg = registry;
        bytes32 node = reg.makeNode(reg.baseNode(), label);
        bytes memory packedOwner = abi.encodePacked(owner_);

        reg.setAddr(node, coinType, packedOwner);
        reg.setAddr(node, 60, packedOwner);
        reg.setText(node, "axl_peer_id", Hex.bytesToHexPrefixed(peerId));
        reg.setText(node, "axl_pubkey", pubkeyPem);

        reg.createSubnode(reg.baseNode(), label, owner_, new bytes[](0));

        emit ChatNameRegistered(label, owner_, peerId, pubkeyPem);
    }
}

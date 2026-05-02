// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Hex} from "./libraries/Hex.sol";

interface IEnsRegistry {
    function owner(bytes32 node) external view returns (address);
    function setSubnodeRecord(
        bytes32 node,
        bytes32 label,
        address owner_,
        address resolver_,
        uint64 ttl
    ) external;
    function setOwner(bytes32 node, address owner_) external;
}

interface IPublicResolver {
    function setAddr(bytes32 node, address addr_) external;
    function setText(bytes32 node, string calldata key, string calldata value) external;
}

/// @title ChatRegistrar
/// @notice Optional L1 ENS registrar helper for `*.anton.eth` on Sepolia/mainnet.
///         The contract must be the owner/operator for the parent node in the ENS registry.
///         It creates a subname, writes `addr(60)`, `axl_peer_id`, and `axl_pubkey`,
///         then transfers the subname to the requested owner.
contract ChatRegistrar {
    error NotOwner();
    error PeerIdInvalidLength();
    error PubKeyEmpty();

    event ChatNameRegistered(string label, address indexed owner, bytes peerId, string pubkeyPem);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    address public owner;
    IEnsRegistry public immutable registry;
    IPublicResolver public immutable resolver;
    bytes32 public immutable parentNode;

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    constructor(address registry_, address resolver_, bytes32 parentNode_) {
        owner = msg.sender;
        registry = IEnsRegistry(registry_);
        resolver = IPublicResolver(resolver_);
        parentNode = parentNode_;
        emit OwnershipTransferred(address(0), msg.sender);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
        emit OwnershipTransferred(msg.sender, newOwner);
    }

    function makeNode(string calldata label) public view returns (bytes32) {
        return keccak256(abi.encodePacked(parentNode, keccak256(bytes(label))));
    }

    function available(string calldata label) external view returns (bool) {
        return registry.owner(makeNode(label)) == address(0) && bytes(label).length >= 3;
    }

    /// @param label Plain label under `anton.eth` (e.g. `"alice"` for `alice.anton.eth`).
    /// @param owner_ Address that will own the ERC721 subname and receive `addr` records.
    /// @param peerId Raw 32-byte ed25519 public key bytes (encoded as `0x…` hex in `axl_peer_id`).
    /// @param pubkeyPem PEM-encoded public key string stored under `axl_pubkey`.
    function registerWithRecords(
        string calldata label,
        address owner_,
        bytes calldata peerId,
        string calldata pubkeyPem
    ) external onlyOwner {
        if (peerId.length != 32) revert PeerIdInvalidLength();
        if (bytes(pubkeyPem).length == 0) revert PubKeyEmpty();

        bytes32 labelhash = keccak256(bytes(label));
        bytes32 node = keccak256(abi.encodePacked(parentNode, labelhash));

        registry.setSubnodeRecord(parentNode, labelhash, address(this), address(resolver), 0);
        resolver.setAddr(node, owner_);
        resolver.setText(node, "axl_peer_id", Hex.bytesToHexPrefixed(peerId));
        resolver.setText(node, "axl_pubkey", pubkeyPem);
        registry.setOwner(node, owner_);

        emit ChatNameRegistered(label, owner_, peerId, pubkeyPem);
    }
}

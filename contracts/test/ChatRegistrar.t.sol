// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";

import {ChatRegistrar} from "../src/ChatRegistrar.sol";
import {Hex} from "../src/libraries/Hex.sol";

contract HexTest is Test {
    function test_bytesToHexPrefixed_empty() public pure {
        assertEq(Hex.bytesToHexPrefixed(hex""), "0x");
    }

    function test_bytesToHexPrefixed_vector() public pure {
        assertEq(Hex.bytesToHexPrefixed(hex"deadbeef"), "0xdeadbeef");
    }

    function test_bytesToHexPrefixed_peerSized() public pure {
        bytes memory pk = new bytes(32);
        pk[0] = 0x01;
        pk[31] = 0xff;
        assertEq(
            Hex.bytesToHexPrefixed(pk),
            "0x01000000000000000000000000000000000000000000000000000000000000ff"
        );
    }
}

contract RegistryStub {
    bytes32 public immutable baseNode = bytes32(uint256(1));

    mapping(bytes32 node => address owner_) internal owners;
    mapping(bytes32 node => address resolver_) internal resolvers;

    constructor() {
        owners[baseNode] = address(this);
    }

    function makeNode(bytes32 parent, string calldata label) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(parent, keccak256(bytes(label))));
    }

    function owner(bytes32 node) external view returns (address) {
        return owners[node];
    }

    function resolver(bytes32 node) external view returns (address) {
        return resolvers[node];
    }

    function setSubnodeRecord(
        bytes32 parent,
        bytes32 label,
        address owner_,
        address resolver_,
        uint64
    ) external {
        bytes32 node = keccak256(abi.encodePacked(parent, label));
        owners[node] = owner_;
        resolvers[node] = resolver_;
    }

    function setOwner(bytes32 node, address owner_) external {
        owners[node] = owner_;
    }
}

contract ResolverStub {
    mapping(bytes32 node => address value) internal addrs;
    mapping(bytes32 node => mapping(string key => string value)) internal texts;

    function setAddr(bytes32 node, address addr_) external {
        addrs[node] = addr_;
    }

    function setText(bytes32 node, string calldata key, string calldata value) external {
        texts[node][key] = value;
    }

    function addrOf(bytes32 node) external view returns (address) {
        return addrs[node];
    }

    function textOf(bytes32 node, string calldata key) external view returns (string memory) {
        return texts[node][key];
    }
}

contract ChatRegistrarTest is Test {
    RegistryStub internal stub;
    ResolverStub internal resolver;
    ChatRegistrar internal registrar;

    function setUp() public {
        stub = new RegistryStub();
        resolver = new ResolverStub();
        registrar = new ChatRegistrar(address(stub), address(resolver), stub.baseNode());
    }

    function test_registerWithRecords_peerNot32_reverts() public {
        vm.expectRevert(ChatRegistrar.PeerIdInvalidLength.selector);
        registrar.registerWithRecords("alice", address(this), hex"00", "pem");
    }

    function test_registerWithRecords_emptyPem_reverts() public {
        bytes memory pk = new bytes(32);
        vm.expectRevert(ChatRegistrar.PubKeyEmpty.selector);
        registrar.registerWithRecords("alice", address(this), pk, "");
    }

    function test_registerWithRecords_writesResolverFields() public {
        bytes memory pk = new bytes(32);
        pk[0] = 0xab;
        pk[31] = 0x01;
        address owner_ = address(uint160(uint256(keccak256("owner"))));
        string memory pem = "-----BEGIN PUBLIC KEY-----\nabc\n-----END PUBLIC KEY-----";

        registrar.registerWithRecords("alice", owner_, pk, pem);

        bytes32 node = stub.makeNode(stub.baseNode(), "alice");

        assertEq(stub.owner(node), owner_);
        assertEq(stub.resolver(node), address(resolver));
        assertEq(resolver.addrOf(node), owner_);
        assertEq(resolver.textOf(node, "axl_peer_id"), Hex.bytesToHexPrefixed(pk));
        assertEq(resolver.textOf(node, "axl_pubkey"), pem);
    }

    function test_available_checks_registry_owner() public view {
        assertTrue(registrar.available("zzz"));
    }
}

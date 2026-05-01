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

/// @dev Minimal registry surface so `ChatRegistrar` can be exercised without Durin fixtures.
contract RegistryStub {
    bytes32 public baseNode = bytes32(uint256(1));

    mapping(bytes32 node => mapping(uint256 coinType => bytes value)) internal addrs;
    mapping(bytes32 node => mapping(string key => string value)) internal texts;

    function makeNode(bytes32 parent, string calldata label) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(parent, keccak256(bytes(label))));
    }

    function setAddr(bytes32 node, uint256 coinType_, bytes calldata addr_) external {
        addrs[node][coinType_] = addr_;
    }

    function setAddr(bytes32 node, address addr_) external {
        addrs[node][60] = abi.encodePacked(addr_);
    }

    function setText(bytes32 node, string calldata key, string calldata value) external {
        texts[node][key] = value;
    }

    function createSubnode(bytes32, string calldata, address, bytes[] calldata)
        external
        pure
        returns (bytes32)
    {
        return bytes32(uint256(0xdead));
    }

    function addrOf(bytes32 node, uint256 coinType_) external view returns (bytes memory) {
        return addrs[node][coinType_];
    }

    function textOf(bytes32 node, string calldata key) external view returns (string memory) {
        return texts[node][key];
    }
}

contract ChatRegistrarTest is Test {
    RegistryStub internal stub;
    ChatRegistrar internal registrar;

    function setUp() public {
        stub = new RegistryStub();
        registrar = new ChatRegistrar(address(stub));
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

        assertEq(stub.addrOf(node, registrar.coinType()), abi.encodePacked(owner_));
        assertEq(stub.addrOf(node, 60), abi.encodePacked(owner_));
        assertEq(stub.textOf(node, "axl_peer_id"), Hex.bytesToHexPrefixed(pk));
        assertEq(stub.textOf(node, "axl_pubkey"), pem);
    }

    function test_available_delegatesToDurinLogic() public view {
        assertTrue(registrar.available("zzz"));
    }
}

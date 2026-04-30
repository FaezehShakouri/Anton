// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {ChatRegistrar} from "../src/ChatRegistrar.sol";

contract ChatRegistrarScaffoldTest is Test {
    ChatRegistrar internal registrar;

    function setUp() public {
        registrar = new ChatRegistrar();
    }

    /// The scaffold stub reverts unconditionally so accidental deployments
    /// surface immediately. The real test suite (mocking the Durin L2
    /// Registry, asserting events, fuzzing labels) lands in a later plan step.
    function test_registerWithRecords_revertsBeforeImplementation() public {
        vm.expectRevert(ChatRegistrar.NotImplemented.selector);
        registrar.registerWithRecords("alice", address(0xA11CE), hex"00", "");
    }
}

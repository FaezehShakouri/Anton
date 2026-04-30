// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ChatRegistrar} from "../src/ChatRegistrar.sol";

/// @title Deploy (scaffold placeholder)
/// @notice Deploys `ChatRegistrar` and (later) wires it into the Durin L2
///         Registry by calling `addRegistrar(ChatRegistrar)` as the registry
///         owner. The full deployment flow lands in a later plan step.
contract Deploy is Script {
    function run() external returns (ChatRegistrar registrar) {
        uint256 pk = vm.envUint("PRIVATE_KEY");
        vm.startBroadcast(pk);
        registrar = new ChatRegistrar();
        vm.stopBroadcast();

        console.log("ChatRegistrar deployed at:", address(registrar));
    }
}

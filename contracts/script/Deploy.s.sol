// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ChatRegistrar} from "../src/ChatRegistrar.sol";

/// @notice Deploys the optional L1 ENS `ChatRegistrar` helper.
/// @dev The deployed registrar must be approved as an operator for the parent ENS name.
contract Deploy is Script {
    function run() external returns (ChatRegistrar registrar) {
        uint256 pk = vm.envUint("PRIVATE_KEY");
        address ensRegistry = vm.envAddress("ENS_REGISTRY");
        address publicResolver = vm.envAddress("ENS_PUBLIC_RESOLVER");
        bytes32 parentNode = vm.envBytes32("ENS_PARENT_NODE");

        vm.startBroadcast(pk);
        registrar = new ChatRegistrar(ensRegistry, publicResolver, parentNode);
        vm.stopBroadcast();

        console.log("ChatRegistrar:", address(registrar));
        console.log("ENS_REGISTRY:", ensRegistry);
        console.log("ENS_PUBLIC_RESOLVER:", publicResolver);
        console.logBytes32(parentNode);
    }
}

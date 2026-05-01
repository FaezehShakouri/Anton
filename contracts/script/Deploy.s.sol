// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ChatRegistrar} from "../src/ChatRegistrar.sol";

/// @notice Deploys `ChatRegistrar` pointing at an initialized Durin `L2Registry`.
/// @dev After deployment, the registry owner must call `addRegistrar(address(registrar))`
///      once (see `script/AddRegistrar.s.sol`).
contract Deploy is Script {
    function run() external returns (ChatRegistrar registrar) {
        uint256 pk = vm.envUint("PRIVATE_KEY");
        address l2Registry = vm.envAddress("L2_REGISTRY");

        vm.startBroadcast(pk);
        registrar = new ChatRegistrar(l2Registry);
        vm.stopBroadcast();

        console.log("ChatRegistrar:", address(registrar));
        console.log("L2_REGISTRY:", l2Registry);
        console.log(
            "Next (registry owner): forge script script/AddRegistrar.s.sol --rpc-url ... --broadcast"
        );
    }
}

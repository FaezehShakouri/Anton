// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {IL2Registry} from "durin/src/interfaces/IL2Registry.sol";

/// @notice Grants `ChatRegistrar` the registrar role on an `L2Registry`.
/// @dev `PRIVATE_KEY` must belong to the registry admin (owner of the base node NFT).
contract AddRegistrar is Script {
    function run() external {
        uint256 pk = vm.envUint("PRIVATE_KEY");
        address registryAddr = vm.envAddress("L2_REGISTRY");
        address registrarAddr = vm.envAddress("CHAT_REGISTRAR");

        vm.startBroadcast(pk);
        IL2Registry(registryAddr).addRegistrar(registrarAddr);
        vm.stopBroadcast();

        console.log("addRegistrar OK:", registrarAddr);
    }
}

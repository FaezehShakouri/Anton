// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title ChatRegistrar (scaffold placeholder)
/// @notice Mints `*.chat.eth` subnames on a Durin-style L2 Registry, atomically
///         setting `addr(60)`, `axl_peer_id`, and `axl_pubkey` text records so
///         the Axen onboarding flow is one transaction rather than three.
///
/// This file is intentionally a stub for the scaffold step. The real
/// implementation extends `durin/examples/L2Registrar.sol` and lives in a
/// later plan step. Once Durin is installed via `forge install
/// namestonehq/durin`, the inheritance chain becomes:
///
///     contract ChatRegistrar is L2Registrar { ... }
///
/// with a `registerWithRecords(label, owner, peerId, pubkeyPem)` entrypoint
/// that issues `setAddr` + `setText("axl_peer_id", ...)` + `setText("axl_pubkey",
/// ...)` + `createSubnode` in a single transaction.
contract ChatRegistrar {
    /// @notice Emitted when a `*.chat.eth` subname is minted with full Axen
    ///         identity records. Wired up here so off-chain indexers and
    ///         the Axen demo can subscribe to it from day one.
    event ChatNameRegistered(
        string label,
        address indexed owner,
        bytes peerId,
        string pubkeyPem
    );

    error NotImplemented();

    /// @notice Reserved entrypoint mirroring the final ABI. The implementation
    ///         lands in a later plan step; calling this in the scaffold reverts
    ///         loudly so accidental deployments are obvious.
    function registerWithRecords(
        string calldata, /* label */
        address, /* owner */
        bytes calldata, /* peerId */
        string calldata /* pubkeyPem */
    ) external pure {
        revert NotImplemented();
    }
}

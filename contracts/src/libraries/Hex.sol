// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @notice Lowercase hex encoding with `0x` prefix (matches Anton Rust `peer_id_hex`).
library Hex {
    function bytesToHexPrefixed(bytes memory data) internal pure returns (string memory) {
        bytes memory alphabet = "0123456789abcdef";
        bytes memory str = new bytes(2 + data.length * 2);
        str[0] = 0x30;
        str[1] = 0x78;
        unchecked {
            for (uint256 i = 0; i < data.length; i++) {
                str[2 + i * 2] = alphabet[uint8(data[i] >> 4)];
                str[3 + i * 2] = alphabet[uint8(data[i] & 0x0f)];
            }
        }
        return string(str);
    }
}

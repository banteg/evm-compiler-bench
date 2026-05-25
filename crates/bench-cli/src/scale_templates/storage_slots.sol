// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract {{CONTRACT_NAME}} {
{{SLOTS}}

    function writeAll(uint256 seed) external returns (uint256 total) {
{{WRITE_BODY}}
    }

    function readAll() external view returns (uint256 total) {
{{READ_BODY}}
    }
}

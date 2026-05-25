// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract {{CONTRACT_NAME}} {
    uint256 public sink;

    function setSink(uint256 value) external returns (uint256) {
        sink = value;
        return value;
    }

{{FUNCTIONS}}
}

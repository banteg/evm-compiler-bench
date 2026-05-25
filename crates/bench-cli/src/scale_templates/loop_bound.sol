// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract {{CONTRACT_NAME}} {
    function runLoop() external pure returns (uint256 total) {
        for (uint256 i = 0; i < {{N}}; i++) {
            total += i;
        }
    }
}

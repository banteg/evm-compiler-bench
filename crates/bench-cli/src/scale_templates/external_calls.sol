// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract {{CONTRACT_NAME}} {
    function ping(uint256) external {}

    function callMany() external returns (uint256 total) {
        for (uint256 i = 0; i < {{N}}; i++) {
            this.ping(i);
            total += i;
        }
    }
}

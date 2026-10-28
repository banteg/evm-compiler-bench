// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract {{CONTRACT_NAME}} {
    function ping(uint256 x) external pure returns (uint256) {
        return x;
    }

    function callMany() external view returns (uint256 total) {
        for (uint256 i = 0; i < {{N}}; i++) {
            total += this.ping(i);
        }
    }
}

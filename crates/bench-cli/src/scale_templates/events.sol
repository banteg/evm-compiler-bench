// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract {{CONTRACT_NAME}} {
    event Tick(uint256 indexed index, uint256 value);

    function emitMany() external returns (uint256 total) {
        for (uint256 i = 0; i < {{N}}; i++) {
            emit Tick(i, i + 1);
            total += i + 1;
        }
    }
}

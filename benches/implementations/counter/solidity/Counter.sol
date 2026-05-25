// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract Counter {
    uint256 public value;

    constructor(uint256 initialValue) {
        value = initialValue;
    }

    function increment() external returns (uint256) {
        value += 1;
        return value;
    }

    function add(uint256 amount) external returns (uint256) {
        value += amount;
        return value;
    }

    function reset() external returns (uint256) {
        value = 0;
        return value;
    }
}

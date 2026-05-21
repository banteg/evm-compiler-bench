// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract OwnablePausable {
    address public owner;
    uint256 public counter;
    bool public paused;

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event Paused(address indexed account);
    event Unpaused(address indexed account);

    constructor() {
        owner = msg.sender;
        emit OwnershipTransferred(address(0), msg.sender);
    }

    modifier onlyOwner() {
        require(msg.sender == owner, "owner");
        _;
    }

    modifier whenNotPaused() {
        require(!paused, "paused");
        _;
    }

    function transferOwnership(address newOwner) external onlyOwner returns (bool) {
        require(newOwner != address(0), "zero");
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
        return true;
    }

    function pause() external onlyOwner returns (bool) {
        paused = true;
        emit Paused(msg.sender);
        return true;
    }

    function unpause() external onlyOwner returns (bool) {
        paused = false;
        emit Unpaused(msg.sender);
        return true;
    }

    function guardedIncrement() external whenNotPaused returns (uint256) {
        counter += 1;
        return counter;
    }
}

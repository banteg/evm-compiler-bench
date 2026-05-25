// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract Create2AddressHashing {
    mapping(bytes32 => uint256) public saltValue;

    event Recorded(bytes32 indexed salt, address predicted, uint256 value);

    function initCodeHash(uint256 value) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes32(value)));
    }

    function computeAddress(bytes32 salt, uint256 value) public view returns (address) {
        bytes32 digest = keccak256(abi.encodePacked(bytes1(0xff), address(this), salt, initCodeHash(value)));
        return address(uint160(uint256(digest)));
    }

    function record(bytes32 salt, uint256 value) external returns (uint256) {
        address predicted = computeAddress(salt, value);
        saltValue[salt] = value;
        emit Recorded(salt, predicted, value);
        return value;
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract Eip1167CodehashBookkeeping {
    mapping(bytes32 => uint256) public cloneValue;

    event Cloned(bytes32 indexed salt, address predicted, uint256 value);

    function proxyCodeHash(address implementation) public pure returns (bytes32) {
        return keccak256(
            abi.encodePacked(
                hex"3d602d80600a3d3981f3",
                hex"363d3d373d3d3d363d73",
                bytes20(implementation),
                hex"5af43d82803e903d91602b57fd5bf3"
            )
        );
    }

    function predictClone(address implementation, bytes32 salt) public view returns (address) {
        bytes32 digest = keccak256(abi.encodePacked(bytes1(0xff), address(this), salt, proxyCodeHash(implementation)));
        return address(uint160(uint256(digest)));
    }

    function recordClone(address implementation, bytes32 salt, uint256 value) external returns (uint256) {
        address predicted = predictClone(implementation, salt);
        cloneValue[salt] = value;
        emit Cloned(salt, predicted, value);
        return value;
    }
}

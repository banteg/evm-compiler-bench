// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract Erc20PermitHashing {
    mapping(address => uint256) public nonces;

    function hashPermit(
        address owner,
        address spender,
        uint256 value,
        uint256 nonce,
        uint256 deadline
    ) public pure returns (bytes32) {
        return keccak256(
            abi.encodePacked(
                bytes32(uint256(uint160(owner))),
                bytes32(uint256(uint160(spender))),
                bytes32(value),
                bytes32(nonce),
                bytes32(deadline)
            )
        );
    }

    function useNonce(address owner) external returns (uint256) {
        uint256 current = nonces[owner];
        nonces[owner] = current + 1;
        return current;
    }

    function hashCurrentPermit(
        address owner,
        address spender,
        uint256 value,
        uint256 deadline
    ) external view returns (bytes32) {
        return hashPermit(owner, spender, value, nonces[owner], deadline);
    }
}

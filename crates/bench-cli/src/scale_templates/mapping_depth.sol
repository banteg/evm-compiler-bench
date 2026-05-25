// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract {{CONTRACT_NAME}} {
{{LINKS}}

    function writeChain(uint256 seed) external returns (uint256 current) {
        current = seed;
{{WRITE_BODY}}
    }

    function readChain(uint256 seed) external view returns (uint256 current) {
        current = seed;
{{READ_BODY}}
    }
}

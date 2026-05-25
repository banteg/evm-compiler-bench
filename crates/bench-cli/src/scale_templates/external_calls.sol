// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract {{CONTRACT_NAME}} {
    function ping(uint256) external pure {}

    function callMany() external returns (uint256 total) {
        for (uint256 i = 0; i < {{N}}; i++) {
            (bool ok,) = address(this).staticcall(abi.encodeWithSelector(bytes4(0x773acdef), i));
            require(ok);
            total += i;
        }
    }
}

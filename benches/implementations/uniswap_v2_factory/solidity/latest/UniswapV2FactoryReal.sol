// SPDX-License-Identifier: GPL-3.0-or-later
pragma solidity ^0.8.35;

interface IUniswapV2PairFactoryTarget {
    function initialize(address token0, address token1) external;
}

contract UniswapV2FactoryReal {
    address public feeTo;
    address public feeToSetter;

    mapping(address => mapping(address => address)) public getPair;
    address[] public allPairs;
    bytes private pairCode;

    event PairCreated(address indexed token0, address indexed token1, address pair, uint256);

    constructor(bytes memory pairCode_, address feeToSetter_) {
        pairCode = pairCode_;
        feeToSetter = feeToSetter_;
    }

    function allPairsLength() external view returns (uint256) {
        return allPairs.length;
    }

    function createPair(address tokenA, address tokenB) external returns (address pair) {
        require(tokenA != tokenB, "UniswapV2: IDENTICAL_ADDRESSES");
        (address token0, address token1) = tokenA < tokenB ? (tokenA, tokenB) : (tokenB, tokenA);
        require(token0 != address(0), "UniswapV2: ZERO_ADDRESS");
        require(getPair[token0][token1] == address(0), "UniswapV2: PAIR_EXISTS");
        bytes32 salt = keccak256(abi.encodePacked(token0, token1));
        bytes memory code = pairCode;
        assembly {
            pair := create2(0, add(code, 0x20), mload(code), salt)
        }
        require(pair != address(0) && _hasCode(pair), "UniswapV2: CREATE2_FAILED");
        IUniswapV2PairFactoryTarget(pair).initialize(token0, token1);
        getPair[token0][token1] = pair;
        getPair[token1][token0] = pair;
        allPairs.push(pair);
        emit PairCreated(token0, token1, pair, allPairs.length);
    }

    function setFeeTo(address feeTo_) external {
        require(msg.sender == feeToSetter, "UniswapV2: FORBIDDEN");
        feeTo = feeTo_;
    }

    function setFeeToSetter(address feeToSetter_) external {
        require(msg.sender == feeToSetter, "UniswapV2: FORBIDDEN");
        feeToSetter = feeToSetter_;
    }

    function _hasCode(address account) private view returns (bool) {
        uint256 size;
        assembly {
            size := extcodesize(account)
        }
        return size != 0;
    }
}

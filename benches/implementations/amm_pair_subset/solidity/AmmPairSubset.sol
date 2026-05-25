// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

contract AmmPairSubset {
    uint256 public reserve0;
    uint256 public reserve1;
    uint256 public totalLiquidity;

    event Mint(uint256 amount0, uint256 amount1, uint256 liquidity);
    event Burn(uint256 liquidity, uint256 amount0, uint256 amount1);
    event Swap(uint256 amount0Out, uint256 amount1Out, uint256 amount0In, uint256 amount1In);
    event Sync(uint256 reserve0, uint256 reserve1);

    function mint(uint256 amount0, uint256 amount1) external returns (uint256 liquidity) {
        require(amount0 > 0 && amount1 > 0, "amount");
        liquidity = amount0 + amount1;
        reserve0 += amount0;
        reserve1 += amount1;
        totalLiquidity += liquidity;
        emit Mint(amount0, amount1, liquidity);
    }

    function burn(uint256 liquidity) external returns (uint256 amount0, uint256 amount1) {
        require(totalLiquidity >= liquidity && liquidity > 0, "liquidity");
        amount0 = reserve0 * liquidity / totalLiquidity;
        amount1 = reserve1 * liquidity / totalLiquidity;
        reserve0 -= amount0;
        reserve1 -= amount1;
        totalLiquidity -= liquidity;
        emit Burn(liquidity, amount0, amount1);
    }

    function swap(uint256 amount0Out, uint256 amount1Out, uint256 amount0In, uint256 amount1In) external returns (bool) {
        require(amount0Out <= reserve0 && amount1Out <= reserve1, "reserve");
        require(amount0In > 0 || amount1In > 0, "input");
        reserve0 = reserve0 + amount0In - amount0Out;
        reserve1 = reserve1 + amount1In - amount1Out;
        emit Swap(amount0Out, amount1Out, amount0In, amount1In);
        return true;
    }

    function sync(uint256 balance0, uint256 balance1) external returns (bool) {
        reserve0 = balance0;
        reserve1 = balance1;
        emit Sync(balance0, balance1);
        return true;
    }

    function getReserves() external view returns (uint256, uint256) {
        return (reserve0, reserve1);
    }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract CurveStableSwap2CoinReal {
    uint256 public constant N_COINS = 2;
    uint256 public constant A_PRECISION = 100;
    uint256 public constant FEE_DENOMINATOR = 10_000_000_000;

    string public constant name = "Real-Derived Curve Stableswap 2-Coin";
    string public constant symbol = "RD-CURVE-2";
    uint8 public constant decimals = 18;

    uint256 public A;
    uint256 public fee;
    uint256 public admin_fee;
    bool public initialized;

    uint256[2] public balances;
    uint256[2] public admin_balances;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;

    event AddLiquidity(address indexed provider, uint256 amount0, uint256 amount1, uint256 minted, uint256 invariant);
    event TokenExchange(address indexed buyer, uint256 soldId, uint256 tokensSold, uint256 boughtId, uint256 tokensBought);
    event RemoveLiquidity(address indexed provider, uint256 amount0, uint256 amount1, uint256 burned);
    event RemoveLiquidityOne(address indexed provider, uint256 tokenAmount, uint256 coinIndex, uint256 coinAmount);
    event Transfer(address indexed from, address indexed to, uint256 value);

    modifier ready() {
        require(initialized, "not initialized");
        _;
    }

    function initialize(uint256 amp, uint256 swapFee, uint256 adminFee) external returns (bool) {
        require(!initialized, "initialized");
        require(amp > 0, "amp");
        require(swapFee <= FEE_DENOMINATOR / 10, "fee");
        require(adminFee <= FEE_DENOMINATOR, "admin");
        initialized = true;
        A = amp;
        fee = swapFee;
        admin_fee = adminFee;
        return true;
    }

    function add_liquidity(uint256 amount0, uint256 amount1, uint256 min_mint_amount)
        external
        ready
        returns (uint256 minted)
    {
        require(amount0 > 0 || amount1 > 0, "amount");
        uint256 d0 = totalSupply == 0 ? 0 : _getD(balances[0], balances[1]);
        balances[0] += amount0;
        balances[1] += amount1;
        uint256 d1 = _getD(balances[0], balances[1]);
        if (totalSupply == 0) {
            minted = d1;
        } else {
            minted = totalSupply * (d1 - d0) / d0;
        }
        require(minted >= min_mint_amount && minted > 0, "slippage");
        _mint(msg.sender, minted);
        emit AddLiquidity(msg.sender, amount0, amount1, minted, d1);
    }

    function exchange(uint256 i, uint256 j, uint256 dx, uint256 min_dy) external ready returns (uint256 dy) {
        require(i < N_COINS && j < N_COINS && i != j, "coin");
        require(dx > 0, "dx");
        uint256 x = balances[i] + dx;
        uint256 y = _getY(i, j, x);
        uint256 oldY = balances[j];
        dy = oldY - y - 1;
        uint256 feeAmount = dy * fee / FEE_DENOMINATOR;
        uint256 adminCut = feeAmount * admin_fee / FEE_DENOMINATOR;
        uint256 userDy = dy - feeAmount;
        require(userDy >= min_dy, "slippage");

        balances[i] = x;
        balances[j] = oldY - dy + feeAmount - adminCut;
        admin_balances[j] += adminCut;
        emit TokenExchange(msg.sender, i, dx, j, userDy);
        return userDy;
    }

    function remove_liquidity(uint256 lpAmount, uint256 min0, uint256 min1)
        external
        ready
        returns (uint256 amount0, uint256 amount1)
    {
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        uint256 supply = totalSupply;
        amount0 = balances[0] * lpAmount / supply;
        amount1 = balances[1] * lpAmount / supply;
        require(amount0 >= min0 && amount1 >= min1, "slippage");
        _burn(msg.sender, lpAmount);
        balances[0] -= amount0;
        balances[1] -= amount1;
        emit RemoveLiquidity(msg.sender, amount0, amount1, lpAmount);
    }

    function remove_liquidity_one_coin(uint256 lpAmount, uint256 i, uint256 minAmount)
        external
        ready
        returns (uint256 amount)
    {
        require(i < N_COINS, "coin");
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        amount = balances[i] * lpAmount / totalSupply;
        uint256 feeAmount = amount * fee / FEE_DENOMINATOR;
        uint256 adminCut = feeAmount * admin_fee / FEE_DENOMINATOR;
        uint256 userAmount = amount - feeAmount;
        require(userAmount >= minAmount, "slippage");
        _burn(msg.sender, lpAmount);
        balances[i] = balances[i] - amount + feeAmount - adminCut;
        admin_balances[i] += adminCut;
        emit RemoveLiquidityOne(msg.sender, lpAmount, i, userAmount);
        return userAmount;
    }

    function get_D(uint256 x0, uint256 x1) external view returns (uint256) {
        return _getD(x0, x1);
    }

    function get_virtual_price() external view returns (uint256) {
        if (totalSupply == 0) {
            return 1e18;
        }
        return _getD(balances[0], balances[1]) * 1e18 / totalSupply;
    }

    function _mint(address to, uint256 value) internal {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
    }

    function _burn(address from, uint256 value) internal {
        balanceOf[from] -= value;
        totalSupply -= value;
        emit Transfer(from, address(0), value);
    }

    function _getD(uint256 x0, uint256 x1) internal view returns (uint256) {
        uint256 sum = x0 + x1;
        if (sum == 0) {
            return 0;
        }
        uint256 d = sum;
        uint256 ann = A * N_COINS;
        for (uint256 dIdx = 0; dIdx < 255; dIdx++) {
            uint256 dP = d * d / (x0 * N_COINS);
            dP = dP * d / (x1 * N_COINS);
            uint256 previousD = d;
            d = (ann * sum / A_PRECISION + dP * N_COINS) * d
                / ((ann - A_PRECISION) * d / A_PRECISION + (N_COINS + 1) * dP);
            if (d > previousD) {
                if (d - previousD <= 1) return d;
            } else if (previousD - d <= 1) {
                return d;
            }
        }
        return d;
    }

    function _getY(uint256 i, uint256 j, uint256 x) internal view returns (uint256) {
        uint256 d = _getD(balances[0], balances[1]);
        uint256 ann = A * N_COINS;
        uint256 c = d;
        uint256 s;
        for (uint256 idx = 0; idx < N_COINS; idx++) {
            if (idx == j) {
                continue;
            }
            uint256 currentX = idx == i ? x : balances[idx];
            s += currentX;
            c = c * d / (currentX * N_COINS);
        }
        c = c * d * A_PRECISION / (ann * N_COINS);
        uint256 b = s + d * A_PRECISION / ann;
        uint256 y = d;
        for (uint256 yIdx = 0; yIdx < 255; yIdx++) {
            uint256 previousY = y;
            y = (y * y + c) / (2 * y + b - d);
            if (y > previousY) {
                if (y - previousY <= 1) return y;
            } else if (previousY - y <= 1) {
                return y;
            }
        }
        return y;
    }
}

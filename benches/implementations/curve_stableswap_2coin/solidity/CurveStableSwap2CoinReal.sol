// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

interface CurveBenchERC20 {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

contract CurveStableSwap2CoinReal {
    uint256 public constant N_COINS = 2;
    uint256 public constant A_PRECISION = 100;
    uint256 public constant FEE_DENOMINATOR = 10_000_000_000;
    bytes32 public constant EIP712_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract,bytes32 salt)");
    bytes32 public constant EIP2612_TYPEHASH =
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");
    bytes32 public constant ERC1271_MAGIC_VALUE = 0x1626ba7e00000000000000000000000000000000000000000000000000000000;

    string public constant name = "Curve.fi Stablecoin";
    string public constant symbol = "crv2";
    string public constant version = "v7.0.0";
    uint8 public constant decimals = 18;

    address[2] public coins;
    uint256 public A;
    uint256 public fee;
    uint256 public admin_fee;
    uint256 public offpeg_fee_multiplier;
    bool public initialized;

    uint256[2] public rate_multipliers;
    uint256[2] public balances;
    uint256[2] public admin_balances;
    uint256 public totalSupply;
    uint256 public cachedChainId;
    bytes32 public salt;
    bytes32 public nameHash;
    bytes32 public cachedDomainSeparator;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    mapping(address => uint256) public nonces;

    event AddLiquidity(
        address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 invariant, uint256 tokenSupply
    );
    event TokenExchange(
        address indexed buyer, uint256 soldId, uint256 tokensSold, uint256 boughtId, uint256 tokensBought
    );
    event RemoveLiquidity(address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 tokenSupply);
    event RemoveLiquidityOne(address indexed provider, uint256 tokenAmount, uint256 coinIndex, uint256 coinAmount);
    event RemoveLiquidityImbalance(
        address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 invariant, uint256 tokenSupply
    );
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

    modifier ready() {
        require(initialized, "not initialized");
        _;
    }

    constructor(
        string memory name_,
        string memory symbol_,
        uint256 amp,
        uint256 swapFee,
        uint256 offpegFeeMultiplier,
        uint256 maExpTime,
        address[] memory coins_,
        uint256[] memory rateMultipliers,
        uint8[] memory assetTypes,
        bytes4[] memory methodIds,
        address[] memory oracles
    ) {
        name_;
        symbol_;
        offpegFeeMultiplier;
        maExpTime;
        rateMultipliers;
        assetTypes;
        methodIds;
        oracles;
        require(coins_.length == N_COINS, "coin length");
        require(coins_[0] != address(0) && coins_[1] != address(0) && coins_[0] != coins_[1], "coins");
        require(amp > 0, "amp");
        require(swapFee <= FEE_DENOMINATOR / 10, "fee");
        initialized = true;
        coins[0] = coins_[0];
        coins[1] = coins_[1];
        rate_multipliers[0] = rateMultipliers.length > 0 ? rateMultipliers[0] : 1e18;
        rate_multipliers[1] = rateMultipliers.length > 1 ? rateMultipliers[1] : 1e18;
        A = amp;
        fee = swapFee;
        admin_fee = 5_000_000_000;
        offpeg_fee_multiplier = offpegFeeMultiplier;
        cachedChainId = block.chainid;
        salt = block.number == 0 ? bytes32(0) : blockhash(block.number - 1);
        nameHash = keccak256("Curve.fi Stablecoin");
        cachedDomainSeparator = _buildDomainSeparator(cachedChainId);
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return true;
    }

    function transfer(address to, uint256 value) external returns (bool) {
        _transfer(msg.sender, to, value);
        return true;
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= value, "allowance");
            allowance[from][msg.sender] = allowed - value;
            emit Approval(from, msg.sender, allowed - value);
        }
        _transfer(from, to, value);
        return true;
    }

    function add_liquidity(uint256[] calldata amounts, uint256 minMintAmount, address receiver)
        external
        ready
        returns (uint256 minted)
    {
        require(receiver != address(0), "receiver");
        require(amounts.length == N_COINS, "amount length");
        require(amounts[0] > 0 || amounts[1] > 0, "amount");
        uint256[2] memory oldBalances = balances;
        uint256 supply = totalSupply;
        uint256 d0 = supply == 0 ? 0 : _getD(oldBalances[0], oldBalances[1]);
        uint256[2] memory newBalances = oldBalances;
        for (uint256 i = 0; i < N_COINS; i++) {
            if (amounts[i] > 0) {
                _safeTransferFrom(coins[i], msg.sender, address(this), amounts[i]);
                newBalances[i] = oldBalances[i] + amounts[i];
            } else {
                require(supply != 0, "initial amount");
            }
        }
        uint256 d1 = _getD(newBalances[0], newBalances[1]);
        require(d1 > d0, "invariant");
        uint256[] memory fees = new uint256[](N_COINS);
        if (supply == 0) {
            minted = d1;
        } else {
            uint256 baseFee = _baseFee();
            uint256 ys = (d0 + d1) / N_COINS;
            for (uint256 feeIndex = 0; feeIndex < N_COINS; feeIndex++) {
                uint256 idealBalance = d1 * oldBalances[feeIndex] / d0;
                uint256 difference = _absDiff(idealBalance, newBalances[feeIndex]);
                uint256 xs = oldBalances[feeIndex] + newBalances[feeIndex];
                fees[feeIndex] = _dynamicFee(xs, ys, baseFee) * difference / FEE_DENOMINATOR;
                admin_balances[feeIndex] += fees[feeIndex] * admin_fee / FEE_DENOMINATOR;
                newBalances[feeIndex] -= fees[feeIndex];
            }
            d1 = _getD(newBalances[0], newBalances[1]);
            minted = supply * (d1 - d0) / d0;
        }
        require(minted >= minMintAmount && minted > 0, "slippage");
        balances[0] = newBalances[0];
        balances[1] = newBalances[1];
        _mint(receiver, minted);
        emit AddLiquidity(msg.sender, amounts, fees, d1, totalSupply);
    }

    function exchange(int128 i, int128 j, uint256 dx, uint256 minDy, address receiver)
        external
        ready
        returns (uint256 dy)
    {
        require(i >= 0 && j >= 0 && uint256(int256(i)) < N_COINS && uint256(int256(j)) < N_COINS && i != j, "coin");
        require(dx > 0, "dx");
        uint256 coinIn = uint256(int256(i));
        uint256 coinOut = uint256(int256(j));
        _safeTransferFrom(coins[coinIn], msg.sender, address(this), dx);

        uint256 newBalanceIn;
        uint256 newBalanceOut;
        uint256 adminCut;
        (newBalanceIn, newBalanceOut, dy, adminCut) = _calcExchange(coinIn, coinOut, dx);
        require(dy >= minDy, "slippage");

        balances[coinIn] = newBalanceIn;
        balances[coinOut] = newBalanceOut;
        admin_balances[coinOut] += adminCut;
        _safeTransfer(coins[coinOut], receiver, dy);
        emit TokenExchange(msg.sender, coinIn, dx, coinOut, dy);
    }

    function exchange_received(int128 i, int128 j, uint256 dx, uint256 minDy, address receiver)
        external
        ready
        returns (uint256 dy)
    {
        require(i >= 0 && j >= 0 && uint256(int256(i)) < N_COINS && uint256(int256(j)) < N_COINS && i != j, "coin");
        require(dx > 0, "dx");
        uint256 coinIn = uint256(int256(i));
        uint256 coinOut = uint256(int256(j));
        uint256 storedBalance = balances[coinIn] + admin_balances[coinIn];
        uint256 tokenBalance = CurveBenchERC20(coins[coinIn]).balanceOf(address(this));
        require(tokenBalance >= storedBalance + dx, "optimistic transfer");
        uint256 actualDx = tokenBalance - storedBalance;

        uint256 newBalanceIn;
        uint256 newBalanceOut;
        uint256 adminCut;
        (newBalanceIn, newBalanceOut, dy, adminCut) = _calcExchange(coinIn, coinOut, actualDx);
        require(dy >= minDy, "slippage");

        balances[coinIn] = newBalanceIn;
        balances[coinOut] = newBalanceOut;
        admin_balances[coinOut] += adminCut;
        _safeTransfer(coins[coinOut], receiver, dy);
        emit TokenExchange(msg.sender, coinIn, actualDx, coinOut, dy);
    }

    function remove_liquidity(uint256 lpAmount, uint256[] calldata minAmounts, address receiver)
        external
        ready
        returns (uint256[] memory amounts)
    {
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        require(minAmounts.length == N_COINS, "amount length");
        uint256 supply = totalSupply;
        amounts = new uint256[](N_COINS);
        _burn(msg.sender, lpAmount);
        for (uint256 i = 0; i < N_COINS; i++) {
            amounts[i] = balances[i] * lpAmount / supply;
            require(amounts[i] >= minAmounts[i], "slippage");
            balances[i] -= amounts[i];
            _safeTransfer(coins[i], receiver, amounts[i]);
        }
        emit RemoveLiquidity(msg.sender, amounts, _emptyFees(), totalSupply);
    }

    function remove_liquidity_imbalance(uint256[] calldata amounts, uint256 maxBurnAmount, address receiver)
        external
        ready
        returns (uint256 burnAmount)
    {
        require(amounts.length == N_COINS, "amount length");
        require(receiver != address(0), "receiver");
        uint256[2] memory oldBalances = balances;
        uint256 d0 = _getD(oldBalances[0], oldBalances[1]);
        uint256[2] memory newBalances = oldBalances;
        for (uint256 i = 0; i < N_COINS; i++) {
            if (amounts[i] > 0) {
                require(newBalances[i] >= amounts[i], "balance");
                newBalances[i] -= amounts[i];
            }
        }

        uint256 d1 = _getD(newBalances[0], newBalances[1]);
        uint256 baseFee = _baseFee();
        uint256 ys = (d0 + d1) / N_COINS;
        uint256[] memory fees = new uint256[](N_COINS);
        for (uint256 feeIndex = 0; feeIndex < N_COINS; feeIndex++) {
            uint256 idealBalance = d1 * oldBalances[feeIndex] / d0;
            uint256 difference = _absDiff(idealBalance, newBalances[feeIndex]);
            uint256 xs = oldBalances[feeIndex] + newBalances[feeIndex];
            fees[feeIndex] = _dynamicFee(xs, ys, baseFee) * difference / FEE_DENOMINATOR;
            admin_balances[feeIndex] += fees[feeIndex] * admin_fee / FEE_DENOMINATOR;
            newBalances[feeIndex] -= fees[feeIndex];
        }

        d1 = _getD(newBalances[0], newBalances[1]);
        burnAmount = (d0 - d1) * totalSupply / d0 + 1;
        require(burnAmount > 1, "burn");
        require(burnAmount <= maxBurnAmount, "slippage");
        _burn(msg.sender, burnAmount);
        balances[0] = newBalances[0];
        balances[1] = newBalances[1];
        for (uint256 i = 0; i < N_COINS; i++) {
            if (amounts[i] > 0) {
                _safeTransfer(coins[i], receiver, amounts[i]);
            }
        }
        emit RemoveLiquidityImbalance(msg.sender, amounts, fees, d1, totalSupply);
    }

    function remove_liquidity_one_coin(uint256 lpAmount, int128 i, uint256 minAmount, address receiver)
        external
        ready
        returns (uint256 userAmount)
    {
        require(i >= 0 && uint256(int256(i)) < N_COINS, "coin");
        uint256 coinIndex = uint256(int256(i));
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        uint256 feeAmount;
        (userAmount, feeAmount) = _calcWithdrawOneCoin(lpAmount, coinIndex);
        uint256 adminCut = feeAmount * admin_fee / FEE_DENOMINATOR;
        require(userAmount >= minAmount, "slippage");
        _burn(msg.sender, lpAmount);
        balances[coinIndex] = balances[coinIndex] - userAmount - adminCut;
        admin_balances[coinIndex] += adminCut;
        _safeTransfer(coins[coinIndex], receiver, userAmount);
        emit RemoveLiquidityOne(msg.sender, lpAmount, coinIndex, userAmount);
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

    function A_precise() external view returns (uint256) {
        return A * A_PRECISION;
    }

    function get_balances() external view returns (uint256[] memory result) {
        result = new uint256[](N_COINS);
        result[0] = balances[0];
        result[1] = balances[1];
    }

    function stored_rates() external view returns (uint256[] memory result) {
        result = new uint256[](N_COINS);
        result[0] = rate_multipliers[0];
        result[1] = rate_multipliers[1];
    }

    function calc_withdraw_one_coin(uint256 burnAmount, int128 i) external view returns (uint256) {
        require(i >= 0 && uint256(int256(i)) < N_COINS, "coin");
        (uint256 dy,) = _calcWithdrawOneCoin(burnAmount, uint256(int256(i)));
        return dy;
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return _domainSeparator();
    }

    function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
        external
        returns (bool)
    {
        require(owner != address(0), "owner");
        require(block.timestamp <= deadline, "deadline");
        uint256 nonce = nonces[owner];
        bytes32 digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                _domainSeparator(),
                keccak256(abi.encode(EIP2612_TYPEHASH, owner, spender, value, nonce, deadline))
            )
        );
        address recovered = ecrecover(digest, v, r, s);
        if (recovered != owner) {
            bytes memory signature = abi.encodePacked(r, s, bytes1(v));
            (bool ok, bytes memory result) =
                owner.staticcall(abi.encodeWithSignature("isValidSignature(bytes32,bytes)", digest, signature));
            require(ok && result.length >= 32 && abi.decode(result, (bytes32)) == ERC1271_MAGIC_VALUE, "signature");
        }
        allowance[owner][spender] = value;
        nonces[owner] = nonce + 1;
        emit Approval(owner, spender, value);
        return true;
    }

    function _emptyFees() internal pure returns (uint256[] memory fees) {
        fees = new uint256[](N_COINS);
    }

    function _safeTransfer(address coin, address to, uint256 value) internal {
        _optionalReturn(coin, abi.encodeWithSelector(CurveBenchERC20.transfer.selector, to, value), "transfer");
    }

    function _safeTransferFrom(address coin, address from, address to, uint256 value) internal {
        _optionalReturn(
            coin, abi.encodeWithSelector(CurveBenchERC20.transferFrom.selector, from, to, value), "transferFrom"
        );
    }

    function _optionalReturn(address coin, bytes memory data, string memory message) internal {
        (bool ok, bytes memory returndata) = coin.call(data);
        require(ok, message);
        if (returndata.length > 0) {
            require(abi.decode(returndata, (bool)), message);
        }
    }

    function _domainSeparator() internal view returns (bytes32) {
        if (block.chainid != cachedChainId) {
            return _buildDomainSeparator(block.chainid);
        }
        return cachedDomainSeparator;
    }

    function _buildDomainSeparator(uint256 chainId) internal view returns (bytes32) {
        return keccak256(abi.encode(EIP712_TYPEHASH, nameHash, keccak256("v7.0.0"), chainId, address(this), salt));
    }

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }

    function _mint(address to, uint256 value) internal {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
    }

    function _burn(address from, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
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

    function _baseFee() internal view returns (uint256) {
        return fee * N_COINS / (4 * (N_COINS - 1));
    }

    function _dynamicFee(uint256 xpi, uint256 xpj, uint256 base) internal view returns (uint256) {
        if (offpeg_fee_multiplier <= FEE_DENOMINATOR) {
            return base;
        }
        uint256 xps2 = (xpi + xpj) * (xpi + xpj);
        return offpeg_fee_multiplier * base
            / (((offpeg_fee_multiplier - FEE_DENOMINATOR) * 4 * xpi * xpj / xps2) + FEE_DENOMINATOR);
    }

    function _absDiff(uint256 a, uint256 b) internal pure returns (uint256) {
        return a > b ? a - b : b - a;
    }

    function _calcExchange(uint256 coinIn, uint256 coinOut, uint256 dx)
        internal
        view
        returns (uint256 newBalanceIn, uint256 newBalanceOut, uint256 userDy, uint256 adminCut)
    {
        uint256[2] memory oldBalances = balances;
        newBalanceIn = oldBalances[coinIn] + dx;
        uint256 y = _getY(coinIn, coinOut, newBalanceIn, oldBalances, _getD(oldBalances[0], oldBalances[1]));
        uint256 grossDy = oldBalances[coinOut] - y - 1;
        uint256 feeAmount = grossDy
            * _dynamicFee((oldBalances[coinIn] + newBalanceIn) / 2, (oldBalances[coinOut] + y) / 2, fee)
            / FEE_DENOMINATOR;
        adminCut = feeAmount * admin_fee / FEE_DENOMINATOR;
        userDy = grossDy - feeAmount;
        newBalanceOut = oldBalances[coinOut] - grossDy + feeAmount - adminCut;
    }

    function _getY(uint256 i, uint256 j, uint256 x, uint256[2] memory xp, uint256 d) internal view returns (uint256) {
        uint256 ann = A * N_COINS;
        uint256 c = d;
        uint256 s;
        for (uint256 idx = 0; idx < N_COINS; idx++) {
            if (idx == j) {
                continue;
            }
            uint256 currentX = idx == i ? x : xp[idx];
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

    function _getYD(uint256 i, uint256[2] memory xp, uint256 d) internal view returns (uint256) {
        uint256 ann = A * N_COINS;
        uint256 c = d;
        uint256 s;
        for (uint256 idx = 0; idx < N_COINS; idx++) {
            if (idx == i) {
                continue;
            }
            uint256 currentX = xp[idx];
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

    function _calcWithdrawOneCoin(uint256 lpAmount, uint256 i) internal view returns (uint256 dy, uint256 feeAmount) {
        uint256[2] memory xp = balances;
        uint256 d0 = _getD(xp[0], xp[1]);
        uint256 d1 = d0 - lpAmount * d0 / totalSupply;
        uint256 newY = _getYD(i, xp, d1);
        uint256[2] memory xpReduced = xp;
        uint256 baseFee = _baseFee();
        uint256 ys = (d0 + d1) / (2 * N_COINS);

        for (uint256 j = 0; j < N_COINS; j++) {
            uint256 dxExpected;
            uint256 xavg;
            if (j == i) {
                dxExpected = xp[j] * d1 / d0 - newY;
                xavg = (xp[j] + newY) / 2;
            } else {
                dxExpected = xp[j] - xp[j] * d1 / d0;
                xavg = xp[j];
            }
            xpReduced[j] = xp[j] - _dynamicFee(xavg, ys, baseFee) * dxExpected / FEE_DENOMINATOR;
        }

        uint256 reducedY = _getYD(i, xpReduced, d1);
        dy = xpReduced[i] - reducedY - 1;
        feeAmount = xp[i] - newY - dy;
    }
}

    function proofOne() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](1);
        proof[0] = SIBLING;
    }

    function proofEmpty() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](0);
    }

    function proofMany(uint256 n) internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](n);
        for (uint256 i = 0; i < n; i++) {
            proof[i] = keccak256(abi.encodePacked("sibling", i));
        }
    }

    function proofRoot(bytes32[] memory proof, bytes32 leaf) internal pure returns (bytes32 computed) {
        computed = leaf;
        for (uint256 i = 0; i < proof.length; i++) {
            bytes32 sibling = proof[i];
            computed = computed < sibling
                ? keccak256(abi.encodePacked(computed, sibling))
                : keccak256(abi.encodePacked(sibling, computed));
        }
    }

    function curveAmounts(uint256 amount0, uint256 amount1) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](2);
        amounts[0] = amount0;
        amounts[1] = amount1;
    }

    function curveAmounts3(uint256 amount0, uint256 amount1, uint256 amount2) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](3);
        amounts[0] = amount0;
        amounts[1] = amount1;
        amounts[2] = amount2;
    }

    function curveAmounts5(uint256 amount0, uint256 amount1, uint256 amount2, uint256 amount3, uint256 amount4) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](5);
        amounts[0] = amount0;
        amounts[1] = amount1;
        amounts[2] = amount2;
        amounts[3] = amount3;
        amounts[4] = amount4;
    }

    function curveAmounts8(uint256 amount0, uint256 amount1, uint256 amount2, uint256 amount3, uint256 amount4, uint256 amount5, uint256 amount6, uint256 amount7) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](8);
        amounts[0] = amount0;
        amounts[1] = amount1;
        amounts[2] = amount2;
        amounts[3] = amount3;
        amounts[4] = amount4;
        amounts[5] = amount5;
        amounts[6] = amount6;
        amounts[7] = amount7;
    }

    function curveAmounts9(uint256 amount0, uint256 amount1, uint256 amount2) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](9);
        amounts[0] = amount0;
        amounts[1] = amount1;
        for (uint256 i = 2; i < amounts.length; i++) {
            amounts[i] = amount2;
        }
    }

    function benchWarp(uint256 secondsForward) external returns (bool) {
        vm.warp(block.timestamp + secondsForward);
        return true;
    }

    function benchChainId(uint256 newChainId) external returns (bool) {
        vm.chainId(newChainId);
        return true;
    }

    // bench-cli:helpers begin curve
    function fee_receiver() external pure returns (address) {
        return address(0);
    }

    function admin() external view returns (address) {
        return address(this);
    }

    function views_implementation() external view returns (address) {
        return address(this);
    }

    function get_dy(int128 i, int128 j, uint256 dx, address pool) external view returns (uint256) {
        return benchCurveGetDy(i, j, dx, pool);
    }

    function get_dx(int128 i, int128 j, uint256 dy, address pool) external view returns (uint256) {
        return benchCurveGetDx(i, j, dy, pool);
    }

    function calc_token_amount(uint256[] calldata amounts, bool isDeposit, address pool)
        external
        view
        returns (uint256)
    {
        return benchCurveCalcTokenAmount(amounts, isDeposit, pool);
    }

    function dynamic_fee(int128 i, int128 j, address pool) external view returns (uint256) {
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j, pool);
        (,, uint256[] memory xp) = benchCurveRatesBalancesXp(pool);
        return benchCurvePoolDynamicFee(pool, xp[coinIn], xp[coinOut]);
    }

    function benchCurvePoolDynamicFee(address pool, uint256 xpi, uint256 xpj) internal view returns (uint256) {
        return benchCurveDynamicFeeXp(
            xpi,
            xpj,
            benchCurveUint(pool, "fee()"),
            benchCurveUint(pool, "offpeg_fee_multiplier()")
        );
    }

    function benchCurveGetDy(int128 i, int128 j, uint256 dx, address pool) internal view returns (uint256) {
        require(dx > 0, "curve dx");
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j, pool);
        (uint256[] memory rates,, uint256[] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        return benchCurveGetDyFromState(pool, coinIn, coinOut, dx, rates, xp, amp, benchCurveGetD(xp, amp));
    }

    function benchCurveGetDyFromState(
        address pool,
        uint256 coinIn,
        uint256 coinOut,
        uint256 dx,
        uint256[] memory rates,
        uint256[] memory xp,
        uint256 amp,
        uint256 d
    ) internal view returns (uint256) {
        uint256 x = xp[coinIn] + dx * rates[coinIn] / 1e18;
        uint256 y = benchCurveGetY(coinIn, coinOut, x, xp, amp, d);
        uint256 dy = xp[coinOut] - y - 1;
        dy -= benchCurveGetDyFee(pool, coinIn, coinOut, x, y, dy, xp);
        return dy * 1e18 / rates[coinOut];
    }

    function benchCurveGetDyFee(
        address pool,
        uint256 coinIn,
        uint256 coinOut,
        uint256 x,
        uint256 y,
        uint256 dy,
        uint256[] memory xp
    ) internal view returns (uint256) {
        return benchCurvePoolDynamicFee(pool, (xp[coinIn] + x) / 2, (xp[coinOut] + y) / 2) * dy / 10_000_000_000;
    }

    function benchCurveGetDx(int128 i, int128 j, uint256 dy, address pool) internal view returns (uint256) {
        require(dy > 0, "curve dy");
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j, pool);
        (uint256[] memory rates,, uint256[] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        uint256 d = benchCurveGetD(xp, amp);
        uint256 dyWithFee = dy * rates[coinOut] / 1e18 + 1;
        uint256 feeAmount = benchCurvePoolDynamicFee(pool, xp[coinIn], xp[coinOut]);
        uint256 y = xp[coinOut] - dyWithFee * 10_000_000_000 / (10_000_000_000 - feeAmount);
        uint256 x = benchCurveGetY(coinOut, coinIn, y, xp, amp, d);
        return (x - xp[coinIn]) * 1e18 / rates[coinIn];
    }

    function benchCurveCalcTokenAmount(uint256[] calldata amounts, bool isDeposit, address pool)
        internal
        view
        returns (uint256)
    {
        uint256 nCoins = benchCurveUint(pool, "N_COINS()");
        require(amounts.length >= nCoins && amounts.length <= 8, "curve amounts");
        (uint256[] memory rates, uint256[] memory oldBalances, uint256[] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        uint256 d0 = benchCurveGetD(xp, amp);
        uint256[] memory newBalances = benchCurveCopy(oldBalances);
        for (uint256 i = 0; i < nCoins; i++) {
            if (isDeposit) {
                newBalances[i] += amounts[i];
            } else {
                newBalances[i] -= amounts[i];
            }
            xp[i] = rates[i] * newBalances[i] / 1e18;
        }
        uint256 d1 = benchCurveGetD(xp, amp);
        uint256 totalSupply = benchCurveUint(pool, "totalSupply()");
        if (totalSupply == 0) {
            return d1;
        }
        for (uint256 i = 0; i < nCoins; i++) {
            newBalances[i] = benchCurveCalcFeeAdjustedBalance(
                pool,
                rates[i],
                oldBalances[i],
                newBalances[i],
                d0,
                d1
            );
            xp[i] = rates[i] * newBalances[i] / 1e18;
        }
        uint256 d2 = benchCurveGetD(xp, amp);
        return isDeposit ? (d2 - d0) * totalSupply / d0 : (d0 - d2) * totalSupply / d0;
    }

    function benchCurveCalcFeeAdjustedBalance(
        address pool,
        uint256 rate,
        uint256 oldBalance,
        uint256 newBalance,
        uint256 d0,
        uint256 d1
    ) internal view returns (uint256) {
        uint256 idealBalance = d1 * oldBalance / d0;
        uint256 difference = idealBalance > newBalance ? idealBalance - newBalance : newBalance - idealBalance;
        uint256 xs = rate * (oldBalance + newBalance) / 1e18;
        return newBalance - benchCurveCalcBalanceFee(pool, xs, (d0 + d1) / 2) * difference / 10_000_000_000;
    }

    function benchCurveCalcBalanceFee(address pool, uint256 xs, uint256 ys) internal view returns (uint256) {
        uint256 nCoins = benchCurveUint(pool, "N_COINS()");
        return benchCurveDynamicFeeXp(
            xs,
            ys,
            benchCurveUint(pool, "fee()") * nCoins / (4 * (nCoins - 1)),
            benchCurveUint(pool, "offpeg_fee_multiplier()")
        );
    }

    function benchCurveRatesBalancesXp(address pool)
        internal
        view
        returns (uint256[] memory rates, uint256[] memory balances, uint256[] memory xp)
    {
        uint256[] memory rawRates = benchCurveUintArray(pool, "stored_rates()");
        uint256[] memory rawBalances = benchCurveUintArray(pool, "get_balances()");
        uint256 nCoins = benchCurveUint(pool, "N_COINS()");
        require(rawRates.length >= nCoins && rawBalances.length >= nCoins, "curve arrays");
        rates = new uint256[](nCoins);
        balances = new uint256[](nCoins);
        xp = new uint256[](nCoins);
        for (uint256 i = 0; i < nCoins; i++) {
            rates[i] = rawRates[i];
            balances[i] = rawBalances[i];
            xp[i] = rawRates[i] * rawBalances[i] / 1e18;
        }
    }

    function benchCurveCoinPair(int128 i, int128 j, address pool) internal view returns (uint256 coinIn, uint256 coinOut) {
        require(i >= 0 && j >= 0 && i != j, "curve coin");
        coinIn = uint256(int256(i));
        coinOut = uint256(int256(j));
        uint256 nCoins = benchCurveUint(pool, "N_COINS()");
        require(coinIn < nCoins && coinOut < nCoins, "curve coin");
    }

    function benchCurveUint(address pool, string memory signature) internal view returns (uint256 value) {
        (bool ok, bytes memory raw) = pool.staticcall(abi.encodeWithSignature(signature));
        require(ok, "curve view");
        value = abi.decode(raw, (uint256));
    }

    function benchCurveUintArray(address pool, string memory signature) internal view returns (uint256[] memory values) {
        (bool ok, bytes memory raw) = pool.staticcall(abi.encodeWithSignature(signature));
        require(ok, "curve view");
        values = abi.decode(raw, (uint256[]));
    }

    function benchCurveDynamicFeeXp(uint256 xpi, uint256 xpj, uint256 baseFee, uint256 feeMultiplier)
        internal
        pure
        returns (uint256)
    {
        if (feeMultiplier <= 10_000_000_000) {
            return baseFee;
        }
        uint256 xps2 = (xpi + xpj) * (xpi + xpj);
        return feeMultiplier * baseFee / (((feeMultiplier - 10_000_000_000) * 4 * xpi * xpj / xps2) + 10_000_000_000);
    }

    function benchCurveCopy(uint256[] memory source) internal pure returns (uint256[] memory result) {
        result = new uint256[](source.length);
        for (uint256 i = 0; i < source.length; i++) {
            result[i] = source[i];
        }
    }

    function benchCurveGetD(uint256[] memory xp, uint256 amp) internal pure returns (uint256) {
        uint256 nCoins = xp.length;
        uint256 sum;
        for (uint256 i = 0; i < nCoins; i++) {
            sum += xp[i];
        }
        if (sum == 0) {
            return 0;
        }
        uint256 d = sum;
        uint256 ann = amp * nCoins;
        for (uint256 i = 0; i < 255; i++) {
            uint256 dP = d;
            for (uint256 j = 0; j < nCoins; j++) {
                dP = dP * d / xp[j];
            }
            dP /= nCoins ** nCoins;
            uint256 previousD = d;
            d = (ann * sum / 100 + dP * nCoins) * d / ((ann - 100) * d / 100 + (nCoins + 1) * dP);
            if (d > previousD) {
                if (d - previousD <= 1) return d;
            } else if (previousD - d <= 1) {
                return d;
            }
        }
        revert("curve D");
    }

    function benchCurveGetY(uint256 i, uint256 j, uint256 x, uint256[] memory xp, uint256 amp, uint256 d)
        internal
        pure
        returns (uint256)
    {
        uint256 nCoins = xp.length;
        require(i != j && i < nCoins && j < nCoins, "curve y coin");
        uint256 c = d;
        uint256 s;
        for (uint256 idx = 0; idx < nCoins; idx++) {
            if (idx == j) {
                continue;
            }
            uint256 currentX = idx == i ? x : xp[idx];
            s += currentX;
            c = c * d / (currentX * nCoins);
        }
        c = c * d * 100 / (amp * nCoins * nCoins);
        uint256 b = s + d * 100 / (amp * nCoins);
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
        revert("curve y");
    }
    // bench-cli:helpers end curve

    // bench-cli:helpers begin yearn
    function protocol_fee_config() external view returns (uint16, address) {
        return (protocolFeeBps, protocolFeeRecipient);
    }

    function benchYearnSetProtocolFee(uint16 feeBps, address recipient) external returns (bool) {
        protocolFeeBps = feeBps;
        protocolFeeRecipient = recipient;
        return true;
    }
    // bench-cli:helpers end yearn

    // bench-cli:helpers begin uniswap
    function benchUniswapInit(address target, bool feeOn) external returns (bool) {
        BenchERC20 token0 = new BenchERC20();
        BenchERC20 token1 = new BenchERC20();
        BenchUniswapFlashCallee flashCallee = new BenchUniswapFlashCallee();
        BenchUniswapReentrantCallee reentrantCallee = new BenchUniswapReentrantCallee();
        pairDeps[target] = PairDeps(token0, token1, flashCallee, reentrantCallee);
        feeTo = feeOn ? address(0xFEE) : address(0);
        (bool ok,) = target.call(
            abi.encodeWithSignature("initialize(address,address)", address(token0), address(token1))
        );
        require(ok, "pair init");
        return true;
    }

    function benchUniswapInitNoReturn(address target, bool feeOn) external returns (bool) {
        BenchERC20NoReturn token0 = new BenchERC20NoReturn();
        BenchERC20NoReturn token1 = new BenchERC20NoReturn();
        noReturnPairDeps[target] = NoReturnPairDeps(token0, token1);
        feeTo = feeOn ? address(0xFEE) : address(0);
        (bool ok,) = target.call(
            abi.encodeWithSignature("initialize(address,address)", address(token0), address(token1))
        );
        require(ok, "pair init");
        return true;
    }

    function benchUniswapToken0(address target) public view returns (address) {
        return address(pairDeps[target].token0);
    }

    function benchUniswapToken1(address target) public view returns (address) {
        return address(pairDeps[target].token1);
    }

    function benchUniswapTokenBalances(address target, address owner) external view returns (uint256, uint256) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        return (deps.token0.balanceOf(owner), deps.token1.balanceOf(owner));
    }

    function benchUniswapToken0GetterId(address target) external view returns (uint256) {
        (bool ok, bytes memory rawToken) = target.staticcall(abi.encodeWithSignature("token0()"));
        require(ok, "pair token0");
        return _logAddressId(target, abi.decode(rawToken, (address)));
    }

    function benchUniswapToken1GetterId(address target) external view returns (uint256) {
        (bool ok, bytes memory rawToken) = target.staticcall(abi.encodeWithSignature("token1()"));
        require(ok, "pair token1");
        return _logAddressId(target, abi.decode(rawToken, (address)));
    }

    function benchUniswapDomainSeparatorMatches(address target) external view returns (bool) {
        (bool ok, bytes memory rawDomain) = target.staticcall(abi.encodeWithSignature("DOMAIN_SEPARATOR()"));
        require(ok, "pair domain");
        bytes32 expected = keccak256(
            abi.encode(
                keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
                keccak256(bytes("Uniswap V2")),
                keccak256(bytes("1")),
                block.chainid,
                target
            )
        );
        return abi.decode(rawDomain, (bytes32)) == expected;
    }

    function benchUniswapFlashCallee(address target) public view returns (address) {
        return address(pairDeps[target].flashCallee);
    }

    function benchUniswapReentrantCallee(address target) public view returns (address) {
        return address(pairDeps[target].reentrantCallee);
    }

    function benchUniswapSeed(address target, uint256 amount0, uint256 amount1) external returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(target, amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(target, amount1);
        }
        return true;
    }

    function benchUniswapSetTransferReturnValue(address target, bool token0Value, bool token1Value)
        external
        returns (bool)
    {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        deps.token0.setReturnValue(true, token0Value, true);
        deps.token1.setReturnValue(true, token1Value, true);
        return true;
    }

    function benchUniswapSeedNoReturn(address target, uint256 amount0, uint256 amount1) external returns (bool) {
        NoReturnPairDeps storage deps = noReturnPairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(target, amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(target, amount1);
        }
        return true;
    }

    function benchUniswapFundFlashCallee(address target, uint256 amount0, uint256 amount1)
        external
        returns (bool)
    {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.flashCallee) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(address(deps.flashCallee), amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(address(deps.flashCallee), amount1);
        }
        return true;
    }

    function benchUniswapFundReentrantCallee(address target, uint256 amount0, uint256 amount1)
        external
        returns (bool)
    {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.reentrantCallee) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(address(deps.reentrantCallee), amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(address(deps.reentrantCallee), amount1);
        }
        return true;
    }

    function benchUniswapFlashData(address target, uint256 repay0, uint256 repay1, uint256 paddingLength)
        public
        view
        returns (bytes memory data)
    {
        bytes memory padding = new bytes(paddingLength);
        for (uint256 i = 0; i < padding.length; i++) {
            padding[i] = bytes1(uint8(uint256(keccak256(abi.encode(target, repay0, repay1, i)))));
        }
        return bytes.concat(
            abi.encode(benchUniswapToken0(target), benchUniswapToken1(target), repay0, repay1),
            padding
        );
    }

    function benchUniswapStageBurn(address target, uint256 liquidity) external returns (bool) {
        (bool ok,) = target.call(abi.encodeWithSignature("transfer(address,uint256)", target, liquidity));
        require(ok, "stage lp");
        return true;
    }

    function benchUniswapSetFeeTo(address newFeeTo) external returns (bool) {
        feeTo = newFeeTo;
        return true;
    }

    function benchUniswapFactoryState(address target) public returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        BenchUniswapCreate2Factory factory = _uniswapCreate2Factory();
        require(factory.feeToSetter() == address(this), "factory setter");
        uint256 pairCount = factory.allPairsLength();
        require(pairCount > 0, "factory length");
        bool foundPair = false;
        for (uint256 i = 0; i < pairCount; i++) {
            if (factory.allPairs(i) == target) {
                foundPair = true;
            }
        }
        require(foundPair, "factory allPairs");
        require(factory.getPair(address(deps.token0), address(deps.token1)) == target, "factory pair");
        require(factory.getPair(address(deps.token1), address(deps.token0)) == target, "factory pair reverse");
        return true;
    }

    function benchUniswapFactorySetFeeTo(address target, address newFeeTo) external returns (bool) {
        benchUniswapFactoryState(target);
        BenchUniswapCreate2Factory factory = _uniswapCreate2Factory();
        factory.setFeeTo(newFeeTo);
        require(factory.feeTo() == newFeeTo, "factory feeTo");
        return true;
    }

    function benchUniswapFactorySetFeeToFrom(address target, address sender, address newFeeTo)
        external
        returns (bool)
    {
        benchUniswapFactoryState(target);
        BenchUniswapCreate2Factory factory = _uniswapCreate2Factory();
        vm.prank(sender);
        factory.setFeeTo(newFeeTo);
        return true;
    }

    function benchUniswapFactorySetFeeToSetter(address target, address newFeeToSetter) external returns (bool) {
        benchUniswapFactoryState(target);
        BenchUniswapCreate2Factory factory = _uniswapCreate2Factory();
        factory.setFeeToSetter(newFeeToSetter);
        require(factory.feeToSetter() == newFeeToSetter, "factory feeToSetter");
        vm.prank(newFeeToSetter);
        factory.setFeeToSetter(address(this));
        require(factory.feeToSetter() == address(this), "factory feeToSetter reset");
        return true;
    }

    function benchUniswapFactorySetFeeToSetterFrom(address target, address sender, address newFeeToSetter)
        external
        returns (bool)
    {
        benchUniswapFactoryState(target);
        BenchUniswapCreate2Factory factory = _uniswapCreate2Factory();
        vm.prank(sender);
        factory.setFeeToSetter(newFeeToSetter);
        return true;
    }

    function benchUniswapFactoryDuplicatePair(address target) external returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        _uniswapCreate2Factory().deployPair(hex"00", bytes32(0), address(deps.token0), address(deps.token1));
        return true;
    }

    function benchUniswapFactoryIdenticalPair(address target) external returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        _uniswapCreate2Factory().deployPair(hex"00", bytes32(0), address(deps.token0), address(deps.token0));
        return true;
    }

    function benchUniswapFactoryZeroAddressPair(address target) external returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token1) != address(0), "pair deps");
        _uniswapCreate2Factory().deployPair(hex"00", bytes32(0), address(0), address(deps.token1));
        return true;
    }

    function _uniswapCreate2Factory() internal returns (BenchUniswapCreate2Factory) {
        if (address(uniswapCreate2Factory) == address(0)) {
            uniswapCreate2Factory = new BenchUniswapCreate2Factory();
        }
        return uniswapCreate2Factory;
    }

    function benchUniswapPermitOwner() public returns (address) {
        return vm.addr(UNISWAP_PERMIT_KEY);
    }
    // bench-cli:helpers end uniswap

    function _benchDomainSeparator(address target) internal returns (bytes32) {
        (bool ok, bytes memory rawDomain) = target.call(abi.encodeWithSignature("DOMAIN_SEPARATOR()"));
        require(ok, "permit domain");
        return abi.decode(rawDomain, (bytes32));
    }

    function _benchNonce(address target, address owner) internal returns (uint256) {
        (bool ok, bytes memory rawNonce) = target.call(abi.encodeWithSignature("nonces(address)", owner));
        require(ok, "permit nonce");
        return abi.decode(rawNonce, (uint256));
    }

    function _benchPermitDigest(
        address target,
        bytes32 typeHash,
        address owner,
        address spender,
        uint256 value,
        uint256 deadline
    ) internal returns (bytes32) {
        return keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                _benchDomainSeparator(target),
                keccak256(abi.encode(typeHash, owner, spender, value, _benchNonce(target, owner), deadline))
            )
        );
    }

    // bench-cli:helpers begin uniswap
    function benchUniswapPermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchUniswapPermitOwner();
        bytes32 digest = _benchPermitDigest(target, UNISWAP_PERMIT_TYPE_HASH, owner, spender, value, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(UNISWAP_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }
    // bench-cli:helpers end uniswap

    // bench-cli:helpers begin curve
    function benchCurveInit(address target, uint256 amp, uint256 swapFee, uint256 adminFee)
        external
        returns (bool)
    {
        amp;
        swapFee;
        adminFee;
        require(address(curveDeps[target].coin0) != address(0), "curve deps");
        return true;
    }

    function benchCurveStageReceived(address target, uint256 coinIndex, uint256 amount) external returns (bool) {
        CurveDeps storage deps = curveDeps[target];
        require(address(deps.coin0) != address(0), "curve deps");
        if (coinIndex == 0) {
            require(deps.coin0.transfer(target, amount), "curve transfer0");
        } else if (coinIndex == 1) {
            require(deps.coin1.transfer(target, amount), "curve transfer1");
        } else if (coinIndex == 2 && address(deps.coin2) != address(0)) {
            require(deps.coin2.transfer(target, amount), "curve transfer2");
        } else if (coinIndex == 3 && address(deps.coin3) != address(0)) {
            require(deps.coin3.transfer(target, amount), "curve transfer3");
        } else if (coinIndex == 4 && address(deps.coin4) != address(0)) {
            require(deps.coin4.transfer(target, amount), "curve transfer4");
        } else if (coinIndex == 5 && address(deps.coin5) != address(0)) {
            require(deps.coin5.transfer(target, amount), "curve transfer5");
        } else if (coinIndex == 6 && address(deps.coin6) != address(0)) {
            require(deps.coin6.transfer(target, amount), "curve transfer6");
        } else if (coinIndex == 7 && address(deps.coin7) != address(0)) {
            require(deps.coin7.transfer(target, amount), "curve transfer7");
        } else {
            revert("curve coin");
        }
        return true;
    }

    function benchCurveRebaseCoin(address target, uint256 coinIndex, uint256 amount) external returns (bool) {
        CurveDeps storage deps = curveDeps[target];
        require(address(deps.coin0) != address(0), "curve deps");
        if (coinIndex == 0) {
            require(deps.coin0.mint(target, amount), "curve mint0");
        } else if (coinIndex == 1) {
            require(deps.coin1.mint(target, amount), "curve mint1");
        } else if (coinIndex == 2 && address(deps.coin2) != address(0)) {
            require(deps.coin2.mint(target, amount), "curve mint2");
        } else if (coinIndex == 3 && address(deps.coin3) != address(0)) {
            require(deps.coin3.mint(target, amount), "curve mint3");
        } else if (coinIndex == 4 && address(deps.coin4) != address(0)) {
            require(deps.coin4.mint(target, amount), "curve mint4");
        } else if (coinIndex == 5 && address(deps.coin5) != address(0)) {
            require(deps.coin5.mint(target, amount), "curve mint5");
        } else if (coinIndex == 6 && address(deps.coin6) != address(0)) {
            require(deps.coin6.mint(target, amount), "curve mint6");
        } else if (coinIndex == 7 && address(deps.coin7) != address(0)) {
            require(deps.coin7.mint(target, amount), "curve mint7");
        } else {
            revert("curve coin");
        }
        return true;
    }

    function benchCurveSetReturnData(address target, bool enabled) external returns (bool) {
        CurveDeps storage deps = curveDeps[target];
        require(address(deps.coin0) != address(0), "curve deps");
        deps.coin0.setReturnData(enabled);
        deps.coin1.setReturnData(enabled);
        if (address(deps.coin2) != address(0)) {
            deps.coin2.setReturnData(enabled);
        }
        if (address(deps.coin3) != address(0)) {
            deps.coin3.setReturnData(enabled);
            deps.coin4.setReturnData(enabled);
            deps.coin5.setReturnData(enabled);
            deps.coin6.setReturnData(enabled);
            deps.coin7.setReturnData(enabled);
        }
        return true;
    }

    function benchCurvePermitOwner() public returns (address) {
        return vm.addr(CURVE_PERMIT_KEY);
    }

    function benchCurve1271PermitOwner() public returns (address) {
        if (address(curve1271Owner) == address(0)) {
            curve1271Owner = new BenchERC1271Wallet();
        }
        return address(curve1271Owner);
    }

    function benchCurvePermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchCurvePermitOwner();
        bytes32 digest = _benchPermitDigest(target, CURVE_PERMIT_TYPE_HASH, owner, spender, value, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(CURVE_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }

    function benchCurve1271PermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchCurve1271PermitOwner();
        bytes32 digest = _benchPermitDigest(target, CURVE_PERMIT_TYPE_HASH, owner, spender, value, deadline);
        curve1271Owner.setValidDigest(digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            uint8(27),
            bytes32(0),
            bytes32(0)
        );
    }
    // bench-cli:helpers end curve

    // bench-cli:helpers begin yearn
    function benchYearnInit(address target, uint256 limit, uint256 unlockTime, uint256 feeBps)
        external
        returns (bool)
    {
        _benchYearnPrepare(target);
        BenchERC20 asset = yearnDeps[target].asset;
        (bool ok,) = target.call(
            abi.encodeWithSignature(
                "initialize(address,string,string,address,uint256)",
                address(asset),
                "Yearn V3 Vault",
                "yvV3",
                address(this),
                unlockTime
            )
        );
        require(ok, "yearn init");
        (ok,) = target.call(abi.encodeWithSignature("set_role(address,uint256)", address(this), uint256(16_383)));
        require(ok, "yearn roles");
        (ok,) = target.call(abi.encodeWithSignature("set_deposit_limit(uint256,bool)", limit, true));
        require(ok, "yearn limit");
        feeBps;
        asset.mint(address(this), 1e30);
        asset.approve(target, type(uint256).max);
        return true;
    }

    function benchYearnPrepare(address target) external returns (bool) {
        _benchYearnPrepare(target);
        return true;
    }

    function _benchYearnPrepare(address target) internal {
        if (address(yearnDeps[target].asset) == address(0)) {
            BenchERC20 asset = new BenchERC20();
            BenchYearnStrategy strategy = new BenchYearnStrategy(asset);
            BenchYearnStrategy strategy2 = new BenchYearnStrategy(asset);
            BenchYearnStrategy strategy3 = new BenchYearnStrategy(asset);
            BenchYearnAccountant accountant = new BenchYearnAccountant(asset);
            BenchYearnMutatingAccountant mutatingAccountant = new BenchYearnMutatingAccountant(asset);
            BenchYearnReentrantAccountant reentrantAccountant = new BenchYearnReentrantAccountant(asset);
            BenchYearnDepositLimitModule depositLimitModule = new BenchYearnDepositLimitModule();
            BenchYearnWithdrawLimitModule withdrawLimitModule = new BenchYearnWithdrawLimitModule();
            yearnDeps[target] = YearnDeps(
                asset,
                strategy,
                strategy2,
                strategy3,
                accountant,
                mutatingAccountant,
                reentrantAccountant,
                depositLimitModule,
                withdrawLimitModule
            );
        }
    }

    function benchYearnAsset(address target) public view returns (address) {
        return address(yearnDeps[target].asset);
    }

    function benchYearnStrategy(address target) public view returns (address) {
        return address(yearnDeps[target].strategy);
    }

    function benchYearnStrategy2(address target) public view returns (address) {
        return address(yearnDeps[target].strategy2);
    }

    function benchYearnStrategy3(address target) public view returns (address) {
        return address(yearnDeps[target].strategy3);
    }

    function benchYearnAccountant(address target) public view returns (address) {
        return address(yearnDeps[target].accountant);
    }

    function benchYearnReentrantAccountant(address target) public view returns (address) {
        return address(yearnDeps[target].reentrantAccountant);
    }

    function benchYearnDepositLimitModule(address target) public view returns (address) {
        return address(yearnDeps[target].depositLimitModule);
    }

    function benchYearnWithdrawLimitModule(address target) public view returns (address) {
        return address(yearnDeps[target].withdrawLimitModule);
    }

    function benchYearnAssetId(address target) external view returns (uint256) {
        return _benchYearnAddressId(target, _benchYearnAddress(target, "asset()"));
    }

    function benchYearnAccountantId(address target) external view returns (uint256) {
        return _benchYearnAddressId(target, _benchYearnAddress(target, "accountant()"));
    }

    function benchYearnDepositLimitModuleId(address target) external view returns (uint256) {
        return _benchYearnAddressId(target, _benchYearnAddress(target, "deposit_limit_module()"));
    }

    function benchYearnWithdrawLimitModuleId(address target) external view returns (uint256) {
        return _benchYearnAddressId(target, _benchYearnAddress(target, "withdraw_limit_module()"));
    }

    function benchYearnDefaultQueueIds(address target) external view returns (bytes32) {
        (bool ok, bytes memory raw) = target.staticcall(abi.encodeWithSignature("get_default_queue()"));
        require(ok, "yearn queue observer");
        address[] memory queue = abi.decode(raw, (address[]));
        uint256[] memory ids = new uint256[](queue.length);
        for (uint256 i = 0; i < queue.length; i++) {
            ids[i] = _benchYearnAddressId(target, queue[i]);
        }
        return keccak256(abi.encode(ids));
    }

    function _benchYearnAddress(address target, string memory signature) internal view returns (address value) {
        (bool ok, bytes memory raw) = target.staticcall(abi.encodeWithSignature(signature));
        require(ok, "yearn address observer");
        value = abi.decode(raw, (address));
    }

    function _benchYearnAddressId(address target, address value) internal view returns (uint256) {
        YearnDeps storage deps = yearnDeps[target];
        if (value == address(0)) return 0;
        if (value == address(deps.asset)) return 1;
        if (value == address(deps.strategy)) return 2;
        if (value == address(deps.strategy2)) return 3;
        if (value == address(deps.strategy3)) return 4;
        if (value == address(deps.accountant)) return 5;
        if (value == address(deps.reentrantAccountant)) return 6;
        if (value == address(deps.depositLimitModule)) return 7;
        if (value == address(deps.withdrawLimitModule)) return 8;
        if (value == address(deps.mutatingAccountant)) return 9;
        return uint256(uint160(value));
    }

    function benchYearnPermitOwner() public returns (address) {
        return vm.addr(YEARN_PERMIT_KEY);
    }

    function benchYearnPermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchYearnPermitOwner();
        bytes32 digest = _benchPermitDigest(target, YEARN_PERMIT_TYPE_HASH, owner, spender, value, deadline);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(YEARN_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }

    function benchYearnSetReport(address target, uint256 gain, uint256 loss) external returns (bool) {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        yearnDeps[target].strategy.setReport(gain, loss);
        return true;
    }

    function benchYearnSetReportFor(address target, address strategy, uint256 gain, uint256 loss)
        external
        returns (bool)
    {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy)
                || strategy == address(yearnDeps[target].strategy2)
                || strategy == address(yearnDeps[target].strategy3),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setReport(gain, loss);
        return true;
    }

    function benchYearnSetStrategyMaxDeposit(address target, address strategy, uint256 limit) external returns (bool) {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy)
                || strategy == address(yearnDeps[target].strategy2)
                || strategy == address(yearnDeps[target].strategy3),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setMaxDepositLimit(limit);
        return true;
    }

    function benchYearnSetStrategyMaxRedeem(address target, address strategy, uint256 limit) external returns (bool) {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy)
                || strategy == address(yearnDeps[target].strategy2)
                || strategy == address(yearnDeps[target].strategy3),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setMaxRedeemLimit(limit);
        return true;
    }

    function benchYearnSetStrategyRedeemReturnBps(address target, address strategy, uint256 bps)
        external
        returns (bool)
    {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy)
                || strategy == address(yearnDeps[target].strategy2)
                || strategy == address(yearnDeps[target].strategy3),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setRedeemReturnBps(bps);
        return true;
    }

    function benchYearnSetStrategyShareMintBps(address target, address strategy, uint256 bps)
        external
        returns (bool)
    {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy)
                || strategy == address(yearnDeps[target].strategy2)
                || strategy == address(yearnDeps[target].strategy3),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setShareMintBps(bps);
        return true;
    }

    function benchYearnBurnStrategyAssets(address target, uint256 amount) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.strategy) != address(0), "yearn deps");
        deps.asset.burn(address(deps.strategy), amount);
        return true;
    }

    function benchYearnAirdrop(address target, uint256 amount) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.asset) != address(0), "yearn deps");
        deps.asset.mint(target, amount);
        return true;
    }

    function benchYearnBurnVaultAssets(address target, uint256 amount) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.asset) != address(0), "yearn deps");
        deps.asset.burn(target, amount);
        return true;
    }

    function benchYearnSetAssetReturnData(
        address target,
        bool approveEnabled,
        bool transferEnabled,
        bool transferFromEnabled
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.asset) != address(0), "yearn deps");
        deps.asset.setReturnData(approveEnabled, transferEnabled, transferFromEnabled);
        return true;
    }

    function benchYearnSetAssetReturnValue(
        address target,
        bool approveValue,
        bool transferValue,
        bool transferFromValue
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.asset) != address(0), "yearn deps");
        deps.asset.setReturnValue(approveValue, transferValue, transferFromValue);
        return true;
    }

    function benchYearnConfigureAccountant(address target, uint256 fees, uint256 refunds) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.accountant) != address(0), "yearn deps");
        deps.accountant.setReport(target, fees, refunds);
        (bool ok,) = target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.accountant)));
        require(ok, "yearn accountant");
        return true;
    }

    function benchYearnConfigureClippedRefundAccountant(
        address target,
        uint256 fees,
        uint256 refunds,
        uint256 mintedRefunds,
        uint256 approvedRefunds
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.accountant) != address(0), "yearn deps");
        deps.accountant.setClippedRefundReport(target, fees, refunds, mintedRefunds, approvedRefunds);
        (bool ok,) = target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.accountant)));
        require(ok, "yearn accountant");
        return true;
    }

    function benchYearnConfigureMutatingAccountant(
        address target,
        uint256 fees,
        uint256 refunds,
        uint256 preMintedRefunds,
        uint256 preApprovedRefunds,
        uint256 reportMintedRefunds,
        uint256 reportApprovedRefunds
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.mutatingAccountant) != address(0), "yearn deps");
        deps.mutatingAccountant.setReport(
            target,
            fees,
            refunds,
            preMintedRefunds,
            preApprovedRefunds,
            reportMintedRefunds,
            reportApprovedRefunds
        );
        (bool ok,) = target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.mutatingAccountant)));
        require(ok, "yearn mutating accountant");
        return true;
    }

    function benchYearnConfigureReentrantAccountant(address target) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.reentrantAccountant) != address(0), "yearn deps");
        deps.reentrantAccountant.prepare(target);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.reentrantAccountant)));
        require(ok, "yearn reentrant accountant");
        return true;
    }

    function benchYearnSetDepositLimitModule(address target, uint256 limit) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.depositLimitModule) != address(0), "yearn deps");
        deps.depositLimitModule.setLimit(limit);
        deps.depositLimitModule.setSpecialReceiver(address(0), 0);
        deps.depositLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_deposit_limit_module(address,bool)", address(deps.depositLimitModule), true));
        require(ok, "yearn deposit module");
        return true;
    }

    function benchYearnSetReceiverDepositLimitModule(
        address target,
        uint256 defaultLimit,
        address receiver,
        uint256 receiverLimit
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.depositLimitModule) != address(0), "yearn deps");
        deps.depositLimitModule.setLimit(defaultLimit);
        deps.depositLimitModule.setSpecialReceiver(receiver, receiverLimit);
        deps.depositLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_deposit_limit_module(address,bool)", address(deps.depositLimitModule), true));
        require(ok, "yearn deposit module");
        return true;
    }

    function benchYearnSetRevertingDepositLimitModule(address target) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.depositLimitModule) != address(0), "yearn deps");
        deps.depositLimitModule.setShouldRevert(true);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_deposit_limit_module(address,bool)", address(deps.depositLimitModule), true));
        require(ok, "yearn deposit module");
        return true;
    }

    function benchYearnSetWithdrawLimitModule(address target, uint256 limit) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setLimit(limit);
        deps.withdrawLimitModule.clearSpecialCases();
        deps.withdrawLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetOwnerWithdrawLimitModule(
        address target,
        uint256 defaultLimit,
        address owner,
        uint256 ownerLimit
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setLimit(defaultLimit);
        deps.withdrawLimitModule.clearSpecialCases();
        deps.withdrawLimitModule.setSpecialOwner(owner, ownerLimit);
        deps.withdrawLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetMaxLossWithdrawLimitModule(
        address target,
        uint256 defaultLimit,
        uint256 maxLoss,
        uint256 maxLossLimit
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setLimit(defaultLimit);
        deps.withdrawLimitModule.clearSpecialCases();
        deps.withdrawLimitModule.setSpecialMaxLoss(maxLoss, maxLossLimit);
        deps.withdrawLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetStrategyQueueWithdrawLimitModule(
        address target,
        uint256 defaultLimit,
        uint256 queueLimit
    ) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setLimit(defaultLimit);
        deps.withdrawLimitModule.clearSpecialCases();
        deps.withdrawLimitModule.setSpecialStrategiesHash(keccak256(abi.encode(benchYearnTripleQueue(target))), queueLimit);
        deps.withdrawLimitModule.setShouldRevert(false);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetRevertingWithdrawLimitModule(address target) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setShouldRevert(true);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetDefaultQueueCalldata(address target, bool reverse) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        if (reverse) {
            queue[0] = address(yearnDeps[target].strategy2);
            queue[1] = address(yearnDeps[target].strategy);
        } else {
            queue[0] = address(yearnDeps[target].strategy);
            queue[1] = address(yearnDeps[target].strategy2);
        }
        return abi.encodeWithSignature("set_default_queue(address[])", queue);
    }

    function benchYearnSetTripleDefaultQueueCalldata(address target) public view returns (bytes memory) {
        return abi.encodeWithSignature("set_default_queue(address[])", benchYearnTripleQueue(target));
    }

    function benchYearnSetDuplicateDefaultQueueCalldata(address target) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        queue[0] = address(yearnDeps[target].strategy);
        queue[1] = address(yearnDeps[target].strategy);
        return abi.encodeWithSignature("set_default_queue(address[])", queue);
    }

    function benchYearnSetFullDuplicateDefaultQueueCalldata(address target) public view returns (bytes memory) {
        address[] memory queue = new address[](10);
        for (uint256 i = 0; i < queue.length; i++) {
            queue[i] = address(yearnDeps[target].strategy);
        }
        return abi.encodeWithSignature("set_default_queue(address[])", queue);
    }

    function benchYearnWithdrawQueueCalldata(
        address target,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss,
        bool reverse
    ) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        if (reverse) {
            queue[0] = address(yearnDeps[target].strategy2);
            queue[1] = address(yearnDeps[target].strategy);
        } else {
            queue[0] = address(yearnDeps[target].strategy);
            queue[1] = address(yearnDeps[target].strategy2);
        }
        return abi.encodeWithSignature(
            "withdraw(uint256,address,address,uint256,address[])", assets, receiver, owner, maxLoss, queue
        );
    }

    function benchYearnRedeemQueueCalldata(
        address target,
        uint256 shares,
        address receiver,
        address owner,
        uint256 maxLoss,
        bool reverse
    ) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        if (reverse) {
            queue[0] = address(yearnDeps[target].strategy2);
            queue[1] = address(yearnDeps[target].strategy);
        } else {
            queue[0] = address(yearnDeps[target].strategy);
            queue[1] = address(yearnDeps[target].strategy2);
        }
        return abi.encodeWithSignature(
            "redeem(uint256,address,address,uint256,address[])", shares, receiver, owner, maxLoss, queue
        );
    }

    function benchYearnRedeemTripleQueueCalldata(
        address target,
        uint256 shares,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "redeem(uint256,address,address,uint256,address[])",
            shares,
            receiver,
            owner,
            maxLoss,
            benchYearnTripleQueue(target)
        );
    }

    function benchYearnWithdrawTripleQueueCalldata(
        address target,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "withdraw(uint256,address,address,uint256,address[])",
            assets,
            receiver,
            owner,
            maxLoss,
            benchYearnTripleQueue(target)
        );
    }

    function benchYearnWithdrawLongQueueCalldata(
        address target,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "withdraw(uint256,address,address,uint256,address[])",
            assets,
            receiver,
            owner,
            maxLoss,
            benchYearnLongQueue(target)
        );
    }

    function benchYearnRedeemLongQueueCalldata(
        address target,
        uint256 shares,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "redeem(uint256,address,address,uint256,address[])",
            shares,
            receiver,
            owner,
            maxLoss,
            benchYearnLongQueue(target)
        );
    }

    function benchYearnMaxWithdrawLongQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxWithdraw(address,uint256,address[])", owner, maxLoss, benchYearnLongQueue(target));
    }

    function benchYearnMaxWithdrawTripleQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxWithdraw(address,uint256,address[])", owner, maxLoss, benchYearnTripleQueue(target));
    }

    function benchYearnMaxRedeemLongQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxRedeem(address,uint256,address[])", owner, maxLoss, benchYearnLongQueue(target));
    }

    function benchYearnMaxRedeemTripleQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxRedeem(address,uint256,address[])", owner, maxLoss, benchYearnTripleQueue(target));
    }

    function benchYearnLongQueue(address target) public view returns (address[] memory queue) {
        queue = new address[](11);
        queue[0] = address(yearnDeps[target].strategy);
        queue[1] = address(yearnDeps[target].strategy2);
        queue[2] = address(yearnDeps[target].strategy3);
        for (uint256 i = 3; i < queue.length; i++) {
            queue[i] = address(yearnDeps[target].strategy);
        }
    }

    function benchYearnTripleQueue(address target) public view returns (address[] memory queue) {
        queue = new address[](3);
        queue[0] = address(yearnDeps[target].strategy3);
        queue[1] = address(yearnDeps[target].strategy);
        queue[2] = address(yearnDeps[target].strategy2);
    }
    // bench-cli:helpers end yearn

    function _deploy(bytes memory code) internal returns (address target) {
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0), "deploy failed");
    }

    function _deployMinimalProxy(address implementation) internal returns (address target) {
        bytes memory code = abi.encodePacked(
            hex"3d602d80600a3d3981f3363d3d373d3d3d363d73",
            implementation,
            hex"5af43d82803e903d91602b57fd5bf3"
        );
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0) && target.code.length != 0, "proxy deploy failed");
    }

    function _run(address target, bytes memory data, uint256 value, address sender)
        internal
        returns (bool ok, bytes32 retHash, uint256 gasUsed)
    {
        bytes memory ret;
        uint256 startGas = gasleft();
        if (sender == address(this)) {
            (ok, ret) = target.call{value: value}(data);
        } else {
            vm.prank(sender);
            (ok, ret) = target.call{value: value}(data);
        }
        gasUsed = startGas - gasleft();
        retHash = keccak256(ret);
    }

    function _runWithLogs(address target, address destination, bytes memory data, uint256 value, address sender)
        internal
        returns (bool ok, bytes32 retHash, bytes32 logHash, uint256 gasUsed)
    {
        vm.recordLogs();
        (ok, retHash, gasUsed) = _run(destination, data, value, sender);
        // Reverted transactions do not commit logs, even if Foundry recorded subcall logs.
        logHash = ok ? _normalizedLogHash(target, vm.getRecordedLogs()) : bytes32(0);
    }

    function _observe(address target, bytes memory data) internal returns (bytes32) {
        (bool ok, bytes memory ret) = target.call(data);
        return keccak256(abi.encode(ok, ret));
    }

    function _normalizedLogHash(address target, Vm.Log[] memory entries) internal view returns (bytes32 hash) {
        hash = bytes32(0);
        for (uint256 i = 0; i < entries.length; i++) {
            bytes32[] memory topics = new bytes32[](entries[i].topics.length);
            for (uint256 topicIndex = 0; topicIndex < entries[i].topics.length; topicIndex++) {
                topics[topicIndex] = _normalizeLogWord(target, entries[i].topics[topicIndex]);
            }
            hash = keccak256(
                abi.encode(
                    hash,
                    _normalizeLogEmitter(target, entries[i].emitter),
                    topics,
                    _normalizeLogData(target, entries[i].data)
                )
            );
        }
    }

    function _normalizeLogData(address target, bytes memory data) internal view returns (bytes memory normalized) {
        normalized = data;
        for (uint256 offset = 0; offset + 32 <= normalized.length; offset += 32) {
            bytes32 word;
            assembly {
                word := mload(add(add(normalized, 0x20), offset))
            }
            bytes32 normalizedWord = _normalizeLogWord(target, word);
            if (normalizedWord != word) {
                assembly {
                    mstore(add(add(normalized, 0x20), offset), normalizedWord)
                }
            }
        }
    }

    function _normalizeLogEmitter(address target, address emitter) internal view returns (bytes32) {
        uint256 id = _logAddressId(target, emitter);
        if (id != 0) {
            return bytes32(id);
        }
        return bytes32(uint256(uint160(emitter)));
    }

    function _normalizeLogWord(address target, bytes32 word) internal view returns (bytes32) {
        if (uint256(word) >> 160 != 0) {
            return word;
        }
        uint256 id = _logAddressId(target, address(uint160(uint256(word))));
        if (id != 0) {
            return bytes32(id);
        }
        return word;
    }

    function _logAddressId(address target, address account) internal view returns (uint256) {
        if (account == address(0)) return 0;
        if (account == target) return 1;
        if (account == address(this)) return 2;
        if (account == BOB) return 3;
        if (account == CAROL) return 4;
        PairDeps storage pair = pairDeps[target];
        if (account == address(pair.token0)) return 10;
        if (account == address(pair.token1)) return 11;
        if (account == address(pair.flashCallee)) return 12;
        if (account == address(pair.reentrantCallee)) return 13;
        NoReturnPairDeps storage noReturnPair = noReturnPairDeps[target];
        if (account == address(noReturnPair.token0)) return 20;
        if (account == address(noReturnPair.token1)) return 21;
        CurveDeps storage curve = curveDeps[target];
        if (account == address(curve.coin0)) return 30;
        if (account == address(curve.coin1)) return 31;
        if (account == address(curve.coin2)) return 32;
        if (account == address(curve.coin3)) return 33;
        if (account == address(curve.coin4)) return 34;
        if (account == address(curve.coin5)) return 35;
        if (account == address(curve.coin6)) return 36;
        if (account == address(curve.coin7)) return 37;
        YearnDeps storage yearn = yearnDeps[target];
        if (account == address(yearn.asset)) return 40;
        if (account == address(yearn.strategy)) return 41;
        if (account == address(yearn.strategy2)) return 42;
        if (account == address(yearn.strategy3)) return 43;
        if (account == address(yearn.accountant)) return 44;
        if (account == address(yearn.reentrantAccountant)) return 45;
        if (account == address(yearn.depositLimitModule)) return 46;
        if (account == address(yearn.withdrawLimitModule)) return 47;
        if (account == address(yearn.mutatingAccountant)) return 48;
        return 0;
    }

    function _calldataGas(bytes memory data) internal pure returns (uint256 gasCost) {
        for (uint256 i = 0; i < data.length; i++) {
            gasCost += data[i] == 0 ? 4 : 16;
        }
    }

    function _bool(bool value) internal pure returns (string memory) {
        return value ? "true" : "false";
    }

    function _writeRow(
        string memory benchmarkId,
        string memory implementationId,
        string memory profileId,
        string memory scenario,
        string memory stateAccessProfile,
        string memory metadataMode,
        uint256 internalCreateGas,
        uint256 harnessCallGas,
        uint256 intrinsicGas,
        uint256 calldataGas,
        uint256 harnessEstimatedTxGas,
        bool expectedSuccess,
        bool callSucceeded,
        bool scenarioStatusOk
    ) internal {
        string memory line = string.concat(
            "{\"benchmark_id\":\"", benchmarkId,
            "\",\"implementation_id\":\"", implementationId,
            "\",\"profile_id\":\"", profileId
        );
        line = string.concat(
            line,
            "\",\"scenario\":\"", scenario,
            "\",\"state_access_profile\":\"", stateAccessProfile,
            "\",\"metadata_mode\":\"", metadataMode
        );
        line = string.concat(
            line,
            "\",\"internal_create_gas\":", vm.toString(internalCreateGas),
            ",\"harness_call_gas\":", vm.toString(harnessCallGas),
            ",\"intrinsic_gas\":", vm.toString(intrinsicGas),
            ",\"calldata_gas\":", vm.toString(calldataGas)
        );
        line = string.concat(
            line,
            ",\"harness_estimated_tx_gas\":", vm.toString(harnessEstimatedTxGas),
            ",\"expected_success\":", _bool(expectedSuccess),
            ",\"call_succeeded\":", _bool(callSucceeded),
            ",\"scenario_status_ok\":", _bool(scenarioStatusOk),
            "}"
        );
        vm.writeLine(GAS_JSONL_PATH, line);
    }

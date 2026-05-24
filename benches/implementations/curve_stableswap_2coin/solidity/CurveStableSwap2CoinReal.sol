// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

interface CurveBenchERC20 {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

interface CurveBenchERC20Detailed {
    function decimals() external view returns (uint8);
}

interface CurveBenchERC4626 {
    function asset() external view returns (address);
    function convertToAssets(uint256 shares) external view returns (uint256);
}

interface CurveBenchFactory {
    function fee_receiver() external view returns (address);
    function admin() external view returns (address);
    function views_implementation() external view returns (address);
}

interface CurveBenchStableSwapViews {
    function get_dx(int128 i, int128 j, uint256 dy, address pool) external view returns (uint256);
    function get_dy(int128 i, int128 j, uint256 dx, address pool) external view returns (uint256);
    function dynamic_fee(int128 i, int128 j, address pool) external view returns (uint256);
    function calc_token_amount(uint256[] calldata amounts, bool isDeposit, address pool)
        external
        view
        returns (uint256);
}

contract CurveStableSwap2CoinReal {
    uint256 public constant N_COINS = 2;
    uint256 internal constant MAX_COINS = 8;
    uint256 internal constant PRECISION = 1e18;
    uint256 internal constant A_PRECISION = 100;
    uint256 internal constant FEE_DENOMINATOR = 10_000_000_000;
    uint256 internal constant MAX_FEE = 5 * 10 ** 9;
    uint256 internal constant MAX_A = 10 ** 6;
    uint256 internal constant MAX_A_CHANGE = 10;
    uint256 internal constant MIN_RAMP_TIME = 86400;
    bytes32 internal constant EIP712_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract,bytes32 salt)");
    bytes32 internal constant EIP2612_TYPEHASH =
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");
    bytes32 internal constant ERC1271_MAGIC_VALUE = 0x1626ba7e00000000000000000000000000000000000000000000000000000000;

    string public name;
    string public symbol;
    string public constant version = "v7.0.0";
    uint8 public constant decimals = 18;

    CurveBenchFactory internal immutable factory;
    address[2] public coins;
    uint256 public initial_A;
    uint256 public future_A;
    uint256 public initial_A_time;
    uint256 public future_A_time;
    uint256 public fee;
    uint256 public constant admin_fee = 5_000_000_000;
    uint256 public offpeg_fee_multiplier;
    bool internal initialized;

    uint256[2] internal rate_multipliers;
    uint8[2] internal asset_types;
    bool internal pool_contains_rebasing_tokens;
    uint256[2] internal rate_oracles;
    uint256[2] internal call_amount;
    uint256[2] internal scale_factor;
    uint256[2] internal stored_balances;
    uint256[2] public admin_balances;
    uint256[1] internal last_prices_packed;
    uint256 internal last_D_packed;
    uint256 public ma_exp_time;
    uint256 public D_ma_time;
    uint256 public ma_last_time;
    uint256 public totalSupply;
    uint256 internal cachedChainId;
    bytes32 public salt;
    bytes32 internal nameHash;
    bytes32 internal cachedDomainSeparator;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    mapping(address => uint256) public nonces;

    event AddLiquidity(
        address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 invariant, uint256 tokenSupply
    );
    event TokenExchange(address indexed buyer, int128 soldId, uint256 tokensSold, int128 boughtId, uint256 tokensBought);
    event TokenExchangeUnderlying(
        address indexed buyer, int128 soldId, uint256 tokensSold, int128 boughtId, uint256 tokensBought
    );
    event RemoveLiquidity(address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 tokenSupply);
    event RemoveLiquidityOne(
        address indexed provider, int128 tokenId, uint256 tokenAmount, uint256 coinAmount, uint256 tokenSupply
    );
    event RemoveLiquidityImbalance(
        address indexed provider, uint256[] tokenAmounts, uint256[] fees, uint256 invariant, uint256 tokenSupply
    );
    event RampA(uint256 oldA, uint256 newA, uint256 initialTime, uint256 futureTime);
    event StopRampA(uint256 A, uint256 t);
    event ApplyNewFee(uint256 fee, uint256 offpegFeeMultiplier);
    event SetNewMATime(uint256 maExpTime, uint256 dMaTime);
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
        factory = CurveBenchFactory(msg.sender);
        require(coins_.length == N_COINS, "coin length");
        require(rateMultipliers.length >= N_COINS, "rate length");
        require(assetTypes.length >= N_COINS, "asset length");
        require(methodIds.length >= N_COINS, "method length");
        require(oracles.length >= N_COINS, "oracle length");
        require(coins_[0] != address(0) && coins_[1] != address(0) && coins_[0] != coins_[1], "coins");
        require(amp > 0, "amp");
        require(swapFee <= FEE_DENOMINATOR / 10, "fee");
        require(maExpTime != 0, "ma");
        initialized = true;
        name = name_;
        symbol = symbol_;
        coins[0] = coins_[0];
        coins[1] = coins_[1];
        for (uint256 i = 0; i < N_COINS; i++) {
            rate_multipliers[i] = rateMultipliers[i];
            asset_types[i] = assetTypes[i];
            if (assetTypes[i] == 2) {
                pool_contains_rebasing_tokens = true;
            }
            rate_oracles[i] = (uint256(uint32(methodIds[i])) << 224) | uint160(oracles[i]);
            if (assetTypes[i] == 3) {
                call_amount[i] = 10 ** uint256(CurveBenchERC20Detailed(coins_[i]).decimals());
                address underlying = CurveBenchERC4626(coins_[i]).asset();
                scale_factor[i] = 10 ** (18 - uint256(CurveBenchERC20Detailed(underlying).decimals()));
            }
        }
        uint256 preciseA = amp * A_PRECISION;
        initial_A = preciseA;
        future_A = preciseA;
        fee = swapFee;
        offpeg_fee_multiplier = offpegFeeMultiplier;
        ma_exp_time = maExpTime;
        D_ma_time = 62324;
        ma_last_time = _pack2(block.timestamp, block.timestamp);
        last_prices_packed[0] = _pack2(1e18, 1e18);
        cachedChainId = block.chainid;
        salt = block.number == 0 ? bytes32(0) : blockhash(block.number - 1);
        nameHash = keccak256(bytes(name_));
        cachedDomainSeparator = _buildDomainSeparator(cachedChainId);
        emit Transfer(address(0), msg.sender, 0);
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

    function add_liquidity(uint256[] calldata amounts, uint256 minMintAmount)
        external
        ready
        returns (uint256 minted)
    {
        return _addLiquidity(amounts, minMintAmount, msg.sender);
    }

    function add_liquidity(uint256[] calldata amounts, uint256 minMintAmount, address receiver)
        external
        ready
        returns (uint256 minted)
    {
        return _addLiquidity(amounts, minMintAmount, receiver);
    }

    function _addLiquidity(uint256[] calldata amounts, uint256 minMintAmount, address receiver)
        internal
        returns (uint256 minted)
    {
        require(receiver != address(0), "receiver");
        _checkDynArrayAmountLength(amounts.length);
        require(amounts[0] > 0 || amounts[1] > 0, "amount");
        uint256[2] memory oldBalances = _balances();
        uint256[2] memory rates = _storedRates();
        uint256 supply = totalSupply;
        uint256 d0 = supply == 0 ? 0 : _getDMem(rates, oldBalances);
        uint256[2] memory newBalances;
        newBalances[0] = oldBalances[0];
        newBalances[1] = oldBalances[1];
        for (uint256 i = 0; i < N_COINS; i++) {
            if (amounts[i] > 0) {
                newBalances[i] = oldBalances[i] + _transferIn(i, amounts[i], msg.sender, false);
            } else {
                require(supply != 0, "initial amount");
            }
        }
        uint256 d1 = _getDMem(rates, newBalances);
        require(d1 > d0, "invariant");
        uint256[] memory fees = new uint256[](0);
        if (supply == 0) {
            minted = d1;
        } else {
            fees = new uint256[](N_COINS);
            uint256 baseFee = _baseFee();
            uint256 ys = (d0 + d1) / N_COINS;
            for (uint256 feeIndex = 0; feeIndex < N_COINS; feeIndex++) {
                uint256 idealBalance = d1 * oldBalances[feeIndex] / d0;
                uint256 difference = _absDiff(idealBalance, newBalances[feeIndex]);
                uint256 xs = rates[feeIndex] * (oldBalances[feeIndex] + newBalances[feeIndex]) / PRECISION;
                fees[feeIndex] = _dynamicFee(xs, ys, baseFee) * difference / FEE_DENOMINATOR;
                admin_balances[feeIndex] += fees[feeIndex] * admin_fee / FEE_DENOMINATOR;
                newBalances[feeIndex] -= fees[feeIndex];
            }
            d1 = _getDMem(rates, newBalances);
            minted = supply * (d1 - d0) / d0;
        }
        require(minted >= minMintAmount && minted > 0, "slippage");
        _mint(receiver, minted);
        emit AddLiquidity(msg.sender, amounts, fees, d1, totalSupply);
        if (supply == 0) {
            last_D_packed = _pack2(d1, d1);
            uint256 priceTime = ma_last_time & ((uint256(1) << 128) - 1);
            uint256 dTime = ma_last_time >> 128;
            if (dTime < block.timestamp) {
                dTime = block.timestamp;
                ma_last_time = _pack2(priceTime, dTime);
            }
        } else {
            _upkeepOracles(_xpMem(rates, newBalances), _A(), d1);
        }
    }

    function exchange(int128 i, int128 j, uint256 dx, uint256 minDy) external ready returns (uint256 dy) {
        return _exchange(i, j, dx, minDy, msg.sender, false);
    }

    function exchange(int128 i, int128 j, uint256 dx, uint256 minDy, address receiver)
        external
        ready
        returns (uint256 dy)
    {
        return _exchange(i, j, dx, minDy, receiver, false);
    }

    function exchange_received(int128 i, int128 j, uint256 dx, uint256 minDy) external ready returns (uint256 dy) {
        require(!pool_contains_rebasing_tokens, "rebasing");
        return _exchange(i, j, dx, minDy, msg.sender, true);
    }

    function exchange_received(int128 i, int128 j, uint256 dx, uint256 minDy, address receiver)
        external
        ready
        returns (uint256 dy)
    {
        require(!pool_contains_rebasing_tokens, "rebasing");
        return _exchange(i, j, dx, minDy, receiver, true);
    }

    function _exchange(int128 i, int128 j, uint256 dx, uint256 minDy, address receiver, bool expectOptimisticTransfer)
        internal
        returns (uint256 dy)
    {
        require(receiver != address(0), "receiver");
        require(i >= 0 && j >= 0 && uint256(int256(i)) < N_COINS && uint256(int256(j)) < N_COINS && i != j, "coin");
        require(dx > 0, "dx");
        uint256 coinIn = uint256(int256(i));
        uint256 coinOut = uint256(int256(j));
        uint256[2] memory rates = _storedRates();
        uint256[2] memory xp = _xpMem(rates, _balances());
        uint256 actualDx = _transferIn(coinIn, dx, msg.sender, expectOptimisticTransfer);

        uint256 x;
        uint256 y;
        uint256 d;
        uint256 adminCut;
        (x, y, d, dy, adminCut) = _calcExchangeFromXP(coinIn, coinOut, actualDx, rates, xp);
        require(dy >= minDy, "slippage");

        admin_balances[coinOut] += adminCut;
        xp[coinIn] = x;
        xp[coinOut] = y;
        _upkeepOracles(xp, _A(), d);
        _transferOut(coinOut, dy, receiver);
        emit TokenExchange(msg.sender, i, actualDx, j, dy);
    }

    function remove_liquidity(uint256 lpAmount, uint256[] calldata minAmounts)
        external
        ready
        returns (uint256[] memory amounts)
    {
        return _removeLiquidity(lpAmount, minAmounts, msg.sender, true);
    }

    function remove_liquidity(uint256 lpAmount, uint256[] calldata minAmounts, address receiver)
        external
        ready
        returns (uint256[] memory amounts)
    {
        return _removeLiquidity(lpAmount, minAmounts, receiver, true);
    }

    function remove_liquidity(
        uint256 lpAmount,
        uint256[] calldata minAmounts,
        address receiver,
        bool claimAdminFees
    ) external ready returns (uint256[] memory amounts) {
        return _removeLiquidity(lpAmount, minAmounts, receiver, claimAdminFees);
    }

    function _removeLiquidity(uint256 lpAmount, uint256[] calldata minAmounts, address receiver, bool claimAdminFees)
        internal
        returns (uint256[] memory amounts)
    {
        require(receiver != address(0), "receiver");
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        require(minAmounts.length == N_COINS, "amount length");
        uint256 supply = totalSupply;
        uint256[2] memory currentBalances = _balances();
        amounts = new uint256[](N_COINS);
        for (uint256 i = 0; i < N_COINS; i++) {
            amounts[i] = currentBalances[i] * lpAmount / supply;
            require(amounts[i] >= minAmounts[i], "slippage");
            _transferOut(i, amounts[i], receiver);
        }
        _burn(msg.sender, lpAmount);
        _upkeepDOracleAfterLiquidityRemoval(lpAmount, supply);
        emit RemoveLiquidity(msg.sender, amounts, _emptyFees(), totalSupply);
        if (claimAdminFees) {
            _withdrawAdminFees();
        }
    }

    function remove_liquidity_imbalance(uint256[] calldata amounts, uint256 maxBurnAmount)
        external
        ready
        returns (uint256 burnAmount)
    {
        return _removeLiquidityImbalance(amounts, maxBurnAmount, msg.sender);
    }

    function remove_liquidity_imbalance(uint256[] calldata amounts, uint256 maxBurnAmount, address receiver)
        external
        ready
        returns (uint256 burnAmount)
    {
        return _removeLiquidityImbalance(amounts, maxBurnAmount, receiver);
    }

    function _removeLiquidityImbalance(uint256[] calldata amounts, uint256 maxBurnAmount, address receiver)
        internal
        returns (uint256 burnAmount)
    {
        _checkDynArrayAmountLength(amounts.length);
        require(receiver != address(0), "receiver");
        uint256[2] memory oldBalances = _balances();
        uint256[2] memory rates = _storedRates();
        uint256 d0 = _getDMem(rates, oldBalances);
        uint256[2] memory newBalances;
        newBalances[0] = oldBalances[0];
        newBalances[1] = oldBalances[1];
        for (uint256 i = 0; i < N_COINS; i++) {
            if (amounts[i] > 0) {
                require(newBalances[i] >= amounts[i], "balance");
                newBalances[i] -= amounts[i];
                _transferOut(i, amounts[i], receiver);
            }
        }

        uint256 d1 = _getDMem(rates, newBalances);
        uint256 baseFee = _baseFee();
        uint256 ys = (d0 + d1) / N_COINS;
        uint256[] memory fees = new uint256[](N_COINS);
        for (uint256 feeIndex = 0; feeIndex < N_COINS; feeIndex++) {
            uint256 idealBalance = d1 * oldBalances[feeIndex] / d0;
            uint256 difference = _absDiff(idealBalance, newBalances[feeIndex]);
            uint256 xs = rates[feeIndex] * (oldBalances[feeIndex] + newBalances[feeIndex]) / PRECISION;
            fees[feeIndex] = _dynamicFee(xs, ys, baseFee) * difference / FEE_DENOMINATOR;
            admin_balances[feeIndex] += fees[feeIndex] * admin_fee / FEE_DENOMINATOR;
            newBalances[feeIndex] -= fees[feeIndex];
        }

        d1 = _getDMem(rates, newBalances);
        burnAmount = (d0 - d1) * totalSupply / d0 + 1;
        require(burnAmount > 1, "burn");
        require(burnAmount <= maxBurnAmount, "slippage");
        _burn(msg.sender, burnAmount);
        emit RemoveLiquidityImbalance(msg.sender, amounts, fees, d1, totalSupply);
        _upkeepOracles(_xpMem(rates, newBalances), _A(), d1);
    }

    function remove_liquidity_one_coin(uint256 lpAmount, int128 i, uint256 minAmount)
        external
        ready
        returns (uint256 userAmount)
    {
        return _removeLiquidityOneCoin(lpAmount, i, minAmount, msg.sender);
    }

    function remove_liquidity_one_coin(uint256 lpAmount, int128 i, uint256 minAmount, address receiver)
        external
        ready
        returns (uint256 userAmount)
    {
        return _removeLiquidityOneCoin(lpAmount, i, minAmount, receiver);
    }

    function _removeLiquidityOneCoin(uint256 lpAmount, int128 i, uint256 minAmount, address receiver)
        internal
        returns (uint256 userAmount)
    {
        require(receiver != address(0), "receiver");
        require(i >= 0 && uint256(int256(i)) < N_COINS, "coin");
        uint256 coinIndex = uint256(int256(i));
        require(lpAmount > 0 && balanceOf[msg.sender] >= lpAmount, "lp");
        uint256 feeAmount;
        uint256[2] memory xp;
        uint256 amp;
        uint256 d;
        (userAmount, feeAmount, xp, amp, d) = _calcWithdrawOneCoin(lpAmount, coinIndex);
        uint256 adminCut = feeAmount * admin_fee / FEE_DENOMINATOR;
        require(userAmount >= minAmount, "slippage");
        admin_balances[coinIndex] += adminCut;
        _burn(msg.sender, lpAmount);
        _transferOut(coinIndex, userAmount, receiver);
        emit RemoveLiquidityOne(msg.sender, i, lpAmount, userAmount, totalSupply);
        _upkeepOracles(xp, amp, d);
    }

    function get_virtual_price() external view returns (uint256) {
        if (totalSupply == 0) {
            revert();
        }
        uint256[2] memory rates = _storedRates();
        return _getDMem(rates, _balances()) * 1e18 / totalSupply;
    }

    function calc_token_amount(uint256[] calldata amounts, bool isDeposit) external view returns (uint256) {
        return CurveBenchStableSwapViews(factory.views_implementation()).calc_token_amount(
            amounts, isDeposit, address(this)
        );
    }

    function A() external view returns (uint256) {
        return _A() / A_PRECISION;
    }

    function A_precise() external view returns (uint256) {
        return _A();
    }

    function get_balances() external view returns (uint256[] memory result) {
        uint256[2] memory currentBalances = _balances();
        result = new uint256[](N_COINS);
        result[0] = currentBalances[0];
        result[1] = currentBalances[1];
    }

    function balances(uint256 i) external view returns (uint256) {
        require(i < N_COINS, "coin");
        return _balances()[i];
    }

    function stored_rates() external view returns (uint256[] memory result) {
        uint256[2] memory rates = _storedRates();
        result = new uint256[](N_COINS);
        result[0] = rates[0];
        result[1] = rates[1];
    }

    function calc_withdraw_one_coin(uint256 burnAmount, int128 i) external view returns (uint256) {
        require(i >= 0 && uint256(int256(i)) < N_COINS, "coin");
        (uint256 dy,,,,) = _calcWithdrawOneCoin(burnAmount, uint256(int256(i)));
        return dy;
    }

    function get_dy(int128 i, int128 j, uint256 dx) external view returns (uint256) {
        return CurveBenchStableSwapViews(factory.views_implementation()).get_dy(i, j, dx, address(this));
    }

    function get_dx(int128 i, int128 j, uint256 dy) external view returns (uint256) {
        return CurveBenchStableSwapViews(factory.views_implementation()).get_dx(i, j, dy, address(this));
    }

    function dynamic_fee(int128 i, int128 j) external view returns (uint256) {
        return CurveBenchStableSwapViews(factory.views_implementation()).dynamic_fee(i, j, address(this));
    }

    function last_price(uint256 i) external view returns (uint256) {
        require(i < N_COINS - 1, "price");
        return last_prices_packed[i] & ((uint256(1) << 128) - 1);
    }

    function ema_price(uint256 i) external view returns (uint256) {
        require(i < N_COINS - 1, "price");
        return last_prices_packed[i] >> 128;
    }

    function get_p(uint256 i) external view returns (uint256) {
        require(i < N_COINS - 1, "price");
        uint256[2] memory xp = _xpMem(_storedRates(), _balances());
        uint256 d = _getD(xp[0], xp[1]);
        return _getP(xp, _A(), d);
    }

    function price_oracle(uint256 i) external view returns (uint256) {
        require(i < N_COINS - 1, "price");
        return _calcMovingAverage(last_prices_packed[i], ma_exp_time, ma_last_time & ((uint256(1) << 128) - 1));
    }

    function D_oracle() external view returns (uint256) {
        return _calcMovingAverage(last_D_packed, D_ma_time, ma_last_time >> 128);
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

    function ramp_A(uint256 futureA, uint256 futureTime) external {
        require(msg.sender == factory.admin(), "admin");
        require(block.timestamp >= initial_A_time + MIN_RAMP_TIME, "ramp active");
        require(futureTime >= block.timestamp + MIN_RAMP_TIME, "time");

        uint256 initialAPrecise = _A();
        uint256 futureAPrecise = futureA * A_PRECISION;
        require(futureA > 0 && futureA < MAX_A, "A");
        if (futureAPrecise < initialAPrecise) {
            require(futureAPrecise * MAX_A_CHANGE >= initialAPrecise, "A change");
        } else {
            require(futureAPrecise <= initialAPrecise * MAX_A_CHANGE, "A change");
        }

        initial_A = initialAPrecise;
        future_A = futureAPrecise;
        initial_A_time = block.timestamp;
        future_A_time = futureTime;
        emit RampA(initialAPrecise, futureAPrecise, block.timestamp, futureTime);
    }

    function stop_ramp_A() external {
        require(msg.sender == factory.admin(), "admin");
        uint256 currentA = _A();
        initial_A = currentA;
        future_A = currentA;
        initial_A_time = block.timestamp;
        future_A_time = block.timestamp;
        emit StopRampA(currentA, block.timestamp);
    }

    function set_new_fee(uint256 newFee, uint256 newOffpegFeeMultiplier) external {
        require(msg.sender == factory.admin(), "admin");
        require(newFee <= MAX_FEE, "fee");
        require(newOffpegFeeMultiplier * newFee <= MAX_FEE * FEE_DENOMINATOR, "offpeg");
        fee = newFee;
        offpeg_fee_multiplier = newOffpegFeeMultiplier;
        emit ApplyNewFee(newFee, newOffpegFeeMultiplier);
    }

    function set_ma_exp_time(uint256 newMaExpTime, uint256 newDMaTime) external {
        require(msg.sender == factory.admin(), "admin");
        require(newMaExpTime * newDMaTime > 0, "ma");
        ma_exp_time = newMaExpTime;
        D_ma_time = newDMaTime;
        emit SetNewMATime(newMaExpTime, newDMaTime);
    }

    function withdraw_admin_fees() external {
        _withdrawAdminFees();
    }

    function _emptyFees() internal pure returns (uint256[] memory fees) {
        fees = new uint256[](0);
    }

    function _A() internal view returns (uint256) {
        uint256 t1 = future_A_time;
        uint256 a1 = future_A;
        if (block.timestamp < t1) {
            uint256 a0 = initial_A;
            uint256 t0 = initial_A_time;
            if (a1 > a0) {
                return a0 + (a1 - a0) * (block.timestamp - t0) / (t1 - t0);
            }
            return a0 - (a0 - a1) * (block.timestamp - t0) / (t1 - t0);
        }
        return a1;
    }

    function _withdrawAdminFees() internal {
        address receiver = factory.fee_receiver();
        if (receiver == address(0)) {
            return;
        }
        for (uint256 i = 0; i < N_COINS; i++) {
            uint256 amount = admin_balances[i];
            if (amount > 0) {
                _transferOut(i, amount, receiver);
                admin_balances[i] = 0;
            }
        }
    }

    function _pack2(uint256 x, uint256 y) internal pure returns (uint256) {
        require(x < 2 ** 128 && y < 2 ** 128, "pack");
        return x | (y << 128);
    }

    function _calcMovingAverage(uint256 packedValue, uint256 averagingWindow, uint256 lastTime)
        internal
        view
        returns (uint256)
    {
        uint256 lastSpot = packedValue & ((uint256(1) << 128) - 1);
        uint256 lastEma = packedValue >> 128;
        if (lastTime < block.timestamp) {
            uint256 alpha = _wadExp(-int256((block.timestamp - lastTime) * 1e18 / averagingWindow));
            return (lastSpot * (1e18 - alpha) + lastEma * alpha) / 1e18;
        }
        return lastEma;
    }

    function _wadExp(int256 x) internal pure returns (uint256) {
        if (x <= -41446531673892822313) {
            return 0;
        }
        require(x < 135305999368893231589, "wad_exp overflow");

        int256 value = (x << 78) / int256(5 ** 18);
        int256 k = (((value << 96) / 54916777467707473351141471128) + (int256(1) << 95)) >> 96;
        value -= k * 54916777467707473351141471128;

        int256 y = (((value + 1346386616545796478920950773328) * value) >> 96)
            + 57155421227552351082224309758442;
        int256 p = (((((y + value) - 94201549194550492254356042504812) * y) >> 96)
            + 28719021644029726153956944680412240) * value
            + (int256(4385272521454847904659076985693276) << 96);

        int256 q = (((value - 2855989394907223263936484059900) * value) >> 96)
            + 50020603652535783019961831881945;
        q = ((q * value) >> 96) - 533845033583426703283633433725380;
        q = ((q * value) >> 96) + 3604857256930695427073651918091429;
        q = ((q * value) >> 96) - 14423608567350463180887372962807573;
        q = ((q * value) >> 96) + 26449188498355588339934803723976023;

        int256 r = p / q;
        return (uint256(r) * 3822833074963236453042738258902158003155416615667) >> uint256(195 - k);
    }

    function _getP(uint256[2] memory xp, uint256 amp, uint256 d) internal pure returns (uint256) {
        if (d == 0 || xp[0] == 0 || xp[1] == 0) {
            return PRECISION;
        }
        uint256 ann = amp * N_COINS;
        uint256 dr = d / 4;
        dr = dr * d / xp[0];
        dr = dr * d / xp[1];
        uint256 xp0A = ann * xp[0] / A_PRECISION;
        return PRECISION * (xp0A + dr * xp[0] / xp[1]) / (xp0A + dr);
    }

    function _upkeepOracles(uint256[2] memory xp, uint256 amp, uint256 d) internal {
        uint256 lastTime = ma_last_time;
        uint256 priceTime = lastTime & ((uint256(1) << 128) - 1);
        uint256 dTime = lastTime >> 128;
        uint256 spotPrice = _getP(xp, amp, d);
        if (spotPrice > 2 * PRECISION) {
            spotPrice = 2 * PRECISION;
        }
        last_prices_packed[0] = _pack2(spotPrice, _calcMovingAverage(last_prices_packed[0], ma_exp_time, priceTime));
        last_D_packed = _pack2(d, _calcMovingAverage(last_D_packed, D_ma_time, dTime));
        if (priceTime < block.timestamp) {
            priceTime = block.timestamp;
        }
        if (dTime < block.timestamp) {
            dTime = block.timestamp;
        }
        ma_last_time = _pack2(priceTime, dTime);
    }

    function _upkeepDOracleAfterLiquidityRemoval(uint256 lpAmount, uint256 previousSupply) internal {
        uint256 lastTime = ma_last_time;
        uint256 priceTime = lastTime & ((uint256(1) << 128) - 1);
        uint256 dTime = lastTime >> 128;
        uint256 currentPacked = last_D_packed;
        uint256 oldD = currentPacked & ((uint256(1) << 128) - 1);
        uint256 newD = oldD - oldD * lpAmount / previousSupply;
        last_D_packed = _pack2(newD, _calcMovingAverage(currentPacked, D_ma_time, dTime));
        if (dTime < block.timestamp) {
            dTime = block.timestamp;
            ma_last_time = _pack2(priceTime, dTime);
        }
    }

    function _transferIn(uint256 coinIndex, uint256 amount, address sender, bool expectOptimisticTransfer)
        internal
        returns (uint256 received)
    {
        uint256 oldBalance = CurveBenchERC20(coins[coinIndex]).balanceOf(address(this));
        if (expectOptimisticTransfer) {
            received = oldBalance - stored_balances[coinIndex];
            require(received >= amount, "optimistic transfer");
        } else {
            require(amount > 0, "amount");
            _safeTransferFrom(coins[coinIndex], sender, address(this), amount);
            received = CurveBenchERC20(coins[coinIndex]).balanceOf(address(this)) - oldBalance;
        }
        stored_balances[coinIndex] += received;
    }

    function _transferOut(uint256 coinIndex, uint256 amount, address receiver) internal {
        require(receiver != address(0), "receiver");
        if (!pool_contains_rebasing_tokens) {
            stored_balances[coinIndex] -= amount;
            _safeTransfer(coins[coinIndex], receiver, amount);
        } else {
            uint256 coinBalance = CurveBenchERC20(coins[coinIndex]).balanceOf(address(this));
            _safeTransfer(coins[coinIndex], receiver, amount);
            stored_balances[coinIndex] = coinBalance - amount;
        }
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

    function _storedRates() internal view returns (uint256[2] memory rates) {
        rates = rate_multipliers;
        for (uint256 i = 0; i < N_COINS; i++) {
            if (asset_types[i] == 1 && rate_oracles[i] != 0) {
                bytes4 selector = bytes4(uint32(rate_oracles[i] >> 224));
                address oracle = address(uint160(rate_oracles[i]));
                (bool ok, bytes memory response) = oracle.staticcall(abi.encodeWithSelector(selector));
                require(ok && response.length == 32, "rate oracle");
                uint256 fetchedRate = abi.decode(response, (uint256));
                rates[i] = rates[i] * fetchedRate / PRECISION;
            } else if (asset_types[i] == 3) {
                rates[i] = rates[i] * CurveBenchERC4626(coins[i]).convertToAssets(call_amount[i]) * scale_factor[i]
                    / PRECISION;
            }
        }
    }

    function _balances() internal view returns (uint256[2] memory result) {
        for (uint256 i = 0; i < N_COINS; i++) {
            if (pool_contains_rebasing_tokens) {
                result[i] = CurveBenchERC20(coins[i]).balanceOf(address(this)) - admin_balances[i];
            } else {
                result[i] = stored_balances[i] - admin_balances[i];
            }
        }
    }

    function _xpMem(uint256[2] memory rates, uint256[2] memory sourceBalances)
        internal
        pure
        returns (uint256[2] memory xp)
    {
        xp[0] = rates[0] * sourceBalances[0] / PRECISION;
        xp[1] = rates[1] * sourceBalances[1] / PRECISION;
    }

    function _getDMem(uint256[2] memory rates, uint256[2] memory sourceBalances) internal view returns (uint256) {
        uint256[2] memory xp = _xpMem(rates, sourceBalances);
        return _getD(xp[0], xp[1]);
    }

    function _getD(uint256 x0, uint256 x1) internal view returns (uint256) {
        uint256 sum = x0 + x1;
        if (sum == 0) {
            return 0;
        }
        uint256 d = sum;
        uint256 ann = _A() * N_COINS;
        for (uint256 dIdx = 0; dIdx < 255; dIdx++) {
            uint256 dP = d;
            dP = dP * d / x0;
            dP = dP * d / x1;
            dP /= N_COINS ** N_COINS;
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

    function _checkDynArrayAmountLength(uint256 length) internal pure {
        require(length >= N_COINS && length <= MAX_COINS, "amount length");
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
        returns (uint256 x, uint256 y, uint256 d, uint256 userDy, uint256 adminCut)
    {
        uint256[2] memory oldBalances = _balances();
        uint256[2] memory rates = _storedRates();
        uint256[2] memory xp = _xpMem(rates, oldBalances);
        return _calcExchangeFromXP(coinIn, coinOut, dx, rates, xp);
    }

    function _calcExchangeFromXP(
        uint256 coinIn,
        uint256 coinOut,
        uint256 dx,
        uint256[2] memory rates,
        uint256[2] memory xp
    ) internal view returns (uint256 x, uint256 y, uint256 d, uint256 userDy, uint256 adminCut) {
        x = xp[coinIn] + dx * rates[coinIn] / PRECISION;
        d = _getD(xp[0], xp[1]);
        y = _getY(coinIn, coinOut, x, xp, d);
        uint256 grossDy = xp[coinOut] - y - 1;
        uint256 feeAmount = grossDy
            * _dynamicFee((xp[coinIn] + x) / 2, (xp[coinOut] + y) / 2, fee)
            / FEE_DENOMINATOR;
        adminCut = feeAmount * admin_fee / FEE_DENOMINATOR * PRECISION / rates[coinOut];
        userDy = (grossDy - feeAmount) * PRECISION / rates[coinOut];
    }

    function _getY(uint256 i, uint256 j, uint256 x, uint256[2] memory xp, uint256 d) internal view returns (uint256) {
        uint256 ann = _A() * N_COINS;
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
        uint256 ann = _A() * N_COINS;
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

    function _calcWithdrawOneCoin(uint256 lpAmount, uint256 i)
        internal
        view
        returns (uint256 dy, uint256 feeAmount, uint256[2] memory xp, uint256 amp, uint256 d1)
    {
        amp = _A();
        uint256[2] memory rates = _storedRates();
        xp = _xpMem(rates, _balances());
        uint256 d0 = _getD(xp[0], xp[1]);
        d1 = d0 - lpAmount * d0 / totalSupply;
        uint256 newY = _getYD(i, xp, d1);
        uint256[2] memory xpReduced;
        xpReduced[0] = xp[0];
        xpReduced[1] = xp[1];
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
        uint256 dyNoFee = (xp[i] - newY) * PRECISION / rates[i];
        dy = (xpReduced[i] - reducedY - 1) * PRECISION / rates[i];
        feeAmount = dyNoFee - dy;
        xp[i] = newY;
    }
}

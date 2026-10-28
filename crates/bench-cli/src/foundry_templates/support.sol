contract BenchERC20 {
    string public constant name = "Bench Token";
    string public constant symbol = "BENCH";
    uint8 public constant decimals = 18;

    bool public approveReturnData = true;
    bool public transferReturnData = true;
    bool public transferFromReturnData = true;
    bool public approveReturnValue = true;
    bool public transferReturnValue = true;
    bool public transferFromReturnValue = true;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

    function setReturnData(bool approveEnabled, bool transferEnabled, bool transferFromEnabled) external {
        approveReturnData = approveEnabled;
        transferReturnData = transferEnabled;
        transferFromReturnData = transferFromEnabled;
    }

    function setReturnValue(bool approveValue, bool transferValue, bool transferFromValue) external {
        approveReturnValue = approveValue;
        transferReturnValue = transferValue;
        transferFromReturnValue = transferFromValue;
    }

    function mint(address to, uint256 value) external returns (bool) {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
        return true;
    }

    function burn(address from, uint256 value) external returns (bool) {
        require(balanceOf[from] >= value, "burn balance");
        balanceOf[from] -= value;
        totalSupply -= value;
        emit Transfer(from, address(0), value);
        return true;
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return _optionalReturn(approveReturnData, approveReturnValue);
    }

    function transfer(address to, uint256 value) external returns (bool) {
        _transfer(msg.sender, to, value);
        return _optionalReturn(transferReturnData, transferReturnValue);
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= value, "allowance");
            allowance[from][msg.sender] = allowed - value;
        }
        _transfer(from, to, value);
        return _optionalReturn(transferFromReturnData, transferFromReturnValue);
    }

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }

    function _optionalReturn(bool returnData, bool returnValue) internal pure returns (bool) {
        if (!returnData) {
            assembly {
                return(0, 0)
            }
        }
        return returnValue;
    }
}

contract BenchERC20NoReturn {
    string public constant name = "No Return Bench Token";
    string public constant symbol = "NORET";
    uint8 public constant decimals = 18;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

    function mint(address to, uint256 value) external {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
    }

    function approve(address spender, uint256 value) external {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
    }

    function transfer(address to, uint256 value) external {
        _transfer(msg.sender, to, value);
    }

    function transferFrom(address from, address to, uint256 value) external {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= value, "allowance");
            allowance[from][msg.sender] = allowed - value;
        }
        _transfer(from, to, value);
    }

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }
}

contract BenchERC20OptionalReturn {
    string public constant name = "Optional Return Bench Token";
    string public constant symbol = "OPTRET";
    uint8 public constant decimals = 18;

    bool public returnData = true;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

    function setReturnData(bool enabled) external {
        returnData = enabled;
    }

    function mint(address to, uint256 value) external returns (bool) {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
        return true;
    }

    function burn(address from, uint256 value) external returns (bool) {
        require(balanceOf[from] >= value, "burn balance");
        balanceOf[from] -= value;
        totalSupply -= value;
        emit Transfer(from, address(0), value);
        return true;
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        if (!returnData) {
            assembly {
                return(0, 0)
            }
        }
        return true;
    }

    function transfer(address to, uint256 value) external returns (bool) {
        _transfer(msg.sender, to, value);
        if (!returnData) {
            assembly {
                return(0, 0)
            }
        }
        return true;
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= value, "allowance");
            allowance[from][msg.sender] = allowed - value;
        }
        _transfer(from, to, value);
        if (!returnData) {
            assembly {
                return(0, 0)
            }
        }
        return true;
    }

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }
}

contract BenchCurveRateOracle {
    uint256 internal rateValue;

    constructor(uint256 rate_) {
        rateValue = rate_;
    }

    function setRate(uint256 rate_) external {
        rateValue = rate_;
    }

    function rate() external view returns (uint256) {
        return rateValue;
    }
}

contract BenchCurveERC4626 is BenchERC20OptionalReturn {
    address public immutable asset;
    uint256 public assetsPerShare;

    constructor(address asset_, uint256 assetsPerShare_) {
        asset = asset_;
        assetsPerShare = assetsPerShare_;
    }

    function setAssetsPerShare(uint256 assetsPerShare_) external {
        assetsPerShare = assetsPerShare_;
    }

    function convertToAssets(uint256 shares) external view returns (uint256) {
        return shares * assetsPerShare / 1e18;
    }
}

contract BenchERC1271Wallet {
    bytes4 internal constant MAGIC_VALUE = 0x1626ba7e;
    bytes32 public validDigest;

    function setValidDigest(bytes32 digest) external {
        validDigest = digest;
    }

    function isValidSignature(bytes32 digest, bytes calldata) external view returns (bytes4) {
        return digest == validDigest ? MAGIC_VALUE : bytes4(0);
    }
}

contract BenchUniswapFlashCallee {
    function uniswapV2Call(address, uint256, uint256, bytes calldata data) external {
        (address token0, address token1, uint256 repay0, uint256 repay1) =
            abi.decode(data, (address, address, uint256, uint256));
        if (repay0 > 0) {
            require(BenchERC20(token0).transfer(msg.sender, repay0), "repay0");
        }
        if (repay1 > 0) {
            require(BenchERC20(token1).transfer(msg.sender, repay1), "repay1");
        }
    }
}

contract BenchUniswapReentrantCallee {
    function uniswapV2Call(address, uint256, uint256, bytes calldata data) external {
        (address token0, address token1, uint256 repay0, uint256 repay1) =
            abi.decode(data, (address, address, uint256, uint256));
        (bool ok,) = msg.sender.call(abi.encodeWithSignature("sync()"));
        require(ok, "reentrant sync");
        if (repay0 > 0) {
            require(BenchERC20(token0).transfer(msg.sender, repay0), "repay0");
        }
        if (repay1 > 0) {
            require(BenchERC20(token1).transfer(msg.sender, repay1), "repay1");
        }
    }
}

interface BenchUniswapPairLike {
    function initialize(address token0, address token1) external;
}

interface BenchUniswapFactoryLike {
    function feeTo() external view returns (address);
    function feeToSetter() external view returns (address);
    function getPair(address token0, address token1) external view returns (address);
    function allPairs(uint256 index) external view returns (address);
    function allPairsLength() external view returns (uint256);
}

contract BenchUniswapFactoryPair {
    address public factory;
    address public token0;
    address public token1;

    constructor() {
        factory = msg.sender;
    }

    function initialize(address token0_, address token1_) external {
        require(msg.sender == factory, "UniswapV2: FORBIDDEN");
        token0 = token0_;
        token1 = token1_;
    }
}

contract BenchUniswapCreate2Factory {
    address public feeTo;
    address public feeToSetter;
    mapping(address => mapping(address => address)) public getPair;
    address[] public allPairs;
    bytes internal pairCode;

    event PairCreated(address indexed token0, address indexed token1, address pair, uint256);

    constructor(bytes memory code) {
        feeToSetter = msg.sender;
        pairCode = code;
    }

    function allPairsLength() external view returns (uint256) {
        return allPairs.length;
    }

    function setFeeTo(address newFeeTo) external {
        require(msg.sender == feeToSetter, "UniswapV2: FORBIDDEN");
        feeTo = newFeeTo;
    }

    function setFeeToSetter(address newFeeToSetter) external {
        require(msg.sender == feeToSetter, "UniswapV2: FORBIDDEN");
        feeToSetter = newFeeToSetter;
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
        require(pair != address(0) && pair.code.length != 0, "pair create2");
        BenchUniswapPairLike(pair).initialize(token0, token1);
        getPair[token0][token1] = pair;
        getPair[token1][token0] = pair;
        allPairs.push(pair);
        emit PairCreated(token0, token1, pair, allPairs.length);
    }
}

contract BenchYearnStrategy {
    BenchERC20 public immutable asset;
    address public vault;
    bool public emergencyExit;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 public pendingGain;
    uint256 public pendingLoss;
    uint256 public maxDepositLimit = type(uint256).max;
    uint256 public maxRedeemLimit = type(uint256).max;
    uint256 public redeemReturnBps = 10_000;
    uint256 public shareMintBps = 10_000;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

    constructor(BenchERC20 asset_) {
        asset = asset_;
    }

    function setVault(address vault_) external returns (bool) {
        vault = vault_;
        return true;
    }

    function want() external view returns (address) {
        return address(asset);
    }

    function isActive() external view returns (bool) {
        return !emergencyExit;
    }

    function delegatedAssets() external pure returns (uint256) {
        return 0;
    }

    function estimatedTotalAssets() external view returns (uint256) {
        return asset.balanceOf(address(this));
    }

    function setEmergencyExit(bool value) external returns (bool) {
        emergencyExit = value;
        return true;
    }

    function maxDeposit(address) external view returns (uint256) {
        return maxDepositLimit;
    }

    function maxRedeem(address owner) external view returns (uint256) {
        uint256 balance = balanceOf[owner];
        return maxRedeemLimit < balance ? maxRedeemLimit : balance;
    }

    function convertToAssets(uint256 shares) public view returns (uint256) {
        if (totalSupply == 0) {
            return shares;
        }
        return shares * asset.balanceOf(address(this)) / totalSupply;
    }

    function convertToShares(uint256 assets) external view returns (uint256) {
        uint256 totalAssets = asset.balanceOf(address(this));
        if (totalSupply == 0 || totalAssets == 0) {
            return assets;
        }
        return assets * totalSupply / totalAssets;
    }

    function previewWithdraw(uint256 assets) external view returns (uint256) {
        uint256 totalAssets = asset.balanceOf(address(this));
        if (totalSupply == 0 || totalAssets == 0) {
            return assets;
        }
        uint256 shares = assets * totalSupply / totalAssets;
        if (shares * totalAssets < assets * totalSupply) {
            shares += 1;
        }
        return shares;
    }

    function deposit(uint256 assets, address receiver) external returns (uint256 shares) {
        require(asset.transferFrom(msg.sender, address(this), assets), "transferFrom");
        shares = assets * shareMintBps / 10_000;
        totalSupply += shares;
        balanceOf[receiver] += shares;
    }

    function redeem(uint256 shares, address receiver, address owner) external returns (uint256 assets) {
        require(msg.sender == owner, "owner");
        require(balanceOf[owner] >= shares, "shares");
        require(shares <= maxRedeemLimit, "max redeem");
        assets = convertToAssets(shares);
        balanceOf[owner] -= shares;
        totalSupply -= shares;
        assets = assets * redeemReturnBps / 10_000;
        require(asset.transfer(receiver, assets), "transfer");
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
        }
        _transfer(from, to, value);
        return true;
    }

    function increaseDebt(uint256 amount) external returns (bool) {
        amount;
        return true;
    }

    function setReport(uint256 gain, uint256 loss) external returns (bool) {
        pendingGain = gain;
        pendingLoss = loss;
        if (gain > 0) {
            asset.mint(address(this), gain);
        }
        if (loss > 0) {
            uint256 burnAmount = loss > asset.balanceOf(address(this)) ? asset.balanceOf(address(this)) : loss;
            if (burnAmount > 0) {
                asset.burn(address(this), burnAmount);
            }
        }
        return true;
    }

    function setMaxDepositLimit(uint256 limit) external returns (bool) {
        maxDepositLimit = limit;
        return true;
    }

    function setMaxRedeemLimit(uint256 limit) external returns (bool) {
        maxRedeemLimit = limit;
        return true;
    }

    function setRedeemReturnBps(uint256 bps) external returns (bool) {
        redeemReturnBps = bps;
        return true;
    }

    function setShareMintBps(uint256 bps) external returns (bool) {
        shareMintBps = bps;
        return true;
    }

    function report() external returns (uint256 gain, uint256 loss) {
        gain = pendingGain;
        loss = pendingLoss;
        pendingGain = 0;
        pendingLoss = 0;
    }

    function withdrawTo(address receiver, uint256 amount, uint256 maxLoss)
        external
        returns (uint256 withdrawn, uint256 loss)
    {
        uint256 available = asset.balanceOf(address(this));
        uint256 target = amount > available ? available : amount;
        withdrawn = target > available ? available : target;
        loss = target - withdrawn;
        require(target == 0 || loss * 10000 <= target * maxLoss, "loss");
        if (withdrawn > 0) {
            require(asset.transfer(receiver, withdrawn), "transfer");
        }
    }

    function withdraw(uint256 amount) external returns (uint256 loss) {
        uint256 available = asset.balanceOf(address(this));
        uint256 withdrawn = amount > available ? available : amount;
        loss = amount - withdrawn;
        if (withdrawn > 0) {
            require(asset.transfer(msg.sender, withdrawn), "transfer");
        }
    }

    function migrate(address newStrategy) external {
        uint256 available = asset.balanceOf(address(this));
        if (available > 0) {
            require(asset.transfer(newStrategy, available), "transfer");
        }
    }

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }
}

contract BenchYearnAccountant {
    BenchERC20 public immutable asset;
    uint256 public totalFees;
    uint256 public totalRefunds;

    constructor(BenchERC20 asset_) {
        asset = asset_;
    }

    function setReport(address vault, uint256 fees, uint256 refunds) external returns (bool) {
        totalFees = fees;
        totalRefunds = refunds;
        if (refunds > 0) {
            asset.mint(address(this), refunds);
            asset.approve(vault, refunds);
        }
        return true;
    }

    function setClippedRefundReport(
        address vault,
        uint256 fees,
        uint256 refunds,
        uint256 mintedRefunds,
        uint256 approvedRefunds
    ) external returns (bool) {
        totalFees = fees;
        totalRefunds = refunds;
        if (mintedRefunds > 0) {
            asset.mint(address(this), mintedRefunds);
        }
        asset.approve(vault, approvedRefunds);
        return true;
    }

    function report(address, uint256, uint256) external returns (uint256 fees, uint256 refunds) {
        fees = totalFees;
        refunds = totalRefunds;
        totalFees = 0;
        totalRefunds = 0;
    }
}

contract BenchYearnMutatingAccountant {
    BenchERC20 public immutable asset;
    uint256 public totalFees;
    uint256 public totalRefunds;
    uint256 public reportMintedRefunds;
    uint256 public reportApprovedRefunds;

    constructor(BenchERC20 asset_) {
        asset = asset_;
    }

    function setReport(
        address vault,
        uint256 fees,
        uint256 refunds,
        uint256 preMintedRefunds,
        uint256 preApprovedRefunds,
        uint256 mintedDuringReport,
        uint256 approvedDuringReport
    ) external returns (bool) {
        totalFees = fees;
        totalRefunds = refunds;
        reportMintedRefunds = mintedDuringReport;
        reportApprovedRefunds = approvedDuringReport;
        if (preMintedRefunds > 0) {
            asset.mint(address(this), preMintedRefunds);
        }
        asset.approve(vault, preApprovedRefunds);
        return true;
    }

    function report(address, uint256, uint256) external returns (uint256 fees, uint256 refunds) {
        if (reportMintedRefunds > 0) {
            asset.mint(address(this), reportMintedRefunds);
        }
        asset.approve(msg.sender, reportApprovedRefunds);
        fees = totalFees;
        refunds = totalRefunds;
        totalFees = 0;
        totalRefunds = 0;
        reportMintedRefunds = 0;
        reportApprovedRefunds = 0;
    }
}

contract BenchYearnReentrantAccountant {
    BenchERC20 public immutable asset;

    constructor(BenchERC20 asset_) {
        asset = asset_;
    }

    function prepare(address vault) external returns (bool) {
        asset.mint(address(this), 1e18);
        asset.approve(vault, type(uint256).max);
        return true;
    }

    function report(address, uint256, uint256) external returns (uint256 fees, uint256 refunds) {
        (bool ok,) = msg.sender.call(abi.encodeWithSignature("deposit(uint256,address)", uint256(1), address(this)));
        require(ok, "reenter deposit");
        return (fees, refunds);
    }
}

contract BenchYearnDepositLimitModule {
    uint256 public limit;
    address public specialReceiver;
    uint256 public specialReceiverLimit;
    bool public shouldRevert;

    function setLimit(uint256 limit_) external returns (bool) {
        limit = limit_;
        return true;
    }

    function setSpecialReceiver(address receiver, uint256 limit_) external returns (bool) {
        specialReceiver = receiver;
        specialReceiverLimit = limit_;
        return true;
    }

    function setShouldRevert(bool shouldRevert_) external returns (bool) {
        shouldRevert = shouldRevert_;
        return true;
    }

    function available_deposit_limit(address receiver) external view returns (uint256) {
        require(!shouldRevert, "deposit limit module");
        if (receiver == specialReceiver) {
            return specialReceiverLimit;
        }
        return limit;
    }
}

contract BenchYearnWithdrawLimitModule {
    uint256 public limit;
    address public specialOwner;
    uint256 public specialOwnerLimit;
    bool public useSpecialMaxLoss;
    uint256 public specialMaxLoss;
    uint256 public specialMaxLossLimit;
    bool public useSpecialStrategiesHash;
    bytes32 public specialStrategiesHash;
    uint256 public specialStrategiesLimit;
    bool public shouldRevert;

    function setLimit(uint256 limit_) external returns (bool) {
        limit = limit_;
        return true;
    }

    function setSpecialOwner(address owner, uint256 limit_) external returns (bool) {
        specialOwner = owner;
        specialOwnerLimit = limit_;
        return true;
    }

    function setSpecialMaxLoss(uint256 maxLoss, uint256 limit_) external returns (bool) {
        useSpecialMaxLoss = true;
        specialMaxLoss = maxLoss;
        specialMaxLossLimit = limit_;
        return true;
    }

    function setSpecialStrategiesHash(bytes32 strategiesHash, uint256 limit_) external returns (bool) {
        useSpecialStrategiesHash = true;
        specialStrategiesHash = strategiesHash;
        specialStrategiesLimit = limit_;
        return true;
    }

    function clearSpecialCases() external returns (bool) {
        specialOwner = address(0);
        specialOwnerLimit = 0;
        useSpecialMaxLoss = false;
        specialMaxLoss = 0;
        specialMaxLossLimit = 0;
        useSpecialStrategiesHash = false;
        specialStrategiesHash = bytes32(0);
        specialStrategiesLimit = 0;
        return true;
    }

    function setShouldRevert(bool shouldRevert_) external returns (bool) {
        shouldRevert = shouldRevert_;
        return true;
    }

    function available_withdraw_limit(address owner, uint256 maxLoss, address[] calldata strategies) external view returns (uint256) {
        require(!shouldRevert, "withdraw limit module");
        if (owner == specialOwner) {
            return specialOwnerLimit;
        }
        if (useSpecialMaxLoss && maxLoss == specialMaxLoss) {
            return specialMaxLossLimit;
        }
        if (useSpecialStrategiesHash && keccak256(abi.encode(strategies)) == specialStrategiesHash) {
            return specialStrategiesLimit;
        }
        return limit;
    }
}

// Factory-side callbacks that targets make during measured calls. They live on
// this fixed contract, not on the generated shard, so their dispatch cost does
// not depend on how many tests a shard holds.
contract BenchFactoryHub {
    address public immutable admin;
    address public feeTo;
    uint16 internal protocolFeeBps;
    address internal protocolFeeRecipient;

    constructor() {
        admin = msg.sender;
    }

    modifier onlyAdmin() {
        require(msg.sender == admin, "hub admin");
        _;
    }

    function setFeeTo(address newFeeTo) external onlyAdmin {
        feeTo = newFeeTo;
    }

    function setProtocolFee(uint16 feeBps, address recipient) external onlyAdmin {
        protocolFeeBps = feeBps;
        protocolFeeRecipient = recipient;
    }

    function protocol_fee_config() external view returns (uint16, address) {
        return (protocolFeeBps, protocolFeeRecipient);
    }

    function fee_receiver() external pure returns (address) {
        return address(0);
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
}

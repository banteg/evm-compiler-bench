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

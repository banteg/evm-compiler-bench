// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract YearnVaultV3Real {
    uint256 public constant MAX_BPS = 10_000;
    uint256 public constant WAD = 1e18;

    string public constant name = "Real-Derived Yearn V3 Vault";
    string public constant symbol = "RD-YV3";
    uint8 public constant decimals = 18;

    address public roleManager;
    bool public initialized;
    bool public shutdown;
    uint256 public depositLimit;
    uint256 public profitMaxUnlockTime;
    uint256 public fullProfitUnlockDate;
    uint256 public profitUnlockingRate;
    uint256 public lastProfitUpdate;
    uint256 public currentTimestamp;
    uint256 public feeBps;
    uint256 public totalIdle;
    uint256 public totalDebt;
    uint256 public totalSupply;
    address public defaultQueueStrategy;

    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    mapping(address => uint256) public nonces;
    mapping(address => Strategy) public strategies;
    mapping(address => PendingReport) public pendingReports;

    struct Strategy {
        uint256 activation;
        uint256 currentDebt;
        uint256 maxDebt;
        uint256 balance;
    }

    struct PendingReport {
        uint256 gain;
        uint256 loss;
    }

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Deposit(address indexed sender, address indexed owner, uint256 assets, uint256 shares);
    event Withdraw(address indexed sender, address indexed receiver, address indexed owner, uint256 assets, uint256 shares);
    event StrategyChanged(address indexed strategy, uint256 activation);
    event DebtUpdated(address indexed strategy, uint256 currentDebt, uint256 newDebt);
    event StrategyReported(address indexed strategy, uint256 gain, uint256 loss, uint256 totalDebt, uint256 totalIdle);
    event Shutdown();

    modifier onlyManager() {
        require(msg.sender == roleManager, "permission");
        _;
    }

    modifier ready() {
        require(initialized, "not initialized");
        _;
    }

    function initialize(uint256 limit, uint256 unlockTime, uint256 feeBps_) external returns (bool) {
        require(!initialized, "initialized");
        require(feeBps_ <= MAX_BPS, "fee");
        initialized = true;
        roleManager = msg.sender;
        depositLimit = limit;
        profitMaxUnlockTime = unlockTime;
        feeBps = feeBps_;
        currentTimestamp = 1;
        lastProfitUpdate = 1;
        return true;
    }

    function advanceTime(uint256 secondsForward) external ready returns (uint256) {
        currentTimestamp += secondsForward;
        return currentTimestamp;
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

    function deposit(uint256 assets, address receiver) external ready returns (uint256 shares) {
        require(!shutdown, "shutdown");
        require(assets > 0, "assets");
        require(totalAssets() + assets <= depositLimit, "limit");
        shares = _convertToShares(assets, false);
        require(shares > 0, "shares");
        totalIdle += assets;
        _mint(receiver, shares);
        emit Deposit(msg.sender, receiver, assets, shares);
    }

    function mint(uint256 shares, address receiver) external ready returns (uint256 assets) {
        require(!shutdown, "shutdown");
        require(shares > 0, "shares");
        assets = _convertToAssets(shares, true);
        require(totalAssets() + assets <= depositLimit, "limit");
        totalIdle += assets;
        _mint(receiver, shares);
        emit Deposit(msg.sender, receiver, assets, shares);
    }

    function withdraw(uint256 assets, address receiver, address owner, uint256 maxLoss)
        external
        ready
        returns (uint256 shares)
    {
        shares = _convertToShares(assets, true);
        uint256 actualAssets = _redeem(receiver, owner, assets, shares, maxLoss);
        emit Withdraw(msg.sender, receiver, owner, actualAssets, shares);
    }

    function redeem(uint256 shares, address receiver, address owner, uint256 maxLoss)
        external
        ready
        returns (uint256 assets)
    {
        assets = _convertToAssets(shares, false);
        uint256 actualAssets = _redeem(receiver, owner, assets, shares, maxLoss);
        emit Withdraw(msg.sender, receiver, owner, actualAssets, shares);
        return actualAssets;
    }

    function add_strategy(address strategy, bool addToQueue) external ready onlyManager returns (bool) {
        require(strategy != address(0), "strategy");
        require(strategies[strategy].activation == 0, "active");
        strategies[strategy].activation = currentTimestamp;
        if (addToQueue) {
            defaultQueueStrategy = strategy;
        }
        emit StrategyChanged(strategy, currentTimestamp);
        return true;
    }

    function update_max_debt_for_strategy(address strategy, uint256 maxDebt) external ready onlyManager returns (bool) {
        require(strategies[strategy].activation != 0, "inactive");
        strategies[strategy].maxDebt = maxDebt;
        return true;
    }

    function update_debt(address strategy, uint256 targetDebt, uint256 maxLoss)
        external
        ready
        onlyManager
        returns (uint256)
    {
        Strategy storage s = strategies[strategy];
        require(s.activation != 0, "inactive");
        require(targetDebt <= s.maxDebt, "max");
        uint256 previousDebt = s.currentDebt;
        if (targetDebt > previousDebt) {
            uint256 debtIncrease = targetDebt - previousDebt;
            require(totalIdle >= debtIncrease, "idle");
            totalIdle -= debtIncrease;
            totalDebt += debtIncrease;
            s.currentDebt = targetDebt;
            s.balance += debtIncrease;
        } else {
            uint256 debtReduction = previousDebt - targetDebt;
            uint256 withdrawn = _min(debtReduction, s.balance);
            uint256 loss = debtReduction - withdrawn;
            require(debtReduction == 0 || loss * MAX_BPS <= debtReduction * maxLoss, "loss");
            s.balance -= withdrawn;
            s.currentDebt = targetDebt;
            totalDebt = totalDebt + loss - debtReduction;
            totalIdle += withdrawn;
        }
        emit DebtUpdated(strategy, previousDebt, s.currentDebt);
        return s.currentDebt;
    }

    function mock_strategy_report(address strategy, uint256 gain, uint256 loss) external ready onlyManager returns (bool) {
        require(strategies[strategy].activation != 0, "inactive");
        pendingReports[strategy] = PendingReport(gain, loss);
        return true;
    }

    function process_report(address strategy) external ready onlyManager returns (uint256 gain, uint256 loss) {
        Strategy storage s = strategies[strategy];
        require(s.activation != 0, "inactive");
        PendingReport memory report = pendingReports[strategy];
        gain = report.gain;
        loss = _min(report.loss, s.currentDebt);
        delete pendingReports[strategy];

        if (loss > 0) {
            s.currentDebt -= loss;
            s.balance = s.balance > loss ? s.balance - loss : 0;
            totalDebt -= loss;
        }
        if (gain > 0) {
            uint256 fee = gain * feeBps / MAX_BPS;
            uint256 netGain = gain - fee;
            uint256 sharesToLock = _convertToShares(netGain, false);
            s.currentDebt += gain;
            s.balance += gain;
            totalDebt += gain;
            if (sharesToLock > 0) {
                _mint(address(this), sharesToLock);
                if (profitMaxUnlockTime != 0) {
                    profitUnlockingRate = sharesToLock / profitMaxUnlockTime;
                    fullProfitUnlockDate = currentTimestamp + profitMaxUnlockTime;
                    lastProfitUpdate = currentTimestamp;
                }
            }
            if (fee > 0) {
                uint256 feeShares = _convertToShares(fee, false);
                if (feeShares > 0) {
                    _mint(roleManager, feeShares);
                }
            }
        }
        emit StrategyReported(strategy, gain, loss, totalDebt, totalIdle);
    }

    function shutdown_vault() external ready onlyManager returns (bool) {
        shutdown = true;
        depositLimit = 0;
        emit Shutdown();
        return true;
    }

    function permitDigest(address owner, address spender, uint256 value, uint256 deadline)
        external
        view
        returns (bytes32)
    {
        owner;
        spender;
        return keccak256(abi.encode(value, nonces[owner], deadline));
    }

    function totalAssets() public view returns (uint256) {
        return totalIdle + totalDebt;
    }

    function pricePerShare() external view returns (uint256) {
        uint256 supply = _effectiveSupply();
        if (supply == 0) {
            return WAD;
        }
        return totalAssets() * WAD / supply;
    }

    function maxDeposit(address) external view returns (uint256) {
        if (shutdown || totalAssets() >= depositLimit) {
            return 0;
        }
        return depositLimit - totalAssets();
    }

    function maxWithdraw(address owner, uint256) external view returns (uint256) {
        return _convertToAssets(balanceOf[owner], false);
    }

    function strategyState(address strategy)
        external
        view
        returns (uint256 activation, uint256 currentDebt, uint256 maxDebt, uint256 balance)
    {
        Strategy memory s = strategies[strategy];
        return (s.activation, s.currentDebt, s.maxDebt, s.balance);
    }

    function _redeem(address, address owner, uint256 assets, uint256 shares, uint256 maxLoss) internal returns (uint256) {
        require(shares > 0 && balanceOf[owner] >= shares, "shares");
        uint256 actualAssets = _ensureIdle(assets, maxLoss);
        _burn(owner, shares);
        totalIdle -= actualAssets;
        return actualAssets;
    }

    function _ensureIdle(uint256 assets, uint256 maxLoss) internal returns (uint256) {
        if (totalIdle >= assets) {
            return assets;
        }
        address strategy = defaultQueueStrategy;
        require(strategy != address(0), "queue");
        Strategy storage s = strategies[strategy];
        uint256 needed = assets - totalIdle;
        uint256 withdrawn = _min(needed, s.balance);
        s.balance -= withdrawn;
        s.currentDebt -= withdrawn;
        totalDebt -= withdrawn;
        totalIdle += withdrawn;
        if (totalIdle < assets) {
            uint256 loss = assets - totalIdle;
            require(loss * MAX_BPS <= assets * maxLoss, "loss");
            s.currentDebt -= _min(loss, s.currentDebt);
            totalDebt -= _min(loss, totalDebt);
            return totalIdle;
        }
        return assets;
    }

    function _effectiveSupply() internal view returns (uint256) {
        return totalSupply - _unlockedShares();
    }

    function _unlockedShares() internal view returns (uint256) {
        uint256 locked = balanceOf[address(this)];
        if (locked == 0) return 0;
        if (profitMaxUnlockTime == 0 || currentTimestamp >= fullProfitUnlockDate) return locked;
        uint256 elapsed = currentTimestamp - lastProfitUpdate;
        return _min(locked, elapsed * profitUnlockingRate);
    }

    function _convertToShares(uint256 assets, bool roundUp) internal view returns (uint256) {
        uint256 supply = _effectiveSupply();
        uint256 assetsTotal = totalAssets();
        if (supply == 0 || assetsTotal == 0) {
            return assets;
        }
        uint256 shares = assets * supply / assetsTotal;
        if (roundUp && shares * assetsTotal < assets * supply) {
            shares += 1;
        }
        return shares;
    }

    function _convertToAssets(uint256 shares, bool roundUp) internal view returns (uint256) {
        uint256 supply = _effectiveSupply();
        if (supply == 0) {
            return shares;
        }
        uint256 assets = shares * totalAssets() / supply;
        if (roundUp && assets * supply < shares * totalAssets()) {
            assets += 1;
        }
        return assets;
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

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) {
        return a < b ? a : b;
    }
}

// SPDX-License-Identifier: AGPL-3.0
pragma solidity ^0.8.35;

interface YearnV2ERC20 {
    function name() external view returns (string memory);
    function symbol() external view returns (string memory);
    function decimals() external view returns (uint8);
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

interface YearnV2Strategy {
    function want() external view returns (address);
    function vault() external view returns (address);
    function isActive() external view returns (bool);
    function delegatedAssets() external view returns (uint256);
    function estimatedTotalAssets() external view returns (uint256);
    function withdraw(uint256 amount) external returns (uint256);
    function migrate(address newStrategy) external;
    function emergencyExit() external view returns (bool);
}

contract YearnVaultV2Real {
    string internal constant API_VERSION = "0.4.6";
    uint256 public constant MAXIMUM_STRATEGIES = 20;
    uint256 internal constant DEGRADATION_COEFFICIENT = 1e18;
    uint256 internal constant MAX_BPS = 10_000;
    uint256 internal constant SECS_PER_YEAR = 31_556_952;
    bytes32 internal constant DOMAIN_TYPE_HASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    bytes32 internal constant PERMIT_TYPE_HASH =
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    struct StrategyParams {
        uint256 performanceFee;
        uint256 activation;
        uint256 debtRatio;
        uint256 minDebtPerHarvest;
        uint256 maxDebtPerHarvest;
        uint256 lastReport;
        uint256 totalDebt;
        uint256 totalGain;
        uint256 totalLoss;
    }

    string public name;
    string public symbol;
    uint256 public decimals;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 public totalSupply;
    address public token;
    address public governance;
    address public management;
    address public guardian;
    address public pendingGovernance;
    mapping(address => StrategyParams) public strategies;
    address[MAXIMUM_STRATEGIES] public withdrawalQueue;
    bool public emergencyShutdown;
    uint256 public depositLimit;
    uint256 public debtRatio;
    uint256 public totalIdle;
    uint256 public totalDebt;
    uint256 public lastReport;
    uint256 public activation;
    uint256 public lockedProfit;
    uint256 public lockedProfitDegradation;
    address public rewards;
    uint256 public managementFee;
    uint256 public performanceFee;
    mapping(address => uint256) public nonces;
    uint256 private nonreentrantLock;

    event Transfer(address indexed sender, address indexed receiver, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Deposit(address indexed recipient, uint256 shares, uint256 amount);
    event Withdraw(address indexed recipient, uint256 shares, uint256 amount);
    event Sweep(address indexed token, uint256 amount);
    event LockedProfitDegradationUpdated(uint256 value);
    event StrategyAdded(
        address indexed strategy,
        uint256 debtRatio,
        uint256 minDebtPerHarvest,
        uint256 maxDebtPerHarvest,
        uint256 performanceFee
    );
    event StrategyReported(
        address indexed strategy,
        uint256 gain,
        uint256 loss,
        uint256 debtPaid,
        uint256 totalGain,
        uint256 totalLoss,
        uint256 totalDebt,
        uint256 debtAdded,
        uint256 debtRatio
    );
    event FeeReport(uint256 management_fee, uint256 performance_fee, uint256 strategist_fee, uint256 duration);
    event WithdrawFromStrategy(address indexed strategy, uint256 totalDebt, uint256 loss);
    event UpdateGovernance(address governance);
    event UpdateManagement(address management);
    event UpdateRewards(address rewards);
    event UpdateDepositLimit(uint256 depositLimit);
    event UpdatePerformanceFee(uint256 performanceFee);
    event UpdateManagementFee(uint256 managementFee);
    event UpdateGuardian(address guardian);
    event EmergencyShutdown(bool active);
    event UpdateWithdrawalQueue(address[MAXIMUM_STRATEGIES] queue);
    event StrategyUpdateDebtRatio(address indexed strategy, uint256 debtRatio);
    event StrategyUpdateMinDebtPerHarvest(address indexed strategy, uint256 minDebtPerHarvest);
    event StrategyUpdateMaxDebtPerHarvest(address indexed strategy, uint256 maxDebtPerHarvest);
    event StrategyUpdatePerformanceFee(address indexed strategy, uint256 performanceFee);
    event StrategyMigrated(address indexed oldVersion, address indexed newVersion);
    event StrategyRevoked(address indexed strategy);
    event StrategyRemovedFromQueue(address indexed strategy);
    event StrategyAddedToQueue(address indexed strategy);
    event NewPendingGovernance(address indexed pendingGovernance);

    modifier nonReentrant() {
        require(nonreentrantLock != 2, "reentrant call");
        nonreentrantLock = 2;
        _;
        nonreentrantLock = 3;
    }

    function initialize(
        address token_,
        address governance_,
        address rewards_,
        string memory nameOverride,
        string memory symbolOverride
    ) external {
        _initialize(token_, governance_, rewards_, nameOverride, symbolOverride, msg.sender, msg.sender);
    }

    function initialize(
        address token_,
        address governance_,
        address rewards_,
        string memory nameOverride,
        string memory symbolOverride,
        address guardian_
    ) external {
        _initialize(token_, governance_, rewards_, nameOverride, symbolOverride, guardian_, msg.sender);
    }

    function initialize(
        address token_,
        address governance_,
        address rewards_,
        string memory nameOverride,
        string memory symbolOverride,
        address guardian_,
        address management_
    ) external {
        _initialize(token_, governance_, rewards_, nameOverride, symbolOverride, guardian_, management_);
    }

    function _initialize(
        address token_,
        address governance_,
        address rewards_,
        string memory nameOverride,
        string memory symbolOverride,
        address guardian_,
        address management_
    ) internal {
        require(activation == 0, "initialized");
        token = token_;
        string memory baseSymbol = YearnV2ERC20(token_).symbol();
        name = bytes(nameOverride).length == 0 ? string(abi.encodePacked(baseSymbol, " yVault")) : nameOverride;
        symbol = bytes(symbolOverride).length == 0 ? string(abi.encodePacked("yv", baseSymbol)) : symbolOverride;
        uint256 decimals_ = YearnV2ERC20(token_).decimals();
        require(decimals_ < 256, "decimals");
        decimals = decimals_;
        governance = governance_;
        emit UpdateGovernance(governance_);
        management = management_;
        emit UpdateManagement(management_);
        rewards = rewards_;
        emit UpdateRewards(rewards_);
        guardian = guardian_;
        emit UpdateGuardian(guardian_);
        performanceFee = 1000;
        emit UpdatePerformanceFee(1000);
        managementFee = 200;
        emit UpdateManagementFee(200);
        lastReport = block.timestamp;
        activation = block.timestamp;
        lockedProfitDegradation = DEGRADATION_COEFFICIENT * 46 / 1_000_000;
    }

    function apiVersion() external pure returns (string memory) {
        return API_VERSION;
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return _domainSeparator();
    }

    function _domainSeparator() internal view returns (bytes32) {
        return keccak256(
            abi.encode(DOMAIN_TYPE_HASH, keccak256("Yearn Vault"), keccak256(bytes(API_VERSION)), block.chainid, address(this))
        );
    }

    function setName(string memory name_) external {
        require(msg.sender == governance);
        name = name_;
    }

    function setSymbol(string memory symbol_) external {
        require(msg.sender == governance);
        symbol = symbol_;
    }

    function setGovernance(address governance_) external {
        require(msg.sender == governance);
        emit NewPendingGovernance(governance_);
        pendingGovernance = governance_;
    }

    function acceptGovernance() external {
        require(msg.sender == pendingGovernance);
        governance = msg.sender;
        emit UpdateGovernance(msg.sender);
    }

    function setManagement(address management_) external {
        require(msg.sender == governance);
        management = management_;
        emit UpdateManagement(management_);
    }

    function setRewards(address rewards_) external {
        require(msg.sender == governance);
        require(rewards_ != address(this) && rewards_ != address(0));
        rewards = rewards_;
        emit UpdateRewards(rewards_);
    }

    function setLockedProfitDegradation(uint256 degradation) external {
        require(msg.sender == governance);
        require(degradation <= DEGRADATION_COEFFICIENT);
        lockedProfitDegradation = degradation;
        emit LockedProfitDegradationUpdated(degradation);
    }

    function setDepositLimit(uint256 limit) external {
        require(msg.sender == governance || msg.sender == management);
        depositLimit = limit;
        emit UpdateDepositLimit(limit);
    }

    function setPerformanceFee(uint256 fee) external {
        require(msg.sender == governance);
        require(fee <= MAX_BPS / 2);
        performanceFee = fee;
        emit UpdatePerformanceFee(fee);
    }

    function setManagementFee(uint256 fee) external {
        require(msg.sender == governance);
        require(fee <= MAX_BPS);
        managementFee = fee;
        emit UpdateManagementFee(fee);
    }

    function setGuardian(address guardian_) external {
        require(msg.sender == governance || msg.sender == guardian);
        guardian = guardian_;
        emit UpdateGuardian(guardian_);
    }

    function setEmergencyShutdown(bool active) external {
        if (active) {
            require(msg.sender == governance || msg.sender == guardian);
        } else {
            require(msg.sender == governance);
        }
        emergencyShutdown = active;
        emit EmergencyShutdown(active);
    }

    function setWithdrawalQueue(address[MAXIMUM_STRATEGIES] calldata queue) external {
        require(msg.sender == governance || msg.sender == management);
        address[MAXIMUM_STRATEGIES] memory oldQueue = withdrawalQueue;
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            if (queue[i] == address(0)) {
                require(oldQueue[i] == address(0), "cannot remove");
                break;
            }
            require(oldQueue[i] != address(0), "new strategy");
            bool existsInOldQueue;
            for (uint256 j = 0; j < MAXIMUM_STRATEGIES; j++) {
                if (queue[j] == address(0)) break;
                if (queue[i] == oldQueue[j]) {
                    existsInOldQueue = true;
                    break;
                }
            }
            require(existsInOldQueue, "unknown strategy");
            withdrawalQueue[i] = queue[i];
        }
        emit UpdateWithdrawalQueue(queue);
    }

    function transfer(address receiver, uint256 amount) external returns (bool) {
        _transfer(msg.sender, receiver, amount);
        return true;
    }

    function transferFrom(address sender, address receiver, uint256 amount) external returns (bool) {
        uint256 allowed = allowance[sender][msg.sender];
        if (allowed < type(uint256).max) {
            require(allowed >= amount, "allowance");
            unchecked {
                allowance[sender][msg.sender] = allowed - amount;
            }
            emit Approval(sender, msg.sender, allowance[sender][msg.sender]);
        }
        _transfer(sender, receiver, amount);
        return true;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function increaseAllowance(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] += amount;
        emit Approval(msg.sender, spender, allowance[msg.sender][spender]);
        return true;
    }

    function decreaseAllowance(address spender, uint256 amount) external returns (bool) {
        require(allowance[msg.sender][spender] >= amount, "allowance");
        allowance[msg.sender][spender] -= amount;
        emit Approval(msg.sender, spender, allowance[msg.sender][spender]);
        return true;
    }

    function permit(address owner, address spender, uint256 amount, uint256 expiry, bytes calldata signature)
        external
        returns (bool)
    {
        require(owner != address(0), "owner");
        require(expiry >= block.timestamp, "expired");
        require(signature.length == 65, "signature");
        uint256 nonce = nonces[owner];
        bytes32 digest = keccak256(
            abi.encodePacked(
                "\x19\x01",
                _domainSeparator(),
                keccak256(abi.encode(PERMIT_TYPE_HASH, owner, spender, amount, nonce, expiry))
            )
        );
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := calldataload(signature.offset)
            s := calldataload(add(signature.offset, 32))
            v := byte(0, calldataload(add(signature.offset, 64)))
        }
        require(ecrecover(digest, v, r, s) == owner, "permit");
        allowance[owner][spender] = amount;
        nonces[owner] = nonce + 1;
        emit Approval(owner, spender, amount);
        return true;
    }

    function totalAssets() external view returns (uint256) {
        return _totalAssets();
    }

    function _totalAssets() internal view returns (uint256) {
        return totalIdle + totalDebt;
    }

    function deposit() external returns (uint256) {
        return _deposit(type(uint256).max, msg.sender);
    }

    function deposit(uint256 amount) external returns (uint256) {
        return _deposit(amount, msg.sender);
    }

    function deposit(uint256 amount, address recipient) external nonReentrant returns (uint256) {
        return _deposit(amount, recipient);
    }

    function _deposit(uint256 amount, address recipient) internal returns (uint256 shares) {
        require(!emergencyShutdown, "shutdown");
        require(recipient != address(this) && recipient != address(0), "recipient");
        if (amount == type(uint256).max) {
            amount = _min(depositLimit - _totalAssets(), YearnV2ERC20(token).balanceOf(msg.sender));
        } else {
            require(_totalAssets() + amount <= depositLimit, "deposit limit");
        }
        require(amount > 0, "amount");
        shares = _issueSharesForAmount(recipient, amount);
        _safeTransferFrom(token, msg.sender, address(this), amount);
        totalIdle += amount;
        emit Deposit(recipient, shares, amount);
    }

    function _issueSharesForAmount(address to, uint256 amount) internal returns (uint256 shares) {
        uint256 supply = totalSupply;
        if (supply > 0) {
            shares = amount * supply / _freeFunds();
        } else {
            shares = amount;
        }
        require(shares != 0, "shares");
        totalSupply += shares;
        balanceOf[to] += shares;
        emit Transfer(address(0), to, shares);
    }

    function _calculateLockedProfit() internal view returns (uint256) {
        uint256 lockedFundsRatio = (block.timestamp - lastReport) * lockedProfitDegradation;
        if (lockedFundsRatio < DEGRADATION_COEFFICIENT) {
            return lockedProfit - lockedFundsRatio * lockedProfit / DEGRADATION_COEFFICIENT;
        }
        return 0;
    }

    function _freeFunds() internal view returns (uint256) {
        return _totalAssets() - _calculateLockedProfit();
    }

    function pricePerShare() external view returns (uint256) {
        return _shareValue(10 ** decimals);
    }

    function _shareValue(uint256 shares) internal view returns (uint256) {
        if (totalSupply == 0) return shares;
        return shares * _freeFunds() / totalSupply;
    }

    function _sharesForAmount(uint256 amount) internal view returns (uint256) {
        uint256 freeFunds = _freeFunds();
        if (freeFunds > 0) return amount * totalSupply / freeFunds;
        return 0;
    }

    function maxAvailableShares() external view returns (uint256) {
        uint256 value = totalIdle;
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            address strategy = withdrawalQueue[i];
            if (strategy == address(0)) break;
            value += strategies[strategy].totalDebt;
        }
        return _sharesForAmount(value);
    }

    function withdraw() external returns (uint256) {
        return _withdraw(type(uint256).max, msg.sender, 1);
    }

    function withdraw(uint256 maxShares) external returns (uint256) {
        return _withdraw(maxShares, msg.sender, 1);
    }

    function withdraw(uint256 maxShares, address recipient) external returns (uint256) {
        return _withdraw(maxShares, recipient, 1);
    }

    function withdraw(uint256 maxShares, address recipient, uint256 maxLoss) external nonReentrant returns (uint256) {
        return _withdraw(maxShares, recipient, maxLoss);
    }

    function _withdraw(uint256 maxShares, address recipient, uint256 maxLoss) internal returns (uint256 value) {
        require(maxLoss <= MAX_BPS, "max loss");
        uint256 shares = maxShares == type(uint256).max ? balanceOf[msg.sender] : maxShares;
        require(shares <= balanceOf[msg.sender], "shares");
        value = _shareValue(shares);
        if (value > totalIdle) {
            uint256 totalLoss;
            uint256 vaultBalance = totalIdle;
            for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
                address strategy = withdrawalQueue[i];
                if (strategy == address(0)) break;
                uint256 amountNeeded = value - vaultBalance;
                amountNeeded = _min(amountNeeded, strategies[strategy].totalDebt);
                if (amountNeeded == 0) continue;
                uint256 preBalance = YearnV2ERC20(token).balanceOf(address(this));
                uint256 loss = YearnV2Strategy(strategy).withdraw(amountNeeded);
                uint256 withdrawn = YearnV2ERC20(token).balanceOf(address(this)) - preBalance;
                vaultBalance += withdrawn;
                if (loss > 0) {
                    value -= loss;
                    totalLoss += loss;
                    _reportLoss(strategy, loss);
                }
                strategies[strategy].totalDebt -= withdrawn;
                totalDebt -= withdrawn;
                emit WithdrawFromStrategy(strategy, strategies[strategy].totalDebt, loss);
                if (vaultBalance >= value) break;
            }
            require(totalLoss <= maxLoss * (value + totalLoss) / MAX_BPS, "loss");
            totalIdle = vaultBalance;
        }
        totalSupply -= shares;
        balanceOf[msg.sender] -= shares;
        emit Transfer(msg.sender, address(0), shares);
        totalIdle -= value;
        _safeTransfer(token, recipient, value);
        emit Withdraw(recipient, shares, value);
    }

    function addStrategy(
        address strategy,
        uint256 debtRatio_,
        uint256 minDebtPerHarvest,
        uint256 maxDebtPerHarvest,
        uint256 performanceFee_
    ) external {
        require(withdrawalQueue[MAXIMUM_STRATEGIES - 1] == address(0), "queue full");
        require(!emergencyShutdown, "shutdown");
        require(msg.sender == governance, "governance");
        require(strategy != address(0), "strategy");
        require(strategies[strategy].activation == 0, "active");
        require(address(this) == YearnV2Strategy(strategy).vault(), "vault");
        require(token == YearnV2Strategy(strategy).want(), "want");
        require(debtRatio + debtRatio_ <= MAX_BPS, "debt ratio");
        require(minDebtPerHarvest <= maxDebtPerHarvest, "debt harvest");
        require(performanceFee_ <= MAX_BPS / 2, "performance fee");
        strategies[strategy] = StrategyParams({
            performanceFee: performanceFee_,
            activation: block.timestamp,
            debtRatio: debtRatio_,
            minDebtPerHarvest: minDebtPerHarvest,
            maxDebtPerHarvest: maxDebtPerHarvest,
            lastReport: block.timestamp,
            totalDebt: 0,
            totalGain: 0,
            totalLoss: 0
        });
        emit StrategyAdded(strategy, debtRatio_, minDebtPerHarvest, maxDebtPerHarvest, performanceFee_);
        debtRatio += debtRatio_;
        withdrawalQueue[MAXIMUM_STRATEGIES - 1] = strategy;
        _organizeWithdrawalQueue();
    }

    function updateStrategyDebtRatio(address strategy, uint256 debtRatio_) external {
        require(msg.sender == management || msg.sender == governance, "auth");
        require(strategies[strategy].activation > 0, "inactive");
        require(!YearnV2Strategy(strategy).emergencyExit(), "emergency");
        debtRatio -= strategies[strategy].debtRatio;
        strategies[strategy].debtRatio = debtRatio_;
        debtRatio += debtRatio_;
        require(debtRatio <= MAX_BPS, "debt ratio");
        emit StrategyUpdateDebtRatio(strategy, debtRatio_);
    }

    function updateStrategyMinDebtPerHarvest(address strategy, uint256 minDebtPerHarvest) external {
        require(msg.sender == management || msg.sender == governance, "auth");
        require(strategies[strategy].activation > 0, "inactive");
        require(minDebtPerHarvest <= strategies[strategy].maxDebtPerHarvest, "min");
        strategies[strategy].minDebtPerHarvest = minDebtPerHarvest;
        emit StrategyUpdateMinDebtPerHarvest(strategy, minDebtPerHarvest);
    }

    function updateStrategyMaxDebtPerHarvest(address strategy, uint256 maxDebtPerHarvest) external {
        require(msg.sender == management || msg.sender == governance, "auth");
        require(strategies[strategy].activation > 0, "inactive");
        require(strategies[strategy].minDebtPerHarvest <= maxDebtPerHarvest, "max");
        strategies[strategy].maxDebtPerHarvest = maxDebtPerHarvest;
        emit StrategyUpdateMaxDebtPerHarvest(strategy, maxDebtPerHarvest);
    }

    function updateStrategyPerformanceFee(address strategy, uint256 performanceFee_) external {
        require(msg.sender == governance, "governance");
        require(strategies[strategy].activation > 0, "inactive");
        require(performanceFee_ <= MAX_BPS / 2, "performance");
        strategies[strategy].performanceFee = performanceFee_;
        emit StrategyUpdatePerformanceFee(strategy, performanceFee_);
    }

    function _revokeStrategy(address strategy) internal {
        debtRatio -= strategies[strategy].debtRatio;
        strategies[strategy].debtRatio = 0;
        emit StrategyRevoked(strategy);
    }

    function migrateStrategy(address oldVersion, address newVersion) external {
        require(msg.sender == governance, "governance");
        require(newVersion != address(0), "new");
        require(strategies[oldVersion].activation > 0, "old");
        require(strategies[newVersion].activation == 0, "new active");
        require(YearnV2Strategy(newVersion).vault() == address(this), "vault");
        require(YearnV2Strategy(newVersion).want() == token, "want");
        StrategyParams memory strategy = strategies[oldVersion];
        _revokeStrategy(oldVersion);
        debtRatio += strategy.debtRatio;
        strategies[oldVersion].totalDebt = 0;
        strategies[newVersion] = StrategyParams({
            performanceFee: strategy.performanceFee,
            activation: strategy.lastReport,
            debtRatio: strategy.debtRatio,
            minDebtPerHarvest: strategy.minDebtPerHarvest,
            maxDebtPerHarvest: strategy.maxDebtPerHarvest,
            lastReport: strategy.lastReport,
            totalDebt: strategy.totalDebt,
            totalGain: 0,
            totalLoss: 0
        });
        YearnV2Strategy(oldVersion).migrate(newVersion);
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            if (withdrawalQueue[i] == oldVersion) {
                withdrawalQueue[i] = newVersion;
            }
        }
        emit StrategyMigrated(oldVersion, newVersion);
    }

    function revokeStrategy() external {
        _revokeStrategy(msg.sender);
    }

    function revokeStrategy(address strategy) external {
        require(msg.sender == governance || msg.sender == guardian || msg.sender == strategy, "auth");
        _revokeStrategy(strategy);
    }

    function addStrategyToQueue(address strategy) external {
        require(msg.sender == governance || msg.sender == management, "auth");
        require(strategies[strategy].activation > 0, "inactive");
        bool foundOpenSlot;
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            address current = withdrawalQueue[i];
            if (current == address(0)) {
                foundOpenSlot = true;
                break;
            }
            require(current != strategy, "duplicate");
        }
        require(foundOpenSlot, "queue full");
        withdrawalQueue[MAXIMUM_STRATEGIES - 1] = strategy;
        _organizeWithdrawalQueue();
        emit StrategyAddedToQueue(strategy);
    }

    function removeStrategyFromQueue(address strategy) external {
        require(msg.sender == governance || msg.sender == management, "auth");
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            if (withdrawalQueue[i] == strategy) {
                withdrawalQueue[i] = address(0);
                _organizeWithdrawalQueue();
                emit StrategyRemovedFromQueue(strategy);
                return;
            }
        }
        revert("not queued");
    }

    function _organizeWithdrawalQueue() internal {
        uint256 offset;
        for (uint256 i = 0; i < MAXIMUM_STRATEGIES; i++) {
            address strategy = withdrawalQueue[i];
            if (strategy == address(0)) {
                offset += 1;
            } else if (offset > 0) {
                withdrawalQueue[i - offset] = strategy;
                withdrawalQueue[i] = address(0);
            }
        }
    }

    function debtOutstanding() external view returns (uint256) {
        return _debtOutstanding(msg.sender);
    }

    function debtOutstanding(address strategy) external view returns (uint256) {
        return _debtOutstanding(strategy);
    }

    function _debtOutstanding(address strategy) internal view returns (uint256) {
        uint256 strategyDebtLimit = strategies[strategy].debtRatio * _totalAssets() / MAX_BPS;
        uint256 strategyTotalDebt = strategies[strategy].totalDebt;
        if (emergencyShutdown) return strategyTotalDebt;
        if (strategyTotalDebt <= strategyDebtLimit) return 0;
        return strategyTotalDebt - strategyDebtLimit;
    }

    function creditAvailable() external view returns (uint256) {
        return _creditAvailable(msg.sender);
    }

    function creditAvailable(address strategy) external view returns (uint256) {
        return _creditAvailable(strategy);
    }

    function _creditAvailable(address strategy) internal view returns (uint256) {
        if (emergencyShutdown) return 0;
        uint256 vaultTotalAssets = _totalAssets();
        uint256 vaultDebtLimit = debtRatio * vaultTotalAssets / MAX_BPS;
        uint256 strategyDebtLimit = strategies[strategy].debtRatio * vaultTotalAssets / MAX_BPS;
        uint256 strategyTotalDebt = strategies[strategy].totalDebt;
        if (strategyDebtLimit <= strategyTotalDebt || vaultDebtLimit <= totalDebt) return 0;
        uint256 available = strategyDebtLimit - strategyTotalDebt;
        available = _min(available, vaultDebtLimit - totalDebt);
        available = _min(available, totalIdle);
        if (available < strategies[strategy].minDebtPerHarvest) return 0;
        return _min(available, strategies[strategy].maxDebtPerHarvest);
    }

    function availableDepositLimit() external view returns (uint256) {
        uint256 assets = _totalAssets();
        return depositLimit > assets ? depositLimit - assets : 0;
    }

    function expectedReturn() external view returns (uint256) {
        return _expectedReturn(msg.sender);
    }

    function expectedReturn(address strategy) external view returns (uint256) {
        return _expectedReturn(strategy);
    }

    function _expectedReturn(address strategy) internal view returns (uint256) {
        uint256 strategyLastReport = strategies[strategy].lastReport;
        uint256 timeSinceLastHarvest = block.timestamp - strategyLastReport;
        uint256 totalHarvestTime = strategyLastReport - strategies[strategy].activation;
        if (timeSinceLastHarvest > 0 && totalHarvestTime > 0 && YearnV2Strategy(strategy).isActive()) {
            return strategies[strategy].totalGain * timeSinceLastHarvest / totalHarvestTime;
        }
        return 0;
    }

    function report(uint256 gain, uint256 loss, uint256 debtPayment_) external returns (uint256) {
        require(strategies[msg.sender].activation > 0, "inactive");
        require(YearnV2ERC20(token).balanceOf(msg.sender) >= gain + debtPayment_, "available");
        if (loss > 0) {
            _reportLoss(msg.sender, loss);
        }
        uint256 totalFees = _assessFees(msg.sender, gain);
        strategies[msg.sender].totalGain += gain;
        uint256 credit = _creditAvailable(msg.sender);
        uint256 debt = _debtOutstanding(msg.sender);
        uint256 debtPayment = _min(debtPayment_, debt);
        if (debtPayment > 0) {
            strategies[msg.sender].totalDebt -= debtPayment;
            totalDebt -= debtPayment;
            debt -= debtPayment;
        }
        if (credit > 0) {
            strategies[msg.sender].totalDebt += credit;
            totalDebt += credit;
        }
        uint256 totalAvail = gain + debtPayment;
        if (totalAvail < credit) {
            totalIdle -= credit - totalAvail;
            _safeTransfer(token, msg.sender, credit - totalAvail);
        } else if (totalAvail > credit) {
            totalIdle += totalAvail - credit;
            _safeTransferFrom(token, msg.sender, address(this), totalAvail - credit);
        }
        uint256 lockedProfitBeforeLoss = _calculateLockedProfit() + gain - totalFees;
        lockedProfit = lockedProfitBeforeLoss > loss ? lockedProfitBeforeLoss - loss : 0;
        strategies[msg.sender].lastReport = block.timestamp;
        lastReport = block.timestamp;
        emit StrategyReported(
            msg.sender,
            gain,
            loss,
            debtPayment,
            strategies[msg.sender].totalGain,
            strategies[msg.sender].totalLoss,
            strategies[msg.sender].totalDebt,
            credit,
            strategies[msg.sender].debtRatio
        );
        if (strategies[msg.sender].debtRatio == 0 || emergencyShutdown) {
            return YearnV2Strategy(msg.sender).estimatedTotalAssets();
        }
        return debt;
    }

    function _reportLoss(address strategy, uint256 loss) internal {
        uint256 strategyDebt = strategies[strategy].totalDebt;
        if (loss > strategyDebt) {
            loss = strategyDebt;
        }
        if (totalDebt > 0) {
            uint256 ratioChange = loss * debtRatio / totalDebt;
            debtRatio -= ratioChange;
        }
        strategies[strategy].totalLoss += loss;
        strategies[strategy].totalDebt -= loss;
        totalDebt -= loss;
    }

    function _assessFees(address strategy, uint256 gain) internal returns (uint256 totalFee) {
        if (strategies[strategy].activation == block.timestamp) return 0;
        uint256 duration = block.timestamp - strategies[strategy].lastReport;
        require(duration != 0, "duration");
        if (gain == 0) return 0;
        uint256 managementFee_ =
            (strategies[strategy].totalDebt - YearnV2Strategy(strategy).delegatedAssets()) * duration * managementFee
                / MAX_BPS / SECS_PER_YEAR;
        uint256 strategistFee = gain * strategies[strategy].performanceFee / MAX_BPS;
        uint256 performanceFee_ = gain * performanceFee / MAX_BPS;
        totalFee = managementFee_ + strategistFee + performanceFee_;
        if (totalFee > gain) {
            totalFee = gain;
        }
        if (totalFee > 0) {
            uint256 reward = _issueSharesForAmount(address(this), totalFee);
            if (strategistFee > 0) {
                uint256 strategistReward = strategistFee * reward / totalFee;
                _transfer(address(this), strategy, strategistReward);
            }
            if (balanceOf[address(this)] > 0) {
                _transfer(address(this), rewards, balanceOf[address(this)]);
            }
        }
        emit FeeReport(managementFee_, performanceFee_, strategistFee, duration);
    }

    function sweep(address token_) external {
        _sweep(token_, type(uint256).max);
    }

    function sweep(address token_, uint256 amount) external {
        _sweep(token_, amount);
    }

    function _sweep(address token_, uint256 amount) internal {
        require(msg.sender == governance, "governance");
        uint256 value = amount;
        if (value == type(uint256).max) {
            value = YearnV2ERC20(token_).balanceOf(address(this));
        }
        if (token_ == token) {
            value = YearnV2ERC20(token).balanceOf(address(this)) - totalIdle;
        }
        emit Sweep(token_, value);
        _safeTransfer(token_, governance, value);
    }

    function _transfer(address sender, address receiver, uint256 amount) internal {
        require(receiver != address(this) && receiver != address(0), "receiver");
        require(balanceOf[sender] >= amount, "balance");
        balanceOf[sender] -= amount;
        balanceOf[receiver] += amount;
        emit Transfer(sender, receiver, amount);
    }

    function _safeTransfer(address token_, address receiver, uint256 amount) internal {
        (bool ok, bytes memory data) =
            token_.call(abi.encodeWithSelector(YearnV2ERC20.transfer.selector, receiver, amount));
        require(ok && (data.length == 0 || abi.decode(data, (bool))), "Transfer failed!");
    }

    function _safeTransferFrom(address token_, address sender, address receiver, uint256 amount) internal {
        (bool ok, bytes memory data) =
            token_.call(abi.encodeWithSelector(YearnV2ERC20.transferFrom.selector, sender, receiver, amount));
        require(ok && (data.length == 0 || abi.decode(data, (bool))), "Transfer failed!");
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) {
        return a < b ? a : b;
    }
}

// SPDX-License-Identifier: AGPL-3.0
pragma solidity ^0.8.30;

interface YearnBenchERC20 {
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
    function decimals() external view returns (uint8);
    function approve(address spender, uint256 value) external returns (bool);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

interface YearnBenchStrategy {
    function asset() external view returns (address);
    function balanceOf(address account) external view returns (uint256);
    function convertToAssets(uint256 shares) external view returns (uint256);
    function previewWithdraw(uint256 assets) external view returns (uint256);
    function maxDeposit(address receiver) external view returns (uint256);
    function deposit(uint256 assets, address receiver) external returns (uint256);
    function maxRedeem(address owner) external view returns (uint256);
    function redeem(uint256 shares, address receiver, address owner) external returns (uint256);
}

interface YearnBenchAccountant {
    function report(address strategy, uint256 gain, uint256 loss)
        external
        returns (uint256 totalFees, uint256 totalRefunds);
}

interface YearnBenchDepositLimitModule {
    function available_deposit_limit(address receiver) external view returns (uint256);
}

interface YearnBenchWithdrawLimitModule {
    function available_withdraw_limit(address owner, uint256 maxLoss, address[] calldata strategies)
        external
        view
        returns (uint256);
}

interface YearnBenchFactory {
    function protocol_fee_config() external view returns (uint16, address);
}

contract YearnVaultV3Real {
    uint256 internal constant MAX_QUEUE = 10;
    uint256 internal constant MAX_BPS = 10_000;
    uint256 internal constant MAX_BPS_EXTENDED = 1_000_000_000_000;
    uint256 internal constant ADD_STRATEGY_MANAGER = 1 << 0;
    uint256 internal constant REVOKE_STRATEGY_MANAGER = 1 << 1;
    uint256 internal constant FORCE_REVOKE_MANAGER = 1 << 2;
    uint256 internal constant ACCOUNTANT_MANAGER = 1 << 3;
    uint256 internal constant QUEUE_MANAGER = 1 << 4;
    uint256 internal constant REPORTING_MANAGER = 1 << 5;
    uint256 internal constant DEBT_MANAGER = 1 << 6;
    uint256 internal constant MAX_DEBT_MANAGER = 1 << 7;
    uint256 internal constant DEPOSIT_LIMIT_MANAGER = 1 << 8;
    uint256 internal constant WITHDRAW_LIMIT_MANAGER = 1 << 9;
    uint256 internal constant MINIMUM_IDLE_MANAGER = 1 << 10;
    uint256 internal constant PROFIT_UNLOCK_MANAGER = 1 << 11;
    uint256 internal constant DEBT_PURCHASER = 1 << 12;
    uint256 internal constant EMERGENCY_MANAGER = 1 << 13;
    uint256 internal constant ALL_ROLES = (1 << 14) - 1;
    uint256 internal constant STRATEGY_CHANGE_ADDED = 1;
    uint256 internal constant STRATEGY_CHANGE_REVOKED = 2;
    string internal constant API_VERSION = "3.0.4";
    bytes32 internal constant DOMAIN_TYPE_HASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    bytes32 internal constant PERMIT_TYPE_HASH =
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    struct StrategyParams {
        uint256 activation;
        uint256 lastReport;
        uint256 currentDebt;
        uint256 maxDebt;
    }

    struct RedeemQueueState {
        uint256 requestedAssets;
        uint256 currentTotalIdle;
        uint256 currentTotalDebt;
        uint256 assetsNeeded;
        uint256 previousBalance;
    }

    struct ReportState {
        uint256 totalAssets_;
        uint256 currentDebt;
        uint256 gain;
        uint256 loss;
        uint256 totalFees;
        uint256 totalRefunds;
        uint256 totalFeesShares;
        uint16 protocolFeeBps;
        uint256 protocolFeesShares;
        address protocolFeeRecipient;
        address accountant_;
        uint256 sharesToBurn;
        uint256 sharesToLock;
        uint256 profitMaxUnlockTime_;
    }

    uint256 internal nonreentrant_lock;
    address public asset;
    uint8 public decimals;
    uint248 private __decimals_padding;
    address internal factory;
    mapping(address => StrategyParams) internal _strategies;
    uint256 internal default_queue_length;
    address[MAX_QUEUE] internal default_queue_storage;
    bool public use_default_queue;
    uint248 private __use_default_queue_padding;
    bool public auto_allocate;
    uint248 private __auto_allocate_padding;

    mapping(address => uint256) internal _balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    uint256 internal _totalSupply;
    uint256 internal total_debt;
    uint256 internal total_idle;
    uint256 public minimum_total_idle;
    uint256 public deposit_limit;

    address public accountant;
    address public deposit_limit_module;
    address public withdraw_limit_module;

    mapping(address => uint256) public roles;
    address public role_manager;
    address public future_role_manager;

    string public name;
    string public symbol;
    bool internal shutdown;
    uint256 internal profit_max_unlock_time;
    uint256 internal full_profit_unlock_date;
    uint256 internal profit_unlocking_rate;
    uint256 internal last_profit_update;

    mapping(address => uint256) public nonces;

    event Transfer(address indexed sender, address indexed receiver, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Deposit(address indexed sender, address indexed owner, uint256 assets, uint256 shares);
    event Withdraw(
        address indexed sender, address indexed receiver, address indexed owner, uint256 assets, uint256 shares
    );
    event StrategyChanged(address indexed strategy, uint256 indexed changeType);
    event StrategyReported(
        address indexed strategy,
        uint256 gain,
        uint256 loss,
        uint256 currentDebt,
        uint256 protocolFees,
        uint256 totalFees,
        uint256 totalRefunds
    );
    event DebtUpdated(address indexed strategy, uint256 currentDebt, uint256 newDebt);
    event RoleSet(address indexed account, uint256 indexed role);
    event UpdateFutureRoleManager(address indexed futureRoleManager);
    event UpdateRoleManager(address indexed roleManager);
    event UpdateAccountant(address indexed accountant);
    event UpdateDepositLimitModule(address indexed depositLimitModule);
    event UpdateWithdrawLimitModule(address indexed withdrawLimitModule);
    event UpdateDefaultQueue(address[] newDefaultQueue);
    event UpdateUseDefaultQueue(bool useDefaultQueue);
    event UpdateAutoAllocate(bool autoAllocate);
    event UpdatedMaxDebtForStrategy(address indexed sender, address indexed strategy, uint256 newDebt);
    event UpdateDepositLimit(uint256 depositLimit);
    event UpdateMinimumTotalIdle(uint256 minimumTotalIdle);
    event UpdateProfitMaxUnlockTime(uint256 profitMaxUnlockTime);
    event DebtPurchased(address indexed strategy, uint256 amount);
    event Shutdown();

    modifier nonReentrant() {
        require(nonreentrant_lock != 2, "reentrant call");
        nonreentrant_lock = 2;
        _;
        nonreentrant_lock = 3;
    }

    constructor() {
        asset = address(this);
    }

    function initialize(
        address asset_,
        string calldata name_,
        string calldata symbol_,
        address roleManager_,
        uint256 profitMaxUnlockTime_
    ) external {
        require(asset == address(0), "initialized");
        require(asset_ != address(0), "ZERO ADDRESS");
        require(roleManager_ != address(0), "ZERO ADDRESS");
        require(bytes(name_).length <= 64, "name too long");
        require(bytes(symbol_).length <= 32, "symbol too long");
        require(profitMaxUnlockTime_ <= 31_556_952, "profit unlock time too long");

        asset = asset_;
        decimals = YearnBenchERC20(asset_).decimals();
        factory = msg.sender;
        profit_max_unlock_time = profitMaxUnlockTime_;
        name = name_;
        symbol = symbol_;
        role_manager = roleManager_;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        return _approve(msg.sender, spender, amount);
    }

    function transfer(address receiver, uint256 amount) external returns (bool) {
        require(receiver != address(0) && receiver != address(this), "receiver");
        _transfer(msg.sender, receiver, amount);
        return true;
    }

    function transferFrom(address sender, address receiver, uint256 amount) external returns (bool) {
        require(receiver != address(0) && receiver != address(this), "receiver");
        _spendAllowance(sender, msg.sender, amount);
        _transfer(sender, receiver, amount);
        return true;
    }

    function permit(address owner, address spender, uint256 amount, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
        external
        returns (bool)
    {
        require(owner != address(0), "invalid owner");
        require(deadline >= block.timestamp, "permit expired");
        uint256 nonce = nonces[owner];
        bytes32 digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                domain_separator(),
                keccak256(abi.encode(PERMIT_TYPE_HASH, owner, spender, amount, nonce, deadline))
            )
        );
        require(ecrecover(digest, v, r, s) == owner, "invalid signature");

        allowance[owner][spender] = amount;
        nonces[owner] = nonce + 1;
        emit Approval(owner, spender, amount);
        return true;
    }

    function deposit(uint256 assets, address receiver) external nonReentrant returns (uint256 shares) {
        uint256 amount = assets;
        if (amount == type(uint256).max) {
            amount = YearnBenchERC20(asset).balanceOf(msg.sender);
        }
        shares = _convertToShares(amount, false);
        _deposit(receiver, amount, shares);
    }

    function mint(uint256 shares, address receiver) external nonReentrant returns (uint256 assets) {
        assets = _convertToAssets(shares, true);
        _deposit(receiver, assets, shares);
    }

    function withdraw(uint256 assets, address receiver, address owner)
        external
        nonReentrant
        returns (uint256 shares)
    {
        address[] memory queue = new address[](0);
        shares = _withdraw(msg.sender, assets, receiver, owner, 0, queue);
    }

    function withdraw(uint256 assets, address receiver, address owner, uint256 maxLoss)
        external
        nonReentrant
        returns (uint256 shares)
    {
        address[] memory queue = new address[](0);
        shares = _withdraw(msg.sender, assets, receiver, owner, maxLoss, queue);
    }

    function withdraw(uint256 assets, address receiver, address owner, uint256 maxLoss, address[] calldata strategies_)
        external
        nonReentrant
        returns (uint256 shares)
    {
        require(strategies_.length <= MAX_QUEUE, "queue too long");
        shares = _withdraw(msg.sender, assets, receiver, owner, maxLoss, strategies_);
    }

    function redeem(uint256 shares, address receiver, address owner)
        external
        nonReentrant
        returns (uint256 assets)
    {
        address[] memory queue = new address[](0);
        assets = _redeem(msg.sender, receiver, owner, _convertToAssets(shares, false), shares, MAX_BPS, queue);
    }

    function redeem(uint256 shares, address receiver, address owner, uint256 maxLoss)
        external
        nonReentrant
        returns (uint256 assets)
    {
        address[] memory queue = new address[](0);
        assets = _redeem(msg.sender, receiver, owner, _convertToAssets(shares, false), shares, maxLoss, queue);
    }

    function redeem(uint256 shares, address receiver, address owner, uint256 maxLoss, address[] calldata strategies_)
        external
        nonReentrant
        returns (uint256 assets)
    {
        require(strategies_.length <= MAX_QUEUE, "queue too long");
        assets = _redeem(msg.sender, receiver, owner, _convertToAssets(shares, false), shares, maxLoss, strategies_);
    }

    function setName(string calldata newName) external {
        require(msg.sender == role_manager, "not allowed");
        require(bytes(newName).length <= 64, "name too long");
        name = newName;
    }

    function setSymbol(string calldata newSymbol) external {
        require(msg.sender == role_manager, "not allowed");
        require(bytes(newSymbol).length <= 32, "symbol too long");
        symbol = newSymbol;
    }

    function set_accountant(address newAccountant) external {
        _enforceRole(msg.sender, ACCOUNTANT_MANAGER);
        accountant = newAccountant;
        emit UpdateAccountant(newAccountant);
    }

    function set_default_queue(address[] calldata newDefaultQueue) external {
        _enforceRole(msg.sender, QUEUE_MANAGER);
        require(newDefaultQueue.length <= MAX_QUEUE, "queue too long");
        for (uint256 i = 0; i < newDefaultQueue.length; i++) {
            require(_strategies[newDefaultQueue[i]].activation != 0, "!inactive");
        }
        default_queue_length = newDefaultQueue.length;
        for (uint256 i = 0; i < newDefaultQueue.length; i++) {
            default_queue_storage[i] = newDefaultQueue[i];
        }
        emit UpdateDefaultQueue(newDefaultQueue);
    }

    function set_use_default_queue(bool useDefaultQueue) external {
        _enforceRole(msg.sender, QUEUE_MANAGER);
        use_default_queue = useDefaultQueue;
        emit UpdateUseDefaultQueue(useDefaultQueue);
    }

    function set_auto_allocate(bool autoAllocate) external {
        _enforceRole(msg.sender, DEBT_MANAGER);
        auto_allocate = autoAllocate;
        emit UpdateAutoAllocate(autoAllocate);
    }

    function set_deposit_limit(uint256 newDepositLimit) external {
        _setDepositLimit(newDepositLimit, false);
    }

    function set_deposit_limit(uint256 newDepositLimit, bool overrideModule) external {
        _setDepositLimit(newDepositLimit, overrideModule);
    }

    function set_deposit_limit_module(address depositLimitModule) external {
        _setDepositLimitModule(depositLimitModule, false);
    }

    function set_deposit_limit_module(address depositLimitModule, bool overrideLimit) external {
        _setDepositLimitModule(depositLimitModule, overrideLimit);
    }

    function _setDepositLimit(uint256 newDepositLimit, bool overrideModule) internal {
        require(!shutdown, "shutdown");
        _enforceRole(msg.sender, DEPOSIT_LIMIT_MANAGER);
        if (overrideModule) {
            if (deposit_limit_module != address(0)) {
                deposit_limit_module = address(0);
                emit UpdateDepositLimitModule(address(0));
            }
        } else {
            require(deposit_limit_module == address(0), "using module");
        }
        deposit_limit = newDepositLimit;
        emit UpdateDepositLimit(newDepositLimit);
    }

    function _setDepositLimitModule(address depositLimitModule, bool overrideLimit) internal {
        require(!shutdown, "shutdown");
        _enforceRole(msg.sender, DEPOSIT_LIMIT_MANAGER);
        if (overrideLimit) {
            if (deposit_limit != type(uint256).max) {
                deposit_limit = type(uint256).max;
                emit UpdateDepositLimit(type(uint256).max);
            }
        } else {
            require(deposit_limit == type(uint256).max, "using deposit limit");
        }
        deposit_limit_module = depositLimitModule;
        emit UpdateDepositLimitModule(depositLimitModule);
    }

    function set_withdraw_limit_module(address withdrawLimitModule) external {
        _enforceRole(msg.sender, WITHDRAW_LIMIT_MANAGER);
        withdraw_limit_module = withdrawLimitModule;
        emit UpdateWithdrawLimitModule(withdrawLimitModule);
    }

    function set_minimum_total_idle(uint256 minimumTotalIdle) external {
        _enforceRole(msg.sender, MINIMUM_IDLE_MANAGER);
        minimum_total_idle = minimumTotalIdle;
        emit UpdateMinimumTotalIdle(minimumTotalIdle);
    }

    function setProfitMaxUnlockTime(uint256 newProfitMaxUnlockTime) external {
        _enforceRole(msg.sender, PROFIT_UNLOCK_MANAGER);
        require(newProfitMaxUnlockTime <= 31_556_952, "profit unlock time too long");
        if (newProfitMaxUnlockTime == 0) {
            uint256 shareBalance = _balanceOf[address(this)];
            if (shareBalance > 0) {
                _burnShares(shareBalance, address(this));
            }
            profit_unlocking_rate = 0;
            full_profit_unlock_date = 0;
        }
        profit_max_unlock_time = newProfitMaxUnlockTime;
        emit UpdateProfitMaxUnlockTime(newProfitMaxUnlockTime);
    }

    function set_role(address account, uint256 role) external {
        require(msg.sender == role_manager, "not allowed");
        require(role <= ALL_ROLES, "invalid role");
        roles[account] = role;
        emit RoleSet(account, role);
    }

    function add_role(address account, uint256 role) external {
        require(msg.sender == role_manager, "not allowed");
        require(role <= ALL_ROLES, "invalid role");
        uint256 newRoles = roles[account] | role;
        roles[account] = newRoles;
        emit RoleSet(account, newRoles);
    }

    function remove_role(address account, uint256 role) external {
        require(msg.sender == role_manager, "not allowed");
        require(role <= ALL_ROLES, "invalid role");
        uint256 newRoles = roles[account] & ~role;
        roles[account] = newRoles;
        emit RoleSet(account, newRoles);
    }

    function transfer_role_manager(address newRoleManager) external {
        require(msg.sender == role_manager, "not allowed");
        future_role_manager = newRoleManager;
        emit UpdateFutureRoleManager(newRoleManager);
    }

    function accept_role_manager() external {
        require(msg.sender == future_role_manager, "not allowed");
        role_manager = msg.sender;
        future_role_manager = address(0);
        emit UpdateRoleManager(msg.sender);
    }

    function process_report(address strategy) external nonReentrant returns (uint256 gain, uint256 loss) {
        _enforceRole(msg.sender, REPORTING_MANAGER);
        return _processReport(strategy);
    }

    function buy_debt(address strategy, uint256 amount) external nonReentrant {
        _enforceRole(msg.sender, DEBT_PURCHASER);
        StrategyParams storage params = _strategies[strategy];
        require(params.activation != 0, "not active");
        uint256 currentDebt = params.currentDebt;
        uint256 amount_ = amount;
        require(currentDebt > 0, "nothing to buy");
        require(amount_ > 0, "nothing to buy with");
        if (amount_ > currentDebt) {
            amount_ = currentDebt;
        }

        uint256 shares = YearnBenchStrategy(strategy).balanceOf(address(this)) * amount_ / currentDebt;
        require(shares > 0, "cannot buy zero");
        _safeTransferFromToken(asset, msg.sender, address(this), amount_);

        uint256 newDebt = currentDebt - amount_;
        params.currentDebt = newDebt;
        total_debt -= amount_;
        total_idle += amount_;
        emit DebtUpdated(strategy, currentDebt, newDebt);

        _safeTransferToken(strategy, msg.sender, shares);
        emit DebtPurchased(strategy, amount_);
    }

    function add_strategy(address newStrategy) external {
        _enforceRole(msg.sender, ADD_STRATEGY_MANAGER);
        _addStrategy(newStrategy, true);
    }

    function add_strategy(address newStrategy, bool addToQueue) external {
        _enforceRole(msg.sender, ADD_STRATEGY_MANAGER);
        _addStrategy(newStrategy, addToQueue);
    }

    function revoke_strategy(address strategy) external {
        _enforceRole(msg.sender, REVOKE_STRATEGY_MANAGER);
        _revokeStrategy(strategy, false);
    }

    function force_revoke_strategy(address strategy) external {
        _enforceRole(msg.sender, FORCE_REVOKE_MANAGER);
        _revokeStrategy(strategy, true);
    }

    function update_max_debt_for_strategy(address strategy, uint256 newMaxDebt) external {
        _enforceRole(msg.sender, MAX_DEBT_MANAGER);
        require(_strategies[strategy].activation != 0, "inactive strategy");
        _strategies[strategy].maxDebt = newMaxDebt;
        emit UpdatedMaxDebtForStrategy(msg.sender, strategy, newMaxDebt);
    }

    function update_debt(address strategy, uint256 targetDebt) external nonReentrant returns (uint256) {
        _enforceRole(msg.sender, DEBT_MANAGER);
        return _updateDebt(strategy, targetDebt, MAX_BPS);
    }

    function update_debt(address strategy, uint256 targetDebt, uint256 maxLoss)
        external
        nonReentrant
        returns (uint256)
    {
        _enforceRole(msg.sender, DEBT_MANAGER);
        return _updateDebt(strategy, targetDebt, maxLoss);
    }

    function shutdown_vault() external {
        _enforceRole(msg.sender, EMERGENCY_MANAGER);
        require(!shutdown, "shutdown");
        shutdown = true;
        if (deposit_limit_module != address(0)) {
            deposit_limit_module = address(0);
            emit UpdateDepositLimitModule(address(0));
        }
        deposit_limit = 0;
        emit UpdateDepositLimit(0);

        uint256 newRoles = roles[msg.sender] | DEBT_MANAGER;
        roles[msg.sender] = newRoles;
        emit RoleSet(msg.sender, newRoles);
        emit Shutdown();
    }

    function isShutdown() external view returns (bool) {
        return shutdown;
    }

    function unlockedShares() external view returns (uint256) {
        return _unlockedShares();
    }

    function pricePerShare() external view returns (uint256) {
        return _convertToAssets(10 ** uint256(decimals), false);
    }

    function get_default_queue() external view returns (address[] memory) {
        return _copyDefaultQueue();
    }

    function default_queue(uint256 index) external view returns (address) {
        require(index < default_queue_length, "index out of bounds");
        return default_queue_storage[index];
    }

    function strategies(address strategy) external view returns (StrategyParams memory) {
        return _strategies[strategy];
    }

    function balanceOf(address account) public view returns (uint256) {
        if (account == address(this)) {
            return _balanceOf[account] - _unlockedShares();
        }
        return _balanceOf[account];
    }

    function totalSupply() external view returns (uint256) {
        return _effectiveSupply();
    }

    function totalAssets() public view returns (uint256) {
        return total_idle + total_debt;
    }

    function totalIdle() external view returns (uint256) {
        return total_idle;
    }

    function totalDebt() external view returns (uint256) {
        return total_debt;
    }

    function convertToShares(uint256 assets) external view returns (uint256) {
        return _convertToShares(assets, false);
    }

    function previewDeposit(uint256 assets) external view returns (uint256) {
        return _convertToShares(assets, false);
    }

    function previewMint(uint256 shares) external view returns (uint256) {
        return _convertToAssets(shares, true);
    }

    function convertToAssets(uint256 shares) external view returns (uint256) {
        return _convertToAssets(shares, false);
    }

    function maxDeposit(address receiver) external view returns (uint256) {
        return _maxDeposit(receiver);
    }

    function maxMint(address receiver) external view returns (uint256) {
        return _convertToShares(_maxDeposit(receiver), false);
    }

    function maxWithdraw(address owner) external view returns (uint256) {
        address[] memory queue = new address[](0);
        return _maxWithdraw(owner, 0, queue);
    }

    function maxWithdraw(address owner, uint256 maxLoss) external view returns (uint256) {
        address[] memory queue = new address[](0);
        return _maxWithdraw(owner, maxLoss, queue);
    }

    function maxWithdraw(address owner, uint256 maxLoss, address[] calldata strategies_)
        external
        view
        returns (uint256)
    {
        require(strategies_.length <= MAX_QUEUE, "queue too long");
        return _maxWithdraw(owner, maxLoss, strategies_);
    }

    function maxRedeem(address owner) external view returns (uint256) {
        address[] memory queue = new address[](0);
        return _maxRedeem(owner, MAX_BPS, queue);
    }

    function maxRedeem(address owner, uint256 maxLoss) external view returns (uint256) {
        address[] memory queue = new address[](0);
        return _maxRedeem(owner, maxLoss, queue);
    }

    function maxRedeem(address owner, uint256 maxLoss, address[] calldata strategies_) external view returns (uint256) {
        require(strategies_.length <= MAX_QUEUE, "queue too long");
        return _maxRedeem(owner, maxLoss, strategies_);
    }

    function previewWithdraw(uint256 assets) external view returns (uint256) {
        return _convertToShares(assets, true);
    }

    function previewRedeem(uint256 shares) external view returns (uint256) {
        return _convertToAssets(shares, false);
    }

    function FACTORY() external view returns (address) {
        return factory;
    }

    function apiVersion() external pure returns (string memory) {
        return API_VERSION;
    }

    function assess_share_of_unrealised_losses(address strategy, uint256 assetsNeeded) external view returns (uint256) {
        uint256 currentDebt = _strategies[strategy].currentDebt;
        require(currentDebt >= assetsNeeded, "debt");
        return _assessShareOfUnrealisedLosses(strategy, currentDebt, assetsNeeded);
    }

    function profitMaxUnlockTime() external view returns (uint256) {
        return profit_max_unlock_time;
    }

    function fullProfitUnlockDate() external view returns (uint256) {
        return full_profit_unlock_date;
    }

    function profitUnlockingRate() external view returns (uint256) {
        return profit_unlocking_rate;
    }

    function lastProfitUpdate() external view returns (uint256) {
        return last_profit_update;
    }

    function domain_separator() internal view returns (bytes32) {
        return keccak256(
            abi.encode(
                DOMAIN_TYPE_HASH, keccak256("Yearn Vault"), keccak256(bytes(API_VERSION)), block.chainid, address(this)
            )
        );
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return domain_separator();
    }

    function _withdraw(
        address sender,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss,
        address[] memory strategies_
    ) internal returns (uint256 shares) {
        shares = _convertToShares(assets, true);
        _redeem(sender, receiver, owner, assets, shares, maxLoss, strategies_);
    }

    function _deposit(address recipient, uint256 assets, uint256 shares) internal {
        require(assets <= _maxDeposit(recipient), "exceed deposit limit");
        require(assets > 0, "cannot deposit zero");
        require(shares > 0, "cannot mint zero");
        _safeTransferFromToken(asset, msg.sender, address(this), assets);
        total_idle += assets;
        _issueShares(shares, recipient);
        emit Deposit(msg.sender, recipient, assets, shares);

        if (auto_allocate) {
            _updateDebt(default_queue_storage[0], type(uint256).max, 0);
        }
    }

    function _redeem(
        address sender,
        address receiver,
        address owner,
        uint256 assets,
        uint256 shares,
        uint256 maxLoss,
        address[] memory strategies_
    ) internal returns (uint256) {
        require(receiver != address(0), "ZERO ADDRESS");
        require(shares > 0, "no shares to redeem");
        require(assets > 0, "no assets to withdraw");
        require(maxLoss <= MAX_BPS, "max loss");

        if (withdraw_limit_module != address(0)) {
            require(
                assets
                    <= YearnBenchWithdrawLimitModule(withdraw_limit_module)
                        .available_withdraw_limit(owner, maxLoss, strategies_),
                "exceed withdraw limit"
            );
        }

        require(_balanceOf[owner] >= shares, "insufficient shares to redeem");
        if (sender != owner) {
            _spendAllowance(owner, sender, shares);
        }

        RedeemQueueState memory queueState = RedeemQueueState({
            requestedAssets: assets,
            currentTotalIdle: total_idle,
            currentTotalDebt: total_debt,
            assetsNeeded: 0,
            previousBalance: 0
        });

        if (queueState.requestedAssets > queueState.currentTotalIdle) {
            queueState.assetsNeeded = queueState.requestedAssets - queueState.currentTotalIdle;
            queueState.previousBalance = YearnBenchERC20(asset).balanceOf(address(this));
            queueState = _withdrawFromQueue(_queueFor(strategies_), queueState);
            require(queueState.currentTotalIdle >= queueState.requestedAssets, "insufficient assets in vault");
            total_debt = queueState.currentTotalDebt;
        }

        if (assets > queueState.requestedAssets && maxLoss < MAX_BPS) {
            require(assets - queueState.requestedAssets <= assets * maxLoss / MAX_BPS, "too much loss");
        }

        _burnShares(shares, owner);
        total_idle = queueState.currentTotalIdle - queueState.requestedAssets;
        _safeTransferToken(asset, receiver, queueState.requestedAssets);
        emit Withdraw(sender, receiver, owner, queueState.requestedAssets, shares);
        return queueState.requestedAssets;
    }

    function _withdrawFromQueue(address[] memory queue, RedeemQueueState memory state)
        internal
        returns (RedeemQueueState memory)
    {
        for (uint256 i = 0; i < queue.length; i++) {
            state = _withdrawFromQueueStrategy(queue[i], state);
            if (state.requestedAssets <= state.currentTotalIdle) {
                break;
            }
        }
        return state;
    }

    function _withdrawFromQueueStrategy(address strategy, RedeemQueueState memory state)
        internal
        returns (RedeemQueueState memory)
    {
        StrategyParams storage params = _strategies[strategy];
        require(params.activation != 0, "inactive strategy");

        uint256 currentDebt = params.currentDebt;
        uint256 assetsToWithdraw = _min(state.assetsNeeded, currentDebt);
        uint256 maxWithdraw_ =
            YearnBenchStrategy(strategy).convertToAssets(YearnBenchStrategy(strategy).maxRedeem(address(this)));

        uint256 unrealisedLossesShare = _assessShareOfUnrealisedLosses(strategy, currentDebt, assetsToWithdraw);
        if (unrealisedLossesShare > 0) {
            if (maxWithdraw_ < assetsToWithdraw - unrealisedLossesShare) {
                uint256 wanted = assetsToWithdraw - unrealisedLossesShare;
                unrealisedLossesShare = unrealisedLossesShare * maxWithdraw_ / wanted;
                assetsToWithdraw = maxWithdraw_ + unrealisedLossesShare;
            }

            assetsToWithdraw -= unrealisedLossesShare;
            state.requestedAssets -= unrealisedLossesShare;
            state.assetsNeeded -= unrealisedLossesShare;
            state.currentTotalDebt -= unrealisedLossesShare;

            if (maxWithdraw_ == 0 && unrealisedLossesShare > 0) {
                uint256 newDebtForLoss = currentDebt - unrealisedLossesShare;
                params.currentDebt = newDebtForLoss;
                emit DebtUpdated(strategy, currentDebt, newDebtForLoss);
            }
        }

        assetsToWithdraw = _min(assetsToWithdraw, maxWithdraw_);
        if (assetsToWithdraw == 0) {
            return state;
        }

        _withdrawFromStrategy(strategy, assetsToWithdraw);
        uint256 postBalance = YearnBenchERC20(asset).balanceOf(address(this));
        uint256 withdrawn = postBalance - state.previousBalance;
        uint256 loss = 0;
        if (withdrawn > assetsToWithdraw) {
            if (withdrawn > currentDebt) {
                assetsToWithdraw = currentDebt;
            } else {
                assetsToWithdraw += withdrawn - assetsToWithdraw;
            }
        } else if (withdrawn < assetsToWithdraw) {
            loss = assetsToWithdraw - withdrawn;
        }

        state.currentTotalIdle += assetsToWithdraw - loss;
        state.requestedAssets -= loss;
        state.currentTotalDebt -= assetsToWithdraw;

        uint256 newDebt = currentDebt - (assetsToWithdraw + unrealisedLossesShare);
        params.currentDebt = newDebt;
        emit DebtUpdated(strategy, currentDebt, newDebt);

        if (state.requestedAssets > state.currentTotalIdle) {
            state.previousBalance = postBalance;
            state.assetsNeeded -= assetsToWithdraw;
        }
        return state;
    }

    function _addStrategy(address newStrategy, bool addToQueue) internal {
        require(newStrategy != address(this) && newStrategy != address(0), "strategy cannot be zero address");
        require(YearnBenchStrategy(newStrategy).asset() == asset, "invalid asset");
        require(_strategies[newStrategy].activation == 0, "strategy already active");

        _strategies[newStrategy] =
            StrategyParams({activation: block.timestamp, lastReport: block.timestamp, currentDebt: 0, maxDebt: 0});

        if (addToQueue && default_queue_length < MAX_QUEUE) {
            default_queue_storage[default_queue_length] = newStrategy;
            default_queue_length += 1;
        }

        emit StrategyChanged(newStrategy, STRATEGY_CHANGE_ADDED);
    }

    function _revokeStrategy(address strategy, bool force) internal {
        StrategyParams storage params = _strategies[strategy];
        require(params.activation != 0, "strategy not active");
        if (params.currentDebt != 0) {
            require(force, "strategy has debt");
            uint256 loss = params.currentDebt;
            total_debt -= loss;
            emit StrategyReported(strategy, 0, loss, 0, 0, 0, 0);
        }

        delete _strategies[strategy];
        uint256 writeIndex = 0;
        uint256 length = default_queue_length;
        for (uint256 i = 0; i < length; i++) {
            address queuedStrategy = default_queue_storage[i];
            if (queuedStrategy != strategy) {
                if (writeIndex != i) {
                    default_queue_storage[writeIndex] = queuedStrategy;
                }
                writeIndex++;
            }
        }
        default_queue_length = writeIndex;

        emit StrategyChanged(strategy, STRATEGY_CHANGE_REVOKED);
    }

    function _updateDebt(address strategy, uint256 targetDebt, uint256 maxLoss) internal returns (uint256) {
        StrategyParams storage params = _strategies[strategy];
        uint256 newDebt = shutdown ? 0 : targetDebt;
        uint256 currentDebt = params.currentDebt;
        require(newDebt != currentDebt, "new debt equals current debt");

        if (currentDebt > newDebt) {
            uint256 assetsToWithdraw = currentDebt - newDebt;
            uint256 currentIdle = total_idle;
            if (currentIdle + assetsToWithdraw < minimum_total_idle) {
                assetsToWithdraw = minimum_total_idle - currentIdle;
                if (assetsToWithdraw > currentDebt) {
                    assetsToWithdraw = currentDebt;
                }
            }

            uint256 withdrawable =
                YearnBenchStrategy(strategy).convertToAssets(YearnBenchStrategy(strategy).maxRedeem(address(this)));
            if (withdrawable < assetsToWithdraw) {
                assetsToWithdraw = withdrawable;
            }
            if (assetsToWithdraw == 0) {
                return currentDebt;
            }

            uint256 unrealisedLossesShare = _assessShareOfUnrealisedLosses(strategy, currentDebt, assetsToWithdraw);
            require(unrealisedLossesShare == 0, "strategy has unrealised losses");

            uint256 preBalance = YearnBenchERC20(asset).balanceOf(address(this));
            _withdrawFromStrategy(strategy, assetsToWithdraw);
            uint256 postBalance = YearnBenchERC20(asset).balanceOf(address(this));
            uint256 withdrawn = _min(postBalance - preBalance, currentDebt);

            if (withdrawn < assetsToWithdraw && maxLoss < MAX_BPS) {
                require(assetsToWithdraw - withdrawn <= assetsToWithdraw * maxLoss / MAX_BPS, "too much loss");
            } else if (withdrawn > assetsToWithdraw) {
                assetsToWithdraw = withdrawn;
            }

            total_idle += withdrawn;
            total_debt -= assetsToWithdraw;
            newDebt = currentDebt - assetsToWithdraw;
        } else {
            uint256 maxDebt = params.maxDebt;
            if (newDebt > maxDebt) {
                newDebt = maxDebt;
                if (newDebt < currentDebt) {
                    return currentDebt;
                }
            }

            uint256 maxDeposit_ = YearnBenchStrategy(strategy).maxDeposit(address(this));
            if (maxDeposit_ == 0) {
                return currentDebt;
            }

            uint256 assetsToDeposit = newDebt - currentDebt;
            if (assetsToDeposit > maxDeposit_) {
                assetsToDeposit = maxDeposit_;
            }

            uint256 currentIdle = total_idle;
            if (currentIdle <= minimum_total_idle) {
                return currentDebt;
            }

            uint256 availableIdle = currentIdle - minimum_total_idle;
            if (assetsToDeposit > availableIdle) {
                assetsToDeposit = availableIdle;
            }

            if (assetsToDeposit > 0) {
                _safeApproveToken(asset, strategy, assetsToDeposit);
                uint256 preBalance = YearnBenchERC20(asset).balanceOf(address(this));
                YearnBenchStrategy(strategy).deposit(assetsToDeposit, address(this));
                uint256 postBalance = YearnBenchERC20(asset).balanceOf(address(this));
                _safeApproveToken(asset, strategy, 0);

                assetsToDeposit = preBalance - postBalance;
                total_idle -= assetsToDeposit;
                total_debt += assetsToDeposit;
            }

            newDebt = currentDebt + assetsToDeposit;
        }

        params.currentDebt = newDebt;
        emit DebtUpdated(strategy, currentDebt, newDebt);
        return newDebt;
    }

    function _processReport(address strategy) internal returns (uint256 gain, uint256 loss) {
        ReportState memory report = _loadReportState(strategy);
        report = _applyAccountantReport(strategy, report);
        report = _prepareReportShares(report);
        report.currentDebt = _applyReportAssetAccounting(
            strategy, report.currentDebt, report.gain, report.loss, report.totalRefunds, report.accountant_
        );
        _issueReportFeeShares(report);
        _updateProfitUnlock(report.sharesToLock, report.profitMaxUnlockTime_);

        _strategies[strategy].lastReport = block.timestamp;
        if (report.loss + report.totalFees > report.gain + report.totalRefunds || report.profitMaxUnlockTime_ == 0) {
            report.totalFees = _convertToAssets(report.totalFeesShares, false);
        }

        emit StrategyReported(
            strategy,
            report.gain,
            report.loss,
            report.currentDebt,
            report.totalFees * uint256(report.protocolFeeBps) / MAX_BPS,
            report.totalFees,
            report.totalRefunds
        );
        return (report.gain, report.loss);
    }

    function _loadReportState(address strategy) internal view returns (ReportState memory report) {
        if (strategy != address(this)) {
            StrategyParams storage params = _strategies[strategy];
            require(params.activation != 0, "inactive strategy");
            uint256 strategyShares = YearnBenchStrategy(strategy).balanceOf(address(this));
            report.totalAssets_ = YearnBenchStrategy(strategy).convertToAssets(strategyShares);
            report.currentDebt = params.currentDebt;
        } else {
            report.totalAssets_ = YearnBenchERC20(asset).balanceOf(address(this));
            report.currentDebt = total_idle;
        }

        if (report.totalAssets_ > report.currentDebt) {
            report.gain = report.totalAssets_ - report.currentDebt;
        } else {
            report.loss = report.currentDebt - report.totalAssets_;
        }
        report.accountant_ = accountant;
        report.profitMaxUnlockTime_ = profit_max_unlock_time;
    }

    function _applyAccountantReport(address strategy, ReportState memory report)
        internal
        returns (ReportState memory)
    {
        if (report.accountant_ != address(0)) {
            (report.totalFees, report.totalRefunds) =
                YearnBenchAccountant(report.accountant_).report(strategy, report.gain, report.loss);
            if (report.totalRefunds > 0) {
                report.totalRefunds = _min(
                    report.totalRefunds,
                    _min(
                        YearnBenchERC20(asset).balanceOf(report.accountant_),
                        YearnBenchERC20(asset).allowance(report.accountant_, address(this))
                    )
                );
            }
        }
        return report;
    }

    function _prepareReportShares(ReportState memory report) internal returns (ReportState memory) {
        if (report.loss + report.totalFees > 0) {
            report.sharesToBurn = _convertToShares(report.loss + report.totalFees, true);
            if (report.totalFees > 0) {
                report.totalFeesShares = report.sharesToBurn * report.totalFees / (report.loss + report.totalFees);
                (report.protocolFeeBps, report.protocolFeeRecipient) = YearnBenchFactory(factory).protocol_fee_config();
                if (report.protocolFeeBps > 0) {
                    report.protocolFeesShares = report.totalFeesShares * uint256(report.protocolFeeBps) / MAX_BPS;
                }
            }
        }

        if (report.gain + report.totalRefunds > 0 && report.profitMaxUnlockTime_ != 0) {
            report.sharesToLock = _convertToShares(report.gain + report.totalRefunds, false);
        }

        uint256 totalSupply_ = _totalSupply;
        uint256 endingSupply = totalSupply_ + report.sharesToLock - report.sharesToBurn - _unlockedShares();

        if (endingSupply > totalSupply_) {
            _issueShares(endingSupply - totalSupply_, address(this));
        } else if (totalSupply_ > endingSupply) {
            uint256 toBurn = _min(totalSupply_ - endingSupply, _balanceOf[address(this)]);
            _burnShares(toBurn, address(this));
        }

        if (report.sharesToLock > report.sharesToBurn) {
            report.sharesToLock -= report.sharesToBurn;
        } else {
            report.sharesToLock = 0;
        }
        return report;
    }

    function _issueReportFeeShares(ReportState memory report) internal {
        if (report.totalFeesShares > 0) {
            _issueShares(report.totalFeesShares - report.protocolFeesShares, report.accountant_);
            if (report.protocolFeesShares > 0) {
                _issueShares(report.protocolFeesShares, report.protocolFeeRecipient);
            }
        }
    }

    function _updateProfitUnlock(uint256 sharesToLock, uint256 profitMaxUnlockTime_) internal {
        uint256 totalLockedShares = _balanceOf[address(this)];
        if (totalLockedShares > 0) {
            uint256 previouslyLockedTime;
            if (full_profit_unlock_date > block.timestamp) {
                previouslyLockedTime = (totalLockedShares - sharesToLock) * (full_profit_unlock_date - block.timestamp);
            }
            uint256 newProfitLockingPeriod =
                (previouslyLockedTime + sharesToLock * profitMaxUnlockTime_) / totalLockedShares;
            if (newProfitLockingPeriod > 0) {
                profit_unlocking_rate = totalLockedShares * MAX_BPS_EXTENDED / newProfitLockingPeriod;
                full_profit_unlock_date = block.timestamp + newProfitLockingPeriod;
                last_profit_update = block.timestamp;
            } else {
                profit_unlocking_rate = 0;
                full_profit_unlock_date = 0;
            }
        } else {
            full_profit_unlock_date = 0;
        }
    }

    function _applyReportAssetAccounting(
        address strategy,
        uint256 currentDebt,
        uint256 gain,
        uint256 loss,
        uint256 totalRefunds,
        address accountant_
    ) internal returns (uint256) {
        if (totalRefunds > 0) {
            _safeTransferFromToken(asset, accountant_, address(this), totalRefunds);
            total_idle += totalRefunds;
        }

        if (gain > 0) {
            currentDebt += gain;
            if (strategy != address(this)) {
                _strategies[strategy].currentDebt = currentDebt;
                total_debt += gain;
            } else {
                currentDebt += totalRefunds;
                total_idle = currentDebt;
            }
        } else if (loss > 0) {
            currentDebt -= loss;
            if (strategy != address(this)) {
                _strategies[strategy].currentDebt = currentDebt;
                total_debt -= loss;
            } else {
                currentDebt += totalRefunds;
                total_idle = currentDebt;
            }
        }

        return currentDebt;
    }

    function _maxDeposit(address receiver) internal view returns (uint256) {
        if (receiver == address(0) || receiver == address(this)) {
            return 0;
        }
        if (deposit_limit_module != address(0)) {
            return YearnBenchDepositLimitModule(deposit_limit_module).available_deposit_limit(receiver);
        }
        if (deposit_limit == type(uint256).max) {
            return type(uint256).max;
        }
        uint256 totalAssets_ = totalAssets();
        if (totalAssets_ >= deposit_limit) {
            return 0;
        }
        return deposit_limit - totalAssets_;
    }

    function _maxWithdraw(address owner, uint256 maxLoss, address[] memory strategies_)
        internal
        view
        returns (uint256)
    {
        uint256 maxAssets = _convertToAssets(_balanceOf[owner], false);
        if (withdraw_limit_module != address(0)) {
            return _min(
                YearnBenchWithdrawLimitModule(withdraw_limit_module)
                    .available_withdraw_limit(owner, maxLoss, strategies_),
                maxAssets
            );
        }

        uint256 currentIdle = total_idle;
        if (maxAssets > currentIdle) {
            uint256 have = currentIdle;
            uint256 loss;
            address[] memory queue = _queueFor(strategies_);
            for (uint256 i = 0; i < queue.length; i++) {
                address strategy = queue[i];
                require(_strategies[strategy].activation != 0, "inactive strategy");
                uint256 currentDebt = _strategies[strategy].currentDebt;
                uint256 toWithdraw = _min(maxAssets - have, currentDebt);
                uint256 unrealisedLoss = _assessShareOfUnrealisedLosses(strategy, currentDebt, toWithdraw);
                uint256 strategyLimit =
                    YearnBenchStrategy(strategy).convertToAssets(YearnBenchStrategy(strategy).maxRedeem(address(this)));

                uint256 realizableWithdraw = toWithdraw - unrealisedLoss;
                if (strategyLimit < realizableWithdraw) {
                    if (unrealisedLoss != 0) {
                        unrealisedLoss = unrealisedLoss * strategyLimit / realizableWithdraw;
                    }
                    toWithdraw = strategyLimit + unrealisedLoss;
                }

                if (toWithdraw == 0) {
                    continue;
                }
                if (unrealisedLoss > 0 && maxLoss < MAX_BPS) {
                    if (loss + unrealisedLoss > (have + toWithdraw) * maxLoss / MAX_BPS) {
                        break;
                    }
                }
                have += toWithdraw;
                if (have >= maxAssets) {
                    break;
                }
                loss += unrealisedLoss;
            }
            maxAssets = have;
        }
        return maxAssets;
    }

    function _maxRedeem(address owner, uint256 maxLoss, address[] memory strategies_) internal view returns (uint256) {
        return _min(_convertToShares(_maxWithdraw(owner, maxLoss, strategies_), false), _balanceOf[owner]);
    }

    function _assessShareOfUnrealisedLosses(address strategy, uint256 strategyCurrentDebt, uint256 assetsNeeded)
        internal
        view
        returns (uint256)
    {
        uint256 vaultShares = YearnBenchStrategy(strategy).balanceOf(address(this));
        uint256 strategyAssets = YearnBenchStrategy(strategy).convertToAssets(vaultShares);
        if (strategyAssets >= strategyCurrentDebt || strategyCurrentDebt == 0) {
            return 0;
        }
        uint256 numerator = assetsNeeded * strategyAssets;
        uint256 usersShareOfLoss = assetsNeeded - numerator / strategyCurrentDebt;
        if (numerator % strategyCurrentDebt != 0) {
            usersShareOfLoss += 1;
        }
        return usersShareOfLoss;
    }

    function _withdrawFromStrategy(address strategy, uint256 assetsToWithdraw) internal {
        uint256 sharesToRedeem = _min(
            YearnBenchStrategy(strategy).previewWithdraw(assetsToWithdraw),
            YearnBenchStrategy(strategy).balanceOf(address(this))
        );
        YearnBenchStrategy(strategy).redeem(sharesToRedeem, address(this), address(this));
    }

    function _queueFor(address[] memory strategies_) internal view returns (address[] memory queue) {
        require(strategies_.length <= MAX_QUEUE, "queue too long");
        if (strategies_.length != 0 && !use_default_queue) {
            return strategies_;
        }
        queue = _copyDefaultQueue();
    }

    function _copyDefaultQueue() internal view returns (address[] memory queue) {
        uint256 length = default_queue_length;
        queue = new address[](length);
        for (uint256 i = 0; i < length; i++) {
            queue[i] = default_queue_storage[i];
        }
    }

    function _spendAllowance(address owner, address spender, uint256 amount) internal {
        uint256 currentAllowance = allowance[owner][spender];
        if (currentAllowance < type(uint256).max) {
            require(currentAllowance >= amount, "insufficient allowance");
            _approve(owner, spender, currentAllowance - amount);
        }
    }

    function _approve(address owner, address spender, uint256 amount) internal returns (bool) {
        allowance[owner][spender] = amount;
        emit Approval(owner, spender, amount);
        return true;
    }

    function _transfer(address sender, address receiver, uint256 amount) internal {
        uint256 senderBalance = _balanceOf[sender];
        require(senderBalance >= amount, "insufficient funds");
        _balanceOf[sender] = senderBalance - amount;
        _balanceOf[receiver] += amount;
        emit Transfer(sender, receiver, amount);
    }

    function _issueShares(uint256 shares, address recipient) internal {
        _balanceOf[recipient] += shares;
        _totalSupply += shares;
        emit Transfer(address(0), recipient, shares);
    }

    function _burnShares(uint256 shares, address owner) internal {
        _balanceOf[owner] -= shares;
        _totalSupply -= shares;
        emit Transfer(owner, address(0), shares);
    }

    function _effectiveSupply() internal view returns (uint256) {
        return _totalSupply - _unlockedShares();
    }

    function _unlockedShares() internal view returns (uint256) {
        uint256 fullProfitUnlockDate_ = full_profit_unlock_date;
        uint256 unlocked = 0;
        if (fullProfitUnlockDate_ > block.timestamp) {
            unlocked = profit_unlocking_rate * (block.timestamp - last_profit_update) / MAX_BPS_EXTENDED;
        } else if (fullProfitUnlockDate_ != 0) {
            unlocked = _balanceOf[address(this)];
        }
        return unlocked;
    }

    function _convertToAssets(uint256 shares, bool roundUp) internal view returns (uint256) {
        if (shares == type(uint256).max || shares == 0) {
            return shares;
        }
        uint256 supply = _effectiveSupply();
        if (supply == 0) {
            return shares;
        }
        uint256 numerator = shares * totalAssets();
        uint256 amount = numerator / supply;
        if (roundUp && numerator % supply != 0) {
            amount += 1;
        }
        return amount;
    }

    function _convertToShares(uint256 assets, bool roundUp) internal view returns (uint256) {
        if (assets == type(uint256).max || assets == 0) {
            return assets;
        }
        uint256 supply = _effectiveSupply();
        if (supply == 0) {
            return assets;
        }
        uint256 totalAssets_ = totalAssets();
        if (totalAssets_ == 0) {
            return 0;
        }
        uint256 numerator = assets * supply;
        uint256 shares = numerator / totalAssets_;
        if (roundUp && numerator % totalAssets_ != 0) {
            shares += 1;
        }
        return shares;
    }

    function _safeApproveToken(address token, address spender, uint256 amount) internal {
        _optionalReturn(
            token, abi.encodeWithSelector(YearnBenchERC20.approve.selector, spender, amount), "approval failed"
        );
    }

    function _safeTransferFromToken(address token, address sender, address receiver, uint256 amount) internal {
        _optionalReturn(
            token,
            abi.encodeWithSelector(YearnBenchERC20.transferFrom.selector, sender, receiver, amount),
            "transfer failed"
        );
    }

    function _safeTransferToken(address token, address receiver, uint256 amount) internal {
        _optionalReturn(
            token, abi.encodeWithSelector(YearnBenchERC20.transfer.selector, receiver, amount), "transfer failed"
        );
    }

    function _optionalReturn(address token, bytes memory data, string memory message) internal {
        (bool ok, bytes memory returndata) = token.call(data);
        require(ok, message);
        if (returndata.length > 0) {
            require(abi.decode(returndata, (bool)), message);
        }
    }

    function _enforceRole(address account, uint256 role) internal view {
        require(roles[account] & role != 0, "not allowed");
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) {
        return a < b ? a : b;
    }
}

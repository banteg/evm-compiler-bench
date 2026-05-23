// SPDX-License-Identifier: AGPL-3.0
pragma solidity ^0.8.30;

interface YearnBenchERC20 {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

interface YearnBenchStrategy {
    function increaseDebt(uint256 amount) external returns (bool);
    function withdrawTo(address receiver, uint256 amount, uint256 maxLoss) external returns (uint256 withdrawn, uint256 loss);
    function report() external returns (uint256 gain, uint256 loss);
}

contract YearnVaultV3Real {
    uint256 public constant MAX_BPS = 10_000;
    uint256 public constant WAD = 1e18;
    uint256 public constant ADD_STRATEGY_MANAGER = 1 << 0;
    uint256 public constant REPORTING_MANAGER = 1 << 5;
    uint256 public constant DEBT_MANAGER = 1 << 6;
    uint256 public constant MAX_DEBT_MANAGER = 1 << 7;
    uint256 public constant DEPOSIT_LIMIT_MANAGER = 1 << 8;
    uint256 public constant PROFIT_UNLOCK_MANAGER = 1 << 11;
    uint256 public constant EMERGENCY_MANAGER = 1 << 13;
    bytes32 public constant DOMAIN_TYPE_HASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    bytes32 public constant PERMIT_TYPE_HASH =
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    string public name = "Yearn V3 Vault";
    string public symbol = "yvV3";
    string public constant API_VERSION = "3.0.4";
    uint8 public constant decimals = 18;

    address public asset;
    address public roleManager;
    address public futureRoleManager;
    bool public initialized;
    bool public shutdown;
    uint256 public depositLimit;
    uint256 public profitMaxUnlockTime;
    uint256 public fullProfitUnlockDate;
    uint256 public profitUnlockingRate;
    uint256 public lastProfitUpdate;
    uint256 public feeBps;
    uint256 public totalIdle;
    uint256 public totalDebt;
    uint256 public totalSupply;
    address public defaultQueueStrategy;

    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;
    mapping(address => uint256) public nonces;
    mapping(address => uint256) public roles;
    mapping(address => Strategy) public strategies;

    struct Strategy {
        uint256 activation;
        uint256 currentDebt;
        uint256 maxDebt;
        uint256 balance;
    }

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Deposit(address indexed sender, address indexed owner, uint256 assets, uint256 shares);
    event Withdraw(address indexed sender, address indexed receiver, address indexed owner, uint256 assets, uint256 shares);
    event StrategyChanged(address indexed strategy, uint256 activation);
    event DebtUpdated(address indexed strategy, uint256 currentDebt, uint256 newDebt);
    event StrategyReported(address indexed strategy, uint256 gain, uint256 loss, uint256 totalDebt, uint256 totalIdle);
    event RoleSet(address indexed account, uint256 indexed role);
    event UpdateFutureRoleManager(address indexed futureRoleManager);
    event UpdateRoleManager(address indexed roleManager);
    event Shutdown();

    modifier ready() {
        require(initialized, "not initialized");
        _;
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
        require(!initialized, "initialized");
        require(asset_ != address(0), "asset");
        require(roleManager_ != address(0), "role manager");
        initialized = true;
        asset = asset_;
        name = name_;
        symbol = symbol_;
        roleManager = roleManager_;
        profitMaxUnlockTime = profitMaxUnlockTime_;
        lastProfitUpdate = block.timestamp;
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return true;
    }

    function transfer(address to, uint256 value) external returns (bool) {
        require(to != address(0) && to != address(this), "receiver");
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
        require(to != address(0) && to != address(this), "receiver");
        _transfer(from, to, value);
        return true;
    }

    function deposit(uint256 assets, address receiver) external ready returns (uint256 shares) {
        require(!shutdown, "shutdown");
        uint256 amount = assets;
        if (amount == type(uint256).max) {
            amount = YearnBenchERC20(asset).balanceOf(msg.sender);
        }
        require(receiver != address(0) && receiver != address(this), "receiver");
        require(amount > 0, "assets");
        require(totalAssets() + amount <= depositLimit, "limit");
        shares = _convertToShares(amount, false);
        require(shares > 0, "shares");
        _safeTransferFrom(msg.sender, address(this), amount);
        totalIdle += amount;
        _mint(receiver, shares);
        emit Deposit(msg.sender, receiver, amount, shares);
    }

    function mint(uint256 shares, address receiver) external ready returns (uint256 assets) {
        require(!shutdown, "shutdown");
        require(receiver != address(0) && receiver != address(this), "receiver");
        require(shares > 0, "shares");
        assets = _convertToAssets(shares, true);
        require(totalAssets() + assets <= depositLimit, "limit");
        _safeTransferFrom(msg.sender, address(this), assets);
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
        _spendAllowance(owner, shares);
        uint256 actualAssets = _redeem(receiver, owner, assets, shares, maxLoss);
        emit Withdraw(msg.sender, receiver, owner, actualAssets, shares);
    }

    function redeem(uint256 shares, address receiver, address owner, uint256 maxLoss)
        external
        ready
        returns (uint256 assets)
    {
        _spendAllowance(owner, shares);
        assets = _convertToAssets(shares, false);
        uint256 actualAssets = _redeem(receiver, owner, assets, shares, maxLoss);
        emit Withdraw(msg.sender, receiver, owner, actualAssets, shares);
        return actualAssets;
    }

    function add_strategy(address strategy, bool addToQueue) external ready {
        _enforceRole(msg.sender, ADD_STRATEGY_MANAGER);
        require(strategy != address(0), "strategy");
        require(strategies[strategy].activation == 0, "active");
        strategies[strategy].activation = block.timestamp;
        if (addToQueue) {
            defaultQueueStrategy = strategy;
        }
        emit StrategyChanged(strategy, block.timestamp);
    }

    function set_role(address account, uint256 role) external ready {
        require(msg.sender == roleManager, "permission");
        roles[account] = role;
        emit RoleSet(account, role);
    }

    function add_role(address account, uint256 role) external ready {
        require(msg.sender == roleManager, "permission");
        uint256 newRoles = roles[account] | role;
        roles[account] = newRoles;
        emit RoleSet(account, newRoles);
    }

    function remove_role(address account, uint256 role) external ready {
        require(msg.sender == roleManager, "permission");
        uint256 newRoles = roles[account] & ~role;
        roles[account] = newRoles;
        emit RoleSet(account, newRoles);
    }

    function transfer_role_manager(address newRoleManager) external ready {
        require(msg.sender == roleManager, "permission");
        futureRoleManager = newRoleManager;
        emit UpdateFutureRoleManager(newRoleManager);
    }

    function accept_role_manager() external ready {
        require(msg.sender == futureRoleManager, "permission");
        roleManager = msg.sender;
        futureRoleManager = address(0);
        emit UpdateRoleManager(msg.sender);
    }

    function setName(string calldata newName) external ready {
        require(msg.sender == roleManager, "permission");
        name = newName;
    }

    function setSymbol(string calldata newSymbol) external ready {
        require(msg.sender == roleManager, "permission");
        symbol = newSymbol;
    }

    function set_deposit_limit(uint256 limit, bool) external ready {
        require(!shutdown, "shutdown");
        _enforceRole(msg.sender, DEPOSIT_LIMIT_MANAGER);
        depositLimit = limit;
    }

    function update_max_debt_for_strategy(address strategy, uint256 maxDebt) external ready {
        _enforceRole(msg.sender, MAX_DEBT_MANAGER);
        require(strategies[strategy].activation != 0, "inactive");
        strategies[strategy].maxDebt = maxDebt;
    }

    function update_debt(address strategy, uint256 targetDebt, uint256 maxLoss)
        external
        ready
        returns (uint256)
    {
        _enforceRole(msg.sender, DEBT_MANAGER);
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
            _safeTransfer(strategy, debtIncrease);
            require(YearnBenchStrategy(strategy).increaseDebt(debtIncrease), "strategy debt");
        } else {
            uint256 debtReduction = previousDebt - targetDebt;
            (uint256 withdrawn, uint256 loss) = YearnBenchStrategy(strategy).withdrawTo(address(this), debtReduction, maxLoss);
            require(debtReduction == 0 || loss * MAX_BPS <= debtReduction * maxLoss, "loss");
            uint256 realized = withdrawn + loss;
            require(realized <= s.currentDebt && realized <= totalDebt, "debt");
            s.balance -= realized;
            s.currentDebt -= realized;
            totalDebt -= realized;
            totalIdle += withdrawn;
        }
        emit DebtUpdated(strategy, previousDebt, s.currentDebt);
        return s.currentDebt;
    }

    function process_report(address strategy) external ready returns (uint256 gain, uint256 loss) {
        _enforceRole(msg.sender, REPORTING_MANAGER);
        Strategy storage s = strategies[strategy];
        require(s.activation != 0, "inactive");
        (gain, loss) = YearnBenchStrategy(strategy).report();
        loss = _min(loss, s.currentDebt);

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
                    fullProfitUnlockDate = block.timestamp + profitMaxUnlockTime;
                    lastProfitUpdate = block.timestamp;
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

    function shutdown_vault() external ready {
        _enforceRole(msg.sender, EMERGENCY_MANAGER);
        require(!shutdown, "shutdown");
        shutdown = true;
        depositLimit = 0;
        uint256 newRoles = roles[msg.sender] | DEBT_MANAGER;
        roles[msg.sender] = newRoles;
        emit RoleSet(msg.sender, newRoles);
        emit Shutdown();
    }

    function setProfitMaxUnlockTime(uint256 newProfitMaxUnlockTime) external ready {
        _enforceRole(msg.sender, PROFIT_UNLOCK_MANAGER);
        require(newProfitMaxUnlockTime <= 31_556_952, "profit unlock time too long");
        if (newProfitMaxUnlockTime == 0) {
            uint256 lockedShares = balanceOf[address(this)];
            if (lockedShares > 0) {
                _burn(address(this), lockedShares);
            }
            profitUnlockingRate = 0;
            fullProfitUnlockDate = 0;
        }
        profitMaxUnlockTime = newProfitMaxUnlockTime;
    }

    function permitDigest(address owner, address spender, uint256 value, uint256 deadline)
        external
        view
        returns (bytes32)
    {
        return _permitDigest(owner, spender, value, deadline);
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return _domainSeparator();
    }

    function role_manager() external view returns (address) {
        return roleManager;
    }

    function future_role_manager() external view returns (address) {
        return futureRoleManager;
    }

    function permit(
        address owner,
        address spender,
        uint256 value,
        uint256 deadline,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external ready returns (bool) {
        require(owner != address(0), "invalid owner");
        require(deadline >= block.timestamp, "permit expired");
        bytes32 digest = _permitDigest(owner, spender, value, deadline);
        address recovered = ecrecover(digest, v, r, s);
        require(recovered == owner, "invalid signature");
        nonces[owner] += 1;
        allowance[owner][spender] = value;
        emit Approval(owner, spender, value);
        return true;
    }

    function totalAssets() public view returns (uint256) {
        return totalIdle + totalDebt;
    }

    function isShutdown() external view returns (bool) {
        return shutdown;
    }

    function unlockedShares() external view returns (uint256) {
        return _unlockedShares();
    }

    function pricePerShare() external view returns (uint256) {
        uint256 supply = _effectiveSupply();
        if (supply == 0) {
            return WAD;
        }
        return totalAssets() * WAD / supply;
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
        if (shutdown || receiver == address(0) || receiver == address(this) || totalAssets() >= depositLimit) {
            return 0;
        }
        return depositLimit - totalAssets();
    }

    function maxMint(address receiver) external view returns (uint256) {
        if (shutdown || receiver == address(0) || receiver == address(this) || totalAssets() >= depositLimit) {
            return 0;
        }
        return _convertToShares(depositLimit - totalAssets(), false);
    }

    function maxWithdraw(address owner, uint256) external view returns (uint256) {
        return _convertToAssets(balanceOf[owner], false);
    }

    function maxRedeem(address owner, uint256) external view returns (uint256) {
        uint256 shares = _convertToShares(_convertToAssets(balanceOf[owner], false), false);
        return _min(shares, balanceOf[owner]);
    }

    function previewWithdraw(uint256 assets) external view returns (uint256) {
        return _convertToShares(assets, true);
    }

    function previewRedeem(uint256 shares) external view returns (uint256) {
        return _convertToAssets(shares, false);
    }

    function apiVersion() external pure returns (string memory) {
        return API_VERSION;
    }

    function strategyState(address strategy)
        external
        view
        returns (uint256 activation, uint256 currentDebt, uint256 maxDebt, uint256 balance)
    {
        Strategy memory s = strategies[strategy];
        return (s.activation, s.currentDebt, s.maxDebt, s.balance);
    }

    function _redeem(address receiver, address owner, uint256 assets, uint256 shares, uint256 maxLoss)
        internal
        returns (uint256)
    {
        require(shares > 0 && balanceOf[owner] >= shares, "shares");
        uint256 actualAssets = _ensureIdle(assets, maxLoss);
        _burn(owner, shares);
        totalIdle -= actualAssets;
        _safeTransfer(receiver, actualAssets);
        return actualAssets;
    }

    function _spendAllowance(address owner, uint256 shares) internal {
        if (msg.sender == owner) {
            return;
        }
        uint256 allowed = allowance[owner][msg.sender];
        if (allowed != type(uint256).max) {
            require(allowed >= shares, "allowance");
            allowance[owner][msg.sender] = allowed - shares;
            emit Approval(owner, msg.sender, allowed - shares);
        }
    }

    function _ensureIdle(uint256 assets, uint256 maxLoss) internal returns (uint256) {
        if (totalIdle >= assets) {
            return assets;
        }
        address strategy = defaultQueueStrategy;
        require(strategy != address(0), "queue");
        Strategy storage s = strategies[strategy];
        uint256 needed = assets - totalIdle;
        (uint256 withdrawn, uint256 loss) = YearnBenchStrategy(strategy).withdrawTo(address(this), needed, maxLoss);
        require(needed == 0 || loss * MAX_BPS <= needed * maxLoss, "loss");
        uint256 realized = withdrawn + loss;
        require(realized <= s.currentDebt && realized <= totalDebt, "debt");
        s.balance -= realized;
        s.currentDebt -= realized;
        totalDebt -= realized;
        totalIdle += withdrawn;
        if (totalIdle < assets) {
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
        if (profitMaxUnlockTime == 0 || block.timestamp >= fullProfitUnlockDate) return locked;
        uint256 elapsed = block.timestamp - lastProfitUpdate;
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

    function _safeTransfer(address to, uint256 value) internal {
        require(YearnBenchERC20(asset).transfer(to, value), "transfer");
    }

    function _safeTransferFrom(address from, address to, uint256 value) internal {
        require(YearnBenchERC20(asset).transferFrom(from, to, value), "transferFrom");
    }

    function _enforceRole(address account, uint256 role) internal view {
        require(roles[account] & role != 0, "not allowed");
    }

    function _domainSeparator() internal view returns (bytes32) {
        return keccak256(
            abi.encode(
                DOMAIN_TYPE_HASH,
                keccak256("Yearn Vault"),
                keccak256("3.0.4"),
                block.chainid,
                address(this)
            )
        );
    }

    function _permitDigest(address owner, address spender, uint256 value, uint256 deadline)
        internal
        view
        returns (bytes32)
    {
        return keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                _domainSeparator(),
                keccak256(abi.encode(PERMIT_TYPE_HASH, owner, spender, value, nonces[owner], deadline))
            )
        );
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

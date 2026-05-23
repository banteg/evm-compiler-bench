// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

contract UniswapV2PairReal {
    uint256 public constant MINIMUM_LIQUIDITY = 1000;

    string public constant name = "Real-Derived Uniswap V2 Pair";
    string public constant symbol = "RD-UNI-V2";
    uint8 public constant decimals = 18;

    address public factory;
    address public feeTo;
    bool public initialized;

    uint112 private reserve0_;
    uint112 private reserve1_;
    uint32 private blockTimestampLast_;
    uint32 private currentTimestamp;

    uint256 public price0CumulativeLast;
    uint256 public price1CumulativeLast;
    uint256 public kLast;
    uint256 public balance0;
    uint256 public balance1;
    uint256 public totalSupply;

    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Mint(address indexed sender, uint256 amount0, uint256 amount1, uint256 liquidity);
    event Burn(address indexed sender, uint256 amount0, uint256 amount1, address indexed to);
    event Swap(
        address indexed sender,
        uint256 amount0In,
        uint256 amount1In,
        uint256 amount0Out,
        uint256 amount1Out,
        address indexed to
    );
    event Sync(uint112 reserve0, uint112 reserve1);

    modifier ready() {
        require(initialized, "not initialized");
        _;
    }

    function initialize(bool feeOn) external returns (bool) {
        require(!initialized, "initialized");
        initialized = true;
        factory = msg.sender;
        currentTimestamp = 1;
        blockTimestampLast_ = 1;
        if (feeOn) {
            feeTo = address(0xFEE);
        }
        return true;
    }

    function seedBalances(uint256 amount0, uint256 amount1) external ready returns (bool) {
        balance0 += amount0;
        balance1 += amount1;
        return true;
    }

    function setFeeTo(address newFeeTo) external ready returns (bool) {
        require(msg.sender == factory, "forbidden");
        feeTo = newFeeTo;
        if (newFeeTo == address(0)) {
            kLast = 0;
        }
        return true;
    }

    function advanceTime(uint256 secondsForward) external ready returns (uint256) {
        currentTimestamp += uint32(secondsForward);
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

    function getReserves() public view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast) {
        return (reserve0_, reserve1_, blockTimestampLast_);
    }

    function mint(address to) external ready returns (uint256 liquidity) {
        (uint112 reserve0, uint112 reserve1,) = getReserves();
        uint256 amount0 = balance0 - reserve0;
        uint256 amount1 = balance1 - reserve1;
        bool feeOn = _mintFee(reserve0, reserve1);
        uint256 supply = totalSupply;
        if (supply == 0) {
            liquidity = _sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY;
            _mint(address(0), MINIMUM_LIQUIDITY);
        } else {
            liquidity = _min(amount0 * supply / reserve0, amount1 * supply / reserve1);
        }
        require(liquidity > 0, "liquidity");
        _mint(to, liquidity);
        _update(balance0, balance1, reserve0, reserve1);
        if (feeOn) {
            kLast = uint256(reserve0_) * uint256(reserve1_);
        }
        emit Mint(msg.sender, amount0, amount1, liquidity);
    }

    function stageBurn(uint256 liquidity) external ready returns (bool) {
        _transfer(msg.sender, address(this), liquidity);
        return true;
    }

    function burn(address to) external ready returns (uint256 amount0, uint256 amount1) {
        (uint112 reserve0, uint112 reserve1,) = getReserves();
        bool feeOn = _mintFee(reserve0, reserve1);
        uint256 liquidity = balanceOf[address(this)];
        uint256 supply = totalSupply;
        amount0 = liquidity * balance0 / supply;
        amount1 = liquidity * balance1 / supply;
        require(amount0 > 0 && amount1 > 0, "liquidity");
        _burn(address(this), liquidity);
        balance0 -= amount0;
        balance1 -= amount1;
        _update(balance0, balance1, reserve0, reserve1);
        if (feeOn) {
            kLast = uint256(reserve0_) * uint256(reserve1_);
        }
        emit Burn(msg.sender, amount0, amount1, to);
    }

    function swap(
        uint256 amount0Out,
        uint256 amount1Out,
        uint256 amount0In,
        uint256 amount1In,
        address to
    ) external ready returns (bool) {
        require(amount0Out > 0 || amount1Out > 0, "output");
        (uint112 reserve0, uint112 reserve1,) = getReserves();
        require(amount0Out < reserve0 && amount1Out < reserve1, "liquidity");
        require(amount0In > 0 || amount1In > 0, "input");

        balance0 = balance0 + amount0In - amount0Out;
        balance1 = balance1 + amount1In - amount1Out;

        uint256 balance0Adjusted = balance0 * 1000 - amount0In * 3;
        uint256 balance1Adjusted = balance1 * 1000 - amount1In * 3;
        require(
            balance0Adjusted * balance1Adjusted >= uint256(reserve0) * uint256(reserve1) * 1_000_000,
            "k"
        );
        _update(balance0, balance1, reserve0, reserve1);
        emit Swap(msg.sender, amount0In, amount1In, amount0Out, amount1Out, to);
        return true;
    }

    function skim(address to) external ready returns (uint256 amount0, uint256 amount1) {
        amount0 = balance0 - reserve0_;
        amount1 = balance1 - reserve1_;
        balance0 = reserve0_;
        balance1 = reserve1_;
        emit Burn(msg.sender, amount0, amount1, to);
    }

    function sync() external ready returns (bool) {
        _update(balance0, balance1, reserve0_, reserve1_);
        return true;
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

    function _update(uint256 newBalance0, uint256 newBalance1, uint112 oldReserve0, uint112 oldReserve1) internal {
        require(newBalance0 <= type(uint112).max && newBalance1 <= type(uint112).max, "overflow");
        uint32 blockTimestamp = currentTimestamp;
        uint32 timeElapsed = blockTimestamp - blockTimestampLast_;
        if (timeElapsed > 0 && oldReserve0 != 0 && oldReserve1 != 0) {
            price0CumulativeLast += uint256(oldReserve1) * 1e18 / oldReserve0 * timeElapsed;
            price1CumulativeLast += uint256(oldReserve0) * 1e18 / oldReserve1 * timeElapsed;
        }
        reserve0_ = uint112(newBalance0);
        reserve1_ = uint112(newBalance1);
        blockTimestampLast_ = blockTimestamp;
        emit Sync(reserve0_, reserve1_);
    }

    function _mintFee(uint112 reserve0, uint112 reserve1) internal returns (bool feeOn) {
        address currentFeeTo = feeTo;
        feeOn = currentFeeTo != address(0);
        uint256 lastK = kLast;
        if (feeOn) {
            if (lastK != 0) {
                uint256 rootK = _sqrt(uint256(reserve0) * uint256(reserve1));
                uint256 rootKLast = _sqrt(lastK);
                if (rootK > rootKLast) {
                    uint256 numerator = totalSupply * (rootK - rootKLast);
                    uint256 denominator = rootK * 5 + rootKLast;
                    uint256 liquidity = numerator / denominator;
                    if (liquidity > 0) {
                        _mint(currentFeeTo, liquidity);
                    }
                }
            }
        } else if (lastK != 0) {
            kLast = 0;
        }
    }

    function _sqrt(uint256 y) internal pure returns (uint256 z) {
        if (y > 3) {
            z = y;
            uint256 x = y / 2 + 1;
            while (x < z) {
                z = x;
                x = (y / x + x) / 2;
            }
        } else if (y != 0) {
            z = 1;
        }
    }

    function _min(uint256 x, uint256 y) internal pure returns (uint256) {
        return x < y ? x : y;
    }
}

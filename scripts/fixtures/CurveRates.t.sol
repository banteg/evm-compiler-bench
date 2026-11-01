// SPDX-License-Identifier: MIT
pragma solidity ^0.8.35;

interface Vm {
    function getCode(string calldata artifact) external view returns (bytes memory);
    function readFileBinary(string calldata path) external view returns (bytes memory);
}

// The same endpoint can act as the oracle, the ERC4626 share, and its underlying.
contract RateFixture {
    uint256 public rateValue = 1e18;
    uint256 public replySize = 32;
    bool public fail;

    function configure(uint256 value, uint256 size, bool shouldFail) external {
        rateValue = value;
        replySize = size;
        fail = shouldFail;
    }

    function decimals() external pure returns (uint8) { return 17; }
    function asset() external view returns (address) { return address(this); }
    function convertToAssets(uint256 shares) external view returns (uint256) {
        require(shares == 1e17);
        return rateValue;
    }

    fallback() external {
        require(!fail && msg.data.length == 32 && bytes32(msg.data) == bytes32(bytes4(keccak256("rate()"))));
        uint256 value = rateValue;
        uint256 size = replySize;
        assembly {
            mstore(0, value)
            mstore(32, not(value))
            return(0, size)
        }
    }
}

contract CurveRatesTest {
    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    RateFixture fixture = new RateFixture();

    function deploy(bytes memory bytecode, uint8 assetType) internal returns (address pool) {
        address[] memory coins = new address[](2);
        coins[0] = address(fixture);
        coins[1] = address(1);
        uint256[] memory rates = new uint256[](2);
        rates[0] = 1e18;
        rates[1] = 1e18;
        uint8[] memory types_ = new uint8[](2);
        types_[0] = assetType;
        bytes4[] memory methods = new bytes4[](2);
        methods[0] = bytes4(keccak256("rate()"));
        address[] memory oracles = new address[](2);
        oracles[0] = address(fixture);
        bytes memory initcode = bytes.concat(bytecode, abi.encode(
            "Curve", "CRV", uint256(200), uint256(4000000), uint256(5000000000),
            uint256(100), coins, rates, types_, methods, oracles
        ));
        assembly { pool := create(0, add(initcode, 32), mload(initcode)) }
        require(pool != address(0), "deployment");
    }

    function expectRate(address pool, uint256 expected) internal view {
        (bool ok, bytes memory raw) = pool.staticcall(abi.encodeWithSignature("stored_rates()"));
        require(ok, "stored_rates reverted");
        uint256[] memory rates = abi.decode(raw, (uint256[]));
        require(rates.length == 2 && rates[0] == expected && rates[1] == 1e18, "wrong rates");
    }

    function expectRevert(address pool) internal view {
        (bool ok,) = pool.staticcall(abi.encodeWithSignature("stored_rates()"));
        require(!ok, "stored_rates should revert");
    }

    function checkPort(bytes memory bytecode) internal {
        address oraclePool = deploy(bytecode, 1);
        expectRate(oraclePool, 1e18);
        fixture.configure(7, 64, false);
        expectRate(oraclePool, 7);
        fixture.configure(7, 31, false);
        expectRevert(oraclePool);
        fixture.configure(7, 32, true);
        expectRevert(oraclePool);
        fixture.configure(1 << 255, 32, false);
        expectRevert(oraclePool);

        address vaultPool = deploy(bytecode, 3);
        fixture.configure(7, 32, false);
        expectRate(vaultPool, 70);
        fixture.configure(1 << 255, 32, false);
        expectRevert(vaultPool);
        // First multiplication fits, the subsequent 10x scale factor overflows.
        fixture.configure(type(uint256).max / 1e18, 32, false);
        expectRevert(vaultPool);
    }

    function testSolidity() external { checkPort(vm.getCode("CurveStableSwap2CoinReal.sol:CurveStableSwap2CoinReal")); }
    function testVyper() external { checkPort(vm.readFileBinary("vyper.bin")); }
    function testFe() external { checkPort(vm.readFileBinary("fe.bin")); }
}

// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

{{SUPPORT_CONTRACTS}}
interface Vm {
    struct Log { bytes32[] topics; bytes data; address emitter; }
    function createDir(string calldata path, bool recursive) external;
    function writeFile(string calldata path, string calldata data) external;
    function writeLine(string calldata path, string calldata data) external;
    function toString(uint256 value) external pure returns (string memory);
    function prank(address sender) external;
    function deal(address account, uint256 newBalance) external;
    function warp(uint256 newTimestamp) external;
    function chainId(uint256 newChainId) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function addr(uint256 privateKey) external returns (address);
    function recordLogs() external;
    function getRecordedLogs() external returns (Log[] memory entries);
}

contract {{CONTRACT_NAME}} {
    Vm constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    string constant GAS_JSONL_PATH = "{{GAS_JSONL_PATH}}";
    address constant BOB = address(0xB0B);
    address constant CAROL = address(0xCAFe);
    address constant IMPLEMENTATION = address(0x1000000000000000000000000000000000000001);
    bytes32 constant SALT = keccak256("evm-compiler-bench");
    bytes32 constant LEAF = keccak256("leaf");
    bytes32 constant SIBLING = keccak256("sibling");
    bytes32 constant ROOT = LEAF < SIBLING ? keccak256(abi.encodePacked(LEAF, SIBLING)) : keccak256(abi.encodePacked(SIBLING, LEAF));

    uint256 constant UNISWAP_PERMIT_KEY = 0xB0BA;
    bytes32 constant UNISWAP_PERMIT_TYPE_HASH = keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    uint256 constant CURVE_PERMIT_KEY = 0xC0FFEE;
    bytes32 constant CURVE_PERMIT_TYPE_HASH = keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    uint256 constant YEARN_PERMIT_KEY = 0xA11CE;
    bytes32 constant YEARN_PERMIT_TYPE_HASH = keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)");

    struct PairDeps { BenchERC20 token0; BenchERC20 token1; BenchUniswapFlashCallee flashCallee; BenchUniswapReentrantCallee reentrantCallee; BenchUniswapCreate2Factory factory; }
    struct NoReturnPairDeps { BenchERC20NoReturn token0; BenchERC20NoReturn token1; }
    struct CurveDeps { BenchERC20OptionalReturn coin0; BenchERC20OptionalReturn coin1; BenchERC20OptionalReturn coin2; BenchERC20OptionalReturn coin3; BenchERC20OptionalReturn coin4; BenchERC20OptionalReturn coin5; BenchERC20OptionalReturn coin6; BenchERC20OptionalReturn coin7; }
    struct YearnDeps { BenchERC20 asset; BenchYearnStrategy strategy; BenchYearnStrategy strategy2; BenchYearnStrategy strategy3; BenchYearnAccountant accountant; BenchYearnMutatingAccountant mutatingAccountant; BenchYearnReentrantAccountant reentrantAccountant; BenchYearnDepositLimitModule depositLimitModule; BenchYearnWithdrawLimitModule withdrawLimitModule; }
    mapping(address => PairDeps) internal pairDeps;
    mapping(address => NoReturnPairDeps) internal noReturnPairDeps;
    mapping(address => CurveDeps) internal curveDeps;
    mapping(address => YearnDeps) internal yearnDeps;
    BenchERC1271Wallet internal curve1271Owner;
    address public feeTo;
    uint16 public protocolFeeBps;
    address public protocolFeeRecipient;

    receive() external payable {}

    function setUp() public {
        vm.writeFile(GAS_JSONL_PATH, "");
        vm.createDir("{{FAILURE_DIR}}", true);
        vm.deal(address(this), 1000000 ether);
        vm.deal(BOB, 1000000 ether);
        vm.deal(CAROL, 1000000 ether);
        vm.warp(1);
    }

{{CONTRACT_BODY}}}

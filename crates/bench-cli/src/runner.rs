use crate::{
    cache::{self, CacheLookup},
    models::{
        CacheInfo, CallDestination, CallSpec, CompileSet, CompiledArtifact, GasRecord,
        PropertySpec, RandomizedSpec, Scenario,
    },
    scenarios::ScenarioCatalog,
    util::{Progress, ensure_dir, require_success, run_measured, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const FAILURE_DIR: &str = "../results/raw/failures";
const SOL_BASELINE: &str = "solc-latest-legacy-runs200";
const VYPER_BASELINE: &str = "vyper-latest-gas";
const GAS_CACHE_SCHEMA: &str = "gas-v1";
const MAX_ARTIFACTS_PER_GAS_SHARD: usize = 220;

pub fn run_foundry(
    root: &Path,
    evm_version: &str,
    compiled: &CompileSet,
    scenarios: &ScenarioCatalog,
    use_cache: bool,
) -> Result<Vec<GasRecord>> {
    ensure_dir(&root.join("results/raw"))?;
    clear_failure_dir(root)?;
    if compiled.artifacts.is_empty() {
        fs::write(root.join("results/raw/foundry-gas.jsonl"), "")?;
        return Ok(Vec::new());
    }
    let expected_cache = gas_cache_inputs(root, evm_version, compiled, scenarios, use_cache)?;
    if use_cache {
        let mut progress = Progress::new("gas cache", expected_cache.len());
        let mut cached = Vec::with_capacity(expected_cache.len());
        let mut all_hit = true;
        for (index, input) in expected_cache.values().enumerate() {
            match cache::lookup::<GasRecord>(
                root,
                "gas",
                &input.logical_id,
                &input.key,
                &input.fingerprint,
            )? {
                CacheLookup::Hit(mut record) => {
                    record.cache = CacheInfo::hit(&input.key);
                    cached.push(record);
                    progress.update(index + 1, "hit");
                }
                CacheLookup::Miss(_) => {
                    all_hit = false;
                    progress.update(index + 1, "miss; Foundry run required");
                    break;
                }
            }
        }
        if all_hit {
            progress.finish(format!("loaded {} rows from cache", cached.len()));
            write_raw_gas_records(root, &cached)?;
            return Ok(cached);
        }
        progress.finish("cache incomplete; running Foundry");
    } else {
        eprintln!(
            "gas: cache disabled; running Foundry for {} expected rows",
            expected_cache.len()
        );
    }
    clear_generated_shards(root)?;
    let shards = gas_shards(&compiled.artifacts);
    eprintln!(
        "foundry: generating {} gas test shards for {} artifacts and {} expected rows",
        shards.len(),
        compiled.artifacts.len(),
        expected_cache.len()
    );
    let mut records = Vec::new();
    let mut progress = Progress::new("foundry", shards.len());
    for (index, shard_artifacts) in shards.iter().enumerate() {
        let shard_id = format!("{index:03}");
        let contract_name = format!("GeneratedBenchShard{shard_id}");
        let test_file = format!("{contract_name}.t.sol");
        let match_path = format!("test/{test_file}");
        let gas_jsonl = format!("../results/raw/foundry-gas-shard-{shard_id}.jsonl");
        let test_path = root.join("foundry/test").join(&test_file);
        fs::write(
            &test_path,
            generate_test(&contract_name, shard_artifacts, scenarios, &gas_jsonl)?,
        )
        .with_context(|| format!("writing {}", test_path.display()))?;
        progress.update(
            index,
            format!(
                "running shard {}/{} ({} artifacts)",
                index + 1,
                shards.len(),
                shard_artifacts.len()
            ),
        );
        require_success(
            run_measured(
                Command::new("forge")
                    .arg("test")
                    .arg("--root")
                    .arg(root.join("foundry"))
                    .arg("--match-path")
                    .arg(&match_path)
                    .arg("--evm-version")
                    .arg(evm_version)
                    .arg("--via-ir")
                    .arg("--optimize")
                    .arg("-q"),
                None,
            )?,
            &format!("forge test {match_path}"),
        )?;
        let shard_rows = read_gas_records(
            &root.join(format!("results/raw/foundry-gas-shard-{shard_id}.jsonl")),
        )?;
        records.extend(shard_rows);
        progress.update(
            index + 1,
            format!("completed shard {}/{}", index + 1, shards.len()),
        );
    }
    progress.finish(format!("recorded {} gas rows", records.len()));
    annotate_and_store_gas_records(root, &mut records, &expected_cache, use_cache)?;
    write_raw_gas_records(root, &records)?;
    eprintln!("foundry: recorded {} gas rows", records.len());
    Ok(records)
}

#[derive(Debug, Clone)]
struct GasCacheInput {
    key: String,
    logical_id: String,
    fingerprint: serde_json::Value,
    lookup_info: CacheInfo,
}

fn gas_cache_inputs(
    root: &Path,
    evm_version: &str,
    compiled: &CompileSet,
    scenarios: &ScenarioCatalog,
    use_cache: bool,
) -> Result<BTreeMap<String, GasCacheInput>> {
    let mut inputs = BTreeMap::new();
    for artifact in &compiled.artifacts {
        for scenario in &scenarios.get(&artifact.benchmark_id)?.scenarios {
            let fingerprint = gas_fingerprint(evm_version, artifact, scenario)?;
            let key = cache::key_for(&fingerprint)?;
            let logical_id = cache::logical_id(&[
                "gas",
                &artifact.benchmark_id,
                &artifact.implementation_id,
                &artifact.profile_id,
                &scenario.name,
                scenario.state_access_profile.as_str(),
            ]);
            let lookup_info = if use_cache {
                match cache::lookup::<GasRecord>(root, "gas", &logical_id, &key, &fingerprint)? {
                    CacheLookup::Hit(_) => CacheInfo::refreshed(&key),
                    CacheLookup::Miss(info) => info,
                }
            } else {
                CacheInfo::disabled()
            };
            inputs.insert(
                gas_record_key(
                    &artifact.benchmark_id,
                    &artifact.implementation_id,
                    &artifact.profile_id,
                    &scenario.name,
                    scenario.state_access_profile.as_str(),
                ),
                GasCacheInput {
                    key,
                    logical_id,
                    fingerprint,
                    lookup_info,
                },
            );
        }
    }
    Ok(inputs)
}

fn gas_fingerprint(
    evm_version: &str,
    artifact: &CompiledArtifact,
    scenario: &Scenario,
) -> Result<serde_json::Value> {
    Ok(json!({
        "schema": GAS_CACHE_SCHEMA,
        "evm_version": evm_version,
        "runner": {
            "name": "foundry-generated-bench",
            "version": "1",
            "gas_json_schema": "1",
        },
        "artifact": {
            "benchmark_id": artifact.benchmark_id,
            "implementation_id": artifact.implementation_id,
            "profile_id": artifact.profile_id,
            "language": artifact.language.as_str(),
            "metadata_mode": artifact.metadata_mode.as_str(),
            "source_hash": artifact.source_hash,
            "creation_bytecode_hash": sha256_bytes(artifact.creation_bytecode.as_bytes()),
            "runtime_bytecode_hash": sha256_bytes(artifact.runtime_bytecode.as_bytes()),
            "compiler": {
                "name": artifact.compiler.name,
                "version": artifact.compiler.version,
                "binary_sha256": artifact.compiler.binary_sha256,
            },
            "compiler_settings": artifact.compiler_settings,
        },
        "scenario": scenario,
    }))
}

fn annotate_and_store_gas_records(
    root: &Path,
    records: &mut [GasRecord],
    expected_cache: &BTreeMap<String, GasCacheInput>,
    use_cache: bool,
) -> Result<()> {
    for record in records {
        let key = gas_record_key(
            &record.benchmark_id,
            &record.implementation_id,
            &record.profile_id,
            &record.scenario,
            record.state_access_profile.as_str(),
        );
        if let Some(input) = expected_cache.get(&key) {
            record.cache = if use_cache {
                input.lookup_info.clone()
            } else {
                CacheInfo::disabled()
            };
            if use_cache {
                cache::store(
                    root,
                    "gas",
                    &input.logical_id,
                    &input.key,
                    &input.fingerprint,
                    record,
                )?;
            }
        } else {
            record.cache = CacheInfo::disabled();
        }
    }
    Ok(())
}

fn write_raw_gas_records(root: &Path, records: &[GasRecord]) -> Result<()> {
    let rows_path = root.join("results/raw/foundry-gas.jsonl");
    let mut out = String::new();
    for record in records {
        out.push_str(&serde_json::to_string(record)?);
        out.push('\n');
    }
    fs::write(&rows_path, out).with_context(|| format!("writing {}", rows_path.display()))?;
    Ok(())
}

fn read_gas_records(path: &Path) -> Result<Vec<GasRecord>> {
    let rows = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in rows.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str(line)
                .with_context(|| format!("parsing gas jsonl line {}", index + 1))?,
        );
    }
    Ok(records)
}

fn gas_shards(artifacts: &[CompiledArtifact]) -> Vec<Vec<CompiledArtifact>> {
    let mut groups: Vec<(String, Vec<CompiledArtifact>)> = Vec::new();
    let mut positions = BTreeMap::new();
    for artifact in artifacts {
        let index = *positions
            .entry(artifact.benchmark_id.clone())
            .or_insert_with(|| {
                groups.push((artifact.benchmark_id.clone(), Vec::new()));
                groups.len() - 1
            });
        groups[index].1.push(artifact.clone());
    }

    let mut shards = Vec::new();
    let mut current = Vec::new();
    for (_, mut group) in groups {
        if !current.is_empty() && current.len() + group.len() > MAX_ARTIFACTS_PER_GAS_SHARD {
            shards.push(current);
            current = Vec::new();
        }
        current.append(&mut group);
    }
    if !current.is_empty() {
        shards.push(current);
    }
    shards
}

fn gas_record_key(
    benchmark_id: &str,
    implementation_id: &str,
    profile_id: &str,
    scenario: &str,
    state_access_profile: &str,
) -> String {
    format!("{benchmark_id}\0{implementation_id}\0{profile_id}\0{scenario}\0{state_access_profile}")
}

fn clear_failure_dir(root: &Path) -> Result<()> {
    let failure_dir = root.join("results/raw/failures");
    ensure_dir(&failure_dir)?;
    for entry in fs::read_dir(&failure_dir)? {
        let path = entry?.path();
        if path.is_file() {
            fs::remove_file(&path)
                .with_context(|| format!("removing stale failure {}", path.display()))?;
        }
    }
    Ok(())
}

fn clear_generated_shards(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root.join("foundry/test"))? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("GeneratedBenchShard") && name.ends_with(".t.sol") {
            fs::remove_file(&path)
                .with_context(|| format!("removing stale generated test {}", path.display()))?;
        }
    }
    for entry in fs::read_dir(root.join("results/raw"))? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("foundry-gas-shard-") && name.ends_with(".jsonl") {
            fs::remove_file(&path)
                .with_context(|| format!("removing stale gas shard {}", path.display()))?;
        }
    }
    Ok(())
}

fn generate_test(
    contract_name: &str,
    artifacts: &[CompiledArtifact],
    scenarios: &ScenarioCatalog,
    gas_jsonl: &str,
) -> Result<String> {
    if artifacts.is_empty() {
        bail!("no compiled artifacts for Foundry runner");
    }
    let mut out = String::new();
    out.push_str("// SPDX-License-Identifier: MIT\n");
    out.push_str("pragma solidity ^0.8.20;\n\n");
    out.push_str(support_contracts());
    out.push_str("interface Vm {\n");
    out.push_str("    function createDir(string calldata path, bool recursive) external;\n");
    out.push_str("    function writeFile(string calldata path, string calldata data) external;\n");
    out.push_str("    function writeLine(string calldata path, string calldata data) external;\n");
    out.push_str("    function toString(uint256 value) external pure returns (string memory);\n");
    out.push_str("    function prank(address sender) external;\n");
    out.push_str("    function deal(address account, uint256 newBalance) external;\n");
    out.push_str("    function warp(uint256 newTimestamp) external;\n");
    out.push_str("    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);\n");
    out.push_str("    function addr(uint256 privateKey) external returns (address);\n");
    out.push_str("}\n\n");
    out.push_str("contract ");
    out.push_str(contract_name);
    out.push_str(" {\n");
    out.push_str(
        "    Vm constant vm = Vm(address(uint160(uint256(keccak256(\"hevm cheat code\")))));\n",
    );
    out.push_str("    string constant GAS_JSONL_PATH = \"");
    out.push_str(gas_jsonl);
    out.push_str("\";\n");
    out.push_str("    address constant BOB = address(0xB0B);\n");
    out.push_str("    address constant CAROL = address(0xCAFe);\n");
    out.push_str("    address constant IMPLEMENTATION = address(0x1000000000000000000000000000000000000001);\n");
    out.push_str("    bytes32 constant SALT = keccak256(\"evm-compiler-bench\");\n");
    out.push_str("    bytes32 constant LEAF = keccak256(\"leaf\");\n");
    out.push_str("    bytes32 constant SIBLING = keccak256(\"sibling\");\n");
    out.push_str("    bytes32 constant ROOT = LEAF < SIBLING ? keccak256(abi.encodePacked(LEAF, SIBLING)) : keccak256(abi.encodePacked(SIBLING, LEAF));\n\n");
    out.push_str("    uint256 constant UNISWAP_PERMIT_KEY = 0xB0BA;\n");
    out.push_str("    bytes32 constant UNISWAP_PERMIT_TYPE_HASH = keccak256(\"Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)\");\n\n");
    out.push_str("    uint256 constant CURVE_PERMIT_KEY = 0xC0FFEE;\n");
    out.push_str("    bytes32 constant CURVE_PERMIT_TYPE_HASH = keccak256(\"Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)\");\n\n");
    out.push_str("    uint256 constant YEARN_PERMIT_KEY = 0xA11CE;\n");
    out.push_str("    bytes32 constant YEARN_PERMIT_TYPE_HASH = keccak256(\"Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)\");\n\n");
    out.push_str("    struct PairDeps { BenchERC20 token0; BenchERC20 token1; BenchUniswapFlashCallee flashCallee; BenchUniswapReentrantCallee reentrantCallee; }\n");
    out.push_str(
        "    struct NoReturnPairDeps { BenchERC20NoReturn token0; BenchERC20NoReturn token1; }\n",
    );
    out.push_str("    struct CurveDeps { BenchERC20OptionalReturn coin0; BenchERC20OptionalReturn coin1; }\n");
    out.push_str("    struct YearnDeps { BenchERC20 asset; BenchYearnStrategy strategy; BenchYearnStrategy strategy2; BenchYearnStrategy strategy3; BenchYearnAccountant accountant; BenchYearnReentrantAccountant reentrantAccountant; BenchYearnDepositLimitModule depositLimitModule; BenchYearnWithdrawLimitModule withdrawLimitModule; }\n");
    out.push_str("    mapping(address => PairDeps) internal pairDeps;\n");
    out.push_str("    mapping(address => NoReturnPairDeps) internal noReturnPairDeps;\n");
    out.push_str("    mapping(address => CurveDeps) internal curveDeps;\n");
    out.push_str("    mapping(address => YearnDeps) internal yearnDeps;\n");
    out.push_str("    address public feeTo;\n");
    out.push_str("    uint16 public protocolFeeBps;\n");
    out.push_str("    address public protocolFeeRecipient;\n\n");
    out.push_str("    receive() external payable {}\n\n");
    out.push_str("    function setUp() public {\n");
    out.push_str("        vm.writeFile(GAS_JSONL_PATH, \"\");\n");
    out.push_str("        vm.createDir(\"");
    out.push_str(FAILURE_DIR);
    out.push_str("\", true);\n");
    out.push_str("        vm.deal(address(this), 1000000 ether);\n");
    out.push_str("        vm.deal(BOB, 1000000 ether);\n");
    out.push_str("        vm.deal(CAROL, 1000000 ether);\n");
    out.push_str("        vm.warp(1);\n");
    out.push_str("    }\n\n");
    out.push_str(helper_functions());
    out.push_str(randomized_helper_functions());

    for (index, artifact) in artifacts.iter().enumerate() {
        write_deploy_function(&mut out, index, artifact);
    }

    for (index, artifact) in artifacts.iter().enumerate() {
        for scenario in &scenarios.get(&artifact.benchmark_id)?.scenarios {
            write_gas_test(&mut out, index, artifact, scenario);
        }
    }

    let baselines = baseline_pairs(artifacts);
    for (benchmark_id, (solidity_idx, vyper_idx)) in &baselines {
        for scenario in &scenarios.get(benchmark_id)?.scenarios {
            write_diff_test(
                &mut out,
                benchmark_id,
                *solidity_idx,
                *vyper_idx,
                artifacts
                    .get(*solidity_idx)
                    .context("missing solidity baseline")?,
                artifacts
                    .get(*vyper_idx)
                    .context("missing vyper baseline")?,
                scenario,
            );
        }
    }

    for (benchmark_id, (solidity_idx, vyper_idx)) in &baselines {
        let scenario_file = scenarios.get(benchmark_id)?;
        if let Some(randomized) = &scenario_file.randomized {
            write_randomized_diff_test(
                &mut out,
                benchmark_id,
                *solidity_idx,
                *vyper_idx,
                randomized,
            )?;
        }
        for property in &scenario_file.properties {
            write_property_test(
                &mut out,
                benchmark_id,
                *solidity_idx,
                *vyper_idx,
                scenario_file.randomized.as_ref(),
                property,
            )?;
        }
    }

    out.push_str("}\n");
    Ok(out)
}

fn support_contracts() -> &'static str {
    r#"
contract BenchERC20 {
    string public constant name = "Bench Token";
    string public constant symbol = "BENCH";
    uint8 public constant decimals = 18;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Transfer(address indexed from, address indexed to, uint256 value);

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

    function _transfer(address from, address to, uint256 value) internal {
        require(balanceOf[from] >= value, "balance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        emit Transfer(from, to, value);
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

contract BenchYearnStrategy {
    BenchERC20 public immutable asset;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    uint256 public pendingGain;
    uint256 public pendingLoss;

    constructor(BenchERC20 asset_) {
        asset = asset_;
    }

    function maxDeposit(address) external pure returns (uint256) {
        return type(uint256).max;
    }

    function maxRedeem(address owner) external view returns (uint256) {
        return balanceOf[owner];
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
        shares = assets;
        totalSupply += shares;
        balanceOf[receiver] += shares;
    }

    function redeem(uint256 shares, address receiver, address owner) external returns (uint256 assets) {
        require(msg.sender == owner, "owner");
        require(balanceOf[owner] >= shares, "shares");
        assets = convertToAssets(shares);
        balanceOf[owner] -= shares;
        totalSupply -= shares;
        require(asset.transfer(receiver, assets), "transfer");
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

    function report(address, uint256, uint256) external returns (uint256 fees, uint256 refunds) {
        fees = totalFees;
        refunds = totalRefunds;
        totalFees = 0;
        totalRefunds = 0;
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

    function setLimit(uint256 limit_) external returns (bool) {
        limit = limit_;
        return true;
    }

    function available_deposit_limit(address) external view returns (uint256) {
        return limit;
    }
}

contract BenchYearnWithdrawLimitModule {
    uint256 public limit;

    function setLimit(uint256 limit_) external returns (bool) {
        limit = limit_;
        return true;
    }

    function available_withdraw_limit(address, uint256, address[] calldata) external view returns (uint256) {
        return limit;
    }
}

"#
}

fn helper_functions() -> &'static str {
    r#"
    function proofOne() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](1);
        proof[0] = SIBLING;
    }

    function proofEmpty() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](0);
    }

    function proofMany(uint256 n) internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](n);
        for (uint256 i = 0; i < n; i++) {
            proof[i] = keccak256(abi.encodePacked("sibling", i));
        }
    }

    function proofRoot(bytes32[] memory proof, bytes32 leaf) internal pure returns (bytes32 computed) {
        computed = leaf;
        for (uint256 i = 0; i < proof.length; i++) {
            bytes32 sibling = proof[i];
            computed = computed < sibling
                ? keccak256(abi.encodePacked(computed, sibling))
                : keccak256(abi.encodePacked(sibling, computed));
        }
    }

    function curveAmounts(uint256 amount0, uint256 amount1) internal pure returns (uint256[] memory amounts) {
        amounts = new uint256[](2);
        amounts[0] = amount0;
        amounts[1] = amount1;
    }

    function benchWarp(uint256 secondsForward) external returns (bool) {
        vm.warp(block.timestamp + secondsForward);
        return true;
    }

    function fee_receiver() external pure returns (address) {
        return address(0);
    }

    function admin() external view returns (address) {
        return address(this);
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
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j);
        (,, uint256[2] memory xp) = benchCurveRatesBalancesXp(pool);
        return benchCurveDynamicFeeXp(xp[coinIn], xp[coinOut], benchCurveUint(pool, "fee()"), benchCurveUint(pool, "offpeg_fee_multiplier()"));
    }

    function benchCurveGetDy(int128 i, int128 j, uint256 dx, address pool) internal view returns (uint256) {
        require(dx > 0, "curve dx");
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j);
        (uint256[2] memory rates,, uint256[2] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        uint256 d = benchCurveGetD(xp, amp);
        uint256 x = xp[coinIn] + dx * rates[coinIn] / 1e18;
        uint256 y = benchCurveGetY(coinIn, coinOut, x, xp, amp, d);
        uint256 dy = xp[coinOut] - y - 1;
        uint256 feeAmount = benchCurveDynamicFeeXp((xp[coinIn] + x) / 2, (xp[coinOut] + y) / 2, benchCurveUint(pool, "fee()"), benchCurveUint(pool, "offpeg_fee_multiplier()")) * dy / 10_000_000_000;
        return (dy - feeAmount) * 1e18 / rates[coinOut];
    }

    function benchCurveGetDx(int128 i, int128 j, uint256 dy, address pool) internal view returns (uint256) {
        require(dy > 0, "curve dy");
        (uint256 coinIn, uint256 coinOut) = benchCurveCoinPair(i, j);
        (uint256[2] memory rates,, uint256[2] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        uint256 d = benchCurveGetD(xp, amp);
        uint256 dyWithFee = dy * rates[coinOut] / 1e18 + 1;
        uint256 feeAmount = benchCurveDynamicFeeXp(xp[coinIn], xp[coinOut], benchCurveUint(pool, "fee()"), benchCurveUint(pool, "offpeg_fee_multiplier()"));
        uint256 y = xp[coinOut] - dyWithFee * 10_000_000_000 / (10_000_000_000 - feeAmount);
        uint256 x = benchCurveGetY(coinOut, coinIn, y, xp, amp, d);
        return (x - xp[coinIn]) * 1e18 / rates[coinIn];
    }

    function benchCurveCalcTokenAmount(uint256[] calldata amounts, bool isDeposit, address pool)
        internal
        view
        returns (uint256)
    {
        require(amounts.length >= 2, "curve amounts");
        (uint256[2] memory rates, uint256[2] memory oldBalances, uint256[2] memory xp) = benchCurveRatesBalancesXp(pool);
        uint256 amp = benchCurveUint(pool, "A()") * 100;
        uint256 d0 = benchCurveGetD(xp, amp);
        uint256[2] memory newBalances = oldBalances;
        for (uint256 i = 0; i < 2; i++) {
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
        uint256 baseFee = benchCurveUint(pool, "fee()") * 2 / 4;
        uint256 feeMultiplier = benchCurveUint(pool, "offpeg_fee_multiplier()");
        uint256 ys = (d0 + d1) / 2;
        for (uint256 i = 0; i < 2; i++) {
            uint256 idealBalance = d1 * oldBalances[i] / d0;
            uint256 difference = idealBalance > newBalances[i] ? idealBalance - newBalances[i] : newBalances[i] - idealBalance;
            uint256 xs = rates[i] * (oldBalances[i] + newBalances[i]) / 1e18;
            newBalances[i] -= benchCurveDynamicFeeXp(xs, ys, baseFee, feeMultiplier) * difference / 10_000_000_000;
            xp[i] = rates[i] * newBalances[i] / 1e18;
        }
        uint256 d2 = benchCurveGetD(xp, amp);
        return isDeposit ? (d2 - d0) * totalSupply / d0 : (d0 - d2) * totalSupply / d0;
    }

    function benchCurveRatesBalancesXp(address pool)
        internal
        view
        returns (uint256[2] memory rates, uint256[2] memory balances, uint256[2] memory xp)
    {
        uint256[] memory rawRates = benchCurveUintArray(pool, "stored_rates()");
        uint256[] memory rawBalances = benchCurveUintArray(pool, "get_balances()");
        require(rawRates.length >= 2 && rawBalances.length >= 2, "curve arrays");
        for (uint256 i = 0; i < 2; i++) {
            rates[i] = rawRates[i];
            balances[i] = rawBalances[i];
            xp[i] = rawRates[i] * rawBalances[i] / 1e18;
        }
    }

    function benchCurveCoinPair(int128 i, int128 j) internal pure returns (uint256 coinIn, uint256 coinOut) {
        require(i >= 0 && j >= 0 && i != j, "curve coin");
        coinIn = uint256(int256(i));
        coinOut = uint256(int256(j));
        require(coinIn < 2 && coinOut < 2, "curve coin");
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

    function benchCurveGetD(uint256[2] memory xp, uint256 amp) internal pure returns (uint256) {
        uint256 sum = xp[0] + xp[1];
        if (sum == 0) {
            return 0;
        }
        uint256 d = sum;
        uint256 ann = amp * 2;
        for (uint256 i = 0; i < 255; i++) {
            uint256 dP = d * d / (xp[0] * 2);
            dP = dP * d / (xp[1] * 2);
            uint256 previousD = d;
            d = (ann * sum / 100 + dP * 2) * d / ((ann - 100) * d / 100 + 3 * dP);
            if (d > previousD) {
                if (d - previousD <= 1) return d;
            } else if (previousD - d <= 1) {
                return d;
            }
        }
        revert("curve D");
    }

    function benchCurveGetY(uint256 i, uint256 j, uint256 x, uint256[2] memory xp, uint256 amp, uint256 d)
        internal
        pure
        returns (uint256)
    {
        require(i != j && i < 2 && j < 2, "curve y coin");
        uint256 c = d;
        uint256 s;
        for (uint256 idx = 0; idx < 2; idx++) {
            if (idx == j) {
                continue;
            }
            uint256 currentX = idx == i ? x : xp[idx];
            s += currentX;
            c = c * d / (currentX * 2);
        }
        c = c * d * 100 / (amp * 2);
        uint256 b = s + d * 100 / amp;
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

    function protocol_fee_config() external view returns (uint16, address) {
        return (protocolFeeBps, protocolFeeRecipient);
    }

    function benchYearnSetProtocolFee(uint16 feeBps, address recipient) external returns (bool) {
        protocolFeeBps = feeBps;
        protocolFeeRecipient = recipient;
        return true;
    }

    function benchUniswapInit(address target, bool feeOn) external returns (bool) {
        BenchERC20 token0 = new BenchERC20();
        BenchERC20 token1 = new BenchERC20();
        BenchUniswapFlashCallee flashCallee = new BenchUniswapFlashCallee();
        BenchUniswapReentrantCallee reentrantCallee = new BenchUniswapReentrantCallee();
        pairDeps[target] = PairDeps(token0, token1, flashCallee, reentrantCallee);
        feeTo = feeOn ? address(0xFEE) : address(0);
        (bool ok,) = target.call(
            abi.encodeWithSignature("initialize(address,address)", address(token0), address(token1))
        );
        require(ok, "pair init");
        return true;
    }

    function benchUniswapInitNoReturn(address target, bool feeOn) external returns (bool) {
        BenchERC20NoReturn token0 = new BenchERC20NoReturn();
        BenchERC20NoReturn token1 = new BenchERC20NoReturn();
        noReturnPairDeps[target] = NoReturnPairDeps(token0, token1);
        feeTo = feeOn ? address(0xFEE) : address(0);
        (bool ok,) = target.call(
            abi.encodeWithSignature("initialize(address,address)", address(token0), address(token1))
        );
        require(ok, "pair init");
        return true;
    }

    function benchUniswapToken0(address target) public view returns (address) {
        return address(pairDeps[target].token0);
    }

    function benchUniswapToken1(address target) public view returns (address) {
        return address(pairDeps[target].token1);
    }

    function benchUniswapFlashCallee(address target) public view returns (address) {
        return address(pairDeps[target].flashCallee);
    }

    function benchUniswapReentrantCallee(address target) public view returns (address) {
        return address(pairDeps[target].reentrantCallee);
    }

    function benchUniswapSeed(address target, uint256 amount0, uint256 amount1) external returns (bool) {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(target, amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(target, amount1);
        }
        return true;
    }

    function benchUniswapSeedNoReturn(address target, uint256 amount0, uint256 amount1) external returns (bool) {
        NoReturnPairDeps storage deps = noReturnPairDeps[target];
        require(address(deps.token0) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(target, amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(target, amount1);
        }
        return true;
    }

    function benchUniswapFundFlashCallee(address target, uint256 amount0, uint256 amount1)
        external
        returns (bool)
    {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.flashCallee) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(address(deps.flashCallee), amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(address(deps.flashCallee), amount1);
        }
        return true;
    }

    function benchUniswapFundReentrantCallee(address target, uint256 amount0, uint256 amount1)
        external
        returns (bool)
    {
        PairDeps storage deps = pairDeps[target];
        require(address(deps.reentrantCallee) != address(0), "pair deps");
        if (amount0 > 0) {
            deps.token0.mint(address(deps.reentrantCallee), amount0);
        }
        if (amount1 > 0) {
            deps.token1.mint(address(deps.reentrantCallee), amount1);
        }
        return true;
    }

    function benchUniswapStageBurn(address target, uint256 liquidity) external returns (bool) {
        (bool ok,) = target.call(abi.encodeWithSignature("transfer(address,uint256)", target, liquidity));
        require(ok, "stage lp");
        return true;
    }

    function benchUniswapSetFeeTo(address newFeeTo) external returns (bool) {
        feeTo = newFeeTo;
        return true;
    }

    function benchUniswapPermitOwner() public returns (address) {
        return vm.addr(UNISWAP_PERMIT_KEY);
    }

    function benchUniswapPermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchUniswapPermitOwner();
        (bool ok, bytes memory rawDomain) = target.call(abi.encodeWithSignature("DOMAIN_SEPARATOR()"));
        require(ok, "uniswap domain");
        bytes32 domainSeparator = abi.decode(rawDomain, (bytes32));
        bytes memory rawNonce;
        (ok, rawNonce) = target.call(abi.encodeWithSignature("nonces(address)", owner));
        require(ok, "uniswap nonce");
        uint256 nonce = abi.decode(rawNonce, (uint256));
        bytes32 digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                domainSeparator,
                keccak256(abi.encode(UNISWAP_PERMIT_TYPE_HASH, owner, spender, value, nonce, deadline))
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(UNISWAP_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }

    function benchCurveInit(address target, uint256 amp, uint256 swapFee, uint256 adminFee)
        external
        returns (bool)
    {
        amp;
        swapFee;
        adminFee;
        require(address(curveDeps[target].coin0) != address(0), "curve deps");
        return true;
    }

    function benchCurveStageReceived(address target, uint256 coinIndex, uint256 amount) external returns (bool) {
        CurveDeps storage deps = curveDeps[target];
        require(address(deps.coin0) != address(0), "curve deps");
        if (coinIndex == 0) {
            require(deps.coin0.transfer(target, amount), "curve transfer0");
        } else if (coinIndex == 1) {
            require(deps.coin1.transfer(target, amount), "curve transfer1");
        } else {
            revert("curve coin");
        }
        return true;
    }

    function benchCurveSetReturnData(address target, bool enabled) external returns (bool) {
        CurveDeps storage deps = curveDeps[target];
        require(address(deps.coin0) != address(0), "curve deps");
        deps.coin0.setReturnData(enabled);
        deps.coin1.setReturnData(enabled);
        return true;
    }

    function benchCurvePermitOwner() public returns (address) {
        return vm.addr(CURVE_PERMIT_KEY);
    }

    function benchCurvePermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchCurvePermitOwner();
        (bool ok, bytes memory rawDomain) = target.call(abi.encodeWithSignature("DOMAIN_SEPARATOR()"));
        require(ok, "curve domain");
        bytes32 domainSeparator = abi.decode(rawDomain, (bytes32));
        bytes memory rawNonce;
        (ok, rawNonce) = target.call(abi.encodeWithSignature("nonces(address)", owner));
        require(ok, "curve nonce");
        uint256 nonce = abi.decode(rawNonce, (uint256));
        bytes32 digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                domainSeparator,
                keccak256(abi.encode(CURVE_PERMIT_TYPE_HASH, owner, spender, value, nonce, deadline))
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(CURVE_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }

    function benchYearnInit(address target, uint256 limit, uint256 unlockTime, uint256 feeBps)
        external
        returns (bool)
    {
        _benchYearnPrepare(target);
        BenchERC20 asset = yearnDeps[target].asset;
        (bool ok,) = target.call(
            abi.encodeWithSignature(
                "initialize(address,string,string,address,uint256)",
                address(asset),
                "Yearn V3 Vault",
                "yvV3",
                address(this),
                unlockTime
            )
        );
        require(ok, "yearn init");
        (ok,) = target.call(abi.encodeWithSignature("set_role(address,uint256)", address(this), uint256(16_383)));
        require(ok, "yearn roles");
        (ok,) = target.call(abi.encodeWithSignature("set_deposit_limit(uint256,bool)", limit, true));
        require(ok, "yearn limit");
        feeBps;
        asset.mint(address(this), 1e30);
        asset.approve(target, type(uint256).max);
        return true;
    }

    function benchYearnPrepare(address target) external returns (bool) {
        _benchYearnPrepare(target);
        return true;
    }

    function _benchYearnPrepare(address target) internal {
        if (address(yearnDeps[target].asset) == address(0)) {
            BenchERC20 asset = new BenchERC20();
            BenchYearnStrategy strategy = new BenchYearnStrategy(asset);
            BenchYearnStrategy strategy2 = new BenchYearnStrategy(asset);
            BenchYearnStrategy strategy3 = new BenchYearnStrategy(asset);
            BenchYearnAccountant accountant = new BenchYearnAccountant(asset);
            BenchYearnReentrantAccountant reentrantAccountant = new BenchYearnReentrantAccountant(asset);
            BenchYearnDepositLimitModule depositLimitModule = new BenchYearnDepositLimitModule();
            BenchYearnWithdrawLimitModule withdrawLimitModule = new BenchYearnWithdrawLimitModule();
            yearnDeps[target] = YearnDeps(
                asset,
                strategy,
                strategy2,
                strategy3,
                accountant,
                reentrantAccountant,
                depositLimitModule,
                withdrawLimitModule
            );
        }
    }

    function benchYearnAsset(address target) public view returns (address) {
        return address(yearnDeps[target].asset);
    }

    function benchYearnStrategy(address target) public view returns (address) {
        return address(yearnDeps[target].strategy);
    }

    function benchYearnStrategy2(address target) public view returns (address) {
        return address(yearnDeps[target].strategy2);
    }

    function benchYearnStrategy3(address target) public view returns (address) {
        return address(yearnDeps[target].strategy3);
    }

    function benchYearnAccountant(address target) public view returns (address) {
        return address(yearnDeps[target].accountant);
    }

    function benchYearnReentrantAccountant(address target) public view returns (address) {
        return address(yearnDeps[target].reentrantAccountant);
    }

    function benchYearnDepositLimitModule(address target) public view returns (address) {
        return address(yearnDeps[target].depositLimitModule);
    }

    function benchYearnWithdrawLimitModule(address target) public view returns (address) {
        return address(yearnDeps[target].withdrawLimitModule);
    }

    function benchYearnPermitOwner() public returns (address) {
        return vm.addr(YEARN_PERMIT_KEY);
    }

    function benchYearnPermitCalldata(address target, address spender, uint256 value, uint256 deadline)
        public
        returns (bytes memory)
    {
        address owner = benchYearnPermitOwner();
        (bool ok, bytes memory rawDomain) = target.call(abi.encodeWithSignature("DOMAIN_SEPARATOR()"));
        require(ok, "yearn domain");
        bytes32 domainSeparator = abi.decode(rawDomain, (bytes32));
        bytes memory rawNonce;
        (ok, rawNonce) = target.call(abi.encodeWithSignature("nonces(address)", owner));
        require(ok, "yearn nonce");
        uint256 nonce = abi.decode(rawNonce, (uint256));
        bytes32 digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                domainSeparator,
                keccak256(abi.encode(YEARN_PERMIT_TYPE_HASH, owner, spender, value, nonce, deadline))
            )
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(YEARN_PERMIT_KEY, digest);
        return abi.encodeWithSignature(
            "permit(address,address,uint256,uint256,uint8,bytes32,bytes32)",
            owner,
            spender,
            value,
            deadline,
            v,
            r,
            s
        );
    }

    function benchYearnSetReport(address target, uint256 gain, uint256 loss) external returns (bool) {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        yearnDeps[target].strategy.setReport(gain, loss);
        return true;
    }

    function benchYearnSetReportFor(address target, address strategy, uint256 gain, uint256 loss)
        external
        returns (bool)
    {
        require(address(yearnDeps[target].strategy) != address(0), "yearn deps");
        require(
            strategy == address(yearnDeps[target].strategy) || strategy == address(yearnDeps[target].strategy2),
            "yearn strategy"
        );
        BenchYearnStrategy(strategy).setReport(gain, loss);
        return true;
    }

    function benchYearnConfigureAccountant(address target, uint256 fees, uint256 refunds) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.accountant) != address(0), "yearn deps");
        deps.accountant.setReport(target, fees, refunds);
        (bool ok,) = target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.accountant)));
        require(ok, "yearn accountant");
        return true;
    }

    function benchYearnConfigureReentrantAccountant(address target) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.reentrantAccountant) != address(0), "yearn deps");
        deps.reentrantAccountant.prepare(target);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_accountant(address)", address(deps.reentrantAccountant)));
        require(ok, "yearn reentrant accountant");
        return true;
    }

    function benchYearnSetDepositLimitModule(address target, uint256 limit) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.depositLimitModule) != address(0), "yearn deps");
        deps.depositLimitModule.setLimit(limit);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_deposit_limit_module(address,bool)", address(deps.depositLimitModule), true));
        require(ok, "yearn deposit module");
        return true;
    }

    function benchYearnSetWithdrawLimitModule(address target, uint256 limit) external returns (bool) {
        YearnDeps storage deps = yearnDeps[target];
        require(address(deps.withdrawLimitModule) != address(0), "yearn deps");
        deps.withdrawLimitModule.setLimit(limit);
        (bool ok,) =
            target.call(abi.encodeWithSignature("set_withdraw_limit_module(address)", address(deps.withdrawLimitModule)));
        require(ok, "yearn withdraw module");
        return true;
    }

    function benchYearnSetDefaultQueueCalldata(address target, bool reverse) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        if (reverse) {
            queue[0] = address(yearnDeps[target].strategy2);
            queue[1] = address(yearnDeps[target].strategy);
        } else {
            queue[0] = address(yearnDeps[target].strategy);
            queue[1] = address(yearnDeps[target].strategy2);
        }
        return abi.encodeWithSignature("set_default_queue(address[])", queue);
    }

    function benchYearnWithdrawQueueCalldata(
        address target,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss,
        bool reverse
    ) public view returns (bytes memory) {
        address[] memory queue = new address[](2);
        if (reverse) {
            queue[0] = address(yearnDeps[target].strategy2);
            queue[1] = address(yearnDeps[target].strategy);
        } else {
            queue[0] = address(yearnDeps[target].strategy);
            queue[1] = address(yearnDeps[target].strategy2);
        }
        return abi.encodeWithSignature(
            "withdraw(uint256,address,address,uint256,address[])", assets, receiver, owner, maxLoss, queue
        );
    }

    function benchYearnWithdrawLongQueueCalldata(
        address target,
        uint256 assets,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "withdraw(uint256,address,address,uint256,address[])",
            assets,
            receiver,
            owner,
            maxLoss,
            benchYearnLongQueue(target)
        );
    }

    function benchYearnRedeemLongQueueCalldata(
        address target,
        uint256 shares,
        address receiver,
        address owner,
        uint256 maxLoss
    ) public view returns (bytes memory) {
        return abi.encodeWithSignature(
            "redeem(uint256,address,address,uint256,address[])",
            shares,
            receiver,
            owner,
            maxLoss,
            benchYearnLongQueue(target)
        );
    }

    function benchYearnMaxWithdrawLongQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxWithdraw(address,uint256,address[])", owner, maxLoss, benchYearnLongQueue(target));
    }

    function benchYearnMaxRedeemLongQueueCalldata(address target, address owner, uint256 maxLoss)
        public
        view
        returns (bytes memory)
    {
        return abi.encodeWithSignature("maxRedeem(address,uint256,address[])", owner, maxLoss, benchYearnLongQueue(target));
    }

    function benchYearnLongQueue(address target) public view returns (address[] memory queue) {
        queue = new address[](11);
        queue[0] = address(yearnDeps[target].strategy);
        queue[1] = address(yearnDeps[target].strategy2);
        queue[2] = address(yearnDeps[target].strategy3);
        for (uint256 i = 3; i < queue.length; i++) {
            queue[i] = address(yearnDeps[target].strategy);
        }
    }

    function _deploy(bytes memory code) internal returns (address target) {
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0), "deploy failed");
    }

    function _deployMinimalProxy(address implementation) internal returns (address target) {
        bytes memory code = abi.encodePacked(
            hex"3d602d80600a3d3981f3363d3d373d3d3d363d73",
            implementation,
            hex"5af43d82803e903d91602b57fd5bf3"
        );
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0) && target.code.length != 0, "proxy deploy failed");
    }

    function _run(address target, bytes memory data, uint256 value, address sender)
        internal
        returns (bool ok, bytes32 retHash, uint256 gasUsed)
    {
        bytes memory ret;
        uint256 startGas = gasleft();
        if (sender == address(this)) {
            (ok, ret) = target.call{value: value}(data);
        } else {
            vm.prank(sender);
            (ok, ret) = target.call{value: value}(data);
        }
        gasUsed = startGas - gasleft();
        retHash = keccak256(ret);
    }

    function _observe(address target, bytes memory data) internal returns (bytes32) {
        (bool ok, bytes memory ret) = target.call(data);
        return keccak256(abi.encode(ok, ret));
    }

    function _calldataGas(bytes memory data) internal pure returns (uint256 gasCost) {
        for (uint256 i = 0; i < data.length; i++) {
            gasCost += data[i] == 0 ? 4 : 16;
        }
    }

    function _bool(bool value) internal pure returns (string memory) {
        return value ? "true" : "false";
    }

    function _writeRow(
        string memory benchmarkId,
        string memory implementationId,
        string memory profileId,
        string memory scenario,
        string memory stateAccessProfile,
        string memory metadataMode,
        uint256 internalCreateGas,
        uint256 harnessCallGas,
        uint256 intrinsicGas,
        uint256 calldataGas,
        uint256 harnessEstimatedTxGas,
        bool expectedSuccess,
        bool callSucceeded,
        bool scenarioStatusOk
    ) internal {
        vm.writeLine(
            GAS_JSONL_PATH,
            string.concat(
                "{\"benchmark_id\":\"", benchmarkId,
                "\",\"implementation_id\":\"", implementationId,
                "\",\"profile_id\":\"", profileId,
                "\",\"scenario\":\"", scenario,
                "\",\"state_access_profile\":\"", stateAccessProfile,
                "\",\"metadata_mode\":\"", metadataMode,
                "\",\"internal_create_gas\":", vm.toString(internalCreateGas),
                ",\"harness_call_gas\":", vm.toString(harnessCallGas),
                ",\"intrinsic_gas\":", vm.toString(intrinsicGas),
                ",\"calldata_gas\":", vm.toString(calldataGas),
                ",\"harness_estimated_tx_gas\":", vm.toString(harnessEstimatedTxGas),
                ",\"expected_success\":", _bool(expectedSuccess),
                ",\"call_succeeded\":", _bool(callSucceeded),
                ",\"scenario_status_ok\":", _bool(scenarioStatusOk),
                "}"
            )
        );
    }

"#
}

fn randomized_helper_functions() -> &'static str {
    r#"
    function _next(uint256 state) internal pure returns (uint256) {
        return uint256(keccak256(abi.encodePacked(state)));
    }

    function _appendTrace(string memory traceLog, string memory trace) internal pure returns (string memory) {
        if (bytes(traceLog).length == 0) {
            return trace;
        }
        return string.concat(traceLog, ";", trace);
    }

    function _actor(uint256 value) internal view returns (address) {
        uint256 index = value % 3;
        if (index == 0) return address(this);
        if (index == 1) return BOB;
        return CAROL;
    }

    function _actorName(address actor) internal view returns (string memory) {
        if (actor == address(this)) return "this";
        if (actor == BOB) return "BOB";
        if (actor == CAROL) return "CAROL";
        return "unknown";
    }

    function _writeFailure(
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog,
        string memory detail
    ) internal {
        vm.createDir("../results/raw/failures", true);
        vm.writeFile(
            string.concat(
                "../results/raw/failures/",
                benchmarkId,
                "-",
                kind,
                "-",
                vm.toString(seed),
                "-",
                vm.toString(step),
                ".json"
            ),
            string.concat(
                "{\"kind\":\"", kind,
                "\",\"benchmark_id\":\"", benchmarkId,
                "\",\"seed\":", vm.toString(seed),
                ",\"step\":", vm.toString(step),
                ",\"trace\":\"", traceLog,
                "\",\"detail\":\"", detail,
                "\"}"
            )
        );
    }

    function _requireCheck(
        bool condition,
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog,
        string memory detail
    ) internal {
        if (!condition) {
            _writeFailure(kind, benchmarkId, seed, step, traceLog, detail);
            require(condition, detail);
        }
    }

    function _runBoth(
        address solTarget,
        address vyperTarget,
        bytes memory data,
        uint256 value,
        address sender,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog
    ) internal {
        (bool solOk, bytes32 solHash,) = _run(solTarget, data, value, sender);
        (bool vyperOk, bytes32 vyperHash,) = _run(vyperTarget, data, value, sender);
        _requireCheck(solOk == vyperOk, "randomized_differential", benchmarkId, seed, step, traceLog, "status mismatch");
        if (solOk) {
            _requireCheck(solHash == vyperHash, "randomized_differential", benchmarkId, seed, step, traceLog, "return mismatch");
        }
    }

    function _compareState(
        bytes32 solState,
        bytes32 vyperState,
        string memory kind,
        string memory benchmarkId,
        uint256 seed,
        uint256 step,
        string memory traceLog
    ) internal {
        _requireCheck(solState == vyperState, kind, benchmarkId, seed, step, traceLog, "state mismatch");
    }

    function _readUint(address target, bytes memory data) internal returns (uint256 value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "uint read failed");
        value = abi.decode(ret, (uint256));
    }

    function _readBool(address target, bytes memory data) internal returns (bool value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "bool read failed");
        value = abi.decode(ret, (bool));
    }

    function _readAddress(address target, bytes memory data) internal returns (address value) {
        (bool ok, bytes memory ret) = target.call(data);
        require(ok, "address read failed");
        value = abi.decode(ret, (address));
    }

    function _readReserves(address target) internal returns (uint256 reserve0, uint256 reserve1) {
        (bool ok, bytes memory ret) = target.call(abi.encodeWithSignature("getReserves()"));
        require(ok, "reserve read failed");
        (reserve0, reserve1) = abi.decode(ret, (uint256, uint256));
    }

    function _counterState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(_readUint(target, abi.encodeWithSignature("value()"))));
    }

    function _erc20State(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("totalSupply()")),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL)),
            _readUint(target, abi.encodeWithSignature("allowance(address,address)", address(this), BOB)),
            _readUint(target, abi.encodeWithSignature("allowance(address,address)", BOB, CAROL))
        ));
    }

    function _vaultState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)),
            _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL)),
            _readUint(target, abi.encodeWithSignature("totalShares()")),
            _readUint(target, abi.encodeWithSignature("totalAssets()"))
        ));
    }

    function _ownableState(address target) internal returns (bytes32) {
        return keccak256(abi.encode(
            _readAddress(target, abi.encodeWithSignature("owner()")),
            _readBool(target, abi.encodeWithSignature("paused()")),
            _readUint(target, abi.encodeWithSignature("counter()"))
        ));
    }

    function _ammState(address target) internal returns (bytes32) {
        (uint256 reserve0FromPair, uint256 reserve1FromPair) = _readReserves(target);
        return keccak256(abi.encode(
            _readUint(target, abi.encodeWithSignature("reserve0()")),
            _readUint(target, abi.encodeWithSignature("reserve1()")),
            _readUint(target, abi.encodeWithSignature("totalLiquidity()")),
            reserve0FromPair,
            reserve1FromPair
        ));
    }

    struct OwnableModel {
        address owner;
        bool paused;
        uint256 counter;
        string traceLog;
    }

    function _requireOwnableProperty(bool condition, uint256 seed, uint256 step, string memory traceLog, string memory detail) internal {
        _requireCheck(condition, "property", "ownable_pausable", seed, step, traceLog, detail);
    }

    function _randomDiff_counter(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            string memory trace;
            if (op == 0) {
                trace = "increment";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("increment()"), 0, address(this), "counter", seed, i, traceLog);
            } else if (op == 1) {
                uint256 amount = rng % 17;
                trace = string.concat("add:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("add(uint256)", amount), 0, address(this), "counter", seed, i, traceLog);
            } else if (op == 2) {
                trace = "reset";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("reset()"), 0, address(this), "counter", seed, i, traceLog);
            } else {
                trace = "value";
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("value()"), 0, address(this), "counter", seed, i, traceLog);
            }
            _compareState(_counterState(solTarget), _counterState(vyperTarget), "randomized_differential", "counter", seed, i, traceLog);
        }
    }

    function _randomDiff_erc20_minimal(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 5;
            uint256 amount = ((rng % 20) + 1) * 1 ether;
            string memory trace;
            if (op == 0) {
                trace = string.concat("transfer_this_to_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transfer(address,uint256)", BOB, amount), 0, address(this), "erc20_minimal", seed, i, traceLog);
            } else if (op == 1) {
                trace = string.concat("transfer_BOB_to_CAROL:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transfer(address,uint256)", CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            } else if (op == 2) {
                trace = string.concat("approve_this_to_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("approve(address,uint256)", BOB, amount), 0, address(this), "erc20_minimal", seed, i, traceLog);
            } else if (op == 3) {
                trace = string.concat("transferFrom_this_to_CAROL_by_BOB:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transferFrom(address,address,uint256)", address(this), CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            } else {
                trace = string.concat("approve_BOB_to_CAROL:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("approve(address,uint256)", CAROL, amount), 0, BOB, "erc20_minimal", seed, i, traceLog);
            }
            _compareState(_erc20State(solTarget), _erc20State(vyperTarget), "randomized_differential", "erc20_minimal", seed, i, traceLog);
        }
    }

    function _randomDiff_vault_deposit_withdraw(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            address actor = _actor(rng);
            uint256 amount = ((rng % 9) + 1) * (1 ether / 10);
            string memory trace;
            if (rng % 2 == 0) {
                trace = string.concat("deposit_", _actorName(actor), ":", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("deposit()"), amount, actor, "vault_deposit_withdraw", seed, i, traceLog);
            } else {
                trace = string.concat("withdraw_", _actorName(actor), ":", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("withdraw(uint256)", amount), 0, actor, "vault_deposit_withdraw", seed, i, traceLog);
            }
            _compareState(_vaultState(solTarget), _vaultState(vyperTarget), "randomized_differential", "vault_deposit_withdraw", seed, i, traceLog);
        }
    }

    function _randomDiff_ownable_pausable(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            address actor = _actor(rng);
            string memory trace;
            if (op == 0) {
                trace = string.concat("pause_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("pause()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else if (op == 1) {
                trace = string.concat("unpause_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("unpause()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else if (op == 2) {
                trace = string.concat("guardedIncrement_", _actorName(actor));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("guardedIncrement()"), 0, actor, "ownable_pausable", seed, i, traceLog);
            } else {
                address newOwner = _actor(rng / 7);
                trace = string.concat("transferOwnership_", _actorName(actor), "_to_", _actorName(newOwner));
                traceLog = _appendTrace(traceLog, trace);
                _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("transferOwnership(address)", newOwner), 0, actor, "ownable_pausable", seed, i, traceLog);
            }
            _compareState(_ownableState(solTarget), _ownableState(vyperTarget), "randomized_differential", "ownable_pausable", seed, i, traceLog);
        }
    }

    function _randomDiff_amm_pair_subset(address solTarget, address vyperTarget, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            if (op == 0) {
                traceLog = _randomDiffAmmMint(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else if (op == 1) {
                traceLog = _randomDiffAmmBurn(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else if (op == 2) {
                traceLog = _randomDiffAmmSwap(solTarget, vyperTarget, seed, i, rng, traceLog);
            } else {
                traceLog = _randomDiffAmmSync(solTarget, vyperTarget, seed, i, rng, traceLog);
            }
            _compareState(_ammState(solTarget), _ammState(vyperTarget), "randomized_differential", "amm_pair_subset", seed, i, traceLog);
        }
    }

    function _randomDiffAmmMint(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 amount0 = (rng % 200) + 1;
        uint256 amount1 = ((rng / 17) % 200) + 1;
        traceLog = _appendTrace(traceLog, string.concat("mint:", vm.toString(amount0), ":", vm.toString(amount1)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("mint(uint256,uint256)", amount0, amount1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmBurn(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 liquidity = _readUint(solTarget, abi.encodeWithSignature("totalLiquidity()"));
        uint256 burnAmount = liquidity == 0 ? 1 : ((rng % (liquidity + 5)) + 1);
        traceLog = _appendTrace(traceLog, string.concat("burn:", vm.toString(burnAmount)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("burn(uint256)", burnAmount), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmSwap(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        (uint256 reserve0Before,) = _readReserves(solTarget);
        uint256 amount0Out = reserve0Before == 0 ? 0 : rng % (reserve0Before + 1);
        uint256 amount0In = (rng % 13) + 1;
        traceLog = _appendTrace(traceLog, string.concat("swap:", vm.toString(amount0Out), ":0:", vm.toString(amount0In), ":1"));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("swap(uint256,uint256,uint256,uint256)", amount0Out, 0, amount0In, 1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _randomDiffAmmSync(
        address solTarget,
        address vyperTarget,
        uint256 seed,
        uint256 step,
        uint256 rng,
        string memory traceLog
    ) internal returns (string memory) {
        uint256 balance0 = (rng % 500) + 1;
        uint256 balance1 = ((rng / 19) % 500) + 1;
        traceLog = _appendTrace(traceLog, string.concat("sync:", vm.toString(balance0), ":", vm.toString(balance1)));
        _runBoth(solTarget, vyperTarget, abi.encodeWithSignature("sync(uint256,uint256)", balance0, balance1), 0, address(this), "amm_pair_subset", seed, step, traceLog);
        return traceLog;
    }

    function _property_counter(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        uint256 model = 3;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 3;
            string memory trace;
            bool ok;
            if (op == 0) {
                trace = "increment";
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("increment()"), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter increment failed");
                model += 1;
            } else if (op == 1) {
                uint256 amount = rng % 17;
                trace = string.concat("add:", vm.toString(amount));
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("add(uint256)", amount), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter add failed");
                model += amount;
            } else {
                trace = "reset";
                traceLog = _appendTrace(traceLog, trace);
                (ok,,) = _run(target, abi.encodeWithSignature("reset()"), 0, address(this));
                _requireCheck(ok, "property", "counter", seed, i, traceLog, "counter reset failed");
                model = 0;
            }
            _requireCheck(_readUint(target, abi.encodeWithSignature("value()")) == model, "property", "counter", seed, i, traceLog, "counter model mismatch");
        }
    }

    function _property_erc20_minimal(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        uint256 supply = _readUint(target, abi.encodeWithSignature("totalSupply()"));
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 5;
            uint256 amount = ((rng % 20) + 1) * 1 ether;
            string memory trace;
            if (op == 0) {
                trace = string.concat("transfer_this_to_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transfer(address,uint256)", BOB, amount), 0, address(this));
            } else if (op == 1) {
                trace = string.concat("transfer_BOB_to_CAROL:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transfer(address,uint256)", CAROL, amount), 0, BOB);
            } else if (op == 2) {
                trace = string.concat("approve_this_to_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("approve(address,uint256)", BOB, amount), 0, address(this));
            } else if (op == 3) {
                trace = string.concat("transferFrom_this_to_CAROL_by_BOB:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("transferFrom(address,address,uint256)", address(this), CAROL, amount), 0, BOB);
            } else {
                trace = string.concat("approve_BOB_to_CAROL:", vm.toString(amount));
                _run(target, abi.encodeWithSignature("approve(address,uint256)", CAROL, amount), 0, BOB);
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 thisBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this)));
            uint256 bobBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB));
            uint256 carolBalance = _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL));
            _requireCheck(_readUint(target, abi.encodeWithSignature("totalSupply()")) == supply, "property", "erc20_minimal", seed, i, traceLog, "total supply changed");
            _requireCheck(thisBalance <= supply && bobBalance <= supply && carolBalance <= supply, "property", "erc20_minimal", seed, i, traceLog, "sampled balance exceeds supply");
            _requireCheck(thisBalance + bobBalance + carolBalance == supply, "property", "erc20_minimal", seed, i, traceLog, "sampled balances do not sum to supply");
        }
    }

    function _property_vault_deposit_withdraw(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            address actor = _actor(rng);
            uint256 amount = ((rng % 9) + 1) * (1 ether / 10);
            string memory trace;
            if (rng % 2 == 0) {
                trace = string.concat("deposit_", _actorName(actor), ":", vm.toString(amount));
                _run(target, abi.encodeWithSignature("deposit()"), amount, actor);
            } else {
                trace = string.concat("withdraw_", _actorName(actor), ":", vm.toString(amount));
                _run(target, abi.encodeWithSignature("withdraw(uint256)", amount), 0, actor);
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 sampledShares =
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", address(this))) +
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", BOB)) +
                _readUint(target, abi.encodeWithSignature("balanceOf(address)", CAROL));
            uint256 totalShares = _readUint(target, abi.encodeWithSignature("totalShares()"));
            _requireCheck(sampledShares == totalShares, "property", "vault_deposit_withdraw", seed, i, traceLog, "sampled shares do not match total shares");
            _requireCheck(_readUint(target, abi.encodeWithSignature("totalAssets()")) == totalShares, "property", "vault_deposit_withdraw", seed, i, traceLog, "assets do not match shares");
        }
    }

    function _property_ownable_pausable(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        OwnableModel memory model = OwnableModel(address(this), false, 0, "");
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            model = _propertyOwnableStep(target, seed, i, rng, model);
        }
    }

    function _propertyOwnableStep(
        address target,
        uint256 seed,
        uint256 step,
        uint256 rng,
        OwnableModel memory model
    ) internal returns (OwnableModel memory) {
        uint256 op = rng % 4;
        address actor = _actor(rng);
        bool ok;
        if (op == 0) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("pause_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("pause()"), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "pause authorization mismatch");
            if (ok) model.paused = true;
        } else if (op == 1) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("unpause_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("unpause()"), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "unpause authorization mismatch");
            if (ok) model.paused = false;
        } else if (op == 2) {
            model.traceLog = _appendTrace(model.traceLog, string.concat("guardedIncrement_", _actorName(actor)));
            (ok,,) = _run(target, abi.encodeWithSignature("guardedIncrement()"), 0, actor);
            _requireOwnableProperty(ok == !model.paused, seed, step, model.traceLog, "paused guard mismatch");
            if (ok) model.counter += 1;
        } else {
            address newOwner = _actor(rng / 7);
            model.traceLog = _appendTrace(model.traceLog, string.concat("transferOwnership_", _actorName(actor), "_to_", _actorName(newOwner)));
            (ok,,) = _run(target, abi.encodeWithSignature("transferOwnership(address)", newOwner), 0, actor);
            _requireOwnableProperty(ok == (actor == model.owner), seed, step, model.traceLog, "ownership authorization mismatch");
            if (ok) model.owner = newOwner;
        }
        _requireOwnableProperty(_readAddress(target, abi.encodeWithSignature("owner()")) == model.owner, seed, step, model.traceLog, "owner model mismatch");
        _requireOwnableProperty(_readBool(target, abi.encodeWithSignature("paused()")) == model.paused, seed, step, model.traceLog, "paused model mismatch");
        _requireOwnableProperty(_readUint(target, abi.encodeWithSignature("counter()")) == model.counter, seed, step, model.traceLog, "counter model mismatch");
        return model;
    }

    function _property_amm_pair_subset(address target, uint256 seed, uint256 iterations) internal {
        uint256 rng = seed;
        string memory traceLog = "";
        for (uint256 i = 0; i < iterations; i++) {
            rng = _next(rng);
            uint256 op = rng % 4;
            string memory trace;
            if (op == 0) {
                uint256 amount0 = (rng % 200) + 1;
                uint256 amount1 = ((rng / 17) % 200) + 1;
                trace = string.concat("mint:", vm.toString(amount0), ":", vm.toString(amount1));
                _run(target, abi.encodeWithSignature("mint(uint256,uint256)", amount0, amount1), 0, address(this));
            } else if (op == 1) {
                uint256 liquidity = _readUint(target, abi.encodeWithSignature("totalLiquidity()"));
                uint256 burnAmount = liquidity == 0 ? 1 : ((rng % liquidity) + 1);
                trace = string.concat("burn:", vm.toString(burnAmount));
                _run(target, abi.encodeWithSignature("burn(uint256)", burnAmount), 0, address(this));
            } else if (op == 2) {
                (uint256 reserve0Before, uint256 reserve1Before) = _readReserves(target);
                uint256 amount0Out = reserve0Before == 0 ? 0 : rng % (reserve0Before + 1);
                uint256 amount1Out = reserve1Before == 0 ? 0 : (rng / 11) % (reserve1Before + 1);
                uint256 amount0In = (rng % 13) + 1;
                uint256 amount1In = ((rng / 13) % 17) + 1;
                trace = string.concat("swap:", vm.toString(amount0Out), ":", vm.toString(amount1Out), ":", vm.toString(amount0In), ":", vm.toString(amount1In));
                _run(target, abi.encodeWithSignature("swap(uint256,uint256,uint256,uint256)", amount0Out, amount1Out, amount0In, amount1In), 0, address(this));
            } else {
                uint256 balance0 = (rng % 500) + 1;
                uint256 balance1 = ((rng / 19) % 500) + 1;
                trace = string.concat("sync:", vm.toString(balance0), ":", vm.toString(balance1));
                _run(target, abi.encodeWithSignature("sync(uint256,uint256)", balance0, balance1), 0, address(this));
            }
            traceLog = _appendTrace(traceLog, trace);
            uint256 reserve0 = _readUint(target, abi.encodeWithSignature("reserve0()"));
            uint256 reserve1 = _readUint(target, abi.encodeWithSignature("reserve1()"));
            uint256 totalLiquidity = _readUint(target, abi.encodeWithSignature("totalLiquidity()"));
            (uint256 reserve0FromPair, uint256 reserve1FromPair) = _readReserves(target);
            _requireCheck(reserve0 == reserve0FromPair && reserve1 == reserve1FromPair, "property", "amm_pair_subset", seed, i, traceLog, "reserve getter mismatch");
            _requireCheck(totalLiquidity == 0 || reserve0 + reserve1 > 0, "property", "amm_pair_subset", seed, i, traceLog, "liquidity without reserves");
        }
    }

"#
}

fn write_deploy_function(out: &mut String, index: usize, artifact: &CompiledArtifact) {
    out.push_str("    function deployArtifact");
    out.push_str(&index.to_string());
    out.push_str("() internal returns (address target, uint256 deployGas) {\n");
    out.push_str("        bytes memory code = hex\"");
    out.push_str(artifact.creation_bytecode.trim_start_matches("0x"));
    out.push_str("\";\n");
    if artifact.benchmark_id == "curve_stableswap_2coin" {
        out.push_str("        BenchERC20OptionalReturn coin0 = new BenchERC20OptionalReturn();\n");
        out.push_str("        BenchERC20OptionalReturn coin1 = new BenchERC20OptionalReturn();\n");
        out.push_str("        address[] memory coins = new address[](2);\n");
        out.push_str("        coins[0] = address(coin0);\n");
        out.push_str("        coins[1] = address(coin1);\n");
        out.push_str("        uint256[] memory rates = new uint256[](2);\n");
        out.push_str("        rates[0] = 1e18;\n");
        out.push_str("        rates[1] = 1e18;\n");
        out.push_str("        uint8[] memory assetTypes = new uint8[](2);\n");
        out.push_str("        bytes4[] memory methodIds = new bytes4[](2);\n");
        out.push_str("        address[] memory oracles = new address[](2);\n");
        out.push_str("        code = abi.encodePacked(code, abi.encode(\"Curve.fi Stablecoin\", \"crv2\", uint256(200), uint256(4_000_000), uint256(20_000_000_000), uint256(866), coins, rates, assetTypes, methodIds, oracles));\n");
    }
    if let Some(args) = constructor_args(&artifact.benchmark_id) {
        out.push_str("        code = abi.encodePacked(code, ");
        out.push_str(args);
        out.push_str(");\n");
    }
    out.push_str("        uint256 startGas = gasleft();\n");
    if artifact.benchmark_id == "yearn_vault_v3" {
        out.push_str("        address implementation = _deploy(code);\n");
        out.push_str("        target = _deployMinimalProxy(implementation);\n");
    } else {
        out.push_str("        target = _deploy(code);\n");
    }
    out.push_str("        deployGas = startGas - gasleft();\n");
    if artifact.benchmark_id == "curve_stableswap_2coin" {
        out.push_str("        curveDeps[target] = CurveDeps(coin0, coin1);\n");
        out.push_str("        coin0.mint(address(this), 1e30);\n");
        out.push_str("        coin1.mint(address(this), 1e30);\n");
        out.push_str("        coin0.approve(target, type(uint256).max);\n");
        out.push_str("        coin1.approve(target, type(uint256).max);\n");
    }
    out.push_str("    }\n\n");
}

fn write_gas_test(
    out: &mut String,
    index: usize,
    artifact: &CompiledArtifact,
    scenario: &Scenario,
) {
    out.push_str("    function testGas_");
    out.push_str(&index.to_string());
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("() public {\n");
    out.push_str("        (address target, uint256 deployGas) = deployArtifact");
    out.push_str(&index.to_string());
    out.push_str("();\n");
    write_setup(out, "target", &scenario.setup, "setup");
    write_setup(out, "target", &scenario.warmup, "warmup");
    out.push_str("        uint256 calldataGas = _calldataGas(");
    out.push_str(&call_data(&scenario.measured, "target"));
    out.push_str(");\n");
    out.push_str("        (bool ok,, uint256 executionGas) = _run(");
    out.push_str(call_destination(&scenario.measured, "target"));
    out.push_str(", ");
    write_call_args(out, &scenario.measured, "target");
    out.push_str(");\n");
    out.push_str("        bool scenarioStatusOk = ok == ");
    out.push_str(if scenario.expect_success {
        "true"
    } else {
        "false"
    });
    out.push_str(";\n");
    out.push_str("        require(scenarioStatusOk, \"unexpected scenario status\");\n");
    out.push_str("        _writeRow(\"");
    out.push_str(&artifact.benchmark_id);
    out.push_str("\", \"");
    out.push_str(&artifact.implementation_id);
    out.push_str("\", \"");
    out.push_str(&artifact.profile_id);
    out.push_str("\", \"");
    out.push_str(&scenario.name);
    out.push_str("\", \"");
    out.push_str(scenario.state_access_profile.as_str());
    out.push_str("\", \"");
    out.push_str(artifact.metadata_mode.as_str());
    out.push_str(
        "\", deployGas, executionGas, 21000, calldataGas, executionGas + 21000 + calldataGas, ",
    );
    out.push_str(if scenario.expect_success {
        "true"
    } else {
        "false"
    });
    out.push_str(", ok, scenarioStatusOk);\n");
    out.push_str("    }\n\n");
}

fn write_diff_test(
    out: &mut String,
    benchmark_id: &str,
    solidity_idx: usize,
    vyper_idx: usize,
    solidity: &CompiledArtifact,
    vyper: &CompiledArtifact,
    scenario: &Scenario,
) {
    out.push_str("    function testDiff_");
    out.push_str(&sanitize(benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("() public {\n");
    out.push_str("        (address solTarget,) = deployArtifact");
    out.push_str(&solidity_idx.to_string());
    out.push_str("();\n");
    out.push_str("        (address vyperTarget,) = deployArtifact");
    out.push_str(&vyper_idx.to_string());
    out.push_str("();\n");
    write_setup(out, "solTarget", &scenario.setup, "setup");
    write_setup(out, "vyperTarget", &scenario.setup, "setup");
    write_setup(out, "solTarget", &scenario.warmup, "warmup");
    write_setup(out, "vyperTarget", &scenario.warmup, "warmup");
    out.push_str("        (bool solOk, bytes32 solHash,) = _run(");
    out.push_str(call_destination(&scenario.measured, "solTarget"));
    out.push_str(", ");
    write_call_args(out, &scenario.measured, "solTarget");
    out.push_str(");\n");
    out.push_str("        (bool vyperOk, bytes32 vyperHash,) = _run(");
    out.push_str(call_destination(&scenario.measured, "vyperTarget"));
    out.push_str(", ");
    write_call_args(out, &scenario.measured, "vyperTarget");
    out.push_str(");\n");
    out.push_str("        require(solOk == vyperOk, \"differential status mismatch\");\n");
    out.push_str("        require(solOk == ");
    out.push_str(if scenario.expect_success {
        "true"
    } else {
        "false"
    });
    out.push_str(", \"differential unexpected status\");\n");
    out.push_str(
        "        if (solOk) require(solHash == vyperHash, \"differential return mismatch\");\n",
    );
    out.push_str("        require(_observeAll_");
    out.push_str(&sanitize(&solidity.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("(solTarget) == _observeAll_");
    out.push_str(&sanitize(&vyper.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("(vyperTarget), \"differential observer mismatch\");\n");
    out.push_str("    }\n\n");
    write_observer_function(out, &solidity.benchmark_id, scenario);
}

fn write_randomized_diff_test(
    out: &mut String,
    benchmark_id: &str,
    solidity_idx: usize,
    vyper_idx: usize,
    randomized: &RandomizedSpec,
) -> Result<()> {
    let helper = randomized_helper_name(benchmark_id)?;
    out.push_str("    function testRandomDiff_");
    out.push_str(&sanitize(benchmark_id));
    out.push_str("() public {\n");
    out.push_str("        (address solTarget,) = deployArtifact");
    out.push_str(&solidity_idx.to_string());
    out.push_str("();\n");
    out.push_str("        (address vyperTarget,) = deployArtifact");
    out.push_str(&vyper_idx.to_string());
    out.push_str("();\n");
    out.push_str("        ");
    out.push_str(helper);
    out.push_str("(solTarget, vyperTarget, ");
    out.push_str(&randomized.seed.to_string());
    out.push_str(", ");
    out.push_str(&randomized.iterations.to_string());
    out.push_str(");\n");
    out.push_str("    }\n\n");
    Ok(())
}

fn write_property_test(
    out: &mut String,
    benchmark_id: &str,
    solidity_idx: usize,
    vyper_idx: usize,
    randomized: Option<&RandomizedSpec>,
    property: &PropertySpec,
) -> Result<()> {
    let helper = property_helper_name(&property.name)?;
    let seed = property
        .seed
        .or_else(|| randomized.map(|spec| spec.seed))
        .unwrap_or(0);
    let iterations = randomized.map(|spec| spec.iterations).unwrap_or(16);
    out.push_str("    function testProperty_");
    out.push_str(&sanitize(benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&property.name));
    out.push_str("() public {\n");
    out.push_str("        (address solTarget,) = deployArtifact");
    out.push_str(&solidity_idx.to_string());
    out.push_str("();\n");
    out.push_str("        ");
    out.push_str(helper);
    out.push_str("(solTarget, ");
    out.push_str(&seed.to_string());
    out.push_str(", ");
    out.push_str(&iterations.to_string());
    out.push_str(");\n");
    out.push_str("        (address vyperTarget,) = deployArtifact");
    out.push_str(&vyper_idx.to_string());
    out.push_str("();\n");
    out.push_str("        ");
    out.push_str(helper);
    out.push_str("(vyperTarget, ");
    out.push_str(&seed.to_string());
    out.push_str(", ");
    out.push_str(&iterations.to_string());
    out.push_str(");\n");
    out.push_str("    }\n\n");
    Ok(())
}

fn randomized_helper_name(benchmark_id: &str) -> Result<&'static str> {
    match benchmark_id {
        "counter" => Ok("_randomDiff_counter"),
        "erc20_minimal" => Ok("_randomDiff_erc20_minimal"),
        "vault_deposit_withdraw" => Ok("_randomDiff_vault_deposit_withdraw"),
        "ownable_pausable" => Ok("_randomDiff_ownable_pausable"),
        "amm_pair_subset" => Ok("_randomDiff_amm_pair_subset"),
        _ => bail!("unsupported randomized benchmark {benchmark_id}"),
    }
}

fn property_helper_name(property_name: &str) -> Result<&'static str> {
    match property_name {
        "counter_model_matches" => Ok("_property_counter"),
        "erc20_supply_conservation" => Ok("_property_erc20_minimal"),
        "vault_share_accounting" => Ok("_property_vault_deposit_withdraw"),
        "ownable_authorization" => Ok("_property_ownable_pausable"),
        "amm_reserve_liquidity_coherence" => Ok("_property_amm_pair_subset"),
        _ => bail!("unsupported property {property_name}"),
    }
}

fn write_observer_function(out: &mut String, benchmark_id: &str, scenario: &Scenario) {
    let name = format!(
        "_observeAll_{}_{}",
        sanitize(benchmark_id),
        sanitize(&scenario.name)
    );
    if out.contains(&format!("function {name}(")) {
        return;
    }
    out.push_str("    function ");
    out.push_str(&name);
    out.push_str("(address target) internal returns (bytes32 observed) {\n");
    out.push_str("        observed = bytes32(0);\n");
    for observer in &scenario.observers {
        out.push_str("        observed = keccak256(abi.encode(observed, _observe(target, ");
        out.push_str(&observer.data);
        out.push_str(")));\n");
    }
    out.push_str("    }\n\n");
}

fn write_setup(out: &mut String, target: &str, setup: &[CallSpec], label: &str) {
    for call in setup {
        out.push_str("        { (bool setupOk,,) = _run(");
        out.push_str(call_destination(call, target));
        out.push_str(", ");
        write_call_args(out, call, target);
        out.push_str(");\n");
        out.push_str("        require(setupOk, \"");
        out.push_str(label);
        out.push_str(" call failed\"); }\n");
    }
}

fn write_call_args(out: &mut String, call: &CallSpec, target: &str) {
    out.push_str(&call_data(call, target));
    out.push_str(", ");
    out.push_str(&call.value);
    out.push_str(", ");
    out.push_str(call.sender.as_deref().unwrap_or("address(this)"));
}

fn call_destination<'a>(call: &CallSpec, target: &'a str) -> &'a str {
    match call.destination {
        CallDestination::Target => target,
        CallDestination::Harness => "address(this)",
    }
}

fn call_data(call: &CallSpec, target: &str) -> String {
    call.data.replace("{target}", target)
}

fn baseline_pairs(artifacts: &[CompiledArtifact]) -> BTreeMap<String, (usize, usize)> {
    let mut pairs = BTreeMap::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        if artifact.profile_id != SOL_BASELINE {
            continue;
        }
        if let Some((vyper_idx, _)) = artifacts.iter().enumerate().find(|(_, other)| {
            other.benchmark_id == artifact.benchmark_id && other.profile_id == VYPER_BASELINE
        }) {
            pairs.insert(artifact.benchmark_id.clone(), (index, vyper_idx));
        }
    }
    pairs
}

fn constructor_args(benchmark_id: &str) -> Option<&'static str> {
    match benchmark_id {
        "counter" => Some("abi.encode(uint256(3))"),
        "erc20_minimal" => Some("abi.encode(uint256(1000 ether))"),
        _ => None,
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

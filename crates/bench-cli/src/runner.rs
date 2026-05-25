use crate::{
    baselines::baseline_pairs,
    cache::{self, CacheLookup},
    models::{
        CacheInfo, CallDestination, CallSpec, CompileSet, CompiledArtifact, DeploymentVariant,
        GasRecord, PropertySpec, RandomizedSpec, Scenario,
    },
    scenarios::ScenarioCatalog,
    util::{Progress, ensure_dir, require_success, run_measured, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

const FAILURE_DIR: &str = "../results/raw/failures";
const GAS_CACHE_SCHEMA: &str = "gas-v2";
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
    let mut cached = Vec::new();
    let mut missing_keys = BTreeSet::new();
    if use_cache {
        let mut progress = Progress::new("gas cache", expected_cache.len());
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
                    missing_keys.insert(input.record_key.clone());
                    progress.update(index + 1, "miss");
                }
            }
        }
        if missing_keys.is_empty() {
            progress.finish(format!("loaded {} rows from cache", cached.len()));
            write_raw_gas_records(root, &cached)?;
            return Ok(cached);
        }
        progress.finish(format!(
            "loaded {} cached rows; running Foundry for {} missing rows",
            cached.len(),
            missing_keys.len()
        ));
    } else {
        eprintln!(
            "gas: cache disabled; running Foundry for {} expected rows",
            expected_cache.len()
        );
    }
    clear_generated_shards(root)?;
    let selected_gas_keys = if use_cache { Some(&missing_keys) } else { None };
    let artifacts = if use_cache {
        artifacts_for_gas_keys(&compiled.artifacts, &missing_keys)
    } else {
        compiled.artifacts.clone()
    };
    let include_behavior_checks = !use_cache || cached.is_empty();
    let shards = gas_shards(&artifacts);
    eprintln!(
        "foundry: generating {} gas test shards for {} artifacts and {} expected rows",
        shards.len(),
        artifacts.len(),
        selected_gas_keys.map_or(expected_cache.len(), BTreeSet::len)
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
            generate_test(
                &contract_name,
                shard_artifacts,
                scenarios,
                &gas_jsonl,
                selected_gas_keys,
                include_behavior_checks,
            )?,
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
    if use_cache {
        cached.extend(records);
        cached.sort_by(|left, right| {
            gas_record_key(
                &left.benchmark_id,
                &left.implementation_id,
                &left.profile_id,
                &left.scenario,
                left.state_access_profile.as_str(),
            )
            .cmp(&gas_record_key(
                &right.benchmark_id,
                &right.implementation_id,
                &right.profile_id,
                &right.scenario,
                right.state_access_profile.as_str(),
            ))
        });
        write_raw_gas_records(root, &cached)?;
        eprintln!("foundry: recorded {} gas rows", cached.len());
        Ok(cached)
    } else {
        write_raw_gas_records(root, &records)?;
        eprintln!("foundry: recorded {} gas rows", records.len());
        Ok(records)
    }
}

#[derive(Debug, Clone)]
struct GasCacheInput {
    key: String,
    logical_id: String,
    record_key: String,
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
            let record_key = gas_record_key(
                &artifact.benchmark_id,
                &artifact.implementation_id,
                &artifact.profile_id,
                &scenario.name,
                scenario.state_access_profile.as_str(),
            );
            inputs.insert(
                record_key.clone(),
                GasCacheInput {
                    key,
                    logical_id,
                    record_key,
                    fingerprint,
                    lookup_info,
                },
            );
        }
    }
    Ok(inputs)
}

fn artifacts_for_gas_keys(
    artifacts: &[CompiledArtifact],
    selected_keys: &BTreeSet<String>,
) -> Vec<CompiledArtifact> {
    artifacts
        .iter()
        .filter(|artifact| {
            let prefix = format!(
                "{}\0{}\0{}\0",
                artifact.benchmark_id, artifact.implementation_id, artifact.profile_id
            );
            selected_keys.iter().any(|key| key.starts_with(&prefix))
        })
        .cloned()
        .collect()
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
    selected_gas_keys: Option<&BTreeSet<String>>,
    include_behavior_checks: bool,
) -> Result<String> {
    if artifacts.is_empty() {
        bail!("no compiled artifacts for Foundry runner");
    }
    let mut out = String::new();
    out.push_str("// SPDX-License-Identifier: MIT\n");
    out.push_str("pragma solidity ^0.8.20;\n\n");
    out.push_str(support_contracts());
    out.push_str("interface Vm {\n");
    out.push_str("    struct Log { bytes32[] topics; bytes data; address emitter; }\n");
    out.push_str("    function createDir(string calldata path, bool recursive) external;\n");
    out.push_str("    function writeFile(string calldata path, string calldata data) external;\n");
    out.push_str("    function writeLine(string calldata path, string calldata data) external;\n");
    out.push_str("    function toString(uint256 value) external pure returns (string memory);\n");
    out.push_str("    function prank(address sender) external;\n");
    out.push_str("    function deal(address account, uint256 newBalance) external;\n");
    out.push_str("    function warp(uint256 newTimestamp) external;\n");
    out.push_str("    function chainId(uint256 newChainId) external;\n");
    out.push_str("    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);\n");
    out.push_str("    function addr(uint256 privateKey) external returns (address);\n");
    out.push_str("    function recordLogs() external;\n");
    out.push_str("    function getRecordedLogs() external returns (Log[] memory entries);\n");
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
    out.push_str("    struct CurveDeps { BenchERC20OptionalReturn coin0; BenchERC20OptionalReturn coin1; BenchERC20OptionalReturn coin2; BenchERC20OptionalReturn coin3; BenchERC20OptionalReturn coin4; BenchERC20OptionalReturn coin5; BenchERC20OptionalReturn coin6; BenchERC20OptionalReturn coin7; }\n");
    out.push_str("    struct YearnDeps { BenchERC20 asset; BenchYearnStrategy strategy; BenchYearnStrategy strategy2; BenchYearnStrategy strategy3; BenchYearnAccountant accountant; BenchYearnMutatingAccountant mutatingAccountant; BenchYearnReentrantAccountant reentrantAccountant; BenchYearnDepositLimitModule depositLimitModule; BenchYearnWithdrawLimitModule withdrawLimitModule; }\n");
    out.push_str("    mapping(address => PairDeps) internal pairDeps;\n");
    out.push_str("    mapping(address => NoReturnPairDeps) internal noReturnPairDeps;\n");
    out.push_str("    mapping(address => CurveDeps) internal curveDeps;\n");
    out.push_str("    mapping(address => YearnDeps) internal yearnDeps;\n");
    out.push_str("    BenchERC1271Wallet internal curve1271Owner;\n");
    out.push_str("    BenchUniswapCreate2Factory internal uniswapCreate2Factory;\n");
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
    out.push_str(&helper_functions(artifacts));
    out.push_str(randomized_helper_functions());

    for (index, artifact) in artifacts.iter().enumerate() {
        write_deploy_function(&mut out, index, artifact);
    }

    for (index, artifact) in artifacts.iter().enumerate() {
        for scenario in &scenarios.get(&artifact.benchmark_id)?.scenarios {
            let record_key = gas_record_key(
                &artifact.benchmark_id,
                &artifact.implementation_id,
                &artifact.profile_id,
                &scenario.name,
                scenario.state_access_profile.as_str(),
            );
            if selected_gas_keys.is_none_or(|keys| keys.contains(&record_key)) {
                write_gas_test(&mut out, index, artifact, scenario);
            }
        }
    }

    if include_behavior_checks {
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
    }

    out.push_str("}\n");
    Ok(out)
}

fn support_contracts() -> &'static str {
    include_str!("foundry_templates/support.sol")
}

fn helper_functions(artifacts: &[CompiledArtifact]) -> String {
    let mut out = String::new();
    let needs_curve = artifacts
        .iter()
        .any(|artifact| artifact.benchmark_id == "curve_stableswap_2coin");
    let needs_uniswap = artifacts
        .iter()
        .any(|artifact| artifact.benchmark_id == "uniswap_v2_pair");
    let needs_yearn = artifacts
        .iter()
        .any(|artifact| artifact.benchmark_id == "yearn_vault_v3");
    let mut include = true;

    for line in all_helper_functions().lines() {
        let trimmed = line.trim();
        if let Some(section) = trimmed.strip_prefix("// bench-cli:helpers begin ") {
            include = match section {
                "curve" => needs_curve,
                "uniswap" => needs_uniswap,
                "yearn" => needs_yearn,
                _ => true,
            };
            continue;
        }
        if trimmed.starts_with("// bench-cli:helpers end") {
            include = true;
            continue;
        }
        if include {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

fn all_helper_functions() -> &'static str {
    include_str!("foundry_templates/helpers.sol")
}

fn randomized_helper_functions() -> &'static str {
    include_str!("foundry_templates/randomized_helpers.sol")
}

fn write_deploy_function(out: &mut String, index: usize, artifact: &CompiledArtifact) {
    let has_deployment_variants = matches!(
        artifact.benchmark_id.as_str(),
        "curve_stableswap_2coin" | "uniswap_v2_pair"
    );
    out.push_str("    function deployArtifact");
    out.push_str(&index.to_string());
    out.push_str("() internal returns (address target, uint256 deployGas) {\n");
    if has_deployment_variants {
        out.push_str("        return deployArtifact");
        out.push_str(&index.to_string());
        out.push_str("(0);\n");
        out.push_str("    }\n\n");
        out.push_str("    function deployArtifact");
        out.push_str(&index.to_string());
        out.push_str(
            "(uint8 deploymentVariant) internal returns (address target, uint256 deployGas) {\n",
        );
    }
    out.push_str("        bytes memory code = hex\"");
    out.push_str(artifact.creation_bytecode.trim_start_matches("0x"));
    out.push_str("\";\n");
    if artifact.benchmark_id == "curve_stableswap_2coin" {
        out.push_str("        BenchERC20OptionalReturn coin0;\n");
        out.push_str("        BenchERC20OptionalReturn coin1 = new BenchERC20OptionalReturn();\n");
        out.push_str("        BenchERC20OptionalReturn coin2;\n");
        out.push_str("        BenchERC20OptionalReturn coin3;\n");
        out.push_str("        BenchERC20OptionalReturn coin4;\n");
        out.push_str("        BenchERC20OptionalReturn coin5;\n");
        out.push_str("        BenchERC20OptionalReturn coin6;\n");
        out.push_str("        BenchERC20OptionalReturn coin7;\n");
        out.push_str("        if (deploymentVariant == 3) {\n");
        out.push_str(
            "            BenchERC20OptionalReturn underlying0 = new BenchERC20OptionalReturn();\n",
        );
        out.push_str("            coin0 = new BenchCurveERC4626(address(underlying0), 1_125_000_000_000_000_000);\n");
        out.push_str("        } else {\n");
        out.push_str("            coin0 = new BenchERC20OptionalReturn();\n");
        out.push_str("        }\n");
        out.push_str("        if (deploymentVariant == 4 || deploymentVariant == 5 || deploymentVariant == 6) {\n");
        out.push_str("            coin2 = new BenchERC20OptionalReturn();\n");
        out.push_str("        }\n");
        out.push_str("        if (deploymentVariant == 5 || deploymentVariant == 6) {\n");
        out.push_str("            coin3 = new BenchERC20OptionalReturn();\n");
        out.push_str("            coin4 = new BenchERC20OptionalReturn();\n");
        out.push_str("        }\n");
        out.push_str("        if (deploymentVariant == 5) {\n");
        out.push_str("            coin5 = new BenchERC20OptionalReturn();\n");
        out.push_str("            coin6 = new BenchERC20OptionalReturn();\n");
        out.push_str("            coin7 = new BenchERC20OptionalReturn();\n");
        out.push_str("        }\n");
        out.push_str(
            "        uint256 nCoins = deploymentVariant == 5 ? uint256(8) : deploymentVariant == 6 ? uint256(5) : deploymentVariant == 4 ? uint256(3) : uint256(2);\n",
        );
        out.push_str("        address[] memory coins = new address[](nCoins);\n");
        out.push_str("        coins[0] = address(coin0);\n");
        out.push_str("        coins[1] = address(coin1);\n");
        out.push_str("        if (deploymentVariant == 4 || deploymentVariant == 5 || deploymentVariant == 6) {\n");
        out.push_str("            coins[2] = address(coin2);\n");
        out.push_str("        }\n");
        out.push_str("        if (deploymentVariant == 5 || deploymentVariant == 6) {\n");
        out.push_str("            coins[3] = address(coin3);\n");
        out.push_str("            coins[4] = address(coin4);\n");
        out.push_str("        }\n");
        out.push_str("        if (deploymentVariant == 5) {\n");
        out.push_str("            coins[5] = address(coin5);\n");
        out.push_str("            coins[6] = address(coin6);\n");
        out.push_str("            coins[7] = address(coin7);\n");
        out.push_str("        }\n");
        out.push_str("        uint256[] memory rates = new uint256[](nCoins);\n");
        out.push_str("        for (uint256 i = 0; i < nCoins; i++) {\n");
        out.push_str("            rates[i] = 1e18;\n");
        out.push_str("        }\n");
        out.push_str("        uint8[] memory assetTypes = new uint8[](nCoins);\n");
        out.push_str("        bytes4[] memory methodIds = new bytes4[](nCoins);\n");
        out.push_str("        address[] memory oracles = new address[](nCoins);\n");
        out.push_str("        if (deploymentVariant == 1) {\n");
        out.push_str("            BenchCurveRateOracle oracle0 = new BenchCurveRateOracle(1_250_000_000_000_000_000);\n");
        out.push_str("            assetTypes[0] = 1;\n");
        out.push_str("            methodIds[0] = BenchCurveRateOracle.rate.selector;\n");
        out.push_str("            oracles[0] = address(oracle0);\n");
        out.push_str("        } else if (deploymentVariant == 2) {\n");
        out.push_str("            assetTypes[0] = 2;\n");
        out.push_str("        } else if (deploymentVariant == 3) {\n");
        out.push_str("            assetTypes[0] = 3;\n");
        out.push_str("        } else {\n");
        out.push_str("            require(deploymentVariant == 0 || deploymentVariant == 4 || deploymentVariant == 5 || deploymentVariant == 6, \"curve variant\");\n");
        out.push_str("        }\n");
        out.push_str("        code = abi.encodePacked(code, abi.encode(\"Curve.fi Stablecoin\", \"crv2\", uint256(200), uint256(4_000_000), uint256(20_000_000_000), uint256(866), coins, rates, assetTypes, methodIds, oracles));\n");
    }
    if artifact.benchmark_id == "uniswap_v2_pair" {
        out.push_str("        BenchERC20 uniswapToken0;\n");
        out.push_str("        BenchERC20 uniswapToken1;\n");
        out.push_str("        BenchUniswapFlashCallee uniswapFlashCallee;\n");
        out.push_str("        BenchUniswapReentrantCallee uniswapReentrantCallee;\n");
        out.push_str("        BenchUniswapCreate2Factory uniswapFactory;\n");
        out.push_str("        if (deploymentVariant == 1) {\n");
        out.push_str("            uniswapToken0 = new BenchERC20();\n");
        out.push_str("            uniswapToken1 = new BenchERC20();\n");
        out.push_str("            uniswapFlashCallee = new BenchUniswapFlashCallee();\n");
        out.push_str("            uniswapReentrantCallee = new BenchUniswapReentrantCallee();\n");
        out.push_str("            uniswapFactory = _uniswapCreate2Factory();\n");
        out.push_str("        } else {\n");
        out.push_str("            require(deploymentVariant == 0, \"uniswap variant\");\n");
        out.push_str("        }\n");
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
    } else if artifact.benchmark_id == "uniswap_v2_pair" {
        out.push_str("        if (deploymentVariant == 1) {\n");
        out.push_str("            target = uniswapFactory.deployPair(code, keccak256(abi.encode(SALT, keccak256(code))), address(uniswapToken0), address(uniswapToken1));\n");
        out.push_str("        } else {\n");
        out.push_str("            target = _deploy(code);\n");
        out.push_str("        }\n");
    } else {
        out.push_str("        target = _deploy(code);\n");
    }
    out.push_str("        deployGas = startGas - gasleft();\n");
    if artifact.benchmark_id == "curve_stableswap_2coin" {
        out.push_str("        curveDeps[target] = CurveDeps(coin0, coin1, coin2, coin3, coin4, coin5, coin6, coin7);\n");
        out.push_str("        coin0.mint(address(this), 1e30);\n");
        out.push_str("        coin1.mint(address(this), 1e30);\n");
        out.push_str("        if (address(coin2) != address(0)) {\n");
        out.push_str("            coin2.mint(address(this), 1e30);\n");
        out.push_str("            coin2.approve(target, type(uint256).max);\n");
        out.push_str("        }\n");
        out.push_str("        if (address(coin3) != address(0)) {\n");
        out.push_str("            coin3.mint(address(this), 1e30);\n");
        out.push_str("            coin3.approve(target, type(uint256).max);\n");
        out.push_str("            coin4.mint(address(this), 1e30);\n");
        out.push_str("            coin4.approve(target, type(uint256).max);\n");
        out.push_str("        }\n");
        out.push_str("        if (address(coin5) != address(0)) {\n");
        out.push_str("            coin5.mint(address(this), 1e30);\n");
        out.push_str("            coin5.approve(target, type(uint256).max);\n");
        out.push_str("            coin6.mint(address(this), 1e30);\n");
        out.push_str("            coin6.approve(target, type(uint256).max);\n");
        out.push_str("            coin7.mint(address(this), 1e30);\n");
        out.push_str("            coin7.approve(target, type(uint256).max);\n");
        out.push_str("        }\n");
        out.push_str("        coin0.approve(target, type(uint256).max);\n");
        out.push_str("        coin1.approve(target, type(uint256).max);\n");
    }
    if artifact.benchmark_id == "uniswap_v2_pair" {
        out.push_str("        if (deploymentVariant == 1) {\n");
        out.push_str("            pairDeps[target] = PairDeps(uniswapToken0, uniswapToken1, uniswapFlashCallee, uniswapReentrantCallee);\n");
        out.push_str("        }\n");
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
    out.push_str("        (address target, uint256 deployGas) = ");
    out.push_str(&deploy_call(index, artifact, scenario));
    out.push_str(";\n");
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
    out.push_str("        (address solTarget,) = ");
    out.push_str(&deploy_call(solidity_idx, solidity, scenario));
    out.push_str(";\n");
    out.push_str("        (address vyperTarget,) = ");
    out.push_str(&deploy_call(vyper_idx, vyper, scenario));
    out.push_str(";\n");
    out.push_str("        vm.warp(1);\n");
    write_setup(out, "solTarget", &scenario.setup, "setup");
    write_setup(out, "solTarget", &scenario.warmup, "warmup");
    if supports_log_diff(benchmark_id) {
        out.push_str(
            "        (bool solOk, bytes32 solHash, bytes32 solLogHash,) = _runWithLogs(solTarget, ",
        );
        out.push_str(call_destination(&scenario.measured, "solTarget"));
        out.push_str(", ");
        write_call_args(out, &scenario.measured, "solTarget");
        out.push_str(");\n");
    } else {
        out.push_str("        (bool solOk, bytes32 solHash,) = _run(");
        out.push_str(call_destination(&scenario.measured, "solTarget"));
        out.push_str(", ");
        write_call_args(out, &scenario.measured, "solTarget");
        out.push_str(");\n");
    }
    out.push_str("        bytes32 solObserved = _observeAll_");
    out.push_str(&sanitize(&solidity.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("(solTarget);\n");
    out.push_str("        vm.warp(1);\n");
    write_setup(out, "vyperTarget", &scenario.setup, "setup");
    write_setup(out, "vyperTarget", &scenario.warmup, "warmup");
    if supports_log_diff(benchmark_id) {
        out.push_str(
            "        (bool vyperOk, bytes32 vyperHash, bytes32 vyperLogHash,) = _runWithLogs(vyperTarget, ",
        );
        out.push_str(call_destination(&scenario.measured, "vyperTarget"));
        out.push_str(", ");
        write_call_args(out, &scenario.measured, "vyperTarget");
        out.push_str(");\n");
    } else {
        out.push_str("        (bool vyperOk, bytes32 vyperHash,) = _run(");
        out.push_str(call_destination(&scenario.measured, "vyperTarget"));
        out.push_str(", ");
        write_call_args(out, &scenario.measured, "vyperTarget");
        out.push_str(");\n");
    }
    out.push_str("        bytes32 vyperObserved = _observeAll_");
    out.push_str(&sanitize(&vyper.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(&scenario.name));
    out.push_str("(vyperTarget);\n");
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
    out.push_str(
        "        require(solObserved == vyperObserved, \"differential observer mismatch\");\n",
    );
    if supports_log_diff(benchmark_id) {
        out.push_str(
            "        require(solLogHash == vyperLogHash, \"differential log mismatch\");\n",
        );
    }
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

fn supports_log_diff(benchmark_id: &str) -> bool {
    matches!(
        benchmark_id,
        "curve_stableswap_2coin" | "uniswap_v2_pair" | "yearn_vault_v3"
    )
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
        out.push_str("        observed = keccak256(abi.encode(observed, _observe(");
        out.push_str(call_destination(observer, "target"));
        out.push_str(", ");
        out.push_str(&call_data(observer, "target"));
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

fn deploy_call(index: usize, artifact: &CompiledArtifact, scenario: &Scenario) -> String {
    if matches!(
        artifact.benchmark_id.as_str(),
        "curve_stableswap_2coin" | "uniswap_v2_pair"
    ) && scenario.deployment_variant != DeploymentVariant::Standard
    {
        format!(
            "deployArtifact{}({})",
            index,
            scenario.deployment_variant.as_solidity_arg()
        )
    } else {
        format!("deployArtifact{}()", index)
    }
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

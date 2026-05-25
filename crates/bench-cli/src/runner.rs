use crate::{
    cache::{self, CacheLookup},
    models::{
        CacheInfo, CallSpec, CompileSet, CompiledArtifact, GasRecord, PropertySpec, RandomizedSpec,
        Scenario,
    },
    scenarios::ScenarioCatalog,
    util::{Progress, ensure_dir, require_success, run_measured, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const FAILURE_DIR: &str = "../results/raw/failures";
const SOL_BASELINE: &str = "solc-latest-viair-runs10000";
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
    out.push_str("interface Vm {\n");
    out.push_str("    function createDir(string calldata path, bool recursive) external;\n");
    out.push_str("    function writeFile(string calldata path, string calldata data) external;\n");
    out.push_str("    function writeLine(string calldata path, string calldata data) external;\n");
    out.push_str("    function toString(uint256 value) external pure returns (string memory);\n");
    out.push_str("    function prank(address sender) external;\n");
    out.push_str("    function deal(address account, uint256 newBalance) external;\n");
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
    out.push_str("    receive() external payable {}\n\n");
    out.push_str("    function setUp() public {\n");
    out.push_str("        vm.writeFile(GAS_JSONL_PATH, \"\");\n");
    out.push_str("        vm.createDir(\"");
    out.push_str(FAILURE_DIR);
    out.push_str("\", true);\n");
    out.push_str("        vm.deal(address(this), 1000000 ether);\n");
    out.push_str("        vm.deal(BOB, 1000000 ether);\n");
    out.push_str("        vm.deal(CAROL, 1000000 ether);\n");
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

    function _deploy(bytes memory code) internal returns (address target) {
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0), "deploy failed");
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
    if let Some(args) = constructor_args(&artifact.benchmark_id) {
        out.push_str("        code = abi.encodePacked(code, ");
        out.push_str(args);
        out.push_str(");\n");
    }
    out.push_str("        uint256 startGas = gasleft();\n");
    out.push_str("        target = _deploy(code);\n");
    out.push_str("        deployGas = startGas - gasleft();\n");
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
    out.push_str(&scenario.measured.data);
    out.push_str(");\n");
    out.push_str("        (bool ok,, uint256 executionGas) = _run(target, ");
    write_call_args(out, &scenario.measured);
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
    out.push_str("        (bool solOk, bytes32 solHash,) = _run(solTarget, ");
    write_call_args(out, &scenario.measured);
    out.push_str(");\n");
    out.push_str("        (bool vyperOk, bytes32 vyperHash,) = _run(vyperTarget, ");
    write_call_args(out, &scenario.measured);
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
        out.push_str(target);
        out.push_str(", ");
        write_call_args(out, call);
        out.push_str(");\n");
        out.push_str("        require(setupOk, \"");
        out.push_str(label);
        out.push_str(" call failed\"); }\n");
    }
}

fn write_call_args(out: &mut String, call: &CallSpec) {
    out.push_str(&call.data);
    out.push_str(", ");
    out.push_str(&call.value);
    out.push_str(", ");
    out.push_str(call.sender.as_deref().unwrap_or("address(this)"));
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

use crate::{
    models::{Benchmark, BenchmarkSuite, CallSpec, Scenario, ScenarioFile, StateAccessProfile},
    util::{ensure_dir, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub const SCALE_GENERATOR_VERSION: &str = "scale-v1";
const GENERATED_ROOT: &str = "target/bench-generated";
const EXPECTED_FAMILIES: [&str; 7] = [
    "dispatch_N",
    "storage_slots_N",
    "mapping_depth_N",
    "abi_args_N",
    "loop_bound_N",
    "external_calls_N",
    "events_N",
];

#[derive(Debug, Clone)]
pub struct GeneratedSuite {
    pub benchmarks: Vec<Benchmark>,
    pub scenarios: Vec<ScenarioFile>,
    pub manifest: ScaleManifest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConfig {
    pub version: u32,
    pub parameter_name: String,
    pub values: Vec<u64>,
    pub families: Vec<FamilyConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyConfig {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScaleManifest {
    pub generator_version: String,
    pub config_hash: String,
    pub parameter_name: String,
    pub values: Vec<u64>,
    pub benchmarks: Vec<ScaleBenchmarkManifest>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScaleBenchmarkManifest {
    pub benchmark_id: String,
    pub family: String,
    pub parameter_name: String,
    pub parameter_value: u64,
    pub contract_name: String,
    pub solidity_path: String,
    pub solidity_hash: String,
    pub vyper_path: String,
    pub vyper_hash: String,
    pub spec_path: String,
    pub spec_hash: String,
    pub scenario_path: String,
    pub scenario_hash: String,
}

struct GeneratedSource {
    contract_name: String,
    solidity: String,
    vyper: String,
    scenarios: Vec<Scenario>,
    abi: Vec<String>,
    semantics: Vec<String>,
}

pub fn generate_scale_suite(root: &Path, only_benchmark: Option<&str>) -> Result<GeneratedSuite> {
    let (config, config_hash) = load_scale_config(root)?;
    let generated_root = root.join(GENERATED_ROOT);
    if generated_root.exists() {
        fs::remove_dir_all(&generated_root)
            .with_context(|| format!("removing {}", generated_root.display()))?;
    }
    ensure_dir(&generated_root)?;

    let mut benchmarks = Vec::new();
    let mut scenarios = Vec::new();
    let mut manifest_rows = Vec::new();

    for family in &config.families {
        for &parameter_value in &config.values {
            let benchmark_id = benchmark_id(&family.id, parameter_value)?;
            let generated = generate_family(&family.id, parameter_value)?;
            let family_dir = generated_root
                .join("implementations")
                .join(&family.id)
                .join(parameter_value.to_string());
            let solidity_path = family_dir
                .join("solidity")
                .join(format!("{}.sol", generated.contract_name));
            let vyper_path = family_dir
                .join("vyper")
                .join(format!("{}.vy", generated.contract_name));
            write_file(&solidity_path, &generated.solidity)?;
            write_file(&vyper_path, &generated.vyper)?;

            let spec_path = generated_root
                .join("specs")
                .join(format!("{benchmark_id}.yaml"));
            let scenario_path = generated_root
                .join("scenarios")
                .join(format!("{benchmark_id}.yaml"));
            let solidity_rel = rel(root, &solidity_path)?;
            let vyper_rel = rel(root, &vyper_path)?;
            let spec_rel = rel(root, &spec_path)?;
            let scenario_rel = rel(root, &scenario_path)?;

            let scenario_file = ScenarioFile {
                benchmark_id: benchmark_id.clone(),
                scenarios: generated.scenarios,
                randomized: None,
                properties: Vec::new(),
            };
            let scenario_text = serde_yaml::to_string(&scenario_file)?;
            write_file(&scenario_path, &scenario_text)?;

            let spec = json!({
                "id": benchmark_id,
                "title": format!("{} {}", family.title, parameter_value),
                "abi": generated.abi,
                "semantics": generated.semantics,
                "scale": {
                    "family": family.id,
                    "parameter_name": config.parameter_name,
                    "parameter_value": parameter_value
                },
                "scenarios": scenario_file.scenarios.iter().map(|scenario| json!({
                    "name": scenario.name.clone(),
                    "call": scenario.measured.data.clone(),
                    "expected": if scenario.expect_success { "success" } else { "revert" }
                })).collect::<Vec<_>>(),
                "implementations": {
                    "solidity": solidity_rel,
                    "vyper": vyper_rel
                }
            });
            let spec_text = serde_yaml::to_string(&spec)?;
            write_file(&spec_path, &spec_text)?;

            let solidity_hash = sha256_bytes(generated.solidity.as_bytes());
            let vyper_hash = sha256_bytes(generated.vyper.as_bytes());
            let spec_hash = sha256_bytes(spec_text.as_bytes());
            let scenario_hash = sha256_bytes(scenario_text.as_bytes());

            manifest_rows.push(ScaleBenchmarkManifest {
                benchmark_id: benchmark_id.clone(),
                family: family.id.clone(),
                parameter_name: config.parameter_name.clone(),
                parameter_value,
                contract_name: generated.contract_name.clone(),
                solidity_path: solidity_rel.clone(),
                solidity_hash,
                vyper_path: vyper_rel.clone(),
                vyper_hash,
                spec_path: spec_rel,
                spec_hash,
                scenario_path: scenario_rel.clone(),
                scenario_hash: scenario_hash.clone(),
            });

            if only_benchmark.is_none_or(|id| id == benchmark_id) {
                benchmarks.push(Benchmark {
                    id: benchmark_id,
                    contract_name: generated.contract_name,
                    solidity_path: solidity_rel,
                    vyper_path: vyper_rel,
                    suite: BenchmarkSuite::Scale,
                    family: Some(family.id.clone()),
                    parameter_name: Some(config.parameter_name.clone()),
                    parameter_value: Some(parameter_value),
                    scenario_path: Some(scenario_rel),
                    scenario_hash: Some(scenario_hash),
                    generator_version: Some(SCALE_GENERATOR_VERSION.to_string()),
                    provenance: None,
                });
                scenarios.push(scenario_file);
            }
        }
    }

    let manifest = ScaleManifest {
        generator_version: SCALE_GENERATOR_VERSION.to_string(),
        config_hash,
        parameter_name: config.parameter_name,
        values: config.values,
        benchmarks: manifest_rows,
    };
    let manifest_path = generated_root.join("manifest.json");
    write_file(&manifest_path, &serde_json::to_string_pretty(&manifest)?)?;

    Ok(GeneratedSuite {
        benchmarks,
        scenarios,
        manifest,
    })
}

pub fn load_scale_config(root: &Path) -> Result<(ScaleConfig, String)> {
    let family_dir = root.join("benches/families");
    let mut files = yaml_files(&family_dir)?;
    if files.is_empty() {
        bail!("no scale family definitions in {}", family_dir.display());
    }
    if files.len() != 1 {
        bail!(
            "expected one scale family definition file, found {}",
            files.len()
        );
    }
    let path = files.pop().expect("checked non-empty");
    let text = fs::read_to_string(&path)?;
    let config: ScaleConfig =
        serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    validate_scale_config(&config, &path)?;
    Ok((config, sha256_bytes(text.as_bytes())))
}

pub fn validate_scale_config(config: &ScaleConfig, path: &Path) -> Result<()> {
    if config.version != 1 {
        bail!(
            "{} unsupported scale config version {}",
            path.display(),
            config.version
        );
    }
    if config.parameter_name != "N" {
        bail!("{} parameter_name must be N", path.display());
    }
    if config.values.is_empty() {
        bail!("{} values must not be empty", path.display());
    }
    let mut values = BTreeSet::new();
    for value in &config.values {
        if *value == 0 {
            bail!("{} values must be positive", path.display());
        }
        if !values.insert(*value) {
            bail!("{} duplicate scale value {value}", path.display());
        }
    }
    let expected: BTreeSet<_> = EXPECTED_FAMILIES.into_iter().collect();
    let actual: BTreeSet<_> = config
        .families
        .iter()
        .map(|family| family.id.as_str())
        .collect();
    if actual != expected {
        bail!(
            "{} must define exactly the seven milestone-3 families",
            path.display()
        );
    }
    Ok(())
}

fn generate_family(family_id: &str, n: u64) -> Result<GeneratedSource> {
    match family_id {
        "dispatch_N" => Ok(dispatch_family(n)),
        "storage_slots_N" => Ok(storage_slots_family(n)),
        "mapping_depth_N" => Ok(mapping_depth_family(n)),
        "abi_args_N" => Ok(abi_args_family(n)),
        "loop_bound_N" => Ok(loop_bound_family(n)),
        "external_calls_N" => Ok(external_calls_family(n)),
        "events_N" => Ok(events_family(n)),
        _ => bail!("unsupported scale family {family_id}"),
    }
}

fn dispatch_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("Dispatch", n);
    let mut sol = solidity_header(&contract_name);
    sol.push_str("    uint256 public sink;\n\n");
    sol.push_str("    function setSink(uint256 value) external returns (uint256) {\n        sink = value;\n        return value;\n    }\n\n");
    for i in 0..n {
        sol.push_str(&format!(
            "    function f{i:03}() external pure returns (uint256) {{ return {i}; }}\n"
        ));
    }
    sol.push_str("}\n");

    let mut vy = vyper_header();
    vy.push_str("sink: public(uint256)\n\n");
    vy.push_str("@external\ndef setSink(new_value: uint256) -> uint256:\n    self.sink = new_value\n    return new_value\n\n");
    for i in 0..n {
        vy.push_str(&format!(
            "@external\n@pure\ndef f{i:03}() -> uint256:\n    return {i}\n"
        ));
    }

    let mut scenarios = vec![scenario(
        "first_selector",
        StateAccessProfile::Cold,
        Vec::new(),
        Vec::new(),
        call(format!("abi.encodeWithSignature(\"f{:03}()\")", 0)),
        true,
        Vec::new(),
    )];
    if n > 1 {
        scenarios.push(scenario(
            "last_selector",
            StateAccessProfile::Cold,
            Vec::new(),
            Vec::new(),
            call(format!("abi.encodeWithSignature(\"f{:03}()\")", n - 1)),
            true,
            Vec::new(),
        ));
    }
    scenarios.push(scenario(
        "set_sink",
        StateAccessProfile::Warm,
        Vec::new(),
        vec![call("abi.encodeWithSignature(\"sink()\")")],
        call("abi.encodeWithSignature(\"setSink(uint256)\", uint256(99))"),
        true,
        vec![call("abi.encodeWithSignature(\"sink()\")")],
    ));

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios,
        abi: vec![
            "sink() returns (uint256)".to_string(),
            "setSink(uint256) returns (uint256)".to_string(),
            format!("{n} generated selector functions f000..f{:03}", n - 1),
        ],
        semantics: vec![format!(
            "Exposes {n} fixed external functions to exercise selector dispatch."
        )],
    }
}

fn storage_slots_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("StorageSlots", n);
    let mut sol = solidity_header(&contract_name);
    for i in 0..n {
        sol.push_str(&format!("    uint256 public slot{i:03};\n"));
    }
    sol.push_str("\n    function writeAll(uint256 seed) external returns (uint256 total) {\n");
    for i in 0..n {
        sol.push_str(&format!(
            "        slot{i:03} = seed + {i};\n        total += slot{i:03};\n"
        ));
    }
    sol.push_str("    }\n\n    function readAll() external view returns (uint256 total) {\n");
    for i in 0..n {
        sol.push_str(&format!("        total += slot{i:03};\n"));
    }
    sol.push_str("    }\n}\n");

    let mut vy = vyper_header();
    for i in 0..n {
        vy.push_str(&format!("slot{i:03}: public(uint256)\n"));
    }
    vy.push_str("\n@external\ndef writeAll(seed: uint256) -> uint256:\n    total: uint256 = 0\n");
    for i in 0..n {
        vy.push_str(&format!(
            "    self.slot{i:03} = seed + {i}\n    total += self.slot{i:03}\n"
        ));
    }
    vy.push_str(
        "    return total\n\n@external\n@view\ndef readAll() -> uint256:\n    total: uint256 = 0\n",
    );
    for i in 0..n {
        vy.push_str(&format!("    total += self.slot{i:03}\n"));
    }
    vy.push_str("    return total\n");

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: standard_read_write_scenarios("readAll()", "writeAll(uint256)"),
        abi: vec![
            "readAll() returns (uint256)".to_string(),
            "writeAll(uint256) returns (uint256)".to_string(),
        ],
        semantics: vec![format!(
            "Touches {n} independent storage slots in read and write paths."
        )],
    }
}

fn mapping_depth_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("MappingDepth", n);
    let mut sol = solidity_header(&contract_name);
    for i in 0..n {
        sol.push_str(&format!(
            "    mapping(uint256 => uint256) public link{i:03};\n"
        ));
    }
    sol.push_str("\n    function writeChain(uint256 seed) external returns (uint256 current) {\n        current = seed;\n");
    for i in 0..n {
        sol.push_str(&format!(
            "        link{i:03}[current] = current + {add};\n        current = link{i:03}[current];\n",
            add = i + 1
        ));
    }
    sol.push_str("    }\n\n    function readChain(uint256 seed) external view returns (uint256 current) {\n        current = seed;\n");
    for i in 0..n {
        sol.push_str(&format!("        current = link{i:03}[current];\n"));
    }
    sol.push_str("    }\n}\n");

    let mut vy = vyper_header();
    for i in 0..n {
        vy.push_str(&format!("link{i:03}: public(HashMap[uint256, uint256])\n"));
    }
    vy.push_str(
        "\n@external\ndef writeChain(seed: uint256) -> uint256:\n    current: uint256 = seed\n",
    );
    for i in 0..n {
        vy.push_str(&format!(
            "    self.link{i:03}[current] = current + {add}\n    current = self.link{i:03}[current]\n",
            add = i + 1
        ));
    }
    vy.push_str("    return current\n\n@external\n@view\ndef readChain(seed: uint256) -> uint256:\n    current: uint256 = seed\n");
    for i in 0..n {
        vy.push_str(&format!("    current = self.link{i:03}[current]\n"));
    }
    vy.push_str("    return current\n");

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: vec![
            scenario(
                "read_empty_chain",
                StateAccessProfile::Cold,
                Vec::new(),
                Vec::new(),
                call("abi.encodeWithSignature(\"readChain(uint256)\", uint256(5))"),
                true,
                Vec::new(),
            ),
            scenario(
                "write_chain",
                StateAccessProfile::Mixed,
                Vec::new(),
                Vec::new(),
                call("abi.encodeWithSignature(\"writeChain(uint256)\", uint256(5))"),
                true,
                vec![call(
                    "abi.encodeWithSignature(\"readChain(uint256)\", uint256(5))",
                )],
            ),
            scenario(
                "read_after_write",
                StateAccessProfile::Warm,
                vec![call(
                    "abi.encodeWithSignature(\"writeChain(uint256)\", uint256(5))",
                )],
                vec![call(
                    "abi.encodeWithSignature(\"readChain(uint256)\", uint256(5))",
                )],
                call("abi.encodeWithSignature(\"readChain(uint256)\", uint256(5))"),
                true,
                Vec::new(),
            ),
        ],
        abi: vec![
            "writeChain(uint256) returns (uint256)".to_string(),
            "readChain(uint256) returns (uint256)".to_string(),
        ],
        semantics: vec![format!("Performs {n} chained mapping key lookups.")],
    }
}

fn abi_args_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("AbiArgs", n);
    let signature = repeated("uint256", n, ",");
    let args = (1..=n)
        .map(|value| format!("uint256({value})"))
        .collect::<Vec<_>>()
        .join(", ");
    let sol_params = (0..n)
        .map(|i| format!("uint256 a{i:03}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sol_sum = (0..n)
        .map(|i| format!("a{i:03}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let mut sol = solidity_header(&contract_name);
    sol.push_str(&format!(
        "    function sum({sol_params}) external pure returns (uint256 out) {{\n        return {sol_sum};\n    }}\n}}\n"
    ));

    let mut vy = vyper_header();
    let params = (0..n)
        .map(|i| format!("a{i:03}: uint256"))
        .collect::<Vec<_>>()
        .join(", ");
    let sum = (0..n)
        .map(|i| format!("a{i:03}"))
        .collect::<Vec<_>>()
        .join(" + ");
    vy.push_str(&format!(
        "@external\n@pure\ndef sum({params}) -> uint256:\n    return {sum}\n"
    ));

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: vec![scenario(
            "sum_args",
            StateAccessProfile::Cold,
            Vec::new(),
            Vec::new(),
            call(format!(
                "abi.encodeWithSignature(\"sum({signature})\", {args})"
            )),
            true,
            Vec::new(),
        )],
        abi: vec![format!("sum({signature}) returns (uint256)")],
        semantics: vec![format!(
            "Accepts and sums {n} high-level uint256 ABI arguments."
        )],
    }
}

fn loop_bound_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("LoopBound", n);
    let mut sol = solidity_header(&contract_name);
    sol.push_str(&format!(
        "    function runLoop() external pure returns (uint256 total) {{\n        for (uint256 i = 0; i < {n};) {{\n            total += i;\n            unchecked {{ ++i; }}\n        }}\n    }}\n}}\n"
    ));

    let mut vy = vyper_header();
    vy.push_str(&format!(
        "@external\n@pure\ndef runLoop() -> uint256:\n    total: uint256 = 0\n    for i: uint256 in range({n}):\n        total += i\n    return total\n"
    ));

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: vec![simple_scenario("run_loop", "runLoop()")],
        abi: vec!["runLoop() returns (uint256)".to_string()],
        semantics: vec![format!(
            "Runs a statically bounded loop with {n} iterations."
        )],
    }
}

fn external_calls_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("ExternalCalls", n);
    let mut sol = solidity_header(&contract_name);
    sol.push_str("    function ping(uint256) external pure {}\n\n");
    sol.push_str(&format!(
        "    function callMany() external returns (uint256 total) {{\n        for (uint256 i = 0; i < {n};) {{\n            (bool ok,) = address(this).staticcall(abi.encodeWithSignature(\"ping(uint256)\", i));\n            require(ok, \"ping\");\n            total += i;\n            unchecked {{ ++i; }}\n        }}\n    }}\n}}\n"
    ));

    let mut vy = vyper_header();
    vy.push_str("@external\n@view\ndef ping(x: uint256):\n    pass\n\n");
    vy.push_str(&format!(
        "@external\ndef callMany() -> uint256:\n    total: uint256 = 0\n    for i: uint256 in range({n}):\n        raw_call(self, concat(method_id(\"ping(uint256)\"), abi_encode(i)), max_outsize=0, is_static_call=True)\n        total += i\n    return total\n"
    ));

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: vec![simple_scenario("call_many", "callMany()")],
        abi: vec![
            "ping(uint256)".to_string(),
            "callMany() returns (uint256)".to_string(),
        ],
        semantics: vec![format!(
            "Performs {n} deterministic external static calls to a local callee."
        )],
    }
}

fn events_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("Events", n);
    let mut sol = solidity_header(&contract_name);
    sol.push_str("    event Tick(uint256 indexed index, uint256 value);\n\n");
    sol.push_str(&format!(
        "    function emitMany() external returns (uint256 total) {{\n        for (uint256 i = 0; i < {n};) {{\n            emit Tick(i, i + 1);\n            total += i + 1;\n            unchecked {{ ++i; }}\n        }}\n    }}\n}}\n"
    ));

    let mut vy = vyper_header();
    vy.push_str("event Tick:\n    index: indexed(uint256)\n    value: uint256\n\n");
    vy.push_str(&format!(
        "@external\ndef emitMany() -> uint256:\n    total: uint256 = 0\n    for i: uint256 in range({n}):\n        log Tick(index=i, value=i + 1)\n        total += i + 1\n    return total\n"
    ));

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        scenarios: vec![simple_scenario("emit_many", "emitMany()")],
        abi: vec![
            "event Tick(uint256 indexed index, uint256 value)".to_string(),
            "emitMany() returns (uint256)".to_string(),
        ],
        semantics: vec![format!(
            "Emits {n} deterministic events in one measured call."
        )],
    }
}

fn standard_read_write_scenarios(read_sig: &str, write_sig: &str) -> Vec<Scenario> {
    vec![
        simple_scenario("read_initial", read_sig),
        scenario(
            "write_all",
            StateAccessProfile::Mixed,
            Vec::new(),
            Vec::new(),
            call(format!(
                "abi.encodeWithSignature(\"{write_sig}\", uint256(7))"
            )),
            true,
            vec![call(format!("abi.encodeWithSignature(\"{read_sig}\")"))],
        ),
        scenario(
            "read_after_write",
            StateAccessProfile::Warm,
            vec![call(format!(
                "abi.encodeWithSignature(\"{write_sig}\", uint256(7))"
            ))],
            vec![call(format!("abi.encodeWithSignature(\"{read_sig}\")"))],
            call(format!("abi.encodeWithSignature(\"{read_sig}\")")),
            true,
            Vec::new(),
        ),
    ]
}

fn simple_scenario(name: &str, signature: &str) -> Scenario {
    scenario(
        name,
        StateAccessProfile::Cold,
        Vec::new(),
        Vec::new(),
        call(format!("abi.encodeWithSignature(\"{signature}\")")),
        true,
        Vec::new(),
    )
}

fn scenario(
    name: &str,
    state_access_profile: StateAccessProfile,
    setup: Vec<CallSpec>,
    warmup: Vec<CallSpec>,
    measured: CallSpec,
    expect_success: bool,
    observers: Vec<CallSpec>,
) -> Scenario {
    Scenario {
        name: name.to_string(),
        state_access_profile,
        setup,
        warmup,
        measured,
        expect_success,
        observers,
    }
}

fn call(data: impl Into<String>) -> CallSpec {
    CallSpec {
        data: data.into(),
        sender: None,
        value: "0".to_string(),
    }
}

fn solidity_header(contract_name: &str) -> String {
    format!(
        "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.30;\n\ncontract {contract_name} {{\n"
    )
}

fn vyper_header() -> String {
    "# pragma version >=0.4.3,<0.6.0\n\n".to_string()
}

fn contract_name(stem: &str, n: u64) -> String {
    format!("Scale{stem}{n}")
}

fn benchmark_id(family_id: &str, n: u64) -> Result<String> {
    let prefix = family_id
        .strip_suffix("_N")
        .with_context(|| format!("family id {family_id} must end in _N"))?;
    Ok(format!("scale_{prefix}_{n}"))
}

fn repeated(value: &str, count: u64, sep: &str) -> String {
    (0..count).map(|_| value).collect::<Vec<_>>().join(sep)
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

fn rel(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        ) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{EXPECTED_FAMILIES, generate_family, load_scale_config};
    use std::path::Path;

    #[test]
    fn loads_checked_in_scale_config() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let (config, hash) = load_scale_config(root).unwrap();
        assert_eq!(config.families.len(), EXPECTED_FAMILIES.len());
        assert_eq!(config.values, vec![1, 2, 4, 8, 16, 32, 64]);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn generates_every_family_deterministically() {
        for family in EXPECTED_FAMILIES {
            let first = generate_family(family, 4).unwrap();
            let second = generate_family(family, 4).unwrap();
            assert_eq!(first.contract_name, second.contract_name);
            assert_eq!(first.solidity, second.solidity);
            assert_eq!(first.vyper, second.vyper);
            assert!(!first.scenarios.is_empty());
        }
    }
}

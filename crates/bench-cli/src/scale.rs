use crate::{
    models::{
        Benchmark, BenchmarkSuite, CallSpec, DeploymentVariant, Scenario, ScenarioFile,
        StateAccessProfile,
    },
    util::{ensure_dir, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
};

pub const SCALE_GENERATOR_VERSION: &str = "scale-v2";
const GENERATED_ROOT: &str = "target/bench-generated";
const DISPATCH_SOL_TEMPLATE: &str = include_str!("scale_templates/dispatch.sol");
const DISPATCH_VY_TEMPLATE: &str = include_str!("scale_templates/dispatch.vy");
const STORAGE_SLOTS_SOL_TEMPLATE: &str = include_str!("scale_templates/storage_slots.sol");
const STORAGE_SLOTS_VY_TEMPLATE: &str = include_str!("scale_templates/storage_slots.vy");
const MAPPING_DEPTH_SOL_TEMPLATE: &str = include_str!("scale_templates/mapping_depth.sol");
const MAPPING_DEPTH_VY_TEMPLATE: &str = include_str!("scale_templates/mapping_depth.vy");
const ABI_ARGS_SOL_TEMPLATE: &str = include_str!("scale_templates/abi_args.sol");
const ABI_ARGS_VY_TEMPLATE: &str = include_str!("scale_templates/abi_args.vy");
const LOOP_BOUND_SOL_TEMPLATE: &str = include_str!("scale_templates/loop_bound.sol");
const LOOP_BOUND_VY_TEMPLATE: &str = include_str!("scale_templates/loop_bound.vy");
const EXTERNAL_CALLS_SOL_TEMPLATE: &str = include_str!("scale_templates/external_calls.sol");
const EXTERNAL_CALLS_VY_TEMPLATE: &str = include_str!("scale_templates/external_calls.vy");
const EVENTS_SOL_TEMPLATE: &str = include_str!("scale_templates/events.sol");
const EVENTS_VY_TEMPLATE: &str = include_str!("scale_templates/events.vy");
const DISPATCH_FE_TEMPLATE: &str = include_str!("scale_templates/dispatch.fe");
const STORAGE_SLOTS_FE_TEMPLATE: &str = include_str!("scale_templates/storage_slots.fe");
const MAPPING_DEPTH_FE_TEMPLATE: &str = include_str!("scale_templates/mapping_depth.fe");
const ABI_ARGS_FE_TEMPLATE: &str = include_str!("scale_templates/abi_args.fe");
const LOOP_BOUND_FE_TEMPLATE: &str = include_str!("scale_templates/loop_bound.fe");
const EXTERNAL_CALLS_FE_TEMPLATE: &str = include_str!("scale_templates/external_calls.fe");
const EVENTS_FE_TEMPLATE: &str = include_str!("scale_templates/events.fe");
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
    pub fe_path: String,
    pub fe_hash: String,
    pub spec_path: String,
    pub spec_hash: String,
    pub scenario_path: String,
    pub scenario_hash: String,
}

struct GeneratedSource {
    contract_name: String,
    solidity: String,
    vyper: String,
    fe: String,
    scenarios: Vec<Scenario>,
    abi: Vec<String>,
    semantics: Vec<String>,
}

pub fn generate_scale_suite(root: &Path, only_benchmark: Option<&str>) -> Result<GeneratedSuite> {
    let (config, config_hash) = load_scale_config(root)?;
    let generated_root = root.join(GENERATED_ROOT);
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
            let fe_path = family_dir
                .join("fe")
                .join(format!("{}.fe", generated.contract_name));
            write_file(&solidity_path, &generated.solidity)?;
            write_file(&vyper_path, &generated.vyper)?;
            write_file(&fe_path, &generated.fe)?;

            let spec_path = generated_root
                .join("specs")
                .join(format!("{benchmark_id}.yaml"));
            let scenario_path = generated_root
                .join("scenarios")
                .join(format!("{benchmark_id}.yaml"));
            let solidity_rel = rel(root, &solidity_path)?;
            let vyper_rel = rel(root, &vyper_path)?;
            let fe_rel = rel(root, &fe_path)?;
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
                    "vyper": vyper_rel,
                    "fe": fe_rel
                }
            });
            let spec_text = serde_yaml::to_string(&spec)?;
            write_file(&spec_path, &spec_text)?;

            let solidity_hash = sha256_bytes(generated.solidity.as_bytes());
            let vyper_hash = sha256_bytes(generated.vyper.as_bytes());
            let fe_hash = sha256_bytes(generated.fe.as_bytes());
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
                fe_path: fe_rel.clone(),
                fe_hash,
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
                    fe_path: Some(fe_rel),
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
    let sol_functions = (0..n)
        .map(|i| {
            format!("    function f{i:03}() external pure returns (uint256) {{ return {i}; }}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let vy_functions = (0..n)
        .map(|i| format!("@external\n@pure\ndef f{i:03}() -> uint256:\n    return {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol = render_template(
        DISPATCH_SOL_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("FUNCTIONS", &sol_functions),
        ],
    );
    let vy = render_template(DISPATCH_VY_TEMPLATE, &[("FUNCTIONS", &vy_functions)]);
    let fe_variants = (0..n)
        .map(|i| format!("    #[selector = sol(\"f{i:03}()\")]\n    F{i:03} -> u256,"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe_arms = (0..n)
        .map(|i| format!("        F{i:03} -> u256 {{ {i} }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe = render_template(
        DISPATCH_FE_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("MSG_VARIANTS", &fe_variants),
            ("RECV_ARMS", &fe_arms),
        ],
    );

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
        fe,
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
    let sol_slots = (0..n)
        .map(|i| format!("    uint256 public slot{i:03};"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol_writes = (0..n)
        .map(|i| format!("        slot{i:03} = seed + {i};\n        total += slot{i:03};"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol_reads = (0..n)
        .map(|i| format!("        total += slot{i:03};"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol = render_template(
        STORAGE_SLOTS_SOL_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("SLOTS", &sol_slots),
            ("WRITE_BODY", &sol_writes),
            ("READ_BODY", &sol_reads),
        ],
    );

    let vy_slots = (0..n)
        .map(|i| format!("slot{i:03}: public(uint256)"))
        .collect::<Vec<_>>()
        .join("\n");
    let vy_writes = (0..n)
        .map(|i| format!("    self.slot{i:03} = seed + {i}\n    total += self.slot{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let vy_reads = (0..n)
        .map(|i| format!("    total += self.slot{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let vy = render_template(
        STORAGE_SLOTS_VY_TEMPLATE,
        &[
            ("SLOTS", &vy_slots),
            ("WRITE_BODY", &vy_writes),
            ("READ_BODY", &vy_reads),
        ],
    );

    let fe_slots = (0..n)
        .map(|i| format!("    slot{i:03}: u256,"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe_writes = (0..n)
        .map(|i| {
            format!(
                "            store.slot{i:03} = seed + {i}\n            total += store.slot{i:03}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fe_reads = (0..n)
        .map(|i| format!("            total += store.slot{i:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe = render_template(
        STORAGE_SLOTS_FE_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("SLOTS", &fe_slots),
            ("WRITE_BODY", &fe_writes),
            ("READ_BODY", &fe_reads),
        ],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
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
    let sol_links = (0..n)
        .map(|i| format!("    mapping(uint256 => uint256) public link{i:03};"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol_writes = (0..n)
        .map(|i| {
            format!(
                "        link{i:03}[current] = current + {add};\n        current = link{i:03}[current];",
                add = i + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let sol_reads = (0..n)
        .map(|i| format!("        current = link{i:03}[current];"))
        .collect::<Vec<_>>()
        .join("\n");
    let sol = render_template(
        MAPPING_DEPTH_SOL_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("LINKS", &sol_links),
            ("WRITE_BODY", &sol_writes),
            ("READ_BODY", &sol_reads),
        ],
    );

    let vy_links = (0..n)
        .map(|i| format!("link{i:03}: public(HashMap[uint256, uint256])"))
        .collect::<Vec<_>>()
        .join("\n");
    let vy_writes = (0..n)
        .map(|i| {
            format!(
                "    self.link{i:03}[current] = current + {add}\n    current = self.link{i:03}[current]",
                add = i + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let vy_reads = (0..n)
        .map(|i| format!("    current = self.link{i:03}[current]"))
        .collect::<Vec<_>>()
        .join("\n");
    let vy = render_template(
        MAPPING_DEPTH_VY_TEMPLATE,
        &[
            ("LINKS", &vy_links),
            ("WRITE_BODY", &vy_writes),
            ("READ_BODY", &vy_reads),
        ],
    );

    let fe_links = (0..n)
        .map(|i| format!("    link{i:03}: StorageMap<u256, u256>,"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe_writes = (0..n)
        .map(|i| {
            format!(
                "            store.link{i:03}.set(key: current, value: current + {add})\n            current = store.link{i:03}.get(key: current)",
                add = i + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fe_reads = (0..n)
        .map(|i| format!("            current = store.link{i:03}.get(key: current)"))
        .collect::<Vec<_>>()
        .join("\n");
    let fe = render_template(
        MAPPING_DEPTH_FE_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("LINKS", &fe_links),
            ("WRITE_BODY", &fe_writes),
            ("READ_BODY", &fe_reads),
        ],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
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
    let sol = render_template(
        ABI_ARGS_SOL_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("PARAMS", &sol_params),
            ("SUM", &sol_sum),
        ],
    );

    let params = (0..n)
        .map(|i| format!("a{i:03}: uint256"))
        .collect::<Vec<_>>()
        .join(", ");
    let sum = (0..n)
        .map(|i| format!("a{i:03}"))
        .collect::<Vec<_>>()
        .join(" + ");
    let vy = render_template(ABI_ARGS_VY_TEMPLATE, &[("PARAMS", &params), ("SUM", &sum)]);

    let fe_params = (0..n)
        .map(|i| format!("a{i:03}: u256"))
        .collect::<Vec<_>>()
        .join(", ");
    let fe_fields = (0..n)
        .map(|i| format!("a{i:03}"))
        .collect::<Vec<_>>()
        .join(", ");
    let fe = render_template(
        ABI_ARGS_FE_TEMPLATE,
        &[
            ("CONTRACT_NAME", &contract_name),
            ("SIGNATURE", &signature),
            ("PARAMS", &fe_params),
            ("FIELDS", &fe_fields),
            ("SUM", &sum),
        ],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
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
    let n = n.to_string();
    let sol = render_template(
        LOOP_BOUND_SOL_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );
    let vy = render_template(LOOP_BOUND_VY_TEMPLATE, &[("N", &n)]);
    let fe = render_template(
        LOOP_BOUND_FE_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
        scenarios: vec![simple_scenario("run_loop", "runLoop()")],
        abi: vec!["runLoop() returns (uint256)".to_string()],
        semantics: vec![format!(
            "Runs a statically bounded loop with {n} iterations."
        )],
    }
}

fn external_calls_family(n: u64) -> GeneratedSource {
    let contract_name = contract_name("ExternalCalls", n);
    let n = n.to_string();
    let sol = render_template(
        EXTERNAL_CALLS_SOL_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );
    let vy = render_template(EXTERNAL_CALLS_VY_TEMPLATE, &[("N", &n)]);
    let fe = render_template(
        EXTERNAL_CALLS_FE_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
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
    let n = n.to_string();
    let sol = render_template(
        EVENTS_SOL_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );
    let vy = render_template(EVENTS_VY_TEMPLATE, &[("N", &n)]);
    let fe = render_template(
        EVENTS_FE_TEMPLATE,
        &[("CONTRACT_NAME", &contract_name), ("N", &n)],
    );

    GeneratedSource {
        contract_name,
        solidity: sol,
        vyper: vy,
        fe,
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
        deployment_variant: DeploymentVariant::Standard,
        state_access_profile,
        setup,
        warmup,
        measured,
        expect_success,
        compare_return: true,
        observers,
    }
}

fn call(data: impl Into<String>) -> CallSpec {
    CallSpec {
        data: Some(data.into()),
        data_expr: None,
        function_signature: None,
        args: Vec::new(),
        sender: None,
        value: "0".to_string(),
        destination: crate::models::CallDestination::Target,
    }
}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        rendered = rendered.replace(&format!("{{{{{key}}}}}"), value);
    }
    rendered
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
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .context("generated file path must have a UTF-8 file name")?;
    let temp = path.with_file_name(format!(".{file_name}.{}.tmp", process::id()));
    fs::write(&temp, contents).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("moving {} to {}", temp.display(), path.display()))
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
            assert_eq!(first.fe, second.fe);
            assert!(!first.fe.is_empty());
            assert!(!first.scenarios.is_empty());
        }
    }
}

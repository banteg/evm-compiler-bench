import fs from "node:fs";
import path from "node:path";

const [inputPath, outputPath] = process.argv.slice(2);

if (!inputPath || !outputPath) {
  console.error("usage: node make-design-model.mjs <report-model.json> <output.json>");
  process.exit(2);
}

const model = JSON.parse(fs.readFileSync(inputPath, "utf8"));

function compactGas(gas) {
  if (!gas) return null;
  return {
    calldata_gas: gas.calldata_gas,
    evm_fork: gas.evm_fork,
    harness_call_gas: gas.harness_call_gas,
    harness_estimated_tx_gas: gas.harness_estimated_tx_gas,
    internal_create_gas: gas.internal_create_gas,
    intrinsic_gas: gas.intrinsic_gas,
    measurement_scope: gas.measurement_scope,
    metadata_mode: gas.metadata_mode,
    scenario: gas.scenario,
    state_access_profile: gas.state_access_profile,
  };
}

function compactBytecode(bytecode) {
  if (!bytecode) return null;
  return {
    code_deposit_gas: bytecode.code_deposit_gas,
    creation_bytes: bytecode.creation_bytes,
    creation_bytes_stripped: bytecode.creation_bytes_stripped,
    eip170_margin_bytes: bytecode.eip170_margin_bytes,
    eip3860_margin_bytes: bytecode.eip3860_margin_bytes,
    initcode_bytes: bytecode.initcode_bytes,
    linked_runtime_bytes: bytecode.linked_runtime_bytes,
    runtime_bytes: bytecode.runtime_bytes,
    runtime_bytes_stripped: bytecode.runtime_bytes_stripped,
  };
}

function compactCompile(compile) {
  if (!compile) return null;
  return {
    error: compile.error,
    peak_rss_kib: compile.peak_rss_kib,
    status: compile.status,
    wall_ms_samples: compile.wall_ms_samples,
  };
}

function compactCorrectness(correctness) {
  if (!correctness) return null;
  return {
    baseline_differential_check: correctness.baseline_differential_check,
    golden_behavior_check: correctness.golden_behavior_check,
    log_check: correctness.log_check,
    observer_check: correctness.observer_check,
    profile_behavior_check: correctness.profile_behavior_check,
    property_tests: correctness.property_tests,
    randomized_differential_check: correctness.randomized_differential_check,
    return_data_check: correctness.return_data_check,
    scenario_status_check: correctness.scenario_status_check,
  };
}

function compactProfile(profile) {
  return {
    attempted_artifacts: profile.attempted_artifacts,
    compiler_name: profile.compiler_name,
    compiler_version: profile.compiler_version,
    experimental_codegen: profile.experimental_codegen,
    failed_artifacts: profile.failed_artifacts,
    id: profile.id,
    label: profile.label,
    language: profile.language,
    optimizer: profile.optimizer,
    scenario_rows: profile.scenario_rows,
    successful_artifacts: profile.successful_artifacts,
  };
}

function compactRow(row) {
  return {
    benchmark_id: row.benchmark_id,
    bytecode: compactBytecode(row.bytecode),
    compile: compactCompile(row.compile),
    correctness: compactCorrectness(row.correctness),
    family: row.family,
    gas: compactGas(row.gas),
    implementation_id: row.implementation_id,
    language: row.language,
    parameter_name: row.parameter_name,
    parameter_value: row.parameter_value,
    profile_id: row.profile_id,
    status: row.status,
    suite: row.suite,
  };
}

const output = {
  benchmarks: model.benchmarks,
  defaults: model.defaults,
  design_fixture: {
    generated_at: new Date().toISOString(),
    row_count: model.rows.length,
    source_file: path.basename(inputPath),
    note: "Pruned for design-tool upload limits. Full evidence fields live in results/normalized/report-model.json.",
  },
  generated_at: model.generated_at,
  manifest: model.manifest,
  profiles: model.profiles.map(compactProfile),
  real_derived_models: model.real_derived_models,
  rows: model.rows.map(compactRow),
  schema_version: model.schema_version,
  summary: model.summary,
  toolchains: model.toolchains,
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, JSON.stringify(output));

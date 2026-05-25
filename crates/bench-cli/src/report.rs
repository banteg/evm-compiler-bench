use crate::{
    models::{
        CompileFailure, CompileSet, CompiledArtifact, GasRecord, Language, Provenance,
        ScenarioFile, Toolchains,
    },
    scale::ScaleManifest,
    scenarios::ScenarioCatalog,
    util::ensure_dir,
};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SOL_CODEGEN_BASELINE: &str = "solc-latest-viair-runs10000";
const SOL_VIAIR_CODEGEN: &str = "solc-latest-viair-runs200";
const VYPER_GAS_CODEGEN: &str = "vyper-latest-gas";
const VYPER_GAS_VENOM_CODEGEN: &str = "vyper-latest-gas-venom";
const VYPER_ALPHA_GAS_CODEGEN: &str = "vyper-0.5.0a1-gas";
const SCORECARD_TIE_BAND: f64 = 0.02;

pub struct ReportPaths {
    pub normalized_results: PathBuf,
    pub report_model: PathBuf,
    pub run_manifest: PathBuf,
    pub html_report: PathBuf,
    pub methodology_report: PathBuf,
}

#[derive(Debug, Default)]
struct ReportSummary {
    ok_rows: usize,
    compile_failures: usize,
    profiles: BTreeSet<String>,
    benchmarks: BTreeSet<String>,
    fixed_benchmarks: BTreeSet<String>,
    fixed_scenarios: BTreeSet<String>,
    scale_families: BTreeSet<String>,
    scale_values: BTreeSet<u64>,
    real_benchmarks: BTreeSet<String>,
    real_scenarios: BTreeSet<String>,
    successful_artifacts: BTreeSet<String>,
    failed_artifacts: BTreeSet<String>,
    scenario_status_pass: usize,
    scenario_status_fail: usize,
    baseline_differential_rows: usize,
    randomized_rows: usize,
    property_rows: usize,
    golden_rows: usize,
    log_rows: usize,
}

impl ReportSummary {
    fn from_rows(rows: &[serde_json::Value]) -> Self {
        let mut summary = Self::default();

        for row in rows {
            let status = str_at(row, "/status").unwrap_or_default();
            let suite = str_at(row, "/suite").unwrap_or_default();
            let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
            let profile = str_at(row, "/profile_id").unwrap_or_default();
            if !profile.is_empty() {
                summary.profiles.insert(profile);
            }
            if !benchmark.is_empty() {
                summary.benchmarks.insert(benchmark.clone());
            }

            match suite.as_str() {
                "fixed" => {
                    summary.fixed_benchmarks.insert(benchmark.clone());
                    if let Some(scenario) = str_at(row, "/gas/scenario") {
                        summary
                            .fixed_scenarios
                            .insert(format!("{benchmark}\0{scenario}"));
                    }
                }
                "scale" => {
                    if let Some(family) = str_at(row, "/family") {
                        summary.scale_families.insert(family);
                    }
                    if let Some(value) = row
                        .pointer("/parameter_value")
                        .and_then(|value| value.as_u64())
                    {
                        summary.scale_values.insert(value);
                    }
                }
                "real_derived" => {
                    summary.real_benchmarks.insert(benchmark.clone());
                    if let Some(scenario) = str_at(row, "/gas/scenario") {
                        summary
                            .real_scenarios
                            .insert(format!("{benchmark}\0{scenario}"));
                    }
                }
                _ => {}
            }

            if status == "ok" {
                summary.ok_rows += 1;
                summary
                    .successful_artifacts
                    .insert(artifact_key_from_row(row));
                if str_at(row, "/correctness/scenario_status_check").as_deref() == Some("pass") {
                    summary.scenario_status_pass += 1;
                } else {
                    summary.scenario_status_fail += 1;
                }
                if str_at(row, "/correctness/baseline_differential_check").as_deref()
                    == Some("baseline_only")
                {
                    summary.baseline_differential_rows += 1;
                }
                if str_at(row, "/correctness/randomized_differential_check").as_deref()
                    == Some("pass")
                {
                    summary.randomized_rows += 1;
                }
                if str_at(row, "/correctness/property_tests").as_deref() == Some("pass") {
                    summary.property_rows += 1;
                }
                if !matches!(
                    str_at(row, "/correctness/golden_behavior_check").as_deref(),
                    Some("not_run" | "not_applicable") | None
                ) {
                    summary.golden_rows += 1;
                }
                if !matches!(
                    str_at(row, "/correctness/log_check").as_deref(),
                    Some("not_run" | "not_applicable") | None
                ) {
                    summary.log_rows += 1;
                }
            } else if status == "compile_error" {
                summary.compile_failures += 1;
                summary.failed_artifacts.insert(artifact_key_from_row(row));
            }
        }
        summary
    }

    fn attempted_artifacts(&self) -> usize {
        self.successful_artifacts.len() + self.failed_artifacts.len()
    }
}

pub fn write_outputs(
    root: &Path,
    toolchains: &Toolchains,
    compiled: &CompileSet,
    gas_records: &[GasRecord],
    scenarios: &ScenarioCatalog,
    scale_manifest: &ScaleManifest,
) -> Result<ReportPaths> {
    let normalized_dir = root.join("results/normalized");
    let reports_dir = root.join("results/reports");
    ensure_dir(&normalized_dir)?;
    ensure_dir(&reports_dir)?;

    let rows = normalized_rows(root, compiled, gas_records, scenarios)?;
    let normalized_results = normalized_dir.join("results.json");
    fs::write(&normalized_results, serde_json::to_string_pretty(&rows)?)?;

    let run_manifest = normalized_dir.join("run-manifest.json");
    let manifest = json!({
        "run_id": Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        "started_at": Utc::now(),
        "evm_version": toolchains.evm_version,
        "toolchains": toolchains.compilers.values().collect::<Vec<_>>(),
        "profiles": compiled.profiles,
        "scale_generator": {
            "version": scale_manifest.generator_version.clone(),
            "config_hash": scale_manifest.config_hash.clone(),
            "parameter_name": scale_manifest.parameter_name.clone(),
            "values": scale_manifest.values.clone(),
            "benchmarks": scale_manifest.benchmarks.clone()
        },
        "real_derived": {
            "benchmarks": real_derived_manifest(compiled)
        },
        "environment": environment_manifest(root),
        "artifacts": compiled.artifacts.len(),
        "compile_failures": compiled.failures.len(),
        "gas_records": gas_records.len(),
        "cache": cache_manifest(compiled, gas_records)
    });
    fs::write(&run_manifest, serde_json::to_string_pretty(&manifest)?)?;

    let model = report_model(&rows, toolchains, &manifest);
    let report_model = normalized_dir.join("report-model.json");
    fs::write(&report_model, serde_json::to_string(&model)?)?;

    let html_report = reports_dir.join("index.html");
    let methodology_report = reports_dir.join("methodology.html");
    write_static_report_ui(root, &reports_dir, &model).context(
        "building the HTML report requires report-ui/dist; run `npm --prefix report-ui install` and `npm --prefix report-ui run build`",
    )?;
    fs::write(
        &methodology_report,
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0; url=index.html#methodology\"><title>EVM Compiler Bench Methodology</title><p>Methodology moved to <a href=\"index.html#methodology\">the interactive report</a>.</p>",
    )?;

    Ok(ReportPaths {
        normalized_results,
        report_model,
        run_manifest,
        html_report,
        methodology_report,
    })
}

fn write_static_report_ui(
    root: &Path,
    reports_dir: &Path,
    model: &serde_json::Value,
) -> Result<()> {
    let dist_dir = root.join("report-ui/dist");
    let index = dist_dir.join("index.html");
    if !index.is_file() {
        anyhow::bail!(
            "report-ui/dist/index.html is missing; run `npm --prefix report-ui run build`"
        );
    }
    copy_dir_contents(&dist_dir, reports_dir)?;
    let model_json = serde_json::to_string(model)?;
    fs::write(reports_dir.join("report-model.json"), &model_json)?;
    Ok(())
}

fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    ensure_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if target.exists() {
                fs::remove_dir_all(&target)
                    .with_context(|| format!("removing stale {}", target.display()))?;
            }
            copy_dir_contents(&source, &target)?;
        } else if file_type.is_file() {
            fs::copy(&source, &target).with_context(|| {
                format!(
                    "copying report UI {} to {}",
                    source.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn report_model(
    rows: &[serde_json::Value],
    toolchains: &Toolchains,
    manifest: &serde_json::Value,
) -> serde_json::Value {
    let summary = ReportSummary::from_rows(rows);
    json!({
        "schema_version": 1,
        "generated_at": Utc::now(),
        "defaults": {
            "primary_metric": "harness_call_gas",
            "baseline_profile": SOL_CODEGEN_BASELINE,
            "comparison_profile": VYPER_GAS_CODEGEN,
            "tie_band": SCORECARD_TIE_BAND,
            "production_profiles": {
                "solidity": SOL_CODEGEN_BASELINE,
                "solidity_viair": SOL_VIAIR_CODEGEN,
                "vyper": VYPER_GAS_CODEGEN,
                "vyper_experimental": VYPER_GAS_VENOM_CODEGEN,
                "vyper_alpha": VYPER_ALPHA_GAS_CODEGEN
            }
        },
        "summary": {
            "ok_rows": summary.ok_rows,
            "compile_failures": summary.compile_failures,
            "profiles": summary.profiles.len(),
            "benchmarks": summary.benchmarks.len(),
            "attempted_artifacts": summary.attempted_artifacts(),
            "successful_artifacts": summary.successful_artifacts.len(),
            "failed_artifacts": summary.failed_artifacts.len(),
            "fixed": {
                "benchmarks": summary.fixed_benchmarks.len(),
                "scenarios": summary.fixed_scenarios.len()
            },
            "scale": {
                "families": summary.scale_families.len(),
                "values": summary.scale_values.iter().copied().collect::<Vec<_>>()
            },
            "real_derived": {
                "benchmarks": summary.real_benchmarks.len(),
                "scenarios": summary.real_scenarios.len()
            },
            "correctness": {
                "scenario_status_pass": summary.scenario_status_pass,
                "scenario_status_fail": summary.scenario_status_fail,
                "baseline_differential_rows": summary.baseline_differential_rows,
                "randomized_rows": summary.randomized_rows,
                "property_rows": summary.property_rows,
                "golden_rows": summary.golden_rows,
                "log_rows": summary.log_rows
            }
        },
        "toolchains": toolchains.compilers.values().collect::<Vec<_>>(),
        "manifest": manifest,
        "profiles": report_profiles(rows),
        "benchmarks": report_benchmarks(rows),
        "real_derived_models": report_real_models(rows),
        "rows": rows
    })
}

fn report_profiles(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut profiles = BTreeMap::<String, ProfileReportSummary>::new();
    for row in rows {
        let Some(profile_id) = str_at(row, "/profile_id") else {
            continue;
        };
        let entry = profiles
            .entry(profile_id.clone())
            .or_insert_with(|| ProfileReportSummary::from_row(&profile_id, row));
        let key = artifact_key_from_row(row);
        entry.attempted_artifacts.insert(key.clone());
        match str_at(row, "/status").as_deref() {
            Some("ok") => {
                entry.successful_artifacts.insert(key);
                if row.pointer("/gas").is_some_and(|gas| !gas.is_null()) {
                    entry.scenario_rows += 1;
                }
            }
            Some("compile_error") => {
                entry.failed_artifacts.insert(key);
            }
            _ => {}
        }
    }
    profiles
        .into_values()
        .map(ProfileReportSummary::into_value)
        .collect()
}

#[derive(Default)]
struct ProfileReportSummary {
    id: String,
    label: String,
    language: String,
    compiler_name: String,
    compiler_version: String,
    evm_version: String,
    metadata_mode: String,
    optimizer: String,
    experimental_codegen: bool,
    source_variant: String,
    attempted_artifacts: BTreeSet<String>,
    successful_artifacts: BTreeSet<String>,
    failed_artifacts: BTreeSet<String>,
    scenario_rows: usize,
}

impl ProfileReportSummary {
    fn from_row(profile_id: &str, row: &serde_json::Value) -> Self {
        let settings = row.pointer("/compiler/settings");
        Self {
            id: profile_id.to_string(),
            label: profile_label(row),
            language: str_at(row, "/language").unwrap_or_default(),
            compiler_name: str_at(row, "/compiler/name").unwrap_or_default(),
            compiler_version: str_at(row, "/compiler/version").unwrap_or_default(),
            evm_version: settings
                .and_then(|settings| settings.get("evmVersion"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string(),
            metadata_mode: settings
                .and_then(|settings| settings.get("metadataMode"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string(),
            optimizer: profile_optimizer_label(row),
            experimental_codegen: settings
                .and_then(|settings| settings.get("experimentalCodegen"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            source_variant: settings
                .and_then(|settings| settings.get("sourceVariant"))
                .and_then(|value| value.as_str())
                .unwrap_or("default")
                .to_string(),
            ..Self::default()
        }
    }

    fn into_value(self) -> serde_json::Value {
        json!({
            "id": self.id,
            "label": self.label,
            "language": self.language,
            "compiler_name": self.compiler_name,
            "compiler_version": self.compiler_version,
            "evm_version": self.evm_version,
            "metadata_mode": self.metadata_mode,
            "optimizer": self.optimizer,
            "experimental_codegen": self.experimental_codegen,
            "source_variant": self.source_variant,
            "attempted_artifacts": self.attempted_artifacts.len(),
            "successful_artifacts": self.successful_artifacts.len(),
            "failed_artifacts": self.failed_artifacts.len(),
            "scenario_rows": self.scenario_rows
        })
    }
}

fn profile_optimizer_label(row: &serde_json::Value) -> String {
    let settings = row.pointer("/compiler/settings");
    if settings
        .and_then(|settings| settings.get("viaIR"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return "viaIR".to_string();
    }
    if let Some(mode) = settings
        .and_then(|settings| settings.get("optimize"))
        .and_then(|value| value.as_str())
    {
        return mode.to_string();
    }
    if settings
        .and_then(|settings| settings.get("optimizer"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return "legacy optimizer".to_string();
    }
    "none".to_string()
}

fn report_benchmarks(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut benchmarks = BTreeMap::<String, BenchmarkReportSummary>::new();
    for row in rows {
        let Some(benchmark_id) = str_at(row, "/benchmark_id") else {
            continue;
        };
        let entry = benchmarks
            .entry(benchmark_id.clone())
            .or_insert_with(|| BenchmarkReportSummary::from_row(&benchmark_id, row));
        if let Some(scenario) = str_at(row, "/gas/scenario") {
            entry.scenarios.insert(scenario);
        }
        if let Some(profile) = str_at(row, "/profile_id") {
            entry.profiles.insert(profile);
        }
        let key = artifact_key_from_row(row);
        entry.artifacts.insert(key);
        if str_at(row, "/status").as_deref() == Some("compile_error") {
            entry.compile_failures += 1;
        }
    }
    benchmarks
        .into_values()
        .map(BenchmarkReportSummary::into_value)
        .collect()
}

struct BenchmarkReportSummary {
    id: String,
    suite: String,
    family: Option<String>,
    parameter_name: Option<String>,
    parameter_value: Option<u64>,
    scenarios: BTreeSet<String>,
    profiles: BTreeSet<String>,
    artifacts: BTreeSet<String>,
    compile_failures: usize,
}

impl BenchmarkReportSummary {
    fn from_row(benchmark_id: &str, row: &serde_json::Value) -> Self {
        Self {
            id: benchmark_id.to_string(),
            suite: str_at(row, "/suite").unwrap_or_default(),
            family: str_at(row, "/family"),
            parameter_name: str_at(row, "/parameter_name"),
            parameter_value: row
                .pointer("/parameter_value")
                .and_then(|value| value.as_u64()),
            scenarios: BTreeSet::new(),
            profiles: BTreeSet::new(),
            artifacts: BTreeSet::new(),
            compile_failures: 0,
        }
    }

    fn into_value(self) -> serde_json::Value {
        json!({
            "id": self.id,
            "suite": self.suite,
            "family": self.family,
            "parameter_name": self.parameter_name,
            "parameter_value": self.parameter_value,
            "scenarios": self.scenarios.into_iter().collect::<Vec<_>>(),
            "profiles": self.profiles.into_iter().collect::<Vec<_>>(),
            "artifacts": self.artifacts.len(),
            "compile_failures": self.compile_failures
        })
    }
}

fn report_real_models(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut models = BTreeMap::<String, serde_json::Value>::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some("real_derived") {
            continue;
        }
        let Some(benchmark_id) = str_at(row, "/benchmark_id") else {
            continue;
        };
        if !models.contains_key(&benchmark_id) {
            models.insert(
                benchmark_id.clone(),
                json!({
                    "benchmark_id": benchmark_id,
                    "provenance": row.pointer("/provenance").cloned().unwrap_or(serde_json::Value::Null)
                }),
            );
        }
    }
    models.into_values().collect()
}

fn cache_manifest(compiled: &CompileSet, gas_records: &[GasRecord]) -> serde_json::Value {
    let mut compile = BTreeMap::<String, usize>::new();
    let mut gas = BTreeMap::<String, usize>::new();
    for artifact in &compiled.artifacts {
        *compile.entry(artifact.cache.status.clone()).or_default() += 1;
    }
    for failure in &compiled.failures {
        *compile.entry(failure.cache.status.clone()).or_default() += 1;
    }
    for record in gas_records {
        *gas.entry(record.cache.status.clone()).or_default() += 1;
    }
    json!({
        "enabled": !compile.contains_key("disabled") || compile.len() > 1 || !gas.contains_key("disabled") || gas.len() > 1,
        "root": ".cache/bench-cli",
        "compile": compile,
        "gas": gas,
        "statuses": {
            "hit": "entry was reused from cache",
            "miss": "no prior entry matched this logical benchmark/config",
            "stale": "a prior entry existed but one or more fingerprint fields changed",
            "refreshed": "a valid gas cache entry existed but the Foundry batch was rerun because another row missed",
            "disabled": "cache was bypassed for this run"
        }
    })
}

fn normalized_rows(
    root: &Path,
    compiled: &CompileSet,
    gas_records: &[GasRecord],
    scenarios: &ScenarioCatalog,
) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = BTreeMap::new();
    for artifact in &compiled.artifacts {
        artifacts.insert(
            artifact_key(
                &artifact.benchmark_id,
                &artifact.implementation_id,
                &artifact.profile_id,
            ),
            artifact,
        );
    }

    let failure_links = failure_links_by_benchmark(root)?;
    let differential_benchmarks = differential_benchmarks(compiled);
    let mut rows = Vec::with_capacity(gas_records.len() + compiled.failures.len());
    for gas in gas_records {
        let artifact = artifacts
            .get(&artifact_key(
                &gas.benchmark_id,
                &gas.implementation_id,
                &gas.profile_id,
            ))
            .with_context(|| {
                format!(
                    "missing artifact for {}/{}/{}",
                    gas.benchmark_id, gas.implementation_id, gas.profile_id
                )
            })?;
        let scenario_file = scenarios.get(&artifact.benchmark_id)?;
        let failures = failure_links
            .get(&artifact.benchmark_id)
            .cloned()
            .unwrap_or_default();
        rows.push(row(
            artifact,
            gas,
            scenario_file,
            failures,
            differential_benchmarks.contains(&artifact.benchmark_id),
        ));
    }
    for failure in &compiled.failures {
        rows.push(failure_row(failure));
    }
    rows.sort_by(|a, b| {
        let left = sort_key(a);
        let right = sort_key(b);
        left.cmp(&right)
    });
    Ok(rows)
}

fn row(
    artifact: &CompiledArtifact,
    gas: &GasRecord,
    scenario_file: &ScenarioFile,
    failure_links: Vec<String>,
    differential_available: bool,
) -> serde_json::Value {
    let scenario = scenario_file
        .scenarios
        .iter()
        .find(|scenario| scenario.name == gas.scenario);
    let has_observers = scenario.is_some_and(|scenario| !scenario.observers.is_empty());
    let baseline_status = if differential_available {
        "baseline_only"
    } else {
        "not_applicable"
    };
    let randomized_status = correctness_status(
        scenario_file.randomized.is_some(),
        &failure_links,
        "randomized_differential",
    );
    let property_status = correctness_status(
        !scenario_file.properties.is_empty(),
        &failure_links,
        "property",
    );
    json!({
        "status": "ok",
        "benchmark_id": gas.benchmark_id,
        "implementation_id": gas.implementation_id,
        "profile_id": artifact.profile_id,
        "suite": artifact.suite.as_str(),
        "family": artifact.family.clone(),
        "parameter_name": artifact.parameter_name.clone(),
        "parameter_value": artifact.parameter_value,
        "generated": {
            "generator_version": artifact.generator_version.clone(),
            "scenario_path": artifact.scenario_path.clone(),
            "scenario_hash": artifact.scenario_hash.clone()
        },
        "provenance": provenance_value(
            artifact.provenance.as_ref(),
            artifact.language,
            &artifact.implementation_id
        ),
        "language": artifact.language.as_str(),
        "compiler": {
            "name": artifact.compiler.name,
            "version": artifact.compiler.version,
            "binary_path": artifact.compiler.binary_path,
            "binary_sha256": artifact.compiler.binary_sha256,
            "download_source": artifact.compiler.download_source,
            "metadata": artifact.compiler.metadata,
            "settings": artifact.compiler_settings
        },
        "source_hash": artifact.source_hash,
        "cache": {
            "compile": artifact.cache,
            "gas": gas.cache
        },
        "compile": {
            "status": "ok",
            "wall_ms_samples": artifact.compile.wall_ms_samples.clone(),
            "cpu_ms_samples": artifact.compile.cpu_ms_samples.clone(),
            "peak_rss_kib": artifact.compile.peak_rss_kib
        },
        "bytecode": artifact.bytecode,
        "gas": {
            "scenario": gas.scenario,
            "evm_fork": artifact.compiler_settings.get("evmVersion").cloned().unwrap_or_else(|| json!("unknown")),
            "state_access_profile": gas.state_access_profile.as_str(),
            "metadata_mode": gas.metadata_mode.as_str(),
            "internal_create_gas": gas.internal_create_gas,
            "harness_call_gas": gas.harness_call_gas,
            "intrinsic_gas": gas.intrinsic_gas,
            "calldata_gas": gas.calldata_gas,
            "harness_estimated_tx_gas": gas.harness_estimated_tx_gas,
            "total_tx_gas": null,
            "expected_success": gas.expected_success,
            "call_succeeded": gas.call_succeeded,
            "scenario_status_ok": gas.scenario_status_ok,
            "measurement_scope": "foundry_internal_call_harness"
        },
        "correctness": {
            "scenario_status_check": if gas.scenario_status_ok { "pass" } else { "fail" },
            "golden_behavior_check": "not_run",
            "baseline_differential_check": baseline_status,
            "profile_behavior_check": "not_run",
            "observer_check": if has_observers { baseline_status } else { "not_applicable" },
            "return_data_check": baseline_status,
            "log_check": "not_run",
            "randomized_differential_check": randomized_status,
            "property_tests": property_status,
            "properties": scenario_file.properties.iter().map(|property| property.name.clone()).collect::<Vec<_>>(),
            "failure_artifacts": failure_links,
            "scenario_status_ok": gas.scenario_status_ok
        }
    })
}

fn differential_benchmarks(compiled: &CompileSet) -> BTreeSet<String> {
    let mut solidity = BTreeSet::new();
    let mut vyper = BTreeSet::new();
    for artifact in &compiled.artifacts {
        match artifact.profile_id.as_str() {
            SOL_CODEGEN_BASELINE => {
                solidity.insert(artifact.benchmark_id.clone());
            }
            VYPER_GAS_CODEGEN => {
                vyper.insert(artifact.benchmark_id.clone());
            }
            _ => {}
        }
    }
    solidity.intersection(&vyper).cloned().collect()
}

fn failure_row(failure: &CompileFailure) -> serde_json::Value {
    json!({
        "status": "compile_error",
        "benchmark_id": failure.benchmark_id,
        "implementation_id": failure.implementation_id,
        "profile_id": failure.profile_id,
        "suite": failure.suite.as_str(),
        "family": failure.family.clone(),
        "parameter_name": failure.parameter_name.clone(),
        "parameter_value": failure.parameter_value,
        "generated": {
            "generator_version": failure.generator_version.clone(),
            "scenario_path": failure.scenario_path.clone(),
            "scenario_hash": failure.scenario_hash.clone()
        },
        "provenance": provenance_value(
            failure.provenance.as_ref(),
            failure.language,
            &failure.implementation_id
        ),
        "language": failure.language.as_str(),
        "compiler": {
            "name": failure.compiler.name,
            "version": failure.compiler.version,
            "binary_path": failure.compiler.binary_path,
            "binary_sha256": failure.compiler.binary_sha256,
            "download_source": failure.compiler.download_source,
            "metadata": failure.compiler.metadata,
            "settings": failure.compiler_settings
        },
        "source_hash": failure.source_hash,
        "cache": {
            "compile": failure.cache,
            "gas": null
        },
        "compile": {
            "status": "error",
            "error": failure.error
        },
        "bytecode": null,
        "gas": null,
        "correctness": {
            "scenario_status_check": "not_applicable",
            "golden_behavior_check": "not_applicable",
            "baseline_differential_check": "not_applicable",
            "profile_behavior_check": "not_applicable",
            "observer_check": "not_applicable",
            "return_data_check": "not_applicable",
            "log_check": "not_applicable",
            "randomized_differential_check": "not_applicable",
            "property_tests": "not_applicable",
            "properties": [],
            "failure_artifacts": [],
            "scenario_status_ok": false
        }
    })
}

fn real_derived_manifest(compiled: &CompileSet) -> Vec<serde_json::Value> {
    let mut benchmarks = BTreeMap::new();
    for artifact in &compiled.artifacts {
        if let Some(provenance) = &artifact.provenance {
            benchmarks
                .entry(artifact.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&artifact.benchmark_id, provenance));
        }
    }
    for failure in &compiled.failures {
        if let Some(provenance) = &failure.provenance {
            benchmarks
                .entry(failure.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&failure.benchmark_id, provenance));
        }
    }
    benchmarks.into_values().collect()
}

fn provenance_manifest_value(benchmark_id: &str, provenance: &Provenance) -> serde_json::Value {
    json!({
        "benchmark_id": benchmark_id,
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_language": provenance.source_language.as_str(),
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "production_equivalence": provenance.production_equivalence,
        "api_compatibility": &provenance.api_compatibility,
        "storage_layout_compatibility": provenance.storage_layout_compatibility,
        "external_token_semantics": &provenance.external_token_semantics,
        "source_derivation": &provenance.source_derivation,
        "equivalence_scope": &provenance.equivalence_scope,
        "scenario_coverage": &provenance.scenario_coverage,
        "mock_assumptions": &provenance.mock_assumptions,
        "included_features": &provenance.included_features,
        "excluded_features": &provenance.excluded_features,
    })
}

fn provenance_value(
    provenance: Option<&Provenance>,
    port_language: Language,
    port_version: &str,
) -> serde_json::Value {
    let Some(provenance) = provenance else {
        return serde_json::Value::Null;
    };
    json!({
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_language": provenance.source_language.as_str(),
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "production_equivalence": provenance.production_equivalence,
        "api_compatibility": &provenance.api_compatibility,
        "storage_layout_compatibility": provenance.storage_layout_compatibility,
        "external_token_semantics": &provenance.external_token_semantics,
        "source_derivation": &provenance.source_derivation,
        "port_language": port_language.as_str(),
        "port_version": port_version,
        "equivalence_scope": &provenance.equivalence_scope,
        "scenario_coverage": &provenance.scenario_coverage,
        "mock_assumptions": &provenance.mock_assumptions,
        "included_features": &provenance.included_features,
        "excluded_features": &provenance.excluded_features,
    })
}

fn environment_manifest(root: &Path) -> serde_json::Value {
    json!({
        "os": env::consts::OS,
        "arch": env::consts::ARCH,
        "family": env::consts::FAMILY,
        "command_line": env::args().collect::<Vec<_>>(),
        "git": {
            "commit": command_output(root, "git", &["rev-parse", "HEAD"]),
            "dirty": command_output(root, "git", &["status", "--porcelain"]).is_some_and(|output| !output.is_empty())
        },
        "tools": {
            "forge": command_output(root, "forge", &["--version"]),
            "cargo": command_output(root, "cargo", &["--version"]),
            "rustc": command_output(root, "rustc", &["--version"]),
            "uv": command_output(root, "uv", &["--version"])
        },
        "host": {
            "kernel": command_output(root, "uname", &["-a"]),
            "cpu": cpu_model(root)
        }
    })
}

fn cpu_model(root: &Path) -> Option<String> {
    command_output(root, "sysctl", &["-n", "machdep.cpu.brand_string"]).or_else(|| {
        fs::read_to_string("/proc/cpuinfo").ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.split_once(':'))
                .and_then(|(key, value)| {
                    if key.trim() == "model name" {
                        Some(value.trim().to_string())
                    } else {
                        None
                    }
                })
        })
    })
}

fn command_output(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn correctness_status(applicable: bool, failure_links: &[String], kind: &str) -> &'static str {
    if !applicable {
        return "not_applicable";
    }
    if failure_links.iter().any(|link| link.contains(kind)) {
        "fail"
    } else {
        "pass"
    }
}

fn failure_links_by_benchmark(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let failure_dir = root.join("results/raw/failures");
    if !failure_dir.exists() {
        return Ok(links);
    }
    for entry in fs::read_dir(&failure_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((benchmark_id, _)) = file_name
            .split_once("-randomized_differential-")
            .or_else(|| file_name.split_once("-property-"))
        else {
            continue;
        };
        links
            .entry(benchmark_id.to_string())
            .or_default()
            .push(format!("results/raw/failures/{file_name}"));
    }
    for benchmark_links in links.values_mut() {
        benchmark_links.sort();
    }
    Ok(links)
}

fn profile_label(row: &serde_json::Value) -> String {
    let compiler = str_at(row, "/compiler/name")
        .or_else(|| str_at(row, "/language"))
        .unwrap_or_else(|| "compiler".to_string());
    let version = str_at(row, "/compiler/version").unwrap_or_else(|| "unknown".to_string());
    let optimizer = match profile_optimizer_label(row).as_str() {
        "legacy optimizer" => "legacy".to_string(),
        other => other.to_string(),
    };
    let venom = row
        .pointer("/compiler/settings/experimentalCodegen")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        .then_some(" + Venom")
        .unwrap_or("");
    format!("{compiler} {version} {optimizer}{venom}")
}

fn artifact_key_from_row(row: &serde_json::Value) -> String {
    artifact_key(
        &str_at(row, "/benchmark_id").unwrap_or_default(),
        &str_at(row, "/implementation_id").unwrap_or_default(),
        &str_at(row, "/profile_id").unwrap_or_default(),
    )
}

fn artifact_key(benchmark_id: &str, implementation_id: &str, profile_id: &str) -> String {
    format!("{benchmark_id}\0{implementation_id}\0{profile_id}")
}

fn sort_key(row: &serde_json::Value) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        str_at(row, "/benchmark_id").unwrap_or_default(),
        str_at(row, "/implementation_id").unwrap_or_default(),
        str_at(row, "/profile_id").unwrap_or_default(),
        str_at(row, "/gas/scenario").unwrap_or_default()
    )
}

fn str_at(row: &serde_json::Value, pointer: &str) -> Option<String> {
    row.pointer(pointer).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

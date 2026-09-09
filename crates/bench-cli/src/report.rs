use crate::{
    models::{
        CompileFailure, CompileSet, CompiledArtifact, CompilerProfile, GasRecord, Language,
        Provenance, Scenario, ScenarioFile, Toolchains,
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

const SOL_CODEGEN_BASELINE: &str = "solc-latest-legacy-runs200";
const SOL_VIAIR_CODEGEN: &str = "solc-latest-viair-runs200";
const VYPER_GAS_CODEGEN: &str = "vyper-latest-gas";
const FE_CODEGEN_BASELINE: &str = "fe-latest-O2";
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
                if str_at(row, "/correctness/baseline_differential_check")
                    .as_deref()
                    .is_some_and(|status| matches!(status, "pass" | "baseline_only"))
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

    let rows = normalized_rows(
        root,
        compiled,
        gas_records,
        scenarios,
        &toolchains.evm_version,
    )?;
    let normalized_results = normalized_dir.join("results.json");
    fs::write(&normalized_results, serde_json::to_string_pretty(&rows)?)?;

    let run_manifest = normalized_dir.join("run-manifest.json");
    let manifest = json!({
        "run_id": Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        "started_at": Utc::now(),
        "evm_version": toolchains.evm_version,
        "toolchains": toolchains.compilers.values().collect::<Vec<_>>(),
        "profiles": manifest_profiles(&compiled.profiles),
        "scale_generator": {
            "version": scale_manifest.generator_version.clone(),
            "config_hash": scale_manifest.config_hash.clone(),
            "parameter_name": scale_manifest.parameter_name.clone(),
            "values": scale_manifest.values.clone(),
            "benchmarks": scale_manifest.benchmarks.clone()
        },
        "real_derived": {
            "benchmarks": real_derived_manifest(root, compiled)
        },
        "environment": environment_manifest(root),
        "artifacts": compiled.artifacts.len(),
        "compile_failures": compiled.failures.len(),
        "gas_records": gas_records.len(),
        "behavior_checks": crate::behavior::read(root)?,
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

fn manifest_profiles(profiles: &[CompilerProfile]) -> Vec<serde_json::Value> {
    profiles
        .iter()
        .map(|profile| {
            let mut value = serde_json::to_value(profile)
                .expect("serializing compiler profile to manifest value must not fail");
            if value
                .get("source_variant")
                .is_none_or(|source_variant| source_variant.is_null())
            {
                value["source_variant"] = json!("latest");
            }
            value
        })
        .collect()
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
        "schema_version": 2,
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
        "methodology": report_methodology(),
        "profiles": report_profiles(rows),
        "benchmarks": report_benchmarks(rows),
        "real_derived_models": report_real_models(rows),
        "rows": rows
    })
}

fn report_methodology() -> serde_json::Value {
    json!({
        "source_model": {
            "real_derived": "Production-conformance rows use latest-syntax source-language originals plus counterpart-language ports. Pinned upstream files are provenance references, not compiled headline artifacts.",
            "compatibility_variants": "Older source-language profiles compile generated variants of the checked-in latest source. Version pragmas are rewritten to the resolved compiler patch range before supported backward syntax rewrites are applied.",
            "compiled_source_root": "target/bench-source-variants/<profile_id>/"
        },
        "notes": [
            {
                "tag": "A",
                "title": "Foundry internal-call harness gas",
                "body": "Gas is measured via Foundry's internal-call harness. That isolates compiler-generated runtime costs from intrinsic and calldata overhead."
            },
            {
                "tag": "B",
                "title": "Stripped runtime bytes",
                "body": "Bytecode comparisons use runtime bytecode with appended metadata stripped, so trailing CBOR does not skew code-size deltas."
            },
            {
                "tag": "C",
                "title": "Idiomatic source comparison",
                "body": "Headline results compare fixed and scale-suite idiomatic high-level source for each language. Solidity storage packing and Vyper dispatch codegen count as language-native behavior; hand-written assembly and mechanically matched ports belong in diagnostic lanes."
            },
            {
                "tag": "D",
                "title": "Metric-aware geomeans",
                "body": "Runtime gas is aggregated over matched headline scenarios. Current harness deployment gas is scenario/deployment-variant scoped; bytecode size and compile time are deduplicated per benchmark artifact before computing ratios."
            },
            {
                "tag": "E",
                "title": "Metric-specific bands",
                "body": "Gas and bytecode use a +/-0.5% materiality band for W/T/L counts. Compile time uses a +/-2% noise band."
            },
            {
                "tag": "F",
                "title": "Real-derived provenance",
                "body": "Real-derived suites separate benchmark lanes from source lanes. Production-conformance rows use latest-syntax originals plus counterpart-language ports; pinned historical sources remain provenance references, not compiled headline artifacts."
            },
            {
                "tag": "G",
                "title": "Trimmed diagnostic rows",
                "body": "Malformed calldata, decoder-boundary, admin/auth reject, and other adversarial revert rows are excluded from the measured scenario corpus so real-derived results focus on common workflow paths."
            },
            {
                "tag": "H",
                "title": "Compatibility source variants",
                "body": "Older source-language profiles compile generated variants of the checked-in latest source. Version pragmas are rewritten to the resolved compiler patch range, then only supported backward syntax rewrites are applied."
            },
            {
                "tag": "I",
                "title": "Cross-profile behavior checks",
                "body": "Gas rows persist return-data, observer-state, and normalized-log hashes. Report rows compare those hashes against the language baseline profile when both profiles compiled the same scenario; expected-revert rows are compared by status and observer state, not raw revert bytes."
            },
            {
                "tag": "J",
                "title": "Vyper Venom and 0.5.0a1",
                "body": "Vyper Venom rows pass --experimental-codegen. Vyper 0.5.0a1 is pre-release."
            },
            {
                "tag": "K",
                "title": "solx and the Solidity frontend",
                "body": "solx 0.1.8 uses an LLVM backend and embeds a modified solc 0.8.34 frontend. Release, frontend commit, and LLVM build are recorded separately. O3 and Oz use one compiler worker with automatic size fallback disabled. Matched solc 0.8.34 profiles compile the same materialized source; comparisons against latest solc also change the frontend version. LLVM optimizer modes do not map to solc optimizer-runs values."
            },
            {
                "tag": "L",
                "title": "Evidence for behavioral tests",
                "body": "Differential, randomized, and property-test credit applies only to the compiler pairs listed in the run manifest. Evidence is cached against bytecode, scenario, harness source, and Foundry version. Other profiles show not_run even if another profile passed that benchmark's tests. Per-scenario return, state, and log hashes remain separate checks across profiles."
            },
            {
                "tag": "M",
                "title": "Correctness exclusions",
                "body": "An observed scenario or behavior-check failure excludes the entire benchmark/profile artifact from interactive performance comparisons, including size and compile-time rankings. The reliability panel lists those failures, while raw outputs preserve every measured value and correctness status for diagnosis. Compilation coverage remains a separate statistic."
            },
            {
                "tag": "N",
                "title": "Solar source build and optimizer modes",
                "body": "Solar is pinned at 716e9cbcde88165f931173f1c1fda852ed63afa0, newer than release v0.2.0. It has its own Rust frontend and EVM code generator. Package version, source revision, Solidity compatibility (0.8.36), Rust build target, and binary hash are separate identities. Standard JSON optimizer enabled with runs 200 selects gas mode; runs 1 selects size mode. Both use one worker and the shared EVM target, compared with solc 0.8.36 viaIR / runs 200 on identical materialized sources. Main-matrix compile timings use each binary as resolved on the recorded host; the spike separately controls x86-64 execution for both compilers."
            }
        ]
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
    solidity_version: Option<String>,
    source_revision: Option<String>,
    frontend_version: Option<String>,
    frontend_commit: Option<String>,
    llvm_build: Option<String>,
    optimizer_runs: Option<u64>,
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
            solidity_version: str_at(row, "/compiler/metadata/solidity_version"),
            source_revision: str_at(row, "/compiler/metadata/source_revision"),
            frontend_version: str_at(row, "/compiler/metadata/frontend_version"),
            frontend_commit: str_at(row, "/compiler/metadata/frontend_commit"),
            llvm_build: str_at(row, "/compiler/metadata/llvm_build"),
            optimizer_runs: row
                .pointer("/compiler/settings/optimizerRuns")
                .and_then(|v| v.as_u64()),
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
            "solidity_version": self.solidity_version,
            "source_revision": self.source_revision,
            "frontend_version": self.frontend_version,
            "frontend_commit": self.frontend_commit,
            "llvm_build": self.llvm_build,
            "optimizer_runs": self.optimizer_runs,
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
    let mut seen_sources = BTreeSet::<String>::new();
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
                    "provenance": row.pointer("/provenance").cloned().unwrap_or(serde_json::Value::Null),
                    "compiled_sources": []
                }),
            );
        }
        let source_path = str_at(row, "/source_path").unwrap_or_default();
        let source_hash = str_at(row, "/source_hash").unwrap_or_default();
        if source_path.is_empty() || source_hash.is_empty() {
            continue;
        }
        let language = str_at(row, "/language").unwrap_or_default();
        let implementation_id = str_at(row, "/implementation_id").unwrap_or_default();
        let profile_id = str_at(row, "/profile_id").unwrap_or_default();
        let source_variant = row
            .pointer("/compiler/settings/sourceVariant")
            .and_then(|value| value.as_str())
            .unwrap_or("default");
        let key = format!(
            "{benchmark_id}\0{language}\0{implementation_id}\0{profile_id}\0{source_variant}\0{source_path}\0{source_hash}"
        );
        if !seen_sources.insert(key) {
            continue;
        }
        if let Some(sources) = models
            .get_mut(&benchmark_id)
            .and_then(|model| model.get_mut("compiled_sources"))
            .and_then(|sources| sources.as_array_mut())
        {
            sources.push(json!({
                "language": language,
                "profile_id": profile_id,
                "implementation_id": implementation_id,
                "source_variant": source_variant,
                "source_path": source_path,
                "source_hash": source_hash
            }));
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
    harness_evm_version: &str,
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
    let behavior_evidence = crate::behavior::read(root)?;
    let same_source_baselines: BTreeMap<_, _> =
        crate::baselines::comparison_pairs(&compiled.artifacts)
            .into_iter()
            .filter(|(_, _, candidate)| {
                matches!(
                    compiled.artifacts[*candidate].compiler.name.as_str(),
                    "solx" | "solar"
                )
            })
            .map(|(_, baseline, candidate)| {
                (
                    (
                        compiled.artifacts[candidate].benchmark_id.clone(),
                        compiled.artifacts[candidate].profile_id.clone(),
                    ),
                    &compiled.artifacts[baseline],
                )
            })
            .collect();
    let gas_by_artifact_scenario: BTreeMap<_, _> = gas_records
        .iter()
        .map(|gas| {
            (
                (
                    gas.benchmark_id.as_str(),
                    gas.profile_id.as_str(),
                    gas.scenario.as_str(),
                    gas.state_access_profile.as_str(),
                ),
                gas,
            )
        })
        .collect();
    let profile_behavior_baselines = profile_behavior_baselines(gas_records, &artifacts);
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
            root,
            artifact,
            gas,
            scenario_file,
            failures,
            harness_evm_version,
            &behavior_evidence,
            profile_behavior_check(
                gas,
                if matches!(artifact.compiler.name.as_str(), "solx" | "solar") {
                    same_source_baselines
                        .get(&(artifact.benchmark_id.clone(), artifact.profile_id.clone()))
                        .and_then(|baseline| {
                            gas_by_artifact_scenario
                                .get(&(
                                    gas.benchmark_id.as_str(),
                                    baseline.profile_id.as_str(),
                                    gas.scenario.as_str(),
                                    gas.state_access_profile.as_str(),
                                ))
                                .copied()
                        })
                } else {
                    profile_behavior_baselines
                        .get(&profile_behavior_key(gas))
                        .copied()
                },
                scenario_file
                    .scenarios
                    .iter()
                    .find(|scenario| scenario.name == gas.scenario),
                crate::harness::supports_log_diff(&artifact.benchmark_id),
            ),
        ));
    }
    for failure in &compiled.failures {
        rows.push(failure_row(root, failure));
    }
    rows.sort_by(|a, b| {
        let left = sort_key(a);
        let right = sort_key(b);
        left.cmp(&right)
    });
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
fn row(
    root: &Path,
    artifact: &CompiledArtifact,
    gas: &GasRecord,
    scenario_file: &ScenarioFile,
    failure_links: Vec<String>,
    harness_evm_version: &str,
    behavior_evidence: &[crate::behavior::Evidence],
    profile_behavior: ProfileBehaviorCheck,
) -> serde_json::Value {
    let evidence: Vec<_> = behavior_evidence
        .iter()
        .filter(|e| e.covers(artifact))
        .collect();
    let baseline_status = if !evidence.is_empty() {
        "pass"
    } else {
        "not_run"
    };
    let randomized_status = if scenario_file.randomized.is_none() {
        "not_applicable"
    } else if evidence.iter().any(|e| e.randomized) {
        "pass"
    } else {
        "not_run"
    };
    let property_status = if scenario_file.properties.is_empty() {
        "not_applicable"
    } else if evidence.iter().any(|e| {
        scenario_file
            .properties
            .iter()
            .all(|p| e.properties.contains(&p.name))
    }) {
        "pass"
    } else {
        "not_run"
    };
    let deployment_variant = scenario_file
        .scenarios
        .iter()
        .find(|scenario| scenario.name == gas.scenario)
        .map(|scenario| scenario.deployment_variant.as_str())
        .unwrap_or("standard");
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
            &artifact.benchmark_id,
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
        "source_path": report_path(root, &artifact.source_path),
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
            "evm_fork": harness_evm_version,
            "deployment_variant": deployment_variant,
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
        "behavior": {
            "return_hash": gas.return_hash,
            "observer_hash": gas.observer_hash,
            "log_hash": gas.log_hash,
            "profile_baseline": profile_behavior.baseline_profile
        },
        "correctness": {
            "scenario_status_check": if gas.scenario_status_ok { "pass" } else { "fail" },
            "golden_behavior_check": "not_run",
            "baseline_differential_check": baseline_status,
            "behavior_pairs": evidence,
            "profile_behavior_check": profile_behavior.profile_behavior_check,
            "observer_check": profile_behavior.observer_check,
            "return_data_check": profile_behavior.return_data_check,
            "log_check": profile_behavior.log_check,
            "randomized_differential_check": randomized_status,
            "property_tests": property_status,
            "properties": scenario_file.properties.iter().map(|property| property.name.clone()).collect::<Vec<_>>(),
            "failure_artifacts": failure_links,
            "scenario_status_ok": gas.scenario_status_ok
        }
    })
}

#[derive(Debug, Clone)]
struct ProfileBehaviorCheck {
    baseline_profile: Option<String>,
    profile_behavior_check: &'static str,
    return_data_check: &'static str,
    observer_check: &'static str,
    log_check: &'static str,
}

fn profile_behavior_baselines<'a>(
    gas_records: &'a [GasRecord],
    artifacts: &BTreeMap<String, &CompiledArtifact>,
) -> BTreeMap<String, &'a GasRecord> {
    let mut baselines = BTreeMap::new();
    for gas in gas_records {
        let Some(artifact) = artifacts.get(&artifact_key(
            &gas.benchmark_id,
            &gas.implementation_id,
            &gas.profile_id,
        )) else {
            continue;
        };
        let baseline_profile = match artifact.language {
            Language::Solidity => SOL_CODEGEN_BASELINE,
            Language::Vyper => VYPER_GAS_CODEGEN,
            Language::Fe => FE_CODEGEN_BASELINE,
        };
        if gas.profile_id == baseline_profile {
            baselines.insert(profile_behavior_key(gas), gas);
        }
    }
    baselines
}

fn profile_behavior_key(gas: &GasRecord) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}",
        gas.benchmark_id,
        gas.implementation_id,
        gas.scenario,
        gas.state_access_profile.as_str(),
        gas.metadata_mode.as_str()
    )
}

fn profile_behavior_check(
    gas: &GasRecord,
    baseline: Option<&GasRecord>,
    scenario: Option<&Scenario>,
    supports_logs: bool,
) -> ProfileBehaviorCheck {
    let has_observers = scenario.is_some_and(|scenario| !scenario.observers.is_empty());
    let compares_return = scenario.is_some_and(|scenario| scenario.compare_return);
    let Some(baseline) = baseline else {
        return ProfileBehaviorCheck {
            baseline_profile: None,
            profile_behavior_check: "not_run",
            return_data_check: if compares_return {
                "not_run"
            } else {
                "not_applicable"
            },
            observer_check: if has_observers {
                "not_run"
            } else {
                "not_applicable"
            },
            log_check: if supports_logs {
                "not_run"
            } else {
                "not_applicable"
            },
        };
    };

    let status_matches = gas.call_succeeded == baseline.call_succeeded;
    let return_data_check = if compares_return && gas.call_succeeded && baseline.call_succeeded {
        compare_hashes(gas.return_hash.as_ref(), baseline.return_hash.as_ref())
    } else {
        "not_applicable"
    };
    let observer_check = if has_observers {
        compare_hashes(gas.observer_hash.as_ref(), baseline.observer_hash.as_ref())
    } else {
        "not_applicable"
    };
    let log_check = if supports_logs && gas.call_succeeded && baseline.call_succeeded {
        compare_hashes(gas.log_hash.as_ref(), baseline.log_hash.as_ref())
    } else {
        "not_applicable"
    };

    let checks = [return_data_check, observer_check, log_check];
    let profile_behavior_check = if !status_matches || checks.contains(&"fail") {
        "fail"
    } else if checks.contains(&"not_run") {
        "not_run"
    } else {
        "pass"
    };

    ProfileBehaviorCheck {
        baseline_profile: Some(baseline.profile_id.clone()),
        profile_behavior_check,
        return_data_check,
        observer_check,
        log_check,
    }
}

fn compare_hashes(left: Option<&String>, right: Option<&String>) -> &'static str {
    match (left, right) {
        (Some(left), Some(right)) if left == right => "pass",
        (Some(_), Some(_)) => "fail",
        _ => "not_run",
    }
}

fn failure_row(root: &Path, failure: &CompileFailure) -> serde_json::Value {
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
            &failure.benchmark_id,
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
        "source_path": report_path(root, &failure.source_path),
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

fn report_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn real_derived_manifest(root: &Path, compiled: &CompileSet) -> Vec<serde_json::Value> {
    let mut benchmarks = BTreeMap::new();
    let mut seen_variants = BTreeSet::new();
    for artifact in &compiled.artifacts {
        if let Some(provenance) = &artifact.provenance {
            let benchmark = benchmarks
                .entry(artifact.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&artifact.benchmark_id, provenance));
            add_source_variant_to_manifest(
                root,
                benchmark,
                &mut seen_variants,
                SourceVariantManifestEntry {
                    benchmark_id: &artifact.benchmark_id,
                    implementation_id: &artifact.implementation_id,
                    language: artifact.language,
                    profile_id: &artifact.profile_id,
                    source_path: &artifact.source_path,
                    source_hash: &artifact.source_hash,
                    compiler_settings: &artifact.compiler_settings,
                    compile_status: "ok",
                },
            );
        }
    }
    for failure in &compiled.failures {
        if let Some(provenance) = &failure.provenance {
            let benchmark = benchmarks
                .entry(failure.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&failure.benchmark_id, provenance));
            add_source_variant_to_manifest(
                root,
                benchmark,
                &mut seen_variants,
                SourceVariantManifestEntry {
                    benchmark_id: &failure.benchmark_id,
                    implementation_id: &failure.implementation_id,
                    language: failure.language,
                    profile_id: &failure.profile_id,
                    source_path: &failure.source_path,
                    source_hash: &failure.source_hash,
                    compiler_settings: &failure.compiler_settings,
                    compile_status: "compile_error",
                },
            );
        }
    }
    benchmarks.into_values().collect()
}

struct SourceVariantManifestEntry<'a> {
    benchmark_id: &'a str,
    implementation_id: &'a str,
    language: Language,
    profile_id: &'a str,
    source_path: &'a Path,
    source_hash: &'a str,
    compiler_settings: &'a serde_json::Value,
    compile_status: &'a str,
}

fn add_source_variant_to_manifest(
    root: &Path,
    benchmark: &mut serde_json::Value,
    seen_variants: &mut BTreeSet<String>,
    entry: SourceVariantManifestEntry<'_>,
) {
    let source_path = report_path(root, entry.source_path);
    let source_variant = entry
        .compiler_settings
        .get("sourceVariant")
        .and_then(|value| value.as_str())
        .unwrap_or("default");
    let key = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}",
        entry.benchmark_id,
        entry.language.as_str(),
        entry.implementation_id,
        entry.profile_id,
        source_variant,
        source_path,
        entry.source_hash
    );
    if !seen_variants.insert(key) {
        return;
    }
    if let Some(variants) = benchmark
        .get_mut("source_variants")
        .and_then(|value| value.as_array_mut())
    {
        variants.push(json!({
            "language": entry.language.as_str(),
            "implementation_id": entry.implementation_id,
            "profile_id": entry.profile_id,
            "source_variant": source_variant,
            "source_path": source_path,
            "source_hash": entry.source_hash,
            "compile_status": entry.compile_status
        }));
    }
}

fn provenance_manifest_value(benchmark_id: &str, provenance: &Provenance) -> serde_json::Value {
    let source_reference_path = provenance
        .upstream_reference_path(benchmark_id)
        .display()
        .to_string();
    json!({
        "benchmark_id": benchmark_id,
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_reference_path": source_reference_path,
        "source_language": provenance.source_language.as_str(),
        "source_compiler": &provenance.source_compiler,
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "comparison_lane": provenance.comparison_lane.as_str(),
        "source_lane": provenance.source_lane.as_str(),
        "counterpart_lane": provenance.counterpart_lane.as_str(),
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
        "source_variants": [],
    })
}

fn provenance_value(
    provenance: Option<&Provenance>,
    benchmark_id: &str,
    port_language: Language,
    port_version: &str,
) -> serde_json::Value {
    let Some(provenance) = provenance else {
        return serde_json::Value::Null;
    };
    let source_reference_path = provenance
        .upstream_reference_path(benchmark_id)
        .display()
        .to_string();
    json!({
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_reference_path": source_reference_path,
        "source_language": provenance.source_language.as_str(),
        "source_compiler": &provenance.source_compiler,
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "comparison_lane": provenance.comparison_lane.as_str(),
        "implementation_lane": provenance.lane_for_language(port_language).as_str(),
        "source_lane": provenance.source_lane.as_str(),
        "counterpart_lane": provenance.counterpart_lane.as_str(),
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
    let venom = if row
        .pointer("/compiler/settings/experimentalCodegen")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        " + Venom"
    } else {
        ""
    };
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

#[cfg(test)]
mod tests {
    #[test]
    fn property_credit_requires_evidence_for_this_exact_profile() {
        use crate::{
            behavior::Evidence,
            models::CacheInfo,
            test_support::{artifact, gas},
        };
        let artifact = artifact("solx", "solx-0.1.8-O3");
        let gas = gas(&artifact);
        let scenario: crate::models::ScenarioFile = serde_yaml::from_str(include_str!(
            "../../../benches/scenarios/erc20_minimal.yaml"
        ))
        .unwrap();
        let render = |evidence: &[Evidence]| {
            super::row(
                std::path::Path::new("/test"),
                &artifact,
                &gas,
                &scenario,
                vec![],
                "prague",
                evidence,
                super::profile_behavior_check(&gas, None, scenario.scenarios.first(), false),
            )
        };
        let absent = render(&[]);
        assert_eq!(absent["correctness"]["property_tests"], "not_run");
        assert_eq!(
            absent["correctness"]["randomized_differential_check"],
            "not_run"
        );
        let mut proof = Evidence {
            benchmark_id: artifact.benchmark_id.clone(),
            baseline_profile: "solc-0.8.34-viair-runs200".into(),
            compared_profile: "solx-0.1.8-Oz".into(),
            scenario_count: scenario.scenarios.len(),
            randomized: true,
            properties: scenario.properties.iter().map(|p| p.name.clone()).collect(),
            cache: CacheInfo::disabled(),
        };
        assert_eq!(
            render(&[proof.clone()])["correctness"]["property_tests"],
            "not_run"
        );
        proof.compared_profile = artifact.profile_id.clone();
        let verified = render(&[proof]);
        assert_eq!(verified["correctness"]["property_tests"], "pass");
        assert_eq!(
            verified["correctness"]["baseline_differential_check"],
            "pass"
        );
        assert_eq!(
            verified["correctness"]["randomized_differential_check"],
            "pass"
        );
    }
    use super::{manifest_profiles, report_methodology};
    use crate::models::{CompilerProfile, Language, MetadataMode};

    fn profile(id: &str, source_variant: Option<&str>) -> CompilerProfile {
        CompilerProfile {
            id: id.to_string(),
            language: Language::Solidity,
            compiler: "solc".to_string(),
            optimizer: false,
            optimizer_runs: 0,
            optimizer_mode: None,
            experimental_codegen: false,
            via_ir: false,
            metadata_mode: MetadataMode::Off,
            source_variant: source_variant.map(str::to_string),
            evm_version: "latest-shared".to_string(),
        }
    }

    #[test]
    fn manifest_profiles_label_implicit_source_variant_as_latest() {
        let profiles = manifest_profiles(&[
            profile("solc-latest-noopt", None),
            profile("solc-0.5.16-noopt", Some("solidity-0.5")),
        ]);

        assert_eq!(profiles[0]["source_variant"], "latest");
        assert_eq!(profiles[1]["source_variant"], "solidity-0.5");
    }

    #[test]
    fn report_methodology_carries_latest_source_policy() {
        let methodology = report_methodology();
        assert_eq!(
            methodology["source_model"]["compiled_source_root"],
            "target/bench-source-variants/<profile_id>/"
        );
        assert!(
            methodology["source_model"]["real_derived"]
                .as_str()
                .unwrap()
                .contains("latest-syntax source-language originals")
        );
        assert!(
            methodology["source_model"]["compatibility_variants"]
                .as_str()
                .unwrap()
                .contains("generated variants of the checked-in latest source")
        );
    }
}

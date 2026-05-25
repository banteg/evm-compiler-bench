use crate::{
    catalog::checked_in_benchmarks,
    models::{Benchmark, BenchmarkSuite, ComparisonLane, Provenance, ScenarioFile},
    scale::{SCALE_GENERATOR_VERSION, ScaleConfig, ScaleManifest, load_scale_config},
    scenarios::{load_scenario_catalog, validate_scenario_file},
    util::sha256_file,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

const LATEST_SOLIDITY_PRAGMA: &str = "pragma solidity ^0.8.35;";
const LATEST_VYPER_PRAGMA: &str = "# pragma version >=0.4.3,<0.5.0";

#[derive(Debug, Clone, Copy)]
pub struct ValidationSummary {
    pub specs: usize,
    pub scenario_files: usize,
    pub scale_families: usize,
    pub generated_benchmarks: usize,
    pub result_rows: usize,
}

pub fn validate_all(root: &Path) -> Result<ValidationSummary> {
    let specs = validate_specs(root)?;
    let scenario_files = validate_scenarios(root)?;
    validate_latest_source_pragmas(root)?;
    let (scale_config, _) = load_scale_config(root)?;
    let scale_families = scale_config.families.len();
    let generated_benchmarks = validate_generated_outputs_if_present(root, &scale_config)?;
    let result_rows = validate_outputs_if_present(root)?;
    Ok(ValidationSummary {
        specs,
        scenario_files,
        scale_families,
        generated_benchmarks,
        result_rows,
    })
}

fn validate_specs(root: &Path) -> Result<usize> {
    let mut count = 0;
    let benchmarks: BTreeMap<_, _> = checked_in_benchmarks()
        .into_iter()
        .map(|bench| (bench.id.clone(), bench))
        .collect();
    for path in yaml_files(&root.join("benches/specs"))? {
        let text = fs::read_to_string(&path)?;
        let value: serde_yaml::Value =
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        validate_benchmark_spec(root, &value, &path)?;
        let id = value
            .get("id")
            .and_then(|value| value.as_str())
            .with_context(|| format!("{} missing id", path.display()))?;
        let Some(benchmark) = benchmarks.get(id) else {
            bail!(
                "{} id {id} is not in the checked-in benchmark catalog",
                path.display()
            );
        };
        validate_checked_in_spec_metadata(root, &value, &path, benchmark)?;
        count += 1;
    }
    if count != benchmarks.len() {
        bail!(
            "expected {} benchmark specs, found {count}",
            benchmarks.len()
        );
    }
    Ok(count)
}

fn validate_scenarios(root: &Path) -> Result<usize> {
    let catalog = load_scenario_catalog(root, None, &[])?;
    let benchmarks: BTreeMap<_, _> = checked_in_benchmarks()
        .into_iter()
        .map(|bench| (bench.id.clone(), bench))
        .collect();
    let bench_ids: BTreeSet<_> = benchmarks.keys().cloned().collect();
    let mut count = 0;
    for file in catalog.iter() {
        if !bench_ids.contains(&file.benchmark_id) {
            bail!(
                "scenario file references unknown checked-in benchmark {}",
                file.benchmark_id
            );
        }
        if let Some(benchmark) = benchmarks.get(&file.benchmark_id)
            && benchmark.suite == BenchmarkSuite::RealDerived
        {
            validate_real_derived_scenario_coverage(file, benchmark)?;
        }
        count += 1;
    }
    if count != bench_ids.len() {
        bail!("expected {} scenario files, found {count}", bench_ids.len());
    }
    Ok(count)
}

fn validate_real_derived_scenario_coverage(
    file: &ScenarioFile,
    benchmark: &Benchmark,
) -> Result<()> {
    let Some(provenance) = benchmark.provenance.as_ref() else {
        bail!("real-derived benchmark {} has no provenance", benchmark.id);
    };
    let covered: BTreeSet<_> = provenance.scenario_coverage.iter().cloned().collect();
    let actual: BTreeSet<_> = file
        .scenarios
        .iter()
        .map(|scenario| scenario.name.clone())
        .collect();
    if covered != actual {
        bail!(
            "real-derived scenario coverage for {} must exactly match scenario ids; coverage={covered:?} scenarios={actual:?}",
            benchmark.id
        );
    }
    Ok(())
}

fn validate_latest_source_pragmas(root: &Path) -> Result<()> {
    let mut paths = Vec::new();
    collect_source_files(&root.join("benches/implementations"), &mut paths)?;
    collect_source_files(
        &root.join("crates/bench-cli/src/scale_templates"),
        &mut paths,
    )?;

    for path in paths {
        if path_has_component(&path, "upstream") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .with_context(|| format!("reading source pragma from {}", path.display()))?;
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("sol") if !source.contains(LATEST_SOLIDITY_PRAGMA) => {
                bail!(
                    "{} latest-lane Solidity source must use `{LATEST_SOLIDITY_PRAGMA}`",
                    path.display()
                );
            }
            Some("vy") if !source.contains(LATEST_VYPER_PRAGMA) => {
                bail!(
                    "{} latest-lane Vyper source must use `{LATEST_VYPER_PRAGMA}`",
                    path.display()
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("sol" | "vy")
        ) {
            out.push(path);
        }
    }
    Ok(())
}

fn path_has_component(path: &Path, needle: &str) -> bool {
    let needle = OsStr::new(needle);
    path.components()
        .any(|component| matches!(component, Component::Normal(part) if part == needle))
}

fn validate_generated_outputs_if_present(root: &Path, config: &ScaleConfig) -> Result<usize> {
    let manifest_path = root.join("target/bench-generated/manifest.json");
    if !manifest_path.exists() {
        return Ok(0);
    }
    let manifest: ScaleManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    if manifest.generator_version != SCALE_GENERATOR_VERSION {
        bail!(
            "{} generator version {} does not match {}",
            manifest_path.display(),
            manifest.generator_version,
            SCALE_GENERATOR_VERSION
        );
    }
    if manifest.parameter_name != config.parameter_name {
        bail!(
            "{} parameter_name does not match scale config",
            manifest_path.display()
        );
    }
    if manifest.values != config.values {
        bail!(
            "{} values do not match scale config",
            manifest_path.display()
        );
    }

    let families: BTreeSet<_> = config
        .families
        .iter()
        .map(|family| family.id.as_str())
        .collect();
    let values: BTreeSet<_> = config.values.iter().copied().collect();
    let expected_count = families.len() * values.len();
    if manifest.benchmarks.len() != expected_count {
        bail!(
            "{} expected {expected_count} generated benchmarks, found {}",
            manifest_path.display(),
            manifest.benchmarks.len()
        );
    }

    let mut benchmark_ids = BTreeSet::new();
    for benchmark in &manifest.benchmarks {
        if !benchmark_ids.insert(benchmark.benchmark_id.as_str()) {
            bail!(
                "{} duplicate generated benchmark {}",
                manifest_path.display(),
                benchmark.benchmark_id
            );
        }
        if !families.contains(benchmark.family.as_str()) {
            bail!(
                "{} generated benchmark {} has unknown family {}",
                manifest_path.display(),
                benchmark.benchmark_id,
                benchmark.family
            );
        }
        if benchmark.parameter_name != config.parameter_name {
            bail!(
                "{} generated benchmark {} has wrong parameter name {}",
                manifest_path.display(),
                benchmark.benchmark_id,
                benchmark.parameter_name
            );
        }
        if !values.contains(&benchmark.parameter_value) {
            bail!(
                "{} generated benchmark {} has unsupported parameter value {}",
                manifest_path.display(),
                benchmark.benchmark_id,
                benchmark.parameter_value
            );
        }

        validate_generated_path(
            root,
            &benchmark.solidity_path,
            &benchmark.solidity_hash,
            "solidity source",
        )?;
        validate_generated_path(
            root,
            &benchmark.vyper_path,
            &benchmark.vyper_hash,
            "vyper source",
        )?;
        let spec_path =
            validate_generated_path(root, &benchmark.spec_path, &benchmark.spec_hash, "spec")?;
        let scenario_path = validate_generated_path(
            root,
            &benchmark.scenario_path,
            &benchmark.scenario_hash,
            "scenario",
        )?;

        let spec: serde_yaml::Value = serde_yaml::from_str(&fs::read_to_string(&spec_path)?)
            .with_context(|| format!("parsing {}", spec_path.display()))?;
        validate_benchmark_spec(root, &spec, &spec_path)?;
        require_yaml_string(&spec, "id", &spec_path, &benchmark.benchmark_id)?;
        let scale = spec
            .get("scale")
            .with_context(|| format!("{} missing scale metadata", spec_path.display()))?;
        require_yaml_string(scale, "family", &spec_path, &benchmark.family)?;
        require_yaml_string(
            scale,
            "parameter_name",
            &spec_path,
            &benchmark.parameter_name,
        )?;
        let spec_parameter_value = scale
            .get("parameter_value")
            .and_then(|value| value.as_u64())
            .with_context(|| format!("{} missing numeric parameter_value", spec_path.display()))?;
        if spec_parameter_value != benchmark.parameter_value {
            bail!(
                "{} parameter_value {spec_parameter_value} does not match manifest value {}",
                spec_path.display(),
                benchmark.parameter_value
            );
        }
        let implementations = spec
            .get("implementations")
            .with_context(|| format!("{} missing implementations", spec_path.display()))?;
        require_yaml_string(
            implementations,
            "solidity",
            &spec_path,
            &benchmark.solidity_path,
        )?;
        require_yaml_string(implementations, "vyper", &spec_path, &benchmark.vyper_path)?;

        let scenario_file = serde_yaml::from_str(&fs::read_to_string(&scenario_path)?)
            .with_context(|| format!("parsing {}", scenario_path.display()))?;
        validate_scenario_file(&scenario_file, &scenario_path)?;
        if scenario_file.benchmark_id != benchmark.benchmark_id {
            bail!(
                "{} benchmark_id {} does not match manifest benchmark {}",
                scenario_path.display(),
                scenario_file.benchmark_id,
                benchmark.benchmark_id
            );
        }
    }

    Ok(manifest.benchmarks.len())
}

fn validate_outputs_if_present(root: &Path) -> Result<usize> {
    let results_path = root.join("results/normalized/results.json");
    let manifest_path = root.join("results/normalized/run-manifest.json");
    let mut rows = 0;
    if results_path.exists() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&results_path)?)
            .with_context(|| format!("parsing {}", results_path.display()))?;
        let array = value
            .as_array()
            .with_context(|| format!("{} must be an array", results_path.display()))?;
        for row in array {
            require_json_pointer(row, "/status", &results_path)?;
            require_json_pointer(row, "/benchmark_id", &results_path)?;
            require_json_pointer(row, "/implementation_id", &results_path)?;
            require_json_pointer(row, "/profile_id", &results_path)?;
            require_json_pointer(row, "/suite", &results_path)?;
            require_json_pointer(row, "/family", &results_path)?;
            require_json_pointer(row, "/parameter_name", &results_path)?;
            require_json_pointer(row, "/parameter_value", &results_path)?;
            require_json_pointer(row, "/generated", &results_path)?;
            require_json_pointer(row, "/generated/generator_version", &results_path)?;
            require_json_pointer(row, "/generated/scenario_path", &results_path)?;
            require_json_pointer(row, "/generated/scenario_hash", &results_path)?;
            require_json_pointer(row, "/provenance", &results_path)?;
            require_json_pointer(row, "/compiler/name", &results_path)?;
            require_json_pointer(row, "/compiler/version", &results_path)?;
            require_json_pointer(row, "/compiler/metadata", &results_path)?;
            require_json_pointer(row, "/compiler/settings", &results_path)?;
            require_json_pointer(row, "/compiler/settings/metadataMode", &results_path)?;
            require_json_pointer(row, "/cache/compile/status", &results_path)?;
            require_json_pointer(row, "/compile/status", &results_path)?;
            require_json_pointer(row, "/correctness/scenario_status_check", &results_path)?;
            require_json_pointer(row, "/correctness/golden_behavior_check", &results_path)?;
            require_json_pointer(
                row,
                "/correctness/baseline_differential_check",
                &results_path,
            )?;
            require_json_pointer(row, "/correctness/profile_behavior_check", &results_path)?;
            require_json_pointer(row, "/correctness/observer_check", &results_path)?;
            require_json_pointer(row, "/correctness/return_data_check", &results_path)?;
            require_json_pointer(row, "/correctness/log_check", &results_path)?;
            require_json_pointer(
                row,
                "/correctness/randomized_differential_check",
                &results_path,
            )?;
            require_json_pointer(row, "/correctness/property_tests", &results_path)?;
            require_json_pointer(row, "/correctness/failure_artifacts", &results_path)?;
            require_json_pointer(row, "/correctness/scenario_status_ok", &results_path)?;
            require_enum(row, "/status", &["ok", "compile_error"], &results_path)?;
            require_enum(
                row,
                "/suite",
                &["fixed", "scale", "real_derived"],
                &results_path,
            )?;
            require_enum(
                row,
                "/compiler/settings/metadataMode",
                &["on", "off"],
                &results_path,
            )?;
            require_enum(
                row,
                "/cache/compile/status",
                &["hit", "miss", "stale", "refreshed", "disabled"],
                &results_path,
            )?;
            validate_row_status(row, &results_path)?;
            for pointer in [
                "/correctness/scenario_status_check",
                "/correctness/golden_behavior_check",
                "/correctness/baseline_differential_check",
                "/correctness/profile_behavior_check",
                "/correctness/observer_check",
                "/correctness/return_data_check",
                "/correctness/log_check",
            ] {
                require_enum(
                    row,
                    pointer,
                    &["pass", "fail", "not_applicable", "not_run", "baseline_only"],
                    &results_path,
                )?;
            }
            require_enum(
                row,
                "/correctness/randomized_differential_check",
                &["pass", "fail", "not_applicable"],
                &results_path,
            )?;
            require_enum(
                row,
                "/correctness/property_tests",
                &["pass", "fail", "not_applicable"],
                &results_path,
            )?;
            if !row
                .pointer("/correctness/failure_artifacts")
                .is_some_and(|value| value.is_array())
            {
                bail!(
                    "{} failure_artifacts must be an array",
                    results_path.display()
                );
            }
            require_bool_pointer(row, "/correctness/scenario_status_ok", &results_path)?;
            validate_suite_metadata(row, &results_path)?;
            rows += 1;
        }
    }
    if manifest_path.exists() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        for pointer in [
            "/run_id",
            "/started_at",
            "/evm_version",
            "/toolchains",
            "/profiles",
            "/scale_generator",
            "/scale_generator/version",
            "/scale_generator/config_hash",
            "/scale_generator/parameter_name",
            "/scale_generator/values",
            "/scale_generator/benchmarks",
            "/real_derived",
            "/real_derived/benchmarks",
            "/artifacts",
            "/compile_failures",
            "/gas_records",
            "/environment",
        ] {
            require_json_pointer(&value, pointer, &manifest_path)?;
        }
        if !value
            .pointer("/scale_generator/benchmarks")
            .is_some_and(|value| value.is_array())
        {
            bail!(
                "{} scale_generator.benchmarks must be an array",
                manifest_path.display()
            );
        }
        if !value
            .pointer("/real_derived/benchmarks")
            .is_some_and(|value| value.is_array())
        {
            bail!(
                "{} real_derived.benchmarks must be an array",
                manifest_path.display()
            );
        }
        validate_real_derived_manifest(&value, &manifest_path)?;
    }
    Ok(rows)
}

fn validate_real_derived_manifest(value: &Value, path: &Path) -> Result<()> {
    let benchmarks = value
        .pointer("/real_derived/benchmarks")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} real_derived.benchmarks must be an array",
                path.display()
            )
        })?;
    for benchmark in benchmarks {
        for pointer in [
            "/benchmark_id",
            "/source_lane",
            "/counterpart_lane",
            "/source_path",
            "/source_reference_path",
            "/source_blob",
        ] {
            require_string_pointer(benchmark, pointer, path)?;
        }
        require_bool_pointer(benchmark, "/production_equivalence", path)?;
    }
    Ok(())
}

fn validate_benchmark_spec(root: &Path, value: &serde_yaml::Value, path: &Path) -> Result<()> {
    require_sequence(value, "abi", path)?;
    require_sequence(value, "scenarios", path)?;
    let implementations = value
        .get("implementations")
        .with_context(|| format!("{} missing implementations", path.display()))?;
    for language in ["solidity", "vyper"] {
        let implementation = implementations
            .get(language)
            .and_then(|value| value.as_str())
            .with_context(|| format!("{} missing {language} implementation", path.display()))?;
        let implementation_path = root.join(implementation);
        if !implementation_path.exists() {
            bail!(
                "{} references missing {language} implementation {}",
                path.display(),
                implementation_path.display()
            );
        }
    }
    Ok(())
}

fn validate_checked_in_spec_metadata(
    root: &Path,
    value: &serde_yaml::Value,
    path: &Path,
    benchmark: &Benchmark,
) -> Result<()> {
    match benchmark.suite {
        BenchmarkSuite::Fixed => {
            if value.get("real_derived").is_some() {
                bail!(
                    "{} fixed benchmark must not have real_derived metadata",
                    path.display()
                );
            }
        }
        BenchmarkSuite::RealDerived => {
            let Some(provenance) = benchmark.provenance.as_ref() else {
                bail!(
                    "{} real-derived benchmark {} has no catalog provenance",
                    path.display(),
                    benchmark.id
                );
            };
            validate_real_derived_spec(root, value, path, benchmark, provenance)?;
        }
        BenchmarkSuite::Scale => {
            bail!(
                "{} scale benchmarks must be generated, not checked in",
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_real_derived_spec(
    root: &Path,
    value: &serde_yaml::Value,
    path: &Path,
    benchmark: &Benchmark,
    provenance: &Provenance,
) -> Result<()> {
    let real = value
        .get("real_derived")
        .with_context(|| format!("{} missing real_derived metadata", path.display()))?;
    require_yaml_string(real, "suite", path, BenchmarkSuite::RealDerived.as_str())?;
    require_yaml_string(real, "model_kind", path, &provenance.model_kind)?;
    require_yaml_string(
        real,
        "comparison_lane",
        path,
        provenance.comparison_lane.as_str(),
    )?;
    require_yaml_string(real, "source_lane", path, provenance.source_lane.as_str())?;
    require_yaml_string(
        real,
        "counterpart_lane",
        path,
        provenance.counterpart_lane.as_str(),
    )?;
    require_yaml_string(real, "upstream_project", path, &provenance.upstream_project)?;
    require_yaml_string(real, "repository_url", path, &provenance.repository_url)?;
    require_yaml_string(real, "source_commit", path, &provenance.source_commit)?;
    require_yaml_string(real, "source_path", path, &provenance.source_path)?;
    require_yaml_string(real, "source_contract", path, &provenance.source_contract)?;
    require_yaml_string(
        real,
        "source_language",
        path,
        provenance.source_language.as_str(),
    )?;
    require_yaml_string(real, "source_compiler", path, &provenance.source_compiler)?;
    require_sequence(real, "source_profiles", path)?;
    validate_real_derived_lanes(path, provenance)?;
    validate_source_profiles(path, real, provenance)?;
    require_yaml_bool(
        real,
        "production_equivalence",
        path,
        provenance.production_equivalence,
    )?;
    require_yaml_string(
        real,
        "api_compatibility",
        path,
        &provenance.api_compatibility,
    )?;
    require_yaml_bool(
        real,
        "storage_layout_compatibility",
        path,
        provenance.storage_layout_compatibility,
    )?;
    require_yaml_string(
        real,
        "external_token_semantics",
        path,
        &provenance.external_token_semantics,
    )?;
    require_yaml_string(
        real,
        "source_derivation",
        path,
        &provenance.source_derivation,
    )?;
    require_sequence(real, "equivalence_scope", path)?;
    require_sequence(real, "scenario_coverage", path)?;
    require_sequence(real, "mock_assumptions", path)?;
    require_sequence(real, "included_features", path)?;
    let excluded_features = real
        .get("excluded_features")
        .and_then(|value| value.as_sequence())
        .with_context(|| format!("{} missing excluded_features", path.display()))?;
    if !provenance.production_equivalence && excluded_features.is_empty() {
        bail!(
            "{} non-production-equivalent real-derived benchmark must explain excluded_features",
            path.display()
        );
    }
    if provenance.production_equivalence && !excluded_features.is_empty() {
        bail!(
            "{} production-equivalent real-derived benchmark must not list excluded_features",
            path.display()
        );
    }
    validate_source_blob(root, path, benchmark, provenance)?;
    Ok(())
}

fn validate_real_derived_lanes(path: &Path, provenance: &Provenance) -> Result<()> {
    if matches!(
        provenance.source_lane,
        ComparisonLane::FixtureScopedPort | ComparisonLane::DiagnosticLayoutMatched
    ) {
        bail!(
            "{} real-derived source_lane must identify an original source lane",
            path.display()
        );
    }
    if provenance.comparison_lane == ComparisonLane::LatestIdiomatic
        && provenance.source_lane != ComparisonLane::LatestIdiomatic
    {
        bail!(
            "{} latest_idiomatic real-derived comparison_lane requires latest_idiomatic source_lane",
            path.display()
        );
    }
    if provenance.comparison_lane == ComparisonLane::ProductionConformance
        && provenance.source_lane != ComparisonLane::LatestSyntaxOriginal
    {
        bail!(
            "{} production_conformance real-derived comparison_lane requires latest_syntax_original source_lane",
            path.display()
        );
    }
    if provenance.comparison_lane == ComparisonLane::FixtureScopedPort {
        bail!(
            "{} fixture_scoped_port is a source/counterpart lane; use production_conformance or diagnostic_layout_matched for comparison_lane",
            path.display()
        );
    }
    Ok(())
}

fn validate_source_profiles(
    path: &Path,
    real: &serde_yaml::Value,
    provenance: &Provenance,
) -> Result<()> {
    let profiles = real
        .get("source_profiles")
        .and_then(|value| value.as_sequence())
        .with_context(|| format!("{} missing source_profiles", path.display()))?;
    let latest_source_profile_prefix = match provenance.source_language {
        crate::models::Language::Solidity => "solc-latest",
        crate::models::Language::Vyper => "vyper-latest",
    };
    let mut has_latest_source_profile = false;
    for profile in profiles {
        let profile = profile
            .as_str()
            .with_context(|| format!("{} source_profiles must contain strings", path.display()))?;
        let expected_prefix = match provenance.source_language {
            crate::models::Language::Solidity => "solc",
            crate::models::Language::Vyper => "vyper",
        };
        if !profile.starts_with(expected_prefix) {
            bail!(
                "{} source profile {profile} must match source language {}",
                path.display(),
                provenance.source_language.as_str()
            );
        }
        if profile.starts_with(latest_source_profile_prefix) {
            has_latest_source_profile = true;
        }
    }
    if provenance.source_lane == ComparisonLane::LatestSyntaxOriginal && !has_latest_source_profile
    {
        bail!(
            "{} latest_syntax_original source_lane requires at least one {latest_source_profile_prefix} source profile",
            path.display()
        );
    }
    Ok(())
}

fn validate_source_blob(
    root: &Path,
    path: &Path,
    benchmark: &Benchmark,
    provenance: &Provenance,
) -> Result<()> {
    let Some(expected_blob) = provenance.source_blob.as_deref() else {
        if matches!(
            provenance.source_lane,
            ComparisonLane::UpstreamExactHistorical | ComparisonLane::LatestSyntaxOriginal
        ) {
            bail!(
                "{} {} source_lane requires source_blob for provenance validation",
                path.display(),
                provenance.source_lane.as_str()
            );
        }
        return Ok(());
    };

    let source_path = match provenance.source_lane {
        ComparisonLane::UpstreamExactHistorical => {
            let implementation = match provenance.source_language {
                crate::models::Language::Solidity => &benchmark.solidity_path,
                crate::models::Language::Vyper => &benchmark.vyper_path,
            };
            if !implementation.ends_with(&provenance.source_path) {
                bail!(
                    "{} source-language implementation {} does not end with pinned upstream source_path {}",
                    path.display(),
                    implementation,
                    provenance.source_path
                );
            }
            root.join(implementation)
        }
        ComparisonLane::LatestSyntaxOriginal => {
            root.join(provenance.upstream_reference_path(&benchmark.id))
        }
        _ => return Ok(()),
    };

    let actual_blob = git_blob_hash(&source_path)
        .with_context(|| format!("hashing source blob {}", source_path.display()))?;
    if actual_blob != expected_blob {
        bail!(
            "{} source_blob mismatch for {}: expected {}, got {}",
            path.display(),
            source_path.display(),
            expected_blob,
            actual_blob
        );
    }
    Ok(())
}

fn git_blob_hash(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("hash-object")
        .arg(path)
        .output()
        .with_context(|| format!("running git hash-object {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "git hash-object {} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn validate_row_status(row: &Value, path: &Path) -> Result<()> {
    match row.pointer("/status").and_then(|value| value.as_str()) {
        Some("ok") => {
            require_enum(row, "/compile/status", &["ok"], path)?;
            require_json_pointer(row, "/bytecode/runtime_bytes", path)?;
            require_json_pointer(row, "/gas/scenario", path)?;
            require_json_pointer(row, "/gas/state_access_profile", path)?;
            require_json_pointer(row, "/gas/metadata_mode", path)?;
            require_json_pointer(row, "/gas/internal_create_gas", path)?;
            require_json_pointer(row, "/gas/harness_call_gas", path)?;
            require_json_pointer(row, "/gas/intrinsic_gas", path)?;
            require_json_pointer(row, "/gas/calldata_gas", path)?;
            require_json_pointer(row, "/gas/harness_estimated_tx_gas", path)?;
            require_json_pointer(row, "/gas/total_tx_gas", path)?;
            require_json_pointer(row, "/gas/expected_success", path)?;
            require_json_pointer(row, "/gas/call_succeeded", path)?;
            require_json_pointer(row, "/gas/scenario_status_ok", path)?;
            require_json_pointer(row, "/gas/measurement_scope", path)?;
            require_json_pointer(row, "/cache/gas/status", path)?;
            require_enum(
                row,
                "/gas/state_access_profile",
                &["cold", "warm", "mixed"],
                path,
            )?;
            require_enum(row, "/gas/metadata_mode", &["on", "off"], path)?;
            require_enum(
                row,
                "/gas/measurement_scope",
                &["foundry_internal_call_harness"],
                path,
            )?;
            require_enum(
                row,
                "/cache/gas/status",
                &["hit", "miss", "stale", "refreshed", "disabled"],
                path,
            )?;
            require_null(row, "/gas/total_tx_gas", path)?;
            require_bool_pointer(row, "/gas/expected_success", path)?;
            require_bool_pointer(row, "/gas/call_succeeded", path)?;
            require_bool_pointer(row, "/gas/scenario_status_ok", path)?;
            if row.pointer("/gas/metadata_mode") != row.pointer("/compiler/settings/metadataMode") {
                bail!("{} metadata mode mismatch in result row", path.display());
            }
        }
        Some("compile_error") => {
            require_enum(row, "/compile/status", &["error"], path)?;
            require_string_pointer(row, "/compile/error", path)?;
            require_null(row, "/bytecode", path)?;
            require_null(row, "/gas", path)?;
            require_null(row, "/cache/gas", path)?;
        }
        Some(other) => bail!("{} unsupported row status {other}", path.display()),
        None => bail!("{} missing row status", path.display()),
    }
    Ok(())
}

fn validate_generated_path(
    root: &Path,
    relative_path: &str,
    expected_hash: &str,
    label: &str,
) -> Result<PathBuf> {
    if Path::new(relative_path).is_absolute() || relative_path.contains("..") {
        bail!("{label} path {relative_path} must be a generated relative path");
    }
    if !relative_path.starts_with("target/bench-generated/") {
        bail!("{label} path {relative_path} is outside target/bench-generated");
    }
    let path = root.join(relative_path);
    if !path.exists() {
        bail!("missing generated {label} {}", path.display());
    }
    let actual_hash = sha256_file(&path)?;
    if actual_hash != expected_hash {
        bail!(
            "generated {label} {} hash mismatch: expected {expected_hash}, got {actual_hash}",
            path.display()
        );
    }
    Ok(path)
}

fn validate_suite_metadata(row: &Value, path: &Path) -> Result<()> {
    match row.pointer("/suite").and_then(|value| value.as_str()) {
        Some("fixed") => {
            require_null(row, "/family", path)?;
            require_null(row, "/parameter_name", path)?;
            require_null(row, "/parameter_value", path)?;
            require_null(row, "/generated/generator_version", path)?;
            require_null(row, "/generated/scenario_path", path)?;
            require_null(row, "/generated/scenario_hash", path)?;
            require_null(row, "/provenance", path)?;
        }
        Some("scale") => {
            require_string_pointer(row, "/family", path)?;
            require_string_pointer(row, "/parameter_name", path)?;
            require_u64_pointer(row, "/parameter_value", path)?;
            require_string_pointer(row, "/generated/generator_version", path)?;
            require_string_pointer(row, "/generated/scenario_path", path)?;
            require_string_pointer(row, "/generated/scenario_hash", path)?;
            require_null(row, "/provenance", path)?;
        }
        Some("real_derived") => {
            require_null(row, "/family", path)?;
            require_null(row, "/parameter_name", path)?;
            require_null(row, "/parameter_value", path)?;
            require_null(row, "/generated/generator_version", path)?;
            require_null(row, "/generated/scenario_path", path)?;
            require_null(row, "/generated/scenario_hash", path)?;
            for pointer in [
                "/provenance/model_kind",
                "/provenance/comparison_lane",
                "/provenance/upstream_project",
                "/provenance/repository_url",
                "/provenance/source_commit",
                "/provenance/source_path",
                "/provenance/source_language",
                "/provenance/source_compiler",
                "/provenance/source_contract",
                "/provenance/upstream_license",
                "/provenance/checked_at",
                "/provenance/api_compatibility",
                "/provenance/external_token_semantics",
                "/provenance/source_derivation",
                "/provenance/port_language",
                "/provenance/port_version",
                "/provenance/source_reference_path",
            ] {
                require_string_pointer(row, pointer, path)?;
            }
            require_bool_pointer(row, "/provenance/production_equivalence", path)?;
            require_bool_pointer(row, "/provenance/storage_layout_compatibility", path)?;
            require_enum(
                row,
                "/provenance/comparison_lane",
                &[
                    "upstream_exact_historical",
                    "latest_syntax_original",
                    "latest_idiomatic",
                    "production_conformance",
                    "diagnostic_layout_matched",
                    "fixture_scoped_port",
                ],
                path,
            )?;
            require_enum(
                row,
                "/provenance/source_language",
                &["solidity", "vyper"],
                path,
            )?;
            require_enum(
                row,
                "/provenance/port_language",
                &["solidity", "vyper"],
                path,
            )?;
            for pointer in [
                "/provenance/equivalence_scope",
                "/provenance/scenario_coverage",
                "/provenance/mock_assumptions",
                "/provenance/source_profiles",
                "/provenance/included_features",
                "/provenance/excluded_features",
            ] {
                if !row.pointer(pointer).is_some_and(|value| {
                    value.as_array().is_some_and(|items| {
                        !items.is_empty() && items.iter().all(|item| item.is_string())
                    })
                }) {
                    bail!(
                        "{} JSON pointer {pointer} must be a non-empty string array",
                        path.display()
                    );
                }
            }
        }
        Some(other) => bail!("{} unsupported suite {other}", path.display()),
        None => bail!("{} missing suite", path.display()),
    }
    Ok(())
}

fn require_sequence(value: &serde_yaml::Value, key: &str, path: &Path) -> Result<()> {
    if value
        .get(key)
        .and_then(|value| value.as_sequence())
        .is_none_or(|items| items.is_empty())
    {
        bail!("{} missing non-empty {key}", path.display());
    }
    Ok(())
}

fn require_yaml_string(
    value: &serde_yaml::Value,
    key: &str,
    path: &Path,
    expected: &str,
) -> Result<()> {
    let actual = value
        .get(key)
        .and_then(|value| value.as_str())
        .with_context(|| format!("{} missing string {key}", path.display()))?;
    if actual != expected {
        bail!(
            "{} {key} {actual} does not match expected value {expected}",
            path.display()
        );
    }
    Ok(())
}

fn require_yaml_bool(
    value: &serde_yaml::Value,
    key: &str,
    path: &Path,
    expected: bool,
) -> Result<()> {
    let actual = value
        .get(key)
        .and_then(|value| value.as_bool())
        .with_context(|| format!("{} missing boolean {key}", path.display()))?;
    if actual != expected {
        bail!(
            "{} {key} {actual} does not match expected value {expected}",
            path.display()
        );
    }
    Ok(())
}

fn require_json_pointer(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if value.pointer(pointer).is_none() {
        bail!("{} missing JSON pointer {pointer}", path.display());
    }
    Ok(())
}

fn require_string_pointer(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if !value
        .pointer(pointer)
        .is_some_and(|value| value.is_string())
    {
        bail!("{} JSON pointer {pointer} must be a string", path.display());
    }
    Ok(())
}

fn require_u64_pointer(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if !value.pointer(pointer).is_some_and(|value| value.is_u64()) {
        bail!(
            "{} JSON pointer {pointer} must be a positive integer",
            path.display()
        );
    }
    Ok(())
}

fn require_bool_pointer(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if !value
        .pointer(pointer)
        .is_some_and(|value| value.is_boolean())
    {
        bail!(
            "{} JSON pointer {pointer} must be a boolean",
            path.display()
        );
    }
    Ok(())
}

fn require_null(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if !value.pointer(pointer).is_some_and(|value| value.is_null()) {
        bail!("{} JSON pointer {pointer} must be null", path.display());
    }
    Ok(())
}

fn require_enum(value: &Value, pointer: &str, allowed: &[&str], path: &Path) -> Result<()> {
    let actual = value
        .pointer(pointer)
        .and_then(|value| value.as_str())
        .with_context(|| format!("{} JSON pointer {pointer} must be a string", path.display()))?;
    if !allowed.contains(&actual) {
        bail!(
            "{} JSON pointer {pointer} has unsupported value {actual}",
            path.display()
        );
    }
    Ok(())
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
    use std::path::Path;

    #[test]
    fn validates_checked_in_inputs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let checked_in_count = crate::catalog::checked_in_benchmarks().len();
        assert_eq!(super::validate_specs(root).unwrap(), checked_in_count);
        assert_eq!(super::validate_scenarios(root).unwrap(), checked_in_count);
        let (config, _) = crate::scale::load_scale_config(root).unwrap();
        assert_eq!(config.families.len(), 7);
        let generated_count = super::validate_generated_outputs_if_present(root, &config).unwrap();
        let expected_generated_count = config.families.len() * config.values.len();
        assert!(generated_count == 0 || generated_count == expected_generated_count);
    }
}

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
const SOURCE_VARIANT_LABELS: &[&str] = &[
    "latest",
    "solidity-0.4",
    "solidity-0.5",
    "solidity-0.6",
    "solidity-0.7",
    "solidity-0.8",
    "vyper-0.2",
    "vyper-0.3",
    "vyper-0.4",
];

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
    validate_schema_files(root)?;
    validate_latest_source_pragmas(root)?;
    validate_compiler_profile_source_variants(root)?;
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

fn validate_schema_files(root: &Path) -> Result<()> {
    for path in json_schema_files(&root.join("schemas"))? {
        let value: Value = serde_json::from_str(&fs::read_to_string(&path)?)
            .with_context(|| format!("parsing {}", path.display()))?;
        for pointer in ["/$schema", "/title", "/type"] {
            require_string_pointer(&value, pointer, &path)?;
        }
    }
    Ok(())
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

fn validate_compiler_profile_source_variants(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root.join("compiler-profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let profile: crate::models::CompilerProfile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let expected = expected_profile_source_variant(&profile, &path)?;
        match (profile.source_variant.as_deref(), expected) {
            (Some(actual), Some(expected)) if actual == expected => {}
            (None, None) => {}
            (Some(actual), Some(expected)) => bail!(
                "{} profile {} source_variant {actual} does not match expected {expected}",
                path.display(),
                profile.id
            ),
            (Some(actual), None) => bail!(
                "{} latest-source profile {} must not set source_variant {actual}",
                path.display(),
                profile.id
            ),
            (None, Some(expected)) => bail!(
                "{} historical profile {} must set source_variant {expected}",
                path.display(),
                profile.id
            ),
        }
    }
    Ok(())
}

fn expected_profile_source_variant(
    profile: &crate::models::CompilerProfile,
    path: &Path,
) -> Result<Option<&'static str>> {
    match profile.language {
        crate::models::Language::Solidity => {
            expected_solidity_source_variant(&profile.compiler, path)
        }
        crate::models::Language::Vyper => expected_vyper_source_variant(&profile.compiler, path),
    }
}

fn expected_solidity_source_variant(compiler: &str, path: &Path) -> Result<Option<&'static str>> {
    if compiler == "solc" {
        return Ok(None);
    }
    let version = compiler.strip_prefix("solc-").with_context(|| {
        format!(
            "{} Solidity compiler profile must use solc or solc-MAJOR.MINOR.PATCH, got {compiler}",
            path.display()
        )
    })?;
    let (_, minor, _) = parse_semver_prefix(version, path, "Solidity")?;
    Ok(Some(match minor {
        4 => "solidity-0.4",
        5 => "solidity-0.5",
        6 => "solidity-0.6",
        7 => "solidity-0.7",
        8 => "solidity-0.8",
        _ => bail!(
            "{} unsupported Solidity compiler profile version {compiler}",
            path.display()
        ),
    }))
}

fn expected_vyper_source_variant(compiler: &str, path: &Path) -> Result<Option<&'static str>> {
    if compiler == "vyper" {
        return Ok(None);
    }
    let version = compiler.strip_prefix("vyper-").with_context(|| {
        format!(
            "{} Vyper compiler profile must use vyper or vyper-MAJOR.MINOR.PATCH, got {compiler}",
            path.display()
        )
    })?;
    let (_, minor, _) = parse_semver_prefix(version, path, "Vyper")?;
    Ok(match minor {
        2 => Some("vyper-0.2"),
        3 => Some("vyper-0.3"),
        4 => Some("vyper-0.4"),
        5 => None,
        _ => bail!(
            "{} unsupported Vyper compiler profile version {compiler}",
            path.display()
        ),
    })
}

fn parse_semver_prefix(version: &str, path: &Path, language: &str) -> Result<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts
        .next()
        .with_context(|| format!("{} missing {language} major version", path.display()))?
        .parse::<u64>()?;
    let minor = parts
        .next()
        .with_context(|| format!("{} missing {language} minor version", path.display()))?
        .parse::<u64>()?;
    let patch_part = parts
        .next()
        .with_context(|| format!("{} missing {language} patch version", path.display()))?;
    let patch_digits: String = patch_part
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if patch_digits.is_empty() {
        bail!(
            "{} invalid {language} patch version {patch_part}",
            path.display()
        );
    }
    Ok((major, minor, patch_digits.parse::<u64>()?))
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
    let report_model_path = root.join("results/normalized/report-model.json");
    let real_derived_provenance = real_derived_provenance_by_id();
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
            require_json_pointer(row, "/compiler/settings/sourceVariant", &results_path)?;
            require_json_pointer(row, "/cache/compile/status", &results_path)?;
            require_json_pointer(row, "/compile/status", &results_path)?;
            require_json_pointer(row, "/source_path", &results_path)?;
            require_json_pointer(row, "/source_hash", &results_path)?;
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
                "/compiler/settings/sourceVariant",
                SOURCE_VARIANT_LABELS,
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
            validate_real_derived_row_matches_catalog(
                row,
                &real_derived_provenance,
                &results_path,
            )?;
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
        validate_manifest_profiles(&value, &manifest_path)?;
        validate_real_derived_manifest(&value, &manifest_path)?;
        validate_real_derived_manifest_matches_catalog(
            &value,
            &real_derived_provenance,
            &manifest_path,
        )?;
    }
    if report_model_path.exists() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&report_model_path)?)
            .with_context(|| format!("parsing {}", report_model_path.display()))?;
        validate_report_model(&value, &real_derived_provenance, &report_model_path)?;
    }
    Ok(rows)
}

fn real_derived_provenance_by_id() -> BTreeMap<String, Provenance> {
    checked_in_benchmarks()
        .into_iter()
        .filter_map(|benchmark| {
            benchmark
                .provenance
                .map(|provenance| (benchmark.id, provenance))
        })
        .collect()
}

fn validate_report_model(
    value: &Value,
    provenance_by_id: &BTreeMap<String, Provenance>,
    path: &Path,
) -> Result<()> {
    for pointer in [
        "/schema_version",
        "/generated_at",
        "/methodology",
        "/methodology/source_model",
        "/methodology/notes",
        "/real_derived_models",
        "/rows",
    ] {
        require_json_pointer(value, pointer, path)?;
    }
    validate_report_methodology(value, path)?;
    let manifest = value
        .pointer("/manifest")
        .with_context(|| format!("{} missing report manifest", path.display()))?;
    validate_manifest_profiles(manifest, path)?;
    validate_real_derived_manifest(manifest, path)?;
    validate_real_derived_manifest_matches_catalog(manifest, provenance_by_id, path)?;
    let manifest_sources = real_derived_manifest_source_set(manifest, path)?;
    let model_sources = validate_report_real_derived_models(value, provenance_by_id, path)?;
    if manifest_sources != model_sources {
        bail!(
            "{} report real_derived_models compiled_sources do not match embedded manifest source_variants",
            path.display()
        );
    }
    Ok(())
}

fn validate_report_methodology(value: &Value, path: &Path) -> Result<()> {
    require_string_value(
        value,
        "/methodology/source_model/compiled_source_root",
        "target/bench-source-variants/<profile_id>/",
        path,
    )?;
    let real_derived = string_at(value, "/methodology/source_model/real_derived", path)?;
    if !real_derived.contains("latest-syntax source-language originals")
        || !real_derived.contains("provenance references")
        || !real_derived.contains("not compiled headline artifacts")
    {
        bail!(
            "{} methodology real_derived source model must describe latest-syntax originals and provenance-only upstream references",
            path.display()
        );
    }
    let compatibility = string_at(
        value,
        "/methodology/source_model/compatibility_variants",
        path,
    )?;
    if !compatibility.contains("generated variants of the checked-in latest source")
        || !compatibility.contains("resolved compiler patch range")
    {
        bail!(
            "{} methodology compatibility source model must describe generated latest-source variants and exact compiler patch pragmas",
            path.display()
        );
    }
    let notes = value
        .pointer("/methodology/notes")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{} methodology.notes must be an array", path.display()))?;
    if notes.is_empty() {
        bail!("{} methodology.notes must not be empty", path.display());
    }
    let mut tags = BTreeSet::new();
    for note in notes {
        for pointer in ["/tag", "/title", "/body"] {
            require_string_pointer(note, pointer, path)?;
        }
        let tag = string_at(note, "/tag", path)?;
        if !tags.insert(tag.to_string()) {
            bail!("{} duplicate methodology note tag {tag}", path.display());
        }
    }
    for required in ["F", "G"] {
        if !tags.contains(required) {
            bail!(
                "{} methodology must include note {required} for real-derived source policy",
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_report_real_derived_models(
    value: &Value,
    provenance_by_id: &BTreeMap<String, Provenance>,
    path: &Path,
) -> Result<BTreeSet<String>> {
    let models = value
        .pointer("/real_derived_models")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{} real_derived_models must be an array", path.display()))?;
    let mut all_sources = BTreeSet::new();
    for model in models {
        let benchmark_id = string_at(model, "/benchmark_id", path)?;
        let Some(provenance) = provenance_by_id.get(benchmark_id) else {
            bail!(
                "{} report model references unknown real-derived benchmark {benchmark_id}",
                path.display()
            );
        };
        let report_provenance = model.pointer("/provenance").with_context(|| {
            format!(
                "{} report model benchmark {benchmark_id} missing provenance",
                path.display()
            )
        })?;
        validate_real_derived_provenance_fields(report_provenance, benchmark_id, provenance, path)?;

        let compiled_sources = model
            .pointer("/compiled_sources")
            .and_then(|value| value.as_array())
            .with_context(|| {
                format!(
                    "{} report model benchmark {benchmark_id} missing compiled_sources",
                    path.display()
                )
            })?;
        let mut seen_sources = BTreeSet::new();
        for source in compiled_sources {
            for pointer in [
                "/language",
                "/implementation_id",
                "/profile_id",
                "/source_variant",
                "/source_path",
                "/source_hash",
            ] {
                require_string_pointer(source, pointer, path)?;
            }
            require_enum(source, "/language", &["solidity", "vyper"], path)?;
            require_enum(source, "/source_variant", SOURCE_VARIANT_LABELS, path)?;
            validate_real_derived_source_variant_path(source, path)?;
            validate_real_derived_unique_compiled_source(source, &mut seen_sources, path)?;
            let key = real_derived_source_key(benchmark_id, source, path)?;
            if !all_sources.insert(key) {
                bail!(
                    "{} duplicate report compiled source across real-derived models for benchmark {benchmark_id}",
                    path.display()
                );
            }
        }
    }
    Ok(all_sources)
}

fn validate_real_derived_unique_compiled_source(
    source: &Value,
    seen_sources: &mut BTreeSet<String>,
    path: &Path,
) -> Result<()> {
    let key = [
        string_at(source, "/language", path)?,
        string_at(source, "/implementation_id", path)?,
        string_at(source, "/profile_id", path)?,
        string_at(source, "/source_variant", path)?,
        string_at(source, "/source_path", path)?,
        string_at(source, "/source_hash", path)?,
    ]
    .join("\0");
    if !seen_sources.insert(key) {
        bail!(
            "{} duplicate report compiled source for profile {} path {}",
            path.display(),
            string_at(source, "/profile_id", path)?,
            string_at(source, "/source_path", path)?
        );
    }
    Ok(())
}

fn real_derived_manifest_source_set(value: &Value, path: &Path) -> Result<BTreeSet<String>> {
    let benchmarks = value
        .pointer("/real_derived/benchmarks")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} real_derived.benchmarks must be an array",
                path.display()
            )
        })?;
    let mut sources = BTreeSet::new();
    for benchmark in benchmarks {
        let benchmark_id = string_at(benchmark, "/benchmark_id", path)?;
        let variants = benchmark
            .pointer("/source_variants")
            .and_then(|value| value.as_array())
            .with_context(|| {
                format!(
                    "{} real_derived benchmark {benchmark_id} missing source_variants",
                    path.display()
                )
            })?;
        for variant in variants {
            let key = real_derived_source_key(benchmark_id, variant, path)?;
            if !sources.insert(key) {
                bail!(
                    "{} duplicate manifest source variant across real-derived models for benchmark {benchmark_id}",
                    path.display()
                );
            }
        }
    }
    Ok(sources)
}

fn real_derived_source_key(benchmark_id: &str, source: &Value, path: &Path) -> Result<String> {
    Ok([
        benchmark_id,
        string_at(source, "/language", path)?,
        string_at(source, "/implementation_id", path)?,
        string_at(source, "/profile_id", path)?,
        string_at(source, "/source_variant", path)?,
        string_at(source, "/source_path", path)?,
        string_at(source, "/source_hash", path)?,
    ]
    .join("\0"))
}

fn validate_manifest_profiles(value: &Value, path: &Path) -> Result<()> {
    let profiles = value
        .pointer("/profiles")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{} profiles must be an array", path.display()))?;
    for profile in profiles {
        for pointer in ["/id", "/language", "/compiler", "/source_variant"] {
            require_string_pointer(profile, pointer, path)?;
        }
        require_enum(profile, "/language", &["solidity", "vyper"], path)?;
        require_enum(profile, "/source_variant", SOURCE_VARIANT_LABELS, path)?;
    }
    Ok(())
}

fn validate_real_derived_manifest(value: &Value, path: &Path) -> Result<()> {
    let profile_metadata = manifest_profile_metadata(value, path)?;
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
            "/comparison_lane",
            "/source_lane",
            "/counterpart_lane",
            "/source_language",
            "/source_path",
            "/source_reference_path",
            "/source_blob",
        ] {
            require_string_pointer(benchmark, pointer, path)?;
        }
        require_enum(
            benchmark,
            "/comparison_lane",
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
        for pointer in ["/source_lane", "/counterpart_lane"] {
            require_enum(
                benchmark,
                pointer,
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
        }
        require_enum(benchmark, "/source_language", &["solidity", "vyper"], path)?;
        validate_real_derived_manifest_lanes(benchmark, path)?;
        validate_real_derived_manifest_source_profiles(benchmark, path)?;
        validate_real_derived_manifest_excluded_features(benchmark, path)?;
        require_bool_pointer(benchmark, "/production_equivalence", path)?;
        let source_language = string_at(benchmark, "/source_language", path)?;
        let variants = benchmark
            .get("source_variants")
            .and_then(|value| value.as_array())
            .with_context(|| {
                format!(
                    "{} real_derived benchmark missing source_variants",
                    path.display()
                )
            })?;
        if variants.is_empty() {
            bail!(
                "{} real_derived benchmark source_variants must not be empty",
                path.display()
            );
        }
        let mut seen_variants = BTreeSet::new();
        for variant in variants {
            for pointer in [
                "/language",
                "/implementation_id",
                "/profile_id",
                "/source_variant",
                "/source_path",
                "/source_hash",
                "/compile_status",
            ] {
                require_string_pointer(variant, pointer, path)?;
            }
            require_enum(variant, "/language", &["solidity", "vyper"], path)?;
            require_enum(variant, "/source_variant", SOURCE_VARIANT_LABELS, path)?;
            require_enum(variant, "/compile_status", &["ok", "compile_error"], path)?;
            validate_real_derived_source_variant_path(variant, path)?;
            validate_real_derived_unique_source_variant(variant, &mut seen_variants, path)?;
            validate_real_derived_source_variant_profile(variant, &profile_metadata, path)?;
            validate_real_derived_source_variant_language(variant, source_language, path)?;
        }
    }
    Ok(())
}

fn validate_real_derived_source_variant_path(variant: &Value, path: &Path) -> Result<()> {
    let profile_id = string_at(variant, "/profile_id", path)?;
    let source_path = string_at(variant, "/source_path", path)?;
    validate_materialized_source_variant_path(profile_id, source_path, path, "source variant")
}

fn validate_real_derived_row_source_path(row: &Value, path: &Path) -> Result<()> {
    let profile_id = string_at(row, "/profile_id", path)?;
    let source_path = string_at(row, "/source_path", path)?;
    validate_materialized_source_variant_path(profile_id, source_path, path, "row source")
}

fn validate_materialized_source_variant_path(
    profile_id: &str,
    source_path: &str,
    path: &Path,
    context: &str,
) -> Result<()> {
    let source_path = Path::new(source_path);
    if source_path.is_absolute() {
        bail!(
            "{} real-derived {context} path {} must be relative",
            path.display(),
            source_path.display()
        );
    }
    if source_path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!(
            "{} real-derived {context} path {} must not escape the repository",
            path.display(),
            source_path.display()
        );
    }
    if path_has_component(source_path, "upstream") {
        bail!(
            "{} real-derived {context} path {} must not point at an upstream reference",
            path.display(),
            source_path.display()
        );
    }
    let expected_prefix = Path::new("target")
        .join("bench-source-variants")
        .join(profile_id);
    if !source_path.starts_with(&expected_prefix) {
        bail!(
            "{} real-derived {context} path {} must be under {}",
            path.display(),
            source_path.display(),
            expected_prefix.display()
        );
    }
    Ok(())
}

fn validate_real_derived_unique_source_variant(
    variant: &Value,
    seen_variants: &mut BTreeSet<String>,
    path: &Path,
) -> Result<()> {
    let key = [
        string_at(variant, "/language", path)?,
        string_at(variant, "/implementation_id", path)?,
        string_at(variant, "/profile_id", path)?,
        string_at(variant, "/source_variant", path)?,
        string_at(variant, "/source_path", path)?,
        string_at(variant, "/source_hash", path)?,
        string_at(variant, "/compile_status", path)?,
    ]
    .join("\0");
    if !seen_variants.insert(key) {
        bail!(
            "{} duplicate real-derived source variant for profile {} path {}",
            path.display(),
            string_at(variant, "/profile_id", path)?,
            string_at(variant, "/source_path", path)?
        );
    }
    Ok(())
}

fn validate_real_derived_manifest_matches_catalog(
    value: &Value,
    provenance_by_id: &BTreeMap<String, Provenance>,
    path: &Path,
) -> Result<()> {
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
        let benchmark_id = string_at(benchmark, "/benchmark_id", path)?;
        let Some(provenance) = provenance_by_id.get(benchmark_id) else {
            bail!(
                "{} real-derived manifest references unknown benchmark {benchmark_id}",
                path.display()
            );
        };
        validate_real_derived_provenance_fields(benchmark, benchmark_id, provenance, path)?;
    }
    Ok(())
}

fn validate_real_derived_provenance_fields(
    value: &Value,
    benchmark_id: &str,
    provenance: &Provenance,
    path: &Path,
) -> Result<()> {
    let source_blob = provenance
        .source_blob
        .as_deref()
        .with_context(|| format!("real-derived benchmark {benchmark_id} missing source_blob"))?;
    require_string_value(value, "/source_path", &provenance.source_path, path)?;
    let source_reference_path = provenance
        .upstream_reference_path(benchmark_id)
        .to_string_lossy()
        .into_owned();
    require_string_value(
        value,
        "/source_reference_path",
        &source_reference_path,
        path,
    )?;
    require_string_value(value, "/source_blob", source_blob, path)?;
    require_string_array_value(value, "/source_profiles", &provenance.source_profiles, path)?;
    Ok(())
}

fn manifest_profile_metadata(
    value: &Value,
    path: &Path,
) -> Result<BTreeMap<String, (String, String)>> {
    let profiles = value
        .pointer("/profiles")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{} profiles must be an array", path.display()))?;
    let mut metadata = BTreeMap::new();
    for profile in profiles {
        let id = string_at(profile, "/id", path)?;
        let language = string_at(profile, "/language", path)?;
        let source_variant = string_at(profile, "/source_variant", path)?;
        if metadata
            .insert(
                id.to_string(),
                (language.to_string(), source_variant.to_string()),
            )
            .is_some()
        {
            bail!("{} duplicate manifest profile id {id}", path.display());
        }
    }
    Ok(metadata)
}

fn validate_real_derived_source_variant_profile(
    variant: &Value,
    profile_metadata: &BTreeMap<String, (String, String)>,
    path: &Path,
) -> Result<()> {
    let profile_id = string_at(variant, "/profile_id", path)?;
    let language = string_at(variant, "/language", path)?;
    let source_variant = string_at(variant, "/source_variant", path)?;
    let Some((profile_language, profile_source_variant)) = profile_metadata.get(profile_id) else {
        bail!(
            "{} real-derived source variant references unknown profile {profile_id}",
            path.display()
        );
    };
    if language != profile_language {
        bail!(
            "{} real-derived source variant profile {profile_id} has language {language}, expected {profile_language}",
            path.display()
        );
    }
    if source_variant != profile_source_variant {
        bail!(
            "{} real-derived source variant profile {profile_id} has source_variant {source_variant}, expected {profile_source_variant}",
            path.display()
        );
    }
    Ok(())
}

fn validate_real_derived_source_variant_language(
    variant: &Value,
    source_language: &str,
    path: &Path,
) -> Result<()> {
    let language = string_at(variant, "/language", path)?;
    let profile_id = string_at(variant, "/profile_id", path)?;
    if language == source_language {
        validate_real_derived_source_profile_language(
            source_language,
            &[Value::String(profile_id.to_string())],
            path,
            "manifest source variant",
        )?;
    }
    Ok(())
}

fn validate_real_derived_manifest_lanes(benchmark: &Value, path: &Path) -> Result<()> {
    let comparison_lane = string_at(benchmark, "/comparison_lane", path)?;
    let source_lane = string_at(benchmark, "/source_lane", path)?;
    if comparison_lane == "production_conformance" && source_lane != "latest_syntax_original" {
        bail!(
            "{} production_conformance real-derived manifest entry requires latest_syntax_original source_lane",
            path.display()
        );
    }
    if comparison_lane == "latest_idiomatic" && source_lane != "latest_idiomatic" {
        bail!(
            "{} latest_idiomatic real-derived manifest entry requires latest_idiomatic source_lane",
            path.display()
        );
    }
    if matches!(
        source_lane,
        "production_conformance" | "fixture_scoped_port" | "diagnostic_layout_matched"
    ) {
        bail!(
            "{} real-derived manifest source_lane must identify an original source lane",
            path.display()
        );
    }
    Ok(())
}

fn validate_real_derived_manifest_source_profiles(benchmark: &Value, path: &Path) -> Result<()> {
    let profiles = real_derived_source_profiles(benchmark, path)?;
    let source_language = string_at(benchmark, "/source_language", path)?;
    let expected_language_prefix = match source_language {
        "solidity" => "solc",
        "vyper" => "vyper",
        other => bail!(
            "{} unsupported real-derived source_language {other}",
            path.display()
        ),
    };
    for profile in profiles {
        let profile = profile.as_str().with_context(|| {
            format!(
                "{} JSON pointer /source_profiles must be a non-empty string array",
                path.display()
            )
        })?;
        if !profile.starts_with(expected_language_prefix) {
            bail!(
                "{} real-derived manifest source profile {profile} must match source language {source_language}",
                path.display()
            );
        }
    }
    Ok(())
}

fn real_derived_source_profiles<'a>(benchmark: &'a Value, path: &Path) -> Result<&'a Vec<Value>> {
    let profiles = benchmark
        .pointer("/source_profiles")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} JSON pointer /source_profiles must be a non-empty string array",
                path.display()
            )
        })?;
    if profiles.is_empty() {
        bail!(
            "{} JSON pointer /source_profiles must be a non-empty string array",
            path.display()
        );
    }
    Ok(profiles)
}

fn validate_real_derived_manifest_excluded_features(benchmark: &Value, path: &Path) -> Result<()> {
    let production_equivalence = benchmark
        .pointer("/production_equivalence")
        .and_then(|value| value.as_bool())
        .with_context(|| {
            format!(
                "{} JSON pointer /production_equivalence must be a boolean",
                path.display()
            )
        })?;
    let excluded_features = benchmark
        .pointer("/excluded_features")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} JSON pointer /excluded_features must be a string array",
                path.display()
            )
        })?;
    if !excluded_features.iter().all(|item| item.is_string()) {
        bail!(
            "{} JSON pointer /excluded_features must be a string array",
            path.display()
        );
    }
    if production_equivalence && !excluded_features.is_empty() {
        bail!(
            "{} production-equivalent real-derived manifest entry must not list excluded_features",
            path.display()
        );
    }
    if !production_equivalence && excluded_features.is_empty() {
        bail!(
            "{} non-production-equivalent real-derived manifest entry must explain excluded_features",
            path.display()
        );
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
    validate_source_profiles(root, path, real, provenance)?;
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
    validate_source_language_implementation(root, path, benchmark, provenance)?;
    validate_source_blob(root, path, benchmark, provenance)?;
    Ok(())
}

fn validate_real_derived_lanes(path: &Path, provenance: &Provenance) -> Result<()> {
    if matches!(
        provenance.source_lane,
        ComparisonLane::ProductionConformance
            | ComparisonLane::FixtureScopedPort
            | ComparisonLane::DiagnosticLayoutMatched
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

fn validate_source_language_implementation(
    root: &Path,
    path: &Path,
    benchmark: &Benchmark,
    provenance: &Provenance,
) -> Result<()> {
    let implementation = match provenance.source_language {
        crate::models::Language::Solidity => &benchmark.solidity_path,
        crate::models::Language::Vyper => &benchmark.vyper_path,
    };
    let implementation_path = Path::new(implementation);

    if matches!(
        provenance.source_lane,
        ComparisonLane::LatestSyntaxOriginal | ComparisonLane::LatestIdiomatic
    ) {
        let source_lane = provenance.source_lane.as_str();
        if path_has_component(implementation_path, "upstream") {
            bail!(
                "{} {source_lane} source-language implementation must not compile from upstream reference path {}",
                path.display(),
                implementation
            );
        }

        let upstream_reference = provenance.upstream_reference_path(&benchmark.id);
        if implementation_path == upstream_reference {
            bail!(
                "{} {source_lane} source-language implementation must be distinct from upstream reference path {}",
                path.display(),
                upstream_reference.display()
            );
        }

        let source = fs::read_to_string(root.join(implementation_path)).with_context(|| {
            format!(
                "reading latest source-language implementation {}",
                implementation
            )
        })?;
        match provenance.source_language {
            crate::models::Language::Solidity if !source.contains(LATEST_SOLIDITY_PRAGMA) => {
                bail!(
                    "{} {source_lane} Solidity implementation {} must use `{LATEST_SOLIDITY_PRAGMA}`",
                    path.display(),
                    implementation
                );
            }
            crate::models::Language::Vyper if !source.contains(LATEST_VYPER_PRAGMA) => {
                bail!(
                    "{} {source_lane} Vyper implementation {} must use `{LATEST_VYPER_PRAGMA}`",
                    path.display(),
                    implementation
                );
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_source_profiles(
    root: &Path,
    path: &Path,
    real: &serde_yaml::Value,
    provenance: &Provenance,
) -> Result<()> {
    let profile_languages = compiler_profile_languages(root)?;
    let profiles = real
        .get("source_profiles")
        .and_then(|value| value.as_sequence())
        .with_context(|| format!("{} missing source_profiles", path.display()))?;
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
        let Some(profile_language) = profile_languages.get(profile) else {
            bail!(
                "{} source profile {profile} does not match any compiler profile",
                path.display()
            );
        };
        if *profile_language != provenance.source_language {
            bail!(
                "{} source profile {profile} has language {}, expected {}",
                path.display(),
                profile_language.as_str(),
                provenance.source_language.as_str()
            );
        }
    }
    Ok(())
}

fn source_profile_prefix(source_language: &str, path: &Path) -> Result<&'static str> {
    match source_language {
        "solidity" => Ok("solc"),
        "vyper" => Ok("vyper"),
        other => bail!(
            "{} unsupported real-derived source_language {other}",
            path.display()
        ),
    }
}

fn validate_real_derived_source_profile_language(
    source_language: &str,
    profiles: &[Value],
    path: &Path,
    context: &str,
) -> Result<()> {
    let expected_prefix = source_profile_prefix(source_language, path)?;
    for profile in profiles {
        let profile = profile.as_str().with_context(|| {
            format!(
                "{} JSON pointer /provenance/source_profiles must be a string array",
                path.display()
            )
        })?;
        if !profile.starts_with(expected_prefix) {
            bail!(
                "{} real-derived {context} source profile {profile} must match source language {source_language}",
                path.display()
            );
        }
    }
    Ok(())
}

fn compiler_profile_languages(root: &Path) -> Result<BTreeMap<String, crate::models::Language>> {
    let mut profiles = BTreeMap::new();
    for entry in fs::read_dir(root.join("compiler-profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let profile: crate::models::CompilerProfile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if profiles
            .insert(profile.id.clone(), profile.language)
            .is_some()
        {
            bail!("duplicate compiler profile id {}", profile.id);
        }
    }
    Ok(profiles)
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
    require_string_pointer(row, "/compiler/settings/sourceVariant", path)?;
    require_enum(
        row,
        "/compiler/settings/sourceVariant",
        SOURCE_VARIANT_LABELS,
        path,
    )?;
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
                "/provenance/implementation_lane",
                "/provenance/source_lane",
                "/provenance/counterpart_lane",
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
            for pointer in [
                "/provenance/implementation_lane",
                "/provenance/source_lane",
                "/provenance/counterpart_lane",
            ] {
                require_enum(
                    row,
                    pointer,
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
            }
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
            validate_real_derived_row_lanes(row, path)?;
            validate_real_derived_row_source_profiles(row, path)?;
            validate_real_derived_row_source_path(row, path)?;
            validate_real_derived_excluded_features(row, path)?;
        }
        Some(other) => bail!("{} unsupported suite {other}", path.display()),
        None => bail!("{} missing suite", path.display()),
    }
    Ok(())
}

fn validate_real_derived_row_lanes(row: &Value, path: &Path) -> Result<()> {
    let comparison_lane = string_at(row, "/provenance/comparison_lane", path)?;
    let source_lane = string_at(row, "/provenance/source_lane", path)?;
    let implementation_lane = string_at(row, "/provenance/implementation_lane", path)?;
    let source_language = string_at(row, "/provenance/source_language", path)?;
    let port_language = string_at(row, "/provenance/port_language", path)?;

    if comparison_lane == "production_conformance" && source_lane != "latest_syntax_original" {
        bail!(
            "{} production_conformance real-derived row requires latest_syntax_original source_lane",
            path.display()
        );
    }
    if comparison_lane == "latest_idiomatic" && source_lane != "latest_idiomatic" {
        bail!(
            "{} latest_idiomatic real-derived row requires latest_idiomatic source_lane",
            path.display()
        );
    }
    if matches!(
        source_lane,
        "production_conformance" | "fixture_scoped_port" | "diagnostic_layout_matched"
    ) {
        bail!(
            "{} real-derived row source_lane must identify an original source lane",
            path.display()
        );
    }

    let expected_implementation_lane = if port_language == source_language {
        source_lane
    } else {
        string_at(row, "/provenance/counterpart_lane", path)?
    };
    if implementation_lane != expected_implementation_lane {
        bail!(
            "{} real-derived row implementation_lane {implementation_lane} does not match language side {expected_implementation_lane}",
            path.display()
        );
    }
    Ok(())
}

fn validate_real_derived_row_source_profiles(row: &Value, path: &Path) -> Result<()> {
    let source_language = string_at(row, "/provenance/source_language", path)?;
    let profiles = row
        .pointer("/provenance/source_profiles")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} JSON pointer /provenance/source_profiles must be a string array",
                path.display()
            )
        })?;
    validate_real_derived_source_profile_language(source_language, profiles, path, "row")
}

fn validate_real_derived_row_matches_catalog(
    row: &Value,
    provenance_by_id: &BTreeMap<String, Provenance>,
    path: &Path,
) -> Result<()> {
    if row.pointer("/suite").and_then(|value| value.as_str()) != Some("real_derived") {
        return Ok(());
    }
    let benchmark_id = string_at(row, "/benchmark_id", path)?;
    let Some(provenance) = provenance_by_id.get(benchmark_id) else {
        bail!(
            "{} real-derived row references unknown benchmark {benchmark_id}",
            path.display()
        );
    };
    let source_blob = provenance
        .source_blob
        .as_deref()
        .with_context(|| format!("real-derived benchmark {benchmark_id} missing source_blob"))?;
    require_string_value(
        row,
        "/provenance/source_path",
        &provenance.source_path,
        path,
    )?;
    let source_reference_path = provenance
        .upstream_reference_path(benchmark_id)
        .to_string_lossy()
        .into_owned();
    require_string_value(
        row,
        "/provenance/source_reference_path",
        &source_reference_path,
        path,
    )?;
    require_string_value(row, "/provenance/source_blob", source_blob, path)?;
    require_string_array_value(
        row,
        "/provenance/source_profiles",
        &provenance.source_profiles,
        path,
    )?;
    Ok(())
}

fn validate_real_derived_excluded_features(row: &Value, path: &Path) -> Result<()> {
    let production_equivalence = row
        .pointer("/provenance/production_equivalence")
        .and_then(|value| value.as_bool())
        .with_context(|| {
            format!(
                "{} JSON pointer /provenance/production_equivalence must be a boolean",
                path.display()
            )
        })?;
    let excluded_features = row
        .pointer("/provenance/excluded_features")
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} JSON pointer /provenance/excluded_features must be a string array",
                path.display()
            )
        })?;
    if !excluded_features.iter().all(|item| item.is_string()) {
        bail!(
            "{} JSON pointer /provenance/excluded_features must be a string array",
            path.display()
        );
    }
    if production_equivalence && !excluded_features.is_empty() {
        bail!(
            "{} production-equivalent real-derived row must not list excluded_features",
            path.display()
        );
    }
    if !production_equivalence && excluded_features.is_empty() {
        bail!(
            "{} non-production-equivalent real-derived row must explain excluded_features",
            path.display()
        );
    }
    Ok(())
}

fn string_at<'a>(value: &'a Value, pointer: &str, path: &Path) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(|value| value.as_str())
        .with_context(|| format!("{} JSON pointer {pointer} must be a string", path.display()))
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

fn require_string_value(value: &Value, pointer: &str, expected: &str, path: &Path) -> Result<()> {
    let actual = string_at(value, pointer, path)?;
    if actual != expected {
        bail!(
            "{} JSON pointer {pointer} has value {actual}, expected {expected}",
            path.display()
        );
    }
    Ok(())
}

fn require_string_array_value(
    value: &Value,
    pointer: &str,
    expected: &[String],
    path: &Path,
) -> Result<()> {
    let actual = value
        .pointer(pointer)
        .and_then(|value| value.as_array())
        .with_context(|| {
            format!(
                "{} JSON pointer {pointer} must be a string array",
                path.display()
            )
        })?;
    let actual: Result<Vec<_>> = actual
        .iter()
        .map(|item| {
            item.as_str().with_context(|| {
                format!(
                    "{} JSON pointer {pointer} must be a string array",
                    path.display()
                )
            })
        })
        .collect();
    let actual = actual?;
    let expected: Vec<_> = expected.iter().map(String::as_str).collect();
    if actual != expected {
        bail!(
            "{} JSON pointer {pointer} has value {actual:?}, expected {expected:?}",
            path.display()
        );
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

fn json_schema_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
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
        super::validate_compiler_profile_source_variants(root).unwrap();
        let (config, _) = crate::scale::load_scale_config(root).unwrap();
        assert_eq!(config.families.len(), 7);
        let generated_count = super::validate_generated_outputs_if_present(root, &config).unwrap();
        let expected_generated_count = config.families.len() * config.values.len();
        assert!(generated_count == 0 || generated_count == expected_generated_count);
    }

    #[test]
    fn validates_real_derived_excluded_feature_invariant() {
        let path = Path::new("results/normalized/results.json");
        let complete = json!({
            "provenance": {
                "production_equivalence": true,
                "excluded_features": []
            }
        });
        let incomplete = json!({
            "provenance": {
                "production_equivalence": false,
                "excluded_features": ["factory fixture"]
            }
        });
        let contradictory_complete = json!({
            "provenance": {
                "production_equivalence": true,
                "excluded_features": ["factory fixture"]
            }
        });
        let contradictory_incomplete = json!({
            "provenance": {
                "production_equivalence": false,
                "excluded_features": []
            }
        });

        super::validate_real_derived_excluded_features(&complete, path).unwrap();
        super::validate_real_derived_excluded_features(&incomplete, path).unwrap();
        assert!(
            super::validate_real_derived_excluded_features(&contradictory_complete, path).is_err()
        );
        assert!(
            super::validate_real_derived_excluded_features(&contradictory_incomplete, path)
                .is_err()
        );
    }

    #[test]
    fn validates_real_derived_row_lane_consistency() {
        let path = Path::new("results/normalized/results.json");
        let row = json!({
            "provenance": {
                "comparison_lane": "production_conformance",
                "source_lane": "latest_syntax_original",
                "counterpart_lane": "fixture_scoped_port",
                "implementation_lane": "latest_syntax_original",
                "source_language": "solidity",
                "port_language": "solidity"
            }
        });
        let counterpart_row = json!({
            "provenance": {
                "comparison_lane": "production_conformance",
                "source_lane": "latest_syntax_original",
                "counterpart_lane": "fixture_scoped_port",
                "implementation_lane": "fixture_scoped_port",
                "source_language": "solidity",
                "port_language": "vyper"
            }
        });
        let stale_row = json!({
            "provenance": {
                "comparison_lane": "production_conformance",
                "source_lane": "latest_syntax_original",
                "counterpart_lane": "fixture_scoped_port",
                "implementation_lane": "production_conformance",
                "source_language": "solidity",
                "port_language": "vyper"
            }
        });
        let historical_source = json!({
            "provenance": {
                "comparison_lane": "production_conformance",
                "source_lane": "upstream_exact_historical",
                "counterpart_lane": "fixture_scoped_port",
                "implementation_lane": "upstream_exact_historical",
                "source_language": "solidity",
                "port_language": "solidity"
            }
        });
        let comparison_as_source_lane = json!({
            "provenance": {
                "comparison_lane": "latest_idiomatic",
                "source_lane": "production_conformance",
                "counterpart_lane": "fixture_scoped_port",
                "implementation_lane": "production_conformance",
                "source_language": "solidity",
                "port_language": "solidity"
            }
        });

        super::validate_real_derived_row_lanes(&row, path).unwrap();
        super::validate_real_derived_row_lanes(&counterpart_row, path).unwrap();
        assert!(super::validate_real_derived_row_lanes(&stale_row, path).is_err());
        assert!(super::validate_real_derived_row_lanes(&historical_source, path).is_err());
        assert!(super::validate_real_derived_row_lanes(&comparison_as_source_lane, path).is_err());
    }

    #[test]
    fn validates_source_profile_language_in_real_derived_rows() {
        let path = Path::new("results/normalized/results.json");
        let row = json!({
            "provenance": {
                "source_lane": "latest_syntax_original",
                "source_language": "vyper",
                "source_profiles": [
                    "vyper-latest-gas",
                    "vyper-0.3.10-gas"
                ]
            }
        });
        let wrong_language_row = json!({
            "provenance": {
                "source_lane": "latest_syntax_original",
                "source_language": "vyper",
                "source_profiles": [
                    "vyper-latest-gas",
                    "solc-latest-noopt"
                ]
            }
        });
        let latest_idiomatic_row = json!({
            "provenance": {
                "source_lane": "latest_idiomatic",
                "source_language": "vyper",
                "source_profiles": [
                    "vyper-latest-gas",
                    "vyper-0.3.10-gas"
                ]
            }
        });

        super::validate_real_derived_row_source_profiles(&row, path).unwrap();
        super::validate_real_derived_row_source_profiles(&latest_idiomatic_row, path).unwrap();
        assert!(
            super::validate_real_derived_row_source_profiles(&wrong_language_row, path).is_err()
        );
    }

    #[test]
    fn validates_real_derived_manifest_lane_and_profile_invariants() {
        let path = Path::new("results/normalized/run-manifest.json");
        let manifest = json!({
            "profiles": [
                {
                    "id": "solc-latest-noopt",
                    "language": "solidity",
                    "compiler": "solc",
                    "source_variant": "latest"
                },
                {
                    "id": "solc-0.8.20-noopt",
                    "language": "solidity",
                    "compiler": "solc-0.8.20",
                    "source_variant": "solidity-0.8"
                },
                {
                    "id": "solc-0.5.16-noopt",
                    "language": "solidity",
                    "compiler": "solc-0.5.16",
                    "source_variant": "solidity-0.5"
                },
                {
                    "id": "vyper-latest-none",
                    "language": "vyper",
                    "compiler": "vyper",
                    "source_variant": "latest"
                }
            ],
            "real_derived": {
                "benchmarks": [
                    {
                        "benchmark_id": "uniswap_v2_pair",
                        "comparison_lane": "production_conformance",
                        "source_lane": "latest_syntax_original",
                        "counterpart_lane": "fixture_scoped_port",
                        "source_language": "solidity",
                        "source_profiles": ["solc-latest-noopt"],
                        "source_path": "contracts/UniswapV2Pair.sol",
                        "source_reference_path": "benches/implementations/uniswap_v2_pair/solidity/upstream/contracts/UniswapV2Pair.sol",
                        "source_blob": "f87a1db262fba132862eae377d8cdaef74c79f97",
                        "production_equivalence": false,
                        "excluded_features": ["factory fixture"],
                        "source_variants": [
                            {
                                "language": "solidity",
                                "implementation_id": "solidity/handwritten/v1",
                                "profile_id": "solc-latest-noopt",
                                "source_variant": "latest",
                                "source_path": "target/bench-source-variants/solc-latest-noopt/Pair.sol",
                                "source_hash": "abc",
                                "compile_status": "ok"
                            }
                        ]
                    }
                ]
            }
        });
        let mut compatibility_profile = manifest.clone();
        *compatibility_profile
            .pointer_mut("/real_derived/benchmarks/0/source_profiles/0")
            .unwrap() = json!("solc-0.5.16-noopt");
        *compatibility_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/profile_id")
            .unwrap() = json!("solc-0.5.16-noopt");
        *compatibility_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_variant")
            .unwrap() = json!("solidity-0.5");
        *compatibility_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_path")
            .unwrap() = json!("target/bench-source-variants/solc-0.5.16-noopt/Pair.sol");
        let mut wrong_language_profile = manifest.clone();
        *wrong_language_profile
            .pointer_mut("/real_derived/benchmarks/0/source_profiles/0")
            .unwrap() = json!("vyper-latest-none");
        let mut stale_lane = manifest.clone();
        *stale_lane
            .pointer_mut("/real_derived/benchmarks/0/source_lane")
            .unwrap() = json!("upstream_exact_historical");
        let mut comparison_as_source_lane = manifest.clone();
        *comparison_as_source_lane
            .pointer_mut("/real_derived/benchmarks/0/source_lane")
            .unwrap() = json!("production_conformance");
        let mut contradictory_equivalence = manifest.clone();
        *contradictory_equivalence
            .pointer_mut("/real_derived/benchmarks/0/production_equivalence")
            .unwrap() = json!(true);
        let mut unknown_variant_profile = manifest.clone();
        *unknown_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/profile_id")
            .unwrap() = json!("solc-missing-noopt");
        let mut wrong_variant_language = manifest.clone();
        *wrong_variant_language
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/language")
            .unwrap() = json!("vyper");
        let mut wrong_variant_label = manifest.clone();
        *wrong_variant_label
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_variant")
            .unwrap() = json!("solidity-0.5");
        let mut unknown_variant_label = manifest.clone();
        *unknown_variant_label
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_variant")
            .unwrap() = json!("solidity-experimental");
        let mut undeclared_source_variant_profile = manifest.clone();
        *undeclared_source_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/profile_id")
            .unwrap() = json!("solc-0.8.20-noopt");
        *undeclared_source_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_variant")
            .unwrap() = json!("solidity-0.8");
        let mut counterpart_variant_profile = manifest.clone();
        *counterpart_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/language")
            .unwrap() = json!("vyper");
        *counterpart_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/implementation_id")
            .unwrap() = json!("vyper/handwritten/v1");
        *counterpart_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/profile_id")
            .unwrap() = json!("vyper-latest-none");
        *counterpart_variant_profile
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_path")
            .unwrap() = json!("target/bench-source-variants/vyper-latest-none/Pair.vy");
        let mut upstream_variant_path = manifest.clone();
        *upstream_variant_path
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_path")
            .unwrap() = json!(
            "benches/implementations/uniswap_v2_pair/solidity/upstream/contracts/UniswapV2Pair.sol"
        );
        let mut escaped_variant_path = manifest.clone();
        *escaped_variant_path
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_path")
            .unwrap() = json!("target/bench-source-variants/solc-latest-noopt/../Pair.sol");
        let mut wrong_profile_variant_path = manifest.clone();
        *wrong_profile_variant_path
            .pointer_mut("/real_derived/benchmarks/0/source_variants/0/source_path")
            .unwrap() = json!("target/bench-source-variants/solc-0.5.16-noopt/Pair.sol");
        let mut duplicate_variant = manifest.clone();
        let duplicate_entry = duplicate_variant
            .pointer("/real_derived/benchmarks/0/source_variants/0")
            .unwrap()
            .clone();
        duplicate_variant
            .pointer_mut("/real_derived/benchmarks/0/source_variants")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(duplicate_entry);

        super::validate_real_derived_manifest(&manifest, path).unwrap();
        super::validate_real_derived_manifest(&compatibility_profile, path).unwrap();
        super::validate_real_derived_manifest(&counterpart_variant_profile, path).unwrap();
        assert!(super::validate_real_derived_manifest(&wrong_language_profile, path).is_err());
        assert!(super::validate_real_derived_manifest(&stale_lane, path).is_err());
        assert!(super::validate_real_derived_manifest(&comparison_as_source_lane, path).is_err());
        assert!(super::validate_real_derived_manifest(&contradictory_equivalence, path).is_err());
        assert!(super::validate_real_derived_manifest(&unknown_variant_profile, path).is_err());
        assert!(super::validate_real_derived_manifest(&wrong_variant_language, path).is_err());
        assert!(super::validate_real_derived_manifest(&wrong_variant_label, path).is_err());
        assert!(super::validate_real_derived_manifest(&unknown_variant_label, path).is_err());
        assert!(
            super::validate_real_derived_manifest(&undeclared_source_variant_profile, path)
                .is_err()
        );
        assert!(super::validate_real_derived_manifest(&upstream_variant_path, path).is_err());
        assert!(super::validate_real_derived_manifest(&escaped_variant_path, path).is_err());
        assert!(super::validate_real_derived_manifest(&wrong_profile_variant_path, path).is_err());
        assert!(super::validate_real_derived_manifest(&duplicate_variant, path).is_err());
    }

    #[test]
    fn requires_manifest_profiles_to_report_effective_source_variant() {
        let path = Path::new("results/normalized/run-manifest.json");
        let manifest = json!({
            "profiles": [
                {
                    "id": "solc-latest-noopt",
                    "language": "solidity",
                    "compiler": "solc",
                    "source_variant": "latest"
                }
            ]
        });
        let missing_variant = json!({
            "profiles": [
                {
                    "id": "solc-latest-noopt",
                    "language": "solidity",
                    "compiler": "solc"
                }
            ]
        });
        let unknown_variant = json!({
            "profiles": [
                {
                    "id": "solc-latest-noopt",
                    "language": "solidity",
                    "compiler": "solc",
                    "source_variant": "solidity-experimental"
                }
            ]
        });

        super::validate_manifest_profiles(&manifest, path).unwrap();
        assert!(super::validate_manifest_profiles(&missing_variant, path).is_err());
        assert!(super::validate_manifest_profiles(&unknown_variant, path).is_err());
    }

    #[test]
    fn requires_result_rows_to_report_effective_source_variant() {
        let path = Path::new("results/normalized/results.json");
        let row = json!({
            "status": "ok",
            "compile": { "status": "ok" },
            "bytecode": { "runtime_bytes": 1 },
            "gas": {
                "scenario": "noop",
                "evm_fork": "prague",
                "deployment_variant": "standard",
                "state_access_profile": "cold",
                "metadata_mode": "off",
                "internal_create_gas": 0,
                "harness_call_gas": 0,
                "intrinsic_gas": 21000,
                "calldata_gas": 0,
                "harness_estimated_tx_gas": 21000,
                "expected_success": true,
                "call_succeeded": true,
                "scenario_status_ok": true,
                "measurement_scope": "foundry_internal_call_harness",
                "total_tx_gas": null
            },
            "cache": {
                "gas": { "status": "disabled" }
            },
            "compiler": {
                "settings": {
                    "metadataMode": "off",
                    "sourceVariant": "latest"
                }
            }
        });
        let mut missing_variant = row.clone();
        missing_variant
            .pointer_mut("/compiler/settings")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("sourceVariant");
        let mut unknown_variant = row.clone();
        *unknown_variant
            .pointer_mut("/compiler/settings/sourceVariant")
            .unwrap() = json!("solidity-experimental");

        super::validate_row_status(&row, path).unwrap();
        assert!(super::validate_row_status(&missing_variant, path).is_err());
        assert!(super::validate_row_status(&unknown_variant, path).is_err());
    }

    #[test]
    fn accepts_compatibility_profiles_for_latest_source_lanes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let benchmark = crate::catalog::real_derived_benchmarks()
            .into_iter()
            .find(|benchmark| benchmark.id == "uniswap_v2_pair")
            .unwrap();
        let provenance = benchmark.provenance.clone().unwrap();
        let real = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
source_profiles:
  - solc-latest-noopt
  - solc-0.5.16-noopt
"#,
        )
        .unwrap();

        super::validate_source_profiles(
            root,
            Path::new("benches/specs/uniswap_v2_pair.yaml"),
            &real,
            &provenance,
        )
        .unwrap();
    }

    #[test]
    fn rejects_stale_real_derived_output_provenance() {
        let path = Path::new("results/normalized/run-manifest.json");
        let provenance_by_id = super::real_derived_provenance_by_id();
        let provenance = provenance_by_id.get("uniswap_v2_pair").unwrap();
        let mut manifest = json!({
            "real_derived": {
                "benchmarks": [
                    {
                        "benchmark_id": "uniswap_v2_pair",
                        "source_path": &provenance.source_path,
                        "source_reference_path": provenance
                            .upstream_reference_path("uniswap_v2_pair")
                            .to_string_lossy(),
                        "source_blob": provenance.source_blob.as_deref().unwrap(),
                        "source_profiles": &provenance.source_profiles
                    }
                ]
            }
        });

        super::validate_real_derived_manifest_matches_catalog(&manifest, &provenance_by_id, path)
            .unwrap();
        manifest
            .pointer_mut("/real_derived/benchmarks/0/source_profiles")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(
            super::validate_real_derived_manifest_matches_catalog(
                &manifest,
                &provenance_by_id,
                path
            )
            .is_err()
        );
    }

    #[test]
    fn validates_report_model_source_methodology_and_compiled_sources() {
        let path = Path::new("results/normalized/report-model.json");
        let provenance_by_id = super::real_derived_provenance_by_id();
        let provenance = provenance_by_id.get("uniswap_v2_pair").unwrap();
        let mut report_model = json!({
            "schema_version": 1,
            "generated_at": "2026-05-25T00:00:00Z",
            "methodology": {
                "source_model": {
                    "real_derived": "Production-conformance rows use latest-syntax source-language originals plus counterpart-language ports. Pinned upstream files are provenance references, not compiled headline artifacts.",
                    "compatibility_variants": "Older source-language profiles compile generated variants of the checked-in latest source. Version pragmas are rewritten to the resolved compiler patch range before supported backward syntax rewrites are applied.",
                    "compiled_source_root": "target/bench-source-variants/<profile_id>/"
                },
                "notes": [
                    {
                        "tag": "F",
                        "title": "Real-derived provenance",
                        "body": "Production-conformance rows use latest-syntax originals plus counterpart-language ports."
                    },
                    {
                        "tag": "G",
                        "title": "Compatibility source variants",
                        "body": "Older source-language profiles compile generated variants of the checked-in latest source."
                    }
                ]
            },
            "manifest": {
                "profiles": [
                    {
                        "id": "solc-latest-noopt",
                        "language": "solidity",
                        "compiler": "solc",
                        "source_variant": "latest"
                    }
                ],
                "real_derived": {
                    "benchmarks": [
                        {
                            "benchmark_id": "uniswap_v2_pair",
                            "comparison_lane": "production_conformance",
                            "source_lane": "latest_syntax_original",
                            "counterpart_lane": "fixture_scoped_port",
                            "source_language": "solidity",
                            "source_profiles": &provenance.source_profiles,
                            "source_path": &provenance.source_path,
                            "source_reference_path": provenance
                                .upstream_reference_path("uniswap_v2_pair")
                                .to_string_lossy(),
                            "source_blob": provenance.source_blob.as_deref().unwrap(),
                            "production_equivalence": false,
                            "excluded_features": ["factory fixture"],
                            "source_variants": [
                                {
                                    "language": "solidity",
                                    "implementation_id": "solidity/handwritten/v1",
                                    "profile_id": "solc-latest-noopt",
                                    "source_variant": "latest",
                                    "source_path": "target/bench-source-variants/solc-latest-noopt/Pair.sol",
                                    "source_hash": "abc",
                                    "compile_status": "ok"
                                }
                            ]
                        }
                    ]
                }
            },
            "real_derived_models": [
                {
                    "benchmark_id": "uniswap_v2_pair",
                    "provenance": {
                        "source_path": &provenance.source_path,
                        "source_reference_path": provenance
                            .upstream_reference_path("uniswap_v2_pair")
                            .to_string_lossy(),
                        "source_blob": provenance.source_blob.as_deref().unwrap(),
                        "source_profiles": &provenance.source_profiles
                    },
                    "compiled_sources": [
                        {
                            "language": "solidity",
                            "implementation_id": "solidity/handwritten/v1",
                            "profile_id": "solc-latest-noopt",
                            "source_variant": "latest",
                            "source_path": "target/bench-source-variants/solc-latest-noopt/Pair.sol",
                            "source_hash": "abc"
                        }
                    ]
                }
            ],
            "rows": []
        });
        super::validate_report_model(&report_model, &provenance_by_id, path).unwrap();

        let mut missing_methodology = report_model.clone();
        missing_methodology
            .as_object_mut()
            .unwrap()
            .remove("methodology");
        assert!(
            super::validate_report_model(&missing_methodology, &provenance_by_id, path).is_err()
        );

        let mut mismatched_compiled_source = report_model.clone();
        *mismatched_compiled_source
            .pointer_mut("/real_derived_models/0/compiled_sources/0/source_hash")
            .unwrap() = json!("def");
        assert!(
            super::validate_report_model(&mismatched_compiled_source, &provenance_by_id, path)
                .is_err()
        );

        *report_model
            .pointer_mut("/real_derived_models/0/compiled_sources/0/source_path")
            .unwrap() = json!(
            "benches/implementations/uniswap_v2_pair/solidity/upstream/contracts/UniswapV2Pair.sol"
        );
        assert!(super::validate_report_model(&report_model, &provenance_by_id, path).is_err());
    }

    #[test]
    fn rejects_cross_language_profiles_for_source_lanes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let benchmark = crate::catalog::real_derived_benchmarks()
            .into_iter()
            .find(|benchmark| benchmark.id == "uniswap_v2_pair")
            .unwrap();
        let provenance = benchmark.provenance.clone().unwrap();
        let real = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
source_profiles:
  - solc-latest-noopt
  - vyper-latest-none
"#,
        )
        .unwrap();

        let err = super::validate_source_profiles(
            root,
            Path::new("benches/specs/uniswap_v2_pair.yaml"),
            &real,
            &provenance,
        )
        .unwrap_err();

        assert!(err.to_string().contains("must match source language"));
    }

    #[test]
    fn rejects_latest_source_lanes_compiled_from_upstream_reference() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let mut benchmark = crate::catalog::real_derived_benchmarks()
            .into_iter()
            .find(|benchmark| benchmark.id == "uniswap_v2_pair")
            .unwrap();
        let mut provenance = benchmark.provenance.clone().unwrap();
        benchmark.solidity_path =
            "benches/implementations/uniswap_v2_pair/solidity/upstream/contracts/UniswapV2Pair.sol"
                .to_string();

        let err = super::validate_source_language_implementation(
            root,
            Path::new("benches/specs/uniswap_v2_pair.yaml"),
            &benchmark,
            &provenance,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("must not compile from upstream reference path")
        );

        provenance.source_lane = crate::models::ComparisonLane::LatestIdiomatic;
        let err = super::validate_source_language_implementation(
            root,
            Path::new("benches/specs/uniswap_v2_pair.yaml"),
            &benchmark,
            &provenance,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("must not compile from upstream reference path")
        );
    }
}

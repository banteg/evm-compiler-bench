use crate::{
    cache::{self, CacheLookup},
    models::{
        Benchmark, BytecodeMetrics, CacheInfo, CommandStats, CompileFailure, CompileMetrics,
        CompileSet, CompiledArtifact, CompilerProfile, Language, MetadataMode, Toolchain,
        Toolchains,
    },
    util::{Progress, byte_len, require_success, run_measured, sha256_bytes, stripped_cbor_len},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const DEFAULT_COMPILE_SAMPLES: usize = 1;

pub fn compile_all(
    root: &Path,
    toolchains: &Toolchains,
    benchmarks: &[Benchmark],
    profile_filter: &[String],
    use_cache: bool,
) -> Result<CompileSet> {
    let profiles = load_profiles(root, profile_filter)?;
    let total_attempts = benchmarks
        .iter()
        .flat_map(|benchmark| profiles.iter().map(move |profile| (benchmark, profile)))
        .filter(|(benchmark, profile)| profile_applies_to_benchmark(benchmark, profile))
        .count();
    let mut progress = Progress::new("compile", total_attempts);
    let mut attempted = 0usize;
    let mut skipped = 0usize;
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    let mut cache_stale = 0usize;
    let mut cache_disabled = 0usize;
    let mut artifacts = Vec::new();
    let mut failures = Vec::new();
    for benchmark in benchmarks {
        for profile in &profiles {
            if !profile_applies_to_benchmark(benchmark, profile) {
                skipped += 1;
                continue;
            }
            attempted += 1;
            let toolchain = toolchain_for_profile(toolchains, profile)?;
            let evm_version = effective_evm_version(profile, toolchains);
            let cache_input =
                compile_cache_input(root, benchmark, profile, toolchain, &evm_version)
                    .with_context(|| {
                        format!(
                            "preparing compile cache key for {} {}",
                            benchmark.id, profile.id
                        )
                    })?;
            if use_cache {
                match cache::lookup::<CachedCompileResult>(
                    root,
                    "compile",
                    &cache_input.logical_id,
                    &cache_input.key,
                    &cache_input.fingerprint,
                )? {
                    CacheLookup::Hit(mut cached) => {
                        cache_hits += 1;
                        let failed = matches!(cached, CachedCompileResult::Failure(_));
                        match &mut cached {
                            CachedCompileResult::Artifact(artifact) => {
                                refresh_cached_artifact_metadata(artifact, benchmark);
                                artifact.cache = CacheInfo::hit(&cache_input.key);
                            }
                            CachedCompileResult::Failure(failure) => {
                                refresh_cached_failure_metadata(failure, benchmark);
                                failure.cache = CacheInfo::hit(&cache_input.key);
                            }
                        }
                        match cached {
                            CachedCompileResult::Artifact(artifact) => artifacts.push(artifact),
                            CachedCompileResult::Failure(failure) => failures.push(failure),
                        }
                        progress.update(
                            attempted,
                            format!(
                                "cache hit {} {}{}",
                                benchmark.id,
                                profile.id,
                                if failed { " compile_error" } else { "" }
                            ),
                        );
                        continue;
                    }
                    CacheLookup::Miss(info) => {
                        let cache_status = info.status.clone();
                        match cache_status.as_str() {
                            "stale" => cache_stale += 1,
                            _ => cache_misses += 1,
                        }
                        progress.update(
                            attempted.saturating_sub(1),
                            format!("compiling {} {} ({cache_status})", benchmark.id, profile.id),
                        );
                        let ok = compile_and_record(
                            root,
                            benchmark,
                            profile,
                            toolchain,
                            &evm_version,
                            Some((cache_input, info)),
                            &mut artifacts,
                            &mut failures,
                        )?;
                        progress.update(
                            attempted,
                            format!(
                                "{cache_status} {} {}",
                                benchmark.id,
                                if ok { "ok" } else { "compile_error" }
                            ),
                        );
                    }
                }
            } else {
                cache_disabled += 1;
                progress.update(
                    attempted.saturating_sub(1),
                    format!("compiling {} {} (cache disabled)", benchmark.id, profile.id),
                );
                let ok = compile_and_record(
                    root,
                    benchmark,
                    profile,
                    toolchain,
                    &evm_version,
                    None,
                    &mut artifacts,
                    &mut failures,
                )?;
                progress.update(
                    attempted,
                    format!(
                        "disabled {} {} {}",
                        benchmark.id,
                        profile.id,
                        if ok { "ok" } else { "compile_error" }
                    ),
                );
            }
        }
    }
    progress.finish(format!(
        "done: {} artifacts, {} failures, {} skipped; cache hit={}, miss={}, stale={}, disabled={}",
        artifacts.len(),
        failures.len(),
        skipped,
        cache_hits,
        cache_misses,
        cache_stale,
        cache_disabled
    ));
    Ok(CompileSet {
        profiles,
        artifacts,
        failures,
    })
}

fn profile_applies_to_benchmark(benchmark: &Benchmark, profile: &CompilerProfile) -> bool {
    let Some(provenance) = benchmark.provenance.as_ref() else {
        return true;
    };

    if profile.language != provenance.source_language {
        return true;
    }

    provenance
        .source_profiles
        .iter()
        .any(|source_profile| source_profile == &profile.id)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum CachedCompileResult {
    Artifact(CompiledArtifact),
    Failure(CompileFailure),
}

fn refresh_cached_artifact_metadata(artifact: &mut CompiledArtifact, benchmark: &Benchmark) {
    artifact.suite = benchmark.suite;
    artifact.family.clone_from(&benchmark.family);
    artifact
        .parameter_name
        .clone_from(&benchmark.parameter_name);
    artifact.parameter_value = benchmark.parameter_value;
    artifact.scenario_path.clone_from(&benchmark.scenario_path);
    artifact.scenario_hash.clone_from(&benchmark.scenario_hash);
    artifact
        .generator_version
        .clone_from(&benchmark.generator_version);
    artifact.provenance.clone_from(&benchmark.provenance);
}

fn refresh_cached_failure_metadata(failure: &mut CompileFailure, benchmark: &Benchmark) {
    failure.suite = benchmark.suite;
    failure.family.clone_from(&benchmark.family);
    failure.parameter_name.clone_from(&benchmark.parameter_name);
    failure.parameter_value = benchmark.parameter_value;
    failure.scenario_path.clone_from(&benchmark.scenario_path);
    failure.scenario_hash.clone_from(&benchmark.scenario_hash);
    failure
        .generator_version
        .clone_from(&benchmark.generator_version);
    failure.provenance.clone_from(&benchmark.provenance);
}

struct CompileCacheInput {
    key: String,
    logical_id: String,
    fingerprint: serde_json::Value,
}

#[allow(clippy::too_many_arguments)]
fn compile_and_record(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    evm_version: &str,
    cache_state: Option<(CompileCacheInput, CacheInfo)>,
    artifacts: &mut Vec<CompiledArtifact>,
    failures: &mut Vec<CompileFailure>,
) -> Result<bool> {
    let result = match profile.language {
        Language::Solidity => compile_solidity(root, benchmark, profile, toolchain, evm_version),
        Language::Vyper => compile_vyper(root, benchmark, profile, toolchain, evm_version),
    };
    let (cache_input, cache_info) = cache_state
        .map(|(input, info)| (Some(input), info))
        .unwrap_or((None, CacheInfo::disabled()));

    match result {
        Ok(mut artifact) => {
            artifact.cache = cache_info;
            if let Some(input) = cache_input {
                cache::store(
                    root,
                    "compile",
                    &input.logical_id,
                    &input.key,
                    &input.fingerprint,
                    &CachedCompileResult::Artifact(artifact.clone()),
                )?;
            }
            artifacts.push(artifact);
            Ok(true)
        }
        Err(error) => {
            let mut failure = compile_failure(
                root,
                benchmark,
                profile,
                toolchain,
                evm_version,
                error.to_string(),
            )?;
            failure.cache = cache_info;
            if let Some(input) = cache_input {
                cache::store(
                    root,
                    "compile",
                    &input.logical_id,
                    &input.key,
                    &input.fingerprint,
                    &CachedCompileResult::Failure(failure.clone()),
                )?;
            }
            failures.push(failure);
            Ok(false)
        }
    }
}

fn toolchain_for_profile<'a>(
    toolchains: &'a Toolchains,
    profile: &CompilerProfile,
) -> Result<&'a Toolchain> {
    toolchains
        .compilers
        .get(&profile.compiler)
        .with_context(|| {
            format!(
                "profile {} references unresolved compiler {}",
                profile.id, profile.compiler
            )
        })
}

fn effective_evm_version(profile: &CompilerProfile, toolchains: &Toolchains) -> String {
    if profile.evm_version == "latest-shared" {
        toolchains.evm_version.clone()
    } else {
        profile.evm_version.clone()
    }
}

fn compile_cache_input(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    evm_version: &str,
) -> Result<CompileCacheInput> {
    let source_path = source_path_for_profile(root, benchmark, profile, toolchain)?;
    let source = source_fingerprint(profile.language, &source_path)?;
    let compiler_settings = match profile.language {
        Language::Solidity => solidity_compiler_settings(profile, toolchain, evm_version),
        Language::Vyper => vyper_compiler_settings(profile, evm_version),
    };
    let implementation = implementation_id(profile);
    let fingerprint = json!({
        "schema": "compile-v1",
        "benchmark": {
            "id": benchmark.id,
            "contract_name": benchmark.contract_name,
            "suite": benchmark.suite.as_str(),
            "family": benchmark.family,
            "parameter_name": benchmark.parameter_name,
            "parameter_value": benchmark.parameter_value,
            "scenario_path": benchmark.scenario_path,
            "scenario_hash": benchmark.scenario_hash,
            "generator_version": benchmark.generator_version,
        },
        "implementation_id": implementation,
        "profile": profile,
        "compiler": {
            "name": toolchain.name,
            "version": toolchain.version,
            "binary_sha256": toolchain.binary_sha256,
            "download_source": toolchain.download_source,
            "metadata": toolchain.metadata,
        },
        "compiler_settings": compiler_settings,
        "source": source,
        "compile_sample_count": compile_sample_count(),
    });
    let key = cache::key_for(&fingerprint)?;
    let logical_id = cache::logical_id(&["compile", &benchmark.id, &implementation, &profile.id]);
    Ok(CompileCacheInput {
        key,
        logical_id,
        fingerprint,
    })
}

fn source_fingerprint(language: Language, source_path: &Path) -> Result<serde_json::Value> {
    match language {
        Language::Solidity => solidity_source_bundle_fingerprint(source_path),
        Language::Vyper => {
            let source = fs::read(source_path)?;
            Ok(json!({
                "path": source_path.display().to_string(),
                "hash": sha256_bytes(&source),
            }))
        }
    }
}

fn solidity_source_bundle_fingerprint(source_path: &Path) -> Result<serde_json::Value> {
    let source_root = source_path.parent().context("solidity source parent")?;
    let mut bundle = Vec::new();
    for path in solidity_files(source_root)? {
        let key = path
            .strip_prefix(source_root)?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read(&path)?;
        bundle.push(json!({
            "path": key,
            "hash": sha256_bytes(&source),
        }));
    }
    Ok(json!({
        "path": source_path.display().to_string(),
        "bundle": bundle,
    }))
}

fn load_profiles(root: &Path, profile_filter: &[String]) -> Result<Vec<CompilerProfile>> {
    let mut profiles: Vec<CompilerProfile> = Vec::new();
    for entry in fs::read_dir(root.join("compiler-profiles"))? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        let base: CompilerProfile =
            toml::from_str(&text).with_context(|| format!("parsing {}", entry.path().display()))?;
        profiles.push(no_metadata_profile(&base));
    }
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    if !profile_filter.is_empty() {
        let available = profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let requested = profile_filter
            .iter()
            .map(|profile| (profile.as_str(), ()))
            .collect::<BTreeMap<_, _>>();
        profiles.retain(|profile| requested.contains_key(profile.id.as_str()));
        if profiles.len() != requested.len() {
            let selected = profiles
                .iter()
                .map(|profile| (profile.id.as_str(), ()))
                .collect::<BTreeMap<_, _>>();
            let unknown = requested
                .keys()
                .filter(|profile| !selected.contains_key(**profile))
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            bail!("unknown compiler profile(s): {unknown}; available profiles: {available}");
        }
    }
    Ok(profiles)
}

fn no_metadata_profile(base: &CompilerProfile) -> CompilerProfile {
    let mut profile = base.clone();
    profile.metadata_mode = MetadataMode::Off;
    profile
}

fn compile_solidity(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    solc: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = source_path_for_profile(root, benchmark, profile, solc)?;
    let (file_name, sources) = solidity_sources(&source_path)?;
    let metadata_settings = solidity_metadata_settings(profile.metadata_mode, solc);
    let mut input = json!({
        "language": "Solidity",
        "sources": sources,
        "settings": {
            "evmVersion": evm_version,
            "metadata": metadata_settings,
            "optimizer": {
                "enabled": profile.optimizer,
                "runs": profile.optimizer_runs
            },
            "viaIR": profile.via_ir,
            "outputSelection": {
                "*": {
                    "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"]
                }
            }
        }
    });
    let settings = input
        .pointer_mut("/settings")
        .and_then(|value| value.as_object_mut())
        .context("solidity settings object")?;
    if metadata_settings
        .as_object()
        .is_some_and(|object| object.is_empty())
    {
        settings.remove("metadata");
    }
    if !profile.via_ir {
        settings.remove("viaIR");
    }
    let input = serde_json::to_vec(&input)?;
    let measured = repeat_compile_samples(
        || {
            let mut command = Command::new(&solc.binary_path);
            command.arg("--standard-json");
            command
        },
        Some(&input),
        "solc --standard-json",
    )?;
    let output: serde_json::Value = serde_json::from_slice(&measured.output_stdout)?;
    reject_solc_errors(&output)?;
    let contract = output
        .pointer(&format!(
            "/contracts/{file_name}/{}",
            benchmark.contract_name
        ))
        .with_context(|| format!("missing solc contract {}", benchmark.contract_name))?;
    let abi = contract
        .pointer("/abi")
        .context("missing solidity abi")?
        .clone();
    let creation = contract
        .pointer("/evm/bytecode/object")
        .and_then(|value| value.as_str())
        .context("missing solidity creation bytecode")?
        .to_string();
    let runtime = contract
        .pointer("/evm/deployedBytecode/object")
        .and_then(|value| value.as_str())
        .context("missing solidity runtime bytecode")?
        .to_string();
    artifact(
        benchmark,
        profile,
        solc,
        &source_path,
        abi,
        creation,
        runtime,
        measured.wall_ms_samples,
        measured.cpu_ms_samples,
        measured.peak_rss_kib,
        solidity_compiler_settings(profile, solc, evm_version),
    )
}

fn reject_solc_errors(output: &serde_json::Value) -> Result<()> {
    let Some(errors) = output.get("errors").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let fatal: Vec<_> = errors
        .iter()
        .filter(|error| error.get("severity").and_then(|value| value.as_str()) == Some("error"))
        .collect();
    if fatal.is_empty() {
        return Ok(());
    }
    bail!("{}", serde_json::to_string_pretty(&fatal)?);
}

fn solidity_sources(source_path: &Path) -> Result<(String, BTreeMap<String, serde_json::Value>)> {
    let source_root = source_path.parent().context("solidity source parent")?;
    let file_name = source_path
        .strip_prefix(source_root)?
        .to_string_lossy()
        .replace('\\', "/");
    let mut sources = BTreeMap::new();
    for path in solidity_files(source_root)? {
        let key = path
            .strip_prefix(source_root)?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)?;
        sources.insert(key, json!({ "content": source }));
    }
    Ok((file_name, sources))
}

fn solidity_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(solidity_files(&path)?);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("sol") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn solidity_metadata_settings(metadata_mode: MetadataMode, solc: &Toolchain) -> serde_json::Value {
    match solidity_version_tuple(solc) {
        Some(version) if version >= (0, 8, 18) => match metadata_mode {
            MetadataMode::On => json!({
                "bytecodeHash": "ipfs",
                "appendCBOR": true
            }),
            MetadataMode::Off => json!({
                "bytecodeHash": "none",
                "appendCBOR": false
            }),
        },
        Some(version) if version >= (0, 6, 0) => match metadata_mode {
            MetadataMode::On => json!({
                "bytecodeHash": "ipfs"
            }),
            MetadataMode::Off => json!({
                "bytecodeHash": "none"
            }),
        },
        _ => json!({}),
    }
}

fn solidity_compiler_settings(
    profile: &CompilerProfile,
    solc: &Toolchain,
    evm_version: &str,
) -> serde_json::Value {
    json!({
        "evmVersion": evm_version,
        "compiler": profile.compiler,
        "metadataMode": profile.metadata_mode.as_str(),
        "metadata": solidity_metadata_settings(profile.metadata_mode, solc),
        "optimizer": profile.optimizer,
        "optimizerRuns": profile.optimizer_runs,
        "viaIR": profile.via_ir,
        "sourceVariant": source_variant_label(profile)
    })
}

fn solidity_version_tuple(solc: &Toolchain) -> Option<(u64, u64, u64)> {
    let mut parts = solc.version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn vyper_compiler_settings(profile: &CompilerProfile, evm_version: &str) -> serde_json::Value {
    json!({
        "evmVersion": evm_version,
        "compiler": profile.compiler,
        "metadataMode": profile.metadata_mode.as_str(),
        "bytecodeMetadata": profile.metadata_mode == MetadataMode::On,
        "optimize": profile.optimizer_mode.as_deref().unwrap_or("default"),
        "experimentalCodegen": profile.experimental_codegen,
        "sourceVariant": source_variant_label(profile)
    })
}

fn source_variant_label(profile: &CompilerProfile) -> &str {
    profile.source_variant.as_deref().unwrap_or("latest")
}

fn compile_vyper(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    vyper: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = source_path_for_profile(root, benchmark, profile, vyper)?;
    let measured = repeat_compile_samples(
        || {
            let mut command = Command::new(&vyper.binary_path);
            command
                .arg("-f")
                .arg("abi,bytecode,bytecode_runtime")
                .arg("--evm-version")
                .arg(evm_version);
            if let Some(optimizer_mode) = profile.optimizer_mode.as_deref() {
                for arg in vyper_optimizer_args(vyper, optimizer_mode) {
                    command.arg(arg);
                }
            }
            if profile.metadata_mode == MetadataMode::Off && vyper_supports_metadata_arg(vyper) {
                command.arg(vyper_disable_metadata_arg(vyper));
            }
            if profile.experimental_codegen {
                command.arg("--experimental-codegen");
            }
            command.arg(&source_path);
            command
        },
        None,
        "vyper compile",
    )?;
    let stdout = String::from_utf8(measured.output_stdout)?;
    let mut lines = stdout.lines();
    let abi_line = lines.next().context("missing vyper abi output")?;
    let creation = lines
        .next()
        .context("missing vyper bytecode output")?
        .to_string();
    let runtime = lines
        .next()
        .context("missing vyper runtime output")?
        .to_string();
    let abi: serde_json::Value = serde_json::from_str(abi_line)?;
    artifact(
        benchmark,
        profile,
        vyper,
        &source_path,
        abi,
        creation,
        runtime,
        measured.wall_ms_samples,
        measured.cpu_ms_samples,
        measured.peak_rss_kib,
        vyper_compiler_settings(profile, evm_version),
    )
}

fn vyper_optimizer_args(vyper: &Toolchain, optimizer_mode: &str) -> Vec<String> {
    if matches!(vyper_version_tuple(vyper), Some((0, 3, patch)) if patch < 10) {
        return if optimizer_mode == "none" {
            vec!["--no-optimize".to_string()]
        } else {
            Vec::new()
        };
    }
    if vyper_legacy_minor(vyper) < Some(3) {
        return Vec::new();
    }
    if vyper_legacy_minor(vyper) == Some(3) {
        return vec!["--optimize".to_string(), optimizer_mode.to_string()];
    }
    vec!["-O".to_string(), optimizer_mode.to_string()]
}

fn vyper_version_tuple(vyper: &Toolchain) -> Option<(u64, u64, u64)> {
    let mut parts = vyper.version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn vyper_supports_metadata_arg(vyper: &Toolchain) -> bool {
    vyper_legacy_minor(vyper) >= Some(3)
}

fn vyper_disable_metadata_arg(vyper: &Toolchain) -> &'static str {
    if vyper.version.starts_with("0.5.") {
        "--disable-bytecode-metadata"
    } else {
        "--no-bytecode-metadata"
    }
}

fn vyper_legacy_minor(vyper: &Toolchain) -> Option<u64> {
    let mut parts = vyper.version.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    if major == 0 { Some(minor) } else { Some(99) }
}

fn source_path_for_profile(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
) -> Result<PathBuf> {
    match profile.language {
        Language::Solidity => {
            let source_path = root.join(&benchmark.solidity_path);
            let source_root = source_path.parent().context("solidity source parent")?;
            let variant_path = root
                .join("target/bench-source-variants")
                .join(&profile.id)
                .join(&benchmark.solidity_path);
            let variant_root = variant_path.parent().context("solidity variant parent")?;
            for path in solidity_files(source_root)? {
                let relative = path.strip_prefix(source_root)?;
                let target = variant_root.join(relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let source = fs::read_to_string(&path)?;
                let transformed = transform_solidity_source(
                    &source,
                    profile.source_variant.as_deref(),
                    &solidity_pragma_for_toolchain(toolchain)?,
                )
                .with_context(|| {
                    format!(
                        "applying source variant {} to {}",
                        profile.source_variant.as_deref().unwrap_or("latest"),
                        path.display()
                    )
                })?;
                fs::write(target, transformed)?;
            }
            Ok(variant_path)
        }
        Language::Vyper => {
            let source_path = root.join(&benchmark.vyper_path);
            let source = fs::read_to_string(&source_path)?;
            let transformed = transform_vyper_source(
                &source,
                profile.source_variant.as_deref(),
                &vyper_pragma_for_toolchain(toolchain)?,
            )
            .with_context(|| {
                format!(
                    "applying source variant {}",
                    profile.source_variant.as_deref().unwrap_or("latest")
                )
            })?;
            materialize_source_variant(root, &profile.id, &benchmark.vyper_path, transformed)
        }
    }
}

fn materialize_source_variant(
    root: &Path,
    variant: &str,
    source_path: &str,
    transformed: String,
) -> Result<PathBuf> {
    let variant_path = root
        .join("target/bench-source-variants")
        .join(variant)
        .join(source_path);
    if let Some(parent) = variant_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&variant_path, transformed)?;
    Ok(variant_path)
}

fn transform_solidity_source(source: &str, variant: Option<&str>, pragma: &str) -> Result<String> {
    let source = rewrite_solidity_pragma(source, pragma);
    let source = match variant {
        None | Some("solidity-0.8") => source,
        Some("solidity-0.7") => {
            let source = add_solidity_abicoder_pragma(&source, "pragma abicoder v2;");
            rewrite_solidity_pre_08(&source)
        }
        Some("solidity-0.6") => {
            let source = add_solidity_abicoder_pragma(&source, "pragma experimental ABIEncoderV2;");
            let source = rewrite_solidity_pre_08(&source);
            add_constructor_visibility(&source)
        }
        Some("solidity-0.5") => {
            let source = add_solidity_abicoder_pragma(&source, "pragma experimental ABIEncoderV2;");
            let source = rewrite_solidity_pre_08(&source);
            let source = rewrite_solidity_pre_06_immutables(&source);
            let source = rewrite_solidity_pre_06_call_value(&source);
            add_constructor_visibility(&source)
        }
        Some("solidity-0.4") => {
            let source = rewrite_solidity_pre_08(&source);
            let source = rewrite_solidity_pre_06_immutables(&source);
            let source = rewrite_solidity_04_low_level_calls(&source);
            let source = add_constructor_visibility(&source);
            source.replace(" calldata", "")
        }
        Some(other) => bail!("unknown Solidity source variant {other}"),
    };
    Ok(source)
}

fn rewrite_solidity_pragma(source: &str, pragma: &str) -> String {
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        if !replaced && line.trim_start().starts_with("pragma solidity ") {
            lines.push(pragma.to_string());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if replaced {
        lines.join("\n")
    } else {
        format!("{pragma}\n{source}")
    }
}

fn add_solidity_abicoder_pragma(source: &str, pragma: &str) -> String {
    if source.contains("pragma abicoder v2;")
        || source.contains("pragma experimental ABIEncoderV2;")
    {
        return source.to_string();
    }

    let mut inserted = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        lines.push(line.to_string());
        if !inserted && line.trim_start().starts_with("pragma solidity ") {
            lines.push(pragma.to_string());
            inserted = true;
        }
    }
    if inserted {
        lines.join("\n")
    } else {
        format!("{pragma}\n{source}")
    }
}

fn solidity_pragma_for_toolchain(solc: &Toolchain) -> Result<String> {
    let Some((major, minor, patch)) = solidity_version_tuple(solc) else {
        bail!(
            "cannot derive Solidity pragma from solc version {}",
            solc.version
        );
    };
    let upper_major = if major == 0 { 0 } else { major + 1 };
    let upper_minor = if major == 0 { minor + 1 } else { 0 };
    Ok(format!(
        "pragma solidity >={major}.{minor}.{patch} <{upper_major}.{upper_minor}.0;"
    ))
}

fn rewrite_solidity_pre_08(source: &str) -> String {
    let source = remove_numeric_separators(source);
    let source = source
        .replace("unchecked {", "{")
        .replace("10_000_000_000", "10000000000")
        .replace("10_000", "10000")
        .replace("type(uint256).max", "uint256(-1)")
        .replace("type(uint112).max", "uint112(-1)")
        .replace("block.chainid", "uint256(1)");
    rewrite_solidity_address_code_length(&source)
}

fn remove_numeric_separators(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '_'
            && index > 0
            && index + 1 < chars.len()
            && chars[index - 1].is_ascii_digit()
            && chars[index + 1].is_ascii_digit()
        {
            continue;
        }
        output.push(*ch);
    }
    output
}

fn add_constructor_visibility(source: &str) -> String {
    let mut pending_constructor = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if pending_constructor {
            if trimmed.starts_with(')') && trimmed.contains('{') {
                lines.push(add_public_to_constructor_body_line(line));
                pending_constructor = false;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }

        if trimmed.starts_with("constructor(") && !constructor_has_visibility(trimmed) {
            if trimmed.contains('{') {
                lines.push(add_public_to_constructor_body_line(line));
            } else {
                pending_constructor = true;
                lines.push(line.to_string());
            }
        } else {
            lines.push(line.to_string());
        }
    }
    lines.join("\n")
}

fn constructor_has_visibility(line: &str) -> bool {
    line.contains(" public") || line.contains(" internal")
}

fn add_public_to_constructor_body_line(line: &str) -> String {
    let Some(brace_index) = line.find('{') else {
        return line.to_string();
    };
    let Some(close_paren_index) = line[..brace_index].rfind(')') else {
        return line.to_string();
    };
    let mut output = String::with_capacity(line.len() + " public".len());
    output.push_str(&line[..=close_paren_index]);
    output.push_str(" public");
    output.push_str(&line[close_paren_index + 1..]);
    output
}

fn rewrite_solidity_address_code_length(source: &str) -> String {
    const MARKER: &str = ".code.length";
    if !source.contains(MARKER) {
        return source.to_string();
    }

    let mut rewritten = String::with_capacity(source.len());
    let mut remaining = source;
    while let Some(marker_index) = remaining.find(MARKER) {
        let prefix = &remaining[..marker_index];
        if let Some(expr_start) = code_length_expression_start(prefix) {
            rewritten.push_str(&prefix[..expr_start]);
            rewritten.push_str("_benchExtcodesize(");
            rewritten.push_str(&prefix[expr_start..]);
            rewritten.push(')');
        } else {
            rewritten.push_str(prefix);
            rewritten.push_str(MARKER);
        }
        remaining = &remaining[marker_index + MARKER.len()..];
    }
    rewritten.push_str(remaining);
    append_solidity_contract_helper(&rewritten, SOLIDITY_EXTCODESIZE_HELPER)
}

fn code_length_expression_start(prefix: &str) -> Option<usize> {
    let bytes = prefix.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut index = bytes.len();
    if bytes[index - 1] == b')' {
        let mut depth = 0usize;
        while index > 0 {
            index -= 1;
            match bytes[index] {
                b')' => depth += 1,
                b'(' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        while index > 0 && is_solidity_identifier_byte(bytes[index - 1]) {
                            index -= 1;
                        }
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    } else {
        while index > 0 && is_solidity_identifier_byte(bytes[index - 1]) {
            index -= 1;
        }
        (index < bytes.len()).then_some(index)
    }
}

fn is_solidity_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

const SOLIDITY_EXTCODESIZE_HELPER: &str = r#"

    function _benchExtcodesize(address account) internal view returns (uint256 size) {
        assembly { size := extcodesize(account) }
    }
"#;

fn append_solidity_contract_helper(source: &str, helper: &str) -> String {
    let helper_exists = (helper.contains("_benchExtcodesize")
        && source.contains("function _benchExtcodesize("))
        || (helper.contains("_benchStaticcallWord")
            && source.contains("function _benchStaticcallWord("))
        || (helper.contains("_benchCallWord") && source.contains("function _benchCallWord("))
        || (helper.contains("_benchPermitDigest")
            && source.contains("function _benchPermitDigest("))
        || (helper.contains("_benchAcceptPermit")
            && source.contains("function _benchAcceptPermit("))
        || (helper.contains("_benchPermitStructHash")
            && source.contains("function _benchPermitStructHash("));
    if helper_exists {
        return source.to_string();
    }

    let Some(insert_index) = source.rfind('}') else {
        return source.to_string();
    };
    let mut output = String::with_capacity(source.len() + helper.len());
    output.push_str(&source[..insert_index]);
    output.push_str(helper);
    output.push_str(&source[insert_index..]);
    output
}

fn rewrite_solidity_pre_06_call_value(source: &str) -> String {
    source
        .replace(
            "(bool ok,) = msg.sender.call{value: amount}(\"\");",
            "(bool ok,) = msg.sender.call.value(amount)(\"\");",
        )
        .replace(
            "CurveBenchERC20.transfer.selector",
            "bytes4(keccak256(\"transfer(address,uint256)\"))",
        )
        .replace(
            "CurveBenchERC20.transferFrom.selector",
            "bytes4(keccak256(\"transferFrom(address,address,uint256)\"))",
        )
        .replace(
            "YearnBenchERC20.approve.selector",
            "bytes4(keccak256(\"approve(address,uint256)\"))",
        )
        .replace(
            "YearnBenchERC20.transfer.selector",
            "bytes4(keccak256(\"transfer(address,uint256)\"))",
        )
        .replace(
            "YearnBenchERC20.transferFrom.selector",
            "bytes4(keccak256(\"transferFrom(address,address,uint256)\"))",
        )
}

fn rewrite_solidity_pre_06_immutables(source: &str) -> String {
    source
        .replace(
            "function _addLiquidity(uint256[] calldata amounts",
            "function _addLiquidity(uint256[] memory amounts",
        )
        .replace(
            "function _removeLiquidity(uint256 lpAmount, uint256[] calldata minAmounts",
            "function _removeLiquidity(uint256 lpAmount, uint256[] memory minAmounts",
        )
        .replace(
            "function _removeLiquidityImbalance(uint256[] calldata amounts",
            "function _removeLiquidityImbalance(uint256[] memory amounts",
        )
        .replace(" immutable ", " ")
        .replace(" immutable;", ";")
        .replace(" immutable =", " =")
}

fn rewrite_solidity_04_low_level_calls(source: &str) -> String {
    let source = rewrite_solidity_pre_06_call_value(source)
        .replace(
            "(bool ok,) = msg.sender.call.value(amount)(\"\");",
            "bool ok = msg.sender.call.value(amount)();",
        )
        .replace(
            "(bool ok,) = address(this).staticcall(abi.encodeWithSelector(bytes4(0x773acdef), i));",
            "bool ok = address(this).call(abi.encodeWithSelector(bytes4(0x773acdef), i));",
        );
    rewrite_solidity_04_staticcalls(&source)
}

fn rewrite_solidity_04_staticcalls(source: &str) -> String {
    let source = source
        .replace(
            "keccak256(abi.encode(EIP2612_TYPEHASH, owner, spender, value, nonce, deadline))",
            "_benchPermitStructHash(owner, spender, value, nonce, deadline)",
        )
        .replace(
            "uint256 nonce = nonces[owner];\n        bytes32 digest = keccak256(\n            abi.encodePacked(\n                bytes1(0x19),\n                bytes1(0x01),\n                _domainSeparator(),\n                _benchPermitStructHash(owner, spender, value, nonce, deadline)\n            )\n        );",
            "(bytes32 digest, uint256 nonce) = _benchPermitDigest(owner, spender, value, deadline);",
        )
        .replace(
            "(bool ok, bytes memory result) =\n                owner.staticcall(abi.encodeWithSignature(\"isValidSignature(bytes32,bytes)\", digest, signature));\n            require(ok && result.length >= 32 && abi.decode(result, (bytes32)) == ERC1271_MAGIC_VALUE, \"signature\");",
            "(bool ok, bytes32 resultWord, uint256 resultSize) =\n                _benchStaticcallWord(owner, abi.encodeWithSignature(\"isValidSignature(bytes32,bytes)\", digest, signature));\n            require(ok && resultSize >= 32 && resultWord == ERC1271_MAGIC_VALUE, \"signature\");",
        )
        .replace(
            "(bool ok, bytes memory response) = oracle.staticcall(abi.encodeWithSelector(selector));\n                require(ok && response.length == 32, \"rate oracle\");\n                uint256 fetchedRate = abi.decode(response, (uint256));",
            "(bool ok, bytes32 responseWord, uint256 responseSize) = _benchStaticcallWord(oracle, abi.encodeWithSelector(selector));\n                require(ok && responseSize == 32, \"rate oracle\");\n                uint256 fetchedRate = uint256(responseWord);",
        )
        .replace(
            "(bool ok, bytes memory returndata) = coin.call(data);\n        require(ok, message);\n        if (returndata.length > 0) {\n            require(abi.decode(returndata, (bool)), message);\n        }",
            "(bool ok, bytes32 returndataWord, uint256 returndataSize) = _benchCallWord(coin, data);\n        require(ok, message);\n        if (returndataSize > 0) {\n            require(uint256(returndataWord) != 0, message);\n        }",
        )
        .replace(
            "allowance[owner][spender] = value;\n        nonces[owner] = nonce + 1;\n        emit Approval(owner, spender, value);\n        return true;",
            "return _benchAcceptPermit(owner, spender, value, nonce);",
        );
    let source = if source.contains("_benchStaticcallWord(") {
        append_solidity_contract_helper(&source, SOLIDITY_STATICCALL_WORD_HELPER)
    } else {
        source
    };
    let source = if source.contains("_benchCallWord(") {
        append_solidity_contract_helper(&source, SOLIDITY_CALL_WORD_HELPER)
    } else {
        source
    };
    let source = if source.contains("_benchPermitDigest(") {
        append_solidity_contract_helper(&source, SOLIDITY_PERMIT_DIGEST_HELPER)
    } else {
        source
    };
    let source = if source.contains("_benchAcceptPermit(") {
        append_solidity_contract_helper(&source, SOLIDITY_ACCEPT_PERMIT_HELPER)
    } else {
        source
    };
    if source.contains("_benchPermitStructHash(") {
        append_solidity_contract_helper(&source, SOLIDITY_PERMIT_STRUCT_HASH_HELPER)
    } else {
        source
    }
}

const SOLIDITY_STATICCALL_WORD_HELPER: &str = r#"

    function _benchStaticcallWord(address target, bytes memory data)
        internal
        view
        returns (bool ok, bytes32 word, uint256 size)
    {
        assembly {
            let ptr := mload(0x40)
            ok := staticcall(gas, target, add(data, 32), mload(data), ptr, 32)
            size := returndatasize
            let copySize := size
            if gt(copySize, 32) { copySize := 32 }
            returndatacopy(ptr, 0, copySize)
            word := mload(ptr)
        }
    }
"#;

const SOLIDITY_CALL_WORD_HELPER: &str = r#"

    function _benchCallWord(address target, bytes memory data)
        internal
        returns (bool ok, bytes32 word, uint256 size)
    {
        assembly {
            let ptr := mload(0x40)
            ok := call(gas, target, 0, add(data, 32), mload(data), ptr, 32)
            size := returndatasize
            let copySize := size
            if gt(copySize, 32) { copySize := 32 }
            returndatacopy(ptr, 0, copySize)
            word := mload(ptr)
        }
    }
"#;

const SOLIDITY_PERMIT_DIGEST_HELPER: &str = r#"

    function _benchPermitDigest(address owner, address spender, uint256 value, uint256 deadline)
        internal
        view
        returns (bytes32 digest, uint256 nonce)
    {
        nonce = nonces[owner];
        digest = keccak256(
            abi.encodePacked(
                bytes1(0x19),
                bytes1(0x01),
                _domainSeparator(),
                keccak256(abi.encode(EIP2612_TYPEHASH, owner, spender, value, nonce, deadline))
            )
        );
    }
"#;

const SOLIDITY_ACCEPT_PERMIT_HELPER: &str = r#"

    function _benchAcceptPermit(address owner, address spender, uint256 value, uint256 nonce)
        internal
        returns (bool)
    {
        allowance[owner][spender] = value;
        nonces[owner] = nonce + 1;
        emit Approval(owner, spender, value);
        return true;
    }
"#;

const SOLIDITY_PERMIT_STRUCT_HASH_HELPER: &str = r#"

    function _benchPermitStructHash(
        address owner,
        address spender,
        uint256 value,
        uint256 nonce,
        uint256 deadline
    ) internal pure returns (bytes32) {
        return keccak256(abi.encode(EIP2612_TYPEHASH, owner, spender, value, nonce, deadline));
    }
"#;

fn transform_vyper_source(source: &str, variant: Option<&str>, pragma: &str) -> Result<String> {
    let source = strip_vyper_profile_pragmas(&rewrite_vyper_pragma(source, pragma));
    let source = match variant {
        None => source,
        Some("vyper-0.4") => rewrite_vyper_event_logs(&source),
        Some("vyper-0.3") => {
            let mut source = source;
            source = source.replace("@deploy", "@external");
            source = source.replace("@nonreentrant\n", "@nonreentrant('lock')\n");
            source = source.replace("staticcall ", "");
            source = source.replace("extcall ", "");
            source = rewrite_vyper_03_interface_imports(&source);
            source = source.replace("//", "/");
            source = source.replace("abi_encode(", "_abi_encode(");
            source = rewrite_vyper_03_strategy_maps(&source);
            source = rewrite_vyper_03_struct_constructors(&source);
            source = rewrite_vyper_03_pending_reports(&source);
            source = rewrite_typed_for_loops(&source);
            rewrite_vyper_event_logs(&source)
        }
        Some("vyper-0.2") => {
            let mut source = source;
            source = source.replace("@deploy", "@external");
            source = source.replace("@nonreentrant\n", "@nonreentrant('lock')\n");
            source = source.replace("staticcall ", "");
            source = source.replace("extcall ", "");
            source = rewrite_vyper_03_interface_imports(&source);
            source = source.replace("@pure", "@view");
            source = source.replace("//", "/");
            source = source.replace("abi_encode(", "_abi_encode(");
            source = source.replace("public(immutable(String[32]))", "public(String[32])");
            source = source.replace("public(immutable(String[8]))", "public(String[8])");
            source = source.replace("public(immutable(uint8))", "public(uint256)");
            source = source.replace("    name = ", "    self.name = ");
            source = source.replace("    symbol = ", "    self.symbol = ");
            source = source.replace("    decimals = ", "    self.decimals = ");
            source = source.replace(
                "max_value(uint256)",
                "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            );
            source = rewrite_vyper_02_fixed_bytes(&source);
            source = rewrite_vyper_03_strategy_maps(&source);
            source = rewrite_vyper_03_struct_constructors(&source);
            source = rewrite_vyper_03_pending_reports(&source);
            source = rewrite_vyper_02_reserved_fields(&source);
            source = rewrite_vyper_02_owner_checks(&source);
            source = rewrite_vyper_02_manager_checks(&source);
            source = rewrite_vyper_02_uniswap_integer_types(&source);
            source = rewrite_typed_for_loops(&source);
            source = rewrite_vyper_event_logs(&source);
            reorder_vyper_02_internal_functions(&source)
        }
        Some(other) => bail!("unknown Vyper source variant {other}"),
    };
    Ok(source)
}

fn strip_vyper_profile_pragmas(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("# pragma optimize ")
                && !trimmed.starts_with("# pragma evm-version ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rewrite_vyper_03_interface_imports(source: &str) -> String {
    source
        .replace(
            "from ethereum.ercs import IERC20Detailed",
            "from vyper.interfaces import ERC20Detailed",
        )
        .replace(
            "from ethereum.ercs import IERC4626",
            "from vyper.interfaces import ERC4626",
        )
        .replace(
            "from ethereum.ercs import IERC20",
            "from vyper.interfaces import ERC20",
        )
        .replace("IERC20Detailed", "ERC20Detailed")
        .replace("IERC4626", "ERC4626")
        .replace("IERC20", "ERC20")
}

fn rewrite_vyper_pragma(source: &str, pragma: &str) -> String {
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !replaced
            && (trimmed.starts_with("# pragma version ") || trimmed.starts_with("# @version "))
        {
            lines.push(pragma.to_string());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if replaced {
        lines.join("\n")
    } else {
        format!("{pragma}\n{source}")
    }
}

fn vyper_pragma_for_toolchain(vyper: &Toolchain) -> Result<String> {
    let version = vyper.version.split('+').next().unwrap_or(&vyper.version);
    let mut parts = version.split('.');
    let major = parts
        .next()
        .context("missing Vyper major version")?
        .parse::<u64>()?;
    let minor = parts
        .next()
        .context("missing Vyper minor version")?
        .parse::<u64>()?;
    let upper_major = if major == 0 { 0 } else { major + 1 };
    let upper_minor = if major == 0 { minor + 1 } else { 0 };
    Ok(format!(
        "# pragma version >={version},<{upper_major}.{upper_minor}.0"
    ))
}

fn rewrite_typed_for_loops(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let Some(for_start) = line.find("for ") else {
                return line.to_string();
            };
            let prefix = &line[..for_start];
            let rest = &line[for_start + 4..];
            let Some(colon_index) = rest.find(':') else {
                return line.to_string();
            };
            let Some(in_index) = rest.find(" in ") else {
                return line.to_string();
            };
            if colon_index > in_index {
                return line.to_string();
            }
            format!(
                "{prefix}for {} in {}",
                rest[..colon_index].trim(),
                &rest[in_index + 4..]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rewrite_vyper_event_logs(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut output = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let Some(log_index) = line.find("log ") else {
            output.push(line.to_string());
            index += 1;
            continue;
        };
        let Some(open_index) = line[log_index..].find('(').map(|offset| log_index + offset) else {
            output.push(line.to_string());
            index += 1;
            continue;
        };

        let mut block = line.to_string();
        let mut balance = paren_balance(&line[open_index..]);
        while balance > 0 && index + 1 < lines.len() {
            index += 1;
            block.push('\n');
            block.push_str(lines[index]);
            balance += paren_balance(lines[index]);
        }

        if let Some(rewritten) = rewrite_vyper_event_log_block(&block) {
            output.push(rewritten);
        } else {
            output.extend(block.lines().map(str::to_string));
        }
        index += 1;
    }
    output.join("\n")
}

fn rewrite_vyper_02_owner_checks(source: &str) -> String {
    source
        .replace(
            "def _only_owner():\n    assert msg.sender == self.owner, \"owner\"",
            "def _only_owner(sender: address):\n    assert sender == self.owner, \"owner\"",
        )
        .replace("self._only_owner()", "self._only_owner(msg.sender)")
}

fn rewrite_vyper_02_manager_checks(source: &str) -> String {
    source
        .replace(
            "def _only_manager():\n    assert msg.sender == self.roleManager, \"permission\"",
            "def _only_manager(sender: address):\n    assert sender == self.roleManager, \"permission\"",
        )
        .replace("self._only_manager()", "self._only_manager(msg.sender)")
}

fn rewrite_vyper_02_fixed_bytes(source: &str) -> String {
    source.replace("bytes20", "bytes32")
}

fn rewrite_vyper_02_reserved_fields(source: &str) -> String {
    if !source.contains("struct Strategy:") {
        return source.to_string();
    }
    replace_token(
        &source.replace("    balance: uint256", "    strategyBalance: uint256"),
        ".balance",
        ".strategyBalance",
    )
}

fn replace_token(source: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(index) = rest.find(from) {
        let after_index = index + from.len();
        let after = rest[after_index..].chars().next();
        if after.is_some_and(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
            out.push_str(&rest[..after_index]);
        } else {
            out.push_str(&rest[..index]);
            out.push_str(to);
        }
        rest = &rest[after_index..];
    }
    out.push_str(rest);
    out
}

fn rewrite_vyper_03_struct_constructors(source: &str) -> String {
    source
        .replace(
            "    self.pendingReports[strategy] = PendingReport(gain=gain, loss=loss)",
            "    self.pendingReports[strategy].gain = gain\n    self.pendingReports[strategy].loss = loss",
        )
        .replace(
            "    self.pendingReports[strategy] = PendingReport(gain=0, loss=0)",
            "    self.pendingReports[strategy].gain = 0\n    self.pendingReports[strategy].loss = 0",
        )
}

fn rewrite_vyper_03_pending_reports(source: &str) -> String {
    source
        .replace(
            "pendingReports: HashMap[address, PendingReport]",
            "pendingReportGain: HashMap[address, uint256]\npendingReportLoss: HashMap[address, uint256]",
        )
        .replace(
            "self.pendingReports[strategy].gain",
            "self.pendingReportGain[strategy]",
        )
        .replace(
            "self.pendingReports[strategy].loss",
            "self.pendingReportLoss[strategy]",
        )
}

fn rewrite_vyper_03_strategy_maps(source: &str) -> String {
    if !source.contains("strategies: HashMap[address, Strategy]") {
        return source.to_string();
    }
    source
        .replace(
            "strategies: HashMap[address, Strategy]",
            "strategyActivation: HashMap[address, uint256]\nstrategyCurrentDebt: HashMap[address, uint256]\nstrategyMaxDebt: HashMap[address, uint256]\nstrategyBalance: HashMap[address, uint256]",
        )
        .replace(
            "self.strategies[strategy].activation",
            "self.strategyActivation[strategy]",
        )
        .replace(
            "self.strategies[strategy].currentDebt",
            "self.strategyCurrentDebt[strategy]",
        )
        .replace(
            "self.strategies[strategy].maxDebt",
            "self.strategyMaxDebt[strategy]",
        )
        .replace(
            "self.strategies[strategy].balance",
            "self.strategyBalance[strategy]",
        )
}

fn rewrite_vyper_02_uniswap_integer_types(source: &str) -> String {
    source
        .replace("def getReserves() -> (uint112, uint112, uint32):", "def getReserves() -> (uint256, uint256, uint256):")
        .replace("return convert(self.reserve0, uint112), convert(self.reserve1, uint112), convert(self.blockTimestampLast, uint32)", "return self.reserve0, self.reserve1, self.blockTimestampLast")
        .replace(
            "convert(max_value(uint112), uint256)",
            "5192296858534827628530496329220095",
        )
}

#[derive(Debug)]
struct VyperFunctionBlock {
    text: String,
    name: String,
    is_internal: bool,
    ordinal: usize,
}

fn reorder_vyper_02_internal_functions(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let Some(first_function_line) = lines
        .iter()
        .position(|line| line.starts_with('@') && is_vyper_function_decorator(line))
    else {
        return source.to_string();
    };

    let prefix = lines[..first_function_line].join("\n");
    let mut blocks = Vec::new();
    let mut start = first_function_line;
    let mut ordinal = 0usize;
    while start < lines.len() {
        let mut end = start + 1;
        while end < lines.len() {
            if lines[end].starts_with('@') && is_vyper_function_decorator(lines[end]) {
                break;
            }
            end += 1;
        }
        let text = lines[start..end].join("\n");
        blocks.push(VyperFunctionBlock {
            name: vyper_function_name(&text).unwrap_or_default(),
            is_internal: text.lines().any(|line| line.trim() == "@internal"),
            text,
            ordinal,
        });
        ordinal += 1;
        start = end;
    }

    let mut internal: Vec<_> = blocks.iter().filter(|block| block.is_internal).collect();
    internal.sort_by_key(|block| (vyper_internal_order(&block.name), block.ordinal));
    let external: Vec<_> = blocks.iter().filter(|block| !block.is_internal).collect();

    let mut out = String::new();
    out.push_str(&prefix);
    if !out.ends_with("\n\n") {
        out.push_str("\n\n");
    }
    for block in internal.into_iter().chain(external) {
        out.push_str(block.text.trim());
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
}

fn is_vyper_function_decorator(line: &str) -> bool {
    matches!(line.trim(), "@external" | "@internal" | "@deploy")
}

fn vyper_function_name(block: &str) -> Option<String> {
    for line in block.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("def ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once('(') else {
            continue;
        };
        return Some(name.to_string());
    }
    None
}

fn vyper_internal_order(name: &str) -> usize {
    match name {
        "_min" => 0,
        "_sqrt" => 1,
        "_ready" => 2,
        "_only_owner" => 3,
        "_only_manager" => 4,
        "_total_assets" => 5,
        "_unlocked_shares" => 6,
        "_effective_supply" => 7,
        "_convert_to_shares" => 8,
        "_convert_to_assets" => 9,
        "_mint" => 10,
        "_burn" => 11,
        "_transfer" => 12,
        "_ensure_idle" => 13,
        "_redeem" => 14,
        "_getD" => 15,
        "_getY" => 16,
        "_unpack_reserve0" => 17,
        "_unpack_reserve1" => 18,
        "_unpack_block_timestamp" => 19,
        "_pack_reserves" => 20,
        "_update" => 21,
        "_mint_fee" => 22,
        "_compute_address" => 23,
        "_predict_clone" => 24,
        _ => 100,
    }
}

fn rewrite_vyper_event_log_block(block: &str) -> Option<String> {
    let first_line = block.lines().next()?;
    let Some(log_index) = first_line.find("log ") else {
        return None;
    };
    let Some(open_index) = first_line[log_index..]
        .find('(')
        .map(|index| log_index + index)
    else {
        return None;
    };
    if !block.trim_end().ends_with(')') {
        return None;
    }
    let Some(close_index) = block.rfind(')') else {
        return None;
    };
    let args = &block[open_index + 1..close_index];
    if !args.contains('=') {
        return None;
    }
    let values = strip_keyword_args(args);
    Some(format!(
        "{}log {}({})",
        &first_line[..log_index],
        first_line[log_index + 4..open_index].trim(),
        values.join(", ")
    ))
}

fn paren_balance(line: &str) -> i32 {
    line.chars().fold(0, |balance, ch| match ch {
        '(' => balance + 1,
        ')' => balance - 1,
        _ => balance,
    })
}

fn strip_keyword_args(args: &str) -> Vec<String> {
    split_top_level_args(args)
        .into_iter()
        .map(|arg| {
            let mut depth = 0i32;
            for (index, ch) in arg.char_indices() {
                match ch {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    '=' if depth == 0 => return arg[index + 1..].trim().to_string(),
                    _ => {}
                }
            }
            arg.trim().to_string()
        })
        .filter(|arg| !arg.is_empty())
        .collect()
}

fn split_top_level_args(args: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in args.chars() {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

#[allow(clippy::too_many_arguments)]
fn artifact(
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    source_path: &Path,
    abi: serde_json::Value,
    creation_bytecode: String,
    runtime_bytecode: String,
    wall_ms_samples: Vec<f64>,
    cpu_ms_samples: Vec<f64>,
    peak_rss_kib: u64,
    compiler_settings: serde_json::Value,
) -> Result<CompiledArtifact> {
    let source = fs::read(source_path)?;
    let bytecode = bytecode_metrics(&creation_bytecode, &runtime_bytecode)?;
    let language = profile.language;
    Ok(CompiledArtifact {
        benchmark_id: benchmark.id.clone(),
        implementation_id: implementation_id(profile),
        suite: benchmark.suite,
        family: benchmark.family.clone(),
        parameter_name: benchmark.parameter_name.clone(),
        parameter_value: benchmark.parameter_value,
        scenario_path: benchmark.scenario_path.clone(),
        scenario_hash: benchmark.scenario_hash.clone(),
        generator_version: benchmark.generator_version.clone(),
        provenance: benchmark.provenance.clone(),
        language,
        contract_name: benchmark.contract_name.clone(),
        profile_id: profile.id.clone(),
        compiler: toolchain.clone(),
        compiler_settings,
        metadata_mode: profile.metadata_mode,
        source_path: source_path.to_path_buf(),
        source_hash: sha256_bytes(&source),
        abi,
        creation_bytecode,
        runtime_bytecode,
        compile: CompileMetrics {
            wall_ms_samples,
            cpu_ms_samples,
            peak_rss_kib,
        },
        bytecode,
        cache: CacheInfo::disabled(),
    })
}

struct CompileSamples {
    output_stdout: Vec<u8>,
    wall_ms_samples: Vec<f64>,
    cpu_ms_samples: Vec<f64>,
    peak_rss_kib: u64,
}

fn repeat_compile_samples<F>(
    mut command_factory: F,
    stdin: Option<&[u8]>,
    label: &str,
) -> Result<CompileSamples>
where
    F: FnMut() -> Command,
{
    let sample_count = compile_sample_count();
    let mut output_stdout = Vec::new();
    let mut wall_ms_samples = Vec::with_capacity(sample_count);
    let mut cpu_ms_samples = Vec::with_capacity(sample_count);
    let mut peak_rss_kib = 0;
    for sample_index in 0..sample_count {
        let mut command = command_factory();
        let measured = require_success(run_measured(&mut command, stdin)?, label)?;
        let CommandStats {
            wall_ms,
            cpu_ms,
            peak_rss_kib: sample_peak_rss_kib,
        } = measured.stats;
        wall_ms_samples.push(wall_ms);
        cpu_ms_samples.push(cpu_ms);
        peak_rss_kib = peak_rss_kib.max(sample_peak_rss_kib);
        if sample_index + 1 == sample_count {
            output_stdout = measured.output.stdout;
        }
    }
    Ok(CompileSamples {
        output_stdout,
        wall_ms_samples,
        cpu_ms_samples,
        peak_rss_kib,
    })
}

fn compile_sample_count() -> usize {
    env::var("EVM_BENCH_COMPILE_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=20).contains(value))
        .unwrap_or(DEFAULT_COMPILE_SAMPLES)
}

fn compile_failure(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    evm_version: &str,
    error: String,
) -> Result<CompileFailure> {
    let language = profile.language;
    let source_path = source_path_for_profile(root, benchmark, profile, toolchain)?;
    let source = fs::read(&source_path)?;
    let compiler_settings = match language {
        Language::Solidity => solidity_compiler_settings(profile, toolchain, evm_version),
        Language::Vyper => vyper_compiler_settings(profile, evm_version),
    };
    Ok(CompileFailure {
        benchmark_id: benchmark.id.clone(),
        implementation_id: implementation_id(profile),
        suite: benchmark.suite,
        family: benchmark.family.clone(),
        parameter_name: benchmark.parameter_name.clone(),
        parameter_value: benchmark.parameter_value,
        scenario_path: benchmark.scenario_path.clone(),
        scenario_hash: benchmark.scenario_hash.clone(),
        generator_version: benchmark.generator_version.clone(),
        provenance: benchmark.provenance.clone(),
        language,
        contract_name: benchmark.contract_name.clone(),
        profile_id: profile.id.clone(),
        compiler: toolchain.clone(),
        compiler_settings,
        metadata_mode: profile.metadata_mode,
        source_path,
        source_hash: sha256_bytes(&source),
        error,
        cache: CacheInfo::disabled(),
    })
}

fn implementation_id(profile: &CompilerProfile) -> String {
    match profile.source_variant.as_deref() {
        Some(variant) => format!("{}/handwritten/{variant}", profile.language.as_str()),
        None => format!("{}/handwritten/v1", profile.language.as_str()),
    }
}

fn bytecode_metrics(creation: &str, runtime: &str) -> Result<BytecodeMetrics> {
    let creation_bytes = byte_len(creation)?;
    let runtime_bytes = byte_len(runtime)?;
    let creation_bytes_stripped = stripped_cbor_len(creation)?;
    let runtime_bytes_stripped = stripped_cbor_len(runtime)?;
    Ok(BytecodeMetrics {
        creation_bytes,
        creation_bytes_stripped,
        runtime_bytes,
        runtime_bytes_stripped,
        initcode_bytes: creation_bytes,
        linked_runtime_bytes: runtime_bytes,
        eip170_margin_bytes: 24_576 - runtime_bytes as isize,
        eip3860_margin_bytes: 49_152 - creation_bytes as isize,
        code_deposit_gas: 200 * runtime_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        bytecode_metrics, profile_applies_to_benchmark, solidity_pragma_for_toolchain,
        source_fingerprint, transform_solidity_source, transform_vyper_source,
    };
    use crate::models::{CompilerProfile, Language, MetadataMode, Toolchain};
    use std::{collections::BTreeMap, fs, path::PathBuf};

    #[test]
    fn computes_bytecode_metrics() {
        let metrics = bytecode_metrics("0x6001600055", "0x60016000").unwrap();
        assert_eq!(metrics.creation_bytes, 5);
        assert_eq!(metrics.runtime_bytes, 4);
        assert_eq!(metrics.code_deposit_gas, 800);
    }

    #[test]
    fn solidity_fingerprint_includes_import_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("Main.sol");
        let lib_dir = dir.path().join("lib");
        fs::create_dir(&lib_dir).unwrap();
        fs::write(
            &main,
            "pragma solidity ^0.8.35; import './lib/Lib.sol'; contract Main {}",
        )
        .unwrap();
        fs::write(
            lib_dir.join("Lib.sol"),
            "library Lib { function f() internal {} }",
        )
        .unwrap();

        let before = source_fingerprint(Language::Solidity, &main).unwrap();
        fs::write(
            lib_dir.join("Lib.sol"),
            "library Lib { function g() internal {} }",
        )
        .unwrap();
        let after = source_fingerprint(Language::Solidity, &main).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn vyper_fingerprint_uses_single_source_file() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("Main.vy");
        fs::write(&main, "# pragma version >=0.4.3,<0.5.0\n").unwrap();
        fs::write(
            dir.path().join("Other.vy"),
            "# pragma version >=0.4.3,<0.5.0\n",
        )
        .unwrap();

        let before = source_fingerprint(Language::Vyper, &main).unwrap();
        fs::write(dir.path().join("Other.vy"), "# changed\n").unwrap();
        let after = source_fingerprint(Language::Vyper, &main).unwrap();

        assert_eq!(before, after);
    }

    fn toolchain(name: &str, version: &str) -> Toolchain {
        Toolchain {
            name: name.to_string(),
            version: version.to_string(),
            binary_path: PathBuf::from(name),
            binary_sha256: "sha256".to_string(),
            download_source: "test".to_string(),
            version_output: version.to_string(),
            metadata: BTreeMap::new(),
        }
    }

    fn profile(id: &str, language: Language) -> CompilerProfile {
        CompilerProfile {
            id: id.to_string(),
            language,
            compiler: language.as_str().to_string(),
            optimizer: false,
            optimizer_runs: 0,
            optimizer_mode: None,
            experimental_codegen: false,
            via_ir: false,
            metadata_mode: MetadataMode::Off,
            source_variant: None,
            evm_version: "prague".to_string(),
        }
    }

    #[test]
    fn real_derived_source_language_uses_declared_profiles() {
        let benchmark = crate::catalog::real_derived_benchmarks()
            .into_iter()
            .find(|benchmark| benchmark.id == "uniswap_v2_factory")
            .unwrap();

        assert!(profile_applies_to_benchmark(
            &benchmark,
            &profile("solc-0.5.16-noopt", Language::Solidity)
        ));
        assert!(!profile_applies_to_benchmark(
            &benchmark,
            &profile("solc-0.4.26-noopt", Language::Solidity)
        ));
        assert!(profile_applies_to_benchmark(
            &benchmark,
            &profile("vyper-0.3.10-gas", Language::Vyper)
        ));
    }

    #[test]
    fn solidity_pragmas_use_resolved_compiler_patch() {
        assert_eq!(
            solidity_pragma_for_toolchain(&toolchain("solc", "0.8.35")).unwrap(),
            "pragma solidity >=0.8.35 <0.9.0;"
        );
        assert_eq!(
            solidity_pragma_for_toolchain(&toolchain("solc-0.5.16", "0.5.16")).unwrap(),
            "pragma solidity >=0.5.16 <0.6.0;"
        );
    }

    #[test]
    fn rewrites_vyper_03_compatibility_syntax() {
        let source = "# pragma version >=0.4.3,<0.5.0\n# pragma optimize codesize\n# pragma evm-version prague\nfrom ethereum.ercs import IERC20\n\n@deploy\ndef __init__():\n    log Transfer(sender=empty(address), receiver=msg.sender, value=1)\n\n@external\n@view\ndef f(xs: DynArray[uint256, 4], token: address) -> bytes32:\n    for item: uint256 in xs:\n        pass\n    assert extcall IERC20(token).transfer(msg.sender, 1, default_return_value=True)\n    amount: uint256 = staticcall IERC20(token).balanceOf(msg.sender)\n    log Approval(\n        owner=msg.sender,\n        spender=token,\n        value=amount,\n    )\n    return keccak256(abi_encode(4 // 2))\n";
        let rewritten =
            transform_vyper_source(source, Some("vyper-0.3"), "# pragma version >=0.3.7,<0.4.0")
                .unwrap();
        assert!(rewritten.contains("# pragma version >=0.3.7,<0.4.0"));
        assert!(!rewritten.contains("# pragma optimize"));
        assert!(!rewritten.contains("# pragma evm-version"));
        assert!(rewritten.contains("from vyper.interfaces import ERC20"));
        assert!(rewritten.contains("@external\ndef __init__"));
        assert!(rewritten.contains("log Transfer(empty(address), msg.sender, 1)"));
        assert!(rewritten.contains("log Approval(msg.sender, token, amount)"));
        assert!(rewritten.contains("for item in xs:"));
        assert!(rewritten.contains("assert ERC20(token).transfer"));
        assert!(rewritten.contains("amount: uint256 = ERC20(token).balanceOf(msg.sender)"));
        assert!(rewritten.contains("_abi_encode(4 / 2)"));
    }

    #[test]
    fn rewrites_solidity_historical_compatibility_syntax() {
        let source = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.35;\n\ninterface YearnBenchERC20 {\n    function transfer(address receiver, uint256 amount) external returns (bool);\n}\n\ncontract C {\n    uint256 public constant FEE_DENOMINATOR = 10_000_000_000;\n    constructor(uint256 initial) {\n    }\n    function f(bytes32[] calldata proof) external pure returns (uint256) {\n        (bool ok,) = msg.sender.call{value: amount}(\"\");\n        (bool ok,) = address(this).staticcall(abi.encodeWithSelector(bytes4(0x773acdef), i));\n        abi.encodeWithSelector(YearnBenchERC20.transfer.selector, msg.sender, 1);\n        return type(uint256).max + type(uint112).max + proof.length + 1_000_000;\n    }\n}\n";
        let rewritten = transform_solidity_source(
            source,
            Some("solidity-0.4"),
            "pragma solidity >=0.4.26 <0.5.0;",
        )
        .unwrap();
        assert!(rewritten.contains("pragma solidity >=0.4.26 <0.5.0;"));
        assert!(rewritten.contains("10000000000"));
        assert!(rewritten.contains("1000000"));
        assert!(rewritten.contains("constructor(uint256 initial) public {"));
        assert!(rewritten.contains("bytes32[] proof"));
        assert!(rewritten.contains("bool ok = msg.sender.call.value(amount)();"));
        assert!(rewritten.contains(
            "bool ok = address(this).call(abi.encodeWithSelector(bytes4(0x773acdef), i));"
        ));
        assert!(rewritten.contains(
            "abi.encodeWithSelector(bytes4(keccak256(\"transfer(address,uint256)\")), msg.sender, 1);"
        ));
        assert!(rewritten.contains("uint256(-1) + uint112(-1)"));
    }

    #[test]
    fn rewrites_solidity_05_abicoder_opt_in() {
        let source = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.35;\n\nstruct StrategyParams {\n    uint256 debt;\n}\n\ncontract C {\n    uint256 public immutable value;\n    function _addLiquidity(uint256[] calldata amounts, uint256 minMintAmount, address receiver) internal {}\n    function f() external pure returns (StrategyParams memory params) {\n        params.debt = 1;\n    }\n}\n";
        let rewritten = transform_solidity_source(
            source,
            Some("solidity-0.5"),
            "pragma solidity >=0.5.16 <0.6.0;",
        )
        .unwrap();
        assert!(
            rewritten
                .contains("pragma solidity >=0.5.16 <0.6.0;\npragma experimental ABIEncoderV2;")
        );
        assert!(rewritten.contains("uint256 public value;"));
        assert!(rewritten.contains(
            "function _addLiquidity(uint256[] memory amounts, uint256 minMintAmount, address receiver)"
        ));
        assert!(!rewritten.contains("immutable"));
    }

    #[test]
    fn rewrites_solidity_06_multiline_constructor_visibility() {
        let source = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.35;\n\ncontract C {\n    constructor(\n        string memory name,\n        uint256 initial\n    ) {\n        name;\n        initial;\n    }\n}\n";
        let rewritten = transform_solidity_source(
            source,
            Some("solidity-0.6"),
            "pragma solidity >=0.6.12 <0.7.0;",
        )
        .unwrap();
        assert!(rewritten.contains("pragma experimental ABIEncoderV2;"));
        assert!(rewritten.contains("    ) public {"));
    }

    #[test]
    fn rewrites_solidity_07_address_code_length() {
        let source = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.35;\n\ncontract C {\n    function f(address owner) external view returns (bool) {\n        return owner.code.length > 0 || address(this).code.length > 0;\n    }\n}\n";
        let rewritten = transform_solidity_source(
            source,
            Some("solidity-0.7"),
            "pragma solidity >=0.7.6 <0.8.0;",
        )
        .unwrap();
        assert!(rewritten.contains("pragma solidity >=0.7.6 <0.8.0;\npragma abicoder v2;"));
        assert!(rewritten.contains(
            "return _benchExtcodesize(owner) > 0 || _benchExtcodesize(address(this)) > 0;"
        ));
        assert!(rewritten.contains(
            "function _benchExtcodesize(address account) internal view returns (uint256 size)"
        ));
    }

    #[test]
    fn rewrites_vyper_02_compatibility_syntax() {
        let source = "# pragma version >=0.4.3,<0.5.0\n\nstruct Strategy:\n    balance: uint256\n\n@external\n@pure\ndef getReserves() -> (uint112, uint112, uint32):\n    self._only_owner()\n    amount0: uint256 = self.balance0\n    return convert(self.reserve0, uint112), convert(self.reserve1, uint112), convert(self.blockTimestampLast, uint32)\n\n@internal\n@view\ndef _only_owner():\n    assert msg.sender == self.owner, \"owner\"\n\n@internal\n@pure\ndef _min(a: uint256, b: uint256) -> uint256:\n    if a < b:\n        return a\n    return b\n";
        let rewritten = transform_vyper_source(
            source,
            Some("vyper-0.2"),
            "# pragma version >=0.2.16,<0.3.0",
        )
        .unwrap();
        assert!(rewritten.contains("# pragma version >=0.2.16,<0.3.0"));
        assert!(rewritten.contains("@view\ndef getReserves() -> (uint256, uint256, uint256):"));
        assert!(rewritten.contains("    strategyBalance: uint256"));
        assert!(rewritten.contains("amount0: uint256 = self.balance0"));
        assert!(rewritten.contains("def _only_owner(sender: address):"));
        assert!(rewritten.contains("assert sender == self.owner"));
        assert!(rewritten.contains("self._only_owner(msg.sender)"));
        assert!(rewritten.contains("return self.reserve0, self.reserve1, self.blockTimestampLast"));
        assert!(rewritten.find("def _min").unwrap() < rewritten.find("def getReserves").unwrap());
    }

    #[test]
    fn rewrites_vyper_03_struct_constructor_assignments() {
        let source = "# pragma version >=0.4.3,<0.5.0\n\nstruct Strategy:\n    activation: uint256\n    currentDebt: uint256\n    maxDebt: uint256\n    balance: uint256\n\nstruct PendingReport:\n    gain: uint256\n    loss: uint256\n\nstrategies: HashMap[address, Strategy]\npendingReports: HashMap[address, PendingReport]\n\n@external\ndef f(strategy: address, gain: uint256, loss: uint256):\n    self.strategies[strategy].activation = 1\n    self.strategies[strategy].currentDebt += gain\n    self.strategies[strategy].maxDebt = loss\n    self.strategies[strategy].balance += gain\n    self.pendingReports[strategy] = PendingReport(gain=gain, loss=loss)\n    self.pendingReports[strategy] = PendingReport(gain=0, loss=0)\n";
        let rewritten = transform_vyper_source(
            source,
            Some("vyper-0.3"),
            "# pragma version >=0.3.10,<0.4.0",
        )
        .unwrap();
        assert!(rewritten.contains("strategyActivation: HashMap[address, uint256]"));
        assert!(rewritten.contains("strategyCurrentDebt: HashMap[address, uint256]"));
        assert!(rewritten.contains("strategyMaxDebt: HashMap[address, uint256]"));
        assert!(rewritten.contains("strategyBalance: HashMap[address, uint256]"));
        assert!(rewritten.contains("self.strategyActivation[strategy] = 1"));
        assert!(rewritten.contains("self.strategyCurrentDebt[strategy] += gain"));
        assert!(rewritten.contains("self.strategyMaxDebt[strategy] = loss"));
        assert!(rewritten.contains("self.strategyBalance[strategy] += gain"));
        assert!(rewritten.contains("pendingReportGain: HashMap[address, uint256]"));
        assert!(rewritten.contains("pendingReportLoss: HashMap[address, uint256]"));
        assert!(rewritten.contains("self.pendingReportGain[strategy] = gain"));
        assert!(rewritten.contains("self.pendingReportLoss[strategy] = loss"));
        assert!(rewritten.contains("self.pendingReportGain[strategy] = 0"));
        assert!(rewritten.contains("self.pendingReportLoss[strategy] = 0"));
    }
}

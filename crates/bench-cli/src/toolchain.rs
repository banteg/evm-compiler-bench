use crate::{
    models::{Language, Toolchain, Toolchains},
    util::{Progress, ensure_dir, require_success, run_measured, sha256_bytes, sha256_file},
};
use anyhow::{Context, Result, anyhow, bail};
use pep440_rs::Version;
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SOLC_INDEX_ROOT: &str = "https://binaries.soliditylang.org";
const PYPI_VYPER_JSON: &str = "https://pypi.org/pypi/vyper/json";
const FE_RELEASES_REPO: &str = "argotorg/fe";
const FE_RELEASES_LATEST: &str = "https://api.github.com/repos/argotorg/fe/releases/latest";
const LEGACY_VYPER_PYTHON: &str = "3.11";
const EVM_ORDER: &[&str] = &["osaka", "prague", "cancun", "shanghai", "paris", "london"];

pub fn resolve_toolchains(
    root: &Path,
    offline: bool,
    profile_filter: &[String],
) -> Result<Toolchains> {
    let compiler_refs = compiler_refs_from_profiles(root, profile_filter)?;
    let extra_compilers: BTreeSet<_> = compiler_refs
        .iter()
        .map(|compiler_ref| compiler_ref.compiler.clone())
        .filter(|compiler| !matches!(compiler.as_str(), "solc" | "vyper" | "vyper-prerelease"))
        .collect();
    let mut progress = Progress::new("toolchains", 3 + extra_compilers.len());
    progress.update(0, "resolving latest solc");
    let solc = resolve_solc(root, offline)?;
    progress.update(1, format!("resolved solc {}", solc.version));
    progress.update(1, "resolving latest vyper");
    let vyper = resolve_vyper(root, offline)?;
    progress.update(2, format!("resolved vyper {}", vyper.version));
    progress.update(2, "resolving latest vyper prerelease");
    let vyper_prerelease = resolve_vyper_prerelease(root, offline)?;
    progress.update(
        3,
        format!("resolved vyper prerelease {}", vyper_prerelease.version),
    );
    let mut compilers = BTreeMap::from([
        ("solc".to_string(), solc.clone()),
        ("vyper".to_string(), vyper.clone()),
        ("vyper-prerelease".to_string(), vyper_prerelease.clone()),
    ]);
    let mut resolved = 3usize;
    for compiler_ref in compiler_refs {
        if compilers.contains_key(&compiler_ref.compiler) {
            continue;
        }
        progress.update(resolved, format!("resolving {}", compiler_ref.compiler));
        let toolchain = match compiler_ref.language {
            Language::Solidity
                if crate::solx::profile_version(&compiler_ref.compiler).is_some() =>
            {
                crate::solx::resolve(
                    root,
                    offline,
                    crate::solx::profile_version(&compiler_ref.compiler).unwrap(),
                )?
            }
            Language::Solidity
                if crate::solar::profile_revision(&compiler_ref.compiler).is_some() =>
            {
                crate::solar::resolve(
                    root,
                    offline,
                    crate::solar::profile_revision(&compiler_ref.compiler).unwrap(),
                )?
            }
            Language::Solidity => {
                let Some(version) = compiler_ref.compiler.strip_prefix("solc-") else {
                    bail!("unsupported solidity compiler {}", compiler_ref.compiler);
                };
                resolve_solc_version(
                    root,
                    offline,
                    version,
                    &env_var_for_version("EVM_BENCH_SOLC", version),
                    "historical",
                )?
            }
            Language::Vyper => {
                let Some(version) = compiler_ref.compiler.strip_prefix("vyper-") else {
                    bail!("unsupported vyper compiler {}", compiler_ref.compiler);
                };
                resolve_vyper_version(
                    root,
                    offline,
                    version,
                    &env_var_for_version("EVM_BENCH_VYPER", version),
                    "historical",
                )?
            }
            Language::Fe => {
                if compiler_ref.compiler != "fe" {
                    bail!("unsupported fe compiler {}", compiler_ref.compiler);
                }
                resolve_fe(root, offline)?
            }
        };
        resolved += 1;
        progress.update(
            resolved,
            format!("resolved {} {}", compiler_ref.compiler, toolchain.version),
        );
        compilers.insert(compiler_ref.compiler, toolchain);
    }
    let mut evm_version = latest_shared_evm(&solc, &[&vyper, &vyper_prerelease])?;
    for toolchain in compilers
        .values()
        .filter(|t| matches!(t.name.as_str(), "solx" | "solar"))
    {
        let start = EVM_ORDER
            .iter()
            .position(|evm| *evm == evm_version)
            .context("shared EVM order")?;
        let mut shared = None;
        for evm in &EVM_ORDER[start..] {
            if (if toolchain.name == "solar" {
                crate::solar::supports_evm(toolchain, evm)
            } else {
                crate::solx::supports_evm(toolchain, evm)
            })? {
                shared = Some((*evm).to_string());
                break;
            }
        }
        evm_version = shared.context("no shared EVM target supported by Solidity backend")?;
    }
    progress.finish(format!(
        "resolved {} compilers; shared EVM {}",
        compilers.len(),
        evm_version
    ));
    Ok(Toolchains {
        solc,
        vyper,
        vyper_prerelease,
        compilers,
        evm_version,
    })
}

fn resolve_solc(root: &Path, offline: bool) -> Result<Toolchain> {
    if offline {
        return resolve_path_toolchain("solc", env::var_os("EVM_BENCH_SOLC").map(PathBuf::from));
    }
    let index = fetch_solc_index()?;
    let latest_release = index.latest_release.clone();
    resolve_solc_from_index(root, &latest_release, "EVM_BENCH_SOLC", "latest", index)
}

fn resolve_solc_version(
    root: &Path,
    offline: bool,
    version: &str,
    env_var: &str,
    channel: &str,
) -> Result<Toolchain> {
    if let Some(local) =
        local_toolchain_if_version("solc", env::var_os(env_var).map(PathBuf::from), version)?
    {
        return Ok(local);
    }
    if let Some(cached) = cached_solc_toolchain(root, version, channel)? {
        return Ok(cached);
    }
    if offline {
        bail!("cached solc {version} not found");
    }
    let index = fetch_solc_index()?;
    resolve_solc_from_index(root, version, env_var, channel, index)
}

fn resolve_solc_from_index(
    root: &Path,
    version: &str,
    env_var: &str,
    channel: &str,
    index: SolcIndex,
) -> Result<Toolchain> {
    if let Some(local) =
        local_toolchain_if_version("solc", env::var_os(env_var).map(PathBuf::from), version)?
    {
        return Ok(local);
    }
    let release_path = index
        .releases
        .get(version)
        .with_context(|| format!("solc release {version} missing from index"))?;
    let build = index
        .builds
        .iter()
        .find(|build| build.path == *release_path)
        .with_context(|| format!("solc build {release_path} missing from index"))?;
    let platform = solc_platform()?;
    let target_dir = root.join(".cache/toolchains/solc").join(version);
    ensure_dir(&target_dir)?;
    let target = target_dir.join(&build.path);
    let source = format!("{SOLC_INDEX_ROOT}/{platform}/{}", build.path);
    if !target.exists() {
        let bytes = reqwest::blocking::get(&source)
            .with_context(|| format!("downloading {source}"))?
            .error_for_status()?
            .bytes()?;
        let actual = sha256_bytes(&bytes);
        let expected = build.sha256.trim_start_matches("0x");
        if actual != expected {
            bail!("solc checksum mismatch for {source}: expected {expected}, got {actual}");
        }
        fs::write(&target, bytes)?;
        make_executable(&target)?;
    }
    let version_output = command_stdout(Command::new(&target).arg("--version"))?;
    Ok(Toolchain {
        name: "solc".to_string(),
        version: parse_solc_version(&version_output)?,
        binary_sha256: sha256_file(&target)?,
        binary_path: target,
        download_source: source,
        version_output,
        metadata: BTreeMap::from([
            ("resolver".to_string(), "solidity_binary_index".to_string()),
            ("index_root".to_string(), SOLC_INDEX_ROOT.to_string()),
            ("platform".to_string(), platform.to_string()),
            ("release".to_string(), version.to_string()),
            ("channel".to_string(), channel.to_string()),
        ]),
    })
}

fn cached_solc_toolchain(root: &Path, version: &str, channel: &str) -> Result<Option<Toolchain>> {
    let target_dir = root.join(".cache/toolchains/solc").join(version);
    if !target_dir.exists() {
        return Ok(None);
    }
    let mut entries = fs::read_dir(&target_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    entries.sort();
    let Some(binary_path) = entries.into_iter().next() else {
        return Ok(None);
    };
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let actual_version = parse_solc_version(&version_output)?;
    if actual_version != version {
        bail!(
            "cached solc at {} has version {actual_version}, expected {version}",
            binary_path.display()
        );
    }
    Ok(Some(Toolchain {
        name: "solc".to_string(),
        version: actual_version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source: format!("{SOLC_INDEX_ROOT}/"),
        version_output,
        metadata: BTreeMap::from([
            (
                "resolver".to_string(),
                "solidity_binary_index_cache".to_string(),
            ),
            ("release".to_string(), version.to_string()),
            ("channel".to_string(), channel.to_string()),
        ]),
    }))
}

fn resolve_fe(root: &Path, offline: bool) -> Result<Toolchain> {
    // A local binary always wins, used for testing an unreleased Fe build.
    if let Some(path) = env::var_os("EVM_BENCH_FE").map(PathBuf::from) {
        return fe_toolchain(path, "local".to_string(), "local_path", "local");
    }
    if offline {
        return cached_fe_latest(root)?
            .context("cached fe toolchain not found; run online once or set EVM_BENCH_FE");
    }
    match resolve_fe_release(root) {
        Ok(toolchain) => Ok(toolchain),
        // The anonymous GitHub API is rate-limited to 60 requests/hour; a
        // cached binary keeps the whole run alive when the lookup fails.
        Err(error) => match cached_fe_latest(root)? {
            Some(toolchain) => {
                eprintln!(
                    "warning: fe release lookup failed ({error:#}); using cached fe {}",
                    toolchain.version
                );
                Ok(toolchain)
            }
            None => Err(error),
        },
    }
}

fn resolve_fe_release(root: &Path) -> Result<Toolchain> {
    let release = fetch_fe_latest_release()?;
    let asset_name = fe_release_asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| {
            format!(
                "fe release {} is missing asset {asset_name}",
                release.tag_name
            )
        })?;
    let version = release.tag_name.trim_start_matches('v').to_string();
    let target_dir = root.join(".cache/toolchains/fe").join(&version);
    let target = target_dir.join(asset_name);
    if !target.exists() {
        ensure_dir(&target_dir)?;
        let bytes = github_client()?
            .get(&asset.browser_download_url)
            .send()
            .with_context(|| format!("downloading {}", asset.browser_download_url))?
            .error_for_status()?
            .bytes()?;
        // Stage and rename so an interrupted download never leaves a
        // truncated binary at the path later runs trust via `exists()`.
        let staging = target_dir.join(format!("{asset_name}.partial"));
        fs::write(&staging, &bytes)?;
        make_executable(&staging)?;
        fs::rename(&staging, &target)?;
    }
    fe_toolchain(
        target,
        asset.browser_download_url.clone(),
        "github_release",
        "stable",
    )
}

fn fe_toolchain(
    binary_path: PathBuf,
    download_source: String,
    resolver: &str,
    channel: &str,
) -> Result<Toolchain> {
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let version = parse_fe_version(&version_output)?;
    Ok(Toolchain {
        name: "fe".to_string(),
        version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source,
        version_output,
        metadata: BTreeMap::from([
            ("resolver".to_string(), resolver.to_string()),
            ("channel".to_string(), channel.to_string()),
            ("repository".to_string(), FE_RELEASES_REPO.to_string()),
        ]),
    })
}

fn cached_fe_latest(root: &Path) -> Result<Option<Toolchain>> {
    let dir = root.join(".cache/toolchains/fe");
    if !dir.exists() {
        return Ok(None);
    }
    let asset_name = fe_release_asset_name()?;
    let mut versions: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    versions.sort_by_key(|path| fe_version_sort_key(path));
    for version in versions.into_iter().rev() {
        let binary = version.join(asset_name);
        if binary.exists() {
            return Ok(Some(fe_toolchain(
                binary,
                "github_release_cache".to_string(),
                "github_release_cache",
                "stable",
            )?));
        }
    }
    Ok(None)
}

fn fe_version_sort_key(path: &Path) -> (u64, u64, u64) {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(stable_version_tuple)
        .unwrap_or((0, 0, 0))
}

fn fetch_fe_latest_release() -> Result<GithubRelease> {
    Ok(github_client()?
        .get(FE_RELEASES_LATEST)
        .send()
        .with_context(|| format!("fetching {FE_RELEASES_LATEST}"))?
        .error_for_status()?
        .json()?)
}

fn github_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("evm-compiler-bench")
        .build()
        .context("building github http client")
}

fn fe_release_asset_name() -> Result<&'static str> {
    Ok(match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "fe_linux_amd64",
        ("linux", "aarch64") => "fe_linux_arm64",
        ("macos", "x86_64") => "fe_mac_amd64",
        ("macos", "aarch64") => "fe_mac_arm64",
        ("windows", "x86_64") => "fe_windows_amd64.exe",
        (os, arch) => bail!("unsupported fe release platform {os}/{arch}"),
    })
}

fn resolve_vyper(root: &Path, offline: bool) -> Result<Toolchain> {
    if offline {
        return resolve_path_toolchain("vyper", env::var_os("EVM_BENCH_VYPER").map(PathBuf::from));
    }
    let latest = latest_vyper_release(&fetch_vyper_releases()?, false)?;
    resolve_vyper_version(root, offline, &latest, "EVM_BENCH_VYPER", "stable")
}

fn resolve_vyper_prerelease(root: &Path, offline: bool) -> Result<Toolchain> {
    let version = if offline {
        if let Some(path) = env::var_os("EVM_BENCH_VYPER_PRERELEASE") {
            let mut local = resolve_path_toolchain("vyper", Some(PathBuf::from(path)))?;
            if !local.version.parse::<Version>()?.any_prerelease() {
                bail!("EVM_BENCH_VYPER_PRERELEASE must report a prerelease version");
            }
            local.metadata.insert("channel".into(), "prerelease".into());
            return Ok(local);
        }
        cached_vyper_prerelease(root)?
            .context("no cached Vyper prerelease; run online or set EVM_BENCH_VYPER_PRERELEASE")?
    } else {
        latest_vyper_release(&fetch_vyper_releases()?, true)?
    };
    let mut toolchain = resolve_vyper_version(
        root,
        offline,
        &version,
        "EVM_BENCH_VYPER_PRERELEASE",
        "prerelease",
    )?;
    toolchain
        .metadata
        .insert("channel".into(), "prerelease".into());
    Ok(toolchain)
}

fn cached_vyper_prerelease(root: &Path) -> Result<Option<String>> {
    let directory = root.join(".cache/toolchains/vyper");
    if !directory.exists() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(version) = name.parse::<Version>() else {
            continue;
        };
        if version.any_prerelease()
            && entry
                .path()
                .join(bin_dir())
                .join(binary_name("vyper"))
                .is_file()
        {
            candidates.push((version, name));
        }
    }
    Ok(candidates.into_iter().max().map(|(_, name)| name))
}

fn resolve_vyper_version(
    root: &Path,
    offline: bool,
    version: &str,
    env_var: &str,
    channel: &str,
) -> Result<Toolchain> {
    if let Some(local) =
        local_toolchain_if_version("vyper", env::var_os(env_var).map(PathBuf::from), version)?
    {
        return Ok(local);
    }

    let venv = root.join(".cache/toolchains/vyper").join(version);
    let binary = venv.join(bin_dir()).join(binary_name("vyper"));
    let legacy_python = legacy_vyper_python(version);
    if binary.exists() && legacy_python.is_some() && !cached_vyper_uses_legacy_python(&venv) {
        fs::remove_dir_all(&venv)
            .with_context(|| format!("refreshing legacy vyper venv {}", venv.display()))?;
    }
    if binary.exists() {
        return cached_vyper_toolchain(&binary, version, channel);
    }
    if offline {
        bail!("cached vyper {version} not found at {}", binary.display());
    }
    if !binary.exists() {
        ensure_dir(&venv)?;
        let mut venv_command = Command::new("uv");
        venv_command.arg("venv").arg(&venv);
        if let Some(python) = legacy_python {
            venv_command.arg("--python").arg(python);
        }
        require_success(run_measured(&mut venv_command, None)?, "uv venv")?;
        let python = venv.join(bin_dir()).join(binary_name("python"));
        let mut install_command = Command::new("uv");
        install_command
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&python)
            .arg("--prerelease")
            .arg("allow")
            .arg(format!("vyper=={version}"));
        if needs_setuptools_pin(version) {
            install_command.arg("setuptools==80.9.0");
        }
        require_success(
            run_measured(&mut install_command, None)?,
            "uv pip install vyper",
        )?;
    }
    cached_vyper_toolchain(&binary, version, channel)
}

fn legacy_vyper_python(version: &str) -> Option<&'static str> {
    if version_tuple_loose(version).is_some_and(|tuple| tuple < (0, 4, 0)) {
        Some(LEGACY_VYPER_PYTHON)
    } else {
        None
    }
}

fn needs_setuptools_pin(version: &str) -> bool {
    version_tuple_loose(version).is_some_and(|tuple| tuple < (0, 3, 0))
}

fn cached_vyper_uses_legacy_python(venv: &Path) -> bool {
    let python = venv.join(bin_dir()).join(binary_name("python"));
    command_stdout(Command::new(&python).arg("--version"))
        .ok()
        .is_some_and(|version| version.contains("Python 3.11"))
}

fn cached_vyper_toolchain(binary: &Path, version: &str, channel: &str) -> Result<Toolchain> {
    let version_output = command_stdout(Command::new(binary).arg("--version"))?;
    let actual_version = parse_vyper_version(&version_output)?;
    if actual_version != version {
        bail!(
            "cached vyper at {} has version {actual_version}, expected {version}",
            binary.display()
        );
    }
    let venv = binary
        .parent()
        .and_then(|bin| bin.parent())
        .context("vyper venv root")?;
    let python = venv.join(bin_dir()).join(binary_name("python"));
    let mut metadata = BTreeMap::from([
        ("resolver".to_string(), "pypi_uv_venv".to_string()),
        ("pypi_json".to_string(), PYPI_VYPER_JSON.to_string()),
        ("package".to_string(), format!("vyper=={version}")),
        ("channel".to_string(), channel.to_string()),
    ]);
    if let Ok(uv_version) = command_stdout(Command::new("uv").arg("--version")) {
        metadata.insert("uv_version".to_string(), uv_version.trim().to_string());
    }
    if let Ok(python_version) = command_stdout(Command::new(&python).arg("--version")) {
        metadata.insert(
            "python_version".to_string(),
            python_version.trim().to_string(),
        );
    }
    Ok(Toolchain {
        name: "vyper".to_string(),
        version: actual_version,
        binary_sha256: sha256_file(binary)?,
        binary_path: binary.to_path_buf(),
        download_source: format!("https://pypi.org/project/vyper/{version}/"),
        version_output,
        metadata,
    })
}

fn resolve_path_toolchain(name: &str, env_path: Option<PathBuf>) -> Result<Toolchain> {
    let binary_path = match env_path {
        Some(path) => path,
        None => which::which(name).with_context(|| format!("{name} not found on PATH"))?,
    };
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let version = match name {
        "solc" => parse_solc_version(&version_output)?,
        "vyper" => parse_vyper_version(&version_output)?,
        _ => return Err(anyhow!("unknown toolchain {name}")),
    };
    Ok(Toolchain {
        name: name.to_string(),
        version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source: "local".to_string(),
        version_output,
        metadata: BTreeMap::from([("resolver".to_string(), "local_path".to_string())]),
    })
}

fn local_toolchain_if_version(
    name: &str,
    env_path: Option<PathBuf>,
    latest: &str,
) -> Result<Option<Toolchain>> {
    let Some(binary_path) = env_path else {
        return Ok(None);
    };
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let version = match name {
        "vyper" => parse_vyper_version(&version_output)?,
        "solc" => parse_solc_version(&version_output)?,
        _ => return Err(anyhow!("unknown toolchain {name}")),
    };
    if version != latest {
        return Ok(None);
    }
    Ok(Some(Toolchain {
        name: name.to_string(),
        version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source: "local".to_string(),
        version_output,
        metadata: BTreeMap::from([("resolver".to_string(), "local_path".to_string())]),
    }))
}

fn latest_shared_evm(solc: &Toolchain, vypers: &[&Toolchain]) -> Result<String> {
    let solc_help = command_stdout(Command::new(&solc.binary_path).arg("--help"))?;
    let vyper_helps = vypers
        .iter()
        .map(|vyper| command_stdout(Command::new(&vyper.binary_path).arg("--help")))
        .collect::<Result<Vec<_>>>()?;
    for evm in EVM_ORDER {
        if solc_help.contains(evm) && vyper_helps.iter().all(|help| help.contains(evm)) {
            return Ok((*evm).to_string());
        }
    }
    bail!("could not find shared EVM target between solc and vyper");
}

fn fetch_solc_index() -> Result<SolcIndex> {
    let platform = solc_platform()?;
    let url = format!("{SOLC_INDEX_ROOT}/{platform}/list.json");
    Ok(reqwest::blocking::get(&url)
        .with_context(|| format!("fetching {url}"))?
        .error_for_status()?
        .json()?)
}

fn fetch_vyper_releases() -> Result<PypiPackage> {
    reqwest::blocking::get(PYPI_VYPER_JSON)
        .with_context(|| format!("fetching {PYPI_VYPER_JSON}"))?
        .error_for_status()?
        .json()
        .context("parsing Vyper PyPI releases")
}

fn latest_vyper_release(payload: &PypiPackage, prerelease: bool) -> Result<String> {
    payload
        .releases
        .iter()
        .filter(|(_, files)| files.iter().any(|file| !file.yanked))
        .filter_map(|(name, _)| {
            let version = name.parse::<Version>().ok()?;
            (version.any_prerelease() == prerelease).then(|| (version, name.clone()))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, version)| version)
        .with_context(|| {
            format!(
                "no installable Vyper {} releases found on PyPI",
                if prerelease { "prerelease" } else { "stable" }
            )
        })
}

fn stable_version_tuple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn version_tuple_loose(version: &str) -> Option<(u64, u64, u64)> {
    let alpha_index = version
        .char_indices()
        .find_map(|(index, ch)| ch.is_ascii_alphabetic().then_some(index));
    let core = alpha_index.map_or(version, |index| &version[..index]);
    stable_version_tuple(core)
}

#[derive(Debug, Deserialize)]
struct ProfileCompilerRef {
    id: String,
    language: Language,
    compiler: String,
}

fn compiler_refs_from_profiles(
    root: &Path,
    profile_filter: &[String],
) -> Result<Vec<ProfileCompilerRef>> {
    let mut refs = Vec::new();
    let profiles_dir = root.join("compiler-profiles");
    if !profiles_dir.exists() {
        return Ok(refs);
    }
    for entry in fs::read_dir(profiles_dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        let compiler_ref: ProfileCompilerRef =
            toml::from_str(&text).with_context(|| format!("parsing {}", entry.path().display()))?;
        refs.push(compiler_ref);
    }
    if !profile_filter.is_empty() {
        let available = refs
            .iter()
            .map(|compiler_ref| compiler_ref.id.as_str())
            .collect::<BTreeSet<_>>();
        let requested = profile_filter
            .iter()
            .map(|profile| profile.as_str())
            .collect::<BTreeSet<_>>();
        let unknown = requested
            .iter()
            .filter(|profile| !available.contains(**profile))
            .copied()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            let available = available.into_iter().collect::<Vec<_>>().join(", ");
            bail!(
                "unknown compiler profile(s): {}; available profiles: {available}",
                unknown.join(", ")
            );
        }
        refs.retain(|compiler_ref| requested.contains(compiler_ref.id.as_str()));
    }
    Ok(refs)
}

fn env_var_for_version(prefix: &str, version: &str) -> String {
    let suffix = version
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{prefix}_{suffix}")
}

fn command_stdout(command: &mut Command) -> Result<String> {
    let output = require_success(run_measured(command, None)?, "command")?.output;
    Ok(String::from_utf8(output.stdout)?)
}

fn parse_solc_version(output: &str) -> Result<String> {
    parse_version(output, r"Version:\s*([0-9]+\.[0-9]+\.[0-9]+)")
}

fn parse_vyper_version(output: &str) -> Result<String> {
    output
        .split_whitespace()
        .filter_map(|token| token.split('+').next()?.parse::<Version>().ok())
        .find(|version| version.release().len() >= 3)
        .map(|version| version.to_string())
        .context("unable to parse Vyper version output")
}

fn parse_fe_version(output: &str) -> Result<String> {
    parse_version(output, r"fe\s+([0-9]+\.[0-9]+\.[0-9]+)")
}

fn parse_version(output: &str, pattern: &str) -> Result<String> {
    let re = Regex::new(pattern)?;
    let captures = re
        .captures(output)
        .with_context(|| format!("could not parse version from {output:?}"))?;
    Ok(captures[1].to_string())
}

fn solc_platform() -> Result<&'static str> {
    match env::consts::OS {
        "macos" => Ok("macosx-amd64"),
        "linux" => Ok("linux-amd64"),
        "windows" => Ok("windows-amd64"),
        other => bail!("unsupported solc platform {other}"),
    }
}

fn bin_dir() -> &'static str {
    if cfg!(windows) { "Scripts" } else { "bin" }
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolcIndex {
    latest_release: String,
    releases: BTreeMap<String, String>,
    builds: Vec<SolcBuild>,
}

#[derive(Debug, Deserialize)]
struct SolcBuild {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct PypiPackage {
    releases: BTreeMap<String, Vec<PypiFile>>,
}

#[derive(Debug, Deserialize)]
struct PypiFile {
    #[serde(default)]
    yanked: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[cfg(test)]
mod tests {
    use super::{
        PypiPackage, cached_fe_latest, cached_vyper_prerelease, compiler_refs_from_profiles,
        fe_release_asset_name, latest_vyper_release, make_executable, parse_solc_version,
        parse_vyper_version, stable_version_tuple,
    };
    use std::fs;

    #[test]
    fn parses_solc_version() {
        assert_eq!(
            parse_solc_version("Version: 0.8.35+commit.whatever.Darwin.appleclang").unwrap(),
            "0.8.35"
        );
    }

    #[test]
    fn parses_vyper_version() {
        assert_eq!(
            parse_vyper_version("0.4.3+commit.bff19ea2").unwrap(),
            "0.4.3"
        );
        assert_eq!(
            parse_vyper_version("0.5.0a1+commit.7d73c468").unwrap(),
            "0.5.0a1"
        );
        for version in ["0.5.0b1", "0.5.0rc1", "0.6.0.dev12", "0.5.0b2.dev1"] {
            assert_eq!(
                parse_vyper_version(&format!("{version}+commit.abcdef12\n")).unwrap(),
                version
            );
        }
    }

    #[test]
    fn classifies_stable_vyper_versions() {
        assert_eq!(stable_version_tuple("0.4.3"), Some((0, 4, 3)));
        assert_eq!(stable_version_tuple("0.5.0a1"), None);
    }

    #[test]
    fn selects_installable_vyper_releases_by_python_version_order() {
        let mut payload: PypiPackage = serde_json::from_value(serde_json::json!({
            "releases": {
                "0.4.3": [{"yanked": false}],
                "0.5.0a99": [{}],
                "0.5.0b2": [{}],
                "0.5.0b10": [{"yanked": true}, {"yanked": false}],
                "0.5.0rc1": [{"yanked": true}],
                "0.5.0": [],
                "invalid": [{}]
            }
        }))
        .unwrap();
        assert_eq!(latest_vyper_release(&payload, false).unwrap(), "0.4.3");
        assert_eq!(latest_vyper_release(&payload, true).unwrap(), "0.5.0b10");
        payload.releases.get_mut("0.5.0rc1").unwrap()[0].yanked = false;
        assert_eq!(latest_vyper_release(&payload, true).unwrap(), "0.5.0rc1");
        payload
            .releases
            .insert("0.5.0".into(), vec![super::PypiFile { yanked: false }]);
        assert_eq!(latest_vyper_release(&payload, false).unwrap(), "0.5.0");
        assert_eq!(latest_vyper_release(&payload, true).unwrap(), "0.5.0rc1");
        payload
            .releases
            .insert("0.6.0.dev1".into(), vec![super::PypiFile { yanked: false }]);
        assert_eq!(latest_vyper_release(&payload, true).unwrap(), "0.6.0.dev1");
        payload.releases.clear();
        assert!(latest_vyper_release(&payload, true).is_err());
        assert!(latest_vyper_release(&payload, false).is_err());
    }

    #[test]
    fn offline_prerelease_selection_ignores_stable_and_incomplete_caches() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(cached_vyper_prerelease(root.path()).unwrap(), None);
        for version in ["0.5.0a99", "0.5.0b2", "0.5.0b10", "0.5.0rc1", "0.6.0"] {
            let directory = root
                .path()
                .join(".cache/toolchains/vyper")
                .join(version)
                .join(super::bin_dir());
            fs::create_dir_all(&directory).unwrap();
            if version != "0.5.0rc1" {
                fs::write(
                    directory.join(super::binary_name("vyper")),
                    "cached compiler",
                )
                .unwrap();
            }
        }
        assert_eq!(
            cached_vyper_prerelease(root.path()).unwrap().as_deref(),
            Some("0.5.0b10")
        );
    }

    #[test]
    fn filters_compiler_refs_by_requested_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("compiler-profiles");
        fs::create_dir(&profiles).unwrap();
        fs::write(
            profiles.join("solc.toml"),
            r#"
id = "solc-latest-noopt"
language = "solidity"
compiler = "solc"
"#,
        )
        .unwrap();
        fs::write(
            profiles.join("fe.toml"),
            r#"
id = "fe-latest-O2"
language = "fe"
compiler = "fe"
"#,
        )
        .unwrap();

        let filter = vec!["solc-latest-noopt".to_string()];
        let refs = compiler_refs_from_profiles(dir.path(), &filter).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].id, "solc-latest-noopt");
        assert_eq!(refs[0].compiler, "solc");

        let missing = vec!["missing".to_string()];
        assert!(compiler_refs_from_profiles(dir.path(), &missing).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cached_fe_latest_skips_incomplete_newer_version() {
        let dir = tempfile::tempdir().unwrap();
        let asset_name = fe_release_asset_name().unwrap();
        let old_dir = dir.path().join(".cache/toolchains/fe/1.0.0");
        let new_dir = dir.path().join(".cache/toolchains/fe/2.0.0");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();
        let binary = old_dir.join(asset_name);
        fs::write(&binary, "#!/bin/sh\necho 'fe 1.0.0'\n").unwrap();
        make_executable(&binary).unwrap();

        let cached = cached_fe_latest(dir.path()).unwrap().unwrap();
        assert_eq!(cached.version, "1.0.0");
        assert_eq!(cached.binary_path, binary);
    }
}

//! Pinned solx releases have three independent identities: solx, its embedded
//! Solidity frontend, and LLVM. Never interpret the solx version as Solidity.
use crate::{
    models::{CompilerProfile, Toolchain},
    util::{ensure_dir, sha256_bytes, sha256_file},
};
use anyhow::{Context, Result, bail};
use regex::Regex;
use std::{collections::BTreeMap, env, fs, path::Path, process::Command};

pub const WORKER_THREADS: usize = 1;
const RELEASE_ROOT: &str = "https://github.com/NomicFoundation/solx/releases/download";

pub fn profile_version(compiler: &str) -> Option<&str> {
    compiler.strip_prefix("solx-")
}

pub fn validate_profile(profile: &CompilerProfile) -> Result<()> {
    let version = profile_version(&profile.compiler).context("expected pinned solx compiler")?;
    if !Regex::new(r"^\d+\.\d+\.\d+$")?.is_match(version) {
        bail!("solx compiler must pin a release: {}", profile.compiler);
    }
    if !matches!(profile.optimizer_mode.as_deref(), Some("3" | "z"))
        || !profile.optimizer
        || profile.optimizer_runs != 0
        || profile.via_ir
        || profile.experimental_codegen
    {
        bail!(
            "{}: solx profiles require optimizer=true, optimizer_mode=3 or z, zero optimizer_runs, and legacy frontend codegen",
            profile.id
        );
    }
    Ok(())
}

pub fn resolve(root: &Path, offline: bool, version: &str) -> Result<Toolchain> {
    let override_key = format!("EVM_BENCH_SOLX_{}", version.replace('.', "_"));
    if let Some(binary) = env::var_os(&override_key).or_else(|| env::var_os("EVM_BENCH_SOLX")) {
        let binary = Path::new(&binary)
            .canonicalize()
            .context("resolving solx override")?;
        return describe(&binary, version, "local_override", "local_override", None);
    }
    let asset = release_asset_name(version, env::consts::OS, env::consts::ARCH)?;
    let dir = root.join(".cache/toolchains/solx").join(version);
    let binary = dir.join(&asset);
    let checksum_path = dir.join(format!("{asset}.sha256"));
    let url = format!("{RELEASE_ROOT}/{version}/{asset}");
    if !binary.exists() || !checksum_path.exists() {
        if offline {
            bail!(
                "verified cached solx {version} not found at {}",
                binary.display()
            );
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("evm-compiler-bench")
            .build()?;
        let checksum = client
            .get(format!("{url}.sha256"))
            .send()?
            .error_for_status()?
            .text()?;
        let expected = parse_checksum(&checksum)?;
        let bytes = client.get(&url).send()?.error_for_status()?.bytes()?;
        verify_checksum(&bytes, &expected)?;
        ensure_dir(&dir)?;
        fs::write(&binary, bytes)?;
        fs::write(&checksum_path, checksum)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
        }
    }
    let expected = parse_checksum(&fs::read_to_string(&checksum_path)?)?;
    // Verify cache hits too, before executing the binary.
    verify_checksum(&fs::read(&binary)?, &expected)?;
    describe(&binary, version, &url, "github_release", Some(&expected))
}

fn describe(
    binary: &Path,
    expected_version: &str,
    source: &str,
    resolver: &str,
    checksum: Option<&str>,
) -> Result<Toolchain> {
    let output = Command::new(binary).arg("--version").output()?;
    if !output.status.success() {
        bail!(
            "solx --version failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let version_output = String::from_utf8(output.stdout)?;
    let (version, mut metadata) = parse_version_output(&version_output)?;
    if version != expected_version {
        bail!("solx override/cache reports {version}, profile requires {expected_version}");
    }
    metadata.insert("resolver".into(), resolver.into());
    metadata.insert("repository".into(), "NomicFoundation/solx".into());
    metadata.insert("channel".into(), "pinned".into());
    if let Some(checksum) = checksum {
        metadata.insert("upstream_sha256".into(), checksum.into());
    }
    Ok(Toolchain {
        name: "solx".into(),
        version,
        binary_path: binary.to_path_buf(),
        binary_sha256: sha256_file(binary)?,
        download_source: source.into(),
        version_output,
        metadata,
    })
}

fn parse_version_output(output: &str) -> Result<(String, BTreeMap<String, String>)> {
    let captures =
        Regex::new(r"(?m)^solx v(\d+\.\d+\.\d+),.*Front end: (\w+), LLVM build: ([0-9a-f]+)")?
            .captures(output)
            .context("unrecognized solx --version identity")?;
    if &captures[2] != "solc" {
        bail!(
            "unsupported solx frontend {}; this adapter requires the solc frontend",
            &captures[2]
        );
    }
    let frontend = Regex::new(r"(?m)^Version: (\d+\.\d+\.\d+)\+commit\.([0-9a-f]+)")?
        .captures(output)
        .context("missing embedded Solidity frontend version")?;
    Ok((
        captures[1].into(),
        BTreeMap::from([
            ("frontend".into(), captures[2].into()),
            ("frontend_version".into(), frontend[1].into()),
            ("frontend_commit".into(), frontend[2].into()),
            ("llvm_build".into(), captures[3].into()),
        ]),
    ))
}

fn release_asset_name(version: &str, os: &str, arch: &str) -> Result<String> {
    let platform = match (os, arch) {
        ("macos", "x86_64" | "aarch64") => "macosx",
        ("linux", "x86_64") => "linux-amd64-gnu",
        ("linux", "aarch64") => "linux-arm64-gnu",
        ("windows", "x86_64") => "windows-amd64-gnu",
        _ => bail!("unsupported solx platform {os}/{arch}"),
    };
    Ok(format!(
        "solx-{platform}-v{version}{}",
        if os == "windows" { ".exe" } else { "" }
    ))
}

fn parse_checksum(text: &str) -> Result<String> {
    let digest = text
        .split_whitespace()
        .next()
        .context("empty solx checksum")?;
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid solx SHA-256 checksum");
    }
    Ok(digest.to_ascii_lowercase())
}

fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_bytes(bytes);
    if actual != expected {
        bail!("solx checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

pub fn supports_evm(toolchain: &Toolchain, evm: &str) -> Result<bool> {
    // Probe the actual bundled frontend/backend instead of inferring support
    // from a help string or a release number.
    let input = serde_json::to_vec(&serde_json::json!({
        "language": "Solidity",
        "sources": {"Probe.sol": {"content": "contract Probe { function f() external pure returns (uint256) { return 1; } }"}},
        "settings": {"evmVersion": evm, "optimizer": {"mode": "3"},
            "outputSelection": {"*": {"*": ["evm.bytecode.object"]}}}
    }))?;
    let output = crate::util::run_measured(
        Command::new(&toolchain.binary_path)
            .arg("--standard-json")
            .arg("--threads")
            .arg(WORKER_THREADS.to_string()),
        Some(&input),
    )?;
    if !output.output.status.success() {
        return Ok(false);
    }
    let value: serde_json::Value = serde_json::from_slice(&output.output.stdout)?;
    Ok(!value["errors"]
        .as_array()
        .is_some_and(|errors| errors.iter().any(|e| e["severity"] == "error"))
        && value
            .pointer("/contracts/Probe.sol/Probe/evm/bytecode/object")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_release_frontend_and_llvm_versions() {
        let (release, metadata) = parse_version_output("solx v0.1.8, LLVM-based Solidity compiler for the EVM, Front end: solc, LLVM build: 7d0702e169\nVersion: 0.8.34+commit.91fef221.Darwin.appleclang\n").unwrap();
        assert_eq!(release, "0.1.8");
        assert_eq!(metadata["frontend_version"], "0.8.34");
        assert_eq!(metadata["frontend_commit"], "91fef221");
        assert_eq!(metadata["llvm_build"], "7d0702e169");
        assert!(parse_version_output("solx v0.1.8").is_err());
    }
    #[test]
    fn verifies_release_checksums_and_platforms() {
        let digest = sha256_bytes(b"compiler");
        assert_eq!(
            parse_checksum(&format!("{digest}  solx-macosx-v0.1.8\n")).unwrap(),
            digest
        );
        assert!(verify_checksum(b"corrupt", &digest).is_err());
        assert!(parse_checksum("not-a-checksum").is_err());
        assert_eq!(
            release_asset_name("0.1.8", "macos", "aarch64").unwrap(),
            "solx-macosx-v0.1.8"
        );
        assert_eq!(
            release_asset_name("0.1.8", "windows", "x86_64").unwrap(),
            "solx-windows-amd64-gnu-v0.1.8.exe"
        );
    }
}

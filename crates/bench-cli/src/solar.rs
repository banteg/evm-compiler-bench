//! Solar's package version, source revision, and Solidity compatibility are distinct.
use crate::{
    models::{CompilerProfile, Toolchain},
    util::{ensure_dir, sha256_file},
};
use anyhow::{Context, Result, bail};
use regex::Regex;
use std::{collections::BTreeMap, env, fs, path::Path, process::Command};
const REPOSITORY: &str = "https://github.com/paradigmxyz/solar";
const RUST: &str = "1.96.0";
pub fn profile_revision(compiler: &str) -> Option<&str> {
    compiler.strip_prefix("solar-")
}
fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        bail!("Solar requires a full lowercase 40-character commit SHA");
    }
    Ok(())
}
pub fn validate_profile(profile: &CompilerProfile) -> Result<()> {
    validate_revision(
        profile_revision(&profile.compiler).context("expected pinned Solar compiler")?,
    )?;
    let valid_mode = matches!(
        (profile.optimizer_mode.as_deref(), profile.optimizer_runs),
        (Some("gas"), 200) | (Some("size"), 1)
    );
    if !valid_mode || !profile.optimizer || profile.via_ir || profile.experimental_codegen {
        bail!(
            "{}: Solar requires gas/runs200 or size/runs1, optimizer=true, and no viaIR/experimental switch",
            profile.id
        );
    }
    Ok(())
}
fn checked(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("running {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub fn resolve(root: &Path, offline: bool, revision: &str) -> Result<Toolchain> {
    validate_revision(revision)?;
    if let Some(binary) = env::var_os("EVM_BENCH_SOLAR") {
        return describe(
            &Path::new(&binary).canonicalize()?,
            revision,
            "local_override",
            BTreeMap::new(),
        );
    }
    let dir = root
        .join(".cache/toolchains/solar")
        .join(revision)
        .join(format!("{}-{}", env::consts::OS, env::consts::ARCH));
    let binary = dir.join(format!("solar{}", env::consts::EXE_SUFFIX));
    let receipt = dir.join("build.json");
    if !binary.exists() || !receipt.exists() {
        if offline {
            bail!("cached Solar {revision} unavailable; resolve online or set EVM_BENCH_SOLAR");
        }
        ensure_dir(&dir)?;
        checked(Command::new("rustup").args([
            "toolchain",
            "install",
            RUST,
            "--profile",
            "minimal",
        ]))?;
        let source = dir.join("source");
        if !source.exists() {
            checked(Command::new("git").arg("init").arg(&source))?;
        }
        checked(Command::new("git").current_dir(&source).args([
            "fetch",
            "--depth=1",
            REPOSITORY,
            revision,
        ]))?;
        checked(
            Command::new("git")
                .current_dir(&source)
                .args(["checkout", "--detach", revision]),
        )?;
        let head = checked(
            Command::new("git")
                .current_dir(&source)
                .args(["rev-parse", "HEAD"]),
        )?;
        let dirty = checked(Command::new("git").current_dir(&source).args([
            "status",
            "--porcelain",
            "--untracked-files=all",
        ]))?;
        if head != revision || !dirty.is_empty() {
            bail!("Solar build source must be clean at {revision}");
        }
        let rustc = checked(Command::new("rustup").args(["run", RUST, "rustc", "-vV"]))?;
        let target = rustc
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .context("Rust host triple")?
            .to_string();
        checked(
            Command::new("cargo")
                .current_dir(&source)
                .args([
                    &format!("+{RUST}"),
                    "build",
                    "--release",
                    "--locked",
                    "--target",
                    &target,
                    "-p",
                    "solar-compiler",
                    "--bin",
                    "solar",
                ])
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env("CARGO_TARGET_DIR", source.join("target")),
        )?;
        fs::copy(
            source
                .join("target")
                .join(&target)
                .join("release")
                .join(format!("solar{}", env::consts::EXE_SUFFIX)),
            &binary,
        )?;
        let metadata = BTreeMap::from([
            ("rustc".into(), rustc),
            ("build_target".into(), target),
            (
                "build_flags".into(),
                "--release --locked -p solar-compiler --bin solar; RUSTFLAGS unset".into(),
            ),
            (
                "cargo_lock_sha256".into(),
                sha256_file(&source.join("Cargo.lock"))?,
            ),
        ]);
        let described = describe(&binary, revision, "source_build", metadata)?;
        fs::write(&receipt, serde_json::to_vec_pretty(&described)?)?;
    }
    let built: Toolchain = serde_json::from_slice(&fs::read(&receipt)?)?;
    if sha256_file(&binary)? != built.binary_sha256 {
        bail!("cached Solar binary checksum differs from its build receipt");
    }
    describe(&binary, revision, "source_build", built.metadata)
}
fn describe(
    binary: &Path,
    revision: &str,
    resolver: &str,
    mut metadata: BTreeMap<String, String>,
) -> Result<Toolchain> {
    let version_output = checked(
        Command::new(binary)
            .arg("--version")
            .env_remove("SOLC_WRAPPER"),
    )?;
    let captures = Regex::new(r"(?m)^(?:solar )?Version:\s*(\d+\.\d+\.\d+)")?
        .captures(&version_output)
        .context("Solar package version")?;
    let version = captures[1].to_string();
    let commit = Regex::new(r"(?m)^Commit SHA:\s*([0-9a-f]{40})")?
        .captures(&version_output)
        .context("Solar source revision")?;
    if &commit[1] != revision {
        bail!("Solar binary reports {}, expected {revision}", &commit[1]);
    }
    let wrapper = checked(
        Command::new(binary)
            .arg("--version")
            .env("SOLC_WRAPPER", "1"),
    )?;
    let compatibility = Regex::new(r"Version:\s*(\d+\.\d+\.\d+)\+commit\.")?
        .captures(&wrapper)
        .context("Solar Solidity compatibility version")?;
    metadata.extend(BTreeMap::from([
        ("resolver".into(), resolver.into()),
        ("repository".into(), "paradigmxyz/solar".into()),
        ("source_revision".into(), revision.into()),
        ("solidity_version".into(), compatibility[1].into()),
        ("channel".into(), "pinned_commit".into()),
    ]));
    Ok(Toolchain {
        name: "solar".into(),
        version,
        binary_path: binary.to_path_buf(),
        binary_sha256: sha256_file(binary)?,
        download_source: format!("{REPOSITORY}/tree/{revision}"),
        version_output,
        metadata,
    })
}
pub fn supports_evm(toolchain: &Toolchain, evm: &str) -> Result<bool> {
    let input = serde_json::to_vec(
        &serde_json::json!({"language":"Solidity","sources":{"Probe.sol":{"content":"contract Probe { function f() external pure returns(uint256) { return 1; } }"}},"settings":{"evmVersion":evm,"optimizer":{"enabled":true,"runs":200},"outputSelection":{"*":{"*":["evm.bytecode.object"]}}}}),
    )?;
    let output = crate::util::run_measured(
        Command::new(&toolchain.binary_path)
            .args(["--standard-json", "--threads", "1"])
            .env_remove("SOLC_WRAPPER"),
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
    #[test]
    fn rejects_moving_or_short_revisions() {
        for invalid in [
            "main",
            "0.2.0",
            "716e9cbc",
            "716E9CBCde88165f931173f1c1fda852ed63afa0",
        ] {
            assert!(super::validate_revision(invalid).is_err());
        }
        assert!(super::validate_revision("716e9cbcde88165f931173f1c1fda852ed63afa0").is_ok());
    }
}

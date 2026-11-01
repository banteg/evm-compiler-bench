//! Execute independent harness shards without sharing Foundry build caches or
//! output files. The Solidity source and its relative evidence paths are kept
//! byte-for-byte unchanged inside each job's private project.
use crate::util::{ensure_dir, require_success, run_measured};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};

/// Flags shared by `forge test` and the recorded `forge config`, so the
/// captured configuration matches the measurement run. Under `--isolate`, every
/// top-level call from the test contract runs as its own committed transaction,
/// so measured calls start from cold storage whose original values were written
/// by earlier transactions.
pub(crate) const HARNESS_FLAGS: [&str; 5] = [
    "--via-ir",
    "--optimize",
    "--isolate",
    "--chain-id",
    HARNESS_CHAIN_ID,
];

/// Pre-0.8 Solidity has no `block.chainid`, so its source rewrite embeds the
/// chain id the harness runs on; targets and fixtures then sign the same domain.
pub(crate) const HARNESS_CHAIN_ID: &str = "1";

fn worker_count() -> Result<usize> {
    match std::env::var("EVM_BENCH_FOUNDRY_JOBS") {
        Ok(value) => match value.parse::<usize>() {
            Ok(n @ 1..=8) => Ok(n),
            _ => bail!("EVM_BENCH_FOUNDRY_JOBS must be an integer from 1 to 8"),
        },
        Err(std::env::VarError::NotPresent) => Ok(std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4)),
        Err(error) => Err(error.into()),
    }
}

pub fn run(root: &Path, evm: &str, paths: &[String]) -> Result<()> {
    let workers = worker_count()?.min(paths.len());
    let next = AtomicUsize::new(0);
    let (send, receive) = mpsc::channel();
    let started = std::time::Instant::now();
    eprintln!(
        "foundry: {} isolated shards, {workers} workers",
        paths.len()
    );
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let send = send.clone();
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };
                    let result = execute(root, evm, path, None);
                    if result.is_err() {
                        // Let running shards finish, but do not start further work.
                        next.store(paths.len(), Ordering::Relaxed);
                    }
                    if send.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(send);
        let mut error = None;
        for (completed, (index, result)) in receive.into_iter().enumerate() {
            eprintln!(
                "foundry: {}/{} finished shard {} [{}s]",
                completed + 1,
                paths.len(),
                index + 1,
                started.elapsed().as_secs()
            );
            if let Err(failure) = result {
                error.get_or_insert(failure);
            }
        }
        error.map_or(Ok(()), Err)
    })
}

pub fn execute(
    root: &Path,
    evm: &str,
    relative: &str,
    expected_tests: Option<&BTreeSet<String>>,
) -> Result<()> {
    let file = Path::new(relative)
        .file_name()
        .context("Foundry shard filename")?;
    let job = root.join("target/foundry-jobs").join(file);
    let project = job.join("foundry");
    let raw = job.join("results/raw");
    ensure_dir(&project.join("test"))?;
    ensure_dir(&project.join("src"))?;
    ensure_dir(&raw)?;
    // Outputs belong to this attempt, even when the compiler cache is reused.
    for entry in fs::read_dir(&raw)? {
        let path = entry?.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    fs::copy(
        root.join("foundry/foundry.toml"),
        project.join("foundry.toml"),
    )?;
    fs::copy(
        root.join("foundry").join(relative),
        project.join("test").join(file),
    )?;
    let measured = run_measured(
        Command::new("forge")
            .arg("test")
            .arg("--root")
            .arg(&project)
            .arg("--match-path")
            .arg(relative)
            .arg("--evm-version")
            .arg(evm)
            .args(HARNESS_FLAGS)
            .arg("--threads")
            .arg("1")
            .arg(if expected_tests.is_some() {
                "--json"
            } else {
                "-q"
            }),
        None,
    )?;
    // Each gas shard has a distinct filename. Preserve failure evidence too,
    // with a shard prefix to prevent collisions between independent jobs.
    for entry in fs::read_dir(&raw)? {
        let path = entry?.path();
        if path.is_file() {
            fs::copy(
                &path,
                root.join("results/raw").join(path.file_name().unwrap()),
            )?;
        }
    }
    let failures = raw.join("failures");
    if failures.exists() {
        ensure_dir(&root.join("results/raw/failures"))?;
        for entry in fs::read_dir(failures)? {
            let path = entry?.path();
            if path.is_file() {
                let name = format!(
                    "{}-{}",
                    file.to_string_lossy(),
                    path.file_name().unwrap().to_string_lossy()
                );
                fs::copy(path, root.join("results/raw/failures").join(name))?;
            }
        }
    }
    // Retain Forge's individual outcomes for diagnosing this shard, including failed attempts.
    if expected_tests.is_some() {
        fs::write(
            job.join("behavior-test-results.json"),
            &measured.output.stdout,
        )?;
    }
    let measured = require_success(
        measured,
        &format!(
            "forge test {relative}; isolated project {}",
            project.display()
        ),
    )?;
    if let Some(expected) = expected_tests {
        let name = relative
            .strip_prefix("test/")
            .and_then(|file| file.strip_suffix(".t.sol"))
            .context("invalid generated behavior shard path")?;
        verify_behavior_results(
            &measured.output.stdout,
            &format!("{relative}:{name}"),
            expected,
        )?;
    }
    Ok(())
}

fn verify_behavior_results(output: &[u8], suite: &str, expected: &BTreeSet<String>) -> Result<()> {
    let report: serde_json::Value = serde_json::from_slice(output)
        .with_context(|| format!("no usable Forge test results for {suite}"))?;
    let actual = report
        .get(suite)
        .and_then(|s| s.get("test_results"))
        .and_then(serde_json::Value::as_object)
        .with_context(|| format!("Forge omitted behavior suite {suite}"))?;
    for name in expected {
        let result = actual
            .get(name)
            .with_context(|| format!("Forge did not execute {suite}::{name}"))?;
        if result["status"] != "Success" {
            bail!("Forge did not pass {suite}::{name}: {}", result["status"]);
        }
    }
    if actual.len() != expected.len() || expected.is_empty() {
        bail!(
            "unexpected Forge test inventory for {suite}: {} reported, {} requested",
            actual.len(),
            expected.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_evidence_requires_every_named_test_to_pass() {
        let expected = BTreeSet::from(["testA()".to_owned(), "testB()".to_owned()]);
        let verify = |data: &str| verify_behavior_results(data.as_bytes(), "suite", &expected);
        assert!(verify("").is_err());
        assert!(verify("{}").is_err());
        assert!(verify(r#"{"suite":{"test_results":{"testA()":{"status":"Success"}}}}"#).is_err());
        assert!(verify(r#"{"suite":{"test_results":{"testA()":{"status":"Success"},"testB()":{"status":"Skipped"}}}}"#).is_err());
        assert!(verify(r#"{"suite":{"test_results":{"testA()":{"status":"Success"},"testB()":{"status":"Success"}}}}"#).is_ok());
    }
}

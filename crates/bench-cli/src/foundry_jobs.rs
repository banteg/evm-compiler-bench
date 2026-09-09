//! Execute independent harness shards without sharing Foundry build caches or
//! output files. The Solidity source and its relative evidence paths are kept
//! byte-for-byte unchanged inside each job's private project.
use crate::util::{ensure_dir, require_success, run_measured};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};

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
                    let result = execute(root, evm, path);
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

pub fn execute(root: &Path, evm: &str, relative: &str) -> Result<()> {
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
            .arg("--via-ir")
            .arg("--optimize")
            .arg("--threads")
            .arg("1")
            .arg("-q"),
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
    require_success(
        measured,
        &format!(
            "forge test {relative}; isolated project {}",
            project.display()
        ),
    )?;
    Ok(())
}

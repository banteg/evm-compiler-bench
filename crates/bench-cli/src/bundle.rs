//! The producer exposes a run to the publisher only after every output succeeds.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

pub const FILES: [&str; 4] = [
    "results/normalized/results.json",
    "results/normalized/run-manifest.json",
    "results/normalized/report-model.json",
    "results/raw/foundry-gas.jsonl",
];

#[derive(Serialize, Deserialize)]
struct Receipt {
    run_id: String,
    sha256: BTreeMap<String, String>,
}

pub fn lock(root: &Path) -> Result<fs::File> {
    let target = root.join("target");
    fs::create_dir_all(&target)?;
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target.join("bench-run.lock"))?;
    file.try_lock()
        .context("another benchmark command is using this checkout")?;
    Ok(file)
}

pub fn freeze(root: &Path, run_id: &str) -> Result<()> {
    let runs = root.join("target/bench-runs");
    fs::create_dir_all(&runs)?;
    let staging = tempfile::Builder::new()
        .prefix(".pending-")
        .tempdir_in(&runs)?;
    let mut sha256 = BTreeMap::new();
    for relative in FILES {
        let source = root.join(relative);
        let name = source.file_name().unwrap();
        let target = staging.path().join(name);
        fs::copy(&source, &target)?;
        sha256.insert(
            name.to_string_lossy().into_owned(),
            crate::util::sha256_file(&target)?,
        );
    }
    let receipt = serde_json::to_vec_pretty(&Receipt {
        run_id: run_id.into(),
        sha256,
    })?;
    fs::write(staging.path().join("receipt.json"), &receipt)?;
    fs::rename(staging.path(), runs.join(run_id)).context("finalizing completed run bundle")?;
    let normalized = root.join("results/normalized");
    let mut pointer = tempfile::NamedTempFile::new_in(&normalized)?;
    pointer.write_all(&receipt)?;
    pointer.persist(normalized.join("bundle-receipt.json"))?;
    Ok(())
}

pub fn check_working_files(root: &Path, run_id: &str) -> Result<()> {
    ensure!(
        run_id.bytes().all(|b| b.is_ascii_alphanumeric()) && !run_id.is_empty(),
        "invalid run ID"
    );
    let receipt: Receipt = serde_json::from_slice(
        &fs::read(root.join("results/normalized/bundle-receipt.json"))
            .context("missing completed run bundle; rerun the pipeline")?,
    )?;
    ensure!(
        receipt.run_id == run_id,
        "bundle receipt disagrees with run manifest"
    );
    for relative in FILES {
        let source = root.join(relative);
        let name = source.file_name().unwrap().to_str().unwrap();
        ensure!(
            receipt.sha256.get(name) == Some(&crate::util::sha256_file(&source)?),
            "{relative} disagrees with the completed run bundle"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_run_keeps_previous_snapshot_and_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for relative in FILES {
            let file = root.join(relative);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, relative).unwrap();
        }
        freeze(root, "first").unwrap();
        check_working_files(root, "first").unwrap();
        let pointer = root.join("results/normalized/bundle-receipt.json");
        let published = fs::read(&pointer).unwrap();
        fs::write(root.join(FILES[0]), "changed").unwrap();
        assert!(check_working_files(root, "first").is_err());
        fs::remove_file(root.join(FILES[3])).unwrap();
        assert!(freeze(root, "second").is_err());
        assert!(!root.join("target/bench-runs/second").exists());
        assert_eq!(fs::read(pointer).unwrap(), published);
        assert_eq!(
            fs::read_to_string(root.join("target/bench-runs/first/results.json")).unwrap(),
            FILES[0]
        );
    }
}

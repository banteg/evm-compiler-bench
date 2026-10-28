//! Persist evidence for the exact compiler pairs that passed behavioral tests.
//! Gas cache hits and another profile's property tests are not such evidence.
use crate::{
    baselines::comparison_pairs,
    cache::{self, CacheLookup},
    models::{CacheInfo, CompileSet, CompiledArtifact},
    runner,
    scenarios::ScenarioCatalog,
    util::{ensure_dir, sha256_bytes},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Evidence {
    pub benchmark_id: String,
    pub baseline_profile: String,
    pub compared_profile: String,
    pub scenario_count: usize,
    pub randomized: bool,
    pub properties: Vec<String>,
    pub cache: CacheInfo,
}

impl Evidence {
    pub fn covers(&self, artifact: &CompiledArtifact) -> bool {
        self.benchmark_id == artifact.benchmark_id
            && (self.baseline_profile == artifact.profile_id
                || self.compared_profile == artifact.profile_id)
    }
}

pub fn read(root: &Path) -> Result<Vec<Evidence>> {
    let path = root.join("results/raw/behavior-checks.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn run(
    root: &Path,
    evm: &str,
    compiled: &CompileSet,
    scenarios: &ScenarioCatalog,
    use_cache: bool,
) -> Result<()> {
    ensure_dir(&root.join("results/raw"))?;
    // Never let an interrupted run leave evidence from an older matrix.
    fs::write(root.join("results/raw/behavior-checks.json"), "[]")?;
    let forge = Command::new("forge").arg("--version").output()?;
    if !forge.status.success() {
        bail!("forge --version failed");
    }
    let forge_version = String::from_utf8(forge.stdout)?;
    let config_hash = cache::key_for(&runner::harness_config(root, evm)?)?;
    let mut records = Vec::new();
    let mut pending = BTreeMap::new();
    let mut wanted = Vec::new();
    // Pairs come from the full compile set; shards only run what they are given.
    for (benchmark, a, b) in comparison_pairs(&compiled.artifacts) {
        let left = &compiled.artifacts[a];
        let right = &compiled.artifacts[b];
        let scenario = scenarios.get(&benchmark)?;
        let input = json!({
            "schema": "behavior-v1", "evm": evm, "forge": forge_version,
            "harness": runner::harness_identity(), "harness_config": config_hash, "scenario": scenario,
            "left": {"profile": left.profile_id, "source": left.source_hash,
                "creation": sha256_bytes(left.creation_bytecode.as_bytes()), "settings": left.compiler_settings},
            "right": {"profile": right.profile_id, "source": right.source_hash,
                "creation": sha256_bytes(right.creation_bytecode.as_bytes()), "settings": right.compiler_settings},
        });
        let key = cache::key_for(&input)?;
        let id = cache::logical_id(&["behavior", &benchmark, &left.profile_id, &right.profile_id]);
        if use_cache
            && let CacheLookup::Hit(mut evidence) =
                cache::lookup::<Evidence>(root, "behavior", &id, &key, &input)?
        {
            evidence.cache = CacheInfo::hit(&key);
            records.push(evidence);
            continue;
        }
        wanted.push((benchmark.clone(), a, b));
        let evidence = Evidence {
            benchmark_id: benchmark,
            baseline_profile: left.profile_id.clone(),
            compared_profile: right.profile_id.clone(),
            scenario_count: scenario.scenarios.len(),
            randomized: scenario.randomized.is_some(),
            properties: scenario.properties.iter().map(|p| p.name.clone()).collect(),
            cache: if use_cache {
                CacheInfo::miss(&key)
            } else {
                CacheInfo::disabled()
            },
        };
        pending.insert(
            (
                evidence.benchmark_id.clone(),
                evidence.baseline_profile.clone(),
                evidence.compared_profile.clone(),
            ),
            (id, key, input, evidence),
        );
    }
    eprintln!(
        "behavior: {} verified pairs cached, {} pairs to test",
        records.len(),
        pending.len()
    );
    let shards = runner::behavior_shards(&compiled.artifacts, &wanted, scenarios)?;
    for (index, shard) in shards.iter().enumerate() {
        eprintln!("behavior: running shard {}/{}", index + 1, shards.len());
        runner::run_behavior_shard(root, evm, shard, scenarios, index)?;
        for (benchmark, a, b) in &shard.pairs {
            let identity = (
                benchmark.clone(),
                shard.artifacts[*a].profile_id.clone(),
                shard.artifacts[*b].profile_id.clone(),
            );
            let (id, key, input, evidence) = pending
                .remove(&identity)
                .context("behavior shard ran an unrequested pair")?;
            if use_cache {
                cache::store(root, "behavior", &id, &key, &input, &evidence)?;
            }
            records.push(evidence);
        }
    }
    records.sort_by(|a, b| {
        (&a.benchmark_id, &a.baseline_profile, &a.compared_profile).cmp(&(
            &b.benchmark_id,
            &b.baseline_profile,
            &b.compared_profile,
        ))
    });
    fs::write(
        root.join("results/raw/behavior-checks.json"),
        serde_json::to_vec_pretty(&records)?,
    )
    .context("writing verified behavior evidence")?;
    Ok(())
}

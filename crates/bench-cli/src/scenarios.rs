use crate::{
    harness::{property_benchmark_id, supports_deployment_variant, supports_randomized},
    models::ScenarioFile,
};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct ScenarioCatalog {
    files: BTreeMap<String, ScenarioFile>,
}

impl ScenarioCatalog {
    pub fn get(&self, benchmark_id: &str) -> Result<&ScenarioFile> {
        self.files
            .get(benchmark_id)
            .with_context(|| format!("missing scenario file for {benchmark_id}"))
    }

    pub fn iter(&self) -> impl Iterator<Item = &ScenarioFile> {
        self.files.values()
    }
}

pub fn load_scenario_catalog(
    root: &Path,
    only_benchmark: Option<&str>,
    generated: &[ScenarioFile],
) -> Result<ScenarioCatalog> {
    let scenario_dir = root.join("benches/scenarios");
    let mut files = BTreeMap::new();
    for path in yaml_files(&scenario_dir)? {
        let text = fs::read_to_string(&path)?;
        let file: ScenarioFile =
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        validate_scenario_file(&file, &path)?;
        if only_benchmark.is_none_or(|id| id == file.benchmark_id)
            && files.insert(file.benchmark_id.clone(), file).is_some()
        {
            bail!("duplicate scenario benchmark id in {}", path.display());
        }
    }
    for file in generated {
        validate_scenario_file(file, Path::new("generated scenario"))?;
        if only_benchmark.is_none_or(|id| id == file.benchmark_id)
            && files
                .insert(file.benchmark_id.clone(), file.clone())
                .is_some()
        {
            bail!(
                "duplicate generated scenario benchmark id {}",
                file.benchmark_id
            );
        }
    }
    if let Some(benchmark_id) = only_benchmark
        && !files.contains_key(benchmark_id)
    {
        bail!("missing scenario file for requested benchmark {benchmark_id}");
    }
    Ok(ScenarioCatalog { files })
}

pub fn validate_scenario_file(file: &ScenarioFile, path: &Path) -> Result<()> {
    if file.benchmark_id.trim().is_empty() {
        bail!("{} has empty benchmark_id", path.display());
    }
    if file.scenarios.is_empty() {
        bail!("{} has no scenarios", path.display());
    }
    let mut names = BTreeMap::new();
    for scenario in &file.scenarios {
        if scenario.name.trim().is_empty() {
            bail!("{} has scenario with empty name", path.display());
        }
        if !supports_deployment_variant(&file.benchmark_id, scenario.deployment_variant) {
            bail!(
                "{} scenario {} has deployment_variant for unsupported benchmark {}",
                path.display(),
                scenario.name,
                file.benchmark_id
            );
        }
        if names.insert(scenario.name.clone(), ()).is_some() {
            bail!(
                "{} has duplicate scenario {}",
                path.display(),
                scenario.name
            );
        }
        if scenario.measured.data.trim().is_empty() {
            bail!(
                "{} scenario {} has empty measured call",
                path.display(),
                scenario.name
            );
        }
        for (label, calls) in [
            ("setup", &scenario.setup),
            ("warmup", &scenario.warmup),
            ("observers", &scenario.observers),
        ] {
            for call in calls {
                if call.data.trim().is_empty() {
                    bail!(
                        "{} scenario {} has empty {label} call",
                        path.display(),
                        scenario.name
                    );
                }
            }
        }
    }
    if let Some(randomized) = &file.randomized
        && randomized.iterations == 0
    {
        bail!("{} randomized iterations must be non-zero", path.display());
    }
    if file.randomized.is_some() && !supports_randomized(&file.benchmark_id) {
        bail!(
            "{} has randomized config for unsupported benchmark {}",
            path.display(),
            file.benchmark_id
        );
    }
    for property in &file.properties {
        if property_benchmark_id(&property.name) != Some(file.benchmark_id.as_str()) {
            bail!(
                "{} has unsupported property {} for benchmark {}",
                path.display(),
                property.name,
                file.benchmark_id
            );
        }
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
    use super::validate_scenario_file;
    use crate::models::{
        CallDestination, CallSpec, DeploymentVariant, Scenario, ScenarioFile, StateAccessProfile,
    };
    use std::path::Path;

    #[test]
    fn rejects_empty_scenario_list() {
        let file = ScenarioFile {
            benchmark_id: "counter".to_string(),
            scenarios: vec![],
            randomized: None,
            properties: vec![],
        };
        assert!(validate_scenario_file(&file, Path::new("counter.yaml")).is_err());
    }

    #[test]
    fn accepts_minimal_scenario_file() {
        let file = ScenarioFile {
            benchmark_id: "counter".to_string(),
            scenarios: vec![Scenario {
                name: "read".to_string(),
                deployment_variant: DeploymentVariant::Standard,
                state_access_profile: StateAccessProfile::Cold,
                setup: vec![],
                warmup: vec![],
                measured: CallSpec {
                    data: "abi.encodeWithSignature(\"value()\")".to_string(),
                    sender: None,
                    value: "0".to_string(),
                    destination: CallDestination::Target,
                },
                expect_success: true,
                observers: vec![],
            }],
            randomized: None,
            properties: vec![],
        };
        validate_scenario_file(&file, Path::new("counter.yaml")).unwrap();
    }
}

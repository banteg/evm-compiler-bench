use crate::models::{CompiledArtifact, Language};
use std::collections::BTreeMap;

const SOLIDITY_BASELINE_PREFERENCES: &[&str] = &[
    "solc-latest-legacy-runs200",
    "solc-0.8.30-legacy-runs200",
    "solc-0.8.20-legacy-runs200",
    "solc-0.8.13-legacy-runs200",
    "solc-0.8.0-legacy-runs200",
    "solc-0.5.17-legacy-runs200",
    "solc-0.5.16-legacy-runs200",
    "solc-0.4.26-legacy-runs200",
];

const VYPER_BASELINE_PREFERENCES: &[&str] = &[
    "vyper-latest-gas",
    "vyper-prerelease-gas",
    "vyper-0.4.0-gas",
    "vyper-0.3.10-gas",
    "vyper-0.3.7-default",
    "vyper-0.3.7-none",
    "vyper-0.2.16-default",
];

pub fn baseline_pairs(artifacts: &[CompiledArtifact]) -> BTreeMap<String, (usize, usize)> {
    let mut candidates: BTreeMap<String, BaselineCandidates> = BTreeMap::new();

    for (index, artifact) in artifacts.iter().enumerate() {
        let entry = candidates.entry(artifact.benchmark_id.clone()).or_default();
        let score = baseline_score(artifact.language, &artifact.profile_id);
        match artifact.language {
            Language::Solidity if artifact.compiler.name != "solc" => {}
            Language::Solidity => {
                update_candidate(&mut entry.solidity, index, score, &artifact.profile_id)
            }
            Language::Vyper => {
                update_candidate(&mut entry.vyper, index, score, &artifact.profile_id)
            }
            // Baseline pairs drive the Solidity-vs-Vyper pairwise lane; Fe rows
            // are reported standalone and do not form baseline pairs.
            Language::Fe => {}
        }
    }

    candidates
        .into_iter()
        .filter_map(|(benchmark_id, candidates)| {
            Some((
                benchmark_id,
                (candidates.solidity?.index, candidates.vyper?.index),
            ))
        })
        .collect()
}

/// Cross-language baselines plus same-source alternative Solidity compiler comparisons. A backend
/// release is never treated as a Solidity language version or a solc baseline.
pub fn comparison_pairs(artifacts: &[CompiledArtifact]) -> Vec<(String, usize, usize)> {
    let mut pairs: Vec<_> = baseline_pairs(artifacts)
        .into_iter()
        .map(|(benchmark, (a, b))| (benchmark, a, b))
        .collect();
    for (candidate, artifact) in artifacts
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a.compiler.name.as_str(), "solx" | "solar"))
    {
        let Some(frontend) = artifact
            .compiler
            .metadata
            .get(if artifact.compiler.name == "solar" {
                "solidity_version"
            } else {
                "frontend_version"
            })
        else {
            continue;
        };
        let baseline = artifacts
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                a.benchmark_id == artifact.benchmark_id
                    && a.compiler.name == "solc"
                    && a.compiler.version == *frontend
                    && a.source_hash == artifact.source_hash
                    && a.metadata_mode == artifact.metadata_mode
                    && a.compiler_settings["evmVersion"] == artifact.compiler_settings["evmVersion"]
            })
            .min_by_key(|(_, a)| {
                (
                    a.compiler_settings["viaIR"].as_bool() != Some(true),
                    a.compiler_settings["optimizerRuns"].as_u64() != Some(200),
                    &a.profile_id,
                )
            });
        if let Some((baseline, _)) = baseline {
            pairs.push((artifact.benchmark_id.clone(), baseline, candidate));
        }
    }
    pairs
}

#[derive(Default)]
struct BaselineCandidates {
    solidity: Option<BaselineCandidate>,
    vyper: Option<BaselineCandidate>,
}

struct BaselineCandidate {
    index: usize,
    score: usize,
    profile_id: String,
}

fn update_candidate(
    slot: &mut Option<BaselineCandidate>,
    index: usize,
    score: usize,
    profile_id: &str,
) {
    let should_replace = match slot.as_ref() {
        Some(candidate) => (score, profile_id) < (candidate.score, candidate.profile_id.as_str()),
        None => true,
    };
    if should_replace {
        *slot = Some(BaselineCandidate {
            index,
            score,
            profile_id: profile_id.to_string(),
        });
    }
}

fn baseline_score(language: Language, profile_id: &str) -> usize {
    let preferences = match language {
        Language::Solidity => SOLIDITY_BASELINE_PREFERENCES,
        Language::Vyper => VYPER_BASELINE_PREFERENCES,
        Language::Fe => &[],
    };
    preferences
        .iter()
        .position(|preferred| *preferred == profile_id)
        .unwrap_or_else(|| fallback_score(language, profile_id))
}

fn fallback_score(language: Language, profile_id: &str) -> usize {
    match language {
        Language::Solidity if profile_id.contains("-legacy-runs200") => 100,
        Language::Solidity if profile_id.contains("-viair-runs200") => 200,
        Language::Solidity => 300,
        Language::Vyper if profile_id.contains("-gas") && !profile_id.contains("-venom") => 100,
        Language::Vyper if profile_id.contains("-default") => 200,
        Language::Vyper if profile_id.contains("-none") && !profile_id.contains("-venom") => 300,
        Language::Vyper => 400,
        Language::Fe => 100,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn solar_compares_by_compatibility_and_never_becomes_a_solc_baseline() {
        use crate::test_support::artifact;
        let mut solar = artifact("solar", "solar-716e9cbc-gas-runs200");
        solar.compiler.version = "0.2.0".into();
        solar
            .compiler
            .metadata
            .insert("solidity_version".into(), "0.8.36".into());
        let mut solc = artifact("solc", "solc-0.8.36-viair-runs200");
        solc.compiler.version = "0.8.36".into();
        assert_eq!(
            super::comparison_pairs(&[solar.clone(), solc.clone()]),
            vec![("erc20_minimal".into(), 1, 0)]
        );
        let mut vyper = artifact("vyper", "vyper-latest-gas");
        vyper.language = crate::models::Language::Vyper;
        assert!(super::baseline_pairs(&[solar.clone(), vyper]).is_empty());
        solc.compiler.version = "0.2.0".into();
        assert!(super::comparison_pairs(&[solar, solc]).is_empty());
    }

    #[test]
    fn solx_pairs_only_with_the_matching_solidity_source_and_frontend() {
        use crate::test_support::artifact;
        let solx = artifact("solx", "solx-0.1.8-O3");
        let solc = artifact("solc", "solc-0.8.34-viair-runs200");
        let mut vyper = artifact("vyper", "vyper-latest-gas");
        vyper.language = crate::models::Language::Vyper;
        assert!(super::baseline_pairs(&[solx.clone(), vyper]).is_empty());
        assert_eq!(
            super::comparison_pairs(&[solc.clone(), solx.clone()]),
            vec![("erc20_minimal".into(), 0, 1)]
        );
        let mut different = solc.clone();
        different.source_hash = "different-source".into();
        assert!(super::comparison_pairs(&[different, solx.clone()]).is_empty());
        let mut different = solc;
        different.compiler.version = "0.8.35".into();
        assert!(super::comparison_pairs(&[different, solx]).is_empty());
    }
    use super::baseline_score;
    use crate::models::Language;

    #[test]
    fn ranks_latest_profiles_before_compatibility_variants() {
        assert!(
            baseline_score(Language::Solidity, "solc-latest-legacy-runs200")
                < baseline_score(Language::Solidity, "solc-0.5.16-legacy-runs200")
        );
        assert!(
            baseline_score(Language::Solidity, "solc-0.5.16-legacy-runs200")
                < baseline_score(Language::Solidity, "solc-0.5.16-noopt")
        );
        assert!(
            baseline_score(Language::Vyper, "vyper-latest-gas")
                < baseline_score(Language::Vyper, "vyper-0.3.10-gas")
        );
        assert!(
            baseline_score(Language::Vyper, "vyper-0.3.10-gas")
                < baseline_score(Language::Vyper, "vyper-0.3.10-none")
        );
        assert!(
            baseline_score(Language::Vyper, "vyper-0.3.7-default")
                < baseline_score(Language::Vyper, "vyper-0.3.7-none")
        );
    }
}

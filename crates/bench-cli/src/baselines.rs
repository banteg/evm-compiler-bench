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
    "vyper-0.5.0a1-gas",
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

use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Solidity,
    Vyper,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solidity => "solidity",
            Self::Vyper => "vyper",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BenchmarkSuite {
    Fixed,
    Scale,
    #[serde(rename = "real_derived")]
    RealDerived,
}

impl BenchmarkSuite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Scale => "scale",
            Self::RealDerived => "real_derived",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub model_kind: String,
    pub upstream_project: String,
    pub repository_url: String,
    pub source_commit: String,
    pub source_path: String,
    pub source_language: Language,
    pub source_contract: String,
    #[serde(default)]
    pub source_blob: Option<String>,
    pub upstream_license: String,
    pub checked_at: String,
    pub production_equivalence: bool,
    pub api_compatibility: String,
    pub storage_layout_compatibility: bool,
    pub external_token_semantics: String,
    pub source_derivation: String,
    pub equivalence_scope: Vec<String>,
    pub scenario_coverage: Vec<String>,
    pub mock_assumptions: Vec<String>,
    pub included_features: Vec<String>,
    pub excluded_features: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Benchmark {
    pub id: String,
    pub contract_name: String,
    pub solidity_path: String,
    pub vyper_path: String,
    pub suite: BenchmarkSuite,
    pub family: Option<String>,
    pub parameter_name: Option<String>,
    pub parameter_value: Option<u64>,
    pub scenario_path: Option<String>,
    pub scenario_hash: Option<String>,
    pub generator_version: Option<String>,
    pub provenance: Option<Provenance>,
}

impl Benchmark {
    pub fn fixed(id: &str, contract_name: &str, solidity_path: &str, vyper_path: &str) -> Self {
        Self {
            id: id.to_string(),
            contract_name: contract_name.to_string(),
            solidity_path: solidity_path.to_string(),
            vyper_path: vyper_path.to_string(),
            suite: BenchmarkSuite::Fixed,
            family: None,
            parameter_name: None,
            parameter_value: None,
            scenario_path: None,
            scenario_hash: None,
            generator_version: None,
            provenance: None,
        }
    }

    pub fn real_derived(
        id: &str,
        contract_name: &str,
        solidity_path: &str,
        vyper_path: &str,
        provenance: Provenance,
    ) -> Self {
        Self {
            id: id.to_string(),
            contract_name: contract_name.to_string(),
            solidity_path: solidity_path.to_string(),
            vyper_path: vyper_path.to_string(),
            suite: BenchmarkSuite::RealDerived,
            family: None,
            parameter_name: None,
            parameter_value: None,
            scenario_path: None,
            scenario_hash: None,
            generator_version: None,
            provenance: Some(provenance),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CallSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_expr: Option<String>,
    #[serde(default, rename = "function", skip_serializing_if = "Option::is_none")]
    pub function_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<SolArg>,
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default = "default_call_value")]
    pub value: String,
    #[serde(
        default = "default_call_destination",
        skip_serializing_if = "is_default_call_destination"
    )]
    pub destination: CallDestination,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SolArg {
    Expr(String),
    Typed(BTreeMap<String, YamlValue>),
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CallDestination {
    Target,
    Harness,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentVariant {
    Standard,
    CurveOracle,
    CurveRebasing,
    CurveErc4626,
    CurveThreeCoin,
    CurveEightCoin,
    CurveFiveCoin,
    UniswapCreate2,
}

impl DeploymentVariant {
    pub fn as_solidity_arg(self) -> &'static str {
        match self {
            Self::Standard => "0",
            Self::CurveOracle => "1",
            Self::CurveRebasing => "2",
            Self::CurveErc4626 => "3",
            Self::CurveThreeCoin => "4",
            Self::CurveEightCoin => "5",
            Self::CurveFiveCoin => "6",
            Self::UniswapCreate2 => "1",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StateAccessProfile {
    Cold,
    Warm,
    Mixed,
}

impl StateAccessProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataMode {
    On,
    Off,
}

impl MetadataMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(
        default = "default_deployment_variant",
        skip_serializing_if = "is_default_deployment_variant"
    )]
    pub deployment_variant: DeploymentVariant,
    #[serde(default = "default_state_access_profile")]
    pub state_access_profile: StateAccessProfile,
    #[serde(default)]
    pub setup: Vec<CallSpec>,
    #[serde(default)]
    pub warmup: Vec<CallSpec>,
    pub measured: CallSpec,
    pub expect_success: bool,
    #[serde(default)]
    pub observers: Vec<CallSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RandomizedSpec {
    pub seed: u64,
    #[serde(default = "default_random_iterations")]
    pub iterations: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertySpec {
    pub name: String,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioFile {
    pub benchmark_id: String,
    pub scenarios: Vec<Scenario>,
    #[serde(default)]
    pub randomized: Option<RandomizedSpec>,
    #[serde(default)]
    pub properties: Vec<PropertySpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfile {
    pub id: String,
    pub language: Language,
    pub compiler: String,
    #[serde(default)]
    pub optimizer: bool,
    #[serde(default)]
    pub optimizer_runs: u32,
    #[serde(default)]
    pub optimizer_mode: Option<String>,
    #[serde(default)]
    pub experimental_codegen: bool,
    #[serde(default)]
    pub via_ir: bool,
    #[serde(default = "default_metadata_mode")]
    pub metadata_mode: MetadataMode,
    #[serde(default)]
    pub source_variant: Option<String>,
    pub evm_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Toolchain {
    pub name: String,
    pub version: String,
    pub binary_path: PathBuf,
    pub binary_sha256: String,
    pub download_source: String,
    pub version_output: String,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Toolchains {
    pub solc: Toolchain,
    pub vyper: Toolchain,
    pub vyper_alpha: Toolchain,
    pub compilers: BTreeMap<String, Toolchain>,
    pub evm_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandStats {
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub peak_rss_kib: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompileMetrics {
    pub wall_ms_samples: Vec<f64>,
    pub cpu_ms_samples: Vec<f64>,
    pub peak_rss_kib: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BytecodeMetrics {
    pub creation_bytes: usize,
    pub creation_bytes_stripped: usize,
    pub runtime_bytes: usize,
    pub runtime_bytes_stripped: usize,
    pub initcode_bytes: usize,
    pub linked_runtime_bytes: usize,
    pub eip170_margin_bytes: isize,
    pub eip3860_margin_bytes: isize,
    pub code_deposit_gas: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheInfo {
    pub status: String,
    pub key: String,
    #[serde(default)]
    pub invalidated_by: Vec<String>,
}

impl CacheInfo {
    pub fn disabled() -> Self {
        Self {
            status: "disabled".to_string(),
            key: String::new(),
            invalidated_by: Vec::new(),
        }
    }

    pub fn hit(key: impl Into<String>) -> Self {
        Self {
            status: "hit".to_string(),
            key: key.into(),
            invalidated_by: Vec::new(),
        }
    }

    pub fn refreshed(key: impl Into<String>) -> Self {
        Self {
            status: "refreshed".to_string(),
            key: key.into(),
            invalidated_by: Vec::new(),
        }
    }

    pub fn miss(key: impl Into<String>) -> Self {
        Self {
            status: "miss".to_string(),
            key: key.into(),
            invalidated_by: Vec::new(),
        }
    }

    pub fn stale(key: impl Into<String>, invalidated_by: Vec<String>) -> Self {
        Self {
            status: "stale".to_string(),
            key: key.into(),
            invalidated_by,
        }
    }
}

impl Default for CacheInfo {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompiledArtifact {
    pub benchmark_id: String,
    pub implementation_id: String,
    pub suite: BenchmarkSuite,
    pub family: Option<String>,
    pub parameter_name: Option<String>,
    pub parameter_value: Option<u64>,
    pub scenario_path: Option<String>,
    pub scenario_hash: Option<String>,
    pub generator_version: Option<String>,
    pub provenance: Option<Provenance>,
    pub language: Language,
    pub contract_name: String,
    pub profile_id: String,
    pub compiler: Toolchain,
    pub compiler_settings: serde_json::Value,
    pub metadata_mode: MetadataMode,
    pub source_path: PathBuf,
    pub source_hash: String,
    pub abi: serde_json::Value,
    pub creation_bytecode: String,
    pub runtime_bytecode: String,
    pub compile: CompileMetrics,
    pub bytecode: BytecodeMetrics,
    #[serde(default)]
    pub cache: CacheInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompileFailure {
    pub benchmark_id: String,
    pub implementation_id: String,
    pub suite: BenchmarkSuite,
    pub family: Option<String>,
    pub parameter_name: Option<String>,
    pub parameter_value: Option<u64>,
    pub scenario_path: Option<String>,
    pub scenario_hash: Option<String>,
    pub generator_version: Option<String>,
    pub provenance: Option<Provenance>,
    pub language: Language,
    pub contract_name: String,
    pub profile_id: String,
    pub compiler: Toolchain,
    pub compiler_settings: serde_json::Value,
    pub metadata_mode: MetadataMode,
    pub source_path: PathBuf,
    pub source_hash: String,
    pub error: String,
    #[serde(default)]
    pub cache: CacheInfo,
}

#[derive(Debug, Clone)]
pub struct CompileSet {
    pub profiles: Vec<CompilerProfile>,
    pub artifacts: Vec<CompiledArtifact>,
    pub failures: Vec<CompileFailure>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GasRecord {
    pub benchmark_id: String,
    pub implementation_id: String,
    pub profile_id: String,
    pub scenario: String,
    pub state_access_profile: StateAccessProfile,
    pub metadata_mode: MetadataMode,
    pub internal_create_gas: u64,
    pub harness_call_gas: u64,
    pub intrinsic_gas: u64,
    pub calldata_gas: u64,
    pub harness_estimated_tx_gas: u64,
    pub expected_success: bool,
    pub call_succeeded: bool,
    pub scenario_status_ok: bool,
    #[serde(default)]
    pub cache: CacheInfo,
}

fn default_call_value() -> String {
    "0".to_string()
}

fn default_call_destination() -> CallDestination {
    CallDestination::Target
}

fn is_default_call_destination(destination: &CallDestination) -> bool {
    *destination == CallDestination::Target
}

fn default_deployment_variant() -> DeploymentVariant {
    DeploymentVariant::Standard
}

fn is_default_deployment_variant(variant: &DeploymentVariant) -> bool {
    *variant == DeploymentVariant::Standard
}

fn default_state_access_profile() -> StateAccessProfile {
    StateAccessProfile::Cold
}

fn default_metadata_mode() -> MetadataMode {
    MetadataMode::Off
}

fn default_random_iterations() -> usize {
    16
}

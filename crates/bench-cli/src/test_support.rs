use crate::models::{CompiledArtifact, GasRecord};
use serde_json::json;

pub fn artifact(compiler: &str, profile: &str) -> CompiledArtifact {
    serde_json::from_value(json!({
        "benchmark_id": "erc20_minimal", "implementation_id": "solidity/handwritten/v1",
        "suite": "fixed", "language": "solidity", "contract_name": "Erc20Minimal", "profile_id": profile,
        "compiler": {"name": compiler, "version": if compiler == "solx" {"0.1.8"} else {"0.8.34"},
            "binary_path": "/test/compiler", "binary_sha256": "digest", "download_source": "test", "version_output": "test",
            "metadata": if compiler == "solx" {json!({"frontend_version":"0.8.34"})} else {json!({})}},
        "compiler_settings": {"evmVersion":"prague", "viaIR":compiler == "solc", "optimizerRuns":200},
        "metadata_mode":"off", "source_path":"/test/Source.sol", "source_hash":"identical-source",
        "abi":[], "creation_bytecode":"6000", "runtime_bytecode":"6000",
        "compile":{"wall_ms_samples":[1.0], "cpu_ms_samples":[1.0], "peak_rss_kib":1},
        "bytecode":{"creation_bytes":2, "creation_bytes_stripped":2, "runtime_bytes":2, "runtime_bytes_stripped":2,
            "initcode_bytes":2, "linked_runtime_bytes":2, "eip170_margin_bytes":24574,
            "eip3860_margin_bytes":49150, "code_deposit_gas":400}
    })).unwrap()
}

pub fn gas(artifact: &CompiledArtifact) -> GasRecord {
    serde_json::from_value(json!({
        "benchmark_id":artifact.benchmark_id, "implementation_id":artifact.implementation_id,
        "profile_id":artifact.profile_id, "scenario":"total_supply", "state_access_profile":"cold", "metadata_mode":"off",
        "internal_create_gas":1, "harness_call_gas":1, "intrinsic_gas":21000, "calldata_gas":1,
        "harness_estimated_tx_gas":21002, "expected_success":true, "call_succeeded":true, "scenario_status_ok":true,
        "return_hash":"return", "observer_hash":"state"
    })).unwrap()
}

use crate::models::DeploymentVariant;

pub fn supports_deployment_variant(benchmark_id: &str, variant: DeploymentVariant) -> bool {
    variant == DeploymentVariant::Standard
        || matches!(benchmark_id, "curve_stableswap_2coin" | "uniswap_v2_pair")
}

pub fn has_deployment_variants(benchmark_id: &str) -> bool {
    matches!(benchmark_id, "curve_stableswap_2coin" | "uniswap_v2_pair")
}

pub fn constructor_args(benchmark_id: &str) -> Option<&'static str> {
    match benchmark_id {
        "counter" => Some("abi.encode(uint256(3))"),
        "erc20_minimal" => Some("abi.encode(uint256(1000 ether))"),
        "uniswap_v2_factory" => {
            Some("abi.encode(type(BenchUniswapFactoryPair).creationCode, address(this))")
        }
        _ => None,
    }
}

pub fn supports_log_diff(benchmark_id: &str) -> bool {
    matches!(
        benchmark_id,
        "curve_stableswap_2coin" | "uniswap_v2_pair" | "yearn_vault_v2" | "yearn_vault_v3"
    )
}

pub fn randomized_helper_name(benchmark_id: &str) -> Option<&'static str> {
    match benchmark_id {
        "counter" => Some("_randomDiff_counter"),
        "erc20_minimal" => Some("_randomDiff_erc20_minimal"),
        "vault_deposit_withdraw" => Some("_randomDiff_vault_deposit_withdraw"),
        "ownable_pausable" => Some("_randomDiff_ownable_pausable"),
        "amm_pair_subset" => Some("_randomDiff_amm_pair_subset"),
        _ => None,
    }
}

pub fn supports_randomized(benchmark_id: &str) -> bool {
    randomized_helper_name(benchmark_id).is_some()
}

pub fn property_helper_name(property_name: &str) -> Option<&'static str> {
    match property_name {
        "counter_model_matches" => Some("_property_counter"),
        "erc20_supply_conservation" => Some("_property_erc20_minimal"),
        "vault_share_accounting" => Some("_property_vault_deposit_withdraw"),
        "ownable_authorization" => Some("_property_ownable_pausable"),
        "amm_reserve_liquidity_coherence" => Some("_property_amm_pair_subset"),
        _ => None,
    }
}

pub fn property_benchmark_id(property_name: &str) -> Option<&'static str> {
    match property_name {
        "counter_model_matches" => Some("counter"),
        "erc20_supply_conservation" => Some("erc20_minimal"),
        "vault_share_accounting" => Some("vault_deposit_withdraw"),
        "ownable_authorization" => Some("ownable_pausable"),
        "amm_reserve_liquidity_coherence" => Some("amm_pair_subset"),
        _ => None,
    }
}

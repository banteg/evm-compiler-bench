use crate::models::{Benchmark, Provenance};
use serde::Deserialize;

pub fn fixed_benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark::fixed(
            "counter",
            "Counter",
            "benches/implementations/counter/solidity/Counter.sol",
            "benches/implementations/counter/vyper/Counter.vy",
        ),
        Benchmark::fixed(
            "erc20_minimal",
            "Erc20Minimal",
            "benches/implementations/erc20_minimal/solidity/Erc20Minimal.sol",
            "benches/implementations/erc20_minimal/vyper/Erc20Minimal.vy",
        ),
        Benchmark::fixed(
            "erc20_permit_hashing",
            "Erc20PermitHashing",
            "benches/implementations/erc20_permit_hashing/solidity/Erc20PermitHashing.sol",
            "benches/implementations/erc20_permit_hashing/vyper/Erc20PermitHashing.vy",
        ),
        Benchmark::fixed(
            "ownable_pausable",
            "OwnablePausable",
            "benches/implementations/ownable_pausable/solidity/OwnablePausable.sol",
            "benches/implementations/ownable_pausable/vyper/OwnablePausable.vy",
        ),
        Benchmark::fixed(
            "vault_deposit_withdraw",
            "VaultDepositWithdraw",
            "benches/implementations/vault_deposit_withdraw/solidity/VaultDepositWithdraw.sol",
            "benches/implementations/vault_deposit_withdraw/vyper/VaultDepositWithdraw.vy",
        ),
        Benchmark::fixed(
            "create2_address_hashing",
            "Create2AddressHashing",
            "benches/implementations/create2_address_hashing/solidity/Create2AddressHashing.sol",
            "benches/implementations/create2_address_hashing/vyper/Create2AddressHashing.vy",
        ),
        Benchmark::fixed(
            "eip1167_codehash_bookkeeping",
            "Eip1167CodehashBookkeeping",
            "benches/implementations/eip1167_codehash_bookkeeping/solidity/Eip1167CodehashBookkeeping.sol",
            "benches/implementations/eip1167_codehash_bookkeeping/vyper/Eip1167CodehashBookkeeping.vy",
        ),
        Benchmark::fixed(
            "merkle_verifier",
            "MerkleVerifier",
            "benches/implementations/merkle_verifier/solidity/MerkleVerifier.sol",
            "benches/implementations/merkle_verifier/vyper/MerkleVerifier.vy",
        ),
        Benchmark::fixed(
            "amm_pair_subset",
            "AmmPairSubset",
            "benches/implementations/amm_pair_subset/solidity/AmmPairSubset.sol",
            "benches/implementations/amm_pair_subset/vyper/AmmPairSubset.vy",
        ),
        Benchmark::fixed(
            "scaling_dispatch_N",
            "ScalingDispatchN",
            "benches/implementations/scaling_dispatch_N/solidity/ScalingDispatchN.sol",
            "benches/implementations/scaling_dispatch_N/vyper/ScalingDispatchN.vy",
        ),
    ]
}

pub fn real_derived_benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark::real_derived(
            "uniswap_v2_pair",
            "UniswapV2Pair",
            "benches/implementations/uniswap_v2_pair/solidity/latest/UniswapV2PairReal.sol",
            "benches/implementations/uniswap_v2_pair/vyper/UniswapV2PairReal.vy",
            provenance_from_spec(include_str!("../../../benches/specs/uniswap_v2_pair.yaml")),
        ),
        Benchmark::real_derived(
            "curve_stableswap_2coin",
            "CurveStableSwap2CoinReal",
            "benches/implementations/curve_stableswap_2coin/solidity/CurveStableSwap2CoinReal.sol",
            "benches/implementations/curve_stableswap_2coin/vyper/upstream/contracts/main/CurveStableSwapNG.vy",
            provenance_from_spec(include_str!(
                "../../../benches/specs/curve_stableswap_2coin.yaml"
            )),
        ),
        Benchmark::real_derived(
            "yearn_vault_v3",
            "YearnVaultV3Real",
            "benches/implementations/yearn_vault_v3/solidity/YearnVaultV3Real.sol",
            "benches/implementations/yearn_vault_v3/vyper/upstream/contracts/VaultV3.vy",
            provenance_from_spec(include_str!("../../../benches/specs/yearn_vault_v3.yaml")),
        ),
    ]
}

pub fn checked_in_benchmarks() -> Vec<Benchmark> {
    fixed_benchmarks()
        .into_iter()
        .chain(real_derived_benchmarks())
        .collect()
}

pub fn all_benchmarks(generated: Vec<Benchmark>, only_benchmark: Option<&str>) -> Vec<Benchmark> {
    checked_in_benchmarks()
        .into_iter()
        .chain(generated)
        .filter(|benchmark| only_benchmark.is_none_or(|id| id == benchmark.id))
        .collect()
}

#[derive(Debug, Deserialize)]
struct SpecWithProvenance {
    real_derived: RealDerivedProvenance,
}

#[derive(Debug, Deserialize)]
struct RealDerivedProvenance {
    #[allow(dead_code)]
    suite: String,
    #[serde(flatten)]
    provenance: Provenance,
}

fn provenance_from_spec(text: &str) -> Provenance {
    let spec: SpecWithProvenance =
        serde_yaml::from_str(text).expect("checked-in real-derived spec provenance must parse");
    spec.provenance()
}

impl SpecWithProvenance {
    fn provenance(self) -> Provenance {
        self.real_derived.provenance
    }
}

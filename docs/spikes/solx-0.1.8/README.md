# solx 0.1.8 gas and size spike

2026-09-09. Recommendation: add solx as a distinct Solidity compiler, initially
with O3 and Oz profiles. This sample shows substantial size reductions and
useful, workload-dependent gas improvements. It does not establish a universally
best compiler or optimizer configuration.

## Comparison

Identical Solidity source bundles for five existing benchmarks, with only their
version pragmas rewritten to `=0.8.34`. All compilers target Cancun and disable
CBOR metadata. The Uniswap case is this repository's modernized, real-derived
pair implementation, not the original historical deployment artifact.

- Upstream solc 0.8.34, commit `80d5c536`: legacy and via-IR, optimizer runs 200.
- solx 0.1.8: O3 and Oz, legacy frontend path, one worker, size fallback disabled.
  Its embedded Solidity frontend is 0.8.34, commit `91fef221`, and LLVM build is
  `7d0702e169889fe4f1a2241c57bef7d2c1c68737`. It is a modified frontend, so this is
  a compiler-product comparison rather than a perfectly isolated backend test.
- Both downloaded macOS binaries were verified against upstream SHA-256 values.
  Exact binaries, versions and URLs are in [toolchains.json](toolchains.json).

## Results

Runtime bytecode bytes, with metadata disabled:

| Benchmark | solc legacy | solc via-IR | solx O3 | solx Oz |
|---|---:|---:|---:|---:|
| ERC-20 | 1,474 | 1,277 | 1,005 | 991 |
| Vault | 835 | 732 | 594 | 588 |
| AMM subset | 1,364 | 1,174 | 958 | 1,025 |
| Merkle verifier | 571 | 438 | 350 | 329 |
| Uniswap V2 pair | 8,243 | 7,631 | 6,938 | 6,334 |

Deltas versus solc via-IR / runs 200; negative means smaller or cheaper.
Gas is the median of per-scenario percentage changes among expected-success
calls within each benchmark. Scenarios returning `false` without reverting are
included. Expected-revert calls are retained in the CSV but excluded here.

| Benchmark | O3 size | Oz size | O3 gas | Oz gas | Successful scenarios |
|---|---:|---:|---:|---:|---:|
| ERC-20 | -21.3% | -22.4% | -1.24% | -0.34% | 4 |
| Vault | -18.9% | -19.7% | -1.04% | -1.11% | 2 |
| AMM subset | -18.4% | -12.7% | -4.27% | -0.25% | 4 |
| Merkle verifier | -20.1% | -24.9% | -7.22% | -4.14% | 6 |
| Uniswap V2 pair | -9.1% | -17.0% | -1.67% | -1.50% | 22 |

Selected internal-call gas observations:

| Call | solc via-IR | solx O3 | solx Oz |
|---|---:|---:|---:|
| ERC-20 transfer | 23,433 | 23,059 | 23,117 |
| AMM subset swap | 3,526 | 3,120 | 3,153 |
| AMM subset burn | 3,476 | 3,190 | 3,749 |
| Merkle proof, 16 elements | 5,852 | 5,413 | 5,585 |
| Uniswap token0-input swap | 100,095 | 98,433 | 98,883 |
| Uniswap fee-on mint | 65,003 | 59,706 | 60,818 |

Oz is not always smaller than O3: the AMM subset grows by 67 bytes and its burn
cost exceeds the solc via-IR baseline by 7.85%. Other small regressions include
O3 ERC-20 approve (+0.15%) and empty Merkle verification (+1.69%). The Uniswap
factory getter has a large relative saving on a small absolute cost; it should
not be used to characterize swap savings.

## Validation and limits

All 20 compilations succeeded. Each of two Foundry runs passed 211 generated
tests: 164 gas tests, 41 scenario differential tests, three randomized
differential tests and three property tests. The first run paired solc via-IR
with O3; the second paired it with Oz. The property/randomized checks cover
ERC-20, vault and AMM subset. Existing Uniswap scenario checks also compare logs.
These are the repository's existing behavioral checks, not an exhaustive
compiler-correctness proof.

All 164 gas records were exactly identical across the two runs, including their
scenario status. The main runner and Foundry templates were reused unchanged;
the temporary Rust bridge only replaces baseline selection to compare two
Solidity compiler profiles. The harness itself was compiled using solc 0.8.35.

Gas uses the repository's internal-call measurement and includes harness call
overhead. These are not end-user transaction gas numbers. Small costs and
storage-heavy operations have different sensitivity to code generation. There
is no traffic weighting, full optimizer-runs sweep, scale-family survey, or
compilation-speed conclusion in this spike. In particular, Oz has not been
compared with every solc size-oriented optimizer configuration.

## Evidence and reproduction

- [gas.csv](gas.csv): all 41 scenarios across four profiles, including failures
  expected by the scenario specification.
- [size.csv](size.csv): runtime and creation bytecode sizes for all 20 artifacts.
- [summary.json](summary.json): unrounded per-benchmark summary.
- [provenance.json](provenance.json): repository revision, source and harness
  hashes, Foundry version, and repeated-run equality check.
- [Spike script](../../../scripts/solx_spike.py): compilation and bridge setup.

Place the two pinned binaries from `toolchains.json` in
`target/solx-spike/bin/{solc,solx}` and make them executable. The script checks
their pinned hashes. Run `python3 scripts/solx_spike.py` from the repository root.
It requires the normal repository dependencies, cached solc 0.8.35 for the
harness, and the listed macOS compiler builds. Cargo must be able to unpack its
cached dependencies, and Foundry needs access to macOS system configuration.

Full standard-JSON inputs/outputs, materialized sources, compiled artifacts,
generated tests and raw gas records remain under `target/solx-spike/`. Normal
benchmark profiles, runner code, and published results were not changed.

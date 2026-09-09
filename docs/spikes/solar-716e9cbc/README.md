# Solar feasibility spike

2026-09-09. **Recommendation: add Solar as an experimental, revision-pinned
compiler with gas and size profiles.** Both modes compile and pass the existing
execution checks for all 64 benchmarks. There is no corpus-level compilation or
runtime blocker to integration at the tested revision.

This tests current main at
[`716e9cbcde88165f931173f1c1fda852ed63afa0`](https://github.com/paradigmxyz/solar/tree/716e9cbcde88165f931173f1c1fda852ed63afa0),
not the July 7 `v0.2.0` release. Both identify their package version as 0.2.0;
the commit and binary hash are essential parts of the measurement identity.

## Configuration and source parity

- Solar: clean upstream checkout, Rust 1.96.0, release build, one compiler
  thread. The native version output records the full commit; its solc-compatible
  version output identifies Solidity 0.8.36 compatibility.
- Reference: upstream solc 0.8.36, commit `8a079791`, legacy and viaIR with
  optimizer runs=200. The official universal macOS binary is SHA-256 verified.
- Both compilers receive identical source bundles from the existing 64-case
  corpus, targeting Prague with CBOR metadata disabled. The spike changes no
  contract source. Every recompiled solc viaIR artifact reproduces the existing
  baseline creation and runtime bytecode exactly.
- Solar's Standard JSON optimizer mapping selects gas mode for enabled
  `runs >= 200`, size mode for enabled `runs < 200`, and no optimization when
  disabled. This spike uses runs=200 and runs=1 respectively. The mapping and
  lifetime-aware runs value are in the
  [pinned implementation](https://github.com/paradigmxyz/solar/blob/716e9cbcde88165f931173f1c1fda852ed63afa0/crates/cli/src/standard_json/compile.rs#L270-L277).
  Standard JSON overrides the CLI optimization mode; a CLI `-O` flag alone is
  insufficient to describe a JSON-mode profile. No `viaIR` setting is sent to
  Solar, which has its own pipeline.
- All 128 Solar ABI objects match their solc counterparts after ignoring only
  top-level ABI entry order. Names, types, mutability, event fields, and nested
  components are retained in that comparison.

The real-derived cases are this repository's fixture-scoped Uniswap, Curve,
and Yearn implementations. These are not full upstream project builds or a
claim of production equivalence.

## Results

Relative to solc 0.8.36 viaIR/runs=200:

| Solar profile | Compile | Runtime gas | Runtime bytecode | Scenario wins / ties / losses |
| --- | ---: | ---: | ---: | ---: |
| Gas, runs=200 | 64/64 | -8.5% | -25.9% | 210 / 0 / 37 |
| Size, runs=1 | 64/64 | -3.0% | -26.6% | 169 / 0 / 78 |

Gas is the geometric mean of 247 matched scenario ratios. Runtime bytecode is
the geometric mean of 64 artifact ratios, counting each artifact once. The gas
population contains 242 expected-success and five expected-revert scenarios.
All observed outcomes match those expectations. Gas includes the existing
internal-call harness overhead; it is not transaction gas or traffic weighted.
The size profile is compared against the same runs=200 reference, not a complete
solc size-optimization sweep.

Selected per-benchmark results, using the geometric mean of that benchmark's
scenario gas ratios:

| Benchmark | Gas-mode gas | Gas-mode size | Size-mode gas | Size-mode size |
| --- | ---: | ---: | ---: | ---: |
| ERC-20 | -3.2% | -19.5% | -2.3% | -22.1% |
| AMM subset | -3.5% | -9.2% | -2.0% | -13.5% |
| Merkle verifier | -26.6% | -47.9% | -24.1% | -45.2% |
| Uniswap V2 pair | -2.2% | -4.1% | +0.5% | -4.5% |
| Curve two-coin fixture | -6.1% | +6.8% | -0.1% | +11.1% |
| Yearn V2 fixture | -8.0% | -26.5% | +0.2% | -24.6% |
| Yearn V3 fixture | -10.6% | -22.0% | +2.1% | -20.5% |

Useful optimization targets remain visible:

- The 64-iteration loop costs 7,278 gas in gas mode versus 5,807 for solc viaIR
  (+25.3%). The 32- and 16-iteration variants regress by 21.9% and 16.6%.
- Yearn V3 `profit_unlock_after_time` costs 3,802 gas in gas mode and 4,889 in
  size mode versus 3,309 for solc (+14.9% and +47.7%).
- Curve grows from 19,544 runtime bytes to 20,876 in gas mode and 21,713 in
  size mode. Size mode also produces more bytecode than gas mode for the
  Merkle and both Yearn fixtures. The mode name is an optimization objective,
  not a guarantee of a smaller artifact.
- A 16-element Merkle proof improves from 5,828 to 3,752 gas in gas mode
  (-35.6%), while counter addition rises from 870 to 923 (+6.1%).

These are performance differences between passing artifacts, not correctness
failures. The CSV files retain individual scenarios and all four profiles.

## Execution evidence

All 64 independent Foundry projects pass: **1,499 generated tests**, comprising
985 gas tests, 494 scenario differential tests, ten randomized differential
tests, and ten property tests. The two Solar modes account for all 128 tested
Solar/solc pairs and 494 Solar scenario executions. The existing harness checks
state observers, return data, and logs where the scenario supports them.
Randomized/property coverage applies to five supported benchmark families;
it is not credited to every benchmark simply because a test exists elsewhere.

There are no runtime exclusions. All 253 successful artifacts are measured;
solc legacy's three existing stack-too-deep failures at ABI arities 16/32/64 are
retained separately. Solc viaIR and both Solar modes compile all 64.

This is execution evidence for the recorded scenarios and property seeds, not
an exhaustive compiler-correctness proof. Compiler outputs, generated tests,
run logs, source bundles, and per-case raw gas records remain under
`target/solar-spike/`.

## Compilation timing

Median wall time in milliseconds over five fresh compiler processes per cell:

| Benchmark | solc legacy | solc viaIR | Solar gas | Solar size |
| --- | ---: | ---: | ---: | ---: |
| Counter | 52.8 | 106.9 | 32.3 | 31.9 |
| ERC-20 | 105.1 | 242.7 | 40.6 | 40.0 |
| Merkle | 69.8 | 121.8 | 34.7 | 37.0 |
| Uniswap V2 pair | 501.9 | 1,528.5 | 75.1 | 78.0 |
| Yearn V3 fixture | 1,176.7 | 4,877.2 | 143.9 | 156.9 |

Both compilers are explicitly launched through `arch -x86_64` on the same
Apple Silicon macOS host. Solar was built by the x86_64 Rust toolchain; solc is
universal. Thus this table compares the same execution architecture, including
Rosetta and process/JSON overhead. It is not a native ARM or whole-project
parallel compilation benchmark. No harness build runs concurrently with these
samples. Single-sample coverage timings are diagnostic only; use
[compile-times.csv](compile-times.csv) for the controlled comparison.

All 100 repeated compilations reproduce their initial creation and runtime
bytecode exactly, with deterministic outputs across all five repetitions.

## Integration work

1. Add a resolver with a full source revision/binary hash and separate package
   version and Solidity compatibility version. A pinned current-main build is
   required to represent these results faithfully.
2. Add gas/runs=200 and size/runs=1 profiles with explicit Prague, metadata-off,
   and one-thread settings. Build source variants using Solidity compatibility
   0.8.36 rather than Solar's package version 0.2.0.
3. Extend compiler-pair selection to choose the same-source, same-EVM solc
   0.8.36 reference, preserving the existing behavior evidence and invalid-result
   exclusions. Use a pinned solc profile so a future `latest` release does not
   silently change the reference.
4. Add Solar to compiler facets, configuration copy, and scale curves. Keep gas
   and size modes distinct and retain regressions and compile failures.

No new benchmark ABI or execution harness is needed. The isolated Rust bridge
reuses the existing runner and templates; its only behavioral policy change is
selecting Solar/solc compiler pairs.

## Reproduction and evidence

[solar_spike.py](../../../scripts/solar_spike.py) requires the completed local
v3 baseline with all 64 solc 0.8.36 viaIR artifacts and their materialized
sources/cache. It verifies source hashes and the pinned solc binary, then
recompiles every artifact in isolation.

```sh
git clone --depth 1 https://github.com/paradigmxyz/solar.git target/solar-spike/upstream
git -C target/solar-spike/upstream fetch --depth 1 origin 716e9cbcde88165f931173f1c1fda852ed63afa0
git -C target/solar-spike/upstream checkout --detach FETCH_HEAD
rustup toolchain install 1.96.0-x86_64-apple-darwin --profile minimal
cd target/solar-spike/upstream
cargo +1.96.0-x86_64-apple-darwin build --release --locked -p solar-compiler --bin solar
cd ../../..
uv run scripts/solar_spike.py prepare
uv run scripts/solar_spike.py bridge
uv run scripts/solar_spike.py compile
uv run scripts/solar_spike.py measure
uv run scripts/solar_spike.py compile --repeats 5 --benchmark counter --benchmark erc20_minimal --benchmark merkle_verifier --benchmark uniswap_v2_pair --benchmark yearn_vault_v3
uv run scripts/solar_spike.py summarize
```

- [summary.json](summary.json): full-precision ratios, coverage, checks, and
  per-benchmark results.
- [gas.csv](gas.csv), [size.csv](size.csv), and
  [compile-times.csv](compile-times.csv): individual measurements.
- [coverage.json](coverage.json), [compile-failures.json](compile-failures.json),
  and [runtime-status.json](runtime-status.json): complete pass/fail accounting.
- [toolchains.json](toolchains.json), [environment.json](environment.json), and
  [input-provenance.json](input-provenance.json): exact compiler identities,
  source/scenario/harness hashes, and execution environment.

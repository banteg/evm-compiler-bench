# EVM Compiler Bench

Head-to-head benchmark harness for EVM compiler profiles. The project compares
Solidity and Vyper implementations under pinned compiler versions, optimizer
settings, codegen backends, and EVM targets.

The report is meant to show compiler tradeoffs, not crown a language winner.
Runtime gas, stripped bytecode size, deploy gas, compile time, and compile
failures are all first-class outputs.

Published report: https://evm.banteg.xyz/

## What is measured

- Fixed matched contracts: hand-written Solidity and Vyper ports of common
  contract motifs.
- Generated scale studies: deterministic N=1..64 families for compiler stress
  surfaces such as dispatch, ABI arguments, events, loops, storage slots, and
  external calls.
- Real-derived models: clean-room benchmark models inspired by Uniswap V2 Pair,
  Curve 2-coin stableswap, and Yearn V3 vault shapes.
- Compiler version axes: historical solc and Vyper profiles, current latest
  profiles, Vyper 0.5.0a1, and Vyper Venom via `--experimental-codegen`.

Gas is measured through the Foundry internal-call harness. It is useful for
isolating generated runtime code costs, but it is not end-user transaction gas.

## Repository layout

- `benches/specs/`: benchmark intent and equivalence scope.
- `benches/scenarios/`: setup and measured calls.
- `benches/implementations/`: Solidity and Vyper source implementations.
- `benches/families/`: generated scale-family definitions.
- `compiler-profiles/`: compiler version, optimizer, EVM, and source-variant
  matrix.
- `crates/bench-cli/`: Rust benchmark runner.
- `foundry/`: generated Foundry gas harness.
- `report-ui/`: interactive static report frontend.
- `results/`: local benchmark outputs, intentionally not checked in.
- `worker/`: Cloudflare Worker serving the static report and R2 result blobs.

## Requirements

- Rust and Cargo.
- Foundry, including `forge`.
- Node.js and npm for the report UI.
- `uv` for Vyper toolchain resolution.
- Wrangler only for publishing or deploying the Cloudflare Worker.

The runner downloads missing solc and Vyper compilers unless `--offline` is
used. Resolved compilers and run outputs are cached locally.

## Running locally

Resolve toolchains:

```sh
cargo run --release -- toolchains
```

Run the full pipeline:

```sh
cargo run --release -- run
cargo run --release -- validate
```

Run one benchmark while iterating:

```sh
cargo run --release -- run --benchmark counter
```

Ignore result caches for a fresh run:

```sh
cargo run --release -- run --no-cache
```

The full current matrix is large: 44 compiler profiles across 62 benchmarks,
which means 2,728 compile attempts before gas scenarios are measured.

## Report UI

Start the interactive report locally:

```sh
npm --prefix report-ui ci
npm --prefix report-ui run dev
```

The dev server loads `results/normalized/report-model.json` by default. After a
benchmark run, the most useful local files are:

- `results/normalized/report-model.json`
- `results/normalized/results.json`
- `results/normalized/run-manifest.json`
- `results/raw/foundry-gas.jsonl`

Build the static report:

```sh
just build-report-ui
```

## Publishing

Benchmark runs are produced locally. Cloudflare builds and deploys only the
static Worker site from `master`; it does not run the benchmark suite.

After a local benchmark run:

```sh
cargo run --release -- run
cargo run --release -- validate
just publish-dev-results
```

`just publish-dev-results` uploads the current `results/` artifacts to the
`evm-compilers` R2 bucket and updates only the dev latest-run pointer. Use
`just publish-prod-results` from a clean `master` worktree when the public report
is ready. See `docs/publishing.md` for the Cloudflare setup.

## Shareable archives

Create a source plus reports archive:

```sh
just zip
```

Create a smaller frontend plus sample-data archive for design tools:

```sh
just zip-design
```

## Scope notes

- Stripped runtime bytecode is used for bytecode comparisons so appended
  compiler metadata does not dominate code-size deltas.
- Missing compile rows are excluded from pairwise ratios; they are still shown
  as compile failures.
- Real-derived contracts are benchmark models with
  `production_equivalence=false`; they are not production gas claims for the
  upstream protocols.
- Vyper Venom rows use `--experimental-codegen`.
- Vyper 0.5.0a1 is pre-release.

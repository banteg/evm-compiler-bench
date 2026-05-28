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
- Real-derived contracts: upstream source-language originals where available,
  plus counterpart-language ports with provenance and explicit benchmark scope
  recorded in specs.
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

Run one benchmark on only a small unoptimized profile pair while iterating on
parity:

```sh
cargo run --release -- run --benchmark yearn_vault_v3 --profile solc-latest-noopt --profile vyper-0.3.7-none --no-cache
```

Ignore result caches for a fresh run:

```sh
cargo run --release -- run --no-cache
```

The full current matrix is large: 48 compiler profiles across 62 benchmarks,
which means 2,976 compile attempts before gas scenarios are measured.

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

The report model carries the methodology notes and real-derived source policy
used by the UI, including the rule that compiled source variants come from
`target/bench-source-variants/<profile_id>/...` while upstream files remain
provenance references. Its public shape is documented in
`schemas/report_model.schema.json`.

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

- Headline comparisons use idiomatic high-level source for each language, not
  hand-written assembly or mechanically de-optimized ports. Language-native
  advantages such as Solidity storage packing and Vyper dispatch codegen are
  part of the comparison.
- Generated scale families are also high-level source stress tests. If a
  compiler profile cannot lower a generated high-level shape, such as a
  many-argument Solidity ABI function under legacy non-via-IR codegen, the
  missing row remains a compile failure rather than being replaced with
  assembly or calldata parsing that changes what the family measures.
- Stripped runtime bytecode is used for bytecode comparisons so appended
  compiler metadata does not dominate code-size deltas.
- Missing compile rows are excluded from pairwise ratios; they are still shown
  as compile failures.
- Assembly-heavy or mechanically matched variants should be treated as
  diagnostic comparators, not as the primary report lane.
- Benchmark specs make lanes explicit: `latest_idiomatic` is the headline
  lane, `upstream_exact_historical` is for pinned historical protocol source,
  `latest_syntax_original` is for upstream-derived original source modernized
  to the checked-in latest syntax baseline,
  `production_conformance` is for broad real-contract behavior checks that
  preserve upstream scope without claiming a latest-vs-latest shootout,
  and `diagnostic_layout_matched` is for manual parity tricks. Real-derived
  specs still distinguish `source_lane` from `counterpart_lane`; pinned
  upstream-historical sources are not treated as latest-stable shootout
  sources.
- `cargo run --release -- validate` enforces latest-lane pragmas on checked-in
  non-upstream benchmark sources and scale templates: Solidity uses
  `pragma solidity ^0.8.35;` and Vyper uses
  `# pragma version >=0.4.3,<0.5.0`.
- For `latest_syntax_original` real-derived sources, validation also hashes
  the vendored upstream reference under `upstream/` against `source_blob`. The
  compiled implementation remains the modernized latest-syntax source, not the
  pinned historical file. New result provenance includes
  `source_reference_path` to make that distinction explicit, and normalized
  result rows include `source_path` plus `source_hash` for the materialized
  source variant actually compiled for the row. The run manifest also records
  per-benchmark `source_variants` with profile, variant, path, hash, and
  compile status. These compiled paths must be generated under
  `target/bench-source-variants/<profile_id>/...`; `upstream/` paths are
  provenance-only.
- Normalized real-derived row provenance keeps `comparison_lane` as the
  benchmark-level lane, such as `production_conformance`, and records the
  compiled artifact's side as `implementation_lane`. This keeps latest-syntax
  source rows from being mistaken for the benchmark comparison lane.
- During compilation, profile-specific source variants rewrite version pragmas
  to the resolved compiler patch range and apply backward syntax rewrites where
  the older language version has enough features. Per-profile source coverage is
  recorded by `source_variants`; those profiles compile generated compatibility
  variants of the checked-in latest source rather than the pinned upstream
  historical source.
- Real-derived specs record provenance and equivalence scope per benchmark.
  The corpus targets faithful idiomatic ports under explicit scope boundaries;
  `excluded_features` document what is intentionally outside the benchmark and
  should not be read as a production deploy-size claim for the upstream
  protocol. The current scope inventory lives in
  `docs/real-derived-production-equivalence.md`.
- For idiomatic cross-language ports, equivalence is about
  externally observable contract behavior: ABI shape, success or revert,
  accounting state, events, and external calls. Exact language-level decoder
  timing and revert bytes are tracked as approximations unless the upstream
  contract exposes or depends on them.
- Vyper Venom rows use `--experimental-codegen`.
- Vyper 0.5.0a1 is pre-release.

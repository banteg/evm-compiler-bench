# Publishing

Benchmark runs are produced locally. Cloudflare only builds and deploys the
static Worker site from `master`.

## Cloudflare Workers Build

Connect the repository with the Cloudflare Workers & Pages GitHub App, then use
these build settings:

- Root directory: `.`
- Build command: `npm --prefix report-ui ci && npm --prefix report-ui run build`
- Deploy command: `npx wrangler deploy --env=""`
- Worker name: `evm-compiler-bench`

The Worker serves `report-ui/dist` as static assets and reads benchmark result
objects from the `evm-compilers` R2 bucket.

Production Worker deployments use `BENCH_CHANNEL=prod`. Dev deployments use
`BENCH_CHANNEL=dev`, either through the `dev` Wrangler environment or the
Cloudflare branch environment variable for the `dev` branch.

## Local Result Publish

After a local benchmark run:

```sh
cargo run --release -- run
cargo run --release -- validate
just publish-dev-results
```

`just publish-dev-results` uploads the current files under `results/` as
immutable run artifacts under `evm-compiler-bench/runs/{run_id}/`, and updates
`evm-compiler-bench/channels/dev/latest.json` in R2. Result JSON is stored
uncompressed in R2; Cloudflare handles HTTP compression for browser requests.

Use `just publish-prod-results` only after the report is ready for the public
site. The prod publish recipe refuses to run unless the current branch is
`master` and the worktree is clean, then updates
`evm-compiler-bench/channels/prod/latest.json`.

The Worker falls back to the legacy `evm-compiler-bench/latest.json` only for
the prod channel, so the current public report keeps working until a prod
channel pointer is published.

The publish command uses remote R2, so Wrangler must be authenticated locally:

```sh
wrangler login
```

Alternatively, set `CLOUDFLARE_API_TOKEN` to a token with permission to write
objects to the `evm-compilers` bucket.

Do not commit generated result blobs or built static assets.

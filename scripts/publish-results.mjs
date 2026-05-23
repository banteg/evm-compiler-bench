#!/usr/bin/env node
import { brotliCompressSync, constants as zlibConstants } from "node:zlib";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = new Set(process.argv.slice(2));

const bucket = valueArg("--bucket") || process.env.CF_R2_BUCKET || "evm-compilers";
const prefix = stripSlashes(valueArg("--prefix") || process.env.CF_R2_PREFIX || "evm-compiler-bench");
const channel = normalizeChannel(valueArg("--channel") || process.env.EVM_BENCH_CHANNEL || process.env.CF_R2_CHANNEL || "dev");
const upload = args.has("--upload");
const deploy = args.has("--deploy");
const wrangler = process.env.WRANGLER_BIN || "wrangler";

const paths = {
  reportModel: join(root, "results/normalized/report-model.json"),
  results: join(root, "results/normalized/results.json"),
  runManifest: join(root, "results/normalized/run-manifest.json"),
  foundryGas: join(root, "results/raw/foundry-gas.jsonl"),
};

const runManifest = JSON.parse(readFileSync(paths.runManifest, "utf8"));
const runId = runManifest.run_id;
if (!runId) throw new Error("run-manifest.json is missing run_id");

const publishDir = join(root, "target/publish", runId);
mkdirSync(publishDir, { recursive: true });

const artifacts = {
  report_model: artifact("report-model.json", paths.reportModel, "application/json; charset=utf-8", false),
  results: artifact("results.json", paths.results, "application/json; charset=utf-8", false),
  run_manifest: artifact("run-manifest.json", paths.runManifest, "application/json; charset=utf-8", false),
  foundry_gas: artifact("foundry-gas.jsonl", paths.foundryGas, "application/x-ndjson; charset=utf-8", false),
};

const publishManifest = {
  schema_version: 1,
  project: "evm-compiler-bench",
  bucket,
  prefix,
  channel,
  run_id: runId,
  commit: runManifest.environment?.git?.commit ?? null,
  dirty: runManifest.environment?.git?.dirty ?? null,
  evm_version: runManifest.evm_version ?? null,
  started_at: runManifest.started_at ?? null,
  published_at: new Date().toISOString(),
  artifacts,
};

const publishManifestPath = join(publishDir, "publish-manifest.json");
writeFileSync(publishManifestPath, JSON.stringify(publishManifest, null, 2) + "\n");
publishManifest.artifacts.publish_manifest = artifact(
  "publish-manifest.json",
  publishManifestPath,
  "application/json; charset=utf-8",
  false,
);

const latestPath = join(publishDir, "latest.json");
writeFileSync(latestPath, JSON.stringify(publishManifest, null, 2) + "\n");

console.log(`prepared ${Object.keys(publishManifest.artifacts).length} artifacts for ${channel} run ${runId}`);
console.log(`publish manifest: ${publishManifestPath}`);
for (const [name, item] of Object.entries(publishManifest.artifacts)) {
  console.log(`${name}: ${item.size_bytes} bytes -> ${item.encoded_size_bytes} bytes at r2://${bucket}/${item.key}`);
}

if (upload) {
  for (const [name, item] of Object.entries(publishManifest.artifacts)) {
    uploadObject(name, item, "public, max-age=31536000, immutable");
  }
  uploadObject("latest", {
    key: `${prefix}/channels/${channel}/latest.json`,
    file: latestPath,
    content_type: "application/json; charset=utf-8",
    content_encoding: null,
  }, "public, max-age=60");
}

if (deploy) {
  run(wrangler, channel === "prod" ? ["deploy", "--env="] : ["deploy", "--env", channel]);
}

function artifact(fileName, sourcePath, contentType, compress) {
  const source = readFileSync(sourcePath);
  const key = `${prefix}/runs/${runId}/${fileName}`;
  const outPath = join(publishDir, compress ? `${fileName}.br` : fileName);
  const encoded = compress
    ? brotliCompressSync(source, {
        params: {
          [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
        },
      })
    : source;
  writeFileSync(outPath, encoded);
  return {
    key,
    path: `/runs/${runId}/${fileName}`,
    file: outPath,
    content_type: contentType,
    content_encoding: compress ? "br" : null,
    size_bytes: source.byteLength,
    encoded_size_bytes: encoded.byteLength,
    sha256: sha256(source),
    encoded_sha256: sha256(encoded),
  };
}

function uploadObject(name, item, cacheControl) {
  const cmd = [
    "r2",
    "object",
    "put",
    `${bucket}/${item.key}`,
    "--remote",
    "--file",
    item.file,
    "--content-type",
    item.content_type,
    "--cache-control",
    cacheControl,
  ];
  if (item.content_encoding) {
    cmd.push("--content-encoding", item.content_encoding);
  }
  console.log(`uploading ${name}: ${bucket}/${item.key}`);
  run(wrangler, cmd);
}

function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: root,
    stdio: "inherit",
    env: process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} exited with ${result.status}`);
  }
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function stripSlashes(value) {
  return value.replace(/^\/+|\/+$/g, "");
}

function normalizeChannel(value) {
  const channel = String(value).trim().toLowerCase();
  if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(channel)) {
    throw new Error(`invalid channel ${JSON.stringify(value)}; expected [a-z0-9][a-z0-9_-]{0,31}`);
  }
  return channel;
}

function valueArg(name) {
  const prefix = `${name}=`;
  const value = process.argv.slice(2).find(arg => arg.startsWith(prefix));
  return value ? value.slice(prefix.length) : null;
}

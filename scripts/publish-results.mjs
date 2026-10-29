#!/usr/bin/env node
import { brotliCompressSync, constants as zlibConstants } from "node:zlib";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { assertProductionSnapshot, publishOptions } from "./publish-policy.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const { bucket, prefix, channel, upload, deploy } = publishOptions(process.argv.slice(2), process.env);
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
if (upload && channel === "prod") {
  const git = (...args) => {
    const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
    if (result.status !== 0) throw new Error("Unable to verify checkout for production publishing");
    return result.stdout.trim();
  };
  assertProductionSnapshot({branch: git("branch", "--show-current"), dirty: git("status", "--porcelain") !== "", commit: git("rev-parse", "HEAD")}, runManifest);
}

const publishDir = join(root, "target/publish", runId);
mkdirSync(publishDir, { recursive: true });
// Local paths of prepared objects by R2 key; kept out of the published manifests.
const localFiles = new Map();

const artifacts = {
  // The browser loads the report model on every cold visit; store it brotli-encoded
  // so the worker streams a few MB instead of the edge compressing it per request.
  // Data downloads stay identity-encoded for clients that cannot decode brotli.
  report_model: artifact("report-model.json", paths.reportModel, "application/json; charset=utf-8", true),
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

// A manifest cannot list its own hash, so the run's publish manifest covers the
// data artifacts and the channel pointer additionally covers the publish manifest.
const publishManifestPath = join(publishDir, "publish-manifest.json");
writeFileSync(publishManifestPath, JSON.stringify(publishManifest, null, 2) + "\n");
const latest = {
  ...publishManifest,
  artifacts: {
    ...artifacts,
    publish_manifest: artifact("publish-manifest.json", publishManifestPath, "application/json; charset=utf-8", false),
  },
};

const latestPath = join(publishDir, "latest.json");
writeFileSync(latestPath, JSON.stringify(latest, null, 2) + "\n");

console.log(`prepared ${Object.keys(latest.artifacts).length} artifacts for ${channel} run ${runId}`);
console.log(`publish manifest: ${publishManifestPath}`);
for (const [name, item] of Object.entries(latest.artifacts)) {
  console.log(`${name}: ${item.size_bytes} bytes -> ${item.encoded_size_bytes} bytes at r2://${bucket}/${item.key}`);
}

if (upload) {
  for (const [name, item] of Object.entries(latest.artifacts)) {
    uploadObject(name, item, localFiles.get(item.key), "public, max-age=31536000, immutable");
  }
  uploadObject("latest", {
    key: `${prefix}/channels/${channel}/latest.json`,
    content_type: "application/json; charset=utf-8",
    content_encoding: null,
  }, latestPath, "public, max-age=60");
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
  localFiles.set(key, outPath);
  return {
    key,
    path: `/runs/${runId}/${fileName}`,
    content_type: contentType,
    content_encoding: compress ? "br" : null,
    size_bytes: source.byteLength,
    encoded_size_bytes: encoded.byteLength,
    sha256: sha256(source),
    encoded_sha256: sha256(encoded),
  };
}

function uploadObject(name, item, file, cacheControl) {
  const cmd = [
    "r2",
    "object",
    "put",
    `${bucket}/${item.key}`,
    "--remote",
    "--file",
    file,
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

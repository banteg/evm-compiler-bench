#!/usr/bin/env node
import { brotliCompressSync, constants as zlibConstants } from "node:zlib";
import { randomUUID } from "node:crypto";
import { linkSync, mkdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { loadCompletedBundle, sha256 } from "./publish-bundle.mjs";
import { assertProductionSnapshot, publishOptions } from "./publish-policy.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const { bucket, prefix, channel, upload, deploy } = publishOptions(process.argv.slice(2), process.env);
const wrangler = process.env.WRANGLER_BIN || "wrangler";

const { files, manifest: runManifest } = loadCompletedBundle(root);
const runId = runManifest.run_id;
if (upload && channel === "prod") {
  const git = (...args) => {
    const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
    if (result.status !== 0) throw new Error("Unable to verify checkout for production publishing");
    return result.stdout.trim();
  };
  assertProductionSnapshot({branch: git("branch", "--show-current"), dirty: git("status", "--porcelain", "--untracked-files=all") !== "", commit: git("rev-parse", "HEAD")}, runManifest);
}

const publishDir = join(root, "target/publish", runId);
mkdirSync(publishDir, { recursive: true });
// Local paths of prepared objects by R2 key; kept out of the published manifests.
const localFiles = new Map();

const artifacts = {
  // The browser loads the report model on every cold visit; store it brotli-encoded
  // so the worker streams a few MB instead of the edge compressing it per request.
  // Data downloads stay identity-encoded for clients that cannot decode brotli.
  report_model: artifact("report-model.json", "application/json; charset=utf-8", true),
  results: artifact("results.json", "application/json; charset=utf-8", false),
  run_manifest: artifact("run-manifest.json", "application/json; charset=utf-8", false),
  foundry_gas: artifact("foundry-gas.jsonl", "application/x-ndjson; charset=utf-8", false),
};

const publishManifest = {
  schema_version: 1,
  project: "evm-compiler-bench",
  bucket,
  prefix,
  run_id: runId,
  commit: runManifest.environment?.git?.commit ?? null,
  dirty: runManifest.environment?.git?.dirty ?? null,
  evm_version: runManifest.evm_version ?? null,
  started_at: runManifest.started_at ?? null,
  artifacts,
};

// A manifest cannot list its own hash, so the run's publish manifest covers the
// data artifacts and the channel pointer additionally covers the publish manifest.
const manifestArtifact = artifact("publish-manifest.json", "application/json; charset=utf-8", false, Buffer.from(JSON.stringify(publishManifest, null, 2) + "\n"));
const publishManifestPath = localFiles.get(manifestArtifact.key);
const latest = {
  ...publishManifest,
  channel,
  published_at: new Date().toISOString(),
  artifacts: {...artifacts, publish_manifest: manifestArtifact},
};
const latestBytes = Buffer.from(JSON.stringify(latest, null, 2) + "\n");
const latestPath = preparedFile("latest.json", latestBytes);

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

function preparedFile(fileName, bytes, digest = sha256(bytes)) {
  // A concurrent preparation (including another channel) must never change the
  // bytes passed by path to an in-flight upload.
  const dir = join(publishDir, digest);
  mkdirSync(dir, {recursive: true});
  const path = join(dir, fileName);
  const pending = join(dir, randomUUID());
  writeFileSync(pending, bytes);
  renameSync(pending, path);
  // Retain the documented flat preparation directory for local inspection.
  linkSync(path, pending);
  renameSync(pending, join(publishDir, fileName));
  return path;
}

function artifact(fileName, contentType, compress, source = files[fileName]) {
  const encoded = compress
    ? brotliCompressSync(source, {
        params: {
          [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
        },
      })
    : source;
  const encodedHash = sha256(encoded);
  const path = `/runs/${runId}/${encodedHash}/${fileName}`;
  const key = `${prefix}${path}`;
  const outPath = preparedFile(compress ? `${fileName}.br` : fileName, encoded, encodedHash);
  localFiles.set(key, outPath);
  return {
    key,
    path,
    content_type: contentType,
    content_encoding: compress ? "br" : null,
    size_bytes: source.byteLength,
    encoded_size_bytes: encoded.byteLength,
    sha256: sha256(source),
    encoded_sha256: encodedHash,
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

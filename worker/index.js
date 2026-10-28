const DEFAULT_R2_PREFIX = "evm-compiler-bench";
const DEFAULT_CHANNEL = "prod";
const DEV_PREVIEW_HOST = "dev-evm-compiler-bench.banteeg.workers.dev";
const HSTS = "max-age=31536000; includeSubDomains";
const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

const DATA_ROUTES = new Map([
  ["/latest.json", "latest"],
  ["/publish-manifest.json", "publish_manifest"],
  ["/report-model.json", "report_model"],
  ["/results.json", "results"],
  ["/run-manifest.json", "run_manifest"],
  ["/foundry-gas.jsonl", "foundry_gas"],
]);

function jsonResponse(value, init = {}) {
  const headers = new Headers(init.headers);
  headers.set("content-type", "application/json; charset=utf-8");
  headers.set("cache-control", init.cacheControl || "public, max-age=60");
  return new Response(JSON.stringify(value), { ...init, headers });
}

function httpsRedirect(url) {
  url.protocol = "https:";
  return new Response(null, {
    status: 308,
    headers: {
      "cache-control": "public, max-age=3600",
      location: url.toString(),
      "strict-transport-security": HSTS,
    },
  });
}

function withSecurityHeaders(response) {
  const headers = new Headers(response.headers);
  headers.set("strict-transport-security", HSTS);
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function notFound(message) {
  return jsonResponse({ error: message }, { status: 404, cacheControl: "no-store" });
}

async function latestManifest(env, url) {
  const prefix = r2Prefix(env);
  const channel = resultChannel(env, url);
  const object = await env.RESULTS.get(`${prefix}/channels/${channel}/latest.json`);
  if (object) return object.json();

  if (channel === "prod") {
    const legacy = await env.RESULTS.get(`${prefix}/latest.json`);
    if (legacy) return legacy.json();
  }

  return null;
}

function objectResponse(object, cacheControl, artifact = {}) {
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  if (artifact.content_type) {
    headers.set("content-type", artifact.content_type);
  }
  if (artifact.content_encoding) {
    headers.set("content-encoding", artifact.content_encoding);
  }
  // The encoding comes from the run manifest or from the object's R2 metadata.
  if (headers.has("content-encoding")) {
    headers.append("vary", "Accept-Encoding");
    if (!cacheControl.includes("no-transform")) {
      cacheControl = `${cacheControl}, no-transform`;
    }
  }
  headers.set("etag", object.httpEtag);
  headers.set("cache-control", cacheControl);
  headers.set("x-r2-key", object.key);
  return new Response(object.body, { headers });
}

async function serveArtifact(env, artifactName, url) {
  const latest = await latestManifest(env, url);
  if (!latest) return notFound("No published benchmark run found.");
  if (artifactName === "latest") {
    return jsonResponse(latest, { cacheControl: "public, max-age=60" });
  }

  const artifact = latest.artifacts?.[artifactName];
  if (!artifact?.key) return notFound(`No artifact named ${artifactName}.`);

  const object = await env.RESULTS.get(artifact.key);
  if (!object) return notFound(`Artifact missing from R2: ${artifact.key}`);

  return objectResponse(object, "public, max-age=60, must-revalidate", artifact);
}

async function serveRunObject(env, pathname) {
  const key = `${r2Prefix(env)}${pathname}`;
  const object = await env.RESULTS.get(key);
  if (!object) return notFound(`No R2 object at ${key}.`);
  return objectResponse(object, "public, max-age=31536000, immutable");
}

function r2Prefix(env) {
  return stripSlashes(env.R2_PREFIX || DEFAULT_R2_PREFIX);
}

function resultChannel(env, url) {
  const hostChannel = channelFromHost(url);
  if (hostChannel) return hostChannel;
  return normalizeChannel(env.BENCH_CHANNEL || env.EVM_BENCH_CHANNEL || DEFAULT_CHANNEL);
}

function channelFromHost(url) {
  const host = url?.hostname?.toLowerCase();
  if (host === DEV_PREVIEW_HOST) return "dev";
  return null;
}

function isLoopback(url) {
  return LOOPBACK_HOSTS.has(url.hostname) || url.hostname.endsWith(".localhost");
}

function stripSlashes(value) {
  return String(value).replace(/^\/+|\/+$/g, "");
}

function normalizeChannel(value) {
  const channel = String(value).trim().toLowerCase();
  if (/^[a-z0-9][a-z0-9_-]{0,31}$/.test(channel)) return channel;
  return DEFAULT_CHANNEL;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    // `wrangler dev` serves plain HTTP on loopback.
    if (url.protocol === "http:" && !isLoopback(url)) {
      return httpsRedirect(url);
    }

    const route = DATA_ROUTES.get(url.pathname);
    if (route) return withSecurityHeaders(await serveArtifact(env, route, url));
    if (url.pathname.startsWith("/runs/")) {
      return withSecurityHeaders(await serveRunObject(env, url.pathname));
    }
    return withSecurityHeaders(await env.ASSETS.fetch(request));
  },
};

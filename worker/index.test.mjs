import test from 'node:test';
import assert from 'node:assert/strict';
import worker from './index.js';

function r2Object(key, value, httpMetadata = {}) {
  return {
    key,
    httpEtag: `"${key}"`,
    body: JSON.stringify(value),
    json: async () => value,
    writeHttpMetadata(headers) {
      if (httpMetadata.contentType) headers.set('content-type', httpMetadata.contentType);
      if (httpMetadata.contentEncoding) headers.set('content-encoding', httpMetadata.contentEncoding);
    },
  };
}

function environment(objects) {
  const requested = [];
  return {
    requested,
    BENCH_CHANNEL: 'prod',
    RESULTS: {
      async get(key) {
        requested.push(key);
        return objects[key] ?? null;
      },
    },
    ASSETS: {fetch: async () => new Response('asset')},
  };
}

test('run objects are served from their own metadata without reading the channel pointer', async () => {
  const key = 'evm-compiler-bench/runs/old/results.json';
  const env = environment({[key]: r2Object(key, [], {contentType: 'application/json', contentEncoding: 'br'})});
  const response = await worker.fetch(new Request('https://evm.example/runs/old/results.json'), env);
  assert.equal(response.status, 200);
  assert.deepEqual(env.requested, [key]);
  assert.equal(response.headers.get('content-encoding'), 'br');
  assert.equal(response.headers.get('vary'), 'Accept-Encoding');
  assert.equal(response.headers.get('cache-control'), 'public, max-age=31536000, immutable, no-transform');
});

test('unencoded run objects keep transformable caching', async () => {
  const key = 'evm-compiler-bench/runs/old/run-manifest.json';
  const env = environment({[key]: r2Object(key, {}, {contentType: 'application/json'})});
  const response = await worker.fetch(new Request('https://evm.example/runs/old/run-manifest.json'), env);
  assert.equal(response.headers.get('vary'), null);
  assert.equal(response.headers.get('cache-control'), 'public, max-age=31536000, immutable');
});

test('plain HTTP redirects to HTTPS except on loopback hosts', async () => {
  const env = environment({});
  const remote = await worker.fetch(new Request('http://evm.example/'), env);
  assert.equal(remote.status, 308);
  assert.equal(remote.headers.get('location'), 'https://evm.example/');
  for (const origin of ['http://localhost:8787', 'http://127.0.0.1:8787', 'http://[::1]:8787', 'http://bench.localhost:8787']) {
    const local = await worker.fetch(new Request(`${origin}/`), env);
    assert.equal(local.status, 200, origin);
    assert.equal(await local.text(), 'asset');
  }
});

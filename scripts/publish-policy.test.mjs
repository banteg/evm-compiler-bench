import test from 'node:test';
import assert from 'node:assert/strict';
import { assertProductionSnapshot, publishOptions } from './publish-policy.mjs';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {loadCompletedBundle, sha256} from './publish-bundle.mjs';

const checkout = {branch: 'master', dirty: false, commit: 'current'};
const manifest = {gas_records: 1, environment: {git: {dirty: false, commit: 'current'}, command_line: ['bench', 'run']}};
test('production publishing rejects dirty, stale, and partial measurements', () => {
  assert.doesNotThrow(() => assertProductionSnapshot(checkout, manifest));
  assert.throws(() => assertProductionSnapshot(checkout, {...manifest, gas_records: 0}), /gas records/);
  assert.throws(() => assertProductionSnapshot(checkout, {...manifest, environment: {git: manifest.environment.git}}));
  assert.throws(() => assertProductionSnapshot({...checkout, dirty: true}, manifest));
  assert.throws(() => assertProductionSnapshot({...checkout, branch: 'feat/test'}, manifest));
  assert.throws(() => assertProductionSnapshot({...checkout, commit: 'different'}, manifest));
  assert.throws(() => assertProductionSnapshot(checkout, {environment: {...manifest.environment, git: {dirty: true, commit: 'current'}}}));
  assert.throws(() => assertProductionSnapshot(checkout, {...manifest, environment: {...manifest.environment, command_line: ['bench', 'run', '--profile=solx-0.1.8-O3']}}));
});

test('publish options parse both option forms and reject unknown options', () => {
  assert.equal(publishOptions(['--channel', 'prod'], {}).channel, 'prod');
  assert.equal(publishOptions(['--channel=prod', '--upload'], {}).channel, 'prod');
  assert.equal(publishOptions([], {EVM_BENCH_CHANNEL: 'prod'}).channel, 'prod');
  assert.deepEqual(publishOptions(['--upload', '--prefix', '/bench/'], {}), {
    bucket: 'evm-compilers', prefix: 'bench', channel: 'dev', upload: true, deploy: false,
  });
  assert.throws(() => publishOptions(['--chanel=prod'], {}));
  assert.throws(() => publishOptions(['--channel'], {}));
  assert.throws(() => publishOptions(['--channel', 'Prod/x'], {}));
});

function bundleFixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'bench-publish-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const runId = '20261101T123456000000001Z';
  const dir = join(root, 'target/bench-runs', runId);
  const normalized = join(root, 'results/normalized');
  mkdirSync(dir, {recursive: true});
  mkdirSync(normalized, {recursive: true});
  const manifest = {bundle_version: 1, run_id: runId, gas_records: 1, compile_failures: 0};
  const identity = {benchmark_id: 'counter', implementation_id: 'solidity', profile_id: 'solc'};
  const gas = {scenario: 'increment', state_access_profile: 'cold', harness_call_gas: 42};
  const bytecode = {initcode_bytes: 32, deployed_runtime_bytes: 24};
  const behavior = {return_hash: '0xab', observer_hash: null, log_hash: null};
  const row = {status: 'ok', ...identity, gas, bytecode, behavior, correctness: {golden_behavior_check: 'pass'}};
  const files = {
    'run-manifest.json': JSON.stringify(manifest),
    'report-model.json': JSON.stringify({manifest, lookups: {compiler: [{name: 'solc'}], provenance: [null]}, rows: [{...row, compiler_ref: 0, provenance_ref: 0}]}),
    'results.json': JSON.stringify([{...row, compiler: {name: 'solc'}, provenance: null, cache: {gas: {status: 'miss'}}}]),
    'foundry-gas.jsonl': JSON.stringify({...identity, ...gas, ...bytecode, ...behavior, golden_check_passed: true, cache: {status: 'miss'}}) + '\n',
  };
  function seal() {
    const hashes = {};
    for (const [name, data] of Object.entries(files)) {
      writeFileSync(join(dir, name), data);
      hashes[name] = sha256(data);
    }
    const receipt = JSON.stringify({run_id: runId, sha256: hashes});
    writeFileSync(join(normalized, 'bundle-receipt.json'), receipt);
    writeFileSync(join(dir, 'receipt.json'), receipt);
  }
  seal();
  return {root, dir, files, manifest, seal};
}

test('promotion validates frozen bytes, counts and run identity', t => {
  const {root, dir, files, manifest, seal} = bundleFixture(t);
  writeFileSync(join(root, 'results/normalized/results.json'), '[]');
  assert.equal(loadCompletedBundle(root).manifest.run_id, manifest.run_id);
  writeFileSync(join(dir, 'results.json'), '[]');
  assert.throws(() => loadCompletedBundle(root), /digest mismatch/);
  const goodModel = files['report-model.json'];
  files['report-model.json'] = goodModel.replace('"harness_call_gas":42', '"harness_call_gas":24');
  seal();
  assert.throws(() => loadCompletedBundle(root), /normalized row/);
  files['report-model.json'] = goodModel;
  const goodGas = files['foundry-gas.jsonl'];
  for (const [field, value, error] of [
    ['initcode_bytes', 33, /deployed bytecode/], ['deployed_runtime_bytes', 23, /deployed bytecode/],
    ['return_hash', '0xbc', /behavior evidence/], ['observer_hash', '0xbc', /behavior evidence/],
    ['log_hash', '0xbc', /behavior evidence/], ['golden_check_passed', false, /normalized evidence/],
  ]) {
    files['foundry-gas.jsonl'] = JSON.stringify({...JSON.parse(goodGas), [field]: value}) + '\n';
    seal();
    assert.throws(() => loadCompletedBundle(root), error);
  }
  files['foundry-gas.jsonl'] = goodGas.replace('"harness_call_gas":42', '"harness_call_gas":24');
  seal();
  assert.throws(() => loadCompletedBundle(root), /Raw harness_call_gas/);
  files['report-model.json'] = JSON.stringify({manifest: {...manifest, run_id: 'other'}, rows: [{}]});
  seal();
  assert.throws(() => loadCompletedBundle(root), /Report model/);
  files['foundry-gas.jsonl'] = '';
  seal();
  assert.throws(() => loadCompletedBundle(root), /Raw gas count/);
  files['foundry-gas.jsonl'] = '{}\n';
  files['results.json'] = '[]';
  seal();
  assert.throws(() => loadCompletedBundle(root), /Normalized result counts/);
});

test('dev, prod and retries prepare identical immutable run objects', t => {
  const {root} = bundleFixture(t);
  const scripts = join(root, 'scripts');
  mkdirSync(scripts);
  for (const name of ['publish-policy.mjs', 'publish-bundle.mjs', 'publish-results.mjs']) {
    copyFileSync(new URL(name, import.meta.url), join(scripts, name));
  }
  const prepare = channel => {
    const output = execFileSync(process.execPath, [join(scripts, 'publish-results.mjs'), '--channel', channel], {encoding: 'utf8'});
    return output.match(/^publish manifest: (.+)$/m)[1];
  };
  const path = prepare('dev');
  const bytes = readFileSync(path);
  assert.equal(prepare('prod'), path);
  assert.equal(prepare('dev'), path);
  assert.deepEqual(readFileSync(path), bytes);
  for (const item of Object.values(JSON.parse(bytes).artifacts)) {
    assert.ok(item.path.includes('/' + item.encoded_sha256 + '/'));
  }
});

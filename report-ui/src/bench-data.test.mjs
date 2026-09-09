import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';

const profiles = [
  {id: 'solc-0.8.34-viair-runs200', language: 'solidity', compiler_name: 'solc', compiler_version: '0.8.34', optimizer: 'viaIR', optimizer_runs: 200},
  {id: 'solx-0.1.8-O3', language: 'solidity', compiler_name: 'solx', compiler_version: '0.1.8', frontend_version: '0.8.34', optimizer: 'O3'},
  {id: 'solx-0.1.8-Oz', language: 'solidity', compiler_name: 'solx', compiler_version: '0.1.8', frontend_version: '0.8.34', optimizer: 'Oz'},
  {id: 'vyper-latest-gas', language: 'vyper', compiler_name: 'vyper', compiler_version: '0.4.3', optimizer: 'gas'},
];
const rows = profiles.flatMap((p, i) => ['transfer', 'approve'].map(scenario => ({
  profile_id: p.id, status: 'ok', suite: 'fixed', benchmark_id: 'erc20', language: p.language,
  bytecode: {runtime_bytes_stripped: 1000 - 100*i},
  gas: {scenario, state_access_profile: 'cold', deployment_variant: 'standard', harness_call_gas: 2000 - 100*i},
})));
function load(extraProfiles = [], dataRows = rows) {
  const context = {window: {__BENCH_DATA: {profiles: [...profiles, ...extraProfiles], rows: dataRows}}};
  vm.runInNewContext(readFileSync(new URL('./bench-data.js', import.meta.url), 'utf8'), context);
  return context.window.Bench;
}

test('same-profile comparisons return ties at scenario and artifact granularity', () => {
  const b = load(), id = profiles[0].id;
  const gas = b.compareProfiles(rows, id, id, 'harness_call_gas');
  assert.equal(gas.length, 2);
  assert.ok(gas.every(r => r.ratio === 1 && r.deltaPct === 0));
  const size = b.compareProfiles(rows, id, id, 'runtime_bytes_stripped');
  assert.equal(size.length, 1);
  assert.equal(size[0].ratio, 1);
});

test('Solidity compiler selection keeps solx release, frontend, modes and solc runs separate', () => {
  // A colliding version in another compiler must not enter solx's facets or resolution.
  const b = load([{...profiles[0], id: 'solc-collision', compiler_version: '0.1.8'}]);
  const knobs = b.profileKnobs(profiles[1]);
  assert.equal(knobs.compiler, 'solx');
  assert.equal(knobs.language, 'solidity');
  assert.equal(knobs.versionKey, '0.1.8');
  assert.equal(knobs.runs, null);
  const facets = b.profileFacets('solidity', '0.1.8', 'O3', 'solx');
  assert.equal(facets.versions.join(), '0.1.8');
  assert.equal(facets.optimizers.join(), 'O3,Oz');
  assert.equal(facets.runs.length, 0);
  assert.equal(b.resolveProfile({...knobs, optimizer: 'Oz'}), 'solx-0.1.8-Oz');
  assert.equal(b.defaultProfileForCompiler('solx'), 'solx-0.1.8-O3');
  assert.equal(b.latestBaselineProfile(profiles[1]), undefined);
  assert.match(b.profileLabel('solx-0.1.8-Oz'), /^solx 0\.1\.8 Oz$/);
  assert.equal(b.compilerOptions().map(p => p.value).join(), 'solc,solx,vyper');
});

test('code-size comparisons count each artifact once, independent of scenario count', () => {
  const b = load();
  const cmp = b.compareProfiles(rows, profiles[0].id, profiles[1].id, 'runtime_bytes_stripped');
  assert.equal(cmp.length, 1);
  assert.equal(cmp[0].ratio, 0.9);
});

test('a cheap unexpected revert cannot become a gas or size win, even through another passing scenario', () => {
  const data = structuredClone(rows);
  const broken = data.find(r => r.profile_id === profiles[1].id);
  broken.gas.harness_call_gas = 1;
  broken.correctness = {scenario_status_check: 'fail'};
  const b = load([], data);
  assert.equal(b.correctnessFailureGroups().length, 1);
  for (const metric of ['harness_call_gas', 'runtime_bytes_stripped']) {
    assert.equal(b.compareProfiles(data, profiles[0].id, profiles[1].id, metric).length, 0);
    assert.ok(b.compareProfiles(data, profiles[0].id, profiles[2].id, metric).length > 0);
  }
});

test('Solar revision and gas/size runs remain distinct from the package and Solidity version', () => {
  const revision = '716e9cbcde88165f931173f1c1fda852ed63afa0';
  const gas = {id: 'solar-716e9cbc-gas-runs200', language: 'solidity', compiler_name: 'solar', compiler_version: '0.2.0', source_revision: revision, solidity_version: '0.8.36', optimizer: 'gas', optimizer_runs: 200};
  const size = {...gas, id: 'solar-716e9cbc-size-runs1', optimizer: 'size', optimizer_runs: 1};
  const next = {...gas, id: 'solar-next-gas-runs200', source_revision: 'a'.repeat(40)};
  const b = load([gas, size, next]);
  const knobs = b.profileKnobs(gas);
  assert.equal(knobs.versionKey, revision);
  assert.equal(knobs.runs, 200);
  assert.equal(b.profileFacets('solidity', revision, 'size', 'solar').runs.join(), '1');
  assert.equal(b.resolveProfile({...knobs, optimizer: 'size', runs: 1}), size.id);
  assert.match(b.profileLabel(gas.id), /^Solar 0\.2\.0 @716e9cbc gas runs200$/);
  assert.equal(b.latestBaselineProfile(gas), undefined);
});

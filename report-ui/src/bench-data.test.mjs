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

test('Vyper prerelease profiles retain exact beta identity and order alpha, beta, RC, stable', () => {
  const beta = {...profiles[3], id: 'vyper-prerelease-gas', compiler_version: '0.5.0b1'};
  const b = load([beta]);
  assert.equal(b.profileVersionKey(beta), '0.5.0b1');
  assert.match(b.profileLabel(beta.id), /0\.5\.0b1/);
  const versions = ['0.5.0.dev1', '0.5.0a99', '0.5.0b1.dev1', '0.5.0b1.dev2', '0.5.0b1', '0.5.0b10', '0.5.0rc1', '0.5.0'];
  for (let i = 1; i < versions.length; i++) assert.ok(b.versionRank(versions[i-1]) < b.versionRank(versions[i]));
});
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
  const revision = 'cb38e98632eebdf6c5bb60a792b83320c1312b28';
  const gas = {id: 'solar-cb38e986-gas-runs200', language: 'solidity', compiler_name: 'solar', compiler_version: '0.2.0', source_revision: revision, solidity_version: '0.8.38', optimizer: 'gas', optimizer_runs: 200};
  const size = {...gas, id: 'solar-cb38e986-size-runs1', optimizer: 'size', optimizer_runs: 1};
  const next = {...gas, id: 'solar-next-gas-runs200', source_revision: 'a'.repeat(40)};
  const b = load([gas, size, next]);
  const knobs = b.profileKnobs(gas);
  assert.equal(knobs.versionKey, revision);
  assert.equal(knobs.runs, 200);
  assert.equal(b.profileFacets('solidity', revision, 'size', 'solar').runs.join(), '1');
  assert.equal(b.resolveProfile({...knobs, optimizer: 'size', runs: 1}), size.id);
  assert.match(b.profileLabel(gas.id), /^Solar 0\.2\.0 @cb38e986 gas runs200$/);
  assert.equal(b.latestBaselineProfile(gas), undefined);
});

test('pair compile coverage counts artifacts once per profile, including compile failures', () => {
  const data = [...rows, {profile_id: profiles[1].id, status: 'compile_error', suite: 'fixed', benchmark_id: 'vault'}];
  const b = load([], data);
  const pair = b.profilePairCompileCoverage(data, profiles[0].id, profiles[1].id);
  assert.deepEqual([pair.passed, pair.attempted, pair.failed], [2, 3, 1]);
  const same = b.profilePairCompileCoverage(data, profiles[0].id, profiles[0].id);
  assert.deepEqual([same.passed, same.attempted, same.failed], [1, 1, 0]);
});

test('version axis compares default-codegen profiles only', () => {
  const vyper = profiles[3];
  const extra = [
    {...vyper, id: 'vyper-latest-gas-venom', experimental_codegen: true},
    {...vyper, id: 'vyper-0.4.0-gas', compiler_version: '0.4.0'},
    {...vyper, id: 'vyper-0.4.0-gas-venom', compiler_version: '0.4.0', experimental_codegen: true},
  ];
  const data = [...rows, ...extra.map(p => ({...rows.at(-1), profile_id: p.id}))];
  const b = load(extra, data);
  assert.equal(b.versionAxisRows('runtime_bytes_stripped').map(r => r.profile).join(), 'vyper-latest-gas,vyper-0.4.0-gas');
});

test('scale scenario preference falls back per N where the preferred scenario is not generated', () => {
  const scaleRows = [1, 2, 4].flatMap(n => [
    ...(n > 1 ? ['last_selector'] : []), 'first_selector', 'set_sink',
  ].map(scenario => ({parameter_value: n, gas: {scenario}})));
  const b = load();
  const picked = b.preferredScenarioRows(scaleRows, ['last_selector', 'first_selector']);
  assert.deepEqual(picked.map(r => `${r.parameter_value}:${r.gas.scenario}`),
    ['1:first_selector', '2:last_selector', '4:last_selector']);
});

test('uniform dispatch statistics require a complete sweep and exclude the state writer', () => {
  const sweep = Array.from({length: 32}, (_, i) => ({
    ...rows[0], family: 'dispatch_N', parameter_value: 32,
    gas: {scenario: 'selector_f' + String(i).padStart(3, '0'), harness_call_gas: 100 + i},
  })).reverse();
  const writer = {...sweep[0], gas: {scenario: 'set_sink', harness_call_gas: 20000}};
  const b = load([], [...sweep, writer]);
  const gas = row => b.valueAt(row, 'harness_call_gas');
  const {mean, p95, worst} = b.dispatchSummary([...sweep, writer], gas);
  assert.deepEqual([mean, p95, worst], [115.5, 130, 131]);
  assert.equal(b.dispatchSummary(sweep.slice(1), gas), null);
  assert.equal(b.dispatchSummary([...sweep.slice(1), sweep[1]], gas), null);
  assert.equal(b.dispatchSummary(sweep, r => r === sweep[0] ? undefined : gas(r)), null);
  const single = {...sweep.at(-1), parameter_value: 1};
  assert.equal(b.dispatchSummary([single], gas).p95, 100);
});

test('deployment costs count a repeated deployment once and preserve independent variants', () => {
  const data = profiles.slice(0, 2).flatMap((p, i) =>
    ['deposit', 'withdraw', 'redeem'].map((scenario, j) => ({
      ...rows[0], profile_id: p.id,
      gas: {scenario, deployment_variant: j === 2 ? 'oracle' : 'standard', internal_create_gas: [100, 400][i] + j * (j === 2 ? 5 : 0)},
    })));
  const b = load([], data);
  const cmp = b.compareProfiles(data, profiles[0].id, profiles[1].id, 'internal_create_gas');
  assert.deepEqual([...cmp].map(x => x.key).sort(), ['fixed|erc20||oracle', 'fixed|erc20||standard']);
  assert.equal(b.comparisonUnit('internal_create_gas').plural, 'deployments');
});

test('size-limited comparisons require an eligible shared deployment on both sides', () => {
  const data = profiles.slice(0, 2).flatMap((p, i) => ['standard','oracle'].map((variant, j) => ({
    ...rows[0], profile_id:p.id, gas:{...rows[0].gas, deployment_variant:variant},
    bytecode:{runtime_bytes_stripped:100, within_size_limits:i === j},
  })));
  const b = load([], data);
  assert.equal(b.legacySizeEvidence, false);
  assert.equal(b.compareProfiles(data, profiles[0].id, profiles[1].id, 'runtime_bytes_stripped').length, 0);
  assert.equal(b.compareProfiles(data, profiles[0].id, profiles[1].id, 'runtime_bytes_stripped', undefined, true).length, 1);
  const unknown = {...data[0],bytecode:{runtime_bytes_stripped:100, eip170_margin_bytes:2000}};
  assert.equal(b.sizeStatus(unknown), 'unknown');
  assert.equal(b.valueAt(unknown, 'runtime_bytes_stripped'), undefined);
  const old = load([], [unknown]);
  assert.equal(old.legacySizeEvidence, true);
  assert.equal(old.valueAt(unknown, 'runtime_bytes_stripped'), 100);
  assert.equal(old.valueAt({...unknown, bytecode:{...unknown.bytecode,eip170_margin_bytes:-1}}, 'runtime_bytes_stripped'), undefined);
});

test('family-balanced ratios give equal votes to each family and to each N', () => {
  const pairs = [
    {row: {suite:'fixed', benchmark_id:'a'}, ratio:1},
    ...[1, 2, 2, 2].map(n => ({row:{suite:'scale', family:'dispatch', parameter_value:n}, ratio:n === 1 ? 4 : 1})),
  ];
  const b = load();
  assert.equal(b.familyBalanced(pairs).families, 2);
  assert.ok(Math.abs(b.familyBalanced(pairs).familyGeomean - Math.sqrt(2)) < 1e-12);
});

test('each version series fixes its workload and historical default stays separate from none', () => {
  const vyper = profiles[3];
  const extra = [
    {...vyper, id:'vyper-0.4.0-gas', compiler_version:'0.4.0'},
    {...vyper, id:'vyper-0.3.10-gas', compiler_version:'0.3.10'},
    {...vyper, id:'vyper-0.2.16-default', compiler_version:'0.2.16'},
  ];
  const data = [vyper, ...extra].flatMap((p,i) => (i === 2 ? ['A'] : ['A','B']).map(k => ({
    ...rows[0], profile_id:p.id, benchmark_id:k,
    gas:{...rows[0].gas, harness_call_gas: k === 'A' ? 100 : 200*(i+1)},
  })));
  const b = load(extra, data);
  const chart = b.versionAxisRows('harness_call_gas');
  const four = chart.find(r => r.profile === extra[0].id);
  assert.deepEqual([four.comparable, four.available, four.deltaPct], [1,2,0]);
  const old = chart.find(r => r.profile === extra[2].id);
  assert.deepEqual([old.config,old.baseline,old.comparable], ['default', vyper.id, 2]);
});

test('a selector sweep produces one equal-probability workload for a matched pair', () => {
  const data = profiles.slice(0,2).flatMap((profile, i) => [0,1].map(j => ({
    ...rows[0], profile_id:profile.id, suite:'scale', benchmark_id:'dispatch_2', family:'dispatch_N', parameter_value:2,
    gas:{...rows[0].gas, scenario:'selector_f00'+j, harness_call_gas: i ? [100,180][j] : [60,80][j]},
  })));
  const b = load([], data);
  const cmp = b.compareProfiles(data, profiles[0].id, profiles[1].id, 'harness_call_gas');
  assert.deepEqual([cmp.length, cmp[0].valueA, cmp[0].valueB, cmp[0].ratio], [1,70,140,2]);
  assert.equal(b.compareProfiles(data.slice(1), profiles[0].id, profiles[1].id, 'harness_call_gas').length, 0);
});

test('only fresh interleaved timings enter the comparator', () => {
  const b = load();
  const value = compile => b.valueAt({...rows[0], compile}, 'compile_wall_ms');
  assert.equal(value({wall_ms_samples: [1, 2, 3]}), undefined);
  assert.equal(value({timing_method: 'interleaved-v1', wall_ms_samples: [1]}), undefined);
  assert.equal(value({timing_method: 'interleaved-v1', wall_ms_samples: [6, 2, 4]}), 4);
});

test('drilldown counts deployment variants, eligible size artifacts and reliability denominators separately', () => {
  const data = profiles.slice(0,2).flatMap((p, i) => ['standard','standard','oracle'].map((variant, j) => ({
    ...rows[0], profile_id:p.id, gas:{...rows[0].gas, scenario:'step'+j, deployment_variant:variant, internal_create_gas:100},
    bytecode:{runtime_bytes_stripped:110, within_size_limits:i === 0 || variant !== 'standard'},
  })));
  data.push({...rows[0], status:'compile_error', profile_id:profiles[2].id});
  const b = load([], data);
  const values = metric => [...b.drillUnits(data, metric)].filter(x => !x.failure);
  assert.equal(values('internal_create_gas').length, 3);
  assert.equal(values('runtime_bytes_stripped').length, 2);
  assert.equal(values('compile_wall_ms').length, 0);
  const attempts = [...b.drillUnits(data, null)];
  assert.equal(attempts.length, 3);
  assert.equal(attempts.filter(x => x.failure === 'compile').length, 1);
});

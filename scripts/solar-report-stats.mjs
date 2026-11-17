// Score the release audit with exactly the same workload units and gates as the report.
import {readFileSync} from 'node:fs';

const [modelPath, baseline, ...profiles] = process.argv.slice(2);
if (!modelPath || !baseline || !profiles.length) {
  throw new Error('usage: node scripts/solar-report-stats.mjs MODEL BASELINE PROFILE...');
}
globalThis.window = {__BENCH_DATA: JSON.parse(readFileSync(modelPath, 'utf8'))};
await import('../report-ui/src/bench-data.js');
const bench = window.Bench;
const scopes = {
  headline: new Set(['fixed', 'scale']),
  fixed: new Set(['fixed']),
  scale: new Set(['scale']),
  real_derived: new Set(['real_derived']),
  all: undefined,
};
const delta = ratio => ratio == null ? null : (ratio - 1) * 100;
const stats = {};
for (const profile of profiles){
  if (!bench.profileById(profile) || !bench.profileById(baseline)) throw new Error('Unknown audit profile');
  const result = {};
  for (const [scope, suites] of Object.entries(scopes)){
    const metrics = {};
    for (const {id} of bench.METRICS){
      const pairs = bench.compareProfiles(bench.D.rows, baseline, profile, id, suites);
      const {geomean, count} = bench.summarize(pairs);
      metrics[id] = {
        n: count,
        geomean_delta_pct: delta(geomean),
        family_balanced_delta_pct: delta(bench.familyBalanced(pairs).familyGeomean),
      };
    }
    result[scope] = metrics;
  }
  stats[profile] = result;
}
console.log(JSON.stringify(stats));

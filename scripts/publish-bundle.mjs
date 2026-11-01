import {createHash} from 'node:crypto';
import {existsSync, readFileSync} from 'node:fs';
import {join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';

export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function loadCompletedBundle(root) {
  const pointer = join(root, 'results/normalized/bundle-receipt.json');
  if (!existsSync(pointer)) throw new Error('No completed run bundle. Run the benchmark pipeline once with this version before publishing.');
  const receipt = JSON.parse(readFileSync(pointer, 'utf8'));
  if (!/^\d{8}T\d{15}Z$/.test(receipt.run_id)) throw new Error('Invalid completed run ID');
  const dir = join(root, 'target/bench-runs', receipt.run_id);
  const frozen = JSON.parse(readFileSync(join(dir, 'receipt.json'), 'utf8'));
  if (!isDeepStrictEqual(receipt, frozen)) throw new Error('Completed run receipts disagree');
  const files = {};
  for (const name of ['run-manifest.json', 'report-model.json', 'results.json', 'foundry-gas.jsonl']) {
    const bytes = readFileSync(join(dir, name));
    if (sha256(bytes) !== receipt.sha256[name]) throw new Error(`Completed run digest mismatch for ${name}`);
    files[name] = bytes;
  }

  const manifest = JSON.parse(files['run-manifest.json']);
  if (manifest.bundle_version !== 1 || manifest.run_id !== receipt.run_id) throw new Error('Completed run identity mismatch');
  const gas = manifest.gas_records;
  const failures = manifest.compile_failures;
  if (!Number.isSafeInteger(gas) || gas < 0 || !Number.isSafeInteger(failures) || failures < 0 || gas + failures === 0) {
    throw new Error('Empty or invalid completed run');
  }
  const raw = files['foundry-gas.jsonl'].toString('utf8').split('\n').filter(line => line.trim());
  if (raw.length !== gas) throw new Error('Raw gas count disagrees with run manifest');
  const results = JSON.parse(files['results.json']);
  if (!Array.isArray(results) || results.length !== gas + failures
    || results.filter(row => row.status === 'ok').length !== gas
    || results.filter(row => row.status === 'compile_error').length !== failures) {
    throw new Error('Normalized result counts disagree with run manifest');
  }
  const model = JSON.parse(files['report-model.json']);
  if (!isDeepStrictEqual(model.manifest, manifest) || !Array.isArray(model.rows) || model.rows.length !== results.length) {
    throw new Error('Report model disagrees with completed run manifest or row count');
  }
  // Expand the report's lookup references and match complete normalized rows.
  // The UI intentionally omits the per-row cache diagnostics.
  const measured = new Map();
  for (const [index, row] of results.entries()) {
    const {cache, ...normalized} = row;
    const {compiler_ref, provenance_ref, ...compact} = model.rows[index];
    const expanded = {
      ...compact,
      compiler: model.lookups.compiler[compiler_ref],
      provenance: model.lookups.provenance[provenance_ref],
    };
    if (!isDeepStrictEqual(expanded, normalized)) throw new Error(`Report model disagrees with normalized row ${index}`);
    if (row.status === 'ok') measured.set(gasKey(row, row.gas), row);
  }
  for (const line of raw) {
    const record = JSON.parse(line);
    const key = gasKey(record, record);
    const row = measured.get(key);
    if (!row) throw new Error(`Unexpected or duplicate raw gas record: ${key}`);
    for (const field of ['harness_call_gas', 'internal_create_gas', 'intrinsic_gas', 'calldata_gas', 'harness_estimated_tx_gas', 'expected_success', 'call_succeeded', 'scenario_status_ok', 'metadata_mode']) {
      if (record[field] !== row.gas[field]) throw new Error(`Raw ${field} disagrees with normalized result: ${key}`);
    }
    for (const field of ['return_hash', 'observer_hash', 'log_hash']) {
      if (record[field] !== row.behavior[field]) throw new Error(`Raw ${field} disagrees with behavior evidence: ${key}`);
    }
    for (const field of ['initcode_bytes', 'deployed_runtime_bytes']) {
      if (record[field] !== row.bytecode[field]) throw new Error(`Raw ${field} disagrees with deployed bytecode: ${key}`);
    }
    const golden = record.golden_check_passed == null ? 'not_run' : record.golden_check_passed ? 'pass' : 'fail';
    if (golden !== row.correctness.golden_behavior_check || !isDeepStrictEqual(record.cache, row.cache.gas)) {
      throw new Error(`Raw check or cache disagrees with normalized evidence: ${key}`);
    }
    measured.delete(key);
  }
  return {files, manifest};
}

function gasKey(row, gas) {
  const fields = [row.benchmark_id, row.implementation_id, row.profile_id, gas.scenario, gas.state_access_profile];
  if (fields.some(value => typeof value !== 'string' || !value)) throw new Error('Invalid gas row identity');
  return JSON.stringify(fields);
}

import test from 'node:test';
import assert from 'node:assert/strict';
import { assertProductionSnapshot } from './publish-policy.mjs';

const checkout = {branch: 'master', dirty: false, commit: 'current'};
const manifest = {environment: {git: {dirty: false, commit: 'current'}, command_line: ['bench', 'run']}};
test('production publishing rejects dirty, stale, and partial measurements', () => {
  assert.doesNotThrow(() => assertProductionSnapshot(checkout, manifest));
  assert.throws(() => assertProductionSnapshot({...checkout, dirty: true}, manifest));
  assert.throws(() => assertProductionSnapshot({...checkout, branch: 'feat/test'}, manifest));
  assert.throws(() => assertProductionSnapshot({...checkout, commit: 'different'}, manifest));
  assert.throws(() => assertProductionSnapshot(checkout, {environment: {...manifest.environment, git: {dirty: true, commit: 'current'}}}));
  assert.throws(() => assertProductionSnapshot(checkout, {environment: {...manifest.environment, command_line: ['bench', 'run', '--profile=solx-0.1.8-O3']}}));
});

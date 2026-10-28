import { parseArgs } from 'node:util';

export function publishOptions(argv, env) {
  // Strict parsing accepts `--channel prod` and `--channel=prod` and rejects
  // unknown or misspelled options instead of silently publishing to dev.
  const { values } = parseArgs({
    args: argv,
    strict: true,
    options: {
      bucket: {type: 'string'},
      prefix: {type: 'string'},
      channel: {type: 'string'},
      upload: {type: 'boolean', default: false},
      deploy: {type: 'boolean', default: false},
    },
  });
  return {
    bucket: values.bucket || env.CF_R2_BUCKET || 'evm-compilers',
    prefix: stripSlashes(values.prefix || env.CF_R2_PREFIX || 'evm-compiler-bench'),
    channel: normalizeChannel(values.channel || env.EVM_BENCH_CHANNEL || env.CF_R2_CHANNEL || 'dev'),
    upload: values.upload,
    deploy: values.deploy,
  };
}

export function assertProductionSnapshot(checkout, manifest) {
  if (checkout.branch !== 'master' || checkout.dirty) {
    throw new Error('Production results must be published from a clean master checkout, including untracked files.');
  }
  if (manifest.environment?.git?.dirty !== false || manifest.environment?.git?.commit !== checkout.commit) {
    throw new Error('Production results must be generated from this clean commit. Run the full pipeline and validate after merging.');
  }
  const args = manifest.environment?.command_line || [];
  if (args.some(arg => /^(--benchmark|--profile)(=|$)/.test(arg))) {
    throw new Error('Production requires the full benchmark/profile matrix, not a filtered development run.');
  }
}

function stripSlashes(value) {
  return value.replace(/^\/+|\/+$/g, '');
}

function normalizeChannel(value) {
  const channel = String(value).trim().toLowerCase();
  if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(channel)) {
    throw new Error(`invalid channel ${JSON.stringify(value)}; expected [a-z0-9][a-z0-9_-]{0,31}`);
  }
  return channel;
}

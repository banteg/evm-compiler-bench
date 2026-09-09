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

import fs from "node:fs";
import path from "node:path";

const [inputPath, outputPath] = process.argv.slice(2);

if (!inputPath || !outputPath) {
  console.error("usage: node make-design-model.mjs <report-model.json> <output.json>");
  process.exit(2);
}

const model = JSON.parse(fs.readFileSync(inputPath, "utf8"));

// Per-row evidence the report UI never reads; it dominates the model size.
const EVIDENCE_FIELDS = new Set([
  "behavior",
  "cache",
  "compiler",
  "generated",
  "provenance",
  "source_hash",
  "source_path",
]);

// Keep check statuses, drop per-check evidence lists (behavior pairs, artifacts, property names).
function compactCorrectness(correctness) {
  if (!correctness) return correctness;
  return Object.fromEntries(Object.entries(correctness).filter(([, value]) => !Array.isArray(value)));
}

function compactRow(row) {
  const out = Object.fromEntries(Object.entries(row).filter(([key]) => !EVIDENCE_FIELDS.has(key)));
  out.correctness = compactCorrectness(row.correctness);
  return out;
}

const output = {
  ...model,
  design_fixture: {
    generated_at: new Date().toISOString(),
    row_count: model.rows.length,
    source_file: path.basename(inputPath),
    note: "Row evidence fields are pruned for design-tool upload limits. Full evidence lives in results/normalized/report-model.json.",
  },
  rows: model.rows.map(compactRow),
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, JSON.stringify(output));

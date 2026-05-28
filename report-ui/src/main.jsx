import React, { useState, useMemo } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

async function loadReportData() {
  if (window.__BENCH_DATA || window.__EVM_BENCH_REPORT_DATA) return;

  const params = new URLSearchParams(window.location.search);
  const configured = params.get("data")
    || window.__EVM_BENCH_DATA_URL
    || document.querySelector('meta[name="evm-bench-data"]')?.content
    || import.meta.env.VITE_BENCH_DATA_URL;

  const latest = await loadPublishManifest();
  const publishedModel = latest?.artifacts?.report_model;
  const publishedVersion = publishedModel?.encoded_sha256
    || publishedModel?.sha256
    || latest?.published_at;
  const publishedPath = publishedModel?.path
    ? `${publishedModel.path}${publishedVersion ? `?v=${publishedVersion}` : ""}`
    : null;
  const candidates = [
    configured,
    publishedPath,
    "./report-model.json",
    "/report-model.json",
  ].filter(Boolean);

  const errors = [];
  for (const candidate of candidates) {
    try {
      const response = await fetch(candidate, { cache: "no-cache" });
      if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
      const text = await response.text();
      if (text.trimStart().startsWith("<")) {
        throw new Error("received HTML instead of JSON");
      }
      window.__EVM_BENCH_REPORT_DATA = JSON.parse(text);
      window.__EVM_BENCH_DATA_SOURCE = candidate;
      break;
    } catch (error) {
      errors.push(`${candidate}: ${error.message}`);
    }
  }

  if (!window.__EVM_BENCH_REPORT_DATA) {
    throw new Error(`Failed to load report data. Tried ${errors.join("; ")}`);
  }
}

async function loadPublishManifest() {
  if (window.__EVM_BENCH_PUBLISH_MANIFEST) return window.__EVM_BENCH_PUBLISH_MANIFEST;
  try {
    const response = await fetch("./latest.json", { cache: "no-cache" });
    if (!response.ok) return null;
    const text = await response.text();
    if (text.trimStart().startsWith("<")) return null;
    window.__EVM_BENCH_PUBLISH_MANIFEST = JSON.parse(text);
    return window.__EVM_BENCH_PUBLISH_MANIFEST;
  } catch {
    // Local generated reports do not need a publish manifest.
    return null;
  }
}

function renderLoadError(error) {
  createRoot(document.getElementById("root")).render(
    React.createElement("main", { className: "shell hero" },
      React.createElement("div", { className: "load-error" },
        React.createElement("strong", null, "Failed to load report data"),
        React.createElement("pre", null, error.message)
      )
    )
  );
}

try {
  await loadReportData();
} catch (error) {
  renderLoadError(error);
  throw error;
}

await import("./bench-data.js");
await import("./bench-charts.jsx");

const Bench = window.Bench;
const BenchCharts = window.BenchCharts;
const { ScenarioDeltaChart, EvolutionChart, ScaleChart, DeltaHistogram } = BenchCharts;

// ============================================================
// Constants & headline finding helpers
// ============================================================

const SUITES = Bench.SUITES;
const METRICS = Bench.METRICS;
const SOL_LEGACY = 'solc-latest-legacy-runs200';
const SOL_VIAIR = 'solc-latest-viair-runs200';
const SOL_NOOPT = 'solc-latest-noopt';
const SOL_0426_LEGACY = 'solc-0.4.26-legacy-runs200';
const VYPER_0310_GAS = 'vyper-0.3.10-gas';
const VYPER_GAS = 'vyper-latest-gas';
const VYPER_GAS_VENOM = 'vyper-latest-gas-venom';
const HEADLINE_SUITES = new Set(['fixed', 'scale']);

// Pre-compute headline stories ONCE
function buildHeadlines() {
  const rows = Bench.D.rows;
  const M = 'harness_call_gas';
  const S = 'runtime_bytes_stripped';

  const v = (a, b, metric) => {
    const cmp = Bench.compareProfiles(rows, a, b, metric, HEADLINE_SUITES);
    return {
      ...Bench.summarize(cmp),
      cmp,
      coverage: Bench.profilePairCompileCoverage(rows, a, b, HEADLINE_SUITES),
    };
  };

  return {
    stableSolVsVyperGas: v(SOL_LEGACY, VYPER_GAS, M),
    stableSolVsVyperSize: v(SOL_LEGACY, VYPER_GAS, S),
    solVsVyperVenomGas: v(SOL_VIAIR, VYPER_GAS_VENOM, M),
    solVsVyperVenomSize: v(SOL_VIAIR, VYPER_GAS_VENOM, S),
    venomGas:        v(VYPER_GAS, VYPER_GAS_VENOM, M),
    venomSize:       v(VYPER_GAS, VYPER_GAS_VENOM, S),
    venomCompile:    v(VYPER_GAS, VYPER_GAS_VENOM, 'compile_wall_ms'),
    viaIRGas:        v(SOL_LEGACY, SOL_VIAIR, M),
    viaIRSize:       v(SOL_LEGACY, SOL_VIAIR, S),
    viaIRCompile:    v(SOL_LEGACY, SOL_VIAIR, 'compile_wall_ms'),
    solEra:          v(SOL_0426_LEGACY, SOL_LEGACY, M),
    nooptGas:        v(SOL_VIAIR, SOL_NOOPT, M),
    nooptSize:       v(SOL_VIAIR, SOL_NOOPT, S),
  };
}

const HEADLINES = buildHeadlines();
const REPORT_VERSION = 'v2';

// ============================================================
// Top bar
// ============================================================
function TopBar() {
  return React.createElement('div', { className: 'shell' },
    React.createElement('div', { className: 'topbar' },
      React.createElement('div', { className: 'brand' },
        React.createElement('span', { className: 'brand-mark' }),
        'EVM Compiler Bench'
      ),
      React.createElement('nav', null,
        React.createElement('a', { href: '#findings' }, 'Findings'),
        React.createElement('a', { href: '#compare' }, 'Compare'),
        React.createElement('a', { href: '#drilldown' }, 'Drilldown'),
        React.createElement('a', { href: '#versions' }, 'Versions'),
        React.createElement('a', { href: '#scale' }, 'Scale'),
        React.createElement('a', { href: '#reliability' }, 'Reliability'),
        React.createElement('a', { href: '#configs' }, 'Configs'),
        React.createElement('a', { href: '#methodology' }, 'Methods'),
      ),
    )
  );
}

// ============================================================
// Hero
// ============================================================
function Hero() {
  const m = Bench.D.manifest;
  const s = Bench.D.summary;
  const gen = new Date(Bench.D.generated_at);
  const solcLegacy = Bench.profileLabel(SOL_LEGACY);
  const solcViaIR = Bench.profileLabel(SOL_VIAIR);
  const vyperGas = Bench.profileLabel(VYPER_GAS);
  const vyperVenom = Bench.profileLabel(VYPER_GAS_VENOM);
  const absDelta = ratio => `${Math.abs((ratio - 1) * 100).toFixed(1)}%`;

  return React.createElement('section', { className: 'shell hero' },
    React.createElement('div', { className: 'hero-eyebrow' },
      React.createElement('span', { className: 'dot' }),
      `Compiler bench · ${REPORT_VERSION} · ${gen.toISOString().slice(0,10)} · ${s.profiles} profiles × ${s.benchmarks} benchmarks`
    ),
    React.createElement('h1', { className: 'hero-title' },
      'The ',
      React.createElement('em', null, 'definitive'),
      ' EVM compiler benchmark.'
    ),
    React.createElement('p', { className: 'hero-lede' },
      'Across ',
      React.createElement('strong', null, s.ok_rows.toLocaleString()),
      ` successful scenario measurements, ${vyperGas} is ${absDelta(HEADLINES.stableSolVsVyperGas.geomean)} lower runtime gas than ${solcLegacy}. Against ${solcViaIR}, ${vyperVenom} is ${absDelta(HEADLINES.solVsVyperVenomGas.geomean)} lower gas and ${absDelta(HEADLINES.solVsVyperVenomSize.geomean)} smaller runtime bytecode. Explore the comprehensive breakdown below.`
    ),

    React.createElement('div', { className: 'hero-strip' },
      React.createElement('div', null,
        React.createElement('div', { className: 'k' }, 'Comparable rows'),
        React.createElement('div', { className: 'v tabular' }, s.ok_rows.toLocaleString()),
        React.createElement('div', { className: 'vs' }, 'fixed · scale · real-derived'),
      ),
      React.createElement('div', null,
        React.createElement('div', { className: 'k' }, 'Artifacts compiled'),
        React.createElement('div', { className: 'v tabular' },
          `${s.successful_artifacts.toLocaleString()}`,
          React.createElement('span', { style: { color: 'var(--fg-4)', fontSize: '14px' } }, ` / ${s.attempted_artifacts.toLocaleString()}`),
        ),
        React.createElement('div', { className: 'vs' },
          `${s.failed_artifacts} failures · ${((s.successful_artifacts/s.attempted_artifacts)*100).toFixed(1)}% pass`),
      ),
      React.createElement('div', null,
        React.createElement('div', { className: 'k' }, 'Correctness'),
        React.createElement('div', { className: 'v tabular' }, s.correctness.scenario_status_pass.toLocaleString()),
        React.createElement('div', { className: 'vs' }, `${s.correctness.property_rows} property · ${s.correctness.randomized_rows} randomized`),
      ),
      React.createElement('div', null,
        React.createElement('div', { className: 'k' }, 'EVM target'),
        React.createElement('div', { className: 'v tabular' }, m.evm_version),
        React.createElement('div', { className: 'vs' }, `${(m.environment?.os || '')} · ${(m.environment?.arch || '')}`),
      ),
    )
  );
}

// ============================================================
// Headline findings grid (the "answer at a glance")
// ============================================================
function FindingsGrid() {
  const solcLegacy = Bench.profileLabel(SOL_LEGACY);
  const solcViaIR = Bench.profileLabel(SOL_VIAIR);
  const solcNoopt = Bench.profileLabel(SOL_NOOPT);
  const vyperGas = Bench.profileLabel(VYPER_GAS);
  const vyperVenom = Bench.profileLabel(VYPER_GAS_VENOM);
  const absDelta = ratio => {
    if (ratio == null || !isFinite(ratio)) return '—';
    return Math.abs((ratio - 1) * 100).toFixed(1) + '%';
  };
  const lowerHigher = (ratio, noun) => {
    if (ratio == null || !isFinite(ratio)) return noun;
    return `${absDelta(ratio)} ${ratio <= 1 ? 'lower' : 'higher'} ${noun}`;
  };
  const passRate = coverage => coverage?.passRate == null
    ? '—'
    : `${(coverage.passRate * 100).toFixed(1)}%`;
  const cards = [
    {
      tag: 'Finding 01',
      span: 4,
      headline: 'Vyper gas beats solc legacy on runtime gas.',
      body: `Comparing stable, optimizer-enabled profiles over fixed and scale benchmarks, ${vyperGas} uses ${absDelta(HEADLINES.stableSolVsVyperGas.geomean)} less runtime gas than ${solcLegacy}.`,
      stat: HEADLINES.stableSolVsVyperGas.geomean,
      statLabel: 'runtime gas (Vyper gas vs solc legacy)',
      altStat: HEADLINES.stableSolVsVyperSize.geomean,
      altLabel: 'runtime bytes',
      count: HEADLINES.stableSolVsVyperGas.count,
      coverage: HEADLINES.stableSolVsVyperGas.coverage,
    },
    {
      tag: 'Finding 02',
      span: 4,
      headline: 'Vyper + Venom beats solc viaIR on both axes.',
      body: `Against ${solcViaIR}, ${vyperVenom} uses ${absDelta(HEADLINES.solVsVyperVenomGas.geomean)} less runtime gas and ${absDelta(HEADLINES.solVsVyperVenomSize.geomean)} fewer runtime bytes.`,
      stat: HEADLINES.solVsVyperVenomGas.geomean,
      statLabel: 'runtime gas (Vyper Venom vs solc viaIR)',
      altStat: HEADLINES.solVsVyperVenomSize.geomean,
      altLabel: 'runtime bytes',
      count: HEADLINES.solVsVyperVenomGas.count,
      coverage: HEADLINES.solVsVyperVenomGas.coverage,
    },
    {
      tag: 'Finding 03',
      span: 4,
      headline: 'Venom makes Vyper smaller and cheaper.',
      body: `Enabling --experimental-codegen ("Venom") in Vyper gives ${lowerHigher(HEADLINES.venomSize.geomean, 'runtime bytecode')}, ${lowerHigher(HEADLINES.venomGas.geomean, 'runtime gas')}, and ${lowerHigher(HEADLINES.venomCompile.geomean, 'compile time')} versus legacy Vyper codegen.`,
      stat: HEADLINES.venomSize.geomean,
      statLabel: 'runtime bytes vs Vyper legacy codegen',
      altStat: HEADLINES.venomGas.geomean,
      altLabel: 'runtime gas',
      count: HEADLINES.venomSize.count,
      coverage: HEADLINES.venomSize.coverage,
    },
    {
      tag: 'Finding 04',
      span: 4,
      headline: 'viaIR buys gas and size with compile time.',
      body: `Switching from ${solcLegacy} to ${solcViaIR} gives ${lowerHigher(HEADLINES.viaIRGas.geomean, 'runtime gas')} and ${lowerHigher(HEADLINES.viaIRSize.geomean, 'runtime bytecode')}, but ${lowerHigher(HEADLINES.viaIRCompile.geomean, 'compile time')}.`,
      stat: HEADLINES.viaIRGas.geomean,
      statLabel: 'runtime gas vs solc legacy',
      altStat: HEADLINES.viaIRCompile.geomean,
      altLabel: 'compile wall time',
      altInvert: true,
      count: HEADLINES.viaIRGas.count,
      coverage: HEADLINES.viaIRGas.coverage,
    },
    {
      tag: 'Finding 05',
      span: 4,
      headline: 'solc legacy nets out flat, with large offsetting moves.',
      body: `Solc 0.4.26 to ${solcLegacy} nets to ${Bench.fmtDelta(HEADLINES.solEra.geomean)} runtime gas, but it is not a smooth drift story: loop bounds and ABI args regress while dispatch and Merkle improve.`,
      stat: HEADLINES.solEra.geomean,
      statLabel: 'runtime gas (solc 0.4.26 → 0.8.35 legacy)',
      neutral: true,
      count: HEADLINES.solEra.count,
      coverage: HEADLINES.solEra.coverage,
    },
    {
      tag: 'Finding 06',
      span: 4,
      headline: 'The optimizer is the biggest lever in the run.',
      body: `Disabling the optimizer entirely in solc 0.8.35 gives ${lowerHigher(HEADLINES.nooptGas.geomean, 'runtime gas')}: the single largest measured effect among all solc profile comparisons.`,
      stat: HEADLINES.nooptGas.geomean,
      statLabel: 'runtime gas without optimizer',
      altStat: HEADLINES.nooptSize.geomean,
      altLabel: 'runtime bytes without optimizer',
      count: HEADLINES.nooptGas.count,
      coverage: HEADLINES.nooptGas.coverage,
    },
  ];

  return React.createElement('section', { id: 'findings', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 01 · Summary'),
        React.createElement('div', { className: 'section-title' }, 'Six findings from this run.'),
        React.createElement('div', { className: 'section-sub' }, 'Each card reports a geometric-mean delta over comparable measurement units; card headers show row count and artifact compile pass rate.')
      )
    ),
    React.createElement('div', { className: 'stories' },
      cards.map((c, i) => {
        const ratio = c.stat;
        const pct = ratio == null ? 0 : (ratio - 1) * 100;
        const tone = c.neutral
          ? 'neutral'
          : pct < -2 ? '' : pct > 2 ? 'bad' : 'warn';
        return React.createElement('div', { key: i, className: `story span-${c.span}` },
          React.createElement('div', { className: 'story-tag' },
            React.createElement('span', null, c.tag),
            React.createElement('span', null, `n=${c.count} · pass ${passRate(c.coverage)}`)
          ),
          React.createElement('h3', { className: 'story-headline' }, c.headline),
          React.createElement('p', { className: 'story-body' }, c.body),
          React.createElement('div', { className: `story-num ${tone}` }, Bench.fmtDelta(ratio)),
          React.createElement('div', { className: 'story-sub' },
            c.statLabel,
            c.altStat != null ? React.createElement('span', { style: { display: 'block', marginTop: '4px', color: 'var(--fg-4)' } },
              `${Bench.fmtDelta(c.altStat)} · ${c.altLabel}`
            ) : null
          )
        );
      })
    )
  );
}

// ============================================================
// Suite scorecards
// ============================================================
function SuiteScorecards({ profileA, profileB, metric }) {
  const rows = useMemo(() =>
    Bench.compareProfiles(Bench.D.rows, profileA, profileB, metric),
    [profileA, profileB, metric]
  );
  const tieBand = Bench.tieBandForMetric(metric);
  const bySuite = useMemo(() => Bench.bySuite(rows, tieBand), [rows, tieBand]);

  return React.createElement('div', { className: 'suite-grid' },
    bySuite.map((s, i) => {
      const pct = s.geomean == null ? null : (s.geomean - 1) * 100;
      const tone = pct == null ? 'tie' : pct < -tieBand * 100 ? 'good' : pct > tieBand * 100 ? 'bad' : 'tie';
      const total = s.count || 1;
      return React.createElement('div', { key: i, className: 'suite-card' },
        React.createElement('div', { className: 'nm' }, SUITES[s.suite].label + ' Suite'),
        React.createElement('div', { className: 'dsc' }, SUITES[s.suite].desc),
        React.createElement('div', { className: `big ${tone}` }, Bench.fmtDelta(s.geomean)),
        React.createElement('div', { className: 'wtl-bar' },
          React.createElement('div', { className: 'w', style: { width: (s.cheaper/total*100) + '%' } }),
          React.createElement('div', { className: 't', style: { width: (s.tie/total*100) + '%' } }),
          React.createElement('div', { className: 'l', style: { width: (s.costlier/total*100) + '%' } }),
        ),
        React.createElement('div', { className: 'ftr' },
          React.createElement('div', null,
            React.createElement('div', { className: 'k' }, 'Cheaper'),
            React.createElement('div', { className: 'v', style: { color: 'var(--accent)' } }, s.cheaper),
          ),
          React.createElement('div', null,
            React.createElement('div', { className: 'k' }, 'Tie'),
            React.createElement('div', { className: 'v' }, s.tie),
          ),
          React.createElement('div', null,
            React.createElement('div', { className: 'k' }, 'Costlier'),
            React.createElement('div', { className: 'v', style: { color: 'var(--bad)' } }, s.costlier),
          ),
        )
      );
    })
  );
}

// ============================================================
// Version evolution
// ============================================================
function VersionEvolution({ metric }) {
  const points = useMemo(() => Bench.versionAxisRows(metric), [metric]);
  return React.createElement('div', { className: 'evo-grid' },
    React.createElement('div', { className: 'evo-side' },
      React.createElement('div', { className: 'evo-head' },
        React.createElement('div', { className: 'evo-title' },
          React.createElement('span', { className: 'lang-sol' }, 'Solidity'),
          React.createElement('span', { style: { color: 'var(--fg-3)' } }, ' / solc')
        ),
        React.createElement('div', { className: 'evo-axis' }, 'Δ vs newest · same codegen')
      ),
      React.createElement(EvolutionChart, { points, language: 'solidity', height: 240 })
    ),
    React.createElement('div', { className: 'evo-side' },
      React.createElement('div', { className: 'evo-head' },
        React.createElement('div', { className: 'evo-title' },
          React.createElement('span', { className: 'lang-vy' }, 'Vyper')
        ),
        React.createElement('div', { className: 'evo-axis' }, 'Δ vs newest · comparable optimize')
      ),
      React.createElement(EvolutionChart, { points, language: 'vyper', height: 240 })
    )
  );
}

// ============================================================
// Scale-N family chart strip
// ============================================================
function ScaleStrip({ metric }) {
  const families = ['dispatch_N', 'storage_slots_N', 'mapping_depth_N', 'abi_args_N',
                    'loop_bound_N', 'events_N', 'external_calls_N']
    .filter(f => Bench.D.rows.some(r => r.family === f));
  const profiles = ['solc-latest-viair-runs200', 'vyper-latest-gas', 'vyper-latest-gas-venom'];
  const palette = {
    'solc-latest-viair-runs200': 'var(--solidity)',
    'vyper-latest-gas': 'var(--vyper)',
    'vyper-latest-gas-venom': 'var(--accent)',
  };
  return React.createElement('div', null,
    React.createElement('div', {
      style: { display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', border: '1px solid var(--line)', background: 'var(--bg-elev)' }
    },
      families.map(f => React.createElement('div', { key: f, style: { padding: '22px', borderRight: '1px solid var(--line)', borderBottom: '1px solid var(--line)' } },
        React.createElement('div', { style: { display: 'flex', justifyContent: 'space-between', marginBottom: '4px', alignItems: 'baseline' } },
          React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '11px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-2)' } },
            f.replace(/_N$/, '').replace(/_/g, ' ')),
          React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '9.5px', color: 'var(--fg-4)', letterSpacing: '0.08em' } }, 'N=1…64')
        ),
        React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', color: 'var(--fg-4)', marginBottom: '10px' } },
          'scenario · ' + (BenchCharts.FAMILY_SCENARIO[f] || '—')
        ),
        React.createElement(ScaleChart, { family: f, metric, profiles, height: 150 })
      ))
    ),
    React.createElement('div', { style: { display: 'flex', gap: '14px', marginTop: '14px', fontFamily: 'var(--mono)', fontSize: '11px', color: 'var(--fg-3)', flexWrap: 'wrap' } },
      profiles.map(p => React.createElement('div', { key: p, style: { display: 'flex', alignItems: 'center', gap: '8px' } },
        React.createElement('span', { style: { width: 12, height: 2, background: palette[p] } }),
        Bench.profileLabel(p)
      ))
    )
  );
}

// ============================================================
// Interactive Comparator
// ============================================================
const CONFIG_EXPLAINERS = {
  'solidity:noopt': 'Optimizer disabled; useful as a control, not a production setting.',
  'solidity:legacy': 'Solidity legacy EVM codegen. The balanced default is optimizer runs=200.',
  'solidity:viaIR': 'Solidity through the IR/Yul pipeline; often better optimized, slower to compile.',
  'vyper:none': 'Vyper optimizer disabled.',
  'vyper:default': 'Historical Vyper default where explicit optimize modes were not available.',
  'vyper:gas': 'Vyper optimizer mode tuned for runtime gas.',
  'vyper:codesize': 'Vyper optimizer mode tuned for smaller bytecode.',
  venom: 'Experimental Vyper backend; orthogonal to the optimizer mode above.',
};

function SegmentedControl({ name, value, options, onChange }) {
  return React.createElement('fieldset', { className: 'segmented' },
    React.createElement('legend', { className: 'sr-only' }, name),
    options.map(option => {
      const id = `${name}-${option.value}`.replace(/[^a-zA-Z0-9_-]/g, '-');
      return React.createElement('label', {
        key: option.value,
        className: `${value === option.value ? 'on' : ''}${option.disabled ? ' disabled' : ''}`,
        title: option.title || '',
        htmlFor: id,
      },
        React.createElement('input', {
          checked: value === option.value,
          disabled: !!option.disabled,
          id,
          name,
          onChange: () => onChange(option.value),
          type: 'radio',
          value: option.value,
        }),
        option.label
      );
    })
  );
}

function ProfilePicker({ title, selected, onChange }) {
  const p = Bench.profileById(selected) || Bench.D.profiles[0];
  const knobs = Bench.profileKnobs(p);
  const facets = Bench.profileFacets(knobs.language, knobs.versionKey, knobs.optimizer);
  const showRuns = knobs.language === 'solidity'
    && (knobs.optimizer === 'legacy' || knobs.optimizer === 'viaIR')
    && facets.runs.length > 1;
  const venomAvailable = Bench.profileOptionExists({
    language: knobs.language,
    versionKey: knobs.versionKey,
    optimizer: knobs.optimizer,
    experimental: true,
  });
  const choose = (patch) => {
    onChange(Bench.resolveProfile({ ...knobs, ...patch }));
  };
  const chooseLang = (l) => onChange(Bench.defaultProfileForLanguage(l));
  const chooseVersion = (versionKey) => {
    const optimizer = Bench.defaultOptimizerForVersion(knobs.language, versionKey);
    const runs = Bench.defaultOptimizerRuns(knobs.language, versionKey, optimizer);
    onChange(Bench.resolveProfile({ ...knobs, versionKey, optimizer, runs, experimental: false }));
  };
  const chooseOptimizer = (optimizer) => {
    const runs = Bench.defaultOptimizerRuns(knobs.language, knobs.versionKey, optimizer);
    choose({ optimizer, runs });
  };
  return React.createElement('div', { className: 'compare-side' },
    React.createElement('div', { className: 'lbl' }, title),
    React.createElement('div', { className: 'knobs' },
      React.createElement('div', { className: 'knob-l' }, 'Lang'),
      React.createElement(SegmentedControl, {
        name: `${title}-language`,
        value: knobs.language,
        options: [
          { value: 'solidity', label: 'Solidity' },
          { value: 'vyper', label: 'Vyper' },
        ],
        onChange: chooseLang,
      }),
      React.createElement('div', { className: 'knob-l' }, 'Version'),
      React.createElement('select', {
        className: 'knob', value: knobs.versionKey,
        onChange: e => chooseVersion(e.target.value),
      },
        facets.versions.map(v => React.createElement('option', { key: v, value: v },
          facets.versionLabels.get(v) || v))
      ),
      React.createElement('div', { className: 'knob-l' }, knobs.language === 'solidity' ? 'Codegen' : 'Optimize'),
      React.createElement(SegmentedControl, {
        name: `${title}-optimizer`,
        value: knobs.optimizer,
        options: facets.optimizers.map(o => ({
          value: o,
          label: o,
          title: CONFIG_EXPLAINERS[`${knobs.language}:${o}`] || '',
        })),
        onChange: chooseOptimizer,
      }),
      showRuns ? React.createElement(React.Fragment, null,
        React.createElement('div', { className: 'knob-l' }, 'Runs'),
        React.createElement('select', {
          className: 'knob',
          value: knobs.runs ?? Bench.defaultOptimizerRuns(knobs.language, knobs.versionKey, knobs.optimizer) ?? '',
          onChange: event => choose({ runs: Number(event.target.value) }),
        },
          facets.runs.map(runs => React.createElement('option', { key: runs, value: runs }, `runs${runs}`))
        ),
      ) : null,
      knobs.language === 'vyper' && facets.supportsExperimental ? React.createElement(React.Fragment, null,
        React.createElement('div', { className: 'knob-l' }, 'Venom'),
        React.createElement(SegmentedControl, {
          name: `${title}-venom`,
          value: knobs.experimental ? 'on' : 'off',
          options: [
            { value: 'off', label: 'off' },
            {
              value: 'on',
              label: 'on',
              disabled: !venomAvailable && !knobs.experimental,
              title: venomAvailable ? CONFIG_EXPLAINERS.venom : 'No Venom build for this version/config',
            },
          ],
          onChange: experimental => choose({ experimental: experimental === 'on' }),
        }),
      ) : null,
    ),
    React.createElement('div', { style: { marginTop: '12px', fontFamily: 'var(--mono)', fontSize: '10.5px', color: 'var(--fg-4)' } },
      React.createElement('code', { title: p.id }, Bench.profileLabel(p.id)))
  );
}

function MetricToggle({ value, onChange }) {
  return React.createElement('div', { className: 'toggle' },
    METRICS.map(m => React.createElement('button', {
      key: m.id,
      className: value === m.id ? 'on' : '',
      onClick: () => onChange(m.id),
    }, m.short))
  );
}

function SectionMetricControl({ metric, setMetric }) {
  return React.createElement('div', { className: 'section-metric-control' },
    React.createElement('div', { className: 'metric-label' }, 'metric'),
    React.createElement(MetricToggle, { value: metric, onChange: setMetric }),
  );
}

// ============================================================
// Drilldown matrix
// ============================================================
const ALL_FILTER = '__all__';
const BALANCED_RUN_FILTER = { op: 'in', values: ['200', 'n/a'] };
const DRILL_AXES = [
  { id: 'suite', label: 'Suite' },
  { id: 'benchmark', label: 'Benchmark' },
  { id: 'family', label: 'Family' },
  { id: 'n', label: 'N' },
  { id: 'language', label: 'Compiler' },
  { id: 'version', label: 'Version' },
  { id: 'mode', label: 'Mode' },
  { id: 'runs', label: 'Runs' },
  { id: 'profile', label: 'Profile' },
  { id: 'status', label: 'Status' },
  { id: 'scenario', label: 'Scenario' },
  { id: 'deployment', label: 'Deployment' },
  { id: 'state', label: 'Access' },
];
const DRILL_AXIS_BY_ID = Object.fromEntries(DRILL_AXES.map(axis => [axis.id, axis]));
const DRILL_AGGREGATIONS = [
  { id: 'median', label: 'Median', needsMetric: true, lowerBetter: true },
  { id: 'mean', label: 'Mean', needsMetric: true, lowerBetter: true },
  { id: 'min', label: 'Min', needsMetric: true, lowerBetter: true },
  { id: 'max', label: 'Max', needsMetric: true, lowerBetter: true },
  { id: 'p90', label: 'P90', needsMetric: true, lowerBetter: true },
  { id: 'count', label: 'Row count', needsMetric: false, lowerBetter: false, unit: 'rows' },
  { id: 'failure_count', label: 'Failure count', needsMetric: false, lowerBetter: true, unit: 'fails' },
  { id: 'failure_rate', label: 'Failure rate', needsMetric: false, lowerBetter: true, unit: '%' },
];
const DRILL_AGG_BY_ID = Object.fromEntries(DRILL_AGGREGATIONS.map(agg => [agg.id, agg]));
const DEFAULT_DRILL_VIEW = {
  rows: ['version'],
  columns: ['n'],
  aggregation: 'median',
  filters: {
    language: { op: 'in', values: ['solidity'] },
    state: { op: 'in', values: ['cold'] },
    family: { op: 'in', values: ['dispatch_N'] },
    runs: BALANCED_RUN_FILTER,
  },
};
const DRILL_PRESETS = [
  {
    label: 'Scale by N',
    metric: 'harness_call_gas',
    view: DEFAULT_DRILL_VIEW,
  },
  {
    label: 'Compiler by suite',
    metric: 'harness_call_gas',
    view: {
      rows: ['suite'],
      columns: ['language'],
      aggregation: 'median',
      filters: { state: { op: 'in', values: ['cold'] }, runs: BALANCED_RUN_FILTER },
    },
  },
  {
    label: 'Version trend',
    metric: 'harness_call_gas',
    view: {
      rows: ['version'],
      columns: ['language'],
      aggregation: 'median',
      filters: {
        state: { op: 'in', values: ['cold'] },
        status: { op: 'in', values: ['ok'] },
        runs: BALANCED_RUN_FILTER,
      },
    },
  },
  {
    label: 'Optimizer mode',
    metric: 'harness_call_gas',
    view: {
      rows: ['mode'],
      columns: ['language'],
      aggregation: 'median',
      filters: { state: { op: 'in', values: ['cold'] }, runs: BALANCED_RUN_FILTER },
    },
  },
  {
    label: 'Failures',
    metric: 'compile_wall_ms',
    view: {
      rows: ['profile'],
      columns: ['status'],
      aggregation: 'failure_count',
      filters: { runs: BALANCED_RUN_FILTER },
    },
  },
];

function scaleFamilyLabel(family) {
  if (!family) return 'none';
  return family.replace(/_N$/, ' (N=1…64)').replace(/_/g, ' ');
}

function drillBenchmarkLabel(row) {
  return row.family ? scaleFamilyLabel(row.family) : row.benchmark_id;
}

function drillModeLabel(profile) {
  if (!profile) return 'unknown';
  const mode = Bench.profileOptimizer(profile);
  return profile.experimental_codegen ? `${mode} + Venom` : mode;
}

function drillCompilerKey(profile, row) {
  const lang = profile?.language || row?.language;
  const compiler = profile?.compiler_name || row?.compiler?.name;
  if (lang === 'solidity' || compiler === 'solc') return 'solc';
  if (lang === 'vyper' || compiler === 'vyper') return 'vyper';
  return compiler || lang || 'unknown';
}

function drillCompilerLabel(value) {
  if (value === 'solc') return 'solc';
  if (value === 'vyper') return 'Vyper';
  return value;
}

function drillVersionKey(profile, row) {
  const compiler = drillCompilerKey(profile, row);
  return `${compiler}|${Bench.profileVersionKey(profile || {})}`;
}

function drillModeKey(profile, row) {
  const compiler = drillCompilerKey(profile, row);
  return `${compiler}|${drillModeLabel(profile)}`;
}

function drillRunsKey(profile) {
  const runs = profile ? Bench.profileOptimizerRuns(profile) : null;
  return runs == null ? 'n/a' : String(runs);
}

function deploymentVariantLabel(value) {
  if (!value || value === 'artifact') return value || 'artifact';
  if (value === 'standard') return 'standard';
  return value.replace(/_/g, ' ');
}

function drillField(row, profile, metric, axis) {
  const artifactLevel = Bench.comparisonLevel(metric) === 'artifact';
  switch (axis) {
    case 'suite': return row.suite || 'unknown';
    case 'benchmark': return drillBenchmarkLabel(row);
    case 'family': return row.family || 'none';
    case 'n': return row.parameter_value == null ? 'none' : String(row.parameter_value);
    case 'language': return row.language || profile?.language || 'unknown';
    case 'version': return drillVersionKey(profile, row);
    case 'mode': return drillModeKey(profile, row);
    case 'runs': return drillRunsKey(profile);
    case 'profile': return row.profile_id;
    case 'status': return row.status === 'ok' ? 'ok' : 'compile_error';
    case 'scenario': return artifactLevel ? 'artifact' : (row.gas?.scenario || 'artifact');
    case 'deployment': return artifactLevel ? 'artifact' : (row.gas?.deployment_variant || 'standard');
    case 'state': return artifactLevel ? 'artifact' : (row.gas?.state_access_profile || 'artifact');
    default: return 'unknown';
  }
}

function drillValueLabel(axis, value) {
  if (value === ALL_FILTER) return 'all';
  if (axis === 'family') return scaleFamilyLabel(value);
  if (axis === 'language') return value === 'solidity' ? 'Solidity' : value === 'vyper' ? 'Vyper' : value;
  if (axis === 'version') {
    const [compiler, version] = String(value).split('|');
    return `${drillCompilerLabel(compiler)} ${version}`;
  }
  if (axis === 'mode') {
    const [compiler, mode] = String(value).split('|');
    return `${drillCompilerLabel(compiler)} ${mode || 'unknown'}`;
  }
  if (axis === 'runs') return value === 'n/a' ? 'n/a' : `runs${value}`;
  if (axis === 'deployment') return deploymentVariantLabel(value);
  if (axis === 'profile') return Bench.profileLabel(value);
  if (axis === 'status') return value === 'compile_error' ? 'compile failed' : value;
  return value;
}

function drillValueRank(axis, value) {
  if (value === 'none') return Number.POSITIVE_INFINITY;
  if (axis === 'n') return Number(value);
  if (axis === 'runs') return value === 'n/a' ? Number.POSITIVE_INFINITY : Number(value);
  if (axis === 'version') {
    const [, version] = String(value).split('|');
    return Bench.versionRank(version || value);
  }
  if (axis === 'status') return value === 'ok' ? 0 : 1;
  if (axis === 'mode') {
    const [, modeValue = ''] = String(value).split('|');
    const [mode] = modeValue.split(' ');
    return Bench.optimizerRank(mode) + (modeValue.toLowerCase().includes('venom') ? 0.25 : 0);
  }
  return null;
}

function sortDrillValues(axis, values) {
  return [...values].sort((a, b) => {
    const ar = drillValueRank(axis, a);
    const br = drillValueRank(axis, b);
    if (ar != null && br != null && ar !== br) return ar - br;
    return drillValueLabel(axis, a).localeCompare(drillValueLabel(axis, b), undefined, { numeric: true });
  });
}

function buildDrillRecords(metric) {
  const artifactLevel = Bench.comparisonLevel(metric) === 'artifact';
  const seenArtifacts = new Set();
  const records = [];
  for (const row of Bench.D.rows) {
    if (row.status !== 'ok' && row.status !== 'compile_error') continue;
    const value = Bench.valueAt(row, metric);
    if (artifactLevel) {
      const key = [row.suite, row.benchmark_id, row.parameter_value ?? '', row.profile_id].join('|');
      if (seenArtifacts.has(key)) continue;
      seenArtifacts.add(key);
    }
    const failed = row.status !== 'ok';
    if (!failed && (value == null || !isFinite(value))) continue;
    const profile = Bench.profileById(row.profile_id);
    const fields = Object.fromEntries(DRILL_AXES.map(axis => [
      axis.id,
      drillField(row, profile, metric, axis.id),
    ]));
    records.push({
      row,
      value: failed ? null : value,
      failed,
      failureReason: failed ? Bench.failureReason(row.compile?.error) : null,
      fields,
    });
  }
  return records;
}

function cloneDrillView(view) {
  return {
    rows: [...(view.rows || DEFAULT_DRILL_VIEW.rows)],
    columns: [...(view.columns || DEFAULT_DRILL_VIEW.columns)],
    aggregation: view.aggregation || DEFAULT_DRILL_VIEW.aggregation,
    filters: Object.fromEntries(Object.entries(view.filters || {}).map(([axis, filter]) => [
      axis,
      { op: filter.op || 'in', values: [...(filter.values || [])] },
    ])),
  };
}

function drillOptions(records, axis) {
  return sortDrillValues(axis, new Set(records.map(record => record.fields[axis])));
}

function normalizeDrillView(view, records) {
  const next = cloneDrillView(view);
  if (!DRILL_AXIS_BY_ID[next.rows[0]]) next.rows = [...DEFAULT_DRILL_VIEW.rows];
  if (!DRILL_AXIS_BY_ID[next.columns[0]]) next.columns = [...DEFAULT_DRILL_VIEW.columns];
  if (!DRILL_AGG_BY_ID[next.aggregation]) next.aggregation = DEFAULT_DRILL_VIEW.aggregation;
  for (const [axis, filter] of Object.entries(next.filters)) {
    if (!DRILL_AXIS_BY_ID[axis]) {
      delete next.filters[axis];
      continue;
    }
    const allowed = new Set(drillOptions(records, axis));
    const values = [...new Set(filter.values || [])].filter(value => allowed.has(value));
    if (values.length) {
      next.filters[axis] = { op: filter.op || 'in', values };
    } else {
      delete next.filters[axis];
    }
  }
  return next;
}

function drillFilterLabel(axis, filter) {
  const values = filter?.values || [];
  if (!values.length) return `${DRILL_AXIS_BY_ID[axis].label}: all`;
  const stringValues = values.map(String);
  if (axis === 'runs' && stringValues.includes('200') && stringValues.includes('n/a') && values.length === 2) {
    return `${DRILL_AXIS_BY_ID[axis].label}: balanced (runs200)`;
  }
  if (values.length === 1) return `${DRILL_AXIS_BY_ID[axis].label}: ${drillValueLabel(axis, values[0])}`;
  return `${DRILL_AXIS_BY_ID[axis].label}: ${values.length} selected`;
}

function passesDrillFilters(record, filters, skipAxis = null) {
  return Object.entries(filters).every(([axis, filter]) => {
    if (axis === skipAxis) return true;
    const values = filter?.values || [];
    if (!values.length) return true;
    if (record.failed && (axis === 'scenario' || axis === 'state')) return true;
    const includes = values.includes(record.fields[axis]);
    return filter.op === 'not-in' ? !includes : includes;
  });
}

function drillOptionCounts(records, axis, filters) {
  const counts = new Map();
  for (const record of records) {
    if (!passesDrillFilters(record, filters, axis)) continue;
    const value = record.fields[axis];
    counts.set(value, (counts.get(value) || 0) + 1);
  }
  return sortDrillValues(axis, counts.keys()).map(value => ({ value, count: counts.get(value) || 0 }));
}

function DrillSelect({ label, value, onChange, axes = DRILL_AXES }) {
  return React.createElement('label', { className: 'drill-query-field' },
    React.createElement('span', null, label),
    React.createElement('select', { className: 'knob', value, onChange: event => onChange(event.target.value) },
      axes.map(axis => React.createElement('option', { key: axis.id, value: axis.id }, axis.label))
    )
  );
}

function DrillFilterPopover({ axis, records, filters, setFilterValues, onClose }) {
  const [search, setSearch] = useState('');
  const selected = filters[axis]?.values || [];
  const selectedSet = new Set(selected);
  const options = useMemo(() => drillOptionCounts(records, axis, filters), [records, axis, filters]);
  const visible = options.filter(option =>
    drillValueLabel(axis, option.value).toLowerCase().includes(search.trim().toLowerCase())
  );
  const toggle = value => {
    const next = selectedSet.has(value)
      ? selected.filter(item => item !== value)
      : [...selected, value];
    setFilterValues(axis, next);
  };
  const selectVisible = () => setFilterValues(axis, sortDrillValues(axis, new Set([...selected, ...visible.map(option => option.value)])));
  return React.createElement('div', { className: 'filter-popover' },
    React.createElement('div', { className: 'filter-popover-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'drill-mini-label' }, 'Filter'),
        React.createElement('div', { className: 'filter-popover-title' }, DRILL_AXIS_BY_ID[axis].label)
      ),
      React.createElement('button', { type: 'button', onClick: onClose, 'aria-label': 'Close filter' }, '×')
    ),
    React.createElement('input', {
      className: 'filter-search',
      placeholder: 'search values',
      value: search,
      onChange: event => setSearch(event.target.value),
    }),
    React.createElement('div', { className: 'filter-options' },
      visible.map(option => {
        const id = `filter-${axis}-${String(option.value).replace(/[^a-zA-Z0-9_-]/g, '-')}`;
        return React.createElement('label', { key: option.value, className: 'filter-option', htmlFor: id },
          React.createElement('input', {
            checked: selectedSet.has(option.value),
            id,
            onChange: () => toggle(option.value),
            type: 'checkbox',
          }),
          React.createElement('span', null, drillValueLabel(axis, option.value)),
          React.createElement('em', null, option.count.toLocaleString())
        );
      })
    ),
    React.createElement('div', { className: 'filter-popover-actions' },
      React.createElement('button', { type: 'button', onClick: selectVisible }, 'select visible'),
      React.createElement('button', { type: 'button', onClick: () => setFilterValues(axis, []) }, 'clear'),
      React.createElement('button', { type: 'button', onClick: onClose }, 'apply')
    )
  );
}

function percentile(sorted, p) {
  if (!sorted.length) return null;
  const index = (sorted.length - 1) * p;
  const lo = Math.floor(index);
  const hi = Math.ceil(index);
  if (lo === hi) return sorted[lo];
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (index - lo);
}

function summarizeDrillCell(group, aggregation) {
  const sorted = [...group.values].sort((a, b) => a - b);
  let value = null;
  switch (aggregation) {
    case 'mean':
      value = sorted.length ? sorted.reduce((sum, item) => sum + item, 0) / sorted.length : null;
      break;
    case 'min':
      value = sorted.length ? sorted[0] : null;
      break;
    case 'max':
      value = sorted.length ? sorted[sorted.length - 1] : null;
      break;
    case 'p90':
      value = percentile(sorted, 0.9);
      break;
    case 'count':
      value = group.total;
      break;
    case 'failure_count':
      value = group.failures;
      break;
    case 'failure_rate':
      value = group.total ? (group.failures / group.total) * 100 : null;
      break;
    case 'median':
    default:
      value = Bench.median(sorted);
      break;
  }
  return value;
}

function drillAggregationInfo(aggregation, metricInfo) {
  const agg = DRILL_AGG_BY_ID[aggregation] || DRILL_AGG_BY_ID.median;
  return {
    ...agg,
    unit: agg.needsMetric ? metricInfo.unit : agg.unit,
    lowerBetter: agg.needsMetric ? metricInfo.lowerBetter : agg.lowerBetter,
  };
}

function formatDrillCellValue(value, aggregation, aggInfo) {
  if (value == null || !isFinite(value)) return null;
  if (aggregation === 'failure_rate') return `${value.toFixed(1)}%`;
  if (aggregation === 'count' || aggregation === 'failure_count') return Math.round(value).toLocaleString();
  return Bench.fmtNum(value);
}

function DrilldownMatrix({ metric, setMetric }) {
  const records = useMemo(() => buildDrillRecords(metric), [metric]);
  const [view, setView] = useState(() => cloneDrillView(DEFAULT_DRILL_VIEW));
  const [activeFilterAxis, setActiveFilterAxis] = useState(null);
  const safeView = useMemo(() => normalizeDrillView(view, records), [view, records]);
  const xAxis = safeView.columns[0];
  const yAxis = safeView.rows[0];
  const filters = safeView.filters;
  const aggregation = safeView.aggregation;
  const metricInfo = METRICS.find(item => item.id === metric) || METRICS[0];
  const aggInfo = drillAggregationInfo(aggregation, metricInfo);
  const setViewPatch = patch => setView(current => cloneDrillView({ ...current, ...patch }));
  const setFilterValues = (axis, values) => setView(current => {
    const next = cloneDrillView(current);
    const unique = sortDrillValues(axis, new Set(values));
    if (unique.length) next.filters[axis] = { op: 'in', values: unique };
    else delete next.filters[axis];
    return next;
  });
  const applyPreset = preset => {
    setMetric(preset.metric);
    setActiveFilterAxis(null);
    setView(cloneDrillView(preset.view));
  };
  const filtered = useMemo(() => records.filter(record => passesDrillFilters(record, filters)), [records, filters]);
  const xValues = useMemo(() => sortDrillValues(xAxis, new Set(filtered.map(record => record.fields[xAxis]))), [filtered, xAxis]);
  const yValues = useMemo(() => sortDrillValues(yAxis, new Set(filtered.map(record => record.fields[yAxis]))), [filtered, yAxis]);
  const cells = useMemo(() => {
    const grouped = new Map();
    for (const record of filtered) {
      const key = `${record.fields[yAxis]}\0${record.fields[xAxis]}`;
      if (!grouped.has(key)) grouped.set(key, { values: [], failures: 0, total: 0, reasons: new Map() });
      const group = grouped.get(key);
      group.total += 1;
      if (record.failed) {
        group.failures += 1;
        if (record.failureReason) {
          group.reasons.set(record.failureReason, (group.reasons.get(record.failureReason) || 0) + 1);
        }
      } else if (record.value != null && isFinite(record.value)) {
        group.values.push(record.value);
      }
    }
    const out = new Map();
    for (const [key, group] of grouped) {
      const reasons = [...group.reasons.entries()]
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
        .map(([reason, count]) => `${reason} (${count})`);
      out.set(key, {
        value: summarizeDrillCell(group, aggregation),
        count: group.values.length,
        failures: group.failures,
        total: group.total,
        reasons,
      });
    }
    return out;
  }, [filtered, xAxis, yAxis, aggregation]);
  const values = [...cells.values()].map(cell => cell.value).filter(value => value != null && isFinite(value));
  const failureRows = filtered.filter(record => record.failed).length;
  const min = values.length ? Math.min(...values) : null;
  const max = values.length ? Math.max(...values) : null;
  const activeFilters = Object.entries(filters);
  const addableAxes = DRILL_AXES.filter(axis => !filters[axis.id]);

  const cellStyle = value => {
    if (value == null || min == null || max == null || min === max) return {};
    const t = (value - min) / (max - min);
    const better = aggInfo.lowerBetter ? 1 - t : t;
    const tone = better >= 0.5 ? 'var(--accent)' : 'var(--bad)';
    const strength = 16 + Math.round(Math.abs(better - 0.5) * 74);
    return {
      background: `color-mix(in srgb, ${tone} ${strength}%, var(--bg-elev) 72%)`,
    };
  };
  const cellTitle = cell => {
    const parts = [];
    if (cell.value != null && isFinite(cell.value)) {
      const unit = aggInfo.unit && aggregation !== 'failure_rate' ? ` ${aggInfo.unit}` : '';
      parts.push(`${aggInfo.label}: ${formatDrillCellValue(cell.value, aggregation, aggInfo)}${unit}`);
      parts.push(`${cell.total} matching row${cell.total === 1 ? '' : 's'}`);
    }
    if (cell.failures) {
      parts.push(`${cell.failures} compile failure${cell.failures === 1 ? '' : 's'}`);
      if (cell.reasons.length) parts.push(cell.reasons.slice(0, 3).join(' · '));
    }
    return parts.join(' · ') || 'no matching rows';
  };

  return React.createElement('section', { id: 'drilldown', className: 'shell section', 'data-screen-label': '04 Drilldown' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 04 · Arbitrary axes'),
        React.createElement('div', { className: 'section-title' }, 'Drill into any two dimensions.'),
        React.createElement('div', { className: 'section-sub' }, 'Build a comparison, narrow the dataset, and choose how matching rows roll up into each cell.')
      )
    ),
    React.createElement('div', { className: 'drill-panel' },
      React.createElement('div', { className: 'drill-query' },
        React.createElement('div', { className: 'drill-query-top' },
          React.createElement(SectionMetricControl, { metric, setMetric }),
          React.createElement('label', { className: 'drill-query-field' },
            React.createElement('span', null, 'Cell value'),
            React.createElement('select', {
              className: 'knob',
              value: aggregation,
              onChange: event => setViewPatch({ aggregation: event.target.value }),
            },
              DRILL_AGGREGATIONS.map(agg => React.createElement('option', { key: agg.id, value: agg.id }, agg.label))
            )
          ),
          React.createElement(DrillSelect, {
            label: 'Rows',
            value: yAxis,
            onChange: axis => setViewPatch({ rows: [axis] }),
          }),
          React.createElement(DrillSelect, {
            label: 'Columns',
            value: xAxis,
            onChange: axis => setViewPatch({ columns: [axis] }),
          }),
          React.createElement('button', {
            className: 'drill-swap',
            type: 'button',
            onClick: () => setViewPatch({ rows: [xAxis], columns: [yAxis] }),
          }, 'swap')
        ),
        React.createElement('div', { className: 'drill-presets' },
          React.createElement('span', null, 'Presets'),
          DRILL_PRESETS.map(preset => React.createElement('button', {
            key: preset.label,
            type: 'button',
            onClick: () => applyPreset(preset),
          }, preset.label))
        ),
        React.createElement('div', { className: 'drill-filter-bar' },
          React.createElement('span', { className: 'drill-filter-bar-label' }, 'Filters'),
          activeFilters.map(([axis, filter]) => React.createElement('button', {
            key: axis,
            className: 'filter-chip',
            type: 'button',
            onClick: () => setActiveFilterAxis(axis),
          },
            React.createElement('span', null, drillFilterLabel(axis, filter)),
            React.createElement('em', {
              onClick: event => {
                event.stopPropagation();
                setFilterValues(axis, []);
                if (activeFilterAxis === axis) setActiveFilterAxis(null);
              },
            }, '×')
          )),
          React.createElement('select', {
            className: 'filter-add',
            value: '',
            onChange: event => {
              if (event.target.value) setActiveFilterAxis(event.target.value);
            },
          },
            React.createElement('option', { value: '' }, '+ Add filter'),
            addableAxes.map(axis => React.createElement('option', { key: axis.id, value: axis.id }, axis.label))
          )
        ),
        activeFilterAxis ? React.createElement(DrillFilterPopover, {
          axis: activeFilterAxis,
          records,
          filters,
          setFilterValues,
          onClose: () => setActiveFilterAxis(null),
        }) : null,
        React.createElement('div', { className: 'drill-active-filters' },
          `${DRILL_AXIS_BY_ID[yAxis].label} × ${DRILL_AXIS_BY_ID[xAxis].label} · ${aggInfo.label.toLowerCase()}${aggInfo.needsMetric ? ` ${metricInfo.short.toLowerCase()}` : ''}`
        )
      ),
      React.createElement('div', { className: 'drill-table-wrap' },
        React.createElement('table', { className: 'drill-table' },
          React.createElement('thead', null,
            React.createElement('tr', null,
              React.createElement('th', { className: 'corner' },
                React.createElement('span', { className: 'axis-y' }, DRILL_AXIS_BY_ID[yAxis].label),
                ' / ',
                React.createElement('span', { className: 'axis-x' }, DRILL_AXIS_BY_ID[xAxis].label)
              ),
              xValues.map(value => React.createElement('th', { key: value }, drillValueLabel(xAxis, value)))
            )
          ),
          React.createElement('tbody', null,
            yValues.map(y => React.createElement('tr', { key: y },
              React.createElement('th', null, drillValueLabel(yAxis, y)),
              xValues.map(x => {
                const cell = cells.get(`${y}\0${x}`);
                const meta = cell && aggInfo.needsMetric
                  ? [
                      cell.total > 1 ? `${cell.total} rows` : null,
                      cell.failures ? `${cell.failures} fail${cell.failures === 1 ? '' : 's'}` : null,
                    ].filter(Boolean).join(' · ')
                  : '';
                return React.createElement('td', {
                  key: x,
                  className: cell ? `has-value ${cell.failures ? 'has-failure' : ''} ${cell.count ? '' : 'failure-only'}` : 'empty',
                  style: cell && cell.count ? cellStyle(cell.value) : {},
                  title: cell ? cellTitle(cell) : 'no matching rows',
                },
                  cell && cell.value != null && isFinite(cell.value) ? React.createElement(React.Fragment, null,
                    React.createElement('span', { className: 'cell-main' }, formatDrillCellValue(cell.value, aggregation, aggInfo)),
                    meta ? React.createElement('span', { className: 'cell-meta' }, meta) : null
                  ) : cell && cell.failures ? React.createElement(React.Fragment, null,
                    React.createElement('span', { className: 'fail-label' }, 'fail'),
                    meta ? React.createElement('span', { className: 'cell-meta' }, meta) : null
                  ) : '—'
                );
              })
            ))
          )
        )
      ),
      React.createElement('div', { className: 'drill-legend' },
        React.createElement('span', null, min == null ? '— min' : `${Bench.fmtNum(min)} min`),
        React.createElement('span', { className: 'legend-ramp' }),
        React.createElement('span', null, max == null ? '— max' : `${Bench.fmtNum(max)} max`),
        React.createElement('span', null, `${filtered.length.toLocaleString()} rows · ${failureRows.toLocaleString()} failed · cell = ${aggInfo.label.toLowerCase()}`)
      )
    )
  );
}

function Comparator({ profileA, profileB, setProfileA, setProfileB, metric, setMetric }) {
  const cmp = useMemo(() =>
    Bench.compareProfiles(Bench.D.rows, profileA, profileB, metric),
    [profileA, profileB, metric]
  );
  const tieBand = Bench.tieBandForMetric(metric);
  const agg = useMemo(() => Bench.summarize(cmp, tieBand), [cmp, tieBand]);
  const profA = Bench.profileById(profileA);
  const profB = Bench.profileById(profileB);
  const compareTitle = `${Bench.profileLabel(profileB)} vs ${Bench.profileLabel(profileA)}.`;
  const unit = Bench.comparisonUnit(metric);

  const presets = [
    [SOL_LEGACY,       VYPER_GAS,       'Stable optimized'],
    [SOL_VIAIR,        VYPER_GAS_VENOM, 'New codegen'],
    [SOL_LEGACY,       SOL_VIAIR,       'solc backend switch'],
    [VYPER_GAS,        VYPER_GAS_VENOM, 'Vyper backend switch'],
    [SOL_0426_LEGACY,  SOL_LEGACY,      'solc version drift'],
    [VYPER_0310_GAS,   VYPER_GAS,       'Vyper version drift'],
  ];

  const totalBuilt = (profA?.successful_artifacts ?? 0) + (profB?.successful_artifacts ?? 0);
  const totalFail  = (profA?.failed_artifacts ?? 0) + (profB?.failed_artifacts ?? 0);

  return React.createElement('section', { id: 'compare', className: 'shell section', 'data-screen-label': '04 Compare' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 03 · Pick any two configurations'),
        React.createElement('div', { className: 'section-title' }, compareTitle),
        React.createElement('div', { className: 'section-sub' }, `Comparisons match on ${unit.match} - different compilers, identical surface. Negative deltas favor the compared profile.`),
      ),
      React.createElement(SectionMetricControl, { metric, setMetric })
    ),

    React.createElement('div', { className: 'compare-bar' },
      React.createElement(ProfilePicker, { title: 'Baseline (A)', selected: profileA, onChange: setProfileA }),
      React.createElement('div', { className: 'compare-vs' }, 'VS'),
      React.createElement(ProfilePicker, { title: 'Compared (B)', selected: profileB, onChange: setProfileB }),
    ),

    React.createElement('div', { className: 'presets' },
      presets.map(([a,b,label]) => React.createElement('button', {
        key: label, className: 'preset',
        title: `${Bench.profileLabel(a)} ↔ ${Bench.profileLabel(b)}`,
        onClick: () => { setProfileA(a); setProfileB(b); },
      }, label))
    ),

    // Big stat tiles
    React.createElement('div', { className: 'stat-row' },
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Geomean Δ'),
        React.createElement('div', { className: `v ${agg.geomean == null ? 'tie' : (agg.geomean < 1 - tieBand ? 'good' : agg.geomean > 1 + tieBand ? 'bad' : 'tie')}` },
          Bench.fmtDelta(agg.geomean)),
        React.createElement('div', { className: 'sub' }, `${Bench.profileLabel(profileB)} vs ${Bench.profileLabel(profileA)}`)
      ),
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Median Δ'),
        React.createElement('div', { className: 'v tie' }, Bench.fmtDelta(agg.median)),
        React.createElement('div', { className: 'sub' }, 'of ' + agg.count + ' comparable ' + unit.plural)
      ),
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Win / Tie / Loss'),
        React.createElement('div', { className: 'v tie tabular wtl-counts' },
          React.createElement('span', { style: { color: 'var(--accent)' } }, agg.cheaper),
          React.createElement('span', { style: { color: 'var(--fg-4)' } }, ' · ' + agg.tie + ' · '),
          React.createElement('span', { style: { color: 'var(--bad)' } }, agg.costlier),
        ),
        React.createElement('div', { className: 'wtl-bar' },
          React.createElement('div', { className: 'w', style: { width: (agg.cheaper / Math.max(1, agg.count) * 100) + '%' } }),
          React.createElement('div', { className: 't', style: { width: (agg.tie / Math.max(1, agg.count) * 100) + '%' } }),
          React.createElement('div', { className: 'l', style: { width: (agg.costlier / Math.max(1, agg.count) * 100) + '%' } }),
        )
      ),
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Compile OK'),
        React.createElement('div', { className: 'v tie tabular' }, totalBuilt + '/' + (totalBuilt + totalFail)),
        React.createElement('div', { className: 'sub' }, totalFail === 0 ? 'no failures' : `${totalFail} fail${totalFail===1?'':'s'} across both`)
      ),
    ),

    // Distribution + suite breakdown
    React.createElement('div', { className: 'compare-detail-grid' },
      React.createElement('div', { className: 'card no-pad' },
        React.createElement('div', { style: { padding: '20px 24px 4px 24px' } },
          React.createElement('div', { className: 'card-head' },
            React.createElement('div', { className: 'card-title' }, `Distribution of ${unit.singular} deltas`),
            React.createElement('div', { className: 'card-sub' }, `${agg.count} ${unit.plural} · negative = compared is cheaper`)
          )
        ),
        React.createElement('div', { style: { padding: '0 24px 16px 24px' } },
          React.createElement(DeltaHistogram, { rows: cmp, height: 80 })
        ),
        React.createElement('div', { style: { padding: '14px 24px 24px 24px', maxHeight: '620px', overflow: 'auto', borderTop: '1px solid var(--line)' } },
          React.createElement('div', { className: 'card-head', style: { marginTop: '4px' } },
            React.createElement('div', { className: 'card-title' }, `Per-${unit.singular} Δ`),
            React.createElement('div', { className: 'card-sub' }, `top ${Math.min(120, cmp.length)} by |Δ|`)
          ),
          React.createElement(ScenarioDeltaChart, { rows: cmp, height: Math.min(2200, Math.max(200, cmp.length * 14 + 30)), limit: 120 })
        )
      ),
      React.createElement('div', { className: 'compare-detail-side' },
        React.createElement(BySuiteCard, { rows: cmp, tieBand }),
        React.createElement(MoversCard, { rows: cmp }),
      )
    )
  );
}

function BySuiteCard({ rows, tieBand }) {
  const split = Bench.bySuite(rows, tieBand);
  return React.createElement('div', { className: 'card' },
    React.createElement('div', { className: 'card-head' },
      React.createElement('div', { className: 'card-title' }, 'By suite'),
      React.createElement('div', { className: 'card-sub' }, 'within this comparison')
    ),
    React.createElement('table', { className: 'tbl' },
      React.createElement('thead', null,
        React.createElement('tr', null,
          React.createElement('th', null, 'Suite'),
          React.createElement('th', { style: { textAlign: 'right' } }, 'Δ geomean'),
          React.createElement('th', { style: { textAlign: 'right' } }, 'n'),
          React.createElement('th', { style: { textAlign: 'right' } }, 'W / T / L'),
        )
      ),
      React.createElement('tbody', null,
        split.map(s => {
          const tone = s.geomean == null ? 'tie' : s.geomean < 1 - tieBand ? 'good' : s.geomean > 1 + tieBand ? 'bad' : 'tie';
          return React.createElement('tr', { key: s.suite },
            React.createElement('td', null, SUITES[s.suite].label),
            React.createElement('td', { className: `num delta ${tone}` }, Bench.fmtDelta(s.geomean)),
            React.createElement('td', { className: 'num' }, s.count),
            React.createElement('td', { className: 'num' }, `${s.cheaper}/${s.tie}/${s.costlier}`),
          );
        })
      )
    )
  );
}

function MoversCard({ rows }) {
  const top = rows.filter(r => r.deltaPct < 0).slice(0, 5);
  const bot = rows.filter(r => r.deltaPct > 0).slice(0, 5);
  return React.createElement('div', { className: 'card' },
    React.createElement('div', { className: 'card-head' },
      React.createElement('div', { className: 'card-title' }, 'Top movers'),
      React.createElement('div', { className: 'card-sub' }, '5 wins / 5 regressions')
    ),
    React.createElement('table', { className: 'tbl' },
      React.createElement('tbody', null,
        top.map(r => React.createElement('tr', { key: 'w-' + r.key },
          React.createElement('td', { className: 'scenario' }, r.label),
          React.createElement('td', { className: 'delta good num' }, Bench.fmtPct(r.deltaPct)),
        )),
        React.createElement('tr', null, React.createElement('td', { colSpan: 2, style: { borderBottom: '1px dashed var(--line)', padding: '4px 0' } })),
        bot.map(r => React.createElement('tr', { key: 'l-' + r.key },
          React.createElement('td', { className: 'scenario' }, r.label),
          React.createElement('td', { className: 'delta bad num' }, Bench.fmtPct(r.deltaPct)),
        )),
      )
    )
  );
}

// ============================================================
// Reliability
// ============================================================
function InlineList({ items, max = 6, formatter = x => x }) {
  const [expanded, setExpanded] = useState(false);
  const shown = expanded ? items : items.slice(0, max);
  const rest = items.length - shown.length;
  return React.createElement(React.Fragment, null,
    shown.map((item, index) => React.createElement('span', { key: `${item}-${index}`, className: 'chip' }, formatter(item))),
    items.length > max ? React.createElement('button', {
      type: 'button',
      className: 'chip muted chip-toggle',
      'aria-expanded': expanded ? 'true' : 'false',
      onClick: () => setExpanded(value => !value),
    }, expanded ? 'show less' : `+${rest} more`) : null
  );
}

function ReliabilityPanel() {
  const groups = Bench.failureGroups();
  const compilerGroups = Bench.failureCompilerGroups();
  const cleanProfiles = Bench.D.profiles
    .filter(p => p.failed_artifacts === 0)
    .sort((a,b) => a.label.localeCompare(b.label));
  return React.createElement('div', { className: 'reliability-grid' },
    React.createElement('div', { className: 'card' },
      React.createElement('div', { className: 'card-head' },
        React.createElement('div', null,
          React.createElement('div', { className: 'card-title' }, 'Compile failure groups'),
          React.createElement('div', { className: 'card-sub' }, 'Grouped by compiler and shared failure reason; rows list the affected benchmarks.')
        ),
        React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: 11, color: 'var(--fg-3)' } },
          `${Bench.D.summary.failed_artifacts}/${Bench.D.summary.attempted_artifacts} failed`)
      ),
      React.createElement('div', { className: 'failure-groups' },
        groups.map(group => React.createElement('div', { key: `${group.compiler}-${group.reason}`, className: 'failure-group' },
          React.createElement('div', { className: 'failure-main' },
            React.createElement('div', { className: 'failure-reason' }, group.reason),
            React.createElement('div', { className: 'failure-meta' },
              `${group.compiler} · ${group.count} artifact${group.count === 1 ? '' : 's'} · ${group.suites.join(', ')}`
            )
          ),
          React.createElement('div', { className: 'failure-detail' },
            React.createElement('div', { className: 'failure-label' }, 'Failed benchmarks'),
            React.createElement('div', { className: 'chip-row' },
              React.createElement(InlineList, { items: group.tests, max: 6 })
            )
          ),
          group.values.length ? React.createElement('div', { className: 'failure-detail' },
            React.createElement('div', { className: 'failure-label' }, 'N values'),
            React.createElement('div', { className: 'chip-row' },
              React.createElement(InlineList, { items: group.values, max: 8 })
            )
          ) : null,
          React.createElement('div', { className: 'failure-detail' },
            React.createElement('div', { className: 'failure-label' }, 'Profiles'),
            React.createElement('div', { className: 'chip-row' },
              React.createElement(InlineList, {
                items: group.profiles,
                max: 8,
                formatter: Bench.profileCompactLabel,
              })
            )
          )
        ))
      )
    ),
    React.createElement('div', { className: 'card' },
      React.createElement('div', { className: 'card-head' },
        React.createElement('div', null,
          React.createElement('div', { className: 'card-title' }, 'By compiler'),
          React.createElement('div', { className: 'card-sub' }, `${cleanProfiles.length} profiles compile all artifacts.`)
        )
      ),
      React.createElement('div', { className: 'compiler-failures' },
        compilerGroups.map(group => React.createElement('div', { key: group.compiler, className: 'compiler-failure' },
          React.createElement('div', { className: 'compiler-failure-top' },
            React.createElement('div', { className: 'compiler-name' }, group.compiler),
            React.createElement('div', { className: 'compiler-count' }, `${group.count} fail${group.count === 1 ? '' : 's'}`)
          ),
          React.createElement('div', { className: 'failure-label' }, 'Reasons'),
          React.createElement('div', { className: 'chip-row' },
            React.createElement(InlineList, { items: group.reasons, max: 4 })
          ),
          React.createElement('div', { className: 'failure-label' }, 'Benchmarks'),
          React.createElement('div', { className: 'chip-row' },
            React.createElement(InlineList, { items: group.tests, max: 5 })
          )
        )),
        React.createElement('div', { className: 'clean-summary' },
          React.createElement('div', { className: 'failure-label' }, 'Clean profiles'),
          React.createElement('div', { className: 'chip-row' },
            React.createElement(InlineList, { items: cleanProfiles.map(p => p.id), max: 10, formatter: Bench.profileCompactLabel })
          )
        )
      )
    )
  );
}

// ============================================================
// Methodology
// ============================================================
function Methodology() {
  const fallbackNotes = [
    {
      tag: 'A',
      title: 'Foundry internal-call harness gas',
      body: 'Gas is measured via Foundry\'s internal-call harness. That isolates compiler-generated runtime costs from intrinsic and calldata overhead.'
    },
    {
      tag: 'B',
      title: 'Stripped runtime bytes',
      body: 'Bytecode comparisons use runtime bytecode with appended metadata stripped, so trailing CBOR doesn\'t skew code-size deltas.'
    },
    {
      tag: 'C',
      title: 'Idiomatic source comparison',
      body: 'Headline results compare fixed and scale-suite idiomatic high-level source for each language. Solidity storage packing and Vyper dispatch codegen count as language-native behavior; hand-written assembly and mechanically matched ports belong in diagnostic lanes.'
    },
    {
      tag: 'D',
      title: 'Metric-aware geomeans',
      body: 'Runtime gas is aggregated over matched headline scenarios. Current harness deployment gas is scenario/deployment-variant scoped; bytecode size and compile time are deduplicated per benchmark artifact before computing ratios.'
    },
    {
      tag: 'E',
      title: 'Metric-specific bands',
      body: 'Gas and bytecode use a +/-0.5% materiality band for W/T/L counts. Compile time uses a +/-2% noise band.'
    },
    {
      tag: 'F',
      title: 'Real-derived provenance',
      body: 'Real-derived suites separate benchmark lanes from source lanes. Production-conformance rows use latest-syntax originals plus counterpart-language ports; pinned historical sources remain provenance references, not compiled headline artifacts.'
    },
    {
      tag: 'G',
      title: 'Trimmed diagnostic rows',
      body: 'Malformed calldata, decoder-boundary, admin/auth reject, and other adversarial revert rows are excluded from the measured scenario corpus so real-derived results focus on common workflow paths.'
    },
    {
      tag: 'H',
      title: 'Compatibility source variants',
      body: 'Older source-language profiles compile generated variants of the checked-in latest source. Version pragmas are rewritten to the resolved compiler patch range, then only supported backward syntax rewrites are applied.'
    },
    {
      tag: 'I',
      title: 'Cross-profile behavior checks',
      body: 'Gas rows persist return-data, observer-state, and normalized-log hashes. Report rows compare those hashes against the language baseline profile when both profiles compiled the same scenario; expected-revert rows are compared by status and observer state, not raw revert bytes.'
    },
    {
      tag: 'J',
      title: 'Vyper Venom and 0.5.0a1',
      body: 'Vyper "Venom" rows pass --experimental-codegen. Vyper 0.5.0a1 is pre-release.'
    },
  ];
  const methods = Array.isArray(Bench.D.methodology?.notes) && Bench.D.methodology.notes.length
    ? Bench.D.methodology.notes
    : fallbackNotes;
  return React.createElement('div', { className: 'methods' },
    methods.map(m => React.createElement('div', { key: m.tag, className: 'method' },
      React.createElement('div', { className: 'nm' }, `Note ${m.tag}`),
      React.createElement('div', { className: 'ttl' }, m.title),
      React.createElement('div', { className: 'body' }, m.body),
    ))
  );
}

function RealDerivedProvenance() {
  const models = Bench.D.real_derived_models || [];
  if (!models.length) return null;
  const laneLabel = value => ({
    latest_syntax_original: 'latest syntax',
    latest_idiomatic: 'idiomatic',
    fixture_scoped_port: 'idiomatic',
    production_conformance: 'prod conformance',
  }[value] || value || 'n/a');
  return React.createElement('div', { className: 'card' },
    React.createElement('div', { className: 'card-head' },
        React.createElement('div', null,
          React.createElement('div', { className: 'card-title' }, 'Real-derived source lanes'),
          React.createElement('div', { className: 'card-sub' }, 'Pinned upstream files are reference inputs; compiled rows use generated source variants.')
      )
    ),
    React.createElement('table', { className: 'tbl source-lanes-table' },
      React.createElement('thead', null,
        React.createElement('tr', null,
          React.createElement('th', null, 'Benchmark'),
          React.createElement('th', null, 'Source'),
          React.createElement('th', null, 'Port'),
          React.createElement('th', null, 'Sources')
        )
      ),
      React.createElement('tbody', null,
        models.map(model => {
          const p = model.provenance || {};
          const compiledSources = model.compiled_sources || [];
          const compiledTitle = compiledSources.length
            ? compiledSources
              .map(s => `${s.profile_id || 'profile'} · ${s.source_variant || 'latest'} · ${s.source_path || 'n/a'} · ${s.source_hash || ''}`)
              .join('\n')
            : 'n/a';
          const compiledLabel = compiledSources.length
            ? `${compiledSources.length} source${compiledSources.length === 1 ? '' : 's'}`
            : 'n/a';
          return React.createElement('tr', { key: model.benchmark_id },
            React.createElement('td', { className: 'scenario' }, model.benchmark_id),
            React.createElement('td', null, laneLabel(p.source_lane)),
            React.createElement('td', null, laneLabel(p.counterpart_lane)),
            React.createElement('td', { className: 'path-cell', title: compiledTitle }, compiledLabel)
          );
        })
      )
    )
  );
}

function CompilerConfigurations() {
  const compilerMeta = (language, modes) => {
    const profiles = Bench.D.profiles.filter(p => p.language === language);
    const versions = new Set(profiles.map(p => p.compiler_version || Bench.profileVersionLabel(p)));
    return {
      profiles: profiles.length,
      versions: versions.size,
      modes: modes.length,
      venom: profiles.filter(p => p.experimental_codegen).length,
    };
  };
  const compilerConfigs = [
    {
      key: 'solidity',
      compiler: 'Solidity',
      engine: 'solc',
      axis: 'Codegen axis · optimizer-runs axis',
      meta: compilerMeta('solidity', ['noopt', 'legacy', 'viaIR']),
      modes: [
        ['noopt', '--no-optimize', CONFIG_EXPLAINERS['solidity:noopt']],
        ['legacy', '--optimize --optimize-runs N', CONFIG_EXPLAINERS['solidity:legacy']],
        ['viaIR', '--via-ir --optimize-runs N', CONFIG_EXPLAINERS['solidity:viaIR']],
      ],
    },
    {
      key: 'vyper',
      compiler: 'Vyper',
      engine: '',
      axis: 'Optimizer axis',
      meta: compilerMeta('vyper', ['none', 'gas', 'codesize']),
      modes: [
        ['none', 'no optimizer', CONFIG_EXPLAINERS['vyper:none']],
        ['gas', '--optimize gas', CONFIG_EXPLAINERS['vyper:gas']],
        ['codesize', '--optimize codesize', CONFIG_EXPLAINERS['vyper:codesize']],
      ],
      independent: ['Venom', '--experimental-codegen', CONFIG_EXPLAINERS.venom],
    },
  ];
  return React.createElement('div', { className: 'config-glossary' },
    React.createElement('div', { className: 'compiler-config-grid' },
      compilerConfigs.map(group => React.createElement('div', { key: group.key, className: `compiler-config ${group.key}` },
        React.createElement('div', { className: 'compiler-config-head' },
          React.createElement('div', null,
            React.createElement('div', { className: 'config-label' }, 'Compiler'),
            React.createElement('div', { className: 'compiler-name' },
              React.createElement('span', { className: `lang-${group.key === 'solidity' ? 'sol' : 'vy'}` }, group.compiler),
              group.engine ? React.createElement(React.Fragment, null, ' · ', group.engine) : null,
            )
          ),
          React.createElement('div', { className: 'config-count' },
            `${group.meta.versions} versions · ${group.meta.modes} modes · ${group.meta.profiles} profiles`
          )
        ),
        React.createElement('div', { className: 'axis-row' },
          React.createElement('span', null, group.axis),
        ),
        React.createElement('div', { className: 'mode-grid' },
          group.modes.map(([name, flag, body]) => React.createElement('div', { key: name, className: 'config-mode' },
            React.createElement('div', { className: 'mode-top' },
              React.createElement('span', { className: 'mode-name' }, name),
              React.createElement('span', { className: 'mode-flag' }, flag),
            ),
            React.createElement('div', { className: 'body' }, body),
          ))
        ),
        group.independent ? React.createElement('div', { className: 'independent-switch' },
          React.createElement('div', { className: 'axis-row' },
            React.createElement('span', null, 'Codegen backend'),
          ),
          React.createElement('div', { className: 'switch-card' },
            React.createElement('div', { className: 'mode-top' },
              React.createElement('span', { className: 'mode-name' }, group.independent[0]),
              React.createElement('span', { className: 'mode-flag accent' }, group.independent[1]),
            ),
            React.createElement('div', { className: 'body' }, group.independent[2]),
            React.createElement('div', { className: 'switch-formula' },
              React.createElement('span', null, 'none | gas | codesize'),
              React.createElement('span', null, '×'),
              React.createElement('span', null, 'venom: off | on'),
              React.createElement('span', null, '='),
              React.createElement('span', null, '6 configs / version'),
            )
          )
        ) : null,
      ))
    )
  );
}

function SectionVersions({ metric, setMetric }) {
  return React.createElement('section', { id: 'versions', className: 'shell section' },
      React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 02 · Versions over time'),
        React.createElement('div', { className: 'section-title' }, 'Compiler versions.'),
        React.createElement('div', { className: 'section-sub' }, 'Each point is the geomean delta vs. the newest comparable profile. Lines near zero indicate small version-to-version changes. For chart continuity, Vyper 0.2 default is grouped with none because modern optimize modes did not exist yet.')
      ),
      React.createElement(SectionMetricControl, { metric, setMetric })
    ),
    React.createElement(VersionEvolution, { metric })
  );
}

function SectionScale({ metric, setMetric }) {
  return React.createElement('section', { id: 'scale', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 05 · Cost vs. shape of the contract'),
        React.createElement('div', { className: 'section-title' }, 'How the metric scales with structural N.'),
        React.createElement('div', { className: 'section-sub' }, 'In dispatch_N, Vyper selector dispatch stays nearly flat as function count grows, while solc viaIR rises with the selector surface. The other panels show how storage, ABI, loop, event, and external-call shapes scale.')
      ),
      React.createElement(SectionMetricControl, { metric, setMetric })
    ),
    React.createElement(ScaleStrip, { metric })
  );
}

function SectionReliability() {
  return React.createElement('section', { id: 'reliability', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 06 · Reliability'),
        React.createElement('div', { className: 'section-title' }, 'Compile failures are first-class data.'),
        React.createElement('div', { className: 'section-sub' }, 'Profile comparisons include both successful artifacts and the benchmark shapes each compiler failed to build. Tracked here per profile.')
      )
    ),
    React.createElement(ReliabilityPanel)
  );
}

function SectionMethodology() {
  const source = window.__EVM_BENCH_DATA_SOURCE || "./report-model.json";
  const published = !!window.__EVM_BENCH_PUBLISH_MANIFEST;
  const dataRoot = published || source.startsWith("/") ? "/" : "../normalized/";
  const rawRoot = published || source.startsWith("/") ? "/" : "../raw/";
  return React.createElement('section', { id: 'methodology', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 08 · How to read this'),
        React.createElement('div', { className: 'section-title' }, 'Methodology and caveats.'),
        React.createElement('div', { className: 'section-sub' }, 'A compact reference for units, aggregation, and scope behind the report numbers.')
      )
    ),
    React.createElement(Methodology),
    React.createElement(RealDerivedProvenance),
    React.createElement('div', { className: 'raw-links-row' },
      React.createElement('div', { className: 'raw-links' },
        React.createElement('a', { href: source }, 'report-model.json'),
        React.createElement('a', { href: `${dataRoot}results.json` }, 'results.json'),
        React.createElement('a', { href: `${dataRoot}run-manifest.json` }, 'run-manifest.json'),
        React.createElement('a', { href: `${rawRoot}foundry-gas.jsonl` }, 'foundry-gas.jsonl'),
      ),
      React.createElement('a', {
        href: 'https://github.com/banteg/evm-compiler-bench',
        rel: 'noreferrer',
        target: '_blank',
      }, 'banteg/evm-compiler-bench'),
    )
  );
}

function SectionCompilerConfigurations() {
  return React.createElement('section', { id: 'configs', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 07 · Compiler configurations'),
        React.createElement('div', { className: 'section-title' }, 'Compiler configurations.'),
        React.createElement('div', { className: 'section-sub' },
          React.createElement('p', null, 'A profile is a compiler version paired with exactly one codegen or optimizer mode. Vyper adds Venom as an independent codegen switch.'),
          React.createElement('p', null, "Optimization is not just a performance choice: Solidity's Yul/viaIR path and Vyper's experimental Venom pipeline have both had correctness bugs. Treat faster profiles as performance evidence, not automatic production guidance; pair them with version pinning, differential tests, and IR/bytecode review.")
        )
      )
    ),
    React.createElement(CompilerConfigurations)
  );
}

// ============================================================
// Root
// ============================================================
function App() {
  const def = Bench.D.defaults;
  const defaultA = Bench.profileById(SOL_LEGACY) ? SOL_LEGACY : def.baseline_profile;
  const defaultB = Bench.profileById(VYPER_GAS) ? VYPER_GAS : def.comparison_profile;
  const [profileA, setProfileA] = useState(defaultA);
  const [profileB, setProfileB] = useState(defaultB);
  const [metric, setMetric] = useState(def.primary_metric || 'harness_call_gas');
  return React.createElement(React.Fragment, null,
    React.createElement(TopBar),
    React.createElement(Hero),
    React.createElement(FindingsGrid),
    React.createElement(Comparator, { profileA, profileB, setProfileA, setProfileB, metric, setMetric }),
    React.createElement(DrilldownMatrix, { metric, setMetric }),
    React.createElement(SectionVersions, { metric, setMetric }),
    React.createElement(SectionScale, { metric, setMetric }),
    React.createElement(SectionReliability),
    React.createElement(SectionCompilerConfigurations),
    React.createElement(SectionMethodology),
  );
}

const root = createRoot(document.getElementById('root'));
root.render(React.createElement(App));

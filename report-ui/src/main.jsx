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
const SOL_VIAIR_HIGH = 'solc-latest-viair-runs10000';
const SOL_NOOPT = 'solc-latest-noopt';
const SOL_0426_LEGACY = 'solc-0.4.26-legacy-runs200';
const VYPER_0310_GAS = 'vyper-0.3.10-gas';
const VYPER_GAS = 'vyper-latest-gas';
const VYPER_GAS_VENOM = 'vyper-latest-gas-venom';

// Pre-compute headline stories ONCE
function buildHeadlines() {
  const rows = Bench.D.rows;
  const M = 'harness_call_gas';
  const S = 'runtime_bytes_stripped';

  const v = (a, b, metric) => {
    const cmp = Bench.compareProfiles(rows, a, b, metric);
    return { ...Bench.summarize(cmp), cmp };
  };

  return {
    stableSolVsVyperGas: v(SOL_VIAIR_HIGH, VYPER_GAS, M),
    stableSolVsVyperSize: v(SOL_VIAIR_HIGH, VYPER_GAS, S),
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
  const solcViaIRHigh = Bench.profileLabel(SOL_VIAIR_HIGH);
  const vyperGas = Bench.profileLabel(VYPER_GAS);
  const vyperVenom = Bench.profileLabel(VYPER_GAS_VENOM);
  const absDelta = ratio => `${Math.abs((ratio - 1) * 100).toFixed(1)}%`;
  const directionWord = ratio => ratio < 1 ? 'lower' : 'higher';

  return React.createElement('section', { className: 'shell hero' },
    React.createElement('div', { className: 'hero-eyebrow' },
      React.createElement('span', { className: 'dot' }),
      `Compiler bench · v${Bench.D.schema_version} · ${gen.toISOString().slice(0,10)} · ${s.profiles} profiles × ${s.benchmarks} benchmarks`
    ),
    React.createElement('h1', { className: 'hero-title' },
      'The ',
      React.createElement('em', null, 'definitive'),
      ' EVM compiler benchmark.'
    ),
    React.createElement('p', { className: 'hero-lede' },
      'Across ',
      React.createElement('strong', null, s.ok_rows.toLocaleString()),
      ` successful scenario measurements, ${vyperGas} is ${absDelta(HEADLINES.stableSolVsVyperGas.geomean)} ${directionWord(HEADLINES.stableSolVsVyperGas.geomean)} runtime gas than ${solcViaIRHigh}. Against ${solcViaIR}, ${vyperVenom} is ${absDelta(HEADLINES.solVsVyperVenomGas.geomean)} ${directionWord(HEADLINES.solVsVyperVenomGas.geomean)} gas and ${absDelta(HEADLINES.solVsVyperVenomSize.geomean)} ${directionWord(HEADLINES.solVsVyperVenomSize.geomean) === 'lower' ? 'smaller' : 'larger'} runtime bytecode. Explore the comprehensive breakdown below.`
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
  const solcViaIRHigh = Bench.profileLabel(SOL_VIAIR_HIGH);
  const solcNoopt = Bench.profileLabel(SOL_NOOPT);
  const vyperGas = Bench.profileLabel(VYPER_GAS);
  const vyperVenom = Bench.profileLabel(VYPER_GAS_VENOM);
  const cards = [
    {
      tag: 'Finding 01',
      span: 4,
      headline: 'Vyper gas vs solc viaIR at high optimization.',
      body: `Comparing each compiler at its strongest stable optimizer mode — Vyper ${vyperGas} against solc ${solcViaIRHigh} (via_ir + optimizer_runs = 10,000) — over comparable scenarios.`,
      stat: HEADLINES.stableSolVsVyperGas.geomean,
      statLabel: 'runtime gas (Vyper gas vs solc viaIR runs=10000)',
      altStat: HEADLINES.stableSolVsVyperSize.geomean,
      altLabel: 'runtime bytes',
      count: HEADLINES.stableSolVsVyperGas.count,
    },
    {
      tag: 'Finding 02',
      span: 4,
      headline: 'Vyper + Venom beats solc viaIR on both axes.',
      body: 'Usually, you trade bytecode size for gas savings. Against solc 0.8.35 viaIR, Vyper 0.4.3 with Venom codegen achieves both significant gas savings and a smaller footprint.',
      stat: HEADLINES.solVsVyperVenomGas.geomean,
      statLabel: 'runtime gas (Vyper Venom vs solc viaIR)',
      altStat: HEADLINES.solVsVyperVenomSize.geomean,
      altLabel: 'runtime bytes',
      count: HEADLINES.solVsVyperVenomGas.count,
    },
    {
      tag: 'Finding 03',
      span: 4,
      headline: 'Venom makes Vyper smaller and cheaper.',
      body: 'Enabling --experimental-codegen ("Venom") in Vyper shrinks runtime bytecode, reduces runtime gas, and even cuts down compile times compared to legacy codegen.',
      stat: HEADLINES.venomSize.geomean,
      statLabel: 'runtime bytes vs Vyper legacy codegen',
      altStat: HEADLINES.venomGas.geomean,
      altLabel: 'runtime gas',
      count: HEADLINES.venomSize.count,
    },
    {
      tag: 'Finding 04',
      span: 4,
      headline: 'viaIR buys gas and size with compile time.',
      body: 'Switching from solc 0.8.35 legacy to viaIR yields modest reductions in runtime gas and bytecode size, but comes with a massive 158% increase in compile time.',
      stat: HEADLINES.viaIRGas.geomean,
      statLabel: 'runtime gas vs solc legacy',
      altStat: HEADLINES.viaIRCompile.geomean,
      altLabel: 'compile wall time',
      altInvert: true,
      count: HEADLINES.viaIRGas.count,
    },
    {
      tag: 'Finding 05',
      span: 4,
      headline: 'Seven years of solc legacy barely move runtime gas.',
      body: 'Solc 0.4.26 was released in 2019. Compiling on the same legacy pipeline up to solc 0.8.35 results in a negligible 0.3% difference in runtime gas.',
      stat: HEADLINES.solEra.geomean,
      statLabel: 'runtime gas (solc 0.4.26 → 0.8.35 legacy)',
      neutral: true,
      count: HEADLINES.solEra.count,
    },
    {
      tag: 'Finding 06',
      span: 4,
      headline: 'The optimizer is the biggest lever in the run.',
      body: 'Disabling the optimizer entirely in solc 0.8.35 triggers a massive 34.9% penalty to runtime gas: the single largest measured effect among all solc profile comparisons.',
      stat: HEADLINES.nooptGas.geomean,
      statLabel: 'runtime gas without optimizer',
      altStat: HEADLINES.nooptSize.geomean,
      altLabel: 'runtime bytes without optimizer',
      count: HEADLINES.nooptGas.count,
    },
  ];

  return React.createElement('section', { id: 'findings', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 01 · Summary'),
        React.createElement('div', { className: 'section-title' }, 'Six findings from this run.'),
        React.createElement('div', { className: 'section-sub' }, 'Each card reports a geometric-mean delta over comparable scenarios; card headers show the row count.')
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
            React.createElement('span', null, `n=${c.count}`)
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
  'solidity:legacy': 'Solidity legacy EVM codegen with optimizer runs=200.',
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
  const facets = Bench.profileFacets(knobs.language, knobs.versionKey);
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
    onChange(Bench.resolveProfile({ ...knobs, versionKey, optimizer, experimental: false }));
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
        onChange: optimizer => choose({ optimizer }),
      }),
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
        React.createElement('div', { className: 'section-sub' }, 'Comparisons match on suite/benchmark/scenario/state — different compilers, identical surface. Negative deltas favor the compared profile.'),
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
        React.createElement('div', { className: 'sub' }, 'of ' + agg.count + ' comparable scenarios')
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
            React.createElement('div', { className: 'card-title' }, 'Distribution of scenario deltas'),
            React.createElement('div', { className: 'card-sub' }, `${agg.count} scenarios · negative = compared is cheaper`)
          )
        ),
        React.createElement('div', { style: { padding: '0 24px 16px 24px' } },
          React.createElement(DeltaHistogram, { rows: cmp, height: 80 })
        ),
        React.createElement('div', { style: { padding: '14px 24px 24px 24px', maxHeight: '620px', overflow: 'auto', borderTop: '1px solid var(--line)' } },
          React.createElement('div', { className: 'card-head', style: { marginTop: '4px' } },
            React.createElement('div', { className: 'card-title' }, 'Per-scenario Δ'),
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
  const methods = [
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
      title: 'Geomean over comparable scenarios',
      body: 'Each summary is a geometric mean of ratios B/A over scenarios where both profiles compiled. Missing scenarios are excluded from the comparison.'
    },
    {
      tag: 'D',
      title: 'Metric-specific bands',
      body: 'Gas and bytecode use a ±0.5% materiality band for W/T/L counts. Compile time uses a ±2% noise band.'
    },
    {
      tag: 'E',
      title: 'Real-derived ≠ production',
      body: 'Curve, Uniswap V2, and Yearn V3 ports are clean-room behavior models with production_equivalence=false. They exercise realistic shapes, not real ERC20 transfer paths.'
    },
    {
      tag: 'F',
      title: 'Vyper Venom and 0.5.0a1',
      body: 'Vyper "Venom" rows pass --experimental-codegen. Vyper 0.5.0a1 is pre-release.'
    },
  ];
  return React.createElement('div', { className: 'methods' },
    methods.map(m => React.createElement('div', { key: m.tag, className: 'method' },
      React.createElement('div', { className: 'nm' }, `Note ${m.tag}`),
      React.createElement('div', { className: 'ttl' }, m.title),
      React.createElement('div', { className: 'body' }, m.body),
    ))
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
      axis: 'Codegen axis',
      meta: compilerMeta('solidity', ['noopt', 'legacy', 'viaIR']),
      modes: [
        ['noopt', '--no-optimize', CONFIG_EXPLAINERS['solidity:noopt']],
        ['legacy', '--optimize=200', CONFIG_EXPLAINERS['solidity:legacy']],
        ['viaIR', '--via-ir', CONFIG_EXPLAINERS['solidity:viaIR']],
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
        React.createElement('div', { className: 'section-eyebrow' }, '§ 04 · Cost vs. shape of the contract'),
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
        React.createElement('div', { className: 'section-eyebrow' }, '§ 05 · Reliability'),
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
        React.createElement('div', { className: 'section-eyebrow' }, '§ 07 · How to read this'),
        React.createElement('div', { className: 'section-title' }, 'Methodology and caveats.'),
        React.createElement('div', { className: 'section-sub' }, 'A compact reference for units, aggregation, and scope behind the report numbers.')
      )
    ),
    React.createElement(Methodology),
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
        React.createElement('div', { className: 'section-eyebrow' }, '§ 06 · Compiler configurations'),
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
    React.createElement(SectionVersions, { metric, setMetric }),
    React.createElement(SectionScale, { metric, setMetric }),
    React.createElement(SectionReliability),
    React.createElement(SectionCompilerConfigurations),
    React.createElement(SectionMethodology),
  );
}

const root = createRoot(document.getElementById('root'));
root.render(React.createElement(App));

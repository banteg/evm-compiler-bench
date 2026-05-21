import React, { useState, useMemo } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import "./bench-data.js";
import "./bench-charts.jsx";

const Bench = window.Bench;
const BenchCharts = window.BenchCharts;
const { ScenarioDeltaChart, EvolutionChart, ScaleChart, DeltaHistogram } = BenchCharts;

// ============================================================
// Constants & headline finding helpers
// ============================================================

const SUITES = Bench.SUITES;
const METRICS = Bench.METRICS;

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
    solVsVyperGas:   v('solc-latest-viair-runs200', 'vyper-latest-gas', M),
    solVsVyperSize:  v('solc-latest-viair-runs200', 'vyper-latest-gas', S),
    solVsVyperVenomGas: v('solc-latest-viair-runs200', 'vyper-latest-gas-venom', M),
    venomGas:        v('vyper-latest-gas', 'vyper-latest-gas-venom', M),
    venomSize:       v('vyper-latest-gas', 'vyper-latest-gas-venom', S),
    viaIRGas:        v('solc-latest-legacy-runs200', 'solc-latest-viair-runs200', M),
    viaIRSize:       v('solc-latest-legacy-runs200', 'solc-latest-viair-runs200', S),
    viaIRCompile:    v('solc-latest-legacy-runs200', 'solc-latest-viair-runs200', 'compile_wall_ms'),
    solEra:          v('solc-0.4.26-legacy-runs200', 'solc-latest-legacy-runs200', M),
    nooptGas:        v('solc-latest-noopt', 'solc-latest-viair-runs200', M),
    nooptSize:       v('solc-latest-noopt', 'solc-latest-viair-runs200', S),
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
        React.createElement('a', { href: '#suites' }, 'Suites'),
        React.createElement('a', { href: '#versions' }, 'Versions'),
        React.createElement('a', { href: '#compare' }, 'Compare'),
        React.createElement('a', { href: '#scale' }, 'Scale'),
        React.createElement('a', { href: '#methodology' }, 'Methods'),
      ),
      React.createElement('div', { className: 'right' },
        React.createElement('span', null,
          React.createElement('span', { className: 'pulse' }),
          (Bench.D.manifest?.evm_version || '').toUpperCase() + ' EVM',
        ),
        React.createElement('span', { title: Bench.D.manifest?.run_id || '' }, 'run ' + (Bench.D.manifest?.run_id || '').slice(0, 8)),
      )
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

  return React.createElement('section', { className: 'shell hero' },
    React.createElement('div', { className: 'hero-eyebrow' },
      React.createElement('span', { className: 'dot' }),
      `Compiler bench · v${Bench.D.schema_version} · ${gen.toISOString().slice(0,10)} · ${s.profiles} profiles × ${s.benchmarks} benchmarks`
    ),
    React.createElement('h1', { className: 'hero-title' },
      'Compiler tradeoffs, ',
      React.createElement('em', null, 'not'),
      ' a language leaderboard.'
    ),
    React.createElement('p', { className: 'hero-lede' },
      'Seven thousand head-to-head measurements across ',
      React.createElement('strong', null, s.profiles),
      ' compiler configurations and ',
      React.createElement('strong', null, s.benchmarks),
      ' benchmarks. Same scenario, same state, same harness — only the compiler changes. Numbers below; methodology at the end.'
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
  const cards = [
    {
      tag: 'Finding 01',
      span: 4,
      headline: 'Vyper Venom is the clearest codegen win in this run.',
      body: 'Switching latest Vyper from the legacy codegen to ' +
            '--experimental-codegen ("Venom") makes contracts smaller, slightly cheaper, and faster to compile.',
      stat: HEADLINES.venomSize.geomean,
      statLabel: 'runtime bytes vs legacy codegen',
      altStat: HEADLINES.venomGas.geomean,
      altLabel: 'runtime gas',
      count: HEADLINES.venomSize.count,
    },
    {
      tag: 'Finding 02',
      span: 4,
      headline: 'solc viaIR buys ~3% gas — at 2.5× the compile time.',
      body: 'Compared head-to-head against the legacy pipeline at the same version, viaIR ships smaller, slightly cheaper bytecode but takes much longer to produce it.',
      stat: HEADLINES.viaIRGas.geomean,
      statLabel: 'runtime gas vs legacy',
      altStat: HEADLINES.viaIRCompile.geomean,
      altLabel: 'compile wall time',
      altInvert: true,
      count: HEADLINES.viaIRGas.count,
    },
    {
      tag: 'Finding 03',
      span: 4,
      headline: 'Version-to-version, the gas needle barely moves.',
      body: 'Comparing solc 0.4 to latest on the same legacy pipeline yields essentially zero geomean change in runtime gas. The big jumps are codegen mode, not version.',
      stat: HEADLINES.solEra.geomean,
      statLabel: 'gas: solc 0.4 → latest (legacy)',
      neutral: true,
      count: HEADLINES.solEra.count,
    },
    {
      tag: 'Finding 04',
      span: 4,
      headline: 'Don\'t ship without an optimizer.',
      body: 'Comparing solc with --no-optimizer to latest viaIR is a worst-vs-best snapshot: a quarter of the gas, and contracts shrink to less than half the runtime size. The optimizer is doing the work.',
      stat: HEADLINES.nooptGas.geomean,
      statLabel: 'runtime gas',
      altStat: HEADLINES.nooptSize.geomean,
      altLabel: 'runtime bytes',
      count: HEADLINES.nooptGas.count,
    },
    {
      tag: 'Finding 05',
      span: 4,
      headline: 'Solidity vs Vyper at their best: a real tradeoff, not a winner.',
      body: 'Latest solc viaIR vs latest Vyper gas: Vyper runs cheaper on the median scenario but produces measurably larger bytecode. Picking is a deployment-cost vs. runtime-cost question.',
      stat: HEADLINES.solVsVyperGas.geomean,
      statLabel: 'gas (Vyper vs solc)',
      altStat: HEADLINES.solVsVyperSize.geomean,
      altLabel: 'bytes (Vyper vs solc)',
      altInvert: false,
      count: HEADLINES.solVsVyperGas.count,
    },
    {
      tag: 'Finding 06',
      span: 4,
      headline: 'Vyper Venom is a stronger harness-gas challenger to solc viaIR.',
      body: 'Comparing latest solc viaIR to latest Vyper gas with Venom enabled gives the clean cross-compiler headline for runtime gas. Missing Venom compile rows are excluded, not counted as wins.',
      stat: HEADLINES.solVsVyperVenomGas.geomean,
      statLabel: 'harness gas (Vyper Venom vs solc viaIR)',
      count: HEADLINES.solVsVyperVenomGas.count,
    },
  ];

  return React.createElement('section', { id: 'findings', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 01 · The answer at a glance'),
        React.createElement('div', { className: 'section-title' }, 'Six findings from the run.'),
        React.createElement('div', { className: 'section-sub' }, 'Each headline is a geomean delta across the entire comparable scenario surface. Hover any number for the underlying sample size.')
      ),
      React.createElement('div', { className: 'section-meta' }, 'Metric · Harness call gas + runtime bytes')
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
  const bySuite = useMemo(() => Bench.bySuite(rows), [rows]);

  return React.createElement('div', { className: 'suite-grid' },
    bySuite.map((s, i) => {
      const pct = s.geomean == null ? null : (s.geomean - 1) * 100;
      const tone = pct == null ? 'tie' : pct < -2 ? 'good' : pct > 2 ? 'bad' : 'tie';
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
        React.createElement('div', { className: 'evo-axis' }, 'Δ vs latest · same codegen')
      ),
      React.createElement(EvolutionChart, { points, language: 'solidity', height: 240 })
    ),
    React.createElement('div', { className: 'evo-side' },
      React.createElement('div', { className: 'evo-head' },
        React.createElement('div', { className: 'evo-title' },
          React.createElement('span', { className: 'lang-vy' }, 'Vyper')
        ),
        React.createElement('div', { className: 'evo-axis' }, 'Δ vs latest · same optimize')
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
      React.createElement('select', {
        className: 'knob', value: knobs.language,
        onChange: e => chooseLang(e.target.value),
      },
        React.createElement('option', { value: 'solidity' }, 'Solidity'),
        React.createElement('option', { value: 'vyper' }, 'Vyper'),
      ),
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
        options: facets.optimizers.map(o => ({ value: o, label: o })),
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
              title: venomAvailable ? '' : 'No Venom build for this version/config',
            },
          ],
          onChange: experimental => choose({ experimental: experimental === 'on' }),
        }),
      ) : null,
    ),
    React.createElement('div', { style: { marginTop: '12px', fontFamily: 'var(--mono)', fontSize: '10.5px', color: 'var(--fg-4)' } },
      React.createElement('code', null, p.id))
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

function Comparator({ profileA, profileB, setProfileA, setProfileB }) {
  const def = Bench.D.defaults;
  const [metric, setMetric] = useState(def.primary_metric || 'harness_call_gas');
  const [suiteFilter, setSuiteFilter] = useState(new Set(['fixed','scale','real_derived']));

  const cmp = useMemo(() =>
    Bench.compareProfiles(Bench.D.rows, profileA, profileB, metric, suiteFilter),
    [profileA, profileB, metric, suiteFilter]
  );
  const agg = useMemo(() => Bench.summarize(cmp), [cmp]);
  const profA = Bench.profileById(profileA);
  const profB = Bench.profileById(profileB);

  const presets = [
    ['solc-latest-viair-runs200', 'vyper-latest-gas',         'solc viaIR ↔ Vyper gas'],
    ['solc-latest-viair-runs200', 'vyper-latest-gas-venom',   'solc viaIR ↔ Vyper Venom'],
    ['solc-latest-legacy-runs200','solc-latest-viair-runs200','solc legacy ↔ viaIR'],
    ['vyper-latest-gas',          'vyper-latest-gas-venom',   'Vyper: legacy ↔ Venom'],
    ['solc-0.4.26-legacy-runs200','solc-latest-legacy-runs200','solc 0.4 → latest'],
    ['vyper-0.2.16-default',      'vyper-latest-gas',         'Vyper 0.2 → latest'],
  ];

  const totalBuilt = (profA?.successful_artifacts ?? 0) + (profB?.successful_artifacts ?? 0);
  const totalFail  = (profA?.failed_artifacts ?? 0) + (profB?.failed_artifacts ?? 0);

  return React.createElement('section', { id: 'compare', className: 'shell section', 'data-screen-label': '04 Compare' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 04 · Pick any two configurations'),
        React.createElement('div', { className: 'section-title' }, 'Head-to-head, scenario by scenario.'),
        React.createElement('div', { className: 'section-sub' }, 'Comparisons match on suite/benchmark/scenario/state — different compilers, identical surface. Negative deltas favor the compared profile.'),
      ),
      React.createElement('div', { className: 'section-meta' }, `tie band ±${(Bench.D.defaults.tie_band*100).toFixed(0)}%`)
    ),

    React.createElement('div', { className: 'compare-bar' },
      React.createElement(ProfilePicker, { title: 'Baseline (A)', selected: profileA, onChange: setProfileA }),
      React.createElement('div', { className: 'compare-vs' }, 'VS'),
      React.createElement(ProfilePicker, { title: 'Compared (B)', selected: profileB, onChange: setProfileB }),
      React.createElement('div', { className: 'compare-meta' },
        React.createElement('div', null,
          React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-4)', marginBottom: '8px' } }, 'Metric'),
          React.createElement(MetricToggle, { value: metric, onChange: setMetric })
        ),
        React.createElement('div', null,
          React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-4)', marginBottom: '8px' } }, 'Suites'),
          React.createElement('div', { className: 'toggle' },
            Object.keys(SUITES).map(s => React.createElement('button', {
              key: s,
              className: suiteFilter.has(s) ? 'on' : '',
              onClick: () => {
                const next = new Set(suiteFilter);
                if (next.has(s) && next.size > 1) next.delete(s);
                else next.add(s);
                setSuiteFilter(next);
              },
            }, SUITES[s].label))
          )
        )
      ),
    ),

    React.createElement('div', { className: 'presets' },
      presets.map(([a,b,label]) => React.createElement('button', {
        key: label, className: 'preset',
        onClick: () => { setProfileA(a); setProfileB(b); },
      }, label))
    ),

    // Big stat tiles
    React.createElement('div', { className: 'stat-row' },
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Geomean Δ'),
        React.createElement('div', { className: `v ${agg.geomean == null ? 'tie' : (agg.geomean < 0.98 ? 'good' : agg.geomean > 1.02 ? 'bad' : 'tie')}` },
          Bench.fmtDelta(agg.geomean)),
        React.createElement('div', { className: 'sub' }, `${profB?.label || profileB} vs ${profA?.label || profileA}`)
      ),
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Median Δ'),
        React.createElement('div', { className: 'v tie' }, Bench.fmtDelta(agg.median)),
        React.createElement('div', { className: 'sub' }, 'of ' + agg.count + ' comparable scenarios')
      ),
      React.createElement('div', { className: 'stat' },
        React.createElement('div', { className: 'k' }, 'Win / Tie / Loss'),
        React.createElement('div', { className: 'v tie tabular' },
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
    React.createElement('div', { style: { marginTop: '24px', display: 'grid', gridTemplateColumns: '1.4fr 1fr', gap: '20px' } },
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
      React.createElement('div', { style: { display: 'flex', flexDirection: 'column', gap: '20px' } },
        React.createElement(BySuiteCard, { rows: cmp }),
        React.createElement(MoversCard, { rows: cmp }),
      )
    )
  );
}

function BySuiteCard({ rows }) {
  const split = Bench.bySuite(rows);
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
          const tone = s.geomean == null ? 'tie' : s.geomean < 0.98 ? 'good' : s.geomean > 1.02 ? 'bad' : 'tie';
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
  const shown = items.slice(0, max);
  const rest = items.length - shown.length;
  return React.createElement(React.Fragment, null,
    shown.map(item => React.createElement('span', { key: item, className: 'chip' }, formatter(item))),
    rest > 0 ? React.createElement('span', { className: 'chip muted' }, `+${rest} more`) : null
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
      body: 'Gas is measured via Foundry\'s internal-call harness, not signed transaction gas. That isolates compiler-generated code costs from intrinsic and calldata overhead.'
    },
    {
      tag: 'B',
      title: 'Stripped runtime bytes',
      body: 'Bytecode comparisons use runtime bytecode with appended metadata stripped, so trailing CBOR doesn\'t skew code-size deltas.'
    },
    {
      tag: 'C',
      title: 'Geomean over comparable scenarios',
      body: 'Each summary is a geometric mean of ratios B/A over scenarios where both profiles compiled. Missing scenarios are excluded — they\'re not zero.'
    },
    {
      tag: 'D',
      title: 'Tie band ±2%',
      body: 'A 2% band around 1.0× is treated as a tie. The window covers measurement jitter and within-noise differences across versions.'
    },
    {
      tag: 'E',
      title: 'Real-derived ≠ production',
      body: 'Curve, Uniswap V2, and Yearn V3 ports are clean-room behavior models with production_equivalence=false. They exercise realistic shapes, not real ERC20 transfer paths.'
    },
    {
      tag: 'F',
      title: 'Vyper Venom and 0.5.0a1',
      body: 'Vyper "Venom" rows pass --experimental-codegen. Vyper 0.5.0a1 is pre-release. Both are reported alongside stable lines; neither is filtered out.'
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

// ============================================================
// Section with metric/suite local control + scorecards + version chart
// ============================================================
function SectionSuites({ profileA, profileB }) {
  const [metric, setMetric] = useState('harness_call_gas');
  const profA = Bench.profileById(profileA);
  const profB = Bench.profileById(profileB);
  return React.createElement('section', { id: 'suites', className: 'shell section', 'data-screen-label': '02 Suites' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 02 · Where the deltas live'),
        React.createElement('div', { className: 'section-title' }, 'How the comparison splits by suite.'),
        React.createElement('div', { className: 'section-sub' }, 'Fixed suite is hand-written ports; Scale exercises N=1..64 along seven axes; Real-derived models large production contracts. The bars below each card show win / tie / loss inside that suite.')
      ),
      React.createElement('div', { style: { display: 'flex', gap: '14px', alignItems: 'center' } },
        React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-4)' } }, 'metric'),
        React.createElement(MetricToggle, { value: metric, onChange: setMetric }),
      )
    ),
    React.createElement('div', { style: { display: 'flex', gap: '12px', marginBottom: '20px', fontFamily: 'var(--mono)', fontSize: '11px', color: 'var(--fg-3)', letterSpacing: '0.06em', alignItems: 'center', flexWrap: 'wrap' } },
      React.createElement('span', null, 'Showing ',
        React.createElement('span', { style: { color: 'var(--fg)' } }, profB?.label || profileB),
        ' vs ',
        React.createElement('span', { style: { color: 'var(--fg)' } }, profA?.label || profileA),
      ),
      React.createElement('span', { style: { color: 'var(--fg-4)' } }, '·'),
      React.createElement('a', { href: '#compare', style: { color: 'var(--fg-3)' } }, 'change in comparator ↓')
    ),
    React.createElement(SuiteScorecards, { profileA, profileB, metric })
  );
}

function SectionVersions() {
  const [metric, setMetric] = useState('harness_call_gas');
  return React.createElement('section', { id: 'versions', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 03 · Versions over time'),
        React.createElement('div', { className: 'section-title' }, 'Compiler versions, normalized to "latest, same config".'),
        React.createElement('div', { className: 'section-sub' }, 'Each point is the geomean delta vs. the latest profile sharing the same language and config. Lines near zero mean the version moved the needle very little. Codegen-mode switches (viaIR, Venom) live in different lines.')
      ),
      React.createElement('div', { style: { display: 'flex', gap: '14px', alignItems: 'center' } },
        React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-4)' } }, 'metric'),
        React.createElement(MetricToggle, { value: metric, onChange: setMetric }),
      )
    ),
    React.createElement(VersionEvolution, { metric })
  );
}

function SectionScale() {
  const [metric, setMetric] = useState('harness_call_gas');
  return React.createElement('section', { id: 'scale', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 05 · Cost vs. shape of the contract'),
        React.createElement('div', { className: 'section-title' }, 'How the metric scales with structural N.'),
        React.createElement('div', { className: 'section-sub' }, 'Each panel plots a structural axis (function count, storage slots, mapping depth, etc.) against the selected metric on a log-x axis. Compilers diverge most under unusual shape, not common ones.')
      ),
      React.createElement('div', { style: { display: 'flex', gap: '14px', alignItems: 'center' } },
        React.createElement('div', { style: { fontFamily: 'var(--mono)', fontSize: '10px', letterSpacing: '0.14em', textTransform: 'uppercase', color: 'var(--fg-4)' } }, 'metric'),
        React.createElement(MetricToggle, { value: metric, onChange: setMetric }),
      )
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
        React.createElement('div', { className: 'section-sub' }, 'A profile that drops scenarios isn\'t a faster profile — it\'s a less complete one. Tracked here per profile.')
      ),
      React.createElement('div', { className: 'section-meta' }, `${Bench.D.summary.compile_failures} compile failures · ${(Bench.D.summary.failed_artifacts/Bench.D.summary.attempted_artifacts*100).toFixed(2)}%`)
    ),
    React.createElement(ReliabilityPanel)
  );
}

function SectionMethodology() {
  return React.createElement('section', { id: 'methodology', className: 'shell section' },
    React.createElement('div', { className: 'section-head' },
      React.createElement('div', null,
        React.createElement('div', { className: 'section-eyebrow' }, '§ 07 · How to read this'),
        React.createElement('div', { className: 'section-title' }, 'Methodology and caveats.'),
        React.createElement('div', { className: 'section-sub' }, 'Six notes that change how you read every number above. Read them.')
      ),
      React.createElement('div', { className: 'section-meta' }, 'plus links to raw data ↓')
    ),
    React.createElement(Methodology),
    React.createElement('div', { style: { marginTop: '28px', display: 'flex', gap: '24px', flexWrap: 'wrap', fontFamily: 'var(--mono)', fontSize: '12px', color: 'var(--fg-2)' } },
      React.createElement('a', { href: './report-model.json' }, 'report-model.json'),
      React.createElement('a', { href: '../normalized/results.json' }, 'results.json'),
      React.createElement('a', { href: '../normalized/run-manifest.json' }, 'run-manifest.json'),
      React.createElement('a', { href: '../raw/foundry-gas.jsonl' }, 'foundry-gas.jsonl'),
    )
  );
}

// ============================================================
// Footer
// ============================================================
function Footer() {
  const m = Bench.D.manifest;
  return React.createElement('div', { className: 'shell' },
    React.createElement('div', { className: 'foot' },
      React.createElement('div', null, `EVM Compiler Bench · run ${m?.run_id || '—'}`),
      React.createElement('div', null, `${m?.environment?.tools?.forge?.split('\n')[0] || ''}`),
      React.createElement('div', null, `commit ${(m?.environment?.git?.commit || '').slice(0, 10)}${m?.environment?.git?.dirty ? ' · dirty' : ''}`),
    )
  );
}

// ============================================================
// Root
// ============================================================
function App() {
  const def = Bench.D.defaults;
  const [profileA, setProfileA] = useState(def.baseline_profile);
  const [profileB, setProfileB] = useState(def.comparison_profile);
  return React.createElement(React.Fragment, null,
    React.createElement(TopBar),
    React.createElement(Hero),
    React.createElement(FindingsGrid),
    React.createElement(SectionSuites, { profileA, profileB }),
    React.createElement(SectionVersions),
    React.createElement(Comparator, { profileA, profileB, setProfileA, setProfileB }),
    React.createElement(SectionScale),
    React.createElement(SectionReliability),
    React.createElement(SectionMethodology),
    React.createElement(Footer),
  );
}

const root = createRoot(document.getElementById('root'));
root.render(React.createElement(App));

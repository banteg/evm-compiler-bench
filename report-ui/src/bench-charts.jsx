import React from "react";

// SVG chart primitives — hand-rolled, fits the data-app aesthetic.
(function(){
  const Bench = window.Bench;
  const { useState, useRef, useLayoutEffect, useMemo } = React;

  // === useContainerWidth — observe container width for responsive SVGs ===
  function useContainerWidth(fallback = 800){
    const ref = useRef(null);
    const [w, setW] = useState(fallback);
    useLayoutEffect(() => {
      if (!ref.current) return;
      const obs = new ResizeObserver(entries => {
        for (const e of entries) setW(Math.max(200, Math.floor(e.contentRect.width)));
      });
      obs.observe(ref.current);
      setW(Math.max(200, Math.floor(ref.current.getBoundingClientRect().width)));
      return () => obs.disconnect();
    }, []);
    return [ref, w];
  }

  // === ScenarioDeltaChart — horizontal bar chart of per-scenario delta% ===
  // rows: [{label, suite, deltaPct}], limit: max bars
  function ScenarioDeltaChart({ rows, height = 360, limit = 80 }){
    const [ref, width] = useContainerWidth(900);
    const data = rows.slice(0, limit);
    if (!data.length) return React.createElement('div', { className: 'placeholder' }, 'No comparable scenarios.');

    const padL = 280, padR = 60, padT = 16, padB = 24;
    const innerW = Math.max(120, width - padL - padR);
    const rowH = Math.max(8, Math.min(18, (height - padT - padB) / data.length));
    const h = Math.max(120, padT + padB + rowH * data.length);

    const maxAbs = Math.max(1, ...data.map(d => Math.abs(d.deltaPct)));
    const xZero = padL + innerW / 2;
    const xScale = v => (v / maxAbs) * (innerW / 2);
    const niceMax = Math.ceil(maxAbs / 5) * 5;

    // Tick marks
    const ticks = [];
    const step = niceMax <= 10 ? 2 : niceMax <= 25 ? 5 : niceMax <= 50 ? 10 : 25;
    for (let v = -niceMax; v <= niceMax; v += step) ticks.push(v);

    return React.createElement('div', { ref, style: { width: '100%' } },
      React.createElement('svg', { width, height: h, viewBox: `0 0 ${width} ${h}` },
        // grid
        ticks.map((t, i) => React.createElement('line', {
          key: 'g'+i,
          x1: xZero + xScale(t), x2: xZero + xScale(t),
          y1: padT, y2: h - padB,
          stroke: t === 0 ? 'var(--fg-4)' : 'var(--line)',
          strokeDasharray: t === 0 ? '' : '2 4',
        })),
        // tick labels
        ticks.map((t, i) => React.createElement('text', {
          key: 't'+i,
          x: xZero + xScale(t), y: h - padB + 14,
          fontSize: 9.5, fontFamily: 'var(--mono)', letterSpacing: '0.06em',
          fill: 'var(--fg-4)', textAnchor: 'middle',
        }, (t > 0 ? '+' : '') + t + '%')),
        // bars
        data.map((d, i) => {
          const y = padT + i * rowH + 1;
          const bw = Math.abs(xScale(d.deltaPct));
          const xs = d.deltaPct >= 0 ? xZero : xZero - bw;
          const tone = d.deltaPct < -2 ? 'var(--accent)' : d.deltaPct > 2 ? 'var(--bad)' : 'var(--fg-4)';
          return React.createElement('g', { key: 'b'+i },
            // label
            React.createElement('text', {
              x: padL - 8, y: y + rowH - 4,
              fontSize: 10.5, fontFamily: 'var(--mono)',
              fill: 'var(--fg-2)', textAnchor: 'end',
            }, d.label.length > 44 ? d.label.slice(0,42) + '…' : d.label),
            React.createElement('rect', {
              x: xs, y, width: Math.max(bw, 0.6), height: rowH - 2, fill: tone,
            }),
            React.createElement('title', {}, `${d.label}\n${d.deltaPct.toFixed(2)}%`)
          );
        })
      )
    );
  }

  // === EvolutionChart — small multiples line chart by version, faceted ===
  // points: [{version, versionKey, config, deltaPct, profile, label, comparable}]
  function EvolutionChart({ points, language, height = 240 }){
    const [ref, width] = useContainerWidth(540);
    const pts = points.filter(p => p.language === language && !p.venom);
    // Group by config
    const byConfig = new Map();
    for (const p of pts){
      if (!byConfig.has(p.config)) byConfig.set(p.config, []);
      byConfig.get(p.config).push(p);
    }
    // Stable version order
    const allVersions = [...new Set(pts.map(p => p.versionKey))]
      .sort((a,b) => Bench.versionRank(a) - Bench.versionRank(b));
    if (!allVersions.length) {
      return React.createElement('div', { ref, style: { padding: '32px 0', color: 'var(--fg-3)', fontFamily: 'var(--mono)', fontSize: 11 } }, 'No version data.');
    }
    const padL = 40, padR = 16, padT = 12, padB = 32;
    const innerW = Math.max(120, width - padL - padR);
    const innerH = height - padT - padB;
    const xOf = i => padL + (allVersions.length <= 1 ? innerW / 2 : (i / (allVersions.length - 1)) * innerW);
    const allDeltas = pts.map(p => p.deltaPct).concat([0]);
    const minY = Math.min(...allDeltas), maxY = Math.max(...allDeltas);
    const span = Math.max(2, maxY - minY);
    const yPad = span * 0.15;
    const y0 = minY - yPad, y1 = maxY + yPad;
    const yOf = v => padT + (1 - (v - y0) / (y1 - y0)) * innerH;
    const yZero = yOf(0);

    // Configs palette
    const configPalette = {
      'viaIR': 'var(--solidity)',
      'legacy': '#5a86d4',
      'noopt': '#3a5a8a',
      'gas': 'var(--vyper)',
      'codesize': '#d089d4',
      'none': '#7e5a82',
      'default': '#a07ea4',
    };

    const configs = [...byConfig.keys()].sort((a,b) => Bench.optimizerRank(b) - Bench.optimizerRank(a));

    // y ticks
    const yTicks = [];
    const tickStep = (y1 - y0) <= 4 ? 1 : (y1 - y0) <= 10 ? 2 : (y1 - y0) <= 25 ? 5 : 10;
    const startTick = Math.ceil(y0 / tickStep) * tickStep;
    for (let t = startTick; t <= y1; t += tickStep) yTicks.push(t);

    return React.createElement('div', { ref, style: { width: '100%' } },
      React.createElement('svg', { width, height, viewBox: `0 0 ${width} ${height}` },
        // grid
        yTicks.map((t, i) => React.createElement('g', { key: 'g'+i },
          React.createElement('line', {
            x1: padL, x2: width - padR, y1: yOf(t), y2: yOf(t),
            stroke: t === 0 ? 'var(--fg-4)' : 'var(--line)',
            strokeDasharray: t === 0 ? '' : '2 4',
          }),
          React.createElement('text', {
            x: padL - 6, y: yOf(t) + 3,
            fontSize: 9.5, fontFamily: 'var(--mono)',
            fill: 'var(--fg-4)', textAnchor: 'end',
          }, (t > 0 ? '+' : '') + t + '%')
        )),
        // zero strong
        React.createElement('line', {
          x1: padL, x2: width - padR, y1: yZero, y2: yZero,
          stroke: 'var(--fg-3)', strokeWidth: 1,
        }),
        // x labels
        allVersions.map((v, i) => React.createElement('text', {
          key: 'x'+i,
          x: xOf(i), y: height - padB + 14,
          fontSize: 9.5, fontFamily: 'var(--mono)',
          fill: 'var(--fg-3)', textAnchor: 'middle',
        }, v)),
        // lines + points by config
        configs.map(cfg => {
          const points = byConfig.get(cfg)
            .slice()
            .sort((a,b) => Bench.versionRank(a.versionKey) - Bench.versionRank(b.versionKey));
          const color = configPalette[cfg] || 'var(--fg-2)';
          const poly = points.map(p => {
            const i = allVersions.indexOf(p.versionKey);
            return `${xOf(i)},${yOf(p.deltaPct)}`;
          }).join(' ');
          return React.createElement('g', { key: cfg },
            React.createElement('polyline', {
              points: poly, fill: 'none', stroke: color, strokeWidth: 1.5,
              strokeLinejoin: 'round',
            }),
            points.map((p, k) => {
              const i = allVersions.indexOf(p.versionKey);
              return React.createElement('g', { key: 'p'+k },
                React.createElement('circle', {
                  cx: xOf(i), cy: yOf(p.deltaPct), r: 3.5,
                  fill: 'var(--bg)', stroke: color, strokeWidth: 1.5,
                }),
                React.createElement('title', {}, `${p.label}\n${p.deltaPct.toFixed(2)}% vs latest ${cfg}`)
              );
            })
          );
        })
      ),
      // Legend
      React.createElement('div', { style: {
        display: 'flex', flexWrap: 'wrap', gap: '12px', marginTop: '10px',
        fontFamily: 'var(--mono)', fontSize: '10.5px', color: 'var(--fg-3)',
      }},
        configs.map(cfg => React.createElement('div', {
          key: cfg,
          style: { display: 'flex', alignItems: 'center', gap: '6px', letterSpacing: '0.08em', textTransform: 'uppercase' }
        },
          React.createElement('span', { style: {
            width: 10, height: 2, background: configPalette[cfg] || 'var(--fg-2)', display: 'inline-block',
          }}),
          cfg
        ))
      )
    );
  }

  // ScaleChart: picks a canonical scenario per family to avoid mixing curves.
  const FAMILY_SCENARIO = {
    'abi_args_N': 'sum_args',
    'dispatch_N': 'last_selector',
    'events_N': 'emit_many',
    'external_calls_N': 'call_many',
    'loop_bound_N': 'run_loop',
    'mapping_depth_N': 'read_after_write',
    'storage_slots_N': 'write_all',
  };
  function ScaleChart({ family, metric, profiles, height = 180 }){
    const [ref, width] = useContainerWidth(360);
    const padL = 40, padR = 12, padT = 14, padB = 26;
    const innerW = Math.max(80, width - padL - padR);
    const innerH = height - padT - padB;

    // Filter rows by canonical scenario only, no aggregation needed
    const scenario = FAMILY_SCENARIO[family];
    const rows = Bench.D.rows.filter(r =>
      r.family === family && r.status === 'ok' &&
      (!scenario || r.gas?.scenario === scenario)
    );
    const series = new Map();
    for (const r of rows){
      if (!profiles.includes(r.profile_id)) continue;
      const v = Bench.valueAt(r, metric);
      if (v == null) continue;
      if (!series.has(r.profile_id)) series.set(r.profile_id, []);
      series.get(r.profile_id).push({ x: r.parameter_value, y: v });
    }
    for (const arr of series.values()) arr.sort((a,b)=>a.x-b.x);

    const allX = [...new Set(rows.map(r => r.parameter_value))].sort((a,b)=>a-b);
    const allY = [...series.values()].flat().map(p => p.y);
    if (!allY.length) return React.createElement('div', { ref, style: { color: 'var(--fg-4)', fontFamily: 'var(--mono)', fontSize: 11 } }, 'no data');

    const xMin = Math.min(...allX), xMax = Math.max(...allX);
    const yMin = 0, yMax = Math.max(...allY) * 1.06;
    // log-x for scale (1..64)
    const xOf = v => padL + (Math.log(v) - Math.log(xMin)) / (Math.log(xMax) - Math.log(xMin)) * innerW;
    const yOf = v => padT + (1 - (v - yMin) / (yMax - yMin)) * innerH;

    const palette = {
      'solc-latest-viair-runs200': 'var(--solidity)',
      'solc-latest-legacy-runs200': '#5a86d4',
      'solc-latest-noopt': '#3a5a8a',
      'vyper-latest-gas': 'var(--vyper)',
      'vyper-latest-gas-venom': 'var(--accent)',
      'vyper-latest-codesize': '#d089d4',
      'vyper-latest-none': '#7e5a82',
    };
    return React.createElement('div', { ref, style: { width: '100%' } },
      React.createElement('svg', { width, height, viewBox: `0 0 ${width} ${height}` },
        // y ticks (2)
        [0.5, 1].map((f, i) => {
          const yv = yMin + (yMax - yMin) * f;
          return React.createElement('g', { key: 'yt'+i },
            React.createElement('line', { x1: padL, x2: width - padR, y1: yOf(yv), y2: yOf(yv), stroke: 'var(--line)', strokeDasharray: '2 4' }),
            React.createElement('text', { x: padL - 4, y: yOf(yv) + 3, fontSize: 9, fontFamily: 'var(--mono)', fill: 'var(--fg-4)', textAnchor: 'end' },
              yv >= 1000 ? `${(yv/1000).toFixed(0)}k` : Math.round(yv))
          );
        }),
        // x ticks at 1, 8, 64
        [1, 8, 64].filter(v => allX.includes(v)).map(v =>
          React.createElement('text', {
            key: 'xt'+v, x: xOf(v), y: height - padB + 12,
            fontSize: 9, fontFamily: 'var(--mono)', fill: 'var(--fg-4)', textAnchor: 'middle',
          }, v)
        ),
        // lines
        [...series.entries()].map(([pid, arr]) => {
          const color = palette[pid] || 'var(--fg-2)';
          const poly = arr.map(p => `${xOf(p.x)},${yOf(p.y)}`).join(' ');
          return React.createElement('g', { key: pid },
            React.createElement('polyline', { points: poly, fill: 'none', stroke: color, strokeWidth: 1.5 }),
            arr.map((p, j) => React.createElement('circle', {
              key: 'p'+j, cx: xOf(p.x), cy: yOf(p.y), r: 2.5,
              fill: color,
            }))
          );
        })
      )
    );
  }

  // === DeltaHistogram — distribution of deltas as a small histogram ===
  function DeltaHistogram({ rows, height = 70 }){
    const [ref, width] = useContainerWidth(400);
    if (!rows.length) return React.createElement('div', { ref });
    const padL = 0, padR = 0, padT = 4, padB = 16;
    const innerW = Math.max(80, width - padL - padR);
    const innerH = height - padT - padB;
    const maxAbs = Math.max(5, ...rows.map(r => Math.abs(r.deltaPct)));
    const niceMax = Math.ceil(maxAbs / 5) * 5;
    const bins = 31;
    const counts = new Array(bins).fill(0);
    for (const r of rows){
      const b = Math.max(0, Math.min(bins - 1, Math.floor(((r.deltaPct + niceMax) / (2 * niceMax)) * bins)));
      counts[b]++;
    }
    const maxC = Math.max(1, ...counts);
    const bw = innerW / bins;
    return React.createElement('div', { ref, style: { width: '100%' } },
      React.createElement('svg', { width, height, viewBox: `0 0 ${width} ${height}` },
        // zero line
        React.createElement('line', {
          x1: padL + innerW / 2, x2: padL + innerW / 2,
          y1: padT, y2: padT + innerH,
          stroke: 'var(--fg-4)', strokeDasharray: '2 3',
        }),
        counts.map((c, i) => {
          const center = -niceMax + (i + 0.5) / bins * 2 * niceMax;
          const tone = center < -2 ? 'var(--accent)' : center > 2 ? 'var(--bad)' : 'var(--fg-4)';
          const bh = (c / maxC) * innerH;
          return React.createElement('rect', {
            key: i,
            x: padL + i * bw + 0.5,
            y: padT + innerH - bh,
            width: Math.max(1, bw - 1),
            height: bh,
            fill: tone,
            opacity: c === 0 ? 0.2 : 0.85,
          });
        }),
        // axis labels
        React.createElement('text', { x: padL, y: height - 3, fontSize: 9, fontFamily: 'var(--mono)', fill: 'var(--fg-4)' }, `−${niceMax}%`),
        React.createElement('text', { x: padL + innerW, y: height - 3, fontSize: 9, fontFamily: 'var(--mono)', fill: 'var(--fg-4)', textAnchor: 'end' }, `+${niceMax}%`),
        React.createElement('text', { x: padL + innerW / 2, y: height - 3, fontSize: 9, fontFamily: 'var(--mono)', fill: 'var(--fg-4)', textAnchor: 'middle' }, '0')
      )
    );
  }

  window.BenchCharts = { ScenarioDeltaChart, EvolutionChart, ScaleChart, DeltaHistogram, FAMILY_SCENARIO };
})();

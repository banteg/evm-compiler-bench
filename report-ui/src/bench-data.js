// Data helpers — all globals on `window.Bench`
(function(){
  const D = window.__BENCH_DATA || window.__EVM_BENCH_REPORT_DATA;

  const METRICS = [
    { id: 'harness_call_gas',       label: 'Harness call gas', short: 'Runtime gas',  unit: 'gas',  lowerBetter: true, hero: true },
    { id: 'runtime_bytes_stripped', label: 'Runtime bytes',    short: 'Code size',    unit: 'B',    lowerBetter: true },
    { id: 'internal_create_gas',    label: 'Internal create gas', short: 'Deploy gas', unit: 'gas', lowerBetter: true },
    { id: 'compile_wall_ms',        label: 'Compile wall time',short: 'Compile time', unit: 'ms',   lowerBetter: true },
  ];

  const SUITES = {
    fixed:         { label: 'Fixed',        desc: 'Hand-written ports of common contract motifs' },
    scale:         { label: 'Scale',        desc: 'Parametric N=1..64 scaling families' },
    real_derived:  { label: 'Real-derived', desc: 'Simplified models of production contracts' },
  };

  function median(values){
    if (!values || !values.length) return undefined;
    const s = [...values].sort((a,b)=>a-b);
    const m = Math.floor(s.length/2);
    return s.length % 2 ? s[m] : (s[m-1]+s[m])/2;
  }

  function valueAt(row, metric){
    if (row.status !== 'ok') return undefined;
    switch(metric){
      case 'harness_call_gas': return row.gas?.harness_call_gas;
      case 'runtime_bytes_stripped': return row.bytecode?.runtime_bytes_stripped ?? row.bytecode?.runtime_bytes;
      case 'internal_create_gas': return row.gas?.internal_create_gas;
      case 'compile_wall_ms': return median(row.compile?.wall_ms_samples);
      case 'peak_rss_kib': return row.compile?.peak_rss_kib;
    }
  }
  function scenarioKey(r){
    return [r.suite, r.benchmark_id, r.gas?.scenario ?? 'artifact', r.gas?.state_access_profile ?? 'artifact', r.parameter_value ?? ''].join('|');
  }
  function scenarioLabel(r){
    const sc = r.gas?.scenario ?? 'artifact';
    const st = r.gas?.state_access_profile ?? 'artifact';
    const n  = r.parameter_value == null ? '' : ` N=${r.parameter_value}`;
    return `${r.benchmark_id}${n} · ${sc} · ${st}`;
  }

  function compareProfiles(rows, pa, pb, metric, suiteSet){
    const L = new Map(), Ra = new Map(), Rb = new Map();
    for (const r of rows){
      if (suiteSet && !suiteSet.has(r.suite)) continue;
      const v = valueAt(r, metric);
      if (v == null) continue;
      const k = scenarioKey(r);
      if (r.profile_id === pa){ Ra.set(k, r); L.set(k, v); }
      else if (r.profile_id === pb){ Rb.set(k, r); }
    }
    const out = [];
    for (const [k, va] of L){
      const rb = Rb.get(k);
      if (!rb) continue;
      const vb = valueAt(rb, metric);
      if (!vb || vb <= 0 || va <= 0) continue;
      const ratio = vb / va;
      out.push({
        key: k,
        row: Ra.get(k),
        rowB: rb,
        label: scenarioLabel(Ra.get(k)),
        suite: Ra.get(k).suite,
        valueA: va,
        valueB: vb,
        ratio,
        deltaPct: (ratio - 1) * 100,
      });
    }
    return out.sort((a,b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct));
  }

  function summarize(rows, tieBand = 0.02){
    const ratios = rows.map(r => r.ratio).filter(r => r > 0);
    const geomean = ratios.length ? Math.exp(ratios.reduce((s,r)=>s+Math.log(r),0)/ratios.length) : null;
    let cheaper=0, tie=0, costlier=0;
    for (const r of rows){
      if (r.ratio < 1 - tieBand) cheaper++;
      else if (r.ratio > 1 + tieBand) costlier++;
      else tie++;
    }
    const median = (() => {
      if (!ratios.length) return null;
      const s = [...ratios].sort((a,b)=>a-b);
      const m = Math.floor(s.length/2);
      return s.length % 2 ? s[m] : (s[m-1]+s[m])/2;
    })();
    return { geomean, median, cheaper, tie, costlier, count: rows.length };
  }

  function bySuite(rows, tieBand){
    const g = new Map();
    for (const r of rows){
      if (!g.has(r.suite)) g.set(r.suite, []);
      g.get(r.suite).push(r);
    }
    return ['fixed','scale','real_derived'].map(s => ({ suite: s, ...summarize(g.get(s) || [], tieBand) }));
  }

  // Profile metadata helpers
  function profileById(id){ return D.profiles.find(p => p.id === id); }
  function profileLabel(id){
    const p = profileById(id);
    return p?.label ?? id;
  }
  function profileVersionKey(p){
    const prefix = p.language === 'solidity' ? 'solc-latest-' : 'vyper-latest-';
    if (String(p.id).startsWith(prefix)) return 'latest';
    return String(p.compiler_version ?? 'unknown');
  }
  function profileVersionLabel(p){
    const key = profileVersionKey(p);
    if (key === 'latest') return `latest (${p.compiler_version})`;
    return key;
  }
  function versionRank(v){
    if (v === 'latest') return Infinity;
    const m = v.match(/^(\d+)\.(\d+)\.(\d+)(?:a(\d+))?/);
    if (!m) return -1;
    const [, ma, mi, pa, al] = m;
    return Number(ma)*1e9 + Number(mi)*1e6 + Number(pa)*1e3 + (al==null?999:Number(al));
  }
  function profileOptimizer(p){
    const id = String(p.id);
    if (p.language === 'solidity'){
      if (id.includes('viair-runs200')) return 'viaIR';
      if (id.includes('legacy-runs200')) return 'legacy';
      if (id.includes('noopt')) return 'noopt';
    }
    if (p.language === 'vyper'){
      if (id.includes('codesize')) return 'codesize';
      if (id.includes('gas')) return 'gas';
      if (id.includes('none')) return 'none';
      if (id.includes('default')) return 'default';
    }
    return 'default';
  }
  function profileKnobs(p){
    return {
      language: p.language,
      versionKey: profileVersionKey(p),
      optimizer: profileOptimizer(p),
      experimental: !!p.experimental_codegen,
    };
  }
  function matchingProfiles(desired){
    return D.profiles.filter(p => {
      const k = profileKnobs(p);
      return (!desired.language || k.language === desired.language)
        && (!desired.versionKey || k.versionKey === desired.versionKey)
        && (!desired.optimizer || k.optimizer === desired.optimizer)
        && (desired.experimental == null || k.experimental === desired.experimental);
    });
  }
  function preferredOptimizer(lang, optimizers){
    const preferred = lang === 'solidity'
      ? ['viaIR','legacy','noopt']
      : ['gas','codesize','none','default'];
    return preferred.find(o => optimizers.includes(o)) ?? optimizers[0];
  }
  function resolveProfile(desired){
    const cands = matchingProfiles({ language: desired.language });
    const exact = cands.find(p => {
      const k = profileKnobs(p);
      return k.versionKey === desired.versionKey
          && k.optimizer === desired.optimizer
          && k.experimental === desired.experimental;
    });
    if (exact) return exact.id;
    const fallback1 = cands.find(p => {
      const k = profileKnobs(p);
      return k.versionKey === desired.versionKey && k.optimizer === desired.optimizer;
    });
    if (fallback1) return fallback1.id;
    const fallback2 = cands.find(p => profileVersionKey(p) === desired.versionKey);
    if (fallback2) return fallback2.id;
    return cands[0]?.id ?? D.profiles[0].id;
  }
  function defaultProfileForLanguage(lang){
    const pref = lang === 'solidity' ? 'solc-latest-viair-runs200' : 'vyper-latest-gas';
    if (D.profiles.some(p => p.id === pref)) return pref;
    return D.profiles.find(p => p.language === lang)?.id ?? D.profiles[0].id;
  }

  function latestBaselineProfile(p){
    const config = profileOptimizer(p);
    const venom = p.experimental_codegen ? '-venom' : '';
    if (p.language === 'solidity'){
      if (config === 'viaIR') return 'solc-latest-viair-runs200';
      if (config === 'legacy') return 'solc-latest-legacy-runs200';
      if (config === 'noopt') return 'solc-latest-noopt';
    }
    if (p.language === 'vyper'){
      return `vyper-latest-${config}${venom}`;
    }
    return undefined;
  }

  function versionAxisRows(metric, suiteSet){
    const ids = new Set(D.profiles.map(p => p.id));
    const out = [];
    for (const p of D.profiles){
      const baseline = latestBaselineProfile(p);
      if (!baseline || baseline === p.id || !ids.has(baseline)) continue;
      const cmp = compareProfiles(D.rows, baseline, p.id, metric, suiteSet);
      const s = summarize(cmp);
      if (!s.geomean) continue;
      out.push({
        language: p.language,
        config: profileOptimizer(p),
        venom: !!p.experimental_codegen,
        profile: p.id,
        label: p.label,
        version: p.compiler_version,
        versionKey: profileVersionKey(p),
        deltaPct: (s.geomean - 1) * 100,
        comparable: s.count,
      });
    }
    return out;
  }

  // Number formatters
  function fmtDelta(ratio, sign = true){
    if (ratio == null || !isFinite(ratio)) return '—';
    const d = (ratio - 1) * 100;
    const s = (sign && d > 0) ? '+' : '';
    return `${s}${d.toFixed(1)}%`;
  }
  function fmtPct(d, sign = true){
    if (d == null || !isFinite(d)) return '—';
    const s = (sign && d > 0) ? '+' : '';
    return `${s}${d.toFixed(1)}%`;
  }
  function fmtNum(v){
    if (v == null || !isFinite(v)) return '—';
    if (Math.abs(v) >= 10000) return Math.round(v).toLocaleString();
    if (Math.abs(v) >= 100) return v.toFixed(0);
    return v.toFixed(v % 1 === 0 ? 0 : 2);
  }
  function deltaTone(ratio, tieBand = 0.02){
    if (ratio == null || !isFinite(ratio)) return 'tie';
    if (ratio < 1 - tieBand) return 'good';
    if (ratio > 1 + tieBand) return 'bad';
    return 'tie';
  }
  function pctTone(pct, tieBand = 2){
    if (pct == null || !isFinite(pct)) return 'tie';
    if (pct < -tieBand) return 'good';
    if (pct > tieBand) return 'bad';
    return 'tie';
  }

  // Build profile options grouped by language
  function profilesByLang(){
    return D.profiles.reduce((acc, p) => {
      (acc[p.language] = acc[p.language] || []).push(p);
      return acc;
    }, {});
  }

  // List allowed versions / optimizers for a selected language/version.
  function profileFacets(lang, versionKey){
    const ps = matchingProfiles({ language: lang });
    const versionProfiles = versionKey ? ps.filter(p => profileVersionKey(p) === versionKey) : ps;
    const versions = [...new Set(ps.map(profileVersionKey))]
      .sort((a,b) => versionRank(b) - versionRank(a));
    const versionLabels = new Map();
    for (const p of ps) versionLabels.set(profileVersionKey(p), profileVersionLabel(p));
    const optimizers = [...new Set(versionProfiles.map(profileOptimizer))]
      .sort((a,b) => optimizerRank(a) - optimizerRank(b));
    const supportsExperimental = versionProfiles.some(p => p.experimental_codegen);
    return { versions, versionLabels, optimizers, supportsExperimental };
  }
  function defaultOptimizerForVersion(lang, versionKey){
    const optimizers = [...new Set(matchingProfiles({ language: lang, versionKey }).map(profileOptimizer))]
      .sort((a,b) => optimizerRank(a) - optimizerRank(b));
    return preferredOptimizer(lang, optimizers);
  }
  function profileOptionExists(desired){
    return matchingProfiles(desired).length > 0;
  }
  function optimizerRank(o){
    const order = ['noopt','none','legacy','default','gas','codesize','viaIR'];
    const i = order.indexOf(o);
    return i === -1 ? 99 : i;
  }

  function failureReason(error){
    const e = String(error || '');
    if (e.includes('YulException') && e.includes('too deep in the stack')) {
      return 'Yul stack depth while lowering viaIR';
    }
    if (e.includes('Stack too deep')) {
      return 'Stack too deep';
    }
    if (e.includes('Unsupported dup depth')) {
      const m = e.match(/Unsupported dup depth\s+\d+/);
      return m ? m[0] : 'Unsupported dup depth';
    }
    if (e.includes('reserved keyword')) {
      const m = e.match(/'[^']+' is a reserved keyword/);
      return m ? m[0] : 'Reserved keyword syntax gap';
    }
    if (e.includes('UnknownType') && e.includes('DynArray')) {
      return 'DynArray unsupported in this Vyper version';
    }
    if (e.includes('CompilerPanic')) {
      const m = e.match(/CompilerPanic:\s*([^\n]+)/);
      return m ? `CompilerPanic: ${m[1]}` : 'Compiler panic';
    }
    const first = e.split('\n').map(s => s.trim()).find(Boolean);
    return first ? first.slice(0, 96) : 'Compiler error';
  }

  function profileCompactLabel(id){
    const p = profileById(id);
    if (!p) return id;
    const version = profileVersionKey(p) === 'latest' ? 'latest' : p.compiler_version;
    const opt = profileOptimizer(p);
    const venom = p.experimental_codegen ? ' venom' : '';
    return `${version} ${opt}${venom}`;
  }

  function failureGroups(){
    const groups = new Map();
    for (const row of D.rows){
      if (row.status === 'ok') continue;
      const profile = profileById(row.profile_id);
      const reason = failureReason(row.compile?.error);
      const compiler = profile?.compiler_name || row.compiler?.name || row.language;
      const key = `${compiler}|${reason}`;
      if (!groups.has(key)) {
        groups.set(key, {
          compiler,
          language: row.language,
          reason,
          count: 0,
          profiles: new Set(),
          tests: new Set(),
          suites: new Set(),
          values: new Set(),
        });
      }
      const g = groups.get(key);
      g.count++;
      g.profiles.add(row.profile_id);
      g.tests.add(row.benchmark_id);
      g.suites.add(row.suite);
      if (row.parameter_value != null) g.values.add(String(row.parameter_value));
    }
    return [...groups.values()].map(g => ({
      ...g,
      profiles: [...g.profiles].sort((a,b) => profileCompactLabel(a).localeCompare(profileCompactLabel(b))),
      tests: [...g.tests].sort(),
      suites: [...g.suites].sort(),
      values: [...g.values].sort((a,b) => Number(a) - Number(b)),
    })).sort((a,b) => b.count - a.count || a.compiler.localeCompare(b.compiler) || a.reason.localeCompare(b.reason));
  }

  function failureCompilerGroups(){
    const groups = new Map();
    for (const g of failureGroups()){
      const key = g.compiler;
      if (!groups.has(key)) {
        groups.set(key, { compiler: key, count: 0, reasons: new Set(), tests: new Set(), profiles: new Set() });
      }
      const out = groups.get(key);
      out.count += g.count;
      out.reasons.add(g.reason);
      g.tests.forEach(t => out.tests.add(t));
      g.profiles.forEach(p => out.profiles.add(p));
    }
    return [...groups.values()].map(g => ({
      compiler: g.compiler,
      count: g.count,
      reasons: [...g.reasons].sort(),
      tests: [...g.tests].sort(),
      profiles: [...g.profiles].sort((a,b) => profileCompactLabel(a).localeCompare(profileCompactLabel(b))),
    })).sort((a,b) => b.count - a.count);
  }

  window.Bench = {
    D,
    METRICS, SUITES,
    valueAt, scenarioKey, scenarioLabel,
    compareProfiles, summarize, bySuite,
    profileById, profileLabel, profileKnobs, profileVersionKey, profileVersionLabel,
    profileOptimizer, resolveProfile, defaultProfileForLanguage,
    versionRank, optimizerRank, profileFacets, profilesByLang,
    defaultOptimizerForVersion, profileOptionExists,
    versionAxisRows, latestBaselineProfile,
    failureGroups, failureCompilerGroups, failureReason, profileCompactLabel,
    fmtDelta, fmtPct, fmtNum, deltaTone, pctTone, median,
  };
})();

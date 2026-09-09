// Data helpers — all globals on `window.Bench`
(function(){
  const D = window.__BENCH_DATA || window.__EVM_BENCH_REPORT_DATA;
  const failedArtifacts = new Set(D.rows.filter(hasCorrectnessFailure)
    .map(r => `${r.profile_id}|${artifactKey(r)}`));

  function hasCorrectnessFailure(row){
    return row.status === 'ok' && (row.gas?.scenario_status_ok === false
      || Object.values(row.correctness || {}).some(status => status === 'fail'));
  }
  function isComparableArtifact(row){
    return row.status === 'ok' && !failedArtifacts.has(`${row.profile_id}|${artifactKey(row)}`);
  }
  function correctnessFailureGroups(){
    const groups = new Map();
    for (const row of D.rows.filter(hasCorrectnessFailure)) {
      const checks = Object.entries(row.correctness || {}).filter(([,v]) => v === 'fail').map(([k]) => k);
      if (!checks.length) checks.push('scenario_status_check');
      const key = [row.benchmark_id, ...checks].join('|');
      if (!groups.has(key)) groups.set(key, {benchmark:row.benchmark_id, checks, count:0, profiles:new Set(), scenarios:new Set()});
      const group = groups.get(key);
      group.count++;
      group.profiles.add(row.profile_id);
      group.scenarios.add(row.gas?.scenario || 'artifact');
    }
    return [...groups.values()].map(g => ({...g, profiles:[...g.profiles].sort(), scenarios:[...g.scenarios].sort()}));
  }

  const METRICS = [
    { id: 'harness_call_gas',       label: 'Harness call gas', short: 'Runtime gas',  unit: 'gas',  lowerBetter: true, hero: true },
    { id: 'runtime_bytes_stripped', label: 'Runtime bytes',    short: 'Code size',    unit: 'B',    lowerBetter: true },
    { id: 'internal_create_gas',    label: 'Harness deployment gas', short: 'Scenario deploy', unit: 'gas', lowerBetter: true },
    { id: 'compile_wall_ms',        label: 'Compile wall time',short: 'Compile time', unit: 'ms',   lowerBetter: true },
  ];

  const SUITES = {
    fixed:         { label: 'Fixed',        desc: 'Hand-written ports of common contract motifs' },
    scale:         { label: 'Scale',        desc: 'Parametric N=1..64 scaling families' },
    real_derived:  { label: 'Real-derived', desc: 'Upstream originals and provenance-tagged ports' },
  };

  function median(values){
    if (!values || !values.length) return undefined;
    const s = [...values].sort((a,b)=>a-b);
    const m = Math.floor(s.length/2);
    return s.length % 2 ? s[m] : (s[m-1]+s[m])/2;
  }

  function valueAt(row, metric){
    if (!isComparableArtifact(row)) return undefined;
    switch(metric){
      case 'harness_call_gas': return row.gas?.harness_call_gas;
      case 'runtime_bytes_stripped': return row.bytecode?.runtime_bytes_stripped ?? row.bytecode?.runtime_bytes;
      case 'internal_create_gas': return row.gas?.internal_create_gas;
      case 'compile_wall_ms': return median(row.compile?.wall_ms_samples);
      case 'peak_rss_kib': return row.compile?.peak_rss_kib;
    }
  }
  function scenarioKey(r){
    return [
      r.suite,
      r.benchmark_id,
      r.gas?.scenario ?? 'artifact',
      r.gas?.state_access_profile ?? 'artifact',
      r.gas?.deployment_variant ?? 'artifact',
      r.parameter_value ?? '',
    ].join('|');
  }
  function scenarioLabel(r){
    const sc = r.gas?.scenario ?? 'artifact';
    const st = r.gas?.state_access_profile ?? 'artifact';
    const variant = r.gas?.deployment_variant && r.gas.deployment_variant !== 'standard'
      ? ` · ${r.gas.deployment_variant}`
      : '';
    const n  = r.parameter_value == null ? '' : ` N=${r.parameter_value}`;
    return `${r.benchmark_id}${n} · ${sc} · ${st}${variant}`;
  }
  function comparisonLevel(metric){
    return metric === 'harness_call_gas' || metric === 'internal_create_gas' ? 'scenario' : 'artifact';
  }
  function comparisonUnit(metric){
    if (comparisonLevel(metric) === 'scenario') {
      return { singular: 'scenario', plural: 'scenarios', match: 'suite/benchmark/scenario/access/deployment' };
    }
    return { singular: 'artifact', plural: 'artifacts', match: 'suite/benchmark/artifact' };
  }
  function artifactKey(r){
    return [r.suite, r.benchmark_id, r.parameter_value ?? ''].join('|');
  }
  function artifactLabel(r){
    const n = r.parameter_value == null ? '' : ` N=${r.parameter_value}`;
    return `${r.benchmark_id}${n} · artifact`;
  }
  function comparisonKey(r, metric){
    return comparisonLevel(metric) === 'scenario' ? scenarioKey(r) : artifactKey(r);
  }
  function comparisonLabel(r, metric){
    return comparisonLevel(metric) === 'scenario' ? scenarioLabel(r) : artifactLabel(r);
  }
  function compareProfiles(rows, pa, pb, metric, suiteSet){
    const L = new Map(), Ra = new Map(), Rb = new Map();
    for (const r of rows){
      if (suiteSet && !suiteSet.has(r.suite)) continue;
      const v = valueAt(r, metric);
      if (v == null) continue;
      const k = comparisonKey(r, metric);
      if (r.profile_id === pa && !L.has(k)){ Ra.set(k, r); L.set(k, v); }
      if (r.profile_id === pb && !Rb.has(k)){ Rb.set(k, r); }
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
        label: comparisonLabel(Ra.get(k), metric),
        suite: Ra.get(k).suite,
        valueA: va,
        valueB: vb,
        ratio,
        deltaPct: (ratio - 1) * 100,
        comparisonLevel: comparisonLevel(metric),
      });
    }
    return out.sort((a,b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct));
  }

  function profilePairCompileCoverage(rows, pa, pb, suiteSet){
    const profiles = new Map([
      [pa, { attempted: new Set(), passed: new Set() }],
      [pb, { attempted: new Set(), passed: new Set() }],
    ]);
    for (const r of rows){
      if (!profiles.has(r.profile_id)) continue;
      if (suiteSet && !suiteSet.has(r.suite)) continue;
      if (r.status !== 'ok' && r.status !== 'compile_error') continue;
      const bucket = profiles.get(r.profile_id);
      const key = artifactKey(r);
      bucket.attempted.add(key);
      if (r.status === 'ok') bucket.passed.add(key);
    }
    let attempted = 0;
    let passed = 0;
    for (const bucket of profiles.values()){
      attempted += bucket.attempted.size;
      passed += bucket.passed.size;
    }
    return {
      attempted,
      passed,
      failed: Math.max(0, attempted - passed),
      passRate: attempted ? passed / attempted : null,
    };
  }

  function defaultTieBand(){
    return D.defaults?.tie_band ?? 0.02;
  }
  function tieBandForMetric(metric){
    switch(metric){
      case 'harness_call_gas':
      case 'internal_create_gas':
      case 'runtime_bytes_stripped':
        return 0.005;
      default:
        return defaultTieBand();
    }
  }

  function summarize(rows, tieBand = defaultTieBand()){
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
    return p ? profileDisplayLabel(p) : id;
  }
  function compilerDisplayName(p){
    if (p.language === 'vyper' || p.compiler_name === 'vyper') return 'Vyper';
    if (p.language === 'fe' || p.compiler_name === 'fe') return 'Fe';
    if (p.compiler_name === 'solar') return 'Solar';
    return p.compiler_name || (p.language === 'solidity' ? 'solc' : p.language);
  }
  function profileCompilerKey(p){
    return p.compiler_name || (String(p.id).startsWith('solx-') ? 'solx'
      : p.language === 'solidity' ? 'solc' : p.language);
  }
  function compilerOptions(){
    return ['solc', 'solar', 'solx', 'vyper', 'fe'].filter(key => D.profiles.some(p => profileCompilerKey(p) === key))
      .map(value => ({ value, label: value === 'vyper' ? 'Vyper' : value === 'fe' ? 'Fe' : value }));
  }
  function profileDisplayLabel(p){
    const opt = profileOptimizer(p);
    const runs = profileOptimizerRuns(p);
    const runsLabel = runs == null ? '' : ` runs${runs}`;
    const venom = p.experimental_codegen ? ' + Venom' : '';
    return `${compilerDisplayName(p)} ${p.compiler_version || profileVersionKey(p)}${p.source_revision ? ` @${p.source_revision.slice(0, 8)}` : ''} ${opt}${runsLabel}${venom}`;
  }
  function profileVersionKey(p){
    const prefix = `${profileCompilerKey(p)}-latest-`;
    if (String(p.id).startsWith(prefix)) return 'latest';
    return p.source_revision || String(p.compiler_version ?? 'unknown');
  }
  function profileVersionLabel(p){
    const key = profileVersionKey(p);
    if (key === 'latest') return `latest (${p.compiler_version})`;
    return p.source_revision ? `${p.compiler_version} @${p.source_revision.slice(0, 8)}` : p.compiler_version || key;
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
    if (profileCompilerKey(p) === 'solar') return p.optimizer || (id.includes('-size-') ? 'size' : 'gas');
    if (profileCompilerKey(p) === 'solx') {
      const mode = id.match(/-O([123sz])(?:-|$)/)?.[1];
      return p.optimizer || (mode ? `O${mode}` : 'unknown');
    }
    if (p.language === 'solidity'){
      if (id.match(/viair-runs\d+/)) return 'viaIR';
      if (id.match(/legacy-runs\d+/)) return 'legacy';
      if (id.includes('noopt')) return 'noopt';
    }
    if (p.language === 'vyper'){
      if (id.includes('codesize')) return 'codesize';
      if (id.includes('gas')) return 'gas';
      if (id.includes('none')) return 'none';
      if (id.includes('default')) return 'default';
    }
    if (p.language === 'fe'){
      const m = id.match(/-O([0-9s]+)\b/);
      if (m) return `O${m[1]}`;
    }
    return 'default';
  }
  function optimizerUsesRuns(lang, optimizer){
    return lang === 'solidity' && ['legacy', 'viaIR', 'gas', 'size'].includes(optimizer);
  }
  function profileOptimizerRuns(p){
    if (!optimizerUsesRuns(p.language, profileOptimizer(p))) return null;
    const m = String(p.id).match(/-runs(\d+)/);
    const runs = m ? Number(m[1]) : Number(p.optimizer_runs);
    return Number.isFinite(runs) && runs > 0 ? runs : null;
  }
  function profileKnobs(p){
    return {
      language: p.language,
      compiler: profileCompilerKey(p),
      versionKey: profileVersionKey(p),
      optimizer: profileOptimizer(p),
      runs: profileOptimizerRuns(p),
      experimental: !!p.experimental_codegen,
    };
  }
  function matchingProfiles(desired){
    return D.profiles.filter(p => {
      const k = profileKnobs(p);
      return (!desired.language || k.language === desired.language)
        && (!desired.compiler || k.compiler === desired.compiler)
        && (!desired.versionKey || k.versionKey === desired.versionKey)
        && (!desired.optimizer || k.optimizer === desired.optimizer)
        && (desired.runs == null || k.runs === Number(desired.runs))
        && (desired.experimental == null || k.experimental === desired.experimental);
    });
  }
  function preferredOptimizer(lang, optimizers){
    const preferred = lang === 'solidity'
      ? ['viaIR','legacy','noopt']
      : lang === 'fe'
        ? ['O2','O1','Os','O0']
        : ['gas','codesize','none','default'];
    return preferred.find(o => optimizers.includes(o)) ?? optimizers[0];
  }
  function resolveProfile(desired){
    const cands = matchingProfiles({ language: desired.language, compiler: desired.compiler });
    const wantedRuns = desired.runs == null
      ? defaultOptimizerRuns(desired.language, desired.versionKey, desired.optimizer, desired.compiler)
      : Number(desired.runs);
    const runsMatch = k => !optimizerUsesRuns(k.language, k.optimizer) || wantedRuns == null || k.runs === wantedRuns;
    const exact = cands.find(p => {
      const k = profileKnobs(p);
      return k.versionKey === desired.versionKey
          && k.optimizer === desired.optimizer
          && runsMatch(k)
          && k.experimental === desired.experimental;
    });
    if (exact) return exact.id;
    const fallback1 = cands.find(p => {
      const k = profileKnobs(p);
      return k.versionKey === desired.versionKey && k.optimizer === desired.optimizer && runsMatch(k);
    });
    if (fallback1) return fallback1.id;
    const fallback2 = cands.find(p => profileVersionKey(p) === desired.versionKey);
    if (fallback2) return fallback2.id;
    return cands[0]?.id ?? D.profiles[0].id;
  }
  function defaultProfileForLanguage(lang){
    const pref = lang === 'solidity' ? 'solc-latest-viair-runs200'
      : lang === 'fe' ? 'fe-latest-O2'
      : 'vyper-latest-gas';
    if (D.profiles.some(p => p.id === pref)) return pref;
    return D.profiles.find(p => p.language === lang)?.id ?? D.profiles[0].id;
  }
  function defaultProfileForCompiler(compiler){
    const pref = {solc: 'solc-latest-viair-runs200', solx: 'solx-0.1.8-O3', solar: 'solar-716e9cbc-gas-runs200', vyper: 'vyper-latest-gas', fe: 'fe-latest-O2'}[compiler];
    return D.profiles.find(p => p.id === pref)?.id
      ?? D.profiles.find(p => profileCompilerKey(p) === compiler)?.id ?? D.profiles[0].id;
  }

  function latestBaselineProfile(p){
    if (['solx', 'solar'].includes(profileCompilerKey(p))) return undefined;
    const config = profileOptimizer(p);
    const venom = p.experimental_codegen ? '-venom' : '';
    if (p.language === 'solidity'){
      if (config === 'viaIR') return 'solc-latest-viair-runs200';
      if (config === 'legacy') return 'solc-latest-legacy-runs200';
      if (config === 'noopt') return 'solc-latest-noopt';
    }
    if (p.language === 'vyper'){
      if (config === 'default') return `vyper-latest-none${venom}`;
      return `vyper-latest-${config}${venom}`;
    }
    return undefined;
  }

  function versionAxisConfig(p){
    const config = profileOptimizer(p);
    return p.language === 'vyper' && config === 'default' ? 'none' : config;
  }

  function versionAxisRows(metric, suiteSet){
    const ids = new Set(D.profiles.map(p => p.id));
    const out = [];
    for (const p of D.profiles){
      const runs = profileOptimizerRuns(p);
      if (p.language === 'solidity' && runs != null && runs !== 200) continue;
      if (p.language === 'vyper' && profileOptimizer(p) === 'default') {
        const hasExplicitNone = D.profiles.some(candidate =>
          candidate.language === 'vyper'
          && profileVersionKey(candidate) === profileVersionKey(p)
          && profileOptimizer(candidate) === 'none'
          && !!candidate.experimental_codegen === !!p.experimental_codegen
        );
        if (hasExplicitNone) continue;
      }
      const baseline = latestBaselineProfile(p);
      if (!baseline || baseline === p.id || !ids.has(baseline)) continue;
      const baselineProfile = D.profiles.find(candidate => candidate.id === baseline);
      const cmp = compareProfiles(D.rows, baseline, p.id, metric, suiteSet);
      const s = summarize(cmp);
      if (!s.geomean) continue;
      out.push({
        language: p.language,
        config: versionAxisConfig(p),
        venom: !!p.experimental_codegen,
        profile: p.id,
        label: profileDisplayLabel(p),
        baseline,
        baselineLabel: baselineProfile ? profileDisplayLabel(baselineProfile) : baseline,
        baselineConfig: baselineProfile ? versionAxisConfig(baselineProfile) : undefined,
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
  function profileFacets(lang, versionKey, optimizer, compiler){
    const ps = matchingProfiles({ language: lang, compiler });
    const versionProfiles = versionKey ? ps.filter(p => profileVersionKey(p) === versionKey) : ps;
    const versions = [...new Set(ps.map(profileVersionKey))]
      .sort((a,b) => versionRank(b) - versionRank(a));
    const versionLabels = new Map();
    for (const p of ps) versionLabels.set(profileVersionKey(p), profileVersionLabel(p));
    const optimizers = [...new Set(versionProfiles.map(profileOptimizer))]
      .sort((a,b) => optimizerRank(a) - optimizerRank(b));
    const runProfiles = optimizer ? versionProfiles.filter(p => profileOptimizer(p) === optimizer) : versionProfiles;
    const runs = [...new Set(runProfiles.map(profileOptimizerRuns).filter(v => v != null))]
      .sort((a,b) => a - b);
    const supportsExperimental = versionProfiles.some(p => p.experimental_codegen);
    return { versions, versionLabels, optimizers, runs, supportsExperimental };
  }
  function defaultOptimizerForVersion(lang, versionKey, compiler){
    const optimizers = [...new Set(matchingProfiles({ language: lang, versionKey, compiler }).map(profileOptimizer))]
      .sort((a,b) => optimizerRank(a) - optimizerRank(b));
    if (compiler === 'solar') return optimizers.includes('gas') ? 'gas' : optimizers[0];
    if (compiler === 'solx') return optimizers.includes('O3') ? 'O3' : optimizers[0];
    return preferredOptimizer(lang, optimizers);
  }
  function defaultOptimizerRuns(lang, versionKey, optimizer, compiler){
    if (!optimizerUsesRuns(lang, optimizer)) return null;
    const runs = profileFacets(lang, versionKey, optimizer, compiler).runs;
    return runs.includes(200) ? 200 : (runs[0] ?? null);
  }
  function profileOptionExists(desired){
    return matchingProfiles(desired).length > 0;
  }
  function optimizerRank(o){
    const order = ['noopt','none','legacy','default','gas','codesize','viaIR','O3','Oz'];
    const i = order.indexOf(o);
    return i === -1 ? 99 : i;
  }

  function failureReason(error){
    const e = normalizedFailureText(error);
    const diagnostic = primaryFailureDiagnostic(e);
    const focused = diagnostic ? `${diagnostic}\n${e}` : e;
    if (focused.includes('YulException') && focused.includes('too deep in the stack')) {
      return 'Yul stack depth while lowering viaIR';
    }
    if (focused.includes('Stack too deep')) {
      return 'Stack too deep';
    }
    if (focused.includes('Unsupported dup depth') || focused.includes('Unsupported swap depth')) {
      const m = focused.match(/Unsupported (?:dup|swap) depth\s+\d+/);
      return m ? m[0] : 'Unsupported dup/swap depth';
    }
    if (focused.includes('EVM backend supports at most')) {
      const m = focused.match(/EVM backend supports at most \d+ [a-z ]+/);
      return m ? m[0] : 'Sonatina backend operand limit';
    }
    if (focused.includes('reserved keyword')) {
      const m = focused.match(/'[^']+' is a reserved keyword/);
      return m ? m[0] : 'Reserved keyword syntax gap';
    }
    if (focused.includes('UnknownType') && focused.includes('DynArray')) {
      return 'DynArray unsupported in this Vyper version';
    }
    if (focused.includes('`isqrt` builtin was removed')) {
      return '`isqrt` builtin removed in this Vyper version';
    }
    if (focused.includes('CompilerPanic')) {
      const m = focused.match(/CompilerPanic:\s*([^\n]+)/);
      return m ? `CompilerPanic: ${m[1]}` : 'Compiler panic';
    }
    if (focused.includes('AssertionError')) {
      return 'Compiler assertion failure';
    }
    const first = diagnostic || firstUsefulFailureLine(e);
    return first ? first.slice(0, 96) : 'Compiler error';
  }

  function primaryFailureDiagnostic(text){
    const diagnostics = failureLines(text)
      .map(cleanDiagnosticPrefix)
      .filter(line => isCompilerDiagnostic(line));
    return diagnostics.find(line => !isContainerDiagnostic(line)) || diagnostics[0] || '';
  }

  function firstUsefulFailureLine(text){
    return failureLines(text)
      .map(cleanDiagnosticPrefix)
      .find(line => !isFailureWrapperLine(line)) || '';
  }

  function failureLines(text){
    return String(text || '').split('\n').map(s => s.trim()).filter(Boolean);
  }

  function cleanDiagnosticPrefix(line){
    return line.replace(/^(?:vyper\.exceptions\.|solcx\.exceptions\.)/, '').replace(/\.$/, '');
  }

  function isCompilerDiagnostic(line){
    return /^(?:[A-Za-z_][\w]*(?:Exception|Error|Panic)|CompilerPanic|UnknownType|UndeclaredDefinition|StackTooDeep|YulException|ParserError|TypeError|DeclarationError):/.test(line);
  }

  function isContainerDiagnostic(line){
    return /^(?:VyperException|UnknownType|StructureException): Compilation failed with the following errors:/.test(line);
  }

  function isFailureWrapperLine(line){
    return /^(?:solc|vyper|command) compile failed with status /.test(line)
      || /^fe build failed with status /.test(line)
      || line === 'stdout:'
      || line === 'stderr:'
      || line.startsWith('Error compiling:');
  }

  function normalizedFailureText(error){
    if (Array.isArray(error)) {
      return error.map(formatCompilerDiagnostic).filter(Boolean).join('\n');
    }
    if (error && typeof error === 'object') {
      return formatCompilerDiagnostic(error) || JSON.stringify(error);
    }
    const text = String(error || '');
    const trimmed = text.trim();
    if (trimmed.startsWith('[') || trimmed.startsWith('{')) {
      try {
        return normalizedFailureText(JSON.parse(trimmed));
      } catch (_) {
        // Fall back to the raw compiler output below.
      }
    }
    return text;
  }

  function formatCompilerDiagnostic(item){
    if (!item || typeof item !== 'object') return String(item || '');
    return item.formattedMessage || item.message || item.type || '';
  }

  function profileCompactLabel(id){
    const p = profileById(id);
    if (!p) return id;
    const version = p.compiler_version || profileVersionKey(p);
    const opt = profileOptimizer(p);
    const runs = profileOptimizerRuns(p);
    const runsLabel = runs == null ? '' : ` runs${runs}`;
    const venom = p.experimental_codegen ? ' venom' : '';
    return `${version} ${opt}${runsLabel}${venom}`;
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
    valueAt, scenarioKey, scenarioLabel, comparisonLevel, comparisonUnit,
    hasCorrectnessFailure, isComparableArtifact, correctnessFailureGroups,
    compareProfiles, profilePairCompileCoverage, summarize, bySuite, tieBandForMetric,
    profileById, profileLabel, profileKnobs, profileVersionKey, profileVersionLabel,
    profileOptimizer, resolveProfile, defaultProfileForLanguage, defaultProfileForCompiler,
    profileCompilerKey, compilerOptions,
    profileOptimizerRuns, versionRank, optimizerRank, profileFacets, profilesByLang,
    defaultOptimizerForVersion, defaultOptimizerRuns, profileOptionExists,
    versionAxisRows, latestBaselineProfile,
    failureGroups, failureCompilerGroups, failureReason, profileCompactLabel,
    profileDisplayLabel,
    fmtDelta, fmtPct, fmtNum, deltaTone, pctTone, median,
  };
})();

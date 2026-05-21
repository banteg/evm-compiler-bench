import { createEffect, createMemo, createResource, createSignal, createUniqueId, For, Match, Show, Switch } from "solid-js";
import { render } from "solid-js/web";
import vegaEmbed from "vega-embed";
import "./styles.css";

import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./components/ui/card";
import { Select } from "./components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "./components/ui/table";
import { cn } from "./lib/utils";

type Json = Record<string, any>;

type ReportModel = {
  schema_version: number;
  generated_at: string;
  defaults: Json;
  summary: Json;
  manifest: Json;
  profiles: Json[];
  benchmarks: Json[];
  real_derived_models: Json[];
  rows: Json[];
};

type MetricId =
  | "harness_call_gas"
  | "runtime_bytes_stripped"
  | "internal_create_gas"
  | "compile_wall_ms"
  | "peak_rss_kib";

type ComparisonRow = {
  key: string;
  label: string;
  suite: string;
  benchmark: string;
  scenario: string;
  state: string;
  valueA: number;
  valueB: number;
  ratio: number;
  deltaPct: number;
};

type ProfileKnobs = {
  language: string;
  versionKey: string;
  optimizer: string;
  experimental: boolean;
};

const metricOptions: Array<{ id: MetricId; label: string; lowerIsBetter: boolean }> = [
  { id: "harness_call_gas", label: "Harness call gas", lowerIsBetter: true },
  { id: "runtime_bytes_stripped", label: "Runtime bytes", lowerIsBetter: true },
  { id: "internal_create_gas", label: "Internal create gas", lowerIsBetter: true },
  { id: "compile_wall_ms", label: "Compile wall ms", lowerIsBetter: true },
  { id: "peak_rss_kib", label: "Peak RSS", lowerIsBetter: true },
];

const suiteLabels: Record<string, string> = {
  fixed: "Fixed",
  scale: "Scale",
  real_derived: "Real-derived",
};

const languageLabels: Record<string, string> = {
  solidity: "Solidity / solc",
  vyper: "Vyper",
};

const optimizerLabels: Record<string, string> = {
  noopt: "no optimizer",
  legacy: "legacy optimizer",
  viaIR: "viaIR",
  default: "default",
  none: "none",
  gas: "gas",
  codesize: "codesize",
};

declare global {
  interface Window {
    __EVM_BENCH_REPORT_DATA?: ReportModel;
  }
}

async function loadReportModel(): Promise<ReportModel> {
  if (window.__EVM_BENCH_REPORT_DATA) {
    return window.__EVM_BENCH_REPORT_DATA;
  }
  const response = await fetch("./report-model.json");
  if (!response.ok) {
    throw new Error(`failed to load report-model.json: ${response.status}`);
  }
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`failed to parse report-model.json: ${String(error)}; response started with ${text.slice(0, 64)}`);
  }
}

function valueAt(row: Json, metric: MetricId): number | undefined {
  if (row.status !== "ok") return undefined;
  switch (metric) {
    case "harness_call_gas":
      return row.gas?.harness_call_gas;
    case "runtime_bytes_stripped":
      return row.bytecode?.runtime_bytes_stripped ?? row.bytecode?.runtime_bytes;
    case "internal_create_gas":
      return row.gas?.internal_create_gas;
    case "compile_wall_ms":
      return median(row.compile?.wall_ms_samples ?? []);
    case "peak_rss_kib":
      return row.compile?.peak_rss_kib;
  }
}

function median(values: number[]): number | undefined {
  if (!values.length) return undefined;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function scenarioKey(row: Json): string {
  const scenario = row.gas?.scenario ?? "artifact";
  const state = row.gas?.state_access_profile ?? "artifact";
  const n = row.parameter_value ?? "";
  return [row.suite, row.benchmark_id, scenario, state, n].join("|");
}

function scenarioLabel(row: Json): string {
  const scenario = row.gas?.scenario ?? "artifact";
  const state = row.gas?.state_access_profile ?? "artifact";
  const n = row.parameter_value == null ? "" : ` N=${row.parameter_value}`;
  return `${row.benchmark_id}${n} / ${scenario} / ${state}`;
}

function compareProfiles(
  rows: Json[],
  profileA: string,
  profileB: string,
  metric: MetricId,
  suites: Set<string>,
): ComparisonRow[] {
  const left = new Map<string, Json>();
  const right = new Map<string, Json>();
  for (const row of rows) {
    if (!suites.has(row.suite)) continue;
    if (row.profile_id === profileA && valueAt(row, metric) != null) {
      left.set(scenarioKey(row), row);
    } else if (row.profile_id === profileB && valueAt(row, metric) != null) {
      right.set(scenarioKey(row), row);
    }
  }
  const out: ComparisonRow[] = [];
  for (const [key, a] of left) {
    const b = right.get(key);
    if (!b) continue;
    const valueA = valueAt(a, metric);
    const valueB = valueAt(b, metric);
    if (!valueA || valueB == null || valueB <= 0) continue;
    const ratio = valueB / valueA;
    out.push({
      key,
      label: scenarioLabel(a),
      suite: a.suite,
      benchmark: a.benchmark_id,
      scenario: a.gas?.scenario ?? "artifact",
      state: a.gas?.state_access_profile ?? "artifact",
      valueA,
      valueB,
      ratio,
      deltaPct: (ratio - 1) * 100,
    });
  }
  return out.sort((a, b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct));
}

function summarize(rows: ComparisonRow[], tieBand: number) {
  const ratios = rows.map((row) => row.ratio).filter((ratio) => ratio > 0);
  const geomean = ratios.length ? Math.exp(ratios.reduce((acc, ratio) => acc + Math.log(ratio), 0) / ratios.length) : undefined;
  let cheaper = 0;
  let tie = 0;
  let costlier = 0;
  for (const row of rows) {
    if (row.ratio < 1 - tieBand) cheaper += 1;
    else if (row.ratio > 1 + tieBand) costlier += 1;
    else tie += 1;
  }
  return { geomean, cheaper, tie, costlier, count: rows.length };
}

function bySuite(rows: ComparisonRow[], tieBand: number) {
  const grouped = new Map<string, ComparisonRow[]>();
  for (const row of rows) {
    if (!grouped.has(row.suite)) grouped.set(row.suite, []);
    grouped.get(row.suite)!.push(row);
  }
  return [...grouped].map(([suite, entries]) => ({ suite, ...summarize(entries, tieBand) }));
}

function formatDelta(ratio: number | undefined): string {
  if (ratio == null || Number.isNaN(ratio)) return "n/a";
  const delta = (ratio - 1) * 100;
  return `${delta >= 0 ? "+" : ""}${delta.toFixed(1)}%`;
}

function formatNumber(value: number | undefined): string {
  if (value == null || Number.isNaN(value)) return "n/a";
  if (Math.abs(value) >= 1000) return Math.round(value).toLocaleString();
  return value.toFixed(value % 1 === 0 ? 0 : 2);
}

function deltaClass(ratio: number | undefined): string {
  if (ratio == null || Number.isNaN(ratio)) return "cell-neutral";
  if (ratio <= 0.98) return "cell-good";
  if (ratio >= 1.02) return "cell-fail";
  return "cell-warn";
}

function compileSummary(model: ReportModel, profileId: string) {
  return model.profiles.find((profile) => profile.id === profileId);
}

function profileById(profiles: Json[], id: string): Json | undefined {
  return profiles.find((profile) => profile.id === id);
}

function profileLabel(profiles: Json[], id: string): string {
  const profile = profileById(profiles, id);
  return profile?.label ?? id;
}

function profileConfig(profileId: string): string {
  if (profileId.includes("viair")) return "viaIR";
  if (profileId.includes("legacy")) return "legacy";
  if (profileId.includes("noopt")) return "noopt";
  if (profileId.includes("codesize-venom")) return "codesize venom";
  if (profileId.includes("gas-venom")) return "gas venom";
  if (profileId.includes("none-venom")) return "none venom";
  if (profileId.includes("codesize")) return "codesize";
  if (profileId.includes("gas")) return "gas";
  if (profileId.includes("none")) return "none";
  return "default";
}

function profileVersionKey(profile: Json): string {
  const latestPrefix = profile.language === "solidity" ? "solc-latest-" : "vyper-latest-";
  if (String(profile.id).startsWith(latestPrefix)) return "latest";
  return String(profile.compiler_version ?? "unknown");
}

function profileVersionLabel(profile: Json): string {
  const key = profileVersionKey(profile);
  if (key === "latest") return `latest (${profile.compiler_version})`;
  return key;
}

function versionRank(version: string): number {
  if (version === "latest") return Number.POSITIVE_INFINITY;
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:a(\d+))?/);
  if (!match) return -1;
  const [, major, minor, patch, alpha] = match;
  const alphaRank = alpha == null ? 999 : Number(alpha);
  return Number(major) * 1_000_000_000 + Number(minor) * 1_000_000 + Number(patch) * 1_000 + alphaRank;
}

function profileOptimizer(profile: Json): string {
  const id = String(profile.id);
  if (profile.language === "solidity") {
    if (id.includes("viair-runs200")) return "viaIR";
    if (id.includes("legacy-runs200")) return "legacy";
    if (id.includes("noopt")) return "noopt";
  }
  return String(profile.optimizer ?? "default");
}

function profileKnobs(profile: Json): ProfileKnobs {
  return {
    language: String(profile.language),
    versionKey: profileVersionKey(profile),
    optimizer: profileOptimizer(profile),
    experimental: Boolean(profile.experimental_codegen),
  };
}

function defaultProfileForLanguage(profiles: Json[], language: string): string {
  const preferred = language === "solidity" ? "solc-latest-viair-runs200" : "vyper-latest-gas";
  if (profiles.some((profile) => profile.id === preferred)) return preferred;
  return profiles.find((profile) => profile.language === language)?.id ?? profiles[0]?.id ?? "";
}

function resolveProfile(profiles: Json[], desired: ProfileKnobs): string {
  const candidates = profiles.filter((profile) => profile.language === desired.language);
  const exact = candidates.find((profile) => {
    const knobs = profileKnobs(profile);
    return (
      knobs.versionKey === desired.versionKey &&
      knobs.optimizer === desired.optimizer &&
      knobs.experimental === desired.experimental
    );
  });
  if (exact) return exact.id;

  const sameOptimizer = candidates.find((profile) => {
    const knobs = profileKnobs(profile);
    return knobs.versionKey === desired.versionKey && knobs.optimizer === desired.optimizer;
  });
  if (sameOptimizer) return sameOptimizer.id;

  const sameVersion = candidates.find((profile) => profileVersionKey(profile) === desired.versionKey);
  if (sameVersion) return sameVersion.id;

  return defaultProfileForLanguage(profiles, desired.language);
}

function optionValues<T>(items: T[], key: (item: T) => string): string[] {
  return [...new Set(items.map(key))];
}

function latestBaselineProfile(profile: Json): string | undefined {
  const config = profileConfig(profile.id);
  if (profile.language === "solidity") {
    if (config === "viaIR") return "solc-latest-viair-runs200";
    if (config === "legacy") return "solc-latest-legacy-runs200";
    if (config === "noopt") return "solc-latest-noopt";
  }
  if (profile.language === "vyper") {
    return `vyper-latest-${config.replace(" ", "-")}`;
  }
  return undefined;
}

function versionAxisRows(model: ReportModel, metric: MetricId, suites: Set<string>) {
  const profilesById = new Map(model.profiles.map((profile) => [profile.id, profile]));
  const out: Json[] = [];
  for (const profile of model.profiles) {
    const baseline = latestBaselineProfile(profile);
    if (!baseline || baseline === profile.id || !profilesById.has(baseline)) continue;
    const compared = compareProfiles(model.rows, baseline, profile.id, metric, suites);
    const summary = summarize(compared, 0.02);
    if (!summary.geomean) continue;
    out.push({
      language: profile.language,
      config: profileConfig(profile.id),
      profile: profile.id,
      version: profile.compiler_version,
      deltaPct: (summary.geomean - 1) * 100,
      comparable: summary.count,
    });
  }
  return out;
}

function chartConfig() {
  return {
    background: "transparent",
    font: "Inter, system-ui, -apple-system, sans-serif",
    axis: {
      labelFontSize: 11,
      titleFontSize: 12,
      labelColor: "#526070",
      titleColor: "#526070",
      gridColor: "#e8edf3",
      domainColor: "#a7b0bd",
      tickColor: "#a7b0bd",
    },
    legend: {
      labelFontSize: 11,
      titleFontSize: 12,
      labelColor: "#334155",
      titleColor: "#334155",
      symbolType: "circle",
    },
    view: { stroke: null },
  };
}

function VegaChart(props: { spec: Json }) {
  let el: HTMLDivElement | undefined;
  createEffect(() => {
    if (!el) return;
    el.replaceChildren();
    vegaEmbed(el, props.spec, { actions: false, renderer: "svg" }).catch((error) => {
      if (el) el.textContent = String(error);
    });
  });
  return <div ref={el} class="vega-chart" />;
}

function CheckboxPill(props: {
  id: string;
  checked: boolean;
  disabled?: boolean;
  label: string;
  title?: string;
  class?: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label
      for={props.id}
      title={props.title}
      class={cn(
        "inline-flex h-10 cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-3 text-sm font-medium ring-offset-background transition-colors hover:bg-accent hover:text-accent-foreground has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring has-[:focus-visible]:ring-offset-2 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50",
        props.checked && "bg-secondary text-secondary-foreground hover:bg-secondary/80",
        props.class,
      )}
    >
      <input
        id={props.id}
        type="checkbox"
        class="h-4 w-4 rounded border-input accent-primary"
        checked={props.checked}
        disabled={props.disabled}
        onChange={(event) => props.onChange(event.currentTarget.checked)}
      />
      <span>{props.label}</span>
    </label>
  );
}

function deltaChartSpec(rows: ComparisonRow[]) {
  const values = rows.slice(0, 80).map((row) => ({
    label: row.label,
    suite: suiteLabels[row.suite] ?? row.suite,
    deltaPct: row.deltaPct,
    valueA: row.valueA,
    valueB: row.valueB,
  }));
  return {
    $schema: "https://vega.github.io/schema/vega-lite/v6.json",
    data: { values },
    width: "container",
    height: Math.max(260, Math.min(900, values.length * 16)),
    mark: { type: "bar", cornerRadiusEnd: 2 },
    encoding: {
      y: { field: "label", type: "nominal", sort: "-x", title: null, axis: { labelLimit: 280 } },
      x: { field: "deltaPct", type: "quantitative", title: "Delta vs baseline profile (%)" },
      color: {
        field: "suite",
        type: "nominal",
        scale: { range: ["#2563eb", "#0f8a5f", "#7c3aed"] },
        legend: { orient: "bottom" },
      },
      tooltip: [
        { field: "label", title: "Scenario" },
        { field: "suite", title: "Suite" },
        { field: "deltaPct", title: "Delta %", format: "+.1f" },
        { field: "valueA", title: "Baseline", format: "," },
        { field: "valueB", title: "Compared", format: "," },
      ],
    },
    config: chartConfig(),
  };
}

function suiteChartSpec(entries: ReturnType<typeof bySuite>) {
  return {
    $schema: "https://vega.github.io/schema/vega-lite/v6.json",
    data: {
      values: entries.map((entry) => ({
        suite: suiteLabels[entry.suite] ?? entry.suite,
        deltaPct: entry.geomean == null ? null : (entry.geomean - 1) * 100,
        count: entry.count,
      })),
    },
    width: "container",
    height: 220,
    mark: { type: "bar", cornerRadiusEnd: 3 },
    encoding: {
      x: { field: "suite", type: "nominal", title: null },
      y: { field: "deltaPct", type: "quantitative", title: "Geomean delta (%)" },
      color: { field: "suite", type: "nominal", legend: null, scale: { range: ["#2563eb", "#0f8a5f", "#7c3aed"] } },
      tooltip: [
        { field: "suite", title: "Suite" },
        { field: "deltaPct", title: "Delta %", format: "+.1f" },
        { field: "count", title: "Comparable rows" },
      ],
    },
    config: chartConfig(),
  };
}

function versionChartSpec(values: Json[]) {
  return {
    $schema: "https://vega.github.io/schema/vega-lite/v6.json",
    data: { values },
    width: "container",
    height: 280,
    facet: { field: "language", type: "nominal", columns: 1, title: null },
    spec: {
      mark: { type: "line", point: { filled: true, size: 65 }, strokeWidth: 2.5 },
      encoding: {
        x: { field: "version", type: "ordinal", title: "Compiler version" },
        y: { field: "deltaPct", type: "quantitative", title: "Delta vs latest same config (%)" },
        color: {
          field: "config",
          type: "nominal",
          title: "Config",
          scale: { range: ["#93c5fd", "#2563eb", "#1e3a8a", "#86efac", "#16a34a", "#166534", "#4ade80", "#15803d", "#14532d"] },
        },
        tooltip: [
          { field: "profile", title: "Profile" },
          { field: "version", title: "Version" },
          { field: "config", title: "Config" },
          { field: "deltaPct", title: "Delta %", format: "+.1f" },
          { field: "comparable", title: "Comparable rows" },
        ],
      },
    },
    config: chartConfig(),
  };
}

function App() {
  const [model] = createResource(loadReportModel);
  return (
    <Switch>
      <Match when={model.loading}>
        <main class="container py-8"><Card><CardContent class="pt-6">Loading report data...</CardContent></Card></main>
      </Match>
      <Match when={model.error}>
        <main class="container py-8">
          <Card class="border-destructive/40">
            <CardContent class="pt-6 text-sm text-destructive">Failed to load report data: {String(model.error)}</CardContent>
          </Card>
        </main>
      </Match>
      <Match when={model()}>
        {(loaded) => <Report model={loaded()} />}
      </Match>
    </Switch>
  );
}

function Report(props: { model: ReportModel }) {
  const model = () => props.model;
  const metricId = createUniqueId();
  const suiteId = createUniqueId();
  const [profileA, setProfileA] = createSignal(model().defaults?.baseline_profile ?? "solc-latest-viair-runs200");
  const [profileB, setProfileB] = createSignal(model().defaults?.comparison_profile ?? "vyper-latest-gas");
  const [metric, setMetric] = createSignal<MetricId>(model().defaults?.primary_metric ?? "harness_call_gas");
  const [suiteFilter, setSuiteFilter] = createSignal(new Set(["fixed", "scale", "real_derived"]));
  const tieBand = () => Number(model().defaults?.tie_band ?? 0.02);

  const comparison = createMemo(() => compareProfiles(model().rows, profileA(), profileB(), metric(), suiteFilter()));
  const aggregate = createMemo(() => summarize(comparison(), tieBand()));
  const suiteRows = createMemo(() => bySuite(comparison(), tieBand()));
  const versionRows = createMemo(() => versionAxisRows(model(), metric(), suiteFilter()));

  const setPreset = (a: string, b: string) => {
    setProfileA(a);
    setProfileB(b);
  };

  const setSuiteChecked = (suite: string, checked: boolean) => {
    const next = new Set(suiteFilter());
    if (checked) next.add(suite);
    else next.delete(suite);
    if (next.size > 0) {
      setSuiteFilter(next);
    }
  };

  return (
    <main class="container space-y-6 py-8">
      <header class="grid gap-6 lg:grid-cols-[1fr_280px] lg:items-start">
        <div class="space-y-3">
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="secondary">EVM Compiler Bench</Badge>
            <Badge variant="outline">{model().manifest?.evm_version ?? "unknown"} EVM</Badge>
          </div>
          <h1 class="max-w-4xl text-4xl font-bold tracking-tight">Compiler tradeoffs, not a language leaderboard</h1>
          <p class="max-w-3xl text-base leading-7 text-muted-foreground">
            Interactive report generated from <code>report-model.json</code>. Pick two compiler configurations and read deltas on the same benchmark/scenario/state surface.
          </p>
        </div>
        <Card>
          <CardHeader class="pb-2">
            <CardDescription>Measured rows</CardDescription>
            <CardTitle>{model().summary?.ok_rows?.toLocaleString()}</CardTitle>
          </CardHeader>
          <CardContent class="text-sm text-muted-foreground">
            {model().summary?.profiles} profiles / {model().summary?.benchmarks} benchmarks
          </CardContent>
        </Card>
      </header>

      <Card>
        <CardContent class="grid gap-4 pt-6 xl:grid-cols-[minmax(390px,1fr)_minmax(390px,1fr)_190px_280px] xl:items-end">
          <ProfilePicker title="Baseline" profiles={model().profiles} selected={profileA()} onChange={setProfileA} />
          <ProfilePicker title="Compared" profiles={model().profiles} selected={profileB()} onChange={setProfileB} />
          <div class="space-y-2">
            <label for={metricId} class="text-sm font-medium leading-none">Metric</label>
            <Select id={metricId} name="metric" value={metric()} onInput={(event) => setMetric(event.currentTarget.value as MetricId)}>
              <For each={metricOptions}>{(option) => <option value={option.id}>{option.label}</option>}</For>
            </Select>
          </div>
          <fieldset class="space-y-2">
            <legend class="text-sm font-medium leading-none">Suites</legend>
            <div class="flex flex-wrap gap-2">
              <For each={Object.keys(suiteLabels)}>
                {(suite) => (
                  <CheckboxPill
                    id={`${suiteId}-${suite}`}
                    checked={suiteFilter().has(suite)}
                    disabled={suiteFilter().size === 1 && suiteFilter().has(suite)}
                    label={suiteLabels[suite]}
                    onChange={(checked) => setSuiteChecked(suite, checked)}
                  />
                )}
              </For>
            </div>
          </fieldset>
        </CardContent>
      </Card>

      <nav class="flex flex-wrap gap-2" aria-label="Comparison presets">
        <Button type="button" variant="outline" size="sm" onClick={() => setPreset("solc-latest-viair-runs200", "vyper-latest-gas")}>latest solc viaIR vs latest Vyper gas</Button>
        <Button type="button" variant="outline" size="sm" onClick={() => setPreset("solc-latest-viair-runs200", "vyper-latest-gas-venom")}>latest solc viaIR vs Vyper gas Venom</Button>
        <Button type="button" variant="outline" size="sm" onClick={() => setPreset("solc-latest-viair-runs200", "vyper-0.5.0a1-gas")}>latest solc viaIR vs Vyper 0.5 gas</Button>
        <Button type="button" variant="outline" size="sm" onClick={() => setPreset("solc-latest-legacy-runs200", "solc-latest-viair-runs200")}>solc legacy vs viaIR</Button>
      </nav>

      <section class="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <MetricCard
          title="Geomean delta"
          value={formatDelta(aggregate().geomean)}
          tone={deltaClass(aggregate().geomean)}
          note={`${profileLabel(model().profiles, profileB())} vs ${profileLabel(model().profiles, profileA())}`}
        />
        <MetricCard title="Comparable rows" value={String(aggregate().count)} note="same scenario/state/profile basis" />
        <MetricCard title="Win / tie / loss" value={`${aggregate().cheaper} / ${aggregate().tie} / ${aggregate().costlier}`} note={`tie band +/-${(tieBand() * 100).toFixed(0)}%`} />
        <MetricCard title="Compile OK" value={`${compileSummary(model(), profileB())?.successful_artifacts ?? 0}/${compileSummary(model(), profileB())?.attempted_artifacts ?? 0}`} note={profileB()} />
      </section>

      <section class="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle class="text-lg">Suite scorecard</CardTitle>
          </CardHeader>
          <CardContent class="space-y-4">
            <VegaChart spec={suiteChartSpec(suiteRows())} />
            <Table>
              <TableHeader><TableRow><TableHead>Suite</TableHead><TableHead>Delta</TableHead><TableHead>Rows</TableHead><TableHead>Win/tie/loss</TableHead></TableRow></TableHeader>
              <TableBody>
                <For each={suiteRows()}>
                  {(entry) => (
                    <TableRow>
                      <TableCell>{suiteLabels[entry.suite] ?? entry.suite}</TableCell>
                      <TableCell><span class={`metric-cell ${deltaClass(entry.geomean)}`}>{formatDelta(entry.geomean)}</span></TableCell>
                      <TableCell>{entry.count}</TableCell>
                      <TableCell>{entry.cheaper} / {entry.tie} / {entry.costlier}</TableCell>
                    </TableRow>
                  )}
                </For>
              </TableBody>
            </Table>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle class="text-lg">Reliability</CardTitle>
            <CardDescription>Compile failures remain first-class data. Failed profiles are not silently dropped.</CardDescription>
          </CardHeader>
          <CardContent>
            <ReliabilityTable model={model()} profiles={[profileA(), profileB()]} />
          </CardContent>
        </Card>
      </section>

      <Card>
        <CardHeader>
          <CardTitle class="text-lg">Head-to-head scenario deltas</CardTitle>
          <CardDescription>Bars show the compared profile delta against the baseline profile. Negative means cheaper, smaller, or faster for the selected metric.</CardDescription>
        </CardHeader>
        <CardContent>
          <VegaChart spec={deltaChartSpec(comparison())} />
        </CardContent>
      </Card>

      <section class="grid gap-4 lg:grid-cols-2">
        <DeltaTable title="Largest improvements" rows={comparison().filter((row) => row.deltaPct < 0).slice(0, 12)} />
        <DeltaTable title="Largest regressions" rows={comparison().filter((row) => row.deltaPct > 0).slice(0, 12)} />
      </section>

      <Card id="versions">
        <CardHeader>
          <CardTitle class="text-lg">Compiler version axis</CardTitle>
          <CardDescription>Each point compares a historical profile to the latest profile with the same compiler line and config.</CardDescription>
        </CardHeader>
        <CardContent>
          <VegaChart spec={versionChartSpec(versionRows())} />
        </CardContent>
      </Card>

      <section class="grid gap-4 lg:grid-cols-2" id="methodology">
        <Card>
          <CardHeader>
            <CardTitle class="text-lg">Methodology scope</CardTitle>
          </CardHeader>
          <CardContent>
            <ul class="list-disc space-y-2 pl-5 text-sm text-muted-foreground">
              <li>Gas is Foundry internal-call harness gas, not measured signed transaction gas.</li>
              <li>Runtime bytecode comparisons use stripped bytecode where available.</li>
              <li>Real-derived rows are benchmark models with <code>production_equivalence=false</code>.</li>
              <li>Vyper Venom uses <code>--experimental-codegen</code>; Vyper 0.5.0a1 is pre-release.</li>
            </ul>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle class="text-lg">Data exports</CardTitle>
          </CardHeader>
          <CardContent class="space-y-2 text-sm">
            <p><a class="text-primary underline-offset-4 hover:underline" href="./report-model.json">report-model.json</a></p>
            <p><a class="text-primary underline-offset-4 hover:underline" href="../normalized/results.json">normalized results.json</a></p>
            <p><a class="text-primary underline-offset-4 hover:underline" href="../normalized/run-manifest.json">run-manifest.json</a></p>
            <p><a class="text-primary underline-offset-4 hover:underline" href="../raw/foundry-gas.jsonl">raw Foundry gas JSONL</a></p>
          </CardContent>
        </Card>
      </section>
    </main>
  );
}

function ProfilePicker(props: { title: string; profiles: Json[]; selected: string; onChange: (profileId: string) => void }) {
  const pickerId = createUniqueId();
  const selectedProfile = createMemo(() => profileById(props.profiles, props.selected) ?? props.profiles[0]);
  const knobs = createMemo(() => profileKnobs(selectedProfile()));
  const languageProfiles = createMemo(() => props.profiles.filter((profile) => profile.language === knobs().language));
  const versionProfiles = createMemo(() =>
    languageProfiles().filter((profile) => profileVersionKey(profile) === knobs().versionKey),
  );
  const optimizerProfiles = createMemo(() =>
    versionProfiles().filter((profile) => profileOptimizer(profile) === knobs().optimizer),
  );

  const languages = createMemo(() =>
    optionValues(props.profiles, (profile) => String(profile.language)).sort((a, b) => {
      const order = ["solidity", "vyper"];
      return order.indexOf(a) - order.indexOf(b);
    }),
  );
  const versions = createMemo(() =>
    optionValues(languageProfiles(), profileVersionKey).sort((a, b) => versionRank(b) - versionRank(a)),
  );
  const versionLabels = createMemo(() => {
    const labels = new Map<string, string>();
    for (const profile of languageProfiles()) labels.set(profileVersionKey(profile), profileVersionLabel(profile));
    return labels;
  });
  const optimizers = createMemo(() =>
    optionValues(versionProfiles(), profileOptimizer).sort((a, b) => optimizerRank(a) - optimizerRank(b)),
  );
  const venomAvailable = createMemo(() => optimizerProfiles().some((profile) => profile.experimental_codegen));

  const choose = (patch: Partial<ProfileKnobs>) => {
    const next = { ...knobs(), ...patch };
    props.onChange(resolveProfile(props.profiles, next));
  };

  const chooseLanguage = (language: string) => {
    props.onChange(defaultProfileForLanguage(props.profiles, language));
  };

  return (
    <fieldset class="min-w-0 space-y-2">
      <legend class="text-sm font-medium leading-none">{props.title}</legend>
      <div class="grid gap-2 md:grid-cols-[1.05fr_1fr_1fr_auto] md:items-end">
        <div class="space-y-1">
          <label for={`${pickerId}-compiler`} class="text-[10px] font-semibold uppercase text-muted-foreground">Compiler</label>
          <Select
            id={`${pickerId}-compiler`}
            name={`${props.title.toLowerCase()}-compiler`}
            aria-describedby={`${pickerId}-selected`}
            value={knobs().language}
            onInput={(event) => chooseLanguage(event.currentTarget.value)}
          >
            <For each={languages()}>{(language) => <option value={language}>{languageLabels[language] ?? language}</option>}</For>
          </Select>
        </div>
        <div class="space-y-1">
          <label for={`${pickerId}-version`} class="text-[10px] font-semibold uppercase text-muted-foreground">Version</label>
          <Select
            id={`${pickerId}-version`}
            name={`${props.title.toLowerCase()}-version`}
            aria-describedby={`${pickerId}-selected`}
            value={knobs().versionKey}
            onInput={(event) => choose({ versionKey: event.currentTarget.value })}
          >
            <For each={versions()}>{(version) => <option value={version}>{versionLabels().get(version) ?? version}</option>}</For>
          </Select>
        </div>
        <div class="space-y-1">
          <label for={`${pickerId}-optimizer`} class="text-[10px] font-semibold uppercase text-muted-foreground">{knobs().language === "solidity" ? "Codegen" : "Optimize"}</label>
          <Select
            id={`${pickerId}-optimizer`}
            name={`${props.title.toLowerCase()}-optimizer`}
            aria-describedby={`${pickerId}-selected`}
            value={knobs().optimizer}
            onInput={(event) => choose({ optimizer: event.currentTarget.value })}
          >
            <For each={optimizers()}>{(optimizer) => <option value={optimizer}>{optimizerLabels[optimizer] ?? optimizer}</option>}</For>
          </Select>
        </div>
        <Show when={knobs().language === "vyper"}>
          <CheckboxPill
            id={`${pickerId}-venom`}
            checked={knobs().experimental}
            disabled={!venomAvailable() && !knobs().experimental}
            label="Venom"
            title={venomAvailable() ? "Use --experimental-codegen" : "Venom is not available for this version/config"}
            onChange={(checked) => choose({ experimental: checked })}
          />
        </Show>
      </div>
      <p id={`${pickerId}-selected`} class="truncate text-xs text-muted-foreground" aria-live="polite"><code>{props.selected}</code></p>
    </fieldset>
  );
}

function optimizerRank(optimizer: string): number {
  const order = ["noopt", "legacy", "viaIR", "default", "none", "gas", "codesize"];
  const index = order.indexOf(optimizer);
  return index === -1 ? order.length : index;
}

function MetricCard(props: { title: string; value: string; note?: string; tone?: string }) {
  return (
    <Card>
      <CardHeader class="pb-2">
        <CardDescription>{props.title}</CardDescription>
        <CardTitle class={cn("w-fit rounded-md px-2 py-1 text-3xl tabular-nums", props.tone)}>{props.value}</CardTitle>
      </CardHeader>
      <CardContent class="text-xs text-muted-foreground">{props.note}</CardContent>
    </Card>
  );
}

function ReliabilityTable(props: { model: ReportModel; profiles: string[] }) {
  const rows = () => props.profiles.map((id) => props.model.profiles.find((profile) => profile.id === id)).filter(Boolean) as Json[];
  return (
    <Table>
      <TableHeader><TableRow><TableHead>Profile</TableHead><TableHead>Compiled</TableHead><TableHead>Failures</TableHead><TableHead>Rows</TableHead></TableRow></TableHeader>
      <TableBody>
        <For each={rows()}>
          {(profile) => (
            <TableRow>
              <TableCell><code>{profile.id}</code></TableCell>
              <TableCell>{profile.successful_artifacts}/{profile.attempted_artifacts}</TableCell>
              <TableCell>{profile.failed_artifacts}</TableCell>
              <TableCell>{profile.scenario_rows}</TableCell>
            </TableRow>
          )}
        </For>
      </TableBody>
    </Table>
  );
}

function DeltaTable(props: { title: string; rows: ComparisonRow[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle class="text-lg">{props.title}</CardTitle>
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader><TableRow><TableHead>Scenario</TableHead><TableHead>Delta</TableHead><TableHead>Baseline</TableHead><TableHead>Compared</TableHead></TableRow></TableHeader>
          <TableBody>
          <For each={props.rows}>
            {(row) => (
              <TableRow>
                <TableCell><code>{row.label}</code></TableCell>
                <TableCell><span class={`metric-cell ${deltaClass(row.ratio)}`}>{formatDelta(row.ratio)}</span></TableCell>
                <TableCell>{formatNumber(row.valueA)}</TableCell>
                <TableCell>{formatNumber(row.valueB)}</TableCell>
              </TableRow>
            )}
          </For>
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

render(() => <App />, document.getElementById("root")!);

// Runs and studies: the data model behind "Run", the run history and parameter sweeps.
//
// A run records one evaluation of a graph: the graph as run (full JSON, a few kB), the
// parameter overrides it was made with, and the results: compact node reports (instruments
// and nodes with a report), a summary of every output port (`ports`: optical average power;
// electrical mean, swing and RMS noise; data symbol count), optionally eye densities and
// signal previews. A study is a list of runs: a single saved run, the
// run history of a session, or a sweep (runs generated from one base graph and a parameter
// grid). One JSON file format holds all of them; CSV gives one row per run, with the report
// metrics and optionally the port values as columns.

export const FORMAT = 'lightstone-study';
export const FORMAT_VERSION = 1;

export function newStudy(app = {}) {
  return { format: FORMAT, version: FORMAT_VERSION, created: new Date().toISOString(), app, runs: [] };
}

/**
 * A run from an engine result. `opts`: { label, note, overrides, sweep, eyes, previews }.
 * Eye densities (Uint32Array) are stored as plain arrays so the file is readable everywhere.
 */
export function makeRun(graph, summary, opts = {}) {
  const run = {
    id: opts.id ?? `run-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
    label: opts.label ?? '',
    note: opts.note ?? '',
    created: new Date().toISOString(),
    graph: JSON.parse(JSON.stringify(graph)),
    overrides: opts.overrides ?? {},
    ok: summary.ok,
    issues: summary.issues ?? [],
    warnings: summary.warnings ?? [],
    ms: summary.ms,
    reports: JSON.parse(JSON.stringify(summary.reports ?? {})),
    ports: JSON.parse(JSON.stringify(summary.ports ?? {})),
  };
  if (opts.sweep) run.sweep = opts.sweep;
  if (opts.eyes && Object.keys(opts.eyes).length) {
    run.eyes = Object.fromEntries(Object.entries(opts.eyes).map(([k, v]) => [k, Array.from(v)]));
  }
  if (opts.previews && Object.keys(opts.previews).length) {
    run.previews = Object.fromEntries(
      Object.entries(opts.previews).map(([k, p]) => [k, p && Object.fromEntries(Object.entries(p).map(([a, b]) => [a, ArrayBuffer.isView(b) ? Array.from(b) : b]))])
    );
  }
  return run;
}

/** Apply dotted overrides { "node.path.to.param": value } to a copy of the graph. */
export function withOverrides(graph, overrides) {
  const g = JSON.parse(JSON.stringify(graph));
  for (const [key, value] of Object.entries(overrides)) {
    const [nodeId, ...path] = key.split('.');
    const n = g.nodes.find((x) => x.id === nodeId);
    if (!n) throw new Error(`no node "${nodeId}" for override ${key}`);
    let o = (n.params ??= {});
    for (const k of path.slice(0, -1)) o = o[k] ??= {};
    o[path.at(-1)] = value;
  }
  return g;
}

/** Linear (or logarithmic) grid of `points` values from `start` to `stop`. */
export function grid(start, stop, points, log = false) {
  const n = Math.max(1, Math.round(points));
  if (n === 1) return [start];
  return Array.from({ length: n }, (_, i) => {
    const u = i / (n - 1);
    return log ? start * (stop / start) ** u : start + (stop - start) * u;
  });
}

/** Separator between node id and port name in port-value metric keys ("rx/out.swing"). */
export const PORT_SEP = '/';

/** Port summary fields that become metrics (type, quantity, flags and alphabet size do not). */
const PORT_FIELDS = ['average_power', 'mean', 'swing', 'noise_rms', 'symbols'];

/**
 * Port values of a run as { "node/port.field": number }: the summaries every run records for
 * every output port (optical average power; electrical mean, swing, RMS noise; data symbol
 * count), independent of the instruments placed in the graph.
 */
export function portMetrics(run) {
  const out = {};
  for (const [node, ports] of Object.entries(run.ports ?? {})) {
    for (const [port, s] of Object.entries(ports ?? {})) {
      for (const f of PORT_FIELDS) if (typeof s?.[f] === 'number' && isFinite(s[f])) out[`${node}${PORT_SEP}${port}.${f}`] = s[f];
    }
  }
  return out;
}

/**
 * Scalar metrics of a run as { "node.key": number }: the instrument and node reports.
 * Arrays of numbers become key[i] (e.g. ana.eye_height[0]); nested objects key.sub; the
 * report type and non-numeric values are left out. With `ports: true` the port values
 * ({@link portMetrics}) are included as well.
 */
export function metrics(run, { ports = false } = {}) {
  const out = ports ? portMetrics(run) : {};
  const walk = (prefix, v) => {
    if (typeof v === 'number' && isFinite(v)) out[prefix] = v;
    else if (typeof v === 'boolean') out[prefix] = v ? 1 : 0;
    else if (Array.isArray(v)) v.forEach((x, i) => (typeof x === 'number' || typeof x === 'object') && walk(`${prefix}[${i}]`, x));
    else if (v && typeof v === 'object') for (const [k, x] of Object.entries(v)) walk(`${prefix}.${k}`, x);
  };
  for (const [node, r] of Object.entries(run.reports ?? {})) {
    for (const [k, v] of Object.entries(r)) if (k !== 'type' && k !== 'counts') walk(`${node}.${k}`, v);
  }
  return out;
}

/**
 * Metric keys present in any of `runs` (a study or a list of runs), in a stable order: most
 * useful report metrics first, port values (with `ports: true`) after all report metrics.
 */
export function metricKeys(runs, { ports = false } = {}) {
  const list = Array.isArray(runs) ? runs : runs.runs;
  const keys = new Set();
  const portKeys = new Set();
  for (const r of list) {
    for (const k of Object.keys(metrics(r))) keys.add(k);
    if (ports) for (const k of Object.keys(portMetrics(r))) portKeys.add(k);
  }
  const rank = (k) => (/\.(ser|tdecq_db|ber)$/.test(k) ? 0 : /\.(eye_height|average_power|emb_length)/.test(k) ? 1 : 2);
  const sorted = [...keys].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
  return [...sorted, ...[...portKeys].filter((k) => !keys.has(k)).sort((a, b) => a.localeCompare(b))];
}

const csvCell = (v) => {
  if (v === null || v === undefined) return '';
  const s = typeof v === 'number' ? String(v) : String(v);
  return /[",\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s;
};

/** CSV with one row per run: label, created, overrides, then the chosen metric columns
 *  (report metrics by default; pass metricKeys(study, { ports: true }) for port values). */
export function toCSV(study, columns = metricKeys(study)) {
  const overrideKeys = [...new Set(study.runs.flatMap((r) => Object.keys(r.overrides ?? {})))];
  const head = ['run', 'label', 'created', 'ok', ...overrideKeys, ...columns];
  const rows = study.runs.map((r, i) => {
    const m = metrics(r, { ports: true });
    return [i + 1, r.label, r.created, r.ok ? 1 : 0, ...overrideKeys.map((k) => r.overrides?.[k]), ...columns.map((k) => m[k])];
  });
  return [head, ...rows].map((row) => row.map(csvCell).join(',')).join('\n') + '\n';
}

/** Parse a study file; accepts a single run too. Throws on anything else. */
export function parseStudy(text) {
  const d = JSON.parse(text);
  if (d?.format === FORMAT && Array.isArray(d.runs)) {
    if (d.version > FORMAT_VERSION) throw new Error(`study format version ${d.version} is newer than this app (${FORMAT_VERSION})`);
    return d;
  }
  if (d?.graph && d?.reports) return { ...newStudy(), runs: [d] };
  throw new Error('not a Lightstone result file');
}

/** Differences between runs for a comparison table: rows whose values are not all equal. */
export function compare(runs, keys) {
  const ms = runs.map((r) => metrics(r, { ports: true }));
  return keys.map((k) => {
    const vals = ms.map((m) => m[k]);
    const defined = vals.filter((v) => v !== undefined);
    const differs = defined.length !== vals.length || defined.some((v) => Math.abs(v - defined[0]) > 1e-12 * Math.max(1, Math.abs(defined[0])));
    return { key: k, values: vals, differs };
  });
}

// Unit tests of the run/study model (run: npm test).
import test from 'node:test';
import assert from 'node:assert/strict';
import { compare, grid, makeRun, metricKeys, metrics, newStudy, parseStudy, portMetrics, toCSV, withOverrides } from '../src/lib/graph/study.js';
import { portText, si } from '../src/lib/graph/catalog.js';

const graph = { settings: { sps: 8 }, nodes: [{ id: 'el1', kind: 'custom_fiber', params: { length: 100, profile: { alpha: 2.0777 } } }], wires: [] };
const summary = (ser, len) => ({
  ok: true,
  ms: 12,
  issues: [],
  warnings: [],
  reports: {
    ana: { type: 'pam4_analysis', ser, eye_height: [0.01, 0.02, 0.03], ser_method: 'semi_analytic', ser_resolution: null },
    el1: { type: 'fiber_span', modes: 90, emb_length: 6.5e12, guided_fraction: 0.99 + len * 0 },
  },
});

test('overrides apply to a copy, including nested paths', () => {
  const g = withOverrides(graph, { 'el1.length': 50, 'el1.profile.alpha': 2.0 });
  assert.equal(g.nodes[0].params.length, 50);
  assert.equal(g.nodes[0].params.profile.alpha, 2.0);
  assert.equal(graph.nodes[0].params.length, 100, 'original unchanged');
  assert.throws(() => withOverrides(graph, { 'nope.length': 1 }), /no node/);
});

test('grids', () => {
  assert.deepEqual(grid(0, 150, 4), [0, 50, 100, 150]);
  assert.deepEqual(grid(5, 5, 1), [5]);
  const g = grid(1, 100, 3, true);
  assert.ok(Math.abs(g[1] - 10) < 1e-12);
});

test('metrics flatten scalars and arrays', () => {
  const r = makeRun(graph, summary(1e-3, 0), { label: 'a', eyes: { eye: new Uint32Array([1, 2]) } });
  const m = metrics(r);
  assert.equal(m['ana.ser'], 1e-3);
  assert.equal(m['ana.eye_height[2]'], 0.03);
  assert.equal(m['el1.modes'], 90);
  assert.ok(!('ana.ser_method' in m) && !('ana.type' in m));
  assert.deepEqual(r.eyes.eye, [1, 2], 'eye stored as a plain array');
});

test('study, CSV and round trip', () => {
  const s = newStudy({ version: 'test' });
  s.runs.push(makeRun(graph, summary(1e-3, 50), { label: 'L = 50 m', overrides: { 'el1.length': 50 } }));
  s.runs.push(makeRun(graph, summary(2e-2, 100), { label: 'L = 100, "long"', overrides: { 'el1.length': 100 } }));
  const keys = metricKeys(s);
  assert.equal(keys[0], 'ana.ser', 'SER first');
  const csv = toCSV(s, ['ana.ser', 'el1.emb_length']).trim().split('\n');
  assert.equal(csv[0], 'run,label,created,ok,el1.length,ana.ser,el1.emb_length');
  assert.match(csv[2], /^2,"L = 100, ""long""",[^,]+,1,100,0\.02,6500000000000$/);
  const back = parseStudy(JSON.stringify(s));
  assert.equal(back.runs.length, 2);
  assert.equal(parseStudy(JSON.stringify(s.runs[0])).runs.length, 1, 'a single run is accepted');
  assert.throws(() => parseStudy('{"nodes": []}'), /not a Lightstone result/);
  assert.throws(() => parseStudy(JSON.stringify({ ...s, version: 99 })), /newer/);
});

test('compare marks differing metrics', () => {
  const a = makeRun(graph, summary(1e-3, 0));
  const b = makeRun(graph, summary(2e-3, 0));
  const rows = compare([a, b], ['ana.ser', 'el1.modes']);
  assert.deepEqual(rows.map((r) => r.differs), [true, false]);
});

const withPorts = (swing) => ({
  ...summary(1e-3, 0),
  ports: {
    'tx/a': { out: { type: 'optical', average_power: 1e-3 } },
    rx: { out: { type: 'electrical', quantity: 'voltage', mean: 0.01, swing, noise_rms: 1e-3, gaussian_exact: true } },
    prbs: { out: { type: 'data', symbols: 8191, levels: 4 } },
  },
});

test('port values are recorded and kept apart from report metrics', () => {
  const r = makeRun(graph, withPorts(0.2));
  assert.equal(r.ports.rx.out.swing, 0.2);
  const pm = portMetrics(r);
  assert.deepEqual(Object.keys(pm).sort(), ['prbs/out.symbols', 'rx/out.mean', 'rx/out.noise_rms', 'rx/out.swing', 'tx/a/out.average_power']);
  assert.ok(!('rx/out.swing' in metrics(r)), 'reports only by default');
  assert.equal(metrics(r, { ports: true })['rx/out.swing'], 0.2);
  const s = { ...newStudy(), runs: [r, makeRun(graph, withPorts(0.3))] };
  assert.ok(!metricKeys(s).some((k) => k.includes('/')), 'default keys: instruments only');
  const all = metricKeys(s, { ports: true });
  assert.equal(all[0], 'ana.ser', 'reports first');
  assert.equal(all.at(-1), 'tx/a/out.average_power', 'port values after reports');
  assert.match(toCSV(s, all).split('\n')[0], /rx\/out\.swing/);
  assert.doesNotMatch(toCSV(s).split('\n')[0], /rx\/out/);
  const rows = compare(s.runs, ['rx/out.swing', 'rx/out.mean']);
  assert.deepEqual(rows.map((x) => x.differs), [true, false]);
  assert.deepEqual(portMetrics(makeRun(graph, summary(1e-3, 0))), {}, 'old runs without ports');
});

test('port value text', () => {
  assert.equal(portText({ type: 'optical', average_power: 1e-3 }), '0.00 dBm');
  assert.equal(portText({ type: 'electrical', quantity: 'voltage', mean: 0.012, swing: 0.25, noise_rms: 1.1e-3, gaussian_exact: true }), 'mean 12 mV · swing 250 mV · noise 1.1 mV rms');
  assert.match(portText({ type: 'electrical', quantity: 'current', mean: 5e-4, swing: 1e-3, noise_rms: 2e-6, gaussian_exact: false }), /500 µA.*1 mA.*2 µA rms \(non-Gaussian\)/);
  assert.equal(portText({ type: 'data', symbols: 8191 }), '8191 symbols');
  assert.equal(si(0.5), '0.5');
  assert.equal(portText(undefined), '');
});

test('error rates: a zero SER is shown as below the floor, or as noise-free', async () => {
  const { errorRates } = await import('../src/lib/graph/catalog.js');
  const base = { ser_method: 'semi_analytic', ser_floor: 2.26e-19, level_mean: [0, 1, 2, 3] };
  assert.deepEqual(errorRates({ ...base, ser: 3.3e-2, ber: 1.6e-2, noise_rms: 1e-3 }), { ser: '3.3e-2', ber: '1.6e-2' });
  assert.deepEqual(errorRates({ ...base, ser: 0, ber: 0, noise_rms: 1e-3 }), { ser: '< 2.3e-19', ber: '< 1.1e-19' });
  assert.deepEqual(errorRates({ ...base, level_mean: [0, 1], ser: 0, ber: 0, noise_rms: 1e-3 }), { ser: '< 2.3e-19', ber: '< 2.3e-19' });
  assert.deepEqual(errorRates({ ...base, ser: 0, ber: 0, noise_rms: 0 }), { ser: '0 (no noise)', ber: '0 (no noise)' });
  const mc = { ser_method: 'monte_carlo', ser: 0, ber: 0, ser_resolution: 1 / 8191, ser_floor: 1 / 8191, noise_rms: 0, level_mean: [0, 1, 2, 3] };
  assert.equal(errorRates(mc).ser, '< 1.2e-4 (counted)');
});

test('driver jitter summary', async () => {
  const { jitterText } = await import('../src/lib/graph/catalog.js');
  assert.equal(jitterText({}), 'no jitter');
  assert.equal(jitterText({ rj_rms: 0, dcd: 0, sj_amplitude: 0 }), 'no jitter');
  assert.equal(jitterText({ rj_rms: 0.3e-12, dcd: 2e-12, sj_amplitude: 5e-12, sj_frequency: 100e6 }), 'RJ 0.3 ps · DCD 2 ps · SJ 5 ps @ 100 MHz');
});

test('WDM and spectrum analyzer texts', async () => {
  const { badge, channelText, display, trace } = await import('../src/lib/graph/catalog.js');
  assert.equal(channelText(undefined), '4 ch · 850…940 nm');
  assert.equal(channelText([910e-9]), '1 ch · 910 nm');
  assert.equal(badge('wdm_demux', {}, null), '4 ch · 850…940 nm · 1.0 dB');
  assert.equal(badge('bandpass_filter', {}, null), '850 nm · 22 nm wide · 0.5 dB');
  const osa = { wavelength: [8.2e-7, 8.5e-7, 9.7e-7], power_dbm: [-90, -17.3, -90], peak: { wavelength: 8.5e-7, power_dbm: -17.3 }, total_power_dbm: -0.35, sources: [{}], rbw: 1e-10 };
  assert.equal(display('optical_spectrum_analyzer', {}, osa).main.value, '-17.30');
  const t = trace('optical_spectrum_analyzer', osa);
  assert.deepEqual(t.range, [-70, -10]);
  assert.equal(t.traces.length, 1);
  const esa = { frequency: [1e9, 5e10], signal_db: [-80, -120], noise_db: [-100, -130], snr_db: 23.4, signal_rms: 0.1, noise_rms: 0.007, unit: 'V', rbw: 2e8 };
  assert.equal(display('electrical_spectrum_analyzer', {}, esa).main.value, '23.4');
  assert.equal(trace('electrical_spectrum_analyzer', esa).traces.length, 2);
});

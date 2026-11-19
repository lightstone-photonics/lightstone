// Unit tests of the graph editor model (run: npm test).
import test from 'node:test';
import assert from 'node:assert/strict';
import { connects } from '../src/lib/graph/catalog.js';
import {
  addNode, autoLayout, autoWire, connect, connectionProblem, emptyGraph, insertSplitter,
  opticalConflict, positions, removeNode, rename,
} from '../src/lib/graph/model.js';

const ports = {
  src: { inputs: [], outputs: [{ name: 'out', type: 'data', monitor: false }] },
  fib: { inputs: [{ name: 'in', type: 'optical', monitor: false }], outputs: [{ name: 'out', type: 'optical', monitor: false }] },
  pin: { inputs: [{ name: 'in', type: 'optical', monitor: false }], outputs: [{ name: 'out', type: 'electrical (current)', monitor: false }] },
  pin2: { inputs: [{ name: 'in', type: 'optical', monitor: false }], outputs: [{ name: 'out', type: 'electrical (current)', monitor: false }] },
  pm: { inputs: [{ name: 'in', type: 'optical', monitor: true }], outputs: [] },
  tia: { inputs: [{ name: 'in', type: 'electrical (current)', monitor: false }], outputs: [{ name: 'out', type: 'electrical (voltage)', monitor: false }] },
};

test('port types connect like PortType::connects_to', () => {
  assert.ok(connects('optical', 'optical'));
  assert.ok(connects('electrical (current)', 'electrical'));
  assert.ok(connects('electrical (voltage)', 'electrical (voltage)'));
  assert.ok(!connects('electrical', 'electrical (current)'));
  assert.ok(!connects('electrical (voltage)', 'electrical (current)'));
  assert.ok(!connects('data', 'optical'));
});

test('add, connect, rename, remove', () => {
  const g = emptyGraph();
  const a = addNode(g, 'custom_fiber', { length: 50 }, [0, 0]);
  const b = addNode(g, 'custom_fiber', {}, [300, 0]);
  assert.deepEqual([a, b], ['fib', 'fib2']);
  const c = addNode(g, 'attenuator', {}, [300, 0]);
  assert.deepEqual(positions(g)[c], [300, 110], 'new nodes do not overlap');
  removeNode(g, c);
  connect(g, 'fib.out', 'fib2.in');
  connect(g, 'fib.out', 'fib2.in');
  assert.equal(g.wires.length, 1, 'an input has one source');
  assert.equal(rename(g, 'fib2', 'span'), '');
  assert.deepEqual(g.wires, [{ from: 'fib.out', to: 'span.in' }]);
  assert.ok(positions(g).span && !positions(g).fib2);
  assert.match(rename(g, 'span', 'fib'), /exists/);
  assert.match(rename(g, 'span', 'a.b'), /letters/);
  removeNode(g, 'fib');
  assert.deepEqual(g.wires, []);
});

test('connection checks and optical fan-out', () => {
  const g = { nodes: [], wires: [{ from: 'fib.out', to: 'pin.in' }], layout: { positions: { fib: [0, 0] } } };
  assert.equal(connectionProblem(g, ports, 'src.out', 'fib.in'), 'Cannot connect data output to optical input.');
  assert.equal(connectionProblem(g, ports, 'fib.out', 'fib.in'), 'A node cannot feed itself.');
  assert.equal(connectionProblem(g, ports, 'pin.out', 'tia.in'), '');
  assert.equal(opticalConflict(g, ports, 'fib.out', 'pm.in'), null, 'monitor inputs may share light');
  assert.equal(opticalConflict(g, ports, 'fib.out', 'pin2.in'), 'pin.in');
  const id = insertSplitter(g, 'fib.out', 'pin.in', 'pin2.in', { ratios: [0.5, 0.5] });
  assert.deepEqual(new Set(g.wires.map((w) => `${w.from}>${w.to}`)), new Set([`fib.out>${id}.in`, `${id}.out0>pin.in`, `${id}.out1>pin2.in`]));
});

test('auto layout follows the data flow and survives loops', () => {
  const g = { nodes: ['a', 'b', 'c', 'd'].map((id) => ({ id, kind: 'attenuator', params: {} })), wires: [
    { from: 'a.out', to: 'b.in' }, { from: 'b.out', to: 'c.in' }, { from: 'a.out', to: 'd.in' }, { from: 'c.out', to: 'b.in2' },
  ] };
  autoLayout(g);
  const p = positions(g);
  assert.ok(p.a[0] < p.b[0] && p.b[0] < p.c[0], JSON.stringify(p));
  assert.equal(p.d[0], p.b[0]);
  // A chain of 7 wraps after 5 columns into a second band.
  const chain = { nodes: [...'abcdefg'].map((id) => ({ id, kind: 'attenuator', params: {} })), wires: [...'abcdef'].map((x, i) => ({ from: `${x}.out`, to: `${'abcdefg'[i + 1]}.in` })) };
  autoLayout(chain);
  const q = positions(chain);
  assert.deepEqual([q.e[0] > q.a[0], q.f[0], q.f[1] > q.a[1]], [true, 0, true]);
});

test('auto-wire picks a free compatible output', () => {
  const g = { nodes: [], wires: [{ from: 'fib.out', to: 'pin.in' }] };
  const pinKind = { inputs: [{ name: 'in', type: 'optical', monitor: false }] };
  const pmKind = { inputs: [{ name: 'in', type: 'optical', monitor: true }] };
  assert.equal(autoWire(g, ports, 'fib', pinKind), null, 'used optical output');
  assert.deepEqual(autoWire(g, ports, 'fib', pmKind), { from: 'fib.out', input: 'in' });
  assert.deepEqual(autoWire({ wires: [] }, ports, 'pin', { inputs: [{ name: 'in', type: 'electrical', monitor: false }] }), { from: 'pin.out', input: 'in' });
});

import { addFrame, frames, inFrame, removeFrame, zoneFrames } from '../src/lib/graph/model.js';
import { ZONES, ZONE_OF } from '../src/lib/graph/catalog.js';

test('frames: zones of a wrapped lane, membership, removal', () => {
  const kinds = { src: 'prbs13q', drv: 'driver', vcsel: 'vcsel', con: 'connector', fib: 'mmf', pin: 'pin', tia: 'tia', rxf: 'bessel_thomson', rx: 'receiver_output' };
  const g = { nodes: Object.entries(kinds).map(([id, kind]) => ({ id, kind, params: {} })), wires: [] };
  const ids = Object.keys(kinds);
  for (let i = 0; i < ids.length - 1; i++) g.wires.push({ from: `${ids[i]}.out`, to: `${ids[i + 1]}.in` });
  autoLayout(g);
  zoneFrames(g, (k) => ZONE_OF[k], ZONES);
  assert.deepEqual(frames(g).map((f) => f.label), ['Transmitter', 'Channel', 'Receiver front end', 'Data recovery']);
  const tx = frames(g)[0];
  assert.deepEqual(inFrame(g, tx).sort(), ['drv', 'src', 'vcsel']);
  const id = addFrame(g, 'Mine', [0, 0, 10, 10]);
  assert.equal(frames(g).length, 5);
  removeFrame(g, id);
  assert.equal(frames(g).length, 4);
});

import { duplicateFrame } from '../src/lib/graph/model.js';

test('duplicate a frame like a lane', () => {
  const g = { nodes: [
    { id: 'src', kind: 'prbs13q', params: {} },
    { id: 'vcsel', kind: 'vcsel', params: { seed: 7 } },
    { id: 'fib', kind: 'custom_fiber', params: { length: 50 } },
    { id: 'pin', kind: 'pin', params: { noise_seed: 3 } },
  ], wires: [{ from: 'src.out', to: 'vcsel.in' }, { from: 'vcsel.out', to: 'fib.in' }, { from: 'fib.out', to: 'pin.in' }],
  layout: { positions: { src: [0, 0], vcsel: [300, 0], fib: [600, 0], pin: [900, 0] } } };
  addFrame(g, 'Lane', [280, -20, 900, 100]); // vcsel, fib, pin (not src)
  const fid = duplicateFrame(g, frames(g)[0].id);
  assert.equal(g.nodes.length, 7);
  const copy = (id) => g.nodes.find((n) => n.id === id);
  assert.deepEqual(['vcsel2', 'fib2', 'pin2'].map((id) => !!copy(id)), [true, true, true]);
  assert.equal(copy('vcsel2').params.seed, null, 'noise seeds cleared');
  assert.equal(copy('pin2').params.noise_seed, null);
  assert.equal(copy('fib2').params.length, 50);
  const ws = new Set(g.wires.map((w) => `${w.from}>${w.to}`));
  for (const w of ['src.out>vcsel2.in', 'vcsel2.out>fib2.in', 'fib2.out>pin2.in']) assert.ok(ws.has(w), w);
  assert.ok(positions(g).fib2[1] > positions(g).fib[1]);
  assert.equal(frames(g).find((f) => f.id === fid).label, 'Lane (copy)');
});

test('renamed kinds: an old "decision" node loads as "receiver_output"', async () => {
  const { normalizeKinds } = await import('../src/lib/graph/model.js');
  const g = normalizeKinds({ nodes: [{ id: 'dec', kind: 'decision' }, { id: 'pin', kind: 'pin' }], wires: [] });
  assert.deepEqual(g.nodes.map((n) => n.kind), ['receiver_output', 'pin']);
});

test('a multimode preset converts to a custom fiber with its values; physics vs. own parameters', async () => {
  const { convertToCustomFiber } = await import('../src/lib/graph/model.js');
  const { isFiberPhysics } = await import('../src/lib/graph/catalog.js');
  const preset = { length: 100, core_radius: 25e-6, na: 0.2, profile: { kind: 'graded', alpha: 2.0777 }, attenuation_db_per_km: 3.5 };
  const g = { nodes: [{ id: 'el1', kind: 'mmf', params: { type: 'om4', length: 60, extra_loss_db: 0.5 } }], wires: [] };
  const before = convertToCustomFiber(g, 'el1', preset);
  assert.deepEqual(before, { kind: 'mmf', params: { type: 'om4', length: 60, extra_loss_db: 0.5 } });
  assert.equal(g.nodes[0].kind, 'custom_fiber');
  assert.deepEqual(g.nodes[0].params, { ...preset, length: 60, extra_loss_db: 0.5 });
  assert.ok(isFiberPhysics('profile.alpha', 2.0) && isFiberPhysics('na', 0.2) && isFiberPhysics('attenuation_db_per_km', 4));
  assert.ok(!isFiberPhysics('length', 50) && !isFiberPhysics('attenuation_db_per_km', 3.5) && !isFiberPhysics('type', 'om3'));
});

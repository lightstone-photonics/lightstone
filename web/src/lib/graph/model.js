// Pure operations on the graph JSON document (the same format as Python's Graph.to_json):
//   { settings, nodes: [{ id, kind, params }], wires: [{ from: "node.port", to: "node.port" }],
//     layout: { positions: { id: [x, y] }, sizes: { id: [w, h] }, frames: [...] } }
// (`sizes` only for nodes the user resized; the engine ignores `layout`).
// `ports` maps node id → { inputs: [{name, type, monitor}], outputs: [...] }.

import { PREFIX, connects, domain } from './catalog.js';

export const split = (ref) => {
  const i = ref.indexOf('.');
  return [ref.slice(0, i), ref.slice(i + 1)];
};

/** Component kinds that were renamed: old name → current name (same as the engine's). */
export const RENAMED_KINDS = { decision: 'receiver_output', fiber: 'custom_fiber' };

/** Replace renamed kinds by their current names, so older saved graphs keep working. */
export function normalizeKinds(g) {
  for (const n of g.nodes ?? []) if (RENAMED_KINDS[n.kind]) n.kind = RENAMED_KINDS[n.kind];
  return g;
}

export function emptyGraph() {
  return { settings: { sps: 8 }, nodes: [], wires: [], layout: { positions: {} } };
}

export function positions(g) {
  g.layout ??= {};
  g.layout.positions ??= {};
  return g.layout.positions;
}

/** Node sizes set by resizing in the editor: { id: [w, h] } (absent = natural size). */
export function nodeSizes(g) {
  g.layout ??= {};
  g.layout.sizes ??= {};
  return g.layout.sizes;
}

/**
 * Turn the multimode preset node `id` into a custom fiber with the preset's values
 * (`preset`: full custom_fiber parameters) and the node's own length and losses. Returns the
 * node's previous { kind, params } for undo.
 */
export function convertToCustomFiber(g, id, preset) {
  const n = g.nodes.find((x) => x.id === id);
  const before = { kind: n.kind, params: JSON.parse(JSON.stringify(n.params ?? {})) };
  const { type, ...own } = n.params ?? {}; // eslint-disable-line no-unused-vars
  n.kind = 'custom_fiber';
  n.params = { ...JSON.parse(JSON.stringify(preset)), ...own };
  return before;
}

export function newId(g, kind) {
  const base = PREFIX[kind] ?? kind;
  const ids = new Set(g.nodes.map((n) => n.id));
  if (!ids.has(base)) return base;
  for (let i = 2; ; i++) if (!ids.has(`${base}${i}`)) return `${base}${i}`;
}

/** `pos`, or the first spot below it that does not overlap an existing node. */
export function freeSpot(g, pos, dx = 250, dy = 110) {
  const taken = Object.values(positions(g));
  let [x, y] = pos;
  while (taken.some(([a, b]) => Math.abs(a - x) < dx && Math.abs(b - y) < dy)) y += dy;
  return [x, y];
}

export function addNode(g, kind, params, pos) {
  const id = newId(g, kind);
  // JSON copy: params may be a Svelte state proxy, which structuredClone cannot copy.
  g.nodes.push({ id, kind, params: JSON.parse(JSON.stringify(params ?? {})) });
  positions(g)[id] = freeSpot(g, pos);
  return id;
}

export function removeNode(g, id) {
  g.nodes = g.nodes.filter((n) => n.id !== id);
  g.wires = g.wires.filter((w) => split(w.from)[0] !== id && split(w.to)[0] !== id);
  delete positions(g)[id];
  delete nodeSizes(g)[id];
}

/** Wire `from` → `to`; an input has at most one source, so an existing wire into `to` is replaced. */
export function connect(g, from, to) {
  g.wires = g.wires.filter((w) => w.to !== to);
  g.wires.push({ from, to });
}

export function disconnect(g, to) {
  g.wires = g.wires.filter((w) => w.to !== to);
}

export function removeWire(g, from, to) {
  g.wires = g.wires.filter((w) => !(w.from === from && w.to === to));
}

/** Rename a node and update its wires, position and size. Returns an error message or ''. */
export function rename(g, oldId, newId_) {
  if (oldId === newId_) return '';
  if (!/^[A-Za-z0-9_-]+$/.test(newId_)) return 'Use letters, digits, "-" and "_" only.';
  if (g.nodes.some((n) => n.id === newId_)) return `A node "${newId_}" exists already.`;
  const re = (ref) => {
    const [n, p] = split(ref);
    return n === oldId ? `${newId_}.${p}` : ref;
  };
  g.nodes.find((n) => n.id === oldId).id = newId_;
  g.wires = g.wires.map((w) => ({ from: re(w.from), to: re(w.to) }));
  const pos = positions(g);
  for (const m of [pos, nodeSizes(g)]) {
    if (m[oldId]) {
      m[newId_] = m[oldId];
      delete m[oldId];
    }
  }
  return '';
}

export function portSpec(ports, ref, side) {
  const [n, p] = split(ref);
  return ports[n]?.[side]?.find((x) => x.name === p);
}

/** Problem with wiring `from` → `to`, or '' if it is allowed. */
export function connectionProblem(g, ports, from, to) {
  const [fn] = split(from);
  const [tn] = split(to);
  if (fn === tn) return 'A node cannot feed itself.';
  const o = portSpec(ports, from, 'outputs');
  const i = portSpec(ports, to, 'inputs');
  if (!o || !i) return 'Unknown port.';
  if (!connects(o.type, i.type)) return `Cannot connect ${o.type} output to ${i.type} input.`;
  return '';
}

/**
 * Optical outputs may feed only one non-monitor input (light cannot be copied). Returns the
 * input already fed by `from` if adding `to` would break that rule, else null.
 */
export function opticalConflict(g, ports, from, to) {
  const o = portSpec(ports, from, 'outputs');
  const i = portSpec(ports, to, 'inputs');
  if (!o || domain(o.type) !== 'optical' || i?.monitor) return null;
  const other = g.wires.find((w) => w.from === from && w.to !== to && !portSpec(ports, w.to, 'inputs')?.monitor);
  return other ? other.to : null;
}

/** Insert a 1×2 splitter so that `from` feeds both `existingTo` and `to`. Returns its id. */
export function insertSplitter(g, from, existingTo, to, params) {
  const [fn] = split(from);
  const pos = positions(g);
  const p = pos[fn] ?? [0, 0];
  const id = addNode(g, 'splitter', params, [p[0] + 120, p[1] + 90]);
  removeWire(g, from, existingTo);
  connect(g, from, `${id}.in`);
  connect(g, `${id}.out0`, existingTo);
  connect(g, `${id}.out1`, to);
  return id;
}

/**
 * Layered layout: column = longest path from a source, row = order within the column.
 * Loops (allowed later, docs/architecture.md, 7.8) are broken at the node that comes first in
 * the node list, so backward wires simply point to the left.
 */
export function autoLayout(g, dx = 280, dy = 120, wrap = 5) {
  const ids = g.nodes.map((n) => n.id);
  const preds = new Map(ids.map((id) => [id, new Set()]));
  for (const w of g.wires) {
    const [a] = split(w.from);
    const [b] = split(w.to);
    if (preds.has(a) && preds.has(b) && a !== b) preds.get(b).add(a);
  }
  const depth = new Map();
  while (depth.size < ids.length) {
    // Nodes whose predecessors are all placed; if none (a loop), take the first unplaced one.
    let ready = ids.filter((id) => !depth.has(id) && [...preds.get(id)].every((p) => depth.has(p)));
    if (!ready.length) ready = [ids.find((id) => !depth.has(id))];
    for (const id of ready) {
      const placed = [...preds.get(id)].filter((p) => depth.has(p)).map((p) => depth.get(p));
      depth.set(id, Math.max(-1, ...placed) + 1);
    }
  }
  // Long chains wrap into bands of `wrap` columns (like lines of text), so they stay
  // readable when the view is fitted to the canvas.
  const rows = new Map();
  const rowOf = new Map();
  for (const id of ids) {
    const c = depth.get(id);
    rowOf.set(id, rows.get(c) ?? 0);
    rows.set(c, (rows.get(c) ?? 0) + 1);
  }
  const bands = Math.floor(Math.max(0, ...depth.values()) / wrap) + 1;
  const bandTop = [0];
  for (let b = 0; b < bands; b++) {
    const height = Math.max(1, ...[...rows].filter(([c]) => Math.floor(c / wrap) === b).map(([, n]) => n));
    bandTop.push(bandTop[b] + height * dy + 0.5 * dy);
  }
  const pos = positions(g);
  for (const id of ids) {
    const c = depth.get(id);
    pos[id] = [(c % wrap) * dx, bandTop[Math.floor(c / wrap)] + rowOf.get(id) * dy];
  }
}

/** Give every node without a position one (to the right of the existing layout). */
export function ensurePositions(g) {
  const pos = positions(g);
  const missing = g.nodes.filter((n) => !pos[n.id]);
  if (!missing.length) return;
  if (missing.length === g.nodes.length) return autoLayout(g);
  const maxX = Math.max(0, ...Object.values(pos).map((p) => p[0]));
  missing.forEach((n, i) => (pos[n.id] = [maxX + 280, i * 120]));
}

/** First free output of `fromId` that can drive the first matching input of a new kind. */
export function autoWire(g, ports, fromId, kindInfo) {
  const outs = ports[fromId]?.outputs ?? [];
  for (const o of outs) {
    const inp = kindInfo.inputs.find((i) => connects(o.type, i.type));
    if (!inp) continue;
    const ref = `${fromId}.${o.name}`;
    const used = g.wires.some((w) => w.from === ref && !portSpec(ports, w.to, 'inputs')?.monitor);
    if (domain(o.type) === 'optical' && used && !inp.monitor) continue;
    return { from: ref, input: inp.name };
  }
  return null;
}

// ---- frames: named areas behind the nodes (layout.frames), e.g. the system-view zones ------

/** Estimated node size before the canvas has measured it [px]. */
export const NODE_SIZE = [250, 60];

export function frames(g) {
  g.layout ??= {};
  g.layout.frames ??= [];
  return g.layout.frames;
}

export function newFrameId(g) {
  const ids = new Set(frames(g).map((f) => f.id));
  for (let i = 1; ; i++) if (!ids.has(`frame${i}`)) return `frame${i}`;
}

/** Bounding box [x, y, w, h] of nodes `ids` with padding (top padding leaves room for the label). */
export function bbox(g, ids, sizes = {}, pad = 12) {
  const pos = positions(g);
  const pts = ids.filter((id) => pos[id]);
  if (!pts.length) return null;
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const id of pts) {
    const [x, y] = pos[id];
    const [w, h] = sizes[id] ?? NODE_SIZE;
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x + w);
    y1 = Math.max(y1, y + h);
  }
  return [x0 - pad, y0 - pad - 18, x1 - x0 + 2 * pad, y1 - y0 + 2 * pad + 18];
}

export function addFrame(g, label, box) {
  const id = newFrameId(g);
  const [x, y, w, h] = box;
  frames(g).push({ id, label, x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) });
  return id;
}

export function removeFrame(g, id) {
  g.layout.frames = frames(g).filter((f) => f.id !== id);
}

/** Node ids whose center lies inside frame `f`. */
export function inFrame(g, f, sizes = {}) {
  const pos = positions(g);
  return g.nodes
    .map((n) => n.id)
    .filter((id) => {
      const p = pos[id];
      if (!p) return false;
      const [w, h] = sizes[id] ?? NODE_SIZE;
      const [cx, cy] = [p[0] + w / 2, p[1] + h / 2];
      return cx >= f.x && cx <= f.x + f.w && cy >= f.y && cy <= f.y + f.h;
    });
}

/**
 * Replace the frames with one per zone (Transmitter, Channel, …) around the nodes of that
 * zone, as in the system view. `zoneOf(kind)` gives a node's zone; nodes without one are
 * left out. Zones are split into separate frames where their nodes lie in different rows of
 * a wrapped layout, so frames do not span unrelated nodes.
 */
export function zoneFrames(g, zoneOf, zones, sizes = {}) {
  g.layout ??= {};
  g.layout.frames = [];
  const pos = positions(g);
  for (const z of zones) {
    const ids = g.nodes.filter((n) => zoneOf(n.kind) === z && pos[n.id]).map((n) => n.id);
    // Group by row band: nodes more than 2 node heights apart vertically go to separate frames.
    const sorted = [...ids].sort((a, b) => pos[a][1] - pos[b][1]);
    const bands = [];
    for (const id of sorted) {
      const last = bands.at(-1);
      if (last && pos[id][1] - pos[last.at(-1)][1] <= 2.2 * NODE_SIZE[1]) last.push(id);
      else bands.push([id]);
    }
    for (const b of bands) addFrame(g, z, bbox(g, b, sizes));
  }
}

/** Parameters holding explicit noise seeds; cleared in copies so their noise is independent. */
const SEED_KEYS = ['seed', 'noise_seed'];

/**
 * Duplicate frame `fid` with the nodes inside it (like adding a lane): nodes get new ids,
 * wires between them are copied, wires from outside into the frame (e.g. a shared pattern
 * source) feed the copies too. Explicit noise seeds are cleared, so the copy draws its own
 * noise (seeds then derive from the new node ids). The copy is placed below the original.
 * Returns the new frame id.
 */
export function duplicateFrame(g, fid, sizes = {}, label) {
  const f = frames(g).find((x) => x.id === fid);
  const members = inFrame(g, f, sizes);
  const pos = positions(g);
  const dy = f.h + 30;
  const idMap = {};
  for (const id of members) {
    const n = g.nodes.find((x) => x.id === id);
    const copy = JSON.parse(JSON.stringify(n));
    // prbs13q's `seed` selects the pattern, not noise: keep it.
    if (n.kind !== 'prbs13q') for (const k of SEED_KEYS) if (copy.params && k in copy.params) copy.params[k] = null;
    copy.id = newId(g, n.kind);
    g.nodes.push(copy);
    idMap[id] = copy.id;
    pos[copy.id] = [pos[id][0], pos[id][1] + dy];
    if (nodeSizes(g)[id]) nodeSizes(g)[copy.id] = [...nodeSizes(g)[id]];
  }
  const re = (ref) => {
    const [n, p] = split(ref);
    return idMap[n] ? `${idMap[n]}.${p}` : ref;
  };
  for (const w of [...g.wires]) {
    const [tn] = split(w.to);
    if (!idMap[tn]) continue;
    g.wires.push({ from: re(w.from), to: re(w.to) });
  }
  const id = newFrameId(g);
  frames(g).push({ ...f, id, y: f.y + dy, label: label ?? `${f.label} (copy)` });
  return id;
}

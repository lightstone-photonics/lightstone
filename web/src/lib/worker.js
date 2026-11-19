// Simulation worker: owns the WASM graph engines so the UI thread never blocks.
import init, { Graph } from './wasm/lightstone_wasm.js';

let graph = null; // the editor's graph (keeps its per-node cache between edits)
let sweepGraph = null; // separate engine for sweeps: keeps the editor's cache intact

async function setup() {
  await init();
  return {
    kinds: JSON.parse(Graph.kinds()),
    referenceGraph: JSON.parse(Graph.lane_graph('')),
    engineVersion: Graph.version(),
    fiberPresets: JSON.parse(Graph.fiber_presets()),
  };
}

const ready = setup();

function eyesOf(g, summary, transfer) {
  const eyes = {};
  for (const [id, r] of Object.entries(summary.reports)) {
    if (r.type === 'eye_diagram') {
      eyes[id] = g.eye(id);
      transfer.push(eyes[id].buffer);
    }
  }
  return eyes;
}

self.onmessage = async (ev) => {
  const info = await ready;
  const msg = ev.data;
  try {
    if (msg.type === 'hello') {
      self.postMessage({ type: 'ready', ...info });
    } else if (msg.type === 'graph') {
      // Run, then collect eye densities, signal previews and the fiber view.
      const json = JSON.stringify(msg.graph);
      if (!graph) graph = new Graph(json);
      else graph.set_graph(json);
      const summary = JSON.parse(graph.run());
      const ports = JSON.parse(graph.ports());
      const transfer = [];
      const eyes = eyesOf(graph, summary, transfer);
      const previews = {};
      if (summary.ok) for (const p of msg.previews ?? []) previews[`${p.node}.${p.port}`] = JSON.parse(graph.preview(p.node, p.port));
      let fiber = null;
      if (summary.ok && msg.fiber) {
        const f = msg.fiber;
        fiber = JSON.parse(graph.fiber_view(f.node));
        if (fiber) {
          fiber.image = graph.facet(f.node, f.kind, f.n, f.extent, f.mode ?? 0);
          Object.assign(fiber, { kind: f.kind, n: f.n, node: f.node });
          transfer.push(fiber.image.buffer);
        }
      }
      self.postMessage({ type: 'graph', id: msg.id, summary, ports, eyes, previews, fiber }, transfer);
    } else if (msg.type === 'sweep_point') {
      const json = JSON.stringify(msg.graph);
      if (!sweepGraph) sweepGraph = new Graph(json);
      else sweepGraph.set_graph(json);
      const summary = JSON.parse(sweepGraph.run());
      const transfer = [];
      const eyes = msg.withEyes ? eyesOf(sweepGraph, summary, transfer) : {};
      self.postMessage({ type: 'sweep_point', id: msg.id, summary, eyes }, transfer);
    }
  } catch (e) {
    self.postMessage({ type: 'error', id: msg.id, message: String(e?.message ?? e) });
  }
};

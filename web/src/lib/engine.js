// Main-thread client for the simulation worker with request coalescing:
// while a job runs, only the most recent pending request per channel is kept.

export function createEngine() {
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  let nextId = 1;
  const channels = {
    graph: { busy: false, pending: null, resolve: new Map() },
    sweep: { busy: false, pending: null, resolve: new Map() },
  };
  let onReady;
  const ready = new Promise((r) => (onReady = r));

  function dispatch(channel) {
    const ch = channels[channel];
    if (ch.busy || !ch.pending) return;
    const { msg, resolve, reject } = ch.pending;
    ch.pending = null;
    ch.busy = true;
    ch.resolve.set(msg.id, { resolve, reject });
    worker.postMessage(msg);
  }

  worker.onmessage = (ev) => {
    const m = ev.data;
    if (m.type === 'ready') return onReady(m);
    const channel = { graph: 'graph', sweep_point: 'sweep' }[m.type] ?? null;
    const chs = channel ? [channels[channel]] : Object.values(channels);
    for (const ch of chs) {
      const r = ch.resolve.get(m.id);
      if (!r) continue;
      ch.resolve.delete(m.id);
      ch.busy = false;
      if (m.type === 'error') r.reject(new Error(m.message));
      else r.resolve(m);
    }
    for (const c of Object.keys(channels)) dispatch(c);
  };

  function request(channel, msg) {
    return new Promise((resolve, reject) => {
      const ch = channels[channel];
      // A superseded pending request resolves with null (its result would be stale).
      if (ch.pending) ch.pending.resolve(null);
      ch.pending = { msg: { ...msg, id: nextId++ }, resolve, reject };
      dispatch(channel);
    });
  }

  worker.postMessage({ type: 'hello' });

  return {
    ready,
    graph: (graph, previews, fiber) => request('graph', { type: 'graph', graph, previews, fiber }),
    sweepPoint: (graph, withEyes) => request('sweep', { type: 'sweep_point', graph, withEyes }),
  };
}

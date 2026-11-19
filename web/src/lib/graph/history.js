// Undo/redo history of the graph editor: a stack of JSON snapshots of the graph document.
//
// record(json, key) is called after every edit (from autosave). An edit without a key is one
// undo step. Edits with the same key that follow each other within `settleMs` (dragging one
// slider, typing into one field) are merged into one step.

export function createHistory({ limit = 100, settleMs = 400 } = {}) {
  let past = []; // snapshots before the current state, oldest first
  let future = []; // undone snapshots, most recent last
  let current = null; // snapshot of the last settled state
  let pending = null; // latest snapshot of an unsettled burst
  let pendingKey = null;
  let last = 0; // time of the last edit of the burst [ms]

  function push(json) {
    if (json === current) return;
    past.push(current);
    if (past.length > limit) past.shift();
    current = json;
    future = [];
  }
  function settle() {
    if (pending !== null) push(pending);
    pending = null;
    pendingKey = null;
  }

  return {
    /** Record the state after an edit; `key` groups repeated edits of one control. */
    record(json, key = null, now = Date.now()) {
      if (current === null) {
        current = json;
        return;
      }
      if (json === (pending ?? current)) return; // nothing changed (a re-run, a restore)
      if (pending !== null && (key === null || key !== pendingKey || now - last > settleMs)) settle();
      if (key === null) {
        push(json);
      } else {
        // First edit of a burst: the state before it is `current`; later ones replace `pending`.
        pending = json;
        pendingKey = key;
        last = now;
      }
    },
    canUndo: () => past.length > 0 || (pending !== null && pending !== current),
    canRedo: () => future.length > 0 && pending === null,
    /** Snapshot to restore, or null. */
    undo() {
      settle();
      if (!past.length) return null;
      future.push(current);
      current = past.pop();
      return current;
    },
    redo() {
      settle();
      if (!future.length) return null;
      past.push(current);
      current = future.pop();
      return current;
    },
  };
}

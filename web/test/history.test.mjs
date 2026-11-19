// Undo/redo history of the graph editor (lib/graph/history.js).
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHistory } from '../src/lib/graph/history.js';

test('each unkeyed edit is one step; undo and redo walk the stack', () => {
  const h = createHistory();
  h.record('a');
  h.record('b');
  h.record('c');
  assert.equal(h.undo(), 'b');
  assert.equal(h.undo(), 'a');
  assert.equal(h.undo(), null);
  assert.equal(h.redo(), 'b');
  assert.equal(h.redo(), 'c');
  assert.equal(h.canRedo(), false);
});

test('a slider drag (same key, quick succession) is one step', () => {
  const h = createHistory({ settleMs: 400 });
  h.record('x0');
  for (let i = 1; i <= 5; i++) h.record(`x${i}`, 'param:drv:dcd', 1000 + 50 * i);
  assert.equal(h.undo(), 'x0');
  assert.equal(h.canUndo(), false);
});

test('different keys or a pause start a new step', () => {
  const h = createHistory({ settleMs: 400 });
  h.record('s');
  h.record('a1', 'k1', 0);
  h.record('b1', 'k2', 100);
  h.record('b2', 'k2', 1000); // after a pause
  assert.equal(h.undo(), 'b1');
  assert.equal(h.undo(), 'a1');
  assert.equal(h.undo(), 's');
});

test('a new edit after undo clears redo; recording the restored state is a no-op', () => {
  const h = createHistory();
  h.record('a');
  h.record('b');
  assert.equal(h.undo(), 'a');
  h.record('a'); // the editor saves the restored graph again
  assert.equal(h.canRedo(), true);
  h.record('c');
  assert.equal(h.canRedo(), false);
  assert.equal(h.undo(), 'a');
});

test('history is limited', () => {
  const h = createHistory({ limit: 3 });
  for (const x of ['0', '1', '2', '3', '4']) h.record(x);
  assert.equal(h.undo(), '3');
  assert.equal(h.undo(), '2');
  assert.equal(h.undo(), '1');
  assert.equal(h.undo(), null);
});

// Model docs (docs/model/*.md): front matter must be valid YAML, as the app parses it at load.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import YAML from 'yaml';

const dir = new URL('../../docs/model/', import.meta.url);

test('every model doc has valid front matter with component and title', () => {
  const files = readdirSync(dir).filter((f) => f.endsWith('.md'));
  assert.ok(files.length > 10);
  for (const f of files) {
    const m = readFileSync(new URL(f, dir), 'utf8').match(/^---\n([\s\S]*?)\n---\n/);
    assert.ok(m, `${f}: no front matter`);
    const meta = YAML.parse(m[1]);
    assert.equal(typeof meta.component, 'string', `${f}: component`);
    assert.equal(typeof meta.title, 'string', `${f}: title`);
  }
});

test('one model doc per component kind (docs/architecture.md, 5.1)', async () => {
  const { DOC, LABEL } = await import('../src/lib/graph/catalog.js');
  const all = readdirSync(dir).filter((f) => f.endsWith('.md'));
  const meta = (f) => YAML.parse(readFileSync(new URL(f, dir), 'utf8').match(/^---\n([\s\S]*?)\n---\n/)[1]);
  const files = new Set(all.map((f) => f.replace(/\.md$/, '')));
  // Docs of Python-only example components (front matter `python_only: true`) have no editor kind.
  const pythonOnly = new Set(all.filter((f) => meta(f).python_only === true).map((f) => f.replace(/\.md$/, '')));
  const kinds = Object.keys(LABEL);
  for (const kind of kinds) assert.ok(files.has(DOC[kind]), `${kind}: doc '${DOC[kind]}' missing`);
  const used = kinds.map((k) => DOC[k]);
  assert.equal(new Set(used).size, used.length, 'two kinds share a doc');
  for (const f of files) assert.ok(used.includes(f) || pythonOnly.has(f), `${f}.md belongs to no component kind`);
  for (const f of pythonOnly) assert.ok(!used.includes(f), `${f}.md is marked python_only but belongs to an editor kind`);
});

test('formulas survive GitHub Markdown (no \\{ \\} \\, \\; \\! inside $…$)', () => {
  // GitHub applies Markdown backslash escapes inside math: \{ becomes {, \, becomes a comma.
  // Use \lbrace, \rbrace, \thinspace, \thickspace, \negthinspace instead.
  const docs = [new URL('../../docs/', import.meta.url), dir];
  for (const d of docs) {
    for (const f of readdirSync(d).filter((x) => x.endsWith('.md'))) {
      const s = readFileSync(new URL(f, d), 'utf8').replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
      for (const m of s.matchAll(/\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$/g)) {
        const t = m[1] ?? m[2];
        assert.ok(!/\\[{},;!]/.test(t), `${f}: ${t.slice(0, 60)}`);
      }
    }
  }
});

// README screenshots of the browser editor (docs/img/screenshot-*.png). Note: screenshot-editor.png
// in the repository was arranged by hand; this script would replace it with an automatic one.
// Requires Playwright (not a project dependency): npm i --no-save playwright
// Usage: npm run build, then  node scripts/screenshots.mjs  (serves dist/ itself).
import { chromium } from 'playwright';
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname } from 'node:path';

const dist = process.env.DIST ?? 'dist';
const out = new URL('../../docs/img/', import.meta.url).pathname;
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml' };
const server = http.createServer(async (req, res) => {
  const p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  try {
    const body = await readFile(join(dist, p === '/' ? 'index.html' : p));
    res.writeHead(200, { 'content-type': types[extname(p)] ?? 'text/html' });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(4176, r));

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1 });
const runs = async () => Number(await page.getAttribute('.canvas .status', 'data-runs'));
const after = async (b) => page.waitForFunction((x) => Number(document.querySelector('.canvas .status')?.dataset.runs ?? 0) > x, b, { timeout: 60000 });
const node = (id) => page.locator(`.svelte-flow__node[data-id="${id}"]`);
const select = async (id) => { const b = await runs(); await page.click('.svelte-flow__controls-fitview'); await node(id).click(); await after(b); };
const palette = async (label) => { const b = await runs(); await page.click(`button[aria-label="Add ${label}"]`); await after(b); };
const settle = () => page.waitForTimeout(600);

await page.goto('http://localhost:4176/');
await after(0);
// Start from the bare reference link (the start page already has an eye and an analyzer).
let b0 = await runs();
await page.click('button.menubtn');
await page.click('.dropdown button:has-text("Reference link (bare)")');
await after(b0);

// 1. Reference link with an eye diagram and an analyzer after the FFE; the eye in the inspector.
await select('ffe');
await palette('Eye diagram');
await select('ffe');
await palette('PAM-4 / NRZ analyzer');
await select('eye');
await page.click('.svelte-flow__controls-fitview');
await settle();
await page.screenshot({ path: `${out}screenshot-editor.png` });

// 2. Fiber inspector: launch beam on the facet, mode powers and the fiber plots (tall window,
// inspector only).
await page.setViewportSize({ width: 1600, height: 2000 });
await select('el1');
await page.click('label:has-text("Guided intensity") input');
await page.waitForTimeout(1500);
// Crop to the facet card and the component-view card.
const card = (t) => page.locator('.inspector > *', { has: page.locator(`h2:has-text("${t}")`) }).first().boundingBox();
const [c1, c2] = [await card('Fiber facet'), await card('component view')];
const clip = { x: Math.min(c1.x, c2.x) - 4, y: c1.y - 4, width: Math.max(c1.width, c2.width) + 8, height: c2.y + c2.height - c1.y + 8 };
await page.screenshot({ path: `${out}screenshot-fiber.png`, clip });
await page.setViewportSize({ width: 1600, height: 1000 });

// 3. The SWDM4 WDM example from the menu.
let b = await runs();
await page.click('button.menubtn');
await page.click('.dropdown button:has-text("SWDM4 WDM link")');
await after(b);
// Canvas only, in a wide window, so the 38 nodes stay readable.
await page.setViewportSize({ width: 2400, height: 1150 });
await page.waitForTimeout(800);
await page.click('.svelte-flow__controls-fitview');
await page.waitForTimeout(800);
await page.click('.svelte-flow__controls-fitview');
await settle();
await page.locator('.canvas').screenshot({ path: `${out}screenshot-wdm.png` });

await browser.close();
server.close();
console.log('wrote', ['editor', 'fiber', 'wdm'].map((n) => `docs/img/screenshot-${n}.png`).join(', '));

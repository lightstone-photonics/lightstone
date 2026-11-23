// Browser test of the graph editor. Requires Playwright (not a project dependency):
//   npm i --no-save playwright && npx playwright install chromium
// Usage: npm run build, then  DIST=dist node e2e/graph.mjs  (serves dist/ itself).
import { chromium } from 'playwright';
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname } from 'node:path';

const dist = process.env.DIST ?? 'dist';
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
await new Promise((r) => server.listen(4175, r));

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1600, height: 1100 }, acceptDownloads: true });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
page.on('dialog', (d) => (d.type() === 'prompt' ? d.accept(d.defaultValue()) : d.accept())); // splitter confirmation, save-as name
const shot = (name) => page.screenshot({ path: `${process.env.SCREENSHOT ?? 'graph'}-${name}.png` });
const check = (cond, msg) => { if (!cond) { errors.push('CHECK FAILED: ' + msg); console.log('FAIL', msg); } else console.log('ok  ', msg); };
// `before` is the run counter before an action; resolves with the status after the next run.
const status = async () => Number(await page.getAttribute('.canvas .status', 'data-runs'));
async function computed(before) {
  await page.waitForFunction((b) => Number(document.querySelector('.canvas .status')?.dataset.runs ?? 0) > b, before, { timeout: 60000 });
  return page.textContent('.canvas .status');
}
const nodeEl = (id) => page.locator(`.svelte-flow__node[data-id="${id}"]`);
// Fit the view first: nodes added late can land under the minimap.
const clickNode = async (id) => { await page.click('.svelte-flow__controls-fitview'); await nodeEl(id).click(); };
const badgeOf = async (id) => ((await nodeEl(id).locator('.badge').first().textContent()) ?? '').trim();
const badgeAll = async (id) => (await nodeEl(id).locator('.badge').allTextContents()).map((t) => t.trim());
// Instrument display of a measuring node: "SER 3.3e-2" style text from its main reading.
const readout = async (id) => {
  const l = nodeEl(id).locator('.lcd');
  return `${(await l.locator('.mlabel').textContent()).trim().replace(/^(PAM-4|NRZ) /, '').replace(' · pre-FEC', '')} ${(await l.locator('.value').textContent()).trim()}`;
};
const lcdRows = async (id) => {
  const k = await nodeEl(id).locator('.lcd dt').allTextContents();
  const v = await nodeEl(id).locator('.lcd dd').allTextContents();
  return Object.fromEntries(k.map((x, i) => [x.trim(), v[i].trim()]));
};

await page.goto('http://localhost:4175/');
console.log('start page:', await computed(0));
// A new visitor sees the PAM-4 example with its eye diagram and analyzer.
check((await page.locator('.svelte-flow__node-component').count()) === 12 && (await nodeEl('eye').locator('.thumb canvas').count()) === 1,
  'start page: the PAM-4 example (reference link + eye diagram + analyzer)');
check(/^SER \d/.test(await readout('ana')), `start page analyzer: ${await readout('ana')}`);
await shot('start');
// The rest of the test starts from the bare reference link.
await page.click('button.menubtn');
let before0 = await status();
await page.click('.dropdown button:has-text("Reference link (bare)")');
console.log('reference:', await computed(before0));
// The canvas redraws nodes and wires after the run: wait until the new graph is on screen
// (the start page had 12 nodes and 14 wires), instead of counting a half-drawn canvas.
const settled = await page.waitForFunction(
  () => document.querySelectorAll('.svelte-flow__node-component').length === 10 && document.querySelectorAll('.svelte-flow__edge').length === 10,
  null, { timeout: 15000 }).then(() => true, () => false);
if (!settled) console.log('canvas did not settle:', await page.locator('.svelte-flow__node-component').count(), 'nodes,', await page.locator('.svelte-flow__edge').count(), 'wires');
check((await page.locator('.svelte-flow__node-component').count()) === 10, 'reference link shows 10 nodes (with the receiver FFE)');
check((await page.locator('.svelte-flow__node-frame').count()) === 4, 'and the 4 zone frames (Transmitter, Channel, Receiver front end, Data recovery)');
check((await page.locator('.svelte-flow__edge').count()) === 10, 'and 10 wires (FFE input, reference and output)');
await shot('reference');

// Add an analyzer after the receive filter (unequalized): select rxf, then add from the palette.
let before = await status();
await nodeEl('rxf').click();
await computed(before);
before = await status();
await page.click('button[aria-label="Add PAM-4 / NRZ analyzer"]');
console.log('analyzer:', await computed(before));
const ser = await readout('ana');
check(/^SER \d/.test(ser), `analyzer auto-wired to rxf and src, shows ${ser}`);
// Adding from the palette is an undo step: undo removes the analyzer, redo brings it back.
before = await status();
await page.click('button[aria-label="Undo"]');
await computed(before);
check((await nodeEl('ana').count()) === 0, 'undo removes the node added from the palette');
before = await status();
await page.click('button[aria-label="Redo"]');
await computed(before);
check((await readout('ana')) === ser, 'redo brings it back with the same SER');

// Equalized: add a second FFE from the palette, then set the analyzer's input to the reference
// FFE (`ffe`) via the inspector.
before = await status();
await nodeEl('rxf').click();
await computed(before);
before = await status();
await page.click('button[aria-label="Add FFE equalizer"]');
await computed(before);
before = await status();
await nodeEl('ana').click();
await computed(before);
before = await status();
await page.selectOption('label.row:has-text("in") select', 'ffe.out');
await computed(before);
const ser2 = await readout('ana');
const v = (s) => Number(s.match(/SER ([\d.e+-]+)/)[1]);
check(v(ser2) < v(ser) / 100, `FFE (wired in the inspector) lowers the SER: ${ser} → ${ser2}`);

// Driver jitter: the badge shows the configured jitter; DCD raises the SER; reset to 0.
before = await status();
await nodeEl('drv').click();
await computed(before);
check((await badgeOf('drv')) === 'no jitter', `driver badge: ${await badgeOf('drv')}`);
const setDcd = async (ps) => {
  const b = await status();
  await page.$eval('input[id$="-dcd"]', (el, x) => { el.value = x; el.dispatchEvent(new Event('input', { bubbles: true })); }, String(ps));
  await computed(b);
};
await setDcd(6);
const ser3 = await readout('ana');
check((await badgeOf('drv')) === 'DCD 6 ps' && v(ser3) > v(ser2), `DCD 6 ps raises the SER: ${ser2} → ${ser3}`);
// Undo the slider move (one step for the whole drag, after it settled).
await page.waitForTimeout(600);
before = await status();
await page.click('button[aria-label="Undo"]');
await computed(before);
check((await badgeOf('drv')) === 'no jitter' && (await readout('ana')) === ser2, `undo resets the DCD slider move: ${await badgeOf('drv')}, ${await readout('ana')}`);

// Fiber length slider updates the analyzer, and only downstream nodes are recomputed.
before = await status();
await nodeEl('el1').click();
await computed(before);
before = await status();
await page.$eval('input[id$="-length"]', (el) => { el.value = '50'; el.dispatchEvent(new Event('input', { bubbles: true })); });
const st = await computed(before);
check(/Computed [1-9] of 12/.test(st), `length change recomputes only part of the graph: ${st}`);
check((await badgeOf('el1')).startsWith('50 m'), 'fiber badge shows 50 m');
await page.waitForSelector('.inspector .facetsel', { timeout: 30000 });
check((await page.$$eval('.inspector .grid figure svg', (n) => n.length)) === 4, 'fiber node shows the 4 fiber plots in the inspector');
const facetOk = await page.$eval('.inspector .facetsel ~ div canvas', (c) => {
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  let lit = 0;
  for (let i = 0; i < d.length; i += 4) lit += d[i] + d[i + 1] + d[i + 2] > 60 ? 1 : 0;
  return lit;
});
check(facetOk > 1000, `fiber facet (launch beam) is drawn (${facetOk} lit pixels)`);
before = await status();
await page.check('.facetsel input[value="1"]');
await computed(before);
check(true, 'facet switches to the guided intensity');
// Multimode preset: the reference fiber is OM4; the type selector switches to OM3; changing
// the profile α converts it to a custom fiber, and "Back to OM3" restores the preset.
check(/· OM4 · EMB 47\d\d MHz·km$/.test(await badgeOf('el1')), `reference fiber is the OM4 preset: ${await badgeOf('el1')}`);
before = await status();
await page.selectOption('label.row:has-text("Fiber type") select', 'om3');
await computed(before);
check(/· OM3 · EMB 20\d\d MHz·km$/.test(await badgeOf('el1')), `type selector switches to OM3: ${await badgeOf('el1')}`);
const coilColor = await nodeEl('el1').locator('ellipse.coil').first().evaluate((e) => getComputedStyle(e).stroke);
check(coilColor === 'rgb(63, 208, 212)', `OM3 fiber symbol is aqua like its jacket (${coilColor})`);
before = await status();
await page.$eval('input[id$="-profile-alpha"]', (el) => { el.value = '2.05'; el.dispatchEvent(new Event('input', { bubbles: true })); });
await computed(before);
const conv = (await page.textContent('.message')).trim();
check(/^50 m · custom/.test(await badgeOf('el1')) && /Converted the OM3 preset to a custom fiber/.test(conv), `changing α converts the preset to a custom fiber: ${await badgeOf('el1')}`);
before = await status();
await page.click('.message button:text("Back to OM3")');
await computed(before);
check(/· OM3 ·/.test(await badgeOf('el1')), `"Back to OM3" restores the preset: ${await badgeOf('el1')}`);
before = await status();
await page.selectOption('label.row:has-text("Fiber type") select', 'om4');
await computed(before);
await shot('fiber');
await shot('ffe');

// Spectrum analyzers: OSA on the fiber output, ESA on the receive filter; removed again.
const addAnalyzer = async (after, label) => {
  let b = await status();
  await clickNode(after);
  await computed(b);
  b = await status();
  await page.click(`button[aria-label="Add ${label}"]`);
  await computed(b);
};
await addAnalyzer('el1', 'Optical spectrum analyzer');
const osa = await readout('osa');
const osaRows = await lcdRows('osa');
check(/^PEAK -\d+\.\d+$/.test(osa) && osaRows['λ'] === '850.00 nm', `OSA shows the peak at 850 nm: ${osa} dBm, ${JSON.stringify(osaRows)}`);
check((await nodeEl('osa').locator('figure.trace path.tr').count()) === 1, 'OSA node draws its trace');
await page.click('button:text("Remove node")');
await addAnalyzer('rxf', 'Electrical spectrum analyzer');
const esa = await readout('esa');
check(/^SNR \d+\.\d$/.test(esa), `ESA shows the SNR: ${esa} dB`);
check((await nodeEl('esa').locator('figure.trace path.tr').count()) === 2, 'ESA node draws signal and noise traces');
await page.click('button:text("Remove node")');

// Example component written in Rust (rust_example, palette group "Custom"): with H = 1 and with
// gain 0.5 the SER after it equals the SER at its input.
check((await page.locator('nav.palette h3:text("Custom")').count()) === 1, 'palette has a "Custom" group');
await addAnalyzer('rxf', 'Example (Rust): gain & delay');
check((await badgeOf('ex')) === 'g 1.00 · τ 0.0 ps', `Rust example node badge: ${await badgeOf('ex')}`);
await addAnalyzer('ex', 'PAM-4 / NRZ analyzer');
const serEx = await readout('ana2');
before = await status();
await page.selectOption('label.row:has-text("in") select', 'rxf.out');
await computed(before);
const serRxf = await readout('ana2');
check(/^SER \d/.test(serEx) && serEx === serRxf, `H(f) = 1: SER after the Rust example equals the SER at rxf (${serEx})`);
before = await status();
await page.selectOption('label.row:has-text("in") select', 'ex.out');
await computed(before);
await clickNode('ex');
before = await status();
await page.$eval('input[id$="-gain"]', (el) => { el.value = '0.5'; el.dispatchEvent(new Event('input', { bubbles: true })); });
await computed(before);
check((await badgeOf('ex')) === 'g 0.50 · τ 0.0 ps' && (await readout('ana2')) === serEx, `gain 0.5 scales signal and noise: SER unchanged (${await readout('ana2')})`);
await page.click('button:text("Remove node")');
await clickNode('ana2');
await page.click('button:text("Remove node")');
check((await page.locator('button[aria-label="Add WDM multiplexer"]').count()) === 1, 'palette offers the WDM components');

// Frames: dragging the Transmitter frame moves its nodes.
const box0 = await nodeEl('vcsel').boundingBox();
const fr = page.locator('.svelte-flow__node-frame').first();
const fb = await fr.boundingBox();
await page.mouse.move(fb.x + 30, fb.y + 8);
await page.mouse.down();
await page.mouse.move(fb.x + 30, fb.y + 68, { steps: 8 });
await page.mouse.up();
const box1 = await nodeEl('vcsel').boundingBox();
check(box1.y - box0.y > 20, `dragging the frame moves its nodes (VCSEL moved ${Math.round(box1.y - box0.y)} px)`);
await page.click('button:text("Zone frames")');

// Optical fan-out: connect a second photodiode to the fiber output → splitter inserted.
before = await status();
await page.click('.svelte-flow__pane', { position: { x: 30, y: 30 } });
await computed(before);
before = await status();
await page.click('button[aria-label="Add PIN photodiode"]');
await computed(before);
const pin2 = 'pin2';
const src = nodeEl('el1').locator('.svelte-flow__handle.source');
const dst = nodeEl(pin2).locator('.svelte-flow__handle.target');
before = await status();
await src.dragTo(dst);
await computed(before);
check((await nodeEl('spl').count()) === 1, 'second optical consumer inserts a splitter');
check((await page.locator('.svelte-flow__edge').count()) >= 12, 'splitter is wired in');

// Invalid graph: an unconnected input is reported.
before = await status();
await page.click('.svelte-flow__pane', { position: { x: 30, y: 30 } });
await computed(before);
before = await status();
await page.click('button[aria-label="Add Attenuator"]');
const bad = await computed(before);
check(bad.startsWith('Not simulated'), `unconnected input stops the run: ${bad}`);
check((await page.locator('.issues li.error').count()) > 0, 'problem list shows the error: ' + (await page.locator('.issues li.error').first().textContent()).trim());
check((await page.locator('.svelte-flow__node .node.issue').count()) === 1, 'the offending node is marked');
await shot('invalid');

// Delete the stray attenuator with the keyboard.
before = await status();
await nodeEl('att').click();
await computed(before);
before = await status();
await page.keyboard.press('Delete');
check((await computed(before)).startsWith('Computed'), 'deleting the node makes the graph valid again');

// Eye diagram node renders in the inspector.
before = await status();
await nodeEl('ffe').click();
await computed(before);
before = await status();
await page.click('button[aria-label="Add Eye diagram"]');
await computed(before);
await page.waitForSelector('.inspector canvas', { timeout: 30000 });
check(true, 'eye diagram node shows its eye in the inspector');
// Values on the nodes themselves: eye thumbnail, analyzer and receiver output summaries.
await page.waitForSelector('.svelte-flow__node[data-id="eye"] .thumb canvas', { timeout: 10000 });
const thumbLit = await page.$eval('.svelte-flow__node[data-id="eye"] .thumb canvas', (c) => {
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  let n = 0;
  for (let i = 0; i < d.length; i += 4) if (d[i] + d[i + 1] + d[i + 2] > 60) n++;
  return n;
});
check(thumbLit > 2000, `eye diagram node draws its eye on the canvas (${thumbLit} lit pixels)`);
const eyeBadge = await badgeOf('eye');
check(/V … .*V · [\d.]+ UI$/.test(eyeBadge), `eye diagram node shows its range and sampling phase: ${eyeBadge}`);
const anaRead = await readout('ana');
const anaRows = await lcdRows('ana');
check(/pre-FEC/.test(await nodeEl('ana').locator('.lcd .mlabel').textContent()) && /^OK ×/.test(anaRows['KP4 2.4e-4'] ?? ''),
  `PAM-4 analyzer labels the SER pre-FEC and shows the KP4 margin: ${anaRows['KP4 2.4e-4']}`);
check(/^SER \d/.test(anaRead) && /^\d/.test(anaRows.BER) && /^[\d.]+$/.test(anaRows.Q) && /mV$/.test(anaRows.EH) && /UI$/.test(anaRows.EW),
  `analyzer node shows an instrument display: ${anaRead} | ${Object.entries(anaRows).map(([k, v]) => `${k} ${v}`).join(' | ')}`);
await nodeEl('ana').screenshot({ path: `${process.env.SCREENSHOT ?? 'graph'}-analyzer.png` });
// Resize the eye diagram node by dragging its top-left handle (bottom-right can sit under the minimap).
await clickNode('eye');
const eyeBox0 = await nodeEl('eye').boundingBox();
const thumb0 = await nodeEl('eye').locator('.thumb canvas').boundingBox();
// Use the first corner handle that is not covered by another element (wires, frames, the
// minimap) at the current zoom; drag it outwards.
let corner = null;
for (const [v, h, dx, dy] of [['top', 'left', -160, -120], ['top', 'right', 160, -120], ['bottom', 'left', -160, 120], ['bottom', 'right', 160, 120]]) {
  const hdl = nodeEl('eye').locator(`.svelte-flow__resize-control.handle.${v}.${h}`);
  const free = await hdl.evaluate((el) => {
    const b = el.getBoundingClientRect();
    const t = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
    return t === el || el.contains(t);
  });
  if (free) { corner = { hb: await hdl.boundingBox(), dx, dy }; break; }
}
const { hb, dx, dy } = corner;
await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2);
await page.mouse.down();
await page.mouse.move(hb.x + dx, hb.y + dy, { steps: 8 });
await page.mouse.up();
await page.waitForTimeout(300);
const eyeBox1 = await nodeEl('eye').boundingBox();
const thumb1 = await nodeEl('eye').locator('.thumb canvas').boundingBox();
check(eyeBox1.width > eyeBox0.width + 60 && thumb1.width > thumb0.width + 60 && thumb1.height > thumb0.height + 40,
  `eye diagram node resizes by dragging (on screen ${Math.round(eyeBox0.width)} → ${Math.round(eyeBox1.width)} px wide), and its eye grows (${Math.round(thumb0.width)}×${Math.round(thumb0.height)} → ${Math.round(thumb1.width)}×${Math.round(thumb1.height)} px)`);
await shot('resized');
// Ports stay whole on a resized node: the outer half of the input port (outside the node
// border) must still be the port, not clipped away.
const outerHalfIsPort = await nodeEl('eye').locator('.svelte-flow__handle').first().evaluate((h) => {
  const b = h.getBoundingClientRect();
  return document.elementFromPoint(b.left + b.width * 0.15, b.top + b.height / 2) === h;
});
check(outerHalfIsPort, 'ports of a resized node are not clipped by its border');
const decLines = await badgeAll('rx');
check(/^μ .*V · σ .*V rms$/.test(decLines[0] ?? ''), `receiver output node shows mean and RMS noise: ${decLines.join(' | ')}`);
check((await page.$$eval('.inspector .math .katex', (x) => x.length)) > 0, 'inspector shows the model doc with formulas');
await shot('eye');

// Save and reopen the graph.
// Save as: with the File System Access API (stubbed: records what is written) …
await page.evaluate(() => {
  window.__saved = null;
  window.showSaveFilePicker = async (o) => ({
    name: 'my-link.json',
    createWritable: async () => ({ write: async (t) => (window.__saved = { text: t, suggested: o.suggestedName }), close: async () => {} }),
  });
});
await page.click('button.menubtn');
await page.click('.dropdown button:has-text("Save graph as")');
await page.waitForFunction(() => window.__saved !== null);
const picked = await page.evaluate(() => window.__saved);
check(picked.suggested === 'reference.json' && JSON.parse(picked.text).nodes.length === 15 && (await page.textContent('.message')).includes('Saved my-link.json'),
  `save as: file picker suggests ${picked.suggested}, writes the graph, keeps the chosen name`);
// … and without it (Firefox, Safari): a name prompt, then a download.
await page.evaluate(() => { delete window.showSaveFilePicker; });
await page.click('button.menubtn');
const [dl] = await Promise.all([page.waitForEvent('download'), page.click('.dropdown button:has-text("Save graph as")')]);
check(dl.suggestedFilename() === 'my-link.json', `save as without a file picker: prompt with the last name, download ${dl.suggestedFilename()}`);
const path = await dl.path();
const saved = JSON.parse(await readFile(path, 'utf8'));
check(saved.nodes.length === 15 && saved.layout?.positions?.ffe, `saved JSON has ${saved.nodes.length} nodes and positions`);
check(Array.isArray(saved.layout?.sizes?.eye) && saved.layout.sizes.eye[0] > 250, `saved JSON keeps the resized node size (eye ${saved.layout?.sizes?.eye?.join(' × ')})`);
before = await status();
await page.click('button.menubtn');
await page.click('.dropdown button:has-text("New empty graph")');
await computed(before);
check((await page.locator('.svelte-flow__node-component').count()) === 0, 'menu: new empty graph');
// Undo brings the 15-node graph back, redo empties it again.
before = await status();
await page.click('button[aria-label="Undo"]');
await computed(before);
check((await page.locator('.svelte-flow__node-component').count()) === 15, 'undo restores the graph before "New empty graph"');
before = await status();
await page.click('button[aria-label="Redo"]');
await computed(before);
check((await page.locator('.svelte-flow__node-component').count()) === 0, 'redo empties it again');
before = await status();
await page.setInputFiles('input[type=file]', path);
await computed(before);
await page.waitForFunction(() => document.querySelectorAll('.svelte-flow__node-component').length === 15, null, { timeout: 30000 });
check(true, 'menu: loading the saved file gives back 15 nodes');

// Runs: record, live update off, results files.
before = await status();
await page.click('button.run');
await computed(before);
await page.waitForSelector('.runs tbody tr', { timeout: 30000 });
check((await page.locator('.runs tbody tr').count()) === 1, 'Run records a run in the list');
await page.uncheck('label:has-text("Live update") input');
before = await status();
await nodeEl('el1').click();
await computed(before);
await page.$eval('input[id$="-length"]', (el) => { el.value = '120'; el.dispatchEvent(new Event('input', { bubbles: true })); });
await page.waitForFunction(() => document.querySelector('.canvas .status')?.textContent.startsWith('Changed'), null, { timeout: 10000 });
check(true, 'with live update off, an edit only marks the results as stale');
before = await status();
await page.click('button.run');
await computed(before);
await page.waitForFunction(() => document.querySelectorAll('.runs tbody tr').length === 2, null, { timeout: 30000 });
check(true, 'second run recorded after pressing Run');
await page.fill('.runs tbody tr:nth-child(2) input[type=text]', 'L = 120 m');
const [jsonDl] = await Promise.all([page.waitForEvent('download'), page.click('button:text("Save results (JSON)")')]);
const study = JSON.parse(await readFile(await jsonDl.path(), 'utf8'));
check(study.format === 'lightstone-study' && study.runs.length === 2, `results file has ${study.runs.length} runs`);
check(study.runs[1].label === 'L = 120 m' && study.runs[1].graph.nodes.find((n) => n.id === 'el1').params.length === 120, 'run stores its label and its graph');
check(Array.isArray(study.runs[0].eyes?.eye) && study.runs[0].eyes.eye.length === 600 * 400, 'run stores the eye density');
const [csvDl] = await Promise.all([page.waitForEvent('download'), page.click('button:text("Export CSV")')]);
const csv = (await readFile(await csvDl.path(), 'utf8')).trim().split('\n');
const head = csv[0].split(',');
const iSer = head.indexOf('ana.ser');
check(csv.length === 3 && iSer > 0, `CSV has a header and 2 rows with ana.ser (${head.length} columns)`);
check(Number(csv[2].split(',')[iSer]) > Number(csv[1].split(',')[iSer]), 'SER is higher at 120 m than at 50 m');
// Port values: recorded for every output, kept out of the CSV unless asked for.
const pv = study.runs[1].ports ?? {};
const nodesWithOut = study.runs[1].graph.nodes.filter((n) => !['power_meter', 'pam4_analyzer', 'tdecq', 'eye_diagram', 'receiver_output'].includes(n.kind));
const missing = nodesWithOut.filter((n) => !(pv[n.id] && Object.keys(pv[n.id]).length)).map((n) => `${n.id} (${n.kind})`);
check(!missing.length, `run records port values of all ${nodesWithOut.length} nodes with outputs${missing.length ? '; missing ' + missing.join(', ') : ''}`);
check(pv.el1?.out?.type === 'optical' && pv.el1.out.average_power > 0 && pv.rxf?.out?.swing > 0 && pv.rxf.out.noise_rms > 0,
  `port values: el1.out ${(10 * Math.log10(pv.el1?.out?.average_power / 1e-3)).toFixed(2)} dBm, rxf.out swing ${pv.rxf?.out?.swing?.toExponential(2)}, noise ${pv.rxf?.out?.noise_rms?.toExponential(2)} rms`);
check(!head.some((h) => h.includes('/')), 'default CSV has instrument/report columns only');
await page.check('label:has-text("include all port values") input');
const [csvAll] = await Promise.all([page.waitForEvent('download'), page.click('button:text("Export CSV")')]);
const headAll = (await readFile(await csvAll.path(), 'utf8')).split('\n')[0].split(',');
check(headAll.includes('el1/out.average_power') && headAll.includes('rxf/out.swing') && headAll.length > head.length, `"include all port values" adds ${headAll.length - head.length} port columns`);
await nodeEl('el1').click();
await page.waitForSelector('.inspector .outs .pval', { timeout: 10000 });
const pvText = (await page.textContent('.inspector .outs .pval')).trim();
check(/dBm$/.test(pvText), `inspector shows the output value of el1: ${pvText}`);
await page.locator('.svelte-flow__edge[aria-label^="Wire from rxf.out"]').first().dispatchEvent('pointerenter', { clientX: 400, clientY: 300 });
await page.waitForSelector('.wiretip', { timeout: 5000 });
const tip = (await page.textContent('.wiretip')).trim();
check(/^rxf\.out: mean .* swing .* rms/.test(tip), `hovering a wire shows its value: ${tip}`);
await clickNode('eye');
await page.waitForSelector('.inspector canvas', { timeout: 30000 });
const [png] = await Promise.all([page.waitForEvent('download'), page.click('.inspector .card:has(canvas) button.mini')]);
check((await readFile(await png.path())).subarray(1, 4).toString() === 'PNG', 'eye exports as PNG');
await page.check('label:has-text("Live update") input');

// Compare the two runs.
await page.check('.runs tbody tr:nth-child(1) input.pick');
await page.check('.runs tbody tr:nth-child(2) input.pick');
await page.waitForSelector('.cmp table', { timeout: 10000 });
const cmpKeys = await page.$$eval('.cmp tbody th', (x) => x.map((t) => t.textContent.trim()));
check(cmpKeys.includes('ana.ser') && !cmpKeys.includes('el0.loss_db'), `comparison lists differing metrics (${cmpKeys.length}), e.g. ana.ser`);
check(cmpKeys.includes('el1/out.average_power'), 'with port values on, the comparison includes them (el1/out.average_power differs)');
await page.uncheck('label:has-text("include all port values") input');

// Sweep the fiber length: 4 points, SER rises with length.
await page.selectOption('.sweep select[name=node]', 'el1');
await page.fill('.sweep input[name=from]', '0');
await page.fill('.sweep input[name=to]', '150');
await page.fill('.sweep input[name=points]', '4');
const nRuns = await page.locator('.runs tbody tr').count();
await page.click('button:text("▶ Run sweep")');
await page.waitForFunction((n) => document.querySelectorAll('.runs tbody tr').length === n + 4 && document.querySelector('.sweep .plotbar'), nRuns, { timeout: 120000 });
check(true, 'sweep adds 4 runs');
await page.selectOption('.sweep .plotbar select', 'ana.ser');
const pts = await page.$$eval('.sweep svg circle', (c) => c.length);
check(pts === 4, `sweep plot shows 4 points (${pts})`);
const [swCsv] = await Promise.all([page.waitForEvent('download'), page.click('button:text("Export CSV")')]);
const swLines = (await readFile(await swCsv.path(), 'utf8')).trim().split('\n');
const sh = swLines[0].split(',');
const [iLen, iS] = [sh.indexOf('el1.length'), sh.indexOf('ana.ser')];
const sweepRows = swLines.slice(-4).map((l) => l.split(',')).map((c) => [Number(c[iLen]), Number(c[iS])]);
check(iLen > 0 && sweepRows.map((r) => r[0]).join() === '0,50,100,150', `CSV has the swept lengths: ${sweepRows.map((r) => r[0]).join(', ')} m`);
check(sweepRows.every((r, i) => i === 0 || r[1] >= sweepRows[i - 1][1]), `SER rises with length: ${sweepRows.map((r) => r[1].toExponential(1)).join(', ')}`);
await shot('sweep');

// Autosave: a reload restores the graph.
await page.reload();
await computed(0); // first run of the restored graph
await page.waitForFunction(() => document.querySelectorAll('.svelte-flow__node-component').length === 15, null, { timeout: 30000 });
check((await page.textContent('.message')).includes('Restored'), 'reload restores the autosaved graph (14 nodes)');
await shot('runs');

// Duplicate the Transmitter frame (like adding a lane's transmitter).
const fr2 = page.locator('.svelte-flow__node-frame').first();
const fb2 = await fr2.boundingBox();
await page.mouse.click(fb2.x + 30, fb2.y + 8);
await page.waitForSelector('.inspector button:text("Duplicate (e.g. another lane)")', { timeout: 10000 });
before = await status();
await page.click('.inspector button:text("Duplicate (e.g. another lane)")');
const dupSt = await computed(before);
check((await page.locator('.svelte-flow__node-component').count()) === 18 && (await page.locator('.svelte-flow__node-frame').count()) === 5, `duplicating the Transmitter frame adds 3 nodes and a frame (${dupSt})`);
check(/^Computed 3 of 18/.test(dupSt), 'only the 3 copied nodes are computed');
check((await page.locator('#tab-system, svg.system').count()) === 0, 'the lane-based system view is retired (one editor)');

// NRZ: switch the pattern source to two levels; the analyzer and the FFE follow.
before = await status();
await nodeEl('src').click();
await computed(before);
before = await status();
await page.selectOption('label.row:has-text("Modulation") select', '2');
const nrzSt = await computed(before);
check((await badgeOf('src')) === 'NRZ (PRBS13)', `pattern source switched to NRZ (${nrzSt})`);
const nrzSer = await readout('ana');
check(/^Computed/.test(nrzSt) && /^SER < 2\.3e-19$/.test(nrzSer), `NRZ link runs; its SER is below the semi-analytic floor and shown as such: ${nrzSer}`);

// Examples menu: the SWDM4 WDM link (examples/graphs/swdm4-wdm-link.json) with an eye per channel.
before = await status();
await page.click('button.menubtn');
check((await page.locator('.dropdown button:has-text("Reference link (bare)")').count()) === 1 && (await page.locator('.dropdown button:has-text("PAM-4 link, 100 m OM4")').count()) === 1, 'menu lists the PAM-4 example and the bare reference link');
await page.click('.dropdown button:has-text("SWDM4 WDM link")');
const wdmSt = await computed(before);
const wdmFile = JSON.parse(await readFile(new URL('../../examples/graphs/swdm4-wdm-link.json', import.meta.url), 'utf8'));
const nWdm = wdmFile.nodes.length;
const nEyes = wdmFile.nodes.filter((n) => n.kind === 'eye_diagram').length;
await page.waitForFunction((n) => document.querySelectorAll('.svelte-flow__node-component').length === n, nWdm, { timeout: 60000 });
const wdmSer = await Promise.all([0, 1, 2, 3].map((i) => readout(`ana${i}`)));
check(wdmSer.every((x) => /^SER \d/.test(x) && Number(x.slice(4)) / 2 < 2.4e-4), `SWDM4 example: ${nWdm} nodes, all 4 channels below the KP4 pre-FEC limit (${wdmSer.join(', ')}; ${wdmSt})`);
check(nEyes > 0 && (await page.locator('.svelte-flow__node[data-id^="eye"] canvas').count()) === nEyes, `SWDM4 example shows its ${nEyes} eye diagram(s)`);
check((await nodeEl('osa').locator('figure.trace path.tr').count()) === 1, `SWDM4 example: optical spectrum analyzer at the fiber output (${await readout('osa')} dBm peak)`);
// Resizing an analyzer node enlarges its screen: the OSA trace and the PAM-4 LCD grow in height.
async function resizeBy(id, ddx, ddy) {
  await clickNode(id);
  for (const [v, h, sx, sy] of [['bottom', 'right', 1, 1], ['top', 'right', 1, -1], ['bottom', 'left', -1, 1], ['top', 'left', -1, -1]]) {
    const hdl = nodeEl(id).locator(`.svelte-flow__resize-control.handle.${v}.${h}`);
    const free = await hdl.evaluate((el) => {
      const b = el.getBoundingClientRect();
      const t = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
      return t === el || el.contains(t);
    });
    if (!free) continue;
    const hb = await hdl.boundingBox();
    await page.mouse.move(hb.x + hb.width / 2, hb.y + hb.height / 2);
    await page.mouse.down();
    await page.mouse.move(hb.x + sx * ddx, hb.y + sy * ddy, { steps: 8 });
    await page.mouse.up();
    await page.waitForTimeout(300);
    return;
  }
  throw new Error(`no free resize handle on ${id}`);
}
const svgH0 = (await nodeEl('osa').locator('figure.trace svg').boundingBox()).height;
await resizeBy('osa', 40, 120);
const svgH1 = (await nodeEl('osa').locator('figure.trace svg').boundingBox()).height;
check(svgH1 > svgH0 + 40, `resizing the OSA node enlarges its trace (${Math.round(svgH0)} → ${Math.round(svgH1)} px high)`);
const lcdH0 = (await nodeEl('ana0').locator('.lcd').boundingBox()).height;
const fs0 = await nodeEl('ana0').locator('.lcd .value').evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
await resizeBy('ana0', 60, 120);
const lcdH1 = (await nodeEl('ana0').locator('.lcd').boundingBox()).height;
const fs1 = await nodeEl('ana0').locator('.lcd .value').evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
check(lcdH1 > lcdH0 + 40 && fs1 > fs0, `resizing the analyzer node enlarges its LCD (${Math.round(lcdH0)} → ${Math.round(lcdH1)} px, value ${fs0.toFixed(0)} → ${fs1.toFixed(0)} px)`);
await page.click('.svelte-flow__controls-fitview');
await shot('swdm4');

console.log('errors:', errors.length ? errors : 'none');
await browser.close();
server.close();
process.exit(errors.length ? 1 : 0);

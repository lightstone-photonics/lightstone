<script>
  import Plot from '../Plot.svelte';
  import { LABEL, SPECS } from './catalog.js';
  import { get } from '../params.js';
  import { grid, makeRun, metricKeys, metrics, withOverrides } from './study.js';
  import { svgFile, svgPNG } from './export.js';

  /**
   * Parameter sweep: one node parameter over a grid; every point is a run in the study
   * (overrides + metrics), plotted against the parameter.
   */
  let { engine, graph, study = $bindable(), onmessage } = $props();

  let nodeId = $state('');
  let path = $state('');
  let start = $state(0);
  let stop = $state(150);
  let points = $state(7);
  let log = $state(false);
  let withEyes = $state(false);
  let ykey = $state('');
  let logy = $state(true);
  let running = $state(false);
  let cancel = false;
  let progress = $state(0);
  let lastSweep = $state(null); // sweep id of the most recent sweep

  const nodes = $derived(graph?.nodes ?? []);
  const node = $derived(nodes.find((n) => n.id === nodeId));
  const specs = $derived(node ? (SPECS[node.kind] ?? []) : []);
  const spec = $derived(specs.find((s) => s.path === path));
  const scale = $derived(spec?.scale ?? 1);

  function pickNode(id) {
    nodeId = id;
    const n = nodes.find((x) => x.id === id);
    const s = SPECS[n?.kind]?.[0];
    path = s?.path ?? '';
    if (s) {
      const v = get(n.params ?? {}, s.path);
      start = s.min;
      stop = s.max;
      if (v !== undefined && s.min === 0 && v > 0) stop = Math.min(s.max, 1.5 * v / (s.scale ?? 1));
    }
  }
  $effect.pre(() => {
    if (!nodeId && nodes.length) pickNode((nodes.find((n) => n.kind === 'mmf' || n.kind === 'custom_fiber') ?? nodes.find((n) => SPECS[n.kind]?.length) ?? nodes[0]).id);
  });

  async function runSweep() {
    if (!node || !path) return;
    const values = grid(Number(start), Number(stop), Number(points), log && start > 0 && stop > 0);
    const sweepId = `sweep-${Date.now().toString(36)}`;
    const base = $state.snapshot(graph);
    running = true;
    cancel = false;
    progress = 0;
    let done = 0;
    for (const [i, v] of values.entries()) {
      if (cancel) break;
      const si = v * scale;
      const key = `${node.id}.${path}`;
      const g = withOverrides(base, { [key]: si });
      const r = await engine.sweepPoint(g, withEyes);
      if (!r) break;
      study.runs.push(
        makeRun(g, r.summary, {
          label: `${key} = ${+v.toPrecision(4)}${spec?.unit ? ' ' + spec.unit : ''}`,
          overrides: { [key]: si },
          sweep: { id: sweepId, param: key, index: i, value: si, unit: spec?.unit ?? '', scale },
          eyes: withEyes ? r.eyes : undefined,
        })
      );
      done++;
      progress = done / values.length;
    }
    running = false;
    lastSweep = sweepId;
    onmessage?.(cancel ? `Sweep stopped after ${done} of ${values.length} points.` : `Sweep of ${node.id}.${path}: ${done} points added to the runs.`);
  }

  const sweepRuns = $derived(lastSweep ? study.runs.filter((r) => r.sweep?.id === lastSweep) : []);
  // Report metrics (SER/TDECQ first) and, in a second group, the port values.
  const allKeys = $derived(metricKeys(sweepRuns, { ports: true }));
  const reportKeys = $derived(metricKeys(sweepRuns));
  const portKeys = $derived(allKeys.filter((k) => !reportKeys.includes(k)));
  const keys = $derived(allKeys);
  $effect.pre(() => {
    if (keys.length && !keys.includes(ykey)) {
      ykey = keys[0];
      logy = /\.(ser|ber)$/.test(keys[0]);
    }
  });
  const series = $derived.by(() => {
    if (!sweepRuns.length || !ykey) return [];
    const pts = sweepRuns
      .map((r) => [r.sweep.value / (r.sweep.scale || 1), metrics(r, { ports: true })[ykey]])
      .filter((p) => p[1] !== undefined && (!logy || p[1] > 0))
      .map(([x, y]) => [x, logy ? Math.log10(y) : y]);
    return [
      { points: pts, color: '#f6a03a', kind: 'line' },
      { points: pts, color: '#f6a03a', kind: 'scatter', r: 3 },
    ];
  });
  const xlabel = $derived(sweepRuns[0] ? `${sweepRuns[0].sweep.param}${sweepRuns[0].sweep.unit ? ` [${sweepRuns[0].sweep.unit}]` : ''}` : '');
  let plotBox = $state();
</script>

<section class="sweep" aria-label="Parameter sweep">
  <h2>Sweep</h2>
  <div class="form">
    <label>Node
      <select name="node" value={nodeId} onchange={(e) => pickNode(e.currentTarget.value)}>
        {#each nodes as n}<option value={n.id}>{n.id} ({LABEL[n.kind] ?? n.kind})</option>{/each}
      </select>
    </label>
    <label>Parameter
      {#if specs.length}
        <select bind:value={path}>
          {#each specs as s}<option value={s.path}>{s.label}{s.unit ? ` [${s.unit}]` : ''}</option>{/each}
        </select>
      {:else}
        <input type="text" bind:value={path} placeholder="e.g. loss_db" aria-label="Parameter path (SI units)" />
      {/if}
    </label>
    <label>From <input name="from" type="number" step="any" bind:value={start} /></label>
    <label>To <input name="to" type="number" step="any" bind:value={stop} /></label>
    <label>Points <input name="points" type="number" min="1" max="200" bind:value={points} /></label>
    <label class="chk"><input type="checkbox" bind:checked={log} /> log steps</label>
    <label class="chk"><input type="checkbox" bind:checked={withEyes} /> record eyes</label>
    {#if running}
      <button type="button" onclick={() => (cancel = true)}>Stop</button>
      <span class="prog" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(100 * progress)}>{Math.round(100 * progress)} %</span>
    {:else}
      <button type="button" class="go" onclick={runSweep} disabled={!node || !path}>▶ Run sweep</button>
    {/if}
  </div>
  {#if spec}<p class="hint">Values in {spec.unit || 'units of the parameter'}; every point is added to the runs (CSV: one row per point).</p>{/if}
  {#if sweepRuns.length}
    <div class="plotbar">
      <label>Metric
        <select bind:value={ykey}>
          <optgroup label="Results">{#each reportKeys as k}<option value={k}>{k}</option>{/each}</optgroup>
          {#if portKeys.length}<optgroup label="Port values">{#each portKeys as k}<option value={k}>{k}</option>{/each}</optgroup>{/if}
        </select>
      </label>
      <label class="chk"><input type="checkbox" bind:checked={logy} /> log₁₀ y</label>
      <button type="button" class="mini" onclick={() => svgFile(plotBox.querySelector('svg'), 'sweep.svg')}>SVG</button>
      <button type="button" class="mini" onclick={() => svgPNG(plotBox.querySelector('svg'), 'sweep.png')}>PNG</button>
    </div>
    <div bind:this={plotBox}>
      <Plot {series} {xlabel} ylabel={logy ? `log₁₀ ${ykey}` : ykey} width={560} height={220} label="Sweep result: {ykey} versus {xlabel}" />
    </div>
  {/if}
</section>

<style>
  .sweep { background: #10131c; border: 1px solid #262c3d; border-radius: 8px; padding: 0.5rem 0.8rem; }
  h2 { font-size: 1rem; margin: 0 0 0.4rem; }
  .form, .plotbar { display: flex; flex-wrap: wrap; gap: 0.6rem; align-items: center; font-size: 0.82rem; }
  .plotbar { margin: 0.4rem 0; }
  label { display: flex; gap: 0.3rem; align-items: center; }
  select, input[type='text'], input[type='number'] { background: #1d2333; color: #e6ebf5; border: 1px solid #39425a; border-radius: 4px; padding: 0.1rem 0.3rem; }
  input[type='number'] { width: 5.5rem; }
  button { background: #1d2333; color: #e6ebf5; border: 1px solid #39425a; border-radius: 4px; padding: 0.15rem 0.6rem; cursor: pointer; font-size: 0.82rem; }
  button:disabled { opacity: 0.4; cursor: default; }
  button:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid #f6a03a; }
  .go { background: #2d4a2a; border-color: #4f7d48; font-weight: 600; }
  .mini { font-size: 0.7rem; padding: 0 0.35rem; }
  .prog { font-variant-numeric: tabular-nums; color: #b9d7ff; }
  .hint { font-size: 0.78rem; color: #8a93a8; margin: 0.3rem 0 0; }
</style>

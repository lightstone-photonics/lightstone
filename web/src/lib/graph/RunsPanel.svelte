<script>
  import { compare, metrics, metricKeys, toCSV, parseStudy } from './study.js';
  import { download, stamp, safe } from './export.js';

  /**
   * Recorded runs of the graph editor (a study): list, labels, save/open as JSON, CSV export.
   * onload(run): load a run's graph back into the editor.
   */
  let { study = $bindable(), onload, onmessage } = $props();
  let fileInput;
  let picked = $state([]); // run ids selected for comparison
  let showAll = $state(false);
  let withPorts = $state(false); // include all port values in comparison and CSV
  const pickedRuns = $derived(study ? study.runs.filter((r) => picked.includes(r.id)) : []);
  const cmpKeys = $derived(pickedRuns.length >= 2 ? metricKeys(pickedRuns, { ports: withPorts }) : []);
  const rows = $derived(pickedRuns.length >= 2 ? compare(pickedRuns, cmpKeys).filter((r) => showAll || r.differs) : []);
  function togglePick(id, on) {
    picked = on ? [...picked, id] : picked.filter((x) => x !== id);
  }

  const keys = $derived(study ? metricKeys(study) : []);
  // Columns shown in the table: SER, TDECQ and power first, at most four.
  const shown = $derived(keys.filter((k) => /\.(ser|tdecq_db|average_power|emb_length)$/.test(k)).slice(0, 4));
  const fmt = (k, v) => {
    if (v === undefined) return '—';
    if (/average_power$/.test(k)) return v > 0 ? `${(10 * Math.log10(v / 1e-3)).toFixed(2)} dBm` : '—';
    if (/emb_length$/.test(k)) return `${(v / 1e9).toFixed(0)} MHz·km`;
    if (/tdecq_db$/.test(k)) return `${v.toFixed(2)} dB`;
    return Math.abs(v) < 1e-2 && v !== 0 ? v.toExponential(2) : v.toPrecision(4);
  };

  function save() {
    download(`lightstone-results-${stamp()}.json`, JSON.stringify($state.snapshot(study)), 'application/json');
  }
  function csv() {
    const s = $state.snapshot(study);
    download(`lightstone-results-${stamp()}.csv`, toCSV(s, metricKeys(s, { ports: withPorts })), 'text/csv');
  }
  async function open(ev) {
    const input = ev.currentTarget;
    const f = input.files?.[0];
    input.value = '';
    if (!f) return;
    try {
      const s = parseStudy(await f.text());
      study.runs.push(...s.runs);
      onmessage?.(`Loaded ${s.runs.length} run(s) from ${f.name}.`);
    } catch (e) {
      onmessage?.(`Could not load ${f.name}: ${e.message ?? e}`);
    }
  }
  function remove(i) {
    picked = picked.filter((x) => x !== study.runs[i].id);
    study.runs.splice(i, 1);
  }
  function saveOne(r) {
    const one = { ...$state.snapshot(study), runs: [$state.snapshot(r)] };
    download(`lightstone-run-${safe(r.label || r.id)}.json`, JSON.stringify(one), 'application/json');
  }
</script>

<section class="runs" aria-label="Recorded runs">
  <header>
    <h2>Runs <span class="count">{study?.runs.length ?? 0}</span></h2>
    <div class="tools">
      <button type="button" onclick={save} disabled={!study?.runs.length}>Save results (JSON)</button>
      <button type="button" onclick={csv} disabled={!study?.runs.length}>Export CSV</button>
      <label class="all" title="Add the value of every output port (power, mean, swing, RMS noise, symbol count) to the CSV and the comparison, not only the instrument results"><input type="checkbox" class="pick" bind:checked={withPorts} /> include all port values</label>
      <button type="button" onclick={() => fileInput.click()}>Open results…</button>
      <input bind:this={fileInput} type="file" accept=".json,application/json" class="sr-only" tabindex="-1" aria-hidden="true" onchange={open} />
      {#if study?.runs.length}<button type="button" onclick={() => (study.runs = [])}>Clear</button>{/if}
    </div>
  </header>
  {#if study?.runs.length}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th scope="col"><span class="sr-only">Compare</span></th><th scope="col">#</th><th scope="col">Label</th><th scope="col">Time</th>
            {#each shown as k}<th scope="col">{k}</th>{/each}
            <th scope="col"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each study.runs as r, i (r.id)}
            {@const m = metrics(r)}
            <tr class:bad={!r.ok}>
              <td><input type="checkbox" class="pick" checked={picked.includes(r.id)} onchange={(e) => togglePick(r.id, e.currentTarget.checked)} aria-label="Compare run {i + 1}" /></td>
              <td>{i + 1}</td>
              <td><input type="text" bind:value={r.label} aria-label="Label of run {i + 1}" placeholder="label" /></td>
              <td class="time">{r.created.slice(11, 19)}</td>
              {#each shown as k}<td class="num">{r.ok ? fmt(k, m[k]) : 'invalid'}</td>{/each}
              <td class="act">
                <button type="button" onclick={() => onload?.(r)} title="Load this run's graph into the editor">Load graph</button>
                <button type="button" onclick={() => saveOne(r)} title="Save this run only">Save</button>
                <button type="button" onclick={() => remove(i)} aria-label="Delete run {i + 1}">✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if pickedRuns.length >= 2}
      <div class="cmp">
        <h3>Comparison of {pickedRuns.length} runs <label class="all"><input type="checkbox" bind:checked={showAll} /> show equal values too</label></h3>
        <div class="scroll">
          <table>
            <thead><tr><th scope="col">Metric</th>{#each pickedRuns as r}<th scope="col">{r.label || r.id}</th>{/each}</tr></thead>
            <tbody>
              {#each rows as row (row.key)}
                <tr class:same={!row.differs}><th scope="row">{row.key}</th>{#each row.values as v}<td class="num">{fmt(row.key, v)}</td>{/each}</tr>
              {:else}
                <tr><td colspan={pickedRuns.length + 1}>All metrics are equal.</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {:else if study.runs.length >= 2}
      <p class="hint">Tick two or more runs to compare them.</p>
    {/if}
  {:else}
    <p class="hint">Press <strong>Run</strong> to record the current results (graph, settings, instrument and node results, the value at every output port, eyes and signal previews). Records are kept until you clear them or reload the page; save them as JSON or CSV.</p>
  {/if}
</section>

<style>
  .runs { background: #10131c; border: 1px solid #262c3d; border-radius: 8px; padding: 0.5rem 0.8rem; }
  header { display: flex; justify-content: space-between; align-items: center; gap: 0.8rem; flex-wrap: wrap; }
  h2 { font-size: 1rem; margin: 0; }
  .count { font-weight: 400; color: #8a93a8; font-size: 0.85rem; }
  .tools { display: flex; gap: 0.35rem; flex-wrap: wrap; align-items: center; }
  button { background: #1d2333; color: #e6ebf5; border: 1px solid #39425a; border-radius: 4px; padding: 0.15rem 0.5rem; cursor: pointer; font-size: 0.8rem; }
  button:disabled { opacity: 0.4; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid #f6a03a; }
  .scroll { max-height: 14rem; overflow: auto; margin-top: 0.4rem; }
  table { border-collapse: collapse; width: 100%; font-size: 0.8rem; }
  th, td { border-bottom: 1px solid #1e2433; padding: 0.2rem 0.4rem; text-align: left; white-space: nowrap; }
  th { color: #8a93a8; font-weight: 500; position: sticky; top: 0; background: #10131c; }
  td.num { font-variant-numeric: tabular-nums; text-align: right; }
  td.time { color: #8a93a8; }
  td.act { display: flex; gap: 0.25rem; }
  tr.bad td { color: #ffb4ab; }
  input { background: #1d2333; color: #e6ebf5; border: 1px solid #39425a; border-radius: 3px; padding: 0.1rem 0.3rem; width: 11rem; }
  .cmp { margin-top: 0.6rem; }
  .cmp h3 { font-size: 0.85rem; margin: 0 0 0.3rem; display: flex; gap: 1rem; align-items: center; }
  .all { font-weight: 400; font-size: 0.78rem; color: #aab3c5; }
  .cmp th[scope='row'] { color: #c9d1e0; font-weight: 400; font-family: ui-monospace, monospace; font-size: 0.75rem; }
  tr.same td, tr.same th { color: #6c7690; }
  input.pick { width: auto; }
  .hint { font-size: 0.8rem; color: #aab3c5; margin: 0.4rem 0 0.1rem; }
</style>

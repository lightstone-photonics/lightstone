<script>
  import { portKind } from './graph/catalog.js';
  /** @type {{ doc: any, live: Array<[string, string]>, ports?: { inputs: any[], outputs: any[] } }} */
  let { doc, live, ports = null } = $props();
  const portRows = $derived(ports ? [
    ...ports.inputs.map((p) => ({ dir: 'Input', ...p })),
    ...ports.outputs.map((p) => ({ dir: 'Output', ...p })),
  ] : []);
  let open = $state(true);
</script>

{#if doc}
  <section class="math" aria-labelledby="math-title-{doc.id}">
    <header>
      <h3 id="math-title-{doc.id}">{doc.meta.title}: model & math</h3>
      <button type="button" aria-expanded={open} onclick={() => (open = !open)}>
        {open ? 'Hide' : 'Show'} equations
      </button>
    </header>

    {#if portRows.length}
      <table class="ports" aria-label="Ports: what goes in and comes out">
        <caption>Ports</caption>
        <thead><tr><th scope="col">Port</th><th scope="col">Direction</th><th scope="col">Signal</th></tr></thead>
        <tbody>
          {#each portRows as p}
            {@const k = portKind(p.type)}
            <tr>
              <th scope="row"><code>{p.name}</code></th>
              <td>{p.dir}{p.monitor ? ' (monitor: observes only)' : ''}</td>
              <td><span class="sig {k.domain}"><span class="pdot"></span>{k.label}</span> <span class="detail">{k.detail}</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    {#if live?.length}
      <table class="live" aria-label="Live values">
        <caption>Live values</caption>
        <tbody>
          {#each live as [k, v]}
            <tr><th scope="row">{k}</th><td>{v}</td></tr>
          {/each}
        </tbody>
      </table>
    {/if}

    {#if open}
      <div class="doc">{@html doc.html}</div>
    {/if}

    {#if doc.meta.validation?.length}
      <p class="validation">
        Validation tests: {doc.meta.validation.join(', ')}. See <code>docs/validation.md</code>.
      </p>
    {/if}

    {#if doc.references.length}
      <h4>References</h4>
      <ul class="refs">
        {#each doc.references as r}
          <li>
            {#if r.url}
              <a href={r.url} target="_blank" rel="noopener noreferrer">{r.title}</a>
            {:else}
              {r.title}
            {/if}
            <span class="badge {r.access}">{r.access}</span>
            {#if r.note}<div class="note">{r.note}</div>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .math {
    background: #10131c;
    border: 1px solid #262c3d;
    border-radius: 8px;
    padding: 0.8rem 1rem;
    line-height: 1.5;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 1rem;
  }
  h3 {
    margin: 0;
    font-size: 1.05rem;
  }
  button {
    background: #1d2333;
    color: #e6ebf5;
    border: 1px solid #39425a;
    border-radius: 4px;
    padding: 0.25rem 0.6rem;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid #f6a03a;
  }
  .ports {
    border-collapse: collapse;
    margin: 0.2rem 0 0.8rem;
    width: 100%;
    font-size: 0.85rem;
  }
  .ports caption {
    text-align: left;
    font-weight: 600;
    color: #f6a03a;
    margin-bottom: 0.2rem;
  }
  .ports th,
  .ports td {
    text-align: left;
    padding: 0.15rem 0.6rem 0.15rem 0;
    vertical-align: top;
  }
  .ports thead th {
    font-weight: 400;
    font-size: 0.75rem;
    color: #8a93a8;
  }
  .ports tbody th {
    font-weight: 400;
  }
  .sig {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-weight: 600;
  }
  .sig.optical { color: #f08a5d; }
  .sig.electrical { color: #6cb6ff; }
  .sig.data { color: #c9d1e0; }
  .pdot { width: 0.55rem; height: 0.55rem; border-radius: 50%; background: currentColor; }
  .sig.data .pdot { border-radius: 1px; }
  .detail { color: #aab3c5; }
  .live {
    margin: 0.7rem 0;
    border-collapse: collapse;
    font-size: 0.9rem;
  }
  .live caption {
    text-align: left;
    font-weight: 600;
    padding-bottom: 0.2rem;
  }
  .live th {
    text-align: left;
    font-weight: normal;
    color: #aab3c5;
    padding: 0.1rem 1rem 0.1rem 0;
  }
  .live td {
    font-variant-numeric: tabular-nums;
  }
  /* Figures are textbook-style line art on white "paper", readable on the dark panel. */
  .doc :global(img) {
    display: block;
    max-width: 100%;
    height: auto;
    margin: 0.75rem 0 0.25rem;
    padding: 0.4rem;
    background: #fff;
    border-radius: 4px;
  }
  .doc :global(table) {
    border-collapse: collapse;
    font-size: 0.85rem;
    margin: 0.5rem 0;
  }
  .doc :global(th),
  .doc :global(td) {
    border: 1px solid #2a3145;
    padding: 0.2rem 0.45rem;
  }
  .doc :global(h2) {
    font-size: 1rem;
    margin-top: 1.1rem;
    color: #f6c56b;
  }
  .doc :global(code) {
    background: #1b2030;
    padding: 0 0.25rem;
    border-radius: 3px;
  }
  .doc :global(.katex-display) {
    overflow-x: auto;
    overflow-y: hidden;
  }
  .validation {
    font-size: 0.85rem;
    color: #aab3c5;
  }
  h4 {
    margin: 0.8rem 0 0.3rem;
    font-size: 0.95rem;
  }
  .refs {
    font-size: 0.85rem;
    padding-left: 1.1rem;
  }
  .refs a {
    color: #8cc8ff;
  }
  .badge {
    font-size: 0.72rem;
    padding: 0 0.35rem;
    border-radius: 3px;
    margin-left: 0.35rem;
    border: 1px solid;
  }
  .badge.free {
    color: #7ee2a8;
    border-color: #2f7a52;
  }
  .badge.paywalled {
    color: #f3b37a;
    border-color: #7a542f;
  }
  .badge.registration {
    color: #9fc3ff;
    border-color: #2f4f7a;
  }
  .note {
    color: #aab3c5;
  }
</style>

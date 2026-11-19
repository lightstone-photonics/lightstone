<script>
  /**
   * Instrument readout on a measuring node, styled like the LCD of a power meter or BERT:
   * one large main reading and a few secondary rows. The text scales with the node width
   * (container query units); in a resized node without a trace it also scales with the
   * height (GraphNode makes the LCD a size container there), so the display fills the node.
   * display: { main: { label, value, unit }, rows: [[label, value]] }
   */
  let { display } = $props();
</script>

<div class="lcd" role="group" aria-label="{display.main.label}: {display.main.value} {display.main.unit}">
  <div class="mlabel">{display.main.label}</div>
  <div class="main">
    <span class="value">{display.main.value}</span>
    {#if display.main.unit}<span class="unit">{display.main.unit}</span>{/if}
  </div>
  {#if display.rows?.length}
    <dl>
      {#each display.rows as [k, v]}
        <dt>{k}</dt>
        <dd>{v}</dd>
      {/each}
    </dl>
  {/if}
</div>

<style>
  .lcd {
    container-type: inline-size;
    margin-top: 0.35rem;
    padding: 0.3rem 0.5rem 0.35rem;
    border-radius: 4px;
    background: linear-gradient(180deg, #0c1a12 0%, #08130d 100%);
    border: 1px solid #22402c;
    box-shadow: inset 0 0 8px rgba(0, 0, 0, 0.7);
    color: #8dffb0;
    font-family: ui-monospace, 'DejaVu Sans Mono', Menlo, monospace;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 0 4px rgba(120, 255, 170, 0.35);
    overflow: hidden;
  }
  .mlabel {
    font-size: clamp(8px, min(5cqi, 9cqb), 18px);
    letter-spacing: 0.08em;
    color: #5fbf80;
  }
  .main {
    display: flex;
    align-items: baseline;
    justify-content: flex-end;
    gap: 0.3em;
    line-height: 1.05;
    white-space: nowrap;
  }
  .value {
    font-size: clamp(16px, min(13cqi, 26cqb), 72px);
    font-weight: 600;
  }
  .unit {
    font-size: clamp(9px, min(6cqi, 12cqb), 30px);
    color: #6fd894;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0 0.6em;
    margin: 0.2rem 0 0;
    font-size: clamp(8px, min(5cqi, 8cqb), 20px);
  }
  dt {
    color: #5fbf80;
  }
  dd {
    margin: 0;
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>

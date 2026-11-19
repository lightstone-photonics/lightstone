<script>
  import { Handle, NodeResizer, Position } from '@xyflow/svelte';
  import { domain } from './catalog.js';
  import GraphSymbol from './GraphSymbol.svelte';
  import EyeThumb from './EyeThumb.svelte';
  import TraceThumb from './TraceThumb.svelte';
  import InstrumentDisplay from './InstrumentDisplay.svelte';

  /**
   * Graph editor node. data: { label, kind, inputs, outputs, badge, issue, warning, linear,
   * eye } (inputs/outputs: [{ name, type, monitor }]; badge: a string or a list of lines;
   * eye: { counts, meta } of an eye_diagram node from the last run, drawn as a thumbnail;
   * display: instrument readout of a measuring node, shown instead of the badge lines;
   * sized: the user resized the node; onsize(id, {x, y, width, height}) after a resize).
   */
  let { id, data, selected } = $props();

  const rows = $derived(Math.max(data.inputs.length, data.outputs.length, 1));
  const top = (i, n) => `${((i + 1) / (n + 1)) * 100}%`;
  const lines = $derived(Array.isArray(data.badge) ? data.badge.filter(Boolean) : data.badge ? [data.badge] : []);
  const title = (p, side) => `${side} ${p.name}: ${p.type}${p.monitor ? ' (monitor, observes only)' : ''}`;
</script>

<NodeResizer
  minWidth={190}
  minHeight={56}
  isVisible={selected}
  color="#f6a03a"
  onResizeEnd={(_, p) => data.onsize?.(id, p)}
/>
<div
  class="node"
  class:sized={data.sized}
  class:selected
  class:issue={!!data.issue}
  class:warning={!data.issue && !!data.warning}
  class:sink={!data.outputs.length}
  class:has-trace={!!data.trace}
  style={data.sized ? '' : `min-height: ${Math.max(56, 22 * rows + 22)}px`}
  aria-label="{data.label} ({id}){lines.length ? `: ${lines.join(', ')}` : ''}{data.issue ? `. Problem: ${data.issue}` : ''}"
>
  {#each data.inputs as p, i (p.name)}
    <Handle
      type="target"
      position={Position.Left}
      id={p.name}
      class="port {domain(p.type)}{p.monitor ? ' monitor' : ''}"
      style="top: {top(i, data.inputs.length)}"
      title={title(p, 'input')}
    />
    {#if data.inputs.length > 1 || p.name !== 'in'}
      <span class="plabel in" style="top: {top(i, data.inputs.length)}">{p.name}</span>
    {/if}
  {/each}
  <div class="body">
    <GraphSymbol kind={data.kind} fiberType={data.fiberType} />
    <div class="text">
      <div class="head">
        <span class="kind">{data.label}</span>
        <span class="id">{id}</span>
      </div>
      {#if !data.display}
        {#each lines as l, i}<div class="badge" class:first={i === 0} title={l}>{l}</div>{/each}
      {/if}
    </div>
  </div>
  {#if data.display}<InstrumentDisplay display={data.display} />{/if}
  {#if data.trace}
    <TraceThumb {...data.trace} label="Spectrum of {id} from the last run" />
  {/if}
  {#if data.eye?.counts?.length && data.eye.meta}
    <EyeThumb counts={data.eye.counts} meta={data.eye.meta} label="Eye diagram of {id} from the last run" />
  {/if}
  {#each data.outputs as p, i (p.name)}
    <Handle
      type="source"
      position={Position.Right}
      id={p.name}
      class="port {domain(p.type)}"
      style="top: {top(i, data.outputs.length)}"
      title={title(p, 'output')}
    />
    {#if data.outputs.length > 1 || p.name !== 'out'}
      <span class="plabel out" style="top: {top(i, data.outputs.length)}">{p.name}</span>
    {/if}
  {/each}
</div>

<style>
  .node {
    position: relative;
    min-width: 190px;
    max-width: 250px;
    padding: 0.35rem 0.75rem;
    background: #151a27;
    border: 1px solid #39425a;
    border-radius: 6px;
    color: #e6ebf5;
    font-size: 0.8rem;
    box-sizing: border-box;
  }
  .node.sized {
    width: 100%;
    height: 100%;
    max-width: none;
    display: flex;
    flex-direction: column;
    /* No overflow clipping here: the ports sit half outside the border. */
  }
  .node.sized :global(.thumb) {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .node.sized .body,
  .node.sized.has-trace :global(.lcd) {
    flex-shrink: 0;
  }
  .node.sized :global(.thumb canvas) {
    height: 100%;
    aspect-ratio: auto;
  }
  /* Spectrum analyzers: the trace screen takes the extra height. */
  .node.sized :global(.trace) {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .node.sized :global(.trace svg) {
    flex: 1;
    min-height: 40px;
    height: auto;
  }
  /* Instrument display without a trace (PAM-4 analyzer, power meter, TDECQ): the LCD takes
     the extra height and its text scales with the smaller of width and height. */
  .node.sized:not(.has-trace) :global(.lcd) {
    flex: 1 1 auto;
    min-height: 0;
    container-type: size;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }
  .node.sink {
    border-style: dashed;
  }
  .node.selected {
    border-color: #f6a03a;
    box-shadow: 0 0 0 1px #f6a03a;
  }
  .node.issue {
    border-color: #ff6b6b;
    background: #2a1418;
  }
  .node.warning {
    border-color: #d9b44a;
  }
  .body {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .text {
    min-width: 0;
    flex: 1;
  }
  .head {
    display: flex;
    justify-content: space-between;
    gap: 0.6rem;
    align-items: baseline;
  }
  .kind {
    font-weight: 600;
  }
  .id {
    color: #8a93a8;
    font-family: ui-monospace, monospace;
    font-size: 0.72rem;
  }
  .badge {
    margin-top: 0.05rem;
    color: #b9d7ff;
    font-variant-numeric: tabular-nums;
    font-size: 0.75rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .badge:hover {
    white-space: normal;
  }
  .badge.first {
    margin-top: 0.2rem;
  }
  .plabel {
    position: absolute;
    transform: translateY(-50%);
    font-size: 0.62rem;
    color: #8a93a8;
    pointer-events: none;
  }
  .plabel.in {
    left: 9px;
  }
  .plabel.out {
    right: 9px;
  }
  :global(.svelte-flow .port) {
    width: 11px;
    height: 11px;
    border: 2px solid #0a0c12;
  }
  :global(.svelte-flow .port.optical) {
    background: #f08a5d;
  }
  :global(.svelte-flow .port.electrical) {
    background: #6cb6ff;
  }
  :global(.svelte-flow .port.data) {
    background: #b7bfd0;
    border-radius: 2px;
  }
  :global(.svelte-flow .port.monitor) {
    background: #0a0c12;
    border: 2px solid #f08a5d;
  }
</style>

<script>
  import { NodeResizer } from '@xyflow/svelte';

  /** A frame: a named area behind the nodes (e.g. Transmitter, Channel). data: { label, onresize } */
  let { id, data, selected } = $props();
</script>

<NodeResizer minWidth={120} minHeight={60} isVisible={selected} color="#f6a03a" onResizeEnd={(_, p) => data.onresize?.(id, p)} />
<div class="frame" class:selected aria-label="Frame {data.label}">
  <span class="label">{data.label}</span>
</div>

<style>
  .frame {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 1px dashed #4a5370;
    border-radius: 10px;
    background: rgba(120, 140, 190, 0.05);
  }
  .frame.selected {
    border-color: #f6a03a;
  }
  .label {
    position: absolute;
    left: 12px;
    top: 6px;
    font-size: 11px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: #8a93a8;
    pointer-events: none;
  }
</style>

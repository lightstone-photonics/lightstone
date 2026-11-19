<script>
  /**
   * Small trace plot on a spectrum-analyzer node: one or two traces over a common x axis,
   * y in dB. `traces`: [{ y, color }]; `range`: [ymin, ymax] (dB); `xlabel`: axis text.
   */
  let { x, traces, range, xlabel, label } = $props();
  const W = 200;
  const H = 70;
  const path = (y) => {
    const [lo, hi] = range;
    const n = y.length;
    return y
      .map((v, i) => {
        const px = (i / Math.max(1, n - 1)) * W;
        const py = H - ((Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo)) * H;
        return `${i ? 'L' : 'M'}${px.toFixed(1)},${py.toFixed(1)}`;
      })
      .join('');
  };
</script>

<figure class="trace" aria-label={label}>
  <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label={label}>
    {#each [0.25, 0.5, 0.75] as g}<line x1="0" x2={W} y1={g * H} y2={g * H} class="grid" />{/each}
    {#each [0.25, 0.5, 0.75] as g}<line y1="0" y2={H} x1={g * W} x2={g * W} class="grid" />{/each}
    {#each traces as t}<path d={path(t.y)} style="stroke: {t.color}" class="tr" />{/each}
  </svg>
  <figcaption>{xlabel}</figcaption>
</figure>

<style>
  .trace { margin: 4px 0 0; }
  svg { width: 100%; height: 70px; display: block; background: #05070c; border: 1px solid #2a3142; border-radius: 3px; }
  .grid { stroke: #1f2636; stroke-width: 0.6; vector-effect: non-scaling-stroke; }
  .tr { fill: none; stroke-width: 1.2; vector-effect: non-scaling-stroke; }
  figcaption { font-size: 9px; color: #8b94a7; text-align: center; }
</style>

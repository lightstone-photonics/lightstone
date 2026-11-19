<script>
  /**
   * Minimal SVG plot: line and scatter series, nice ticks, optional horizontal markers.
   * series: [{ points: [[x, y], ...], color, kind: 'line' | 'scatter', r?: number[] | number, dash?: string, label? }]
   */
  let {
    series = [],
    xlabel = '',
    ylabel = '',
    xdomain = null,
    ydomain = null,
    hlines = [],
    width = 320,
    height = 200,
    label = 'plot',
  } = $props();

  const uid = $props.id();
  const M = { l: 48, r: 10, t: 10, b: 34 };

  function niceTicks(lo, hi, n = 5) {
    if (!(hi > lo)) return [lo];
    const raw = (hi - lo) / n;
    const mag = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
    const out = [];
    for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9 * step; v += step) out.push(+v.toPrecision(12));
    return out;
  }

  function extent(axis) {
    let lo = Infinity, hi = -Infinity;
    for (const s of series) for (const p of s.points) {
      const v = p[axis];
      if (isFinite(v)) { lo = Math.min(lo, v); hi = Math.max(hi, v); }
    }
    if (!isFinite(lo)) return [0, 1];
    if (lo === hi) return [lo - 1, hi + 1];
    const pad = axis === 1 ? 0.06 * (hi - lo) : 0;
    return [lo - pad, hi + pad];
  }

  const xd = $derived(xdomain ?? extent(0));
  const yd = $derived(ydomain ?? extent(1));
  const pw = $derived(width - M.l - M.r);
  const ph = $derived(height - M.t - M.b);
  const sx = (x) => M.l + ((x - xd[0]) / (xd[1] - xd[0])) * pw;
  const sy = (y) => M.t + (1 - (y - yd[0]) / (yd[1] - yd[0])) * ph;
  const clampY = (y) => Math.max(yd[0], Math.min(yd[1], y));
  const fmt = (v) => (Math.abs(v) >= 1000 || (Math.abs(v) < 0.01 && v !== 0) ? v.toExponential(0) : +v.toPrecision(3));
</script>

<svg viewBox="0 0 {width} {height}" role="img" aria-label={label}>
  <rect x={M.l} y={M.t} width={pw} height={ph} class="frame" />
  {#each niceTicks(xd[0], xd[1]) as t}
    <line x1={sx(t)} x2={sx(t)} y1={M.t} y2={M.t + ph} class="grid" />
    <text x={sx(t)} y={M.t + ph + 13} class="tick" text-anchor="middle">{fmt(t)}</text>
  {/each}
  {#each niceTicks(yd[0], yd[1], 4) as t}
    <line x1={M.l} x2={M.l + pw} y1={sy(t)} y2={sy(t)} class="grid" />
    <text x={M.l - 5} y={sy(t) + 3} class="tick" text-anchor="end">{fmt(t)}</text>
  {/each}
  {#each hlines as h}
    {#if h.y >= yd[0] && h.y <= yd[1]}
      <line x1={M.l} x2={M.l + pw} y1={sy(h.y)} y2={sy(h.y)} class="hline" />
      <text x={M.l + pw - 4} y={sy(h.y) - 4} class="tick hl" text-anchor="end">{h.label}</text>
    {/if}
  {/each}
  <clipPath id="clip-{uid}"><rect x={M.l} y={M.t} width={pw} height={ph} /></clipPath>
  <g clip-path="url(#clip-{uid})">
    {#each series as s}
      {#if s.kind === 'scatter'}
        {#each s.points as p, i}
          <circle cx={sx(p[0])} cy={sy(p[1])} r={Array.isArray(s.r) ? s.r[i] : (s.r ?? 2.5)} fill={s.color} fill-opacity="0.75" />
        {/each}
      {:else}
        <polyline
          points={s.points.filter((p) => isFinite(p[1])).map((p) => `${sx(p[0])},${sy(clampY(p[1]))}`).join(' ')}
          fill="none"
          stroke={s.color}
          stroke-width="1.6"
          stroke-dasharray={s.dash ?? ''}
        />
      {/if}
    {/each}
  </g>
  <text x={M.l + pw / 2} y={height - 4} class="axis" text-anchor="middle">{xlabel}</text>
  <text transform="translate(11 {M.t + ph / 2}) rotate(-90)" class="axis" text-anchor="middle">{ylabel}</text>
  {#if series.some((s) => s.label)}
    <g transform="translate({M.l + 8} {M.t + 12})">
      {#each series.filter((s) => s.label) as s, i}
        <line x1="0" x2="14" y1={i * 13 - 3} y2={i * 13 - 3} stroke={s.color} stroke-width="2" stroke-dasharray={s.dash ?? ''} />
        <text x="18" y={i * 13} class="tick legend">{s.label}</text>
      {/each}
    </g>
  {/if}
</svg>

<style>
  svg { width: 100%; height: auto; display: block; }
  .frame { fill: #0b0e16; stroke: #2a3145; }
  .grid { stroke: #1c2232; }
  .tick { fill: #aab3c5; font-size: 9.5px; }
  .tick.legend { fill: #d4dae6; }
  .axis { fill: #c9d1e0; font-size: 10.5px; }
  .hline { stroke: #e0675a; stroke-dasharray: 4 3; }
  .tick.hl { fill: #e0675a; }
</style>

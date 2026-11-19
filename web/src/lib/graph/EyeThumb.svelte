<script>
  import { EYE_BG, EYE_LUT, eyeIndex } from '../colormap.js';

  /**
   * Small eye diagram drawn on an `eye_diagram` node: the density from the last run
   * (`counts`, row-major, row 0 = top) with the report's size and range (`meta`).
   */
  let { counts, meta, label } = $props();
  let canvas = $state();

  $effect(() => {
    if (!canvas || !counts?.length || !meta) return;
    const { width: w, height: h, max_count: maxc } = meta;
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    const img = ctx.createImageData(w, h);
    for (let i = 0; i < w * h; i++) {
      const c = counts[i];
      const o = i * 4;
      if (!c) {
        img.data[o] = EYE_BG[0];
        img.data[o + 1] = EYE_BG[1];
        img.data[o + 2] = EYE_BG[2];
        img.data[o + 3] = 255;
        continue;
      }
      const t = eyeIndex(c, maxc);
      img.data[o] = EYE_LUT[t * 3];
      img.data[o + 1] = EYE_LUT[t * 3 + 1];
      img.data[o + 2] = EYE_LUT[t * 3 + 2];
      img.data[o + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
    // Sampling instant (window centre).
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.35)';
    ctx.lineWidth = Math.max(1, w / 200);
    ctx.beginPath();
    ctx.moveTo(w / 2, 0);
    ctx.lineTo(w / 2, h);
    ctx.stroke();
  });
</script>

<div class="thumb" role="img" aria-label={label}><canvas bind:this={canvas} aria-hidden="true"></canvas></div>

<style>
  .thumb {
    margin-top: 0.35rem;
  }
  canvas {
    display: block;
    width: 100%;
    height: auto;
    aspect-ratio: 3 / 2;
    border-radius: 4px;
    background: #0f110f;
    border: 1px solid #262c3d;
  }
</style>

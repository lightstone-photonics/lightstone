<script>
  import { EYE_BG, EYE_LUT, eyeIndex } from './colormap.js';

  /** @type {{ eye: Uint32Array | null, meta: any, analysis: any, label: string }} */
  let { eye, meta, analysis, label } = $props();
  let canvas = $state();

  const PAD = { left: 56, right: 12, top: 12, bottom: 34 };

  $effect(() => {
    if (!canvas || !eye || !meta) return;
    const { width: w, height: h, max_count: maxc } = meta;
    const ctx = canvas.getContext('2d');
    canvas.width = w + PAD.left + PAD.right;
    canvas.height = h + PAD.top + PAD.bottom;
    ctx.fillStyle = `rgb(${EYE_BG.join(',')})`;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Log-scaled density for a scope-like glow.
    const img = ctx.createImageData(w, h);
    for (let i = 0; i < w * h; i++) {
      const c = eye[i];
      const o = i * 4;
      if (c === 0) {
        img.data[o + 3] = 0;
        continue;
      }
      const t = eyeIndex(c, maxc);
      img.data[o] = EYE_LUT[t * 3];
      img.data[o + 1] = EYE_LUT[t * 3 + 1];
      img.data[o + 2] = EYE_LUT[t * 3 + 2];
      img.data[o + 3] = 255;
    }
    ctx.putImageData(img, PAD.left, PAD.top);
    // Scope graticule: 8 × 8 divisions, faint, on top of the traces.
    ctx.strokeStyle = 'rgba(200, 200, 200, 0.12)';
    ctx.lineWidth = 1;
    for (let i = 0; i <= 8; i++) {
      const x = Math.round(PAD.left + (i / 8) * w) + 0.5;
      const y = Math.round(PAD.top + (i / 8) * h) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, PAD.top);
      ctx.lineTo(x, PAD.top + h);
      ctx.moveTo(PAD.left, y);
      ctx.lineTo(PAD.left + w, y);
      ctx.stroke();
    }

    const yOf = (v) => PAD.top + ((meta.v_max - v) / (meta.v_max - meta.v_min)) * h;
    // Thresholds.
    if (analysis) {
      ctx.setLineDash([4, 4]);
      ctx.strokeStyle = 'rgba(120, 200, 255, 0.7)';
      for (const th of analysis.thresholds) {
        const y = yOf(th);
        ctx.beginPath();
        ctx.moveTo(PAD.left, y);
        ctx.lineTo(PAD.left + w, y);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      // Sampling instant (window center).
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.35)';
      ctx.beginPath();
      ctx.moveTo(PAD.left + w / 2, PAD.top);
      ctx.lineTo(PAD.left + w / 2, PAD.top + h);
      ctx.stroke();
    }

    // Axes.
    ctx.fillStyle = '#c9d1e0';
    ctx.font = '12px system-ui, sans-serif';
    ctx.textAlign = 'center';
    for (const ui of [-1, -0.5, 0, 0.5, 1]) {
      const x = PAD.left + ((ui + 1) / 2) * w;
      ctx.fillText(`${ui} UI`, x, PAD.top + h + 18);
    }
    ctx.textAlign = 'right';
    const span = meta.v_max - meta.v_min;
    for (let k = 0; k <= 4; k++) {
      const v = meta.v_min + (k / 4) * span;
      ctx.fillText(`${(v * 1e3).toFixed(0)} mV`, PAD.left - 6, yOf(v) + 4);
    }
  });
</script>

<div role="img" aria-label={label}><canvas bind:this={canvas} aria-hidden="true"></canvas></div>

<style>
  canvas {
    width: 100%;
    height: auto;
    max-width: 680px;
    display: block;
    border-radius: 6px;
    background: #0f110f;
  }
</style>

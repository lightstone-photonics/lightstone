<script>
  import { LUT, diverging } from './colormap.js';

  /** @type {{ image: Float32Array | null, n: number, kind: number, extent: number, coreRadius: number, offset: number, label: string }} */
  let { image, n, kind, extent, coreRadius, offset, label } = $props();
  let canvas = $state();
  const S = 300;

  $effect(() => {
    if (!canvas || !image || image.length !== n * n) return;
    const ctx = canvas.getContext('2d');
    canvas.width = S;
    canvas.height = S;
    const off = new OffscreenCanvas(n, n);
    const octx = off.getContext('2d');
    const img = octx.createImageData(n, n);
    let max = 0;
    for (const v of image) max = Math.max(max, Math.abs(v));
    max = max || 1;
    for (let iy = 0; iy < n; iy++) {
      for (let ix = 0; ix < n; ix++) {
        // Flip y so +y points up.
        const v = image[(n - 1 - iy) * n + ix] / max;
        const o = (iy * n + ix) * 4;
        let rgb;
        if (kind === 2) rgb = diverging(v);
        else {
          const t = Math.round(255 * Math.sqrt(Math.max(0, v)));
          rgb = [LUT[t * 3], LUT[t * 3 + 1], LUT[t * 3 + 2]];
        }
        img.data[o] = rgb[0];
        img.data[o + 1] = rgb[1];
        img.data[o + 2] = rgb[2];
        img.data[o + 3] = 255;
      }
    }
    octx.putImageData(img, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(off, 0, 0, S, S);

    // Core boundary and fiber axis.
    const px = (m) => (m / (2 * extent)) * S;
    ctx.strokeStyle = 'rgba(160, 220, 255, 0.85)';
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(S / 2, S / 2, px(coreRadius), 0, 2 * Math.PI);
    ctx.stroke();
    ctx.fillStyle = 'rgba(160, 220, 255, 0.9)';
    ctx.fillRect(S / 2 - 1, S / 2 - 1, 3, 3);
    if (kind === 0 && offset !== 0) {
      ctx.strokeStyle = 'rgba(255,255,255,0.6)';
      ctx.beginPath();
      ctx.moveTo(S / 2, S / 2);
      ctx.lineTo(S / 2 + px(offset), S / 2);
      ctx.stroke();
    }
  });
</script>

<div role="img" aria-label={label}><canvas bind:this={canvas} aria-hidden="true"></canvas></div>
<p class="fov">{(2 * extent * 1e6).toFixed(0)} µm field of view · circle = 50 µm core</p>

<style>
  .fov {
    text-align: center;
    font-size: 0.8rem;
    color: #aab3c5;
    margin: 0.3rem 0 0;
  }
  canvas {
    width: 100%;
    max-width: 300px;
    aspect-ratio: 1;
    border-radius: 50%;
    display: block;
    margin: 0 auto;
  }
</style>

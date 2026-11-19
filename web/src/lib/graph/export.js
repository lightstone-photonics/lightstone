// Downloads: text/JSON/CSV files, canvases as PNG, SVG plots as SVG or PNG.

export function download(name, data, type = 'application/octet-stream') {
  const blob = data instanceof Blob ? data : new Blob([data], { type });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

export const stamp = () => new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');

/** A filename-safe version of `s`. */
export const safe = (s) => String(s).replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^_+|_+$/g, '') || 'export';

export function canvasPNG(canvas, name) {
  canvas.toBlob((b) => b && download(name, b), 'image/png');
}

/** Standalone SVG text of an inline <svg>, with computed styles of its elements inlined. */
export function svgText(svg) {
  const clone = svg.cloneNode(true);
  const src = svg.querySelectorAll('*');
  const dst = clone.querySelectorAll('*');
  const props = ['fill', 'stroke', 'stroke-width', 'stroke-dasharray', 'opacity', 'font-size', 'font-family', 'font-weight', 'text-anchor', 'dominant-baseline'];
  src.forEach((el, i) => {
    const cs = getComputedStyle(el);
    dst[i].setAttribute('style', props.map((p) => `${p}:${cs.getPropertyValue(p)}`).join(';'));
  });
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
  const { width, height } = svg.getBoundingClientRect();
  clone.setAttribute('width', String(Math.round(width)));
  clone.setAttribute('height', String(Math.round(height)));
  // Dark plot background, as on screen.
  const bg = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  bg.setAttribute('width', '100%');
  bg.setAttribute('height', '100%');
  bg.setAttribute('fill', '#10131c');
  clone.insertBefore(bg, clone.firstChild);
  return new XMLSerializer().serializeToString(clone);
}

export function svgFile(svg, name) {
  download(name, svgText(svg), 'image/svg+xml');
}

export function svgPNG(svg, name, scale = 2) {
  const text = svgText(svg);
  const { width, height } = svg.getBoundingClientRect();
  const img = new Image();
  img.onload = () => {
    const c = document.createElement('canvas');
    c.width = Math.round(width * scale);
    c.height = Math.round(height * scale);
    const ctx = c.getContext('2d');
    ctx.scale(scale, scale);
    ctx.drawImage(img, 0, 0);
    canvasPNG(c, name);
  };
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(text);
}

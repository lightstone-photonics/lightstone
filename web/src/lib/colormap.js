// Perceptually ordered "glow" colormap (black → violet → orange → pale yellow), 256 entries.
const stops = [
  [0.0, [0, 0, 4]],
  [0.15, [40, 11, 84]],
  [0.35, [120, 28, 109]],
  [0.55, [204, 60, 79]],
  [0.75, [246, 129, 33]],
  [0.9, [251, 196, 69]],
  [1.0, [252, 255, 164]],
];

export const LUT = (() => {
  const lut = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let k = 0;
    while (k < stops.length - 2 && t > stops[k + 1][0]) k++;
    const [t0, c0] = stops[k];
    const [t1, c1] = stops[k + 1];
    const u = (t - t0) / (t1 - t0);
    for (let j = 0; j < 3; j++) lut[i * 3 + j] = c0[j] + u * (c1[j] - c0[j]);
  }
  return lut;
})();

// Color-graded eye diagram like a sampling scope (Keysight style): rare traces green, then
// blue, magenta, red, orange, and the most frequent yellow. The stop colors were measured
// from a Keysight scope screenshot; the stop positions are set so that a typical simulated
// eye has about the same share of each color as that screenshot (mostly orange and yellow,
// thin green/blue/magenta fringes). Index 0 = lowest nonzero count, 255 = maximum.
const eyeStops = [
  [0.0, [34, 96, 52]],
  [0.1, [56, 143, 83]],
  [0.23, [70, 80, 156]],
  [0.32, [184, 64, 173]],
  [0.43, [189, 30, 40]],
  [0.56, [244, 136, 11]],
  [0.8, [233, 201, 9]],
  [1.0, [250, 240, 170]],
];

function build(st) {
  const lut = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let k = 0;
    while (k < st.length - 2 && t > st[k + 1][0]) k++;
    const [t0, c0] = st[k];
    const [t1, c1] = st[k + 1];
    const u = (t - t0) / (t1 - t0);
    for (let j = 0; j < 3; j++) lut[i * 3 + j] = c0[j] + u * (c1[j] - c0[j]);
  }
  return lut;
}

export const EYE_LUT = build(eyeStops);
/** Scope screen background behind the eye. */
export const EYE_BG = [15, 17, 15];

/** LUT index of an eye count `c` (> 0) with maximum `maxc`: logarithmic color grading. */
export function eyeIndex(c, maxc) {
  return Math.min(255, Math.round((255 * Math.log1p(c)) / Math.log1p(Math.max(1, maxc))));
}

// Diverging map for signed mode fields (blue – near-black – red).
export function diverging(v) {
  const t = Math.max(-1, Math.min(1, v));
  if (t >= 0) return [Math.round(20 + 235 * t), Math.round(20 + 60 * t), Math.round(30 + 20 * t)];
  return [Math.round(20 - 0 * t), Math.round(20 - 110 * t), Math.round(30 - 225 * t)];
}

<script>
  import Plot from './Plot.svelte';

  /** Level-1 view of the OM4 fiber: profile, modal delays, transfer function, impulse response. */
  let { summary } = $props();

  const f = $derived(summary?.fiber);
  const L = $derived(f?.length ?? 0);

  // Index profile Δn(r) = n(r) − n₂ (×10³).
  const profile = $derived.by(() => {
    if (!f) return [];
    const { n_core: n1, n_clad: n2, core_radius: a } = f;
    const alpha = f.alpha ?? Infinity;
    const d = (n1 * n1 - n2 * n2) / (2 * n1 * n1);
    const pts = [];
    for (let i = 0; i <= 300; i++) {
      const r = -1.6 * a + (3.2 * a * i) / 300;
      const x = Math.abs(r) / a;
      const n = x < 1 ? n1 * Math.sqrt(1 - 2 * d * (alpha === Infinity ? 0 : x ** alpha)) : n2;
      pts.push([r * 1e6, (n - n2) * 1e3]);
    }
    return pts;
  });

  // Modes carrying < 1e-4 of the strongest mode's power are hidden from the plots: near
  // cutoff they have very large delays but no visible effect on the signal.
  const REL_MIN = 1e-4;

  // Relative delay per mode at length L vs. group; marker area ∝ launched power.
  const dmd = $derived.by(() => {
    if (!summary) return { pts: [], r: [], hidden: 0 };
    const all = summary.modes;
    const pmax = Math.max(...all.map((m) => m.launched_power)) || 1;
    const ms = all.filter((m) => m.launched_power >= REL_MIN * pmax);
    const pts = ms.map((m) => [m.group, m.path_delay * 1e12]);
    // y-range from modes with ≥ 1 % of the peak power; weaker outliers (near-cutoff modes
    // with large delays) are clipped and counted.
    const strong = ms.filter((m) => m.launched_power >= 1e-2 * pmax).map((m) => m.path_delay * 1e12);
    let lo = Math.min(...strong, 0), hi = Math.max(...strong, 0);
    const pad = 0.25 * (hi - lo || 1);
    lo -= pad; hi += pad;
    return {
      pts,
      r: ms.map((m) => 1.2 + 6 * Math.sqrt(m.launched_power / pmax)),
      hidden: all.length - ms.length,
      clipped: pts.filter((p) => p[1] < lo || p[1] > hi).length,
      domain: [lo, hi],
    };
  });

  const db = (v) => (v > 0 ? 10 * Math.log10(v) : -60);
  const hTotal = $derived(f ? f.freq.map((x, i) => [x / 1e9, db(f.mag[i])]) : []);
  const hModal = $derived(f ? f.freq.map((x, i) => [x / 1e9, db(f.mag_modal[i])]) : []);

  // Power-weighted impulse response: Σ w_m · N(t; τ_m, σ_m), σ floored for display.
  const impulse = $derived.by(() => {
    if (!f || !f.terms.length) return [];
    const floor = 0.25e-12;
    // Time span covering 99.9 % of the power (a few weak, early near-cutoff modes would
    // otherwise stretch the axis).
    const tot0 = f.terms.reduce((a, t) => a + t.weight, 0);
    const byDelay = [...f.terms].sort((a, b) => Math.abs(a.delay) - Math.abs(b.delay));
    let acc = 0;
    let span = 3e-12;
    for (const t of byDelay) {
      acc += t.weight;
      span = Math.max(span, Math.abs(t.delay) + 4 * Math.max(t.sigma, floor));
      if (acc >= 0.999 * tot0) break;
    }
    const tot = f.terms.reduce((a, t) => a + t.weight, 0);
    const pts = [];
    for (let i = 0; i <= 400; i++) {
      const tt = -span + (2 * span * i) / 400;
      let v = 0;
      for (const t of f.terms) {
        const s = Math.max(t.sigma, floor);
        v += (t.weight / tot) * Math.exp(-((tt - t.delay) ** 2) / (2 * s * s)) / (s * Math.sqrt(2 * Math.PI));
      }
      pts.push([tt * 1e12, v * 1e-12]);
    }
    return pts;
  });
</script>

{#if summary && f}
  <div class="grid">
    <figure>
      <figcaption>Index profile n(r) − n₂ (span {f.element + 1}) <span>α = {f.alpha?.toFixed(3) ?? 'step'}</span></figcaption>
      <Plot
        series={[{ points: profile, color: '#8cc8ff' }]}
        xlabel="radius r [µm]"
        ylabel="Δn × 10³"
        label="Refractive index profile of the fiber"
      />
    </figure>
    <figure>
      <figcaption>Relative group delay per mode, whole lane (L = {L} m) <span>marker size ∝ launched power{dmd.hidden ? ` · ${dmd.hidden} modes < 10⁻⁴ hidden` : ''}{dmd.clipped ? ` · ${dmd.clipped} weak modes off-scale` : ''}</span></figcaption>
      <Plot
        series={[{ points: dmd.pts, color: '#f6a03a', kind: 'scatter', r: dmd.r }]}
        ydomain={dmd.domain}
        xlabel="principal mode group g"
        ylabel="τ − τ(LP01) [ps]"
        label="Differential mode delay per mode"
      />
    </figure>
    <figure>
      <figcaption>Lane channel |H(f)| <span>optical dB, all spans and losses</span></figcaption>
      <Plot
        series={[
          { points: hModal, color: '#9fc3ff', dash: '4 3', label: 'modal only' },
          { points: hTotal, color: '#f6a03a', label: 'modal + chromatic' },
        ]}
        xlabel="frequency [GHz]"
        ylabel="10·log₁₀|H/H(0)| [dB]"
        ydomain={[-15, 0.5]}
        hlines={[{ y: -3, label: '−3 dB' }]}
        label="Fiber channel transfer function"
      />
    </figure>
    <figure>
      <figcaption>Impulse response (power) <span>mean delay removed · 99.9 % of power shown</span></figcaption>
      <Plot
        series={[{ points: impulse, color: '#f6c56b' }]}
        xlabel="time [ps]"
        ylabel="h(t) [1/ps]"
        label="Fiber impulse response"
      />
    </figure>
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.8rem;
  }
  figure {
    margin: 0;
  }
  figcaption {
    font-size: 0.85rem;
    margin-bottom: 0.25rem;
  }
  figcaption span {
    color: #8a93a8;
    font-size: 0.78rem;
    margin-left: 0.3rem;
  }
  @media (max-width: 700px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>

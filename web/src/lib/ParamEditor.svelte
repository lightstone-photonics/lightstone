<script>
  import { get } from './params.js';
  /**
   * Sliders for a parameter spec list. Values are read from `target` (SI) and reported via
   * `onchange(path, siValue, final)`; `final` is true on release (triggers TDECQ).
   */
  let { specs, target, idPrefix, onchange } = $props();

  const display = (s) => {
    const v = get(target, s.path);
    return v === undefined || v === null ? s.min : v / (s.scale ?? 1);
  };
  function emit(s, ev, final) {
    const v = Number(ev.currentTarget.value) * (s.scale ?? 1);
    onchange?.(s.path, s.digits === 0 && !s.scale ? Math.round(v) : v, final);
  }
</script>

{#each specs as s (s.path)}
  {@const id = `${idPrefix}-${s.path.replaceAll('.', '-')}`}
  {@const v = display(s)}
  <div class="slider">
    <label for={id}>{s.label}</label>
    <input
      {id}
      type="range"
      min={s.min}
      max={s.max}
      step={s.step}
      value={v}
      oninput={(e) => emit(s, e, false)}
      onchange={(e) => emit(s, e, true)}
      aria-valuetext={`${Number(v).toFixed(s.digits)} ${s.unit}`}
    />
    <output for={id}>{Number(v).toFixed(s.digits)} {s.unit}</output>
  </div>
{/each}

<style>
  .slider {
    display: grid;
    grid-template-columns: 9.5rem 1fr 6.5rem;
    align-items: center;
    gap: 0.6rem;
    margin: 0.35rem 0;
  }
  label {
    font-size: 0.9rem;
  }
  output {
    font-variant-numeric: tabular-nums;
    font-size: 0.9rem;
    text-align: right;
  }
  input {
    width: 100%;
    accent-color: #f6a03a;
  }
</style>

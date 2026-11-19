<script>
  /**
   * Schematic symbol of a component kind (style of the system view), 44 × 28 units.
   * `fiberType` colours a multimode fiber like its cable jacket (OM3 aqua, OM4 violet,
   * OM5 lime green); without it (palette, custom fiber) the optical orange is used.
   */
  let { kind, fiberType = null } = $props();
  const JACKET = { om3: '#3fd0d4', om4: '#b07cd8', om5: '#9bd94a' };
  const coil = $derived(JACKET[fiberType] ?? null);
</script>

<svg viewBox="0 0 44 28" class="sym" aria-hidden="true">
  {#if kind === 'prbs13q'}
    <rect x="3" y="5" width="38" height="18" rx="2" class="b" />
    <path d="M7,18 h5 v-4 h5 v8 h5 v-12 h5 v6 h5 v-3 h5" class="l" />
  {:else if kind === 'driver'}
    <path d="M10,4 L34,14 L10,24 Z" class="b" />
    <line x1="2" y1="14" x2="10" y2="14" class="l" /><line x1="34" y1="14" x2="42" y2="14" class="l" />
  {:else if kind === 'vcsel'}
    <rect x="3" y="3" width="26" height="22" rx="2" class="b" />
    <path d="M9,9 h14 l-7,10 z M9,19 h14" class="l" />
    <path d="M28,10 l10,-6 M28,16 l10,-6" class="o" /><path d="M36,3 l3,0.5 l-1.5,2.5 M36,9 l3,0.5 l-1.5,2.5" class="o" />
  {:else if kind === 'connector'}
    <line x1="1" y1="14" x2="12" y2="14" class="o" /><line x1="32" y1="14" x2="43" y2="14" class="o" />
    <rect x="12" y="7" width="9" height="14" class="b" /><rect x="23" y="7" width="9" height="14" class="b" />
  {:else if kind === 'mmf' || kind === 'custom_fiber'}
    {#each [8, 15, 22, 29, 36] as x}<ellipse cx={x} cy="14" rx="5" ry="10" class="coil" style={coil ? `stroke: ${coil}` : ''} />{/each}
  {:else if kind === 'attenuator'}
    <rect x="6" y="4" width="32" height="20" rx="2" class="b" />
    <path d="M12,20 L32,8" class="l" /><path d="M28,7.5 l4,0.5 l-1.5,3.5" class="l" />
  {:else if kind === 'splitter'}
    <path d="M2,14 H16 C24,14 26,6 42,6 M16,14 C24,14 26,22 42,22" class="o" />
    <rect x="12" y="9" width="12" height="10" rx="2" class="b" />
  {:else if kind === 'combiner'}
    <path d="M42,14 H28 C20,14 18,6 2,6 M28,14 C20,14 18,22 2,22" class="o" />
    <rect x="20" y="9" width="12" height="10" rx="2" class="b" />
  {:else if kind === 'bandpass_filter'}
    <line x1="1" y1="14" x2="6" y2="14" class="o" /><line x1="38" y1="14" x2="43" y2="14" class="o" />
    <rect x="6" y="3" width="32" height="22" rx="2" class="b" />
    <path d="M9,21 H15 C17,21 17,7 20,7 H24 C27,7 27,21 29,21 H35" class="o thin" />
  {:else if kind === 'wdm_mux'}
    {#each [['#c9a0ff', 5], ['#6cb6ff', 10.7], ['#9bd94a', 16.3], ['#f08a5d', 22]] as [c, y]}<line x1="1" y1={y} x2="10" y2={y} style="stroke: {c}" class="w" />{/each}
    <path d="M10,2 L32,9 V19 L10,26 Z" class="b" /><line x1="32" y1="14" x2="43" y2="14" class="o" />
  {:else if kind === 'wdm_demux'}
    {#each [['#c9a0ff', 5], ['#6cb6ff', 10.7], ['#9bd94a', 16.3], ['#f08a5d', 22]] as [c, y]}<line x1="34" y1={y} x2="43" y2={y} style="stroke: {c}" class="w" />{/each}
    <path d="M34,2 L12,9 V19 L34,26 Z" class="b" /><line x1="1" y1="14" x2="12" y2="14" class="o" />
  {:else if kind === 'optical_spectrum_analyzer'}
    <rect x="3" y="3" width="38" height="22" rx="2" class="b" />
    <path d="M6,21 H9 L10.5,8 L12,21 H17 L18.5,10 L20,21 H25 L26.5,9 L28,21 H33 L34.5,11 L36,21 H38" class="o thin" />
  {:else if kind === 'electrical_spectrum_analyzer'}
    <rect x="3" y="3" width="38" height="22" rx="2" class="b" />
    <path d="M6,8 C14,8 18,10 22,14 C26,18 30,20 38,20" class="e" /><path d="M6,17 H22 C27,17 30,21 38,22" class="l thin" />
  {:else if kind === 'probe'}
    <line x1="2" y1="10" x2="42" y2="10" class="o" />
    <path d="M22,10 V22 H34" class="o thin" /><rect x="17" y="6" width="10" height="8" rx="1" class="b" />
  {:else if kind === 'pin'}
    <path d="M16,8 h14 l-7,10 z M16,18 h14" class="l" />
    <path d="M4,4 l9,6 M4,10 l9,6" class="o" />
    <line x1="23" y1="18" x2="23" y2="25" class="l" /><line x1="23" y1="3" x2="23" y2="8" class="l" />
  {:else if kind === 'tia'}
    <path d="M10,9 L30,17 L10,25 Z" class="b" />
    <path d="M12,13 V4 H18 l1.5,-2 l3,4 l3,-4 l3,4 l1.5,-2 H34 V17" class="l thin" />
    <line x1="2" y1="17" x2="10" y2="17" class="l" /><line x1="30" y1="17" x2="42" y2="17" class="l" />
  {:else if kind === 'bessel_thomson'}
    <rect x="4" y="3" width="36" height="22" rx="2" class="b" />
    <path d="M8,10 H20 Q28,10 33,21" class="l" />
  {:else if kind === 'ffe'}
    {#each [4, 17, 30] as x}<rect x={x} y="3" width="10" height="8" rx="1" class="b" />{/each}
    {#each [9, 22, 35] as x}<line x1={x} y1="11" x2={x} y2="19" class="l thin" />{/each}
    <path d="M6,19 H38" class="l" /><text x="22" y="27" class="t">Σ c·x</text>
  {:else if kind === 'receiver_output'}
    <rect x="4" y="3" width="36" height="22" rx="2" class="b" />
    <path d="M9,21 h7 v-5 h7 v-5 h7 v-5 h6" class="l" />
  {:else if kind === 'pam4_analyzer'}
    <rect x="3" y="3" width="38" height="22" rx="2" class="b" />
    {#each [8, 12.5, 17, 21.5] as y}<line x1="7" y1={y} x2="37" y2={y} class="l thin" />{/each}
    <path d="M7,8 C14,8 16,21.5 22,21.5 S30,8 37,8" class="o thin" />
  {:else if kind === 'eye_diagram'}
    <rect x="3" y="3" width="38" height="22" rx="2" class="b" />
    <path d="M6,14 C14,4 30,4 38,14 C30,24 14,24 6,14 Z" class="e" />
    <path d="M6,14 C14,24 30,24 38,14 M6,14 C14,4 30,4 38,14" class="e" />
  {:else if kind === 'tdecq'}
    <rect x="3" y="3" width="38" height="22" rx="2" class="b" />
    <path d="M6,14 C12,6 20,6 25,14 C20,22 12,22 6,14 Z" class="e" />
    <text x="33" y="17.5" class="t">dB</text>
  {:else if kind === 'power_meter'}
    <circle cx="22" cy="16" r="10" class="b" />
    <path d="M22,16 L29,9" class="o" /><path d="M14,16 A8,8 0 0 1 30,16" class="l thin" />
  {:else if kind === 'rust_example'}
    <rect x="4" y="3" width="36" height="22" rx="2" class="b" />
    <text x="22" y="17.5" class="t">g·e⁻ʲωτ</text>
  {:else}
    <rect x="6" y="5" width="32" height="18" rx="2" class="b" /><text x="22" y="18" class="t">?</text>
  {/if}
</svg>

<style>
  .sym { width: 44px; height: 28px; flex: none; display: block; }
  .b { fill: #161b28; stroke: #c9d1e0; stroke-width: 1.2; }
  .l { fill: none; stroke: #c9d1e0; stroke-width: 1.2; }
  .o { fill: none; stroke: #f08a5d; stroke-width: 1.6; }
  .thin { stroke-width: 0.9; }
  .coil { fill: none; stroke: #f08a5d; stroke-width: 1.4; }
  .e { fill: none; stroke: #6cb6ff; stroke-width: 1.1; }
  .w { stroke-width: 1.3; fill: none; }
  .t { fill: #c9d1e0; font-size: 7px; text-anchor: middle; font-family: system-ui, sans-serif; }
</style>

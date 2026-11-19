---
component: mmf
title: Multimode fiber (OM3, OM4, OM5)
validation: [V8, V18]
references: [tia-492aaad, tia-568-3-d, king-mmf-effective-bandwidth-2018, ieee-802.3db-fiber-emulation]
---

## What it models

A standard multimode fiber span (node kind `mmf`, *Multimode* in the editor) of type OM3, OM4 or OM5. It does to its input exactly what a custom fiber does (`custom-fiber.md`: launch, mode delays, chromatic dispersion, attenuation), with the physics fixed by the preset; only the length, the attenuation (≤ 3.5 dB/km, the cabled limit) and an extra loss can be set. An `om4` node therefore always means the calibrated OM4. The reference link uses the OM4 preset.

Changing the physics of an `mmf` node (in the editor or with `Graph.set`) turns it into a `custom_fiber` with the preset's values; the editor offers *Back to OMx*. `lightstone.fiber_presets()` returns the presets as custom-fiber parameters.

**Presets** (minimum spec, preset version 1, `config::MmfType`). Each preset is the worst fiber that still carries its name: its effective modal bandwidth at 850 nm with the default VCSEL launch sits at the type's minimum. That is the bandwidth IEEE link budgets assume for laser-launched links, and what V14b validates. All presets: core radius 25 µm, NA 0.200, germania-doped core, 3.5 dB/km (cabled limit). In the table, α is the exponent of the power-law index profile (`custom-fiber.md`), EMB is the effective modal bandwidth and OFL the overfilled-launch bandwidth, both as bandwidth × length in MHz·km.

| Type | α | EMB @ 850 nm (minimum) | Side of the optimum | EMB @ 953 nm | OFL @ 850 nm |
|---|---|---|---|---|---|
| OM3 | 2.1231 | 2002 MHz·km (2000) | over-compensated | 1105 MHz·km | 781 MHz·km |
| OM4 | 2.0777 | 4710 MHz·km (4700) | over-compensated | 1875 MHz·km | 1454 MHz·km |
| OM5 | 2.0109 | 4704 MHz·km (4700) | under-compensated | > 10 000 MHz·km (minimum 2470) | 2319 MHz·km |

- **Why the EMB and not the OFL bandwidth.** In this model a fiber cannot sit at both minimums: with the default VCSEL launch (a centred beam that mainly lights the low modes) the EMB is much higher than the OFL bandwidth. The standard's minimum EMB is defined over worst-case laser launches (DMD masks), so a fiber at the minimum gives a more benign launch a higher bandwidth; calibrating the EMB with the default launch compensates for that. The OFL bandwidth (an overfilled, LED-like launch) is therefore below its nominal minimum (OM3 1500, OM4/OM5 3500 MHz·km); it does not affect laser-launched links.
- **Why the sides.** OM3 and OM4 lie above the optimum α at 850 nm (≈ 2.035), so their bandwidth falls at longer wavelengths, as in published OM4 data (king_3cm_adhoc_01_062818: 2531 MHz·km at 850 nm, 1704 at 916 nm, chromatic dispersion included). OM5 lies below the optimum, which moves to smaller α at longer wavelengths, so it keeps its bandwidth up to 953 nm: that is what distinguishes OM5 from OM4.
- **Not exactly at the minimum.** α is rounded to 10⁻⁴ towards the optimum, so the EMB is 0.1–0.3 % above the minimum.
- **Symbol colors.** In the editor an `mmf` node draws the fiber in the type's real jacket color: OM3 aqua, OM4 erika violet, OM5 lime green (TIA-598 and common practice; OM4 is also sold in aqua, violet is the usual European marking). A `custom_fiber` keeps the neutral symbol.

**Calibration.** The 4700 MHz·km is not computed: it is the OM4 minimum EMB of TIA-492AAAD (OM3: 2000). The only free parameter of a preset is the profile exponent α. It was searched so that the EMB computation (`custom-fiber.md`, steps 1–5) gives the minimum at 850 nm, and rounded to 10⁻⁴ towards the optimum: OM4 α = 2.0777 gives 4710 MHz·km (0.2 % above the minimum). The `mmf_presets` test checks all three presets.

## Assumptions & limits

- **Optimistic away from 850 nm (V18).** Against the IEEE 802.3cm table (TIA worst-case EMB with chromatic dispersion), the presets match at 850 nm but are optimistic at longer wavelengths: effective bandwidth OM4 +24 %, OM3 +9 % at 918 nm. A preset is one ideal power-law fiber at the 850 nm minimum; the worst case is an envelope over all fibers that pass at 850 nm and falls faster with wavelength. OM5 at 953 nm likewise far exceeds its minimum (2470 MHz·km). WDM results at 880–940 nm are therefore optimistic. Planned fix: measured/worst-case DMD fiber import.
- **The preset α values are calibrated, not measured.** Each type is a one-parameter family (α); real fibers of a type vary in profile shape and DMD. The calibration depends on the default VCSEL launch; another launch (offset, more mode groups) gives a different EMB. Importing measured DMD data is planned.
- A "typical" variant per type (real cables are often 5000–8000 MHz·km EMB for OM4) is a later option; use a custom fiber meanwhile.
- The preset fixes the physics only; all modeling limits of the fiber model apply (`custom-fiber.md`).

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `type` | — | — | `om4` | `om3`, `om4`, `om5` |
| `length` | $L$ | m | 100 | 0 … 500 |
| `attenuation_db_per_km` | $\alpha_{850}$ | dB/km | 3.5 (at 850 nm) | 0 … 3.5 |
| `extra_loss_db` | $\text{loss}_\text{dB}$ | dB | 0 | 0 … 3 |

Fixed by the preset: core radius 25 µm, NA 0.200, graded profile with the α of the table, germania-doped core, attenuation spectrum `rayleigh` (`eq:fiber-attenuation-spectrum`), solver grid 600 points per radius, domain 2a.

## Validation

- **V8**: the OM4 preset (the default fiber) has an EMB with the default launch of 4700 … 4747 MHz·km.
- Presets: an `mmf` node gives bit-identical results to a `custom_fiber` with the preset's values; each preset's EMB is at its minimum (within 2 %); physics parameters on an `mmf` node are rejected; the reference lane uses the OM4 preset, a lane with other physics a custom fiber.
- **V18** (`tests/python/test_v18_wavelength_bandwidth.py`): effective bandwidth vs wavelength against the IEEE 802.3cm table (king_3cm_adhoc_01_062818). Same dispersion term on both sides, so only the modal part is compared:

  | λ (nm) | OM3 table | OM3 Lightstone | OM4 table | OM4 Lightstone |
  |---|---|---|---|---|
  | 844 | 1591 | 1699 (+6.8 %) | 2398 | 2537 (+5.8 %) |
  | 850 | 1665 | 1667 (+0.1 %) | 2531 | 2535 (+0.2 %) |
  | 874 | 1460 | 1528 (+4.6 %) | 2246 | 2454 (+9.3 %) |
  | 900 | 1267 | 1370 (+8.1 %) | 1887 | 2260 (+19.8 %) |
  | 918 | 1159 | 1265 (+9.2 %) | 1683 | 2087 (+24.0 %) |

  The test checks the agreement at 850 nm (1 %), the fall with wavelength, and pins the gap (it is a known limitation, not a pass).

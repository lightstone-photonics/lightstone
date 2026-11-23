---
component: wdm-mux
title: WDM multiplexer
validation: [V17a, V17b]
references: [swdm-ieee-ngmmf-2018]
---

## What it models

A WDM multiplexer (node kind `wdm_mux`): N inputs (`in0` … `inN−1`), one per wavelength channel, combined onto one output, for example four VCSELs at 850, 880, 910 and 940 nm onto one OM5 fiber (SWDM4, 30 nm spacing). Each input passes its channel's passband before the light is combined.

## Equations

**Per input port** $i$: every source contribution at `in`$i$ is weighted by the passband $T_i(\lambda_0)$ of channel $i$ at the source's centre wavelength $\lambda_0$, with $T_i$ the passband of `eq:wdm-passband` (`bandpass-filter.md`) centred at the channel wavelength $\lambda_i$ (`channels`) and sharing the width, order, insertion loss and isolation of the mux.

**Combining**: the weighted contributions of all inputs are kept separate on the output, like the `combiner` (`combiner.md`): different lasers are mutually incoherent, so they add as powers at the detector.

$$P_\text{out}(t) = \sum_i \sum_{c \in \text{in}_i} T_i(\lambda_c)\thinspace P_c(t)$$

Here $P_\text{out}(t)$ is the output optical power (W), $i$ runs over the input ports, $c$ over the source contributions at input $i$, $\lambda_c$ is the centre wavelength of source $c$ and $P_c(t)$ its power at the input (W).

A source outside its port's passband (more than 3 dB below the peak) gives a warning; its light is attenuated as computed.

## Assumptions & limits

- Centre-wavelength weighting, no filter dispersion, no polarization dependence and no reflections, as for the bandpass filter (`bandpass-filter.md`).
- The default values are typical of SWDM4 thin-film mux parts, not from a standard.

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `channels` | $\lambda_i$ | m | 850, 880, 910, 940 n | one port each |
| `width` | $w$ | m | 22 n | > 0 |
| `order` | $n$ | — | 3 | 1 … 10 |
| `insertion_loss_db` | IL | dB | 1.0 | ≥ 0 |
| `isolation_db` | ISO | dB | 30 | > 0, or `null` (no floor) |

## Validation

- **V17a** (`graph_tests.rs`): with ideal filters (IL 0, no floor), each channel of a 4-VCSEL SWDM4 link (mux, 100 m OM5, demux) equals the single-wavelength link of that VCSEL to 10⁻⁹ (noiseless waveform, σ(t) and noisy realization).
- **V17b**: with 15 dB isolation, the waveform at a demux port is the weighted sum of the single-wavelength waveforms, with the mux weights $T_j(\lambda_j)$ (to 10⁻⁹; `wdm-demux.md`).
- A source outside a mux port's passband warns.

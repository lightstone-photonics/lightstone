---
component: optical-spectrum-analyzer
title: Optical spectrum analyzer
validation: [S2]
references: []
---

## What it models

The optical spectrum analyzer (OSA, node kind `optical_spectrum_analyzer`) shows the optical power at its input versus wavelength, in dBm per resolution bandwidth, with the peak, the total power and one row per source. It observes only (monitor input, like the power meter). In the editor it shows an instrument readout and a small trace on the node.

## Equations

**Model spectrum** (`eq:osa-model-spectrum`). Every source contribution at the input is a Gaussian line at its centre wavelength $\lambda_c$ with RMS width $\sigma_c$ and its average power $P_c$ at this point. The trace is the power within the resolution bandwidth (RBW) around each wavelength, as a scanning OSA shows it:

$$P_\text{RBW}(\lambda) = \sum_c P_c\left[\Phi\mskip-3mu \left(\frac{\lambda + \mathrm{RBW}/2 - \lambda_c}{\sigma_c}\right) - \Phi\mskip-3mu \left(\frac{\lambda - \mathrm{RBW}/2 - \lambda_c}{\sigma_c}\right)\right]$$

Here $\lambda$ is the wavelength of the trace point in m, $\mathrm{RBW}$ the resolution bandwidth in m (parameter `rbw`), and the sum runs over the source contributions $c$; $\lambda_c$ and $\sigma_c$ are in m and $P_c$ in W. $\Phi$ is the standard normal cumulative distribution function, so the bracket is the fraction of line $c$ that falls inside the window $\lambda \pm \mathrm{RBW}/2$. $P_\text{RBW}(\lambda)$ is in W and is displayed in dBm. The total power is $\sum_c P_c$.

This is a *model* spectrum: it shows what the simulation knows about each source, not a measured VCSEL spectrum with its transverse-mode lines.

## Assumptions & limits

- No VCSEL mode structure (real multimode VCSELs show several lines 0.1–0.2 nm apart) and no modulation sidebands (±R_s/2 ≈ ±0.06 nm at 53 GBd, with R_s the symbol rate, small against the 0.4 nm RMS width of the VCSEL line).
- A narrow filter before it does not reshape the lines (`bandpass-filter.md`).
- No noise floor or detector of its own; the display floor is −270 dBm.

## Parameters

| Name | Unit | Default | Meaning |
|---|---|---|---|
| `start`, `stop` | m | 820 n, 970 n | wavelength span |
| `points` | — | 751 | trace points |
| `rbw` | m | 0.1 n | resolution bandwidth |

## Validation

- **S2** (`tests/spectrum_analyzers.rs`): on the fiber output of the reference link, the OSA reports the fiber's output power as the total (to 10⁻¹²), the peak at 850 nm, the peak power per RBW of the Gaussian line exactly, and its trace sums to the total power (to 10⁻³).

---
component: bandpass-filter
title: Optical bandpass filter
validation: []
references: [swdm-ieee-ngmmf-2018]
---

## What it models

An optical bandpass filter (node kind `bandpass_filter`): it passes the light at its input within one wavelength band and suppresses the rest, like a thin-film filter. Each source contribution at the input is weighted by the filter's transmission at that source's centre wavelength. The same passband is used by every port of the WDM multiplexer and demultiplexer (`wdm-mux.md`, `wdm-demux.md`).

## Equations

**Passband** (`eq:wdm-passband`), centre $\lambda_c$, −3 dB full width $w$, order $n$, insertion loss IL, isolation ISO:

$$T(\lambda) = 10^{-\mathrm{IL}/10}\thinspace\max\negthinspace\Big(\exp\negthinspace\big[-\ln 2\thinspace\big|2(\lambda - \lambda_c)/w\big|^{2n}\big],\ 10^{-\mathrm{ISO}/10}\Big)$$

Here $T(\lambda)$ is the power transmission (linear, 0 to 1) at the wavelength $\lambda$ in m, $\lambda_c$ the centre wavelength in m, $w$ the −3 dB full width in m, $n$ the dimensionless order, and IL and ISO the insertion loss and isolation in dB. The first factor sets the loss at the peak; the $\max$ keeps the transmission from falling below the isolation floor.

- $n = 1$ is a Gaussian; larger $n$ gives a flatter top and steeper edges, like a thin-film filter.
- The isolation is a floor relative to the passband peak. Without it (`isolation_db: null`) the shape alone sets the leakage, which 30 nm away from the centre is below 10⁻⁸⁰ for the default shape.

**Applied per source contribution.** Each contribution is multiplied by $T(\lambda_0)$ at its source's centre wavelength $\lambda_0$ (`vcsel.md`). The filter therefore acts like a loss on that contribution: all its mode bins, and its RIN and jitter terms, which follow the contribution. It is linear, so σ(t) and the semi-analytic SER stay exact.

## Assumptions & limits

- **Centre-wavelength weighting.** The filter does not reshape a source's spectrum. This is accurate while the transmission is flat across the source width (VCSEL RMS spectral width σ_λ ≈ 0.4 nm against a 22 nm passband). The node warns when the transmission changes by more than 1 dB within ±2σ_λ of a source. A spectrally resolved source model is a later option.
- **No dispersion of the filter.** Thin-film filters add group-delay ripple near their edges; not modeled.
- **No polarization dependence and no reflections.** The reflected port of a thin-film filter (back towards the transmitter) is not modeled (signal-spec 11.1).
- The default values are typical of SWDM4 thin-film filters, not from a standard.

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `center` | $\lambda_c$ | m | 850 n | > 0 |
| `width` | $w$ | m | 22 n | > 0 |
| `order` | $n$ | — | 3 | 1 … 10 |
| `insertion_loss_db` | IL | dB | 0.5 | ≥ 0 |
| `isolation_db` | ISO | dB | 30 | > 0, or `null` (no floor) |

## Validation

- Passband unit test (`wdm.rs`): peak = −IL, −3 dB at $\lambda_c \pm w/2$, symmetric, the floor at −(IL + ISO), order 1 a Gaussian.
- A source outside the band is suppressed to exactly the isolation floor; an ideal filter (IL 0, no floor) passes a source in the band unchanged; a filter narrower than the source warns (`graph_tests.rs`).

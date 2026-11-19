---
component: electrical-spectrum-analyzer
title: Electrical spectrum analyzer
validation: [S1]
references: []
---

## What it models

The electrical spectrum analyzer (ESA, node kind `electrical_spectrum_analyzer`) shows the power spectral density of the electrical signal at its input versus frequency, separately for the signal and for the noise, with the SNR and the signal and noise RMS. It observes only (monitor input). In the editor it shows an instrument readout and a small trace on the node.

## Equations

**Power spectral densities** (`eq:esa-psd`), one-sided, in unit²/Hz (V²/Hz after the TIA):

- Signal: the periodogram of the noiseless periodic waveform, $S_x(f_k) = 2|X_k|^2/(N^2\Delta f)$ for $k > 0$; the DC value is reported separately. Here $X_k$ is the $k$-th DFT coefficient of the noiseless waveform, $N$ the number of samples, $\Delta f$ the DFT bin spacing in Hz (sample rate / $N$), $k = 0 \dots N/2$ the bin index and $f_k = k\,\Delta f$ its frequency. The factor 2 folds negative frequencies onto positive ones (one-sided PSD); the Nyquist bin $k = N/2$ has no negative partner and is counted once.
- Noise: from the noise terms, time-averaged, $S_n(f) = \sum g^2\,\bar S\,|H(f)|^2$. The sum runs over the noise terms; for each, $\bar S$ is its time-averaged one-sided PSD at the point of generation in unit²/Hz, $g$ the gain and $H(f)$ the transfer function it has passed since then. A noise realization after a nonlinear block enters through its periodogram.
- Both are averaged over display bins of width $\mathrm{RBW} = f_\text{stop}/\text{points}$. Here the RBW is in Hz, $f_\text{stop}$ is the stop frequency in Hz (`stop_rel` × symbol rate; the start is 0) and points the number of display bins (`points`). The PRBS signal is a line spectrum with lines $R_s/8191$ apart (6.5 MHz), where $R_s$ is the symbol rate in Bd and 8191 the pattern period in symbols; averaging over the RBW gives its envelope.
- $\int S_n\,df = \overline{\sigma^2(t)}$ and $\int S_x\,df$ = the variance of the noiseless waveform. Here σ(t) is the RMS noise versus time and the overline denotes the time average. SNR = their ratio.

## Assumptions & limits

- Densities, not dBm into 50 Ω; the analyzer shows the unit of the wire.
- The noise PSD is time-averaged: RIN and shot noise vary along the waveform (σ(t)), which a spectrum does not show.
- No noise floor or detector of its own.

## Parameters

| Name | Unit | Default | Meaning |
|---|---|---|---|
| `stop_rel` | × baud | 1.5 | stop frequency relative to the symbol rate |
| `points` | — | 400 | display bins |

## Validation

- **S1** (`tests/spectrum_analyzers.rs`): on the reference link after the receive filter, the signal PSD integrates to the variance of the noiseless waveform and the noise PSD to the mean of σ²(t), both to 10⁻⁹ (Parseval); the DC matches the mean. With thermal noise only, the noise PSD at the receive filter's −3 dB frequency is 3.010 dB below its low-frequency value.

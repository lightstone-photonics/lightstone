---
component: pin
title: PIN photodiode
validation: [V10, V15, V17a]
references: [ieee-802.3, agrawal-focs]
---

## What it models

The PIN photodiode (node kind `pin`) converts the optical power arriving at its input into a photocurrent. It sums the power of all modes and all sources (square-law detection in the incoherent limit), adds shot noise, and turns the transmitter's noise carried by the optical signal (RIN, transmitter jitter) into current noise. Its output is an electrical current signal.

This is also where the optical description becomes an electrical one: for each source contribution the photodiode builds the channel's transfer function from the mode bins (`custom-fiber.md`, `eq:fiber-power-propagation`) and applies it to the source's power spectrum.

## Equations

**Photocurrent** (`eq:rx-photocurrent`): $I(t) = R\,P_\text{det}(t)$. Here $I(t)$ is the photocurrent in A, $R$ the responsivity in A/W and $P_\text{det}(t)$ the optical power that reaches the detector, in W. Because the fiber model is incoherent, $P_\text{det}$ is the sum of the mode powers; there are no intermodal interference terms. With several sources (WDM), $I = \sum_c R(\lambda_c)\,P_{\text{det},c}$, where $c$ runs over the sources, $\lambda_c$ is the wavelength of source $c$ and $P_{\text{det},c}$ its detected power.

**Responsivity spectrum** (`eq:rx-responsivity`, since v0.16; `responsivity_spectrum: "constant_qe"`, default): `responsivity` is the value at 850 nm, and the quantum efficiency η is constant, $R = \eta q\lambda/(hc_0)$ (with $q$ the elementary charge, $h$ Planck's constant and $c_0$ the speed of light):

$$R(\lambda) = R_{850}\,\frac{\lambda}{850\ \text{nm}}$$

Here $\lambda$ is the source wavelength in m and $R_{850}$ the responsivity at 850 nm in A/W (`responsivity`). 0.5 A/W at 850 nm (η = 73 %) gives 0.518, 0.535 and 0.553 A/W at 880, 910 and 940 nm. `"flat"` keeps $R$ the same at every wavelength.

**Detector area** (optional, `detector_radius`): with a finite radius $r_d$, each mode contributes only the fraction $\eta_m$ of its power that falls inside $r_d$ (`eq:fiber-power-propagation`). By default all light is collected ($\eta_m = 1$, a lens-coupled receiver).

**Noise terms at the output** (`eq:rx-noise`), as white Gaussian current noise with one-sided PSDs:

$$S_\text{shot}(t) = 2q\,I(t),\qquad S_\text{RIN}(t) = R^2\,\mathrm{RIN}\cdot P_\text{tx}(t)^2 \ \text{(through the channel)}$$

Here $S_\text{shot}(t)$ is the shot noise PSD in A²/Hz, generated here with the receiver's seed, with $q$ the elementary charge and $I(t)$ the noiseless photocurrent. $S_\text{RIN}(t)$ is the current PSD of the transmitter's intensity noise: the VCSEL's RIN realization (`vcsel.md`, `eq:vcsel-rin`; RIN in 1/Hz, $P_\text{tx}(t)$ the transmitted power in W) passes the same channel transfer function as the signal and is scaled by the responsivity $R$ of its source. Transmitter jitter noise (`driver.md`) is carried the same way. With several sources there is one RIN (and jitter) term per source. Each term keeps its strength and filter, so the noise after later linear blocks stays exact (`bessel-thomson.md`).

**Coherence check**: with a finite detector, modes that carry significant power (≥ 1 % of the largest) and are partially coherent (degree of coherence γ > 0.1, from the source's spectral width) would interfere on the detector (modal noise). The photodiode warns that the power model may then be optimistic.

## Assumptions & limits

- Responsivity ∝ λ (constant quantum efficiency): right for a detector whose absorber covers 840–953 nm (InGaAs-based). A GaAs PIN cuts off above ≈ 870 nm; that is not modeled.
- No transit-time or RC bandwidth of the diode itself (the receive filter stands for the whole receiver bandwidth, `bessel-thomson.md`) and no dark current.
- `detector_radius` models a bare detector directly at the fiber end face.
- Modal noise (interference between partially coherent modes on a partial detector) is not modeled, only warned about.

## Parameters

| Name | Symbol | Unit | Default | Range |
|---|---|---|---|---|
| `responsivity` (lane: `rx.responsivity`) | $R_{850}$ | A/W | 0.5 (at 850 nm) | 0.4 … 0.65 |
| `responsivity_spectrum` | — | — | `constant_qe` | `constant_qe`, `flat` |
| `detector_radius` (lane: `rx.detector_radius`) | $r_d$ | m | none (all light) | 10 … 30 µ |
| `shot_noise` (lane: `rx.shot_noise`) | — | — | true | — |
| `noise_seed` | — | — | from the node | shot noise random stream |

## Validation

- **V10**: with a constant optical input, the output noise variance after the receive filter equals $(i_n^2 + 2qI)B_n$ to < 1 %, with $I$ the constant photocurrent ($i_n$ and $B_n$: `tia.md`, `bessel-thomson.md`).
- **V15**: the detected optical power and the noiseless receiver waveform agree with the NumPy reference.
- **V17a** (`wdm-demux.md`): the mean photocurrent equals $R(\lambda)\,P_\text{det}$ for each SWDM4 channel.

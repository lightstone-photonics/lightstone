# Tutorial: Signals and Math in Lightstone

This tutorial follows one signal through the reference link, from bits to the decision circuit, and explains the math Lightstone uses at each step, with real numbers from the default configuration. It is meant as an introduction; a node-by-node walkthrough of the reference graph (what each component does, what is implemented, how the signal is stored) is [`tutorial-reference-link.md`](tutorial-reference-link.md), and the exact definitions are in [`signal-spec.md`](signal-spec.md) and the per-component model docs in [`model/`](model/).

Default link used for all numbers below:

```
PRBS13Q → driver → VCSEL (850 nm) → connector → 100 m OM4 → PIN → TIA → Bessel–Thomson → 5-tap FFE → receiver output
53.125 GBd PAM-4, 0 dBm average power, ER 4 dB, 16 samples per symbol
```

---

## 1. The big picture

A link moves information through four kinds of signals:

| Stage | Signal | Example value |
|---|---|---|
| Pattern generator | **Data**: symbols 0, 1, 2, 3 | `2 0 3 1 1 0 …` |
| Driver | **Electrical**: drive waveform | four current levels |
| VCSEL → fiber → photodiode | **Optical**: power in watts, split over fiber modes | 0.57 … 1.43 mW |
| TIA → filter → receiver output | **Electrical**: voltage | ≈ 410 mV swing |

Each component takes a signal in, changes it, and passes it on. Most of Lightstone's speed comes from one observation: almost every component is **linear**, so the whole chain can be computed in one step (Section 9).

---

## 2. Time: one periodic grid

All signals share one time grid:

- symbol rate R_s = 53.125 GBd, so one symbol (one **unit interval**, UI) lasts T = 1/R_s ≈ 18.8 ps
- s = 16 samples per symbol, so the sample rate is f_s = R_s · s = 850 GS/s
- one PRBS13Q period has 8191 symbols, so N = 8191 × 16 = 131 056 samples

The pattern repeats, so Lightstone treats every waveform as **periodic**. This has a big advantage: a filter becomes a simple multiplication in the frequency domain, and the result is exact.

$$y(t) = (h * x)(t) \quad\Longleftrightarrow\quad \hat y(f) = H(f)\thinspace\hat x(f)$$

In code: FFT, multiply by H(f), inverse FFT. A component that filters is fully described by its **transfer function** H(f).

---

## 3. From bits to light levels

**Gray coding.** PRBS13Q takes bit pairs and maps them to symbols so that neighboring levels differ in only one bit:

| Bits | 00 | 01 | 11 | 10 |
|---|---|---|---|---|
| Symbol | 0 | 1 | 2 | 3 |

A decision error to the neighboring level therefore costs only one bit, which is why BER ≈ SER/2.

**Power levels.** The VCSEL modulates its **optical power**, not the light's field. Four equally spaced levels follow from the average power P̄ and the extinction ratio ER = P₃/P₀:

$$P_0 = \frac{2\bar P}{ER + 1},\qquad P_3 = \frac{2\bar P\thinspace ER}{ER + 1},\qquad P_k = P_0 + \frac{k}{3}(P_3 - P_0)$$

Example: P̄ = 1 mW (0 dBm), ER = 4 dB = 2.512:

| Level | P₀ | P₁ | P₂ | P₃ |
|---|---|---|---|---|
| Power | 0.570 mW | 0.857 mW | 1.144 mW | 1.431 mW |

The **optical modulation amplitude** is OMA_outer = P₃ − P₀ = 0.861 mW. The four levels average exactly 1 mW.

**The VCSEL is not infinitely fast.** Its intensity response is a second-order low-pass with resonance frequency f_r and damping ζ (Section 3.1 explains where that comes from):

$$H_V(f) = \frac{1}{1 - (f/f_r)^2 + j\thinspace2\zeta\thinspace f/f_r}$$

With f_r = 28 GHz and ζ = 0.7 the −3 dB bandwidth is ≈ 28 GHz, about half the symbol rate. The sharp steps of the drive signal become smooth transitions, which is the first source of **intersymbol interference** (ISI): each symbol leaks a little into its neighbors.

### 3.1 How a VCSEL works

A **VCSEL** (vertical-cavity surface-emitting laser) is a semiconductor laser that emits light straight up out of the chip surface, instead of from a cleaved edge like classic laser diodes.

```
            light out (850 nm)
                  ↑
   ┌──────────────────────────────┐
   │   top mirror (DBR)           │   ~20–25 layer pairs, R > 99 %
   ├────────┬───────────┬─────────┤
   │ oxide  │  aperture │  oxide  │   current and light confined to a 5–8 µm spot
   ├────────┴───────────┴─────────┤
   │   active region              │   a few GaAs quantum wells: electrons + holes → photons
   ├──────────────────────────────┤
   │   bottom mirror (DBR)        │   ~30–35 layer pairs, R > 99.9 %
   └──────────────────────────────┘
                substrate
```

- **Mirrors.** Each mirror is a *distributed Bragg reflector* (DBR): a stack of alternating AlGaAs layers, each a quarter wavelength thick. Every interface reflects a little; together they reflect more than 99 %.
- **Cavity.** The space between the mirrors is only about one wavelength long, so light passes the gain region hundreds of times before it leaves. That is why such highly reflective mirrors are needed.
- **Gain.** Current injects electrons and holes into the quantum wells. When they recombine they emit photons, and a photon traveling in the cavity stimulates more recombinations with identical photons. Above the **threshold current** (typically 0.5–1 mA) stimulated emission wins over losses and the device lases. Above threshold, optical power rises roughly linearly with current (the L–I curve). This is what makes direct modulation work: the driver changes the current, the power follows.
- **Oxide aperture.** An oxidized ring in one layer confines current and light to a spot of 5–8 µm. Such a spot supports several **transverse modes**, i.e. several intensity patterns across the aperture. Each has a slightly different wavelength, so a multimode VCSEL emits a set of spectral lines about 0.1–0.2 nm apart. Together they give the RMS spectral width σ_λ ≈ 0.3–0.6 nm.

**Why the response is second order.** Electrons and photons in the cavity exchange energy: more electrons → more gain → more photons → the photons use up electrons → less gain → fewer photons. After a current step this exchange rings at the **relaxation-oscillation frequency** f_r and decays with the damping ζ. That is exactly a second-order system, H_V(f) above. f_r grows roughly with √(I − I_th), so a VCSEL is faster at higher bias current, until heating limits it.

**Where RIN comes from.** A small fraction of photons is emitted spontaneously, with random timing and phase. They add random fluctuations to the power: **relative intensity noise**, quoted in dB/Hz relative to the average power.

**What Lightstone models, and what not:**

| Effect | In Lightstone |
|---|---|
| Linear L–I above threshold (power levels from P̄ and ER) | Yes |
| Relaxation oscillation (second-order response) | Yes, f_r and ζ as parameters |
| RIN | Yes, white RIN |
| Transverse modes (near field at the facet) | Yes, as Laguerre–Gauss mode groups (`source.vcsel_mode_groups`) |
| Spectral width | Yes, as one RMS value σ_λ |
| Bias dependence of f_r, thermal rollover, L–I curvature | No (small-signal model) |
| Frequency chirp | No |
| Mode partition noise (power hopping between transverse modes) | No |

---

## 4. Light in a multimode fiber: modes

### 4.1 What a mode is

A fiber guides light in a discrete set of **modes**. Each mode is a field pattern across the core that keeps its shape while it travels, and each travels at its own speed. Lightstone uses the scalar LP modes LP_lm: l counts the angular nodes, m the radial ones.

How many modes a fiber guides is set by the **V-number**:

$$V = \frac{2\pi}{\lambda}\thinspace a\thinspace NA = \frac{2\pi}{850\ \text{nm}} \cdot 25\ \mu\text{m} \cdot 0.20 \approx 37$$

A graded-index fiber guides about V²/4 ≈ 340 modes counting both polarizations, i.e. ≈ 171 spatial modes. These are grouped into 90 LP labels (most LP modes exist in two orientations, cos lφ and sin lφ). Lightstone's solver finds exactly 90 LP modes and 171 spatial modes.

### 4.2 Why OM4 has a curved index profile

In a fiber with a flat (step) index, the modes would arrive with very different delays: over 100 m about 4–5 ns, more than 200 symbols. The eye would be completely closed.

OM4 uses a **graded index**. The refractive index is highest in the center and falls off almost like a parabola:

$$n^2(r) = n_1^2\left(1 - 2\Delta\thinspace(r/a)^\alpha\right),\qquad \alpha \approx 2$$

High-order modes travel a longer zig-zag path, but through glass with lower index, where light is faster. With α close to its optimum both effects almost cancel. The remaining delay differences are the **differential mode delay (DMD)**; for OM4 they are a few ps per 100 m.

### 4.3 Why Lightstone only tracks power per mode

The field in the fiber is a sum of modes. In principle the detector sees interference between them. Lightstone ignores these interference terms, for two independent reasons:

1. **Orthogonality.** Modes are orthogonal over the fiber cross-section. A detector that collects all the light integrates over the whole cross-section, so every cross term integrates to zero:

$$\iint \psi_k\thinspace\psi_l^*\thinspace dA = \delta_{kl}$$

2. **Incoherence.** A VCSEL is not a perfectly coherent light source, so even without orthogonality, light arriving at different times hardly interferes. Section 4.4 explains this with the **degree of coherence** γ. For modes arriving several ps apart, γ is practically zero.

So the detected power is simply the sum of the mode powers:

$$P_\text{det}(t) = \sum_k P_k(t)$$

That is the **`Power` level** of an optical signal: one power value per mode (or mode group), no phase. Section 10 explains how the format still keeps room for phase.

### 4.4 Degree of coherence

**Intuition.** Interference needs two waves with a stable phase relation. Split a light beam, delay one copy by Δτ, recombine: if the phase of the light changes little within Δτ, the copies interfere and form bright and dark fringes. If the phase has wandered randomly in the meantime, the fringes move faster than any detector can follow and average out, and the powers simply add.

The **degree of coherence** γ(Δτ) measures this, between 1 (perfect interference) and 0 (powers simply add). The detected power of the two recombined copies, with phase difference Δφ, is

$$P = P_1 + P_2 + 2\sqrt{P_1 P_2}\mskip5mu \gamma(\Delta\tau)\cos\Delta\varphi$$

The last term is the interference term; γ scales it.

**Link to the spectrum.** How fast the phase wanders depends on how wide the spectrum is: a single, pure frequency has a phase that never wanders; a broad spectrum is a mix of many frequencies that drift apart quickly. Mathematically, γ(Δτ) is the normalized Fourier transform of the optical power spectrum (Wiener–Khinchin theorem). For a Gaussian spectrum with RMS width σ_ν:

$$\gamma(\Delta\tau) = e^{-2\pi^2\sigma_\nu^2\thinspace\Delta\tau^2}$$

The **coherence time** τ_c is the delay at which γ has dropped to 1/e, and the **coherence length** is how far light travels in that time:

$$\tau_c = \frac{1}{\sqrt2\thinspace\pi\thinspace\sigma_\nu},\qquad L_c = \frac{c}{n}\thinspace\tau_c$$

**Numbers.** The default VCSEL has σ_λ = 0.4 nm, i.e. σ_ν = c·σ_λ/λ² ≈ 166 GHz:

| Δτ | 1 ps | 2 ps | 4 ps |
|---|---|---|---|
| γ | 0.58 | 0.11 | 0.0002 |

τ_c ≈ 1.4 ps, so the coherence length in glass is only ≈ 0.3 mm.

For comparison, a narrow-linewidth DFB laser for coherent links (1 MHz linewidth) has a coherence time of hundreds of ns and a coherence length of tens of meters in fiber. For such a laser the incoherent `Power` model would be wrong, and the `Field` level (Section 10) would be needed.

**What it means for the fiber.** The modes of the default link at 100 m arrive with an RMS delay spread of ≈ 5.1 ps, about four coherence times. Modes in different groups are therefore effectively incoherent. Modes within one group can be closer than 1 ps and partially coherent, but with full-area detection their cross terms vanish anyway by orthogonality (Section 4.3, reason 1). The engine will use this rule as a validity check: if significantly excited modes have γ that is not negligible, and the detector does not collect all the light, it warns that the power model may be optimistic.

### 4.5 Mode groups and mode bins

In a parabolic fiber, modes with the same **principal mode number** g = 2m + l − 1 travel at almost the same speed. They form about 18 **mode groups**.

A **mode bin** is one slot in the mode dimension of an optical signal. It is the unit in which Lightstone stores and propagates power. Depending on the **mode resolution** setting, one mode bin is:

| Resolution | One mode bin = | Number of mode bins (OM4, 850 nm) |
|---|---|---|
| `ModeGroup` (default) | one principal mode group, all its modes together | ≈ 18 |
| `Mode` | one LP mode (both orientations, both polarizations) | 90 |

Each mode bin k carries the same set of quantities, whatever the resolution:

| Quantity | Meaning |
|---|---|
| w_k | power weight: launched power × losses so far |
| τ_k | accumulated relative group delay |
| D_k | accumulated chromatic delay slope (→ broadening σ_k) |
| polarization / field | only at the `Stokes` or `Field` level (Section 10) |

A mode bin is to the mode dimension what a sample is to the time dimension: a discretization. Like coarser time sampling, coarser mode binning saves memory and compute (≈ 5× here) and costs some accuracy. In a mode-group bin all modes share one power-weighted delay, which slightly underestimates the delay spread inside the group, so the eye looks a little better than it is. Lightstone will measure this difference on the reference link (test G2) rather than assume it.

Components never see the resolution directly. A fiber span adds delay to "every mode bin", a connector multiplies "every mode bin" by its loss, and the detector sums "all mode bins". That is why the resolution can be changed later without changing any component.

---

## 5. The launch: which modes get light

The VCSEL's light enters the fiber at the connector. How much power each mode receives is given by the **overlap integral** between the incoming field E_in and the mode field ψ_m:

$$c_m = \iint E_\text{in}(x, y)\thinspace\psi_m(x, y)\thinspace dx\thinspace dy,\qquad P_m = |c_m|^2$$

Intuition: a mode receives power if its field pattern "looks like" the incoming light.

- A small, centered beam overlaps mostly with low-order modes (central, smooth patterns).
- An **offset** beam overlaps with modes that have intensity off-center: power moves into higher mode groups.

Because the modes are orthonormal, ΣP_m ≤ 1. The rest of the light is not guided and is lost.

Why this matters: different modes have different delays, so the launch decides how much DMD the signal will see. A launch with a 10 µm offset spreads power over more mode groups and lowers the effective modal bandwidth. You can watch this in the browser by moving the connector offset slider.

---

## 6. Propagation: the fiber as a filter

Each mode bin k reaches the end of the fiber with three properties:

- **weight** w_k: launch power × fiber loss
- **delay** τ_k: its group delay relative to the average
- **chromatic broadening** σ_k: different wavelengths in the VCSEL spectrum travel at slightly different speeds

$$\sigma_k = |D_k|\thinspace L\thinspace\sigma_\lambda \approx 93\ \tfrac{\text{ps}}{\text{nm·km}} \cdot 0.1\ \text{km} \cdot 0.4\ \text{nm} \approx 3.7\ \text{ps}$$

D ≈ −93 ps/(nm·km) is the dispersion of the launched modes at 850 nm: the fused-silica cladding alone would give −84, the germania-doped core makes it larger (`docs/model/custom-fiber.md`).

The fiber turns into one transfer function acting on optical power:

$$H_\text{fiber}(f) = \sum_k w_k\mskip5mu \underbrace{e^{-j2\pi f\tau_k}}_{\text{delay}}\mskip5mu \underbrace{e^{-2\pi^2 f^2\sigma_k^2}}_{\text{chromatic broadening}}$$

### Worked example: two modes

Two equal modes (w = ½ each), arriving Δτ apart:

$$H(f) = \tfrac12\left(1 + e^{-j2\pi f\Delta\tau}\right),\qquad |H(f)| = |\cos(\pi f\Delta\tau)|$$

The optical −3 dB bandwidth is where |H| = ½, i.e. πfΔτ = π/3:

$$f_\text{3dB} = \frac{1}{3\thinspace\Delta\tau}$$

With Δτ = 10 ps this gives 33 GHz. More delay spread means less bandwidth. This is exactly validation test V7.

For the OM4 preset at 100 m, the RMS delay spread of the default launch is ≈ 5.1 ps and the fiber's channel bandwidth (modal + chromatic) is ≈ 33 GHz. At 150 m it drops to ≈ 22 GHz.

**Modal bandwidth in MHz·km.** Fiber datasheets quote bandwidth × length. OM4 must reach at least 4700 MHz·km (effective modal bandwidth with a laser launch) and 3500 MHz·km (all modes equally filled, "overfilled launch"). Lightstone's OM4 preset is a minimum-spec fiber: its EMB with the default VCSEL launch is ≈ 4710 MHz·km, just at the limit (its OFL bandwidth, ≈ 1450 MHz·km, is below the nominal 3500 because the model is calibrated to the laser launch; `docs/model/mmf.md`).

---

## 7. Detection and noise

### 7.1 Photodiode and TIA

The photodiode converts power to current with responsivity R = 0.5 A/W; the TIA converts current to voltage with transimpedance Z = 1000 Ω:

$$u(t) = Z \cdot R \cdot P_\text{det}(t)$$

At the end of 100 m (cabled attenuation 3.5 dB/km) the average received power is ≈ 0.92 mW (−0.35 dBm). The outer levels are ≈ 410 mV apart at the TIA output, ≈ 137 mV between neighboring levels.

**Receive filter.** A 4th-order Bessel–Thomson low-pass with f_3dB = R_s/2 ≈ 26.6 GHz, like the IEEE reference receiver. Its **noise bandwidth** B_n = ∫|H|²df ≈ 27.8 GHz sets how much noise gets through.

### 7.2 Three noise sources

Each noise source is white Gaussian noise with a known strength (one-sided PSD S), filtered by everything after it. Its standard deviation at the output is σ = √(S·B):

| Source | PSD | σ at I ≈ 0.5 mA |
|---|---|---|
| Thermal (TIA) | i_n² with i_n = 15 pA/√Hz | 2.5 µA → 2.5 mV |
| Shot | 2qI | 2.1 µA → 2.1 mV |
| RIN | RIN · I², RIN = −140 dB/Hz | ≈ 8 µA → ≈ 8 mV |

Two things to notice:

- **RIN dominates** here, and like shot noise it is **signal-dependent**: the top level is noisier than the bottom one. Lightstone computes σ(t) for every sample, not one σ for the whole signal.
- Noise (≈ 9 mV total) is much smaller than the level spacing (137 mV). At 100 m the eye is closed by **ISI**, not by noise. That is why equalization, not more power, is what 100 m links need, and why the reference link ends with a 5-tap FFE (`docs/model/ffe.md`): it brings the SER from 0.10 to 1.9 × 10⁻⁷.

### 7.3 Why noise is carried symbolically

Lightstone doesn't only add random numbers. Each signal carries a **description** of its noise: which sources, how strong, and which filters they have passed. As long as everything is linear, the exact variance at any point follows from

$$\sigma^2(t) = \sum_\text{sources}\tfrac{f_s}{2}\thinspace(g^2 * S)(t),\qquad g = \mathcal{F}^{-1}\lbrace H\rbrace$$

where g is the impulse response the noise has seen. Knowing σ(t) exactly makes very low error rates computable (next section).

---

## 8. Deciding: symbol errors and TDECQ

### 8.1 Thresholds and SER

The decision circuit samples once per symbol at the best phase and compares with three thresholds θ₀, θ₁, θ₂ halfway between the levels. (In Lightstone this is done by the analysis, `pam4_analyzer`; the graph node `receiver_output` only provides the waveforms and σ(t).) A symbol is wrong if noise pushes it across a threshold.

For Gaussian noise, the probability of crossing a threshold at distance d from the sample is

$$Q\mskip-3mu \left(\frac{d}{\sigma}\right),\qquad Q(x) = \tfrac12\thinspace\mathrm{erfc}\mskip-3mu \left(\tfrac{x}{\sqrt2}\right)$$

Lightstone's **semi-analytic SER** applies this to every one of the 8191 noiseless samples y_n with its own σ_n:

$$\mathrm{SER} = \frac1N\sum_n\left[Q\mskip-3mu \left(\frac{y_n - \theta_{k_n - 1}}{\sigma_n}\right) + Q\mskip-3mu \left(\frac{\theta_{k_n} - y_n}{\sigma_n}\right)\right]$$

This includes ISI exactly (each sample has its own position) and resolves SERs like 10⁻¹⁵, far below what counting errors in 8191 symbols could show.

Sanity check (validation test V12): for ideal levels with spacing d and constant σ the formula reduces to

$$\mathrm{SER} = \tfrac32\thinspace Q\mskip-3mu \left(\frac{d}{2\sigma}\right)$$

### 8.2 TDECQ in one paragraph

TDECQ (transmitter and dispersion eye closure, quaternary) is the IEEE figure of merit for PAM-4 optical links. Take the received optical waveform through the reference receiver and a 5-tap equalizer, then ask: **how much Gaussian noise σ_G can be added before the SER reaches 4.8 × 10⁻⁴?** Compare with an ideal transmitter:

$$\mathrm{TDECQ} = 10\log_{10}\frac{\mathrm{OMA_{outer}}/(6\thinspace Q_t)}{\sigma_G / C_\text{eq}},\qquad Q_t = 3.414$$

C_eq corrects for the noise the equalizer itself amplifies. An ideal signal gives 0 dB. Lightstone's default link gives ≈ 1.8 dB at 0 m (VCSEL bandwidth alone), ≈ 2.6 dB at 100 m and ≈ 3.8 dB at 150 m.

---

## 9. Linear vs. nonlinear components

### 9.1 Why linear is fast

All blocks after the VCSEL are linear in optical power or current. Linear blocks just multiply transfer functions, and the order doesn't matter:

$$H_\text{total}(f) = H_V(f)\cdot H_\text{fiber}(f)\cdot R\cdot Z\cdot H_\text{BT}(f)$$

The whole link is one multiplication and one inverse FFT. The 18 mode groups appear only as 18 terms in H_fiber; their individual waveforms never have to be stored. In the graph engine an optical signal therefore stays **lazy**: a reference to the source plus "the transfer function so far", a few kilobytes instead of megabytes.

### 9.2 What a nonlinear block changes

Suppose the TIA clips its output at ±0.5 V: y = clip(x).

**Superposition fails:**

$$\mathrm{clip}(0.4\thinspace\text{V} + 0.3\thinspace\text{V}) = 0.5\ \text{V} \mskip5mu \neq\mskip5mu \mathrm{clip}(0.4\thinspace\text{V}) + \mathrm{clip}(0.3\thinspace\text{V}) = 0.7\ \text{V}$$

Consequences:

1. **Waveforms must exist.** Transfer functions can't be pushed through the clipper, so the engine computes the actual waveform right before it. For one electrical waveform that is cheap (≈ 1 MB). For an optical nonlinear block before the detector it would be one waveform per mode bin, ≈ 19 MB for 18 groups.
2. **Order matters.** "Filter, then clip" ≠ "clip, then filter".
3. **Noise is no longer Gaussian.** Clipping cuts off the noise tails, so σ(t) no longer describes the noise and the semi-analytic SER would be wrong. The engine switches to a **Monte Carlo SER**: simulate a random noise realization, push it through the clipper, count errors. With 8191 symbols that only resolves SER ≳ 10⁻³, and the result says so.

**Custom components** count as nonlinear unless they declare a transfer function, because the engine can't see inside them. Declaring linearity where it is true keeps everything fast.

---

## 10. Polarization and phase: reserved for later

Light also has a **polarization** and, as a wave, a **phase**. Lightstone's current components don't need either: the multimode fiber scrambles polarization, the photodiode doesn't see it, and phase averages out (Section 4.3). The signal format still has room for both, so later components (polarizers, single-mode fibers, coherent receivers) don't require changing existing ones.

An optical signal has a **representation level**:

| Level | Per mode bin | Example |
|---|---|---|
| `Power` (default) | power S₀ | all current components |
| `Stokes` | Stokes vector (S₀, S₁, S₂, S₃) | polarizer, polarization-dependent loss |
| `Field` | complex envelopes (E_x(t), E_y(t)) | interferometer, coherent receiver |

**Stokes vectors** describe partially polarized power:

| Light | Stokes vector |
|---|---|
| unpolarized, 1 mW | (1, 0, 0, 0) mW |
| horizontally polarized, 1 mW | (1, 1, 0, 0) mW |
| right-circular, 1 mW | (1, 0, 0, 1) mW |

Example: an ideal polarizer passes half of unpolarized light, and cos²θ of linearly polarized light at angle θ (Malus' law). For unpolarized light this averages to ½.

**Phase** only exists at the `Field` level. Example: an interferometer splits light into two paths and recombines it. With a phase difference Δφ and full coherence the output power is

$$P_\text{out} = P_\text{in}\cos^2(\Delta\varphi/2)$$

With partial coherence the fringe shrinks by γ(Δτ) from Section 4.4; for a VCSEL and a path difference of a few ps it disappears. Two different lasers never interfere at all.

The engine uses the **lowest level** each wire needs. A signal can always go down (Field → Stokes → Power, by averaging), but not up, because a power value has no phase left to recover.

---

## 11. Cheat sheet

| Quantity | Symbol | Default value |
|---|---|---|
| Symbol rate / UI | R_s / T | 53.125 GBd / 18.8 ps |
| Samples | s, f_s, N | 16 per UI, 850 GS/s, 131 056 |
| Power levels | P₀…P₃ | 0.570 … 1.431 mW |
| OMA_outer | P₃ − P₀ | 0.861 mW |
| VCSEL bandwidth | f_3dB | ≈ 28 GHz |
| V-number | V | ≈ 37 |
| LP / spatial modes | — | 90 / 171 |
| Mode groups (mode bins at default resolution) | — | ≈ 18 |
| Coherence time / length (glass) | τ_c, L_c | ≈ 1.4 ps / ≈ 0.3 mm |
| VCSEL spectral width | σ_λ | 0.4 nm RMS |
| RMS delay spread @ 100 m | — | ≈ 5.1 ps |
| Chromatic broadening @ 100 m | σ_k | ≈ 3.7 ps |
| Fiber channel bandwidth @ 100 m | — | ≈ 33 GHz |
| EMB / OFL (OM4 preset) | — | 4710 / 1450 MHz·km |
| Receive filter | f_3dB, B_n | 26.6 GHz, 27.8 GHz |
| Noise σ (thermal / shot / RIN) | — | 2.5 / 2.1 / ≈ 8 mV |
| TDECQ @ 0 / 100 / 150 m | — | ≈ 1.8 / 3.3 / 5.7 dB |

### Glossary

| Term | Meaning |
|---|---|
| UI | Unit interval, the duration of one symbol |
| VCSEL | Vertical-cavity surface-emitting laser |
| DBR | Distributed Bragg reflector: multilayer mirror of a VCSEL |
| Relaxation oscillation | Energy exchange between electrons and photons; sets the VCSEL's f_r |
| Degree of coherence γ | How strongly two delayed copies of light interfere, 0…1 |
| Coherence time / length | Delay / distance at which γ has dropped to 1/e |
| Mode bin | One slot in the mode dimension of a signal: one mode or one mode group |
| PAM-4 | Pulse amplitude modulation with 4 levels (2 bits per symbol) |
| ISI | Intersymbol interference: one symbol leaking into its neighbors |
| ER / OMA | Extinction ratio P₃/P₀ / optical modulation amplitude P₃ − P₀ |
| DMD | Differential mode delay: delay differences between fiber modes |
| OFL / EMB | Overfilled-launch / effective modal bandwidth of a multimode fiber |
| RIN | Relative intensity noise of the laser |
| SER / BER | Symbol / bit error ratio |
| TDECQ | IEEE PAM-4 transmitter metric: noise penalty after a reference equalizer |
| Lazy / materialized | Signal stored as "source + transfer function" vs. explicit waveforms |

### Further reading in this repository

- Building your own graphs and components in Python: [`tutorial-component-graph.md`](tutorial-component-graph.md)
- Exact signal definitions: [`signal-spec.md`](signal-spec.md)
- Hands-on version of this tutorial (each wire of the reference link inspected, fiber and detection recomputed by hand): `notebooks/signal_tutorial.ipynb`
- Component models with equations, limits and validation: [`model/`](model/)
- Validation tests and status: [`validation.md`](validation.md)
- Interactive version: the browser demo, where every component has its math panel

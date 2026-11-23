# Lightstone – Signal Specification

**Status:** Draft v0.4 (basis for the graph engine, [`architecture.md`](architecture.md) Section 7). v0.2 added polarization (Section 8); v0.3 added phase and coherence (Section 9) and an overview of all signal dimensions (Section 10); v0.4 reserves bidirectional ports and frequency-domain subcircuits for photonic circuits (Section 11).

Implemented in `lightstone-core::signal` (graph engine step 1; status and deviations in docs/architecture.md, Section 7.6). Engine-applied transfer functions, materialization for nonlinear electrical blocks and custom components: step 2, docs/architecture.md, Section 7.7.

This document defines the signals that flow between components in a Lightstone graph, how they are represented internally, and why linear and nonlinear components are handled differently.

---

## 1. Principles

1. **Typed ports.** Every component port has a signal type. Wires connect only matching types.
2. **One time grid per simulation.** All signals share the same grid: symbol rate R_s, samples per symbol s, number of symbols N_sym (an integer number of pattern periods), so N = N_sym·s samples at f_s = R_s·s. All waveforms are periodic, which makes every filter an exact circular convolution.
3. **SI units** everywhere (W, V, A, s, Hz, m).
4. **Lazy where possible.** Signals stay in a compact symbolic form ("source + linear transfer function") as long as they pass through linear components. They are converted into explicit waveforms (**materialized**) only where a component needs them.
5. **Noise is carried, not just added.** Signals carry a description of their noise, so the analysis can compute the exact noise variance σ²(t) while the path is linear and knows when it is not.
6. **Polarization is part of every optical signal**, even where today's components ignore it. The default state is *unpolarized* and costs nothing; components that care about polarization can be added later without changing the signal format (Section 8).
7. **Phase is part of the format too.** An optical signal has a *representation level*: `Power` (default), `Stokes` or `Field`. Only `Field` carries phase (complex envelopes). Today's components run at `Power`; coherent components can be added later within the same `Optical` type, without a separate signal type and without changing existing components (Section 9).

---

## 2. Signal types

| Type | Meaning | Content |
|---|---|---|
| `Data` | Symbol stream | Symbol sequence (PRBS13Q, values 0…3, alphabet size 4; or PRBS13, values 0…1, alphabet size 2 for NRZ), symbol rate |
| `Electrical` | Voltage or current waveform | Waveform x(t) \[V or A\], quantity tag (voltage/current), noise description (Section 5) |
| `Optical` | Optical signal in a mode basis (multimode today; single-mode is the special case of one mode) | One or more **source contributions** (Section 3), mode basis, mode resolution, representation level `Power` / `Stokes` / `Field` (Sections 8, 9) |

Data signals carry no noise and are always copied freely.

---

## 3. Optical signal

### 3.1 Physical model

Light from a VCSEL is partially coherent with a coherence time of ~ps, shorter than the delay spread between fiber modes. Intermodal interference therefore averages out, and optical power can be tracked **per mode** (or per mode group) and summed at the detector:

$$P_\text{det}(t) = \sum_{m} P_m(t)$$

Light from **different** lasers is mutually incoherent, so combining sources is also a sum of powers.

### 3.2 Contents

```
OpticalSignal
  wavelength           λ₀ [m] and RMS spectral width σ_λ [m]      (per source contribution)
  basis                reference to a mode basis: fiber geometry + solved mode set,
                       or "free space" before the first fiber
  resolution           ModeGroup | Mode                           (Section 3.4)
  level                Power | Stokes | Field                     (Sections 8, 9)
  contributions[]      one per source (VCSEL) feeding this wire
     source            reference to the source's waveforms:
                         P_tx(t) spectrum, RIN spectrum, P_tx(t)² spectrum,
                         emitted polarization state (Stokes vector, or Jones vector at `Field`),
                         coherence descriptor (Section 9.4)
     repr              Lazy | Materialized                        (Section 3.3)
```

A wire fed by one VCSEL has one contribution. Behind a combiner of four VCSELs it has four.

### 3.3 Representations

**Mode bins.** A *mode bin* is one slot along the mode dimension of an optical signal, just as an FFT bin is one slot along the frequency axis. A multimode fiber carries light in many modes at once (90 LP modes for OM4 at 850 nm), each arriving at a slightly different time. A mode bin records, for one mode or one group of modes (Section 3.4), how much power it carries and how late that power arrives. It holds no waveform: the waveform is reconstructed from the source signal only when needed. At the detector the mode bins add up as power, P_det(t) = Σ_k P_k(t), so each mode bin contributes one delayed, weighted term to the fiber's transfer function (Section 6.3). Two equal mode bins arriving 10 ps apart, for example, put a notch into |H(f)|, and that is what limits the fiber's bandwidth.

**Naming convention** (docs and code): always say *mode bin* for a slot of the mode dimension and *FFT bin* for a slot of the frequency axis of the simulation grid. In code, mode bins are `ModeBin` / `mode_bins`, and FFT bin indices are `k` with frequency `dsp::freq(k, n, fs)`. Histograms (eye density, TDECQ) use *histogram bins*.

**Lazy** (default). Per mode bin k (mode or mode group):

| Field | Meaning |
|---|---|
| w_k | power weight (launch fraction × losses so far); with `Stokes` polarization a 4-vector **s**_k = (S₀, S₁, S₂, S₃)_k with S₀ = w_k (Section 8) |
| τ_k | accumulated relative group delay \[s\] |
| D_k | accumulated chromatic delay slope \[s/m\] (broadening σ_k = \|D_k\|·σ_λ) |
| H_extra(f) | optional extra linear filter common to all mode bins (e.g. from a custom linear component) |

The waveform it stands for is

$$P_k(t) = \mathcal{F}^{-1}\Big\lbrace \hat P_\text{tx}(f)\mskip5mu w_k\thinspace e^{-j2\pi f \tau_k}\thinspace e^{-2\pi^2 f^2 \sigma_k^2}\thinspace H_\text{extra}(f) \Big\rbrace$$

Nothing is computed yet; a lazy signal costs a few kilobytes.

**Materialized.** Explicit per-mode-bin power waveforms P_k(t), each N samples, plus the matching per-mode-bin RIN waveforms. Produced on demand from the lazy form (one inverse FFT per mode bin and source). With `Stokes` polarization, each mode bin holds four waveforms S₀…S₃(t).

### 3.4 Mode resolution

| Resolution | Mode bins (OM4, 850 nm) | Approximation |
|---|---|---|
| `ModeGroup` (default) | ≈ 18 principal mode groups g = 2m + l − 1 | All modes of a group share one delay and slope (power-weighted mean of the group) |
| `Mode` | ≈ 90 LP modes | None beyond the model itself |

Grouping is nearly exact for an ideal parabolic profile, where modes in one group are degenerate. For the calibrated OM4 preset (α = 2.0777), delays inside a group differ slightly, so grouping is slightly optimistic. The resolution is a simulation setting, so components never depend on it. A required test compares both resolutions on the reference link and reports the TDECQ and SER difference.

### 3.5 Memory

Per waveform: N = 8191 × 16 = 131 056 samples × 8 bytes ≈ **1.05 MB** (native, 16 samples/UI) or ≈ **0.52 MB** in the browser (8 samples/UI).

| Signal on one wire | Native | Browser |
|---|---|---|
| Lazy | ~ kB | ~ kB |
| Materialized, mode groups (18) | ≈ 19 MB | ≈ 9.4 MB |
| Materialized, modes (90) | ≈ 94 MB | ≈ 47 MB |

RIN doubles these numbers, and the `Stokes` and `Field` levels multiply materialized signals by four. This is why signals stay lazy whenever possible, and why `Power` (only S₀) is the default.

### 3.6 Mode basis between components

A lazy optical signal is tied to the mode basis of the fiber it travels in.
- **Same basis** (same core radius, NA, profile type and grid): mode bins map one-to-one by label.
- **Different fibers:** a coupling component with an explicit coupling matrix is required. Not modeled in the first version; the editor reports an error.
- **Free space → fiber:** the launch (connector) component converts the VCSEL near field into mode bin weights via overlap integrals.

---

## 4. Electrical signal

```
ElectricalSignal
  quantity     Voltage | Current
  waveform     x(t), N samples (noiseless part)
  noise        NoiseDescription (Section 5)
```

Electrical signals are always materialized. There is only one waveform, so they are cheap (≈ 1 MB).

---

## 5. Noise description

Noise is not simply added as random numbers at the point where it arises. Each signal carries a list of **noise terms**, each a white Gaussian source with a known (possibly time-varying) strength, together with the linear filter it has passed through since it was generated:

```
NoiseTerm
  kind        Thermal | Shot | RIN | Jitter | Custom | Nonlinear
  strength    S(t): one-sided PSD at the point of generation (constant or waveform)
  filter      H(f) accumulated since generation
NoiseDescription
  terms[]
  gaussian_exact: bool     true while every block since the noise sources was linear
  realization            optional explicit random waveform (for scopes and Monte Carlo)
```

While `gaussian_exact` is true, the variance at any point is exact and cheap:

$$\sigma^2(t) = \sum_\text{terms} \tfrac{f_s}{2}\thinspace(g^2 * S)(t),\qquad g = \mathcal{F}^{-1}\lbrace H\rbrace$$

This is the formula the current receiver already uses (docs/architecture.md, Section 3.5). It is what makes the semi-analytic SER possible: SER values far below 1/N_sym without simulating billions of symbols.

After a nonlinear block, the noise that passed it is carried as one explicit realization (kind `Nonlinear`) with no strength: it is part of the noisy waveform but not of σ²(t), and `gaussian_exact` is false. Noise terms added after the block are Gaussian again and enter σ²(t) as usual.

---

## 6. Linear and nonlinear components

### 6.1 Definitions

A component is **linear (time-invariant)** if its output is a fixed transfer function applied to its input:

$$y = \mathcal{F}^{-1}\lbrace H(f)\thinspace\hat x(f)\rbrace$$

For linear components, superposition holds (y(a + b) = y(a) + y(b)) and the order of linear blocks does not matter (H₁H₂ = H₂H₁).

A component is **nonlinear** if its output depends on the signal level. Examples: TIA clipping, a limiting amplifier, VCSEL compression at high drive, power-dependent loss, the decision circuit. Components are also treated as nonlinear if Lightstone cannot know their behavior, which is the case for every custom component unless it declares itself linear.

Every component declares one of:

| Declaration | Meaning | Engine behavior |
|---|---|---|
| `Linear(H)` | Provides H(f) (per mode bin for optical; optionally a 4×4 Mueller matrix M(f) per mode bin, Section 8) | Signal stays lazy; noise terms get H appended |
| `Nonlinear` | Needs explicit input waveforms | Inputs are materialized; `gaussian_exact` becomes false |

Implementation notes (step 2): a linear component declares H(f) through `Component::transfer`, and the engine applies it. Custom components may instead give an impulse response (continuous h(t) or FIR taps); the engine converts it to H(f) on the periodic grid, which is exact as long as h is band-limited to fs/2 and decays within half a pattern period. For optical signals that H acts on power and is common to all mode bins; per-mode-bin H(f) and Mueller matrices are not implemented yet. H(f) must have a real impulse response, H(−f) = H(f)\*. Nonlinear custom components may only have data and electrical ports for now.

### 6.2 Why this matters

**Cost.** A chain of linear blocks collapses into **one** transfer function. The engine evaluates H_total(f) once and applies it with one FFT, however many blocks and modes there are. A nonlinear block forces the engine to build the actual waveform right before it, and for optical signals that means one waveform per mode bin (Section 3.5).

**Noise statistics.** Gaussian noise through a linear filter stays Gaussian with a computable variance. Through a nonlinear block the distribution changes, for example clipping cuts off the tails. After that, σ(t) no longer describes the noise and the semi-analytic SER would be wrong.

### 6.3 Example

Link: PRBS13Q → driver → VCSEL → connector → 100 m OM4 → PIN → TIA → Bessel–Thomson filter → FFE → receiver output (the FFE, once adapted, is one more fixed linear filter).

**Case A: all linear (today's model).**

Every block after the VCSEL is linear in optical power or current, so the engine computes

$$H_\text{total}(f) = H_\text{VCSEL}(f)\cdot \underbrace{\sum_k w_k\thinspace e^{-j2\pi f\tau_k} e^{-2\pi^2 f^2\sigma_k^2}}_{\text{fiber, 18 mode groups}} \cdot R \cdot Z \cdot H_\text{BT}(f)$$

and obtains the receiver output with a single inverse FFT of $\hat x_\text{NRZ}(f)\thinspace H_\text{total}(f)$. The 18 mode groups only appear as 18 terms in a sum over frequency points; their individual waveforms are never stored. The thermal, shot and RIN noise terms are each filtered linearly, so σ²(t) is exact and the semi-analytic SER is valid down to 10⁻¹⁵ and below.

Cost: a few FFTs of 131 k points, ≈ 50 ms natively.

**Case B: TIA with output clipping at ±0.5 V.**

The TIA now computes y(t) = clip(x(t), −0.5 V, +0.5 V).

1. **Superposition fails.** With inputs a = 0.4 V and b = 0.3 V: clip(a + b) = clip(0.7 V) = 0.5 V, but clip(a) + clip(b) = 0.7 V. The engine can no longer push transfer functions through the TIA.
2. **Order matters.** "Filter, then clip" gives a different waveform than "clip, then filter", so H_BT cannot be merged with the blocks before the TIA.
3. **Materialization.** The engine must compute the explicit TIA input waveform x(t). Here this is cheap, because it is one electrical waveform (≈ 1 MB, one extra FFT). Everything before the TIA, including the 18 fiber mode groups, still collapses into one transfer function.
4. **Noise.** Thermal and shot noise arise inside the receiver before the clipping. Their Gaussian description is no longer valid after the clip, so `gaussian_exact` becomes false. The analysis then reports a **Monte Carlo SER**: an explicit noise realization goes through the clipper and errors are counted. With one pattern period (8191 symbols), SERs below ≈ 10⁻³ cannot be resolved. The result shows the method and its resolution limit; longer patterns lower the limit.

**Case C: a custom optical component in the middle of the fiber.**

Example: a user-written Python component between two fiber spans that applies a power-dependent loss to each mode group.

1. The engine cannot see inside the Python code, so the component counts as nonlinear.
2. The optical signal entering it must be materialized: 18 mode-group waveforms plus 18 RIN waveforms, ≈ 38 MB natively (≈ 19 MB in the browser).
3. The component's output is a materialized optical signal. Downstream linear blocks (second fiber span, receiver) apply their transfer functions to each mode bin's waveform, so cost grows with the number of mode bins.
4. If the custom component declares itself `Linear(H)` and provides H(f) per mode bin, none of this happens and the signal stays lazy.

Rule of thumb: nonlinear electrical blocks are cheap, nonlinear optical blocks before the detector are expensive, and declaring linearity where it is true keeps everything fast.

---

## 7. Fan-out and fan-in

| Situation | Behavior |
|---|---|
| Data or electrical output → several inputs | The signal is shared (copied). No physical effect. |
| Optical output → probe or analyzer | Ideal tap by default: lossless, signal unchanged. **Realistic** option: tap ratio (e.g. 2 %) and through-path insertion loss; the analyzer sees only the tapped power. |
| Optical output → several optical components | Requires an explicit **splitter** (ratio, excess loss). The editor offers to insert one when a second optical wire is drawn. |
| Several optical outputs → one input | Requires an explicit **combiner**. Contributions are kept separate (incoherent sources) and summed at the detector. |

A splitter is linear: each output receives the input's contributions with weights scaled by its ratio, and both stay lazy.

---

## 8. Polarization

### 8.1 Why it is in the format now

Today's 850 nm multimode components do not need polarization: the fiber scrambles it within a short distance and the PIN photodiode measures total power. Later use cases do need it: polarization-dependent loss (PDL), VCSEL polarization switching, polarizers and polarization-sensitive components, and single-mode or coherent systems. Putting polarization into the signal format now means those components can be added without changing existing ones or saved systems.

### 8.2 Representation: Stokes vectors

For incoherent, power-based signals the natural description is the **Stokes vector** per mode bin:

$$\mathbf{s} = (S_0, S_1, S_2, S_3),\qquad S_0 = \text{total power},\qquad \mathrm{DOP} = \frac{\sqrt{S_1^2 + S_2^2 + S_3^2}}{S_0} \le 1$$

| Component | Meaning |
|---|---|
| S₀ | total power (what today's model tracks as w_k) |
| S₁ | horizontal minus vertical linear polarization |
| S₂ | +45° minus −45° linear polarization |
| S₃ | right minus left circular polarization |

Stokes vectors describe partially polarized light (degree of polarization DOP between 0 and 1) and add up for incoherent sources, which matches the incoherent multimode model and the incoherent combination of several VCSELs.

The representation level (Section 9.2) selects what is stored:

| Mode | Stored per mode bin | Use |
|---|---|---|
| `Power` (default, unpolarized) | S₀ only; S₁ = S₂ = S₃ = 0 implied | All current components. No extra cost. |
| `Stokes` | S₀…S₃ | As soon as any component in the graph is polarization-sensitive or a source is declared polarized |

The engine switches a signal to `Stokes` automatically when a polarization-sensitive component is upstream of it or a source emits a polarized state. Converting back is lossless only if S₁ = S₂ = S₃ = 0.

### 8.3 Components: Mueller matrices

A linear, polarization-aware component maps Stokes vectors with a 4×4 **Mueller matrix** M (constant or frequency-dependent per mode bin):

$$\mathbf{s}_\text{out} = M\thinspace\mathbf{s}_\text{in}$$

| Component | Mueller matrix |
|---|---|
| Polarization-independent loss T (all current components) | T·I₄. Acts on S₀ only at the `Power` level, so existing components need no change |
| Ideal depolarizer (model of a long multimode fiber) | diag(1, 0, 0, 0) |
| Linear polarizer at angle θ | standard polarizer matrix (Malus' law) |
| Diattenuator with PDL | standard diattenuator matrix (maximum/minimum transmission) |

**Default for the multimode fiber span:** polarization-independent loss and full depolarization (S₁, S₂, S₃ → 0) at the output. This states the scrambling assumption explicitly instead of hiding it.

**Default for the PIN photodiode:** detects S₀ only (polarization-insensitive). A polarization-sensitive detector would add a responsivity term on S₁…S₃.

### 8.4 Sources

Each source contribution carries its emitted Stokes vector. The VCSEL default is unpolarized (DOP = 0), which matches the current model. A polarized VCSEL (e.g. DOP = 1, linear at 0°) can be set without other changes; it only becomes visible if a polarization-sensitive component follows before the light is depolarized.

VCSEL polarization switching (time-varying polarization) needs time-dependent Stokes waveforms, i.e. materialized `Stokes` signals. The format supports it; no component models it yet.

### 8.5 Polarization at the `Field` level

At the `Field` level (Section 9) polarization is carried as a **Jones vector** per mode bin, two complex envelopes (E_x(t), E_y(t)), and components use 2×2 Jones matrices. This covers polarization mode dispersion (PMD) and polarization multiplexing. The Stokes vector follows from the Jones vector, so `Field` converts to `Stokes` and `Power` without loss of the quantities those levels carry.

### 8.6 Required tests

- **P1:** a graph without polarization-sensitive components gives bit-identical results at the `Power` and `Stokes` levels.
- **P2:** an ideal polarizer transmits exactly half of unpolarized power; for fully polarized light it follows Malus' law, cos²θ.
- **P3:** depolarizer output has DOP = 0 and the same S₀.
- **P4:** incoherent combination of two orthogonally polarized sources of equal power gives DOP = 0.

## 9. Phase and coherence

### 9.1 Why `Power` drops phase, and why the format keeps it anyway

The `Power` and `Stokes` levels are the **incoherent limit**. The VCSEL coherence time (a few ps) is shorter than the delay differences between fiber modes, so relative phases between modes are random, interference terms average out, and the detector sees the sum of mode powers. Carrying phase there would cost memory without changing results.

Later use cases need phase: coherent detection (QAM), laser chirp interacting with chromatic dispersion, PMD and polarization multiplexing, interferometers, modal noise and multipath interference (MPI), laser phase noise. The `Field` level reserves the place for this in the same `Optical` type.

### 9.2 Representation levels

| Level | Per mode bin | Phase | Describes |
|---|---|---|---|
| `Power` (default) | S₀ (power) | no | Incoherent, unpolarized power: all current components |
| `Stokes` | S₀…S₃ | no | Incoherent, partially polarized power (Section 8) |
| `Field` | Jones vector of complex envelopes (E_x(t), E_y(t)) | **yes** | Coherent or partially coherent fields |

Conversions:
- **Downward** (`Field` → `Stokes` → `Power`) is always defined: Stokes parameters follow from the Jones vector by averaging over the coherence time, and S₀ = \|E_x\|² + \|E_y\|².
- **Upward** is only possible at a **source**, which can emit at any level. A `Power` signal cannot be promoted to `Field` later, because its phase information is gone.
- The engine picks the **lowest level** each wire needs: a wire runs at `Field` only if some component downstream of it requires phase. Graphs built from today's components therefore never pay for `Field`.

### 9.3 Complex envelope convention

At the `Field` level each mode bin carries the slowly varying complex envelope relative to a reference optical carrier ν₀ (from the source wavelength):

$$E_\text{physical}(t) = \mathrm{Re}\big\lbrace\thinspace E(t)\thinspace e^{\thinspace j2\pi\nu_0 t}\big\rbrace,\qquad |E(t)|^2 = P(t)\ \ [\mathrm{W}]$$

- Propagation phases are stored **relative to a reference** (the reference mode's β·L, which is ~10⁹ rad over 100 m). Only differences are kept, in f64, to avoid the precision loss that absolute optical phases would cause.
- The **lazy form** of a `Field` signal is the same as for `Power`, except the per-mode-bin transfer functions are **complex**: a linear component declares a complex H_k(f) (including phase) or a 2×2 Jones matrix J_k(f) per mode bin, instead of a power weight.
- A **mode-coupling component** (e.g. an offset connector between fibers) acts on fields with a complex coupling matrix C (mode bins × mode bins). At `Power` level, the same component acts with |C|² (a power-transfer matrix). Both forms belong to the same component.

### 9.4 Coherence descriptor

Every source contribution carries a coherence descriptor: optical spectrum shape (Gaussian or Lorentzian), linewidth or RMS width, and from it the **degree of coherence** γ(Δτ), the coherence function for a delay difference Δτ (e.g. γ = exp(−(π·Δν·Δτ)²/(4 ln 2)) for a Gaussian spectrum).

How it is used:
- **Detection rule** (general form): fields of the **same source** add coherently, weighted by γ of their delay difference; contributions from **different sources** always add as powers. In the incoherent limit (γ → 0 for all mode pairs), this reduces exactly to the `Power` model.
- **Validity check (usable now):** if γ(Δτ) between significantly excited modes is not negligible, i.e. the coherence time is not much shorter than the modal delay spread (very narrow linewidth, very short fiber), the engine warns that the `Power` model is optimistic, because interference effects such as modal noise are not included.
- The fiber already uses the RMS spectral width σ_λ for chromatic broadening. The coherence descriptor extends that same information; it does not add a second, inconsistent source of truth.

### 9.5 Detection at the `Field` level

A photodiode measures intensity integrated over its area. With a finite detector, cross terms between modes do **not** vanish, because the modes are only orthogonal over the whole plane:

$$I(t) = R \sum_{k,l} \eta_{kl}\thinspace\gamma(\tau_k - \tau_l)\thinspace\mathrm{Re}\lbrace E_k(t)\thinspace E_l^*(t)\rbrace$$

η_kl is the detector overlap matrix: η_kl = δ_kl·η_k for full-area detection, where the formula reduces to the incoherent sum. Today's detector already stores η_k (the diagonal); the full matrix is the reserved extension.

### 9.6 Cost

`Field` stores two complex envelopes per mode bin (x, y), i.e. four real waveforms per mode bin: the same as `Stokes`, four times `Power`. Coherent mode coupling makes the lazy form a matrix instead of a vector per mode bin. This is acceptable for single-mode and few-mode cases; for ~90 multimode mode bins it is expensive and only used where explicitly needed.

### 9.7 Required tests

- **F1:** a graph of current components gives bit-identical results with sources emitting at `Power` and at `Field` (incoherent limit).
- **F2:** an interferometer (one source, splitter, two paths, combiner) with zero delay difference and a narrow linewidth shows the expected fringe, P_out ∝ cos²(Δφ/2); with a delay difference ≫ coherence time it shows no fringe.
- **F3:** two different sources never interfere, regardless of linewidth.
- **F4:** `Field` → `Stokes` conversion matches the analytic Stokes vector for linear, circular and elliptical polarization.

## 10. Signal dimensions (overview)

Reserved means: defined in the format, so later components can use it without changing existing ones.

| Dimension | Representation | Used by current components |
|---|---|---|
| Space / modes | Mode basis + mode bins (`ModeGroup` or `Mode`) | Yes |
| Time | Common periodic time grid (R_s, s, N_sym) | Yes |
| Amplitude / power | S₀ per mode bin (`Power`) or \|E\|² (`Field`) | Yes |
| Polarization | Unpolarized, Stokes vector or Jones vector | Reserved (format only) |
| Phase | Complex envelope relative to ν₀ (`Field`) | Reserved (format only) |
| Optical frequency | Center wavelength per contribution; several contributions for several wavelengths | Yes (center + width) |
| Coherence | Coherence descriptor per source: spectrum shape, width, γ(Δτ) | Width yes; validity check planned |
| Noise | Noise terms with accumulated filters (Section 5) | Yes |
| Direction | Forward and backward wave per optical port (Section 11.1) | Forward only; backward reserved |
| Circuit loops | Cycles in the graph, solved per strongly connected component: per frequency (linear, incl. S-matrix subcircuits, Section 11.2) or by fixed-point iteration (nonlinear, steady state) | Reserved (docs/architecture.md, 7.8) |

## 11. Reserved: bidirectional ports and loops

Photonic integrated circuits (a later stage) need two things the current graph does not do: light travelling in **both directions** through a component, and **loops** such as ring resonators. Both are reserved in the format now, so they can be added without changing existing components.

### 11.1 Bidirectional ports

Every optical port has two waves: **forward** (in the wire's direction) and **backward**. Reflections at interfaces (connectors, grating couplers, chip facets, waveguide ends) feed the backward wave.

- Today all components leave the backward wave **empty**. An empty backward wave costs nothing and is never computed.
- A component that reflects declares it. The engine then propagates backward waves only along the wires where they are non-empty.
- Electrical and data ports stay unidirectional.

This also covers effects that already matter in lower stages: back-reflections into lasers and multipath interference (MPI) from double reflections.

### 11.2 Frequency-domain subcircuits

A linear photonic circuit is described in the frequency domain by **scattering matrices**: each component is S(f), mapping incoming waves at all its ports (both directions) to outgoing waves.

$$\mathbf{b}(f) = S(f)\thinspace\mathbf{a}(f)$$

Connecting components links the outgoing waves of one port to the incoming waves of another. For the whole subcircuit this gives one linear system of equations per frequency. The system may contain **loops**, e.g. a ring resonator, and is solved directly for every frequency point of the grid.

Rules:
- A subcircuit is a connected group of components that are all **linear** and support S-matrices. Loops are allowed **inside** it.
- After solving, the subcircuit acts on the rest of the graph like **one linear component** with a transfer function per input/output port pair (signal-spec Section 6). The rest of the engine and the lazy signal form stay unchanged.
- Subcircuits are not marked up in the graph: the engine finds them as strongly connected components (groups of nodes that feed back into each other). The graph of these units is acyclic by definition. General rules for loops, including nonlinear ones, are in docs/architecture.md, Section 7.8.
- S-matrices act on fields, so subcircuits run at the `Field` level (Section 9). A subcircuit that only contains intensity-type components can run at `Power` level with |S|².
- **Nonlinear loops** are not covered by the S-matrix mechanism. In steady state they are solved by fixed-point iteration on the periodic grid (docs/architecture.md, 7.8). Transients, e.g. lasers with optical feedback switching on, need time-stepped simulation and are out of scope.
- **S-parameter import:** a component can be defined from measured or simulated S-parameters in the Touchstone format (`.sNp`). This is the standard exchange format with device-level tools.

### 11.3 Required tests (when implemented)

- **B1:** with all backward waves empty, results are bit-identical to the unidirectional engine.
- **B2:** a ring resonator matches its analytic all-pass/add-drop response (resonance positions, free spectral range, extinction).
- **B3:** a lossless subcircuit's S-matrix is unitary (energy conservation).
- **B4:** a two-mirror cavity (Fabry–Pérot) built from two reflecting components matches the analytic transmission.

## 12. Open points

- Coupling matrices between different fibers and at offset fiber-to-fiber connectors.
- Single-mode domain and coherent components: covered by the `Field` level of the same `Optical` type (Section 9); components (DFB laser, SMF, coherent receiver) still to be built.
- Full detector overlap matrix η_kl and partial-coherence detection (Section 9.5): format reserved, not implemented.
- Polarization-dependent effects not modeled yet: PDL of real components, VCSEL polarization switching noise, vector mode splitting inside LP groups (negligible for OM4 compared with profile-induced DMD).
- Feedback loops: planned as strongly connected components of the graph, solved per frequency (linear) or by fixed-point iteration (nonlinear, periodic steady state); docs/architecture.md, 7.8. Prerequisites in this format: per-mode-bin transfer functions H_k(f) in the lazy optical form, and merging contributions of the same source. Transients (AGC or clock recovery locking in, lasers with optical feedback switching on) need time-stepped simulation and are out of scope for this engine.
- Different symbol rates in one simulation (would need resampling between grids).

# Tutorial: The Reference Link, Step by Step

This tutorial walks through the reference graph one node at a time, in signal order. For every node it answers the same questions:

- **What it does**: the physical device and its job in the link.
- **In Lightstone**: what is implemented today, with the default parameters.
- **Signal model**: what the node receives, what it hands on, and how that signal is stored inside the engine.
- **Numbers**: what the node produces with the default settings.
- **Look at it**: how to inspect it in Python (the browser shows the same in the inspector).
- **Not modeled**: the limits you should know about.

All numbers come from running the code below at the default settings (53.125 GBd PAM-4, 100 m OM4, 16 samples per symbol). The other tutorials go deeper in two directions. [`tutorial-signals-and-math.md`](tutorial-signals-and-math.md) explains the physics and math in more depth. [`tutorial-component-graph.md`](tutorial-component-graph.md) shows how to build and extend graphs. The exact definitions are in [`signal-spec.md`](signal-spec.md) and the per-component documents in [`model/`](model/).

```python
import numpy as np
import lightstone

g = lightstone.Graph.reference()
g.run()
g.nodes()
# ['src', 'drv', 'vcsel', 'el0', 'el1', 'pin', 'tia', 'rxf', 'ffe', 'rx']
```

---

## 0. The graph at a glance

```
 TRANSMITTER                          CHANNEL              RECEIVER FRONT END             DATA RECOVERY
 src ──▶ drv ──▶ vcsel ──▶            el0 ──▶ el1 ──▶      pin ──▶ tia ──▶ rxf ──▶        ffe ──▶ rx
 PRBS13Q  driver   VCSEL              connector  100 m     PIN     TIA     Bessel–        5-tap   receiver
                                                 OM4               1 kΩ    Thomson        FFE     output
  │                                                                                        ▲
  └───────────────────────── transmitted symbols (reference for adaptation) ──────────────┘
```

| # | Node | Kind | Input → output | Linear? | Frame in the browser |
|---|---|---|---|---|---|
| 1 | `src` | `prbs13q` | – → data | – (source) | Transmitter |
| 2 | `drv` | `driver` | data → electrical (drive) | yes | Transmitter |
| 3 | `vcsel` | `vcsel` | electrical (drive) → optical | no (power clipped at 0 W) | Transmitter |
| 4 | `el0` | `connector` | optical → optical | yes | Channel |
| 5 | `el1` | `mmf` (OM4) | optical → optical | yes | Channel |
| 6 | `pin` | `pin` | optical → electrical (current) | yes | Receiver front end |
| 7 | `tia` | `tia` | electrical (current) → electrical (voltage) | yes | Receiver front end |
| 8 | `rxf` | `bessel_thomson` | electrical → electrical | yes | Receiver front end |
| 9 | `ffe` | `ffe` | electrical + data (`ref`) → electrical | yes, once adapted | Data recovery |
| 10 | `rx` | `receiver_output` | electrical → – (sink) | – (sink) | Data recovery |

Three things make sense of everything that follows: the time grid, lazy signals, and noise terms.

### 0.1 One time grid

Every signal in a run lives on the same periodic grid. It is set by the graph's `settings`:

| Quantity | Value | From |
|---|---|---|
| Symbols | 8191 | one PRBS13Q period (`pattern_periods` = 1) |
| Samples per symbol | 16 | `sps` (the browser uses 8) |
| Samples | 131 056 | 8191 × 16 |
| Sample rate f_s | 850 GHz | 53.125 GBd × 16 |
| Record length | 154.2 ns | 8191 / 53.125 GBd |
| Frequency resolution | 6.49 MHz | 1 / record length |

The record is treated as one period of an endlessly repeating signal. That makes every filter an exact circular convolution, which is a multiplication in the frequency domain. This is why most signals are stored as **spectra**.

### 0.2 Lazy signals

A wire does not carry a list of samples. It carries a compact description, and waveforms are produced only where someone needs them:

| Port type | What is stored |
|---|---|
| **data** | The symbol sequence (8191 values 0…3) and the alphabet size (4 = PAM-4, 2 = NRZ) |
| **electrical** | A base spectrum, an accumulated filter H(f) and a scalar gain (waveform = gain · IFFT(base · H)), a quantity tag (drive, current, voltage) and a list of noise terms |
| **optical** | One *contribution* per laser: a reference to the source (its power spectrum, RIN and properties), the propagation state (before the fiber: launch offset, tilt and loss; in the fiber: one *mode bin* per mode, each with power, delay and chromatic delay slope), and an optional extra filter |

A linear component only edits this description: it multiplies a filter, scales a gain or adds a delay. Waveforms are computed only by the VCSEL (it needs its own power waveform), the photodiode (for the shot-noise strength), the FFE (to adapt its taps), the receiver output, the analyzers, and whenever you call `g.output()`.

### 0.3 Noise terms

Noise is not added as random numbers and forgotten. Every noise source becomes a **noise term**: white Gaussian noise of known strength at its origin, plus the filter and gain it has passed since. From the terms the engine computes two things:

- the **exact standard deviation σ(t)** at every sample, which gives the semi-analytic SER (signal-spec 5);
- one **random realization**, the noisy waveform a scope would show.

| Term | Created by | Strength at its origin |
|---|---|---|
| RIN | `vcsel` (realized there), filtered from `pin` on | RIN · P(t)² (follows the optical power) |
| Jitter | `drv` (off by default) | slope noise at the edges |
| Shot | `pin` | 2q · I(t) (follows the photocurrent) |
| Thermal | `tia` | i_n², constant |

σ(t) stays exact as long as every block after the noise sources is linear. The flag `gaussian_exact` records that. In the reference link it is true from start to end.

### 0.4 How a run works

`g.run()` sorts the nodes from the sources to the sinks and evaluates them in that order. Each result is cached under a key made of the node's parameters and its inputs. A node whose key has not changed is not recomputed. The mode solver results are cached as well and shared between nodes.

```python
g.run(); g.computed_last_run        # 0: nothing changed
g.set("el1", length=70.0)
g.run(); g.computed_last_run        # 6: el1, pin, tia, rxf, ffe, rx
g.set("el1", length=100.0); g.run()
```

The first run takes ≈ 0.4 s, mostly the fiber mode solver. Later runs recompute only what is downstream of a change.

---

## 1. `src`: the pattern generator (PRBS13Q)

**What it does.** It provides the test data. Optical PAM-4 links are specified and measured with a standard pseudo-random pattern, PRBS13Q, so the transmitter sends every short symbol combination equally often.

**In Lightstone.**
- PRBS13, polynomial 1 + x + x² + x¹² + x¹³, seed 2731 (`seed`).
- PAM-4 (`levels: 4`, default): two PRBS13 periods (16 382 bits) are taken in pairs and Gray-coded, 00 → 0, 01 → 1, 11 → 2, 10 → 3. Neighboring levels then differ in one bit, so a symbol error to the next level costs one bit (BER ≈ SER/2).
- NRZ (`levels: 2`): one PRBS13 period of 8191 bits. Both patterns have 8191 symbols, so the grid is the same.

**Signal model.** Output `out` is a **data** signal: the 8191 symbols and the alphabet size 4. It carries no noise. Every node that needs the transmitted symbols (the driver, the FFE, the analyzers) is wired to it.

**Numbers.** The first symbols are 1 1 1 1 1 1 2 2 2 2 2 2 1 3 2 1. Each of the four levels occurs 2047 or 2048 times.

**Look at it.**
```python
s = g.output("src")["symbols"]     # numpy array, 8191 symbols
np.bincount(s)                      # [2047, 2048, 2048, 2048]
```

**Not modeled.** The seed and bit order have not been checked bit for bit against IEEE 802.3 Clause 120. Longer patterns (PRBS31) are not available; more `pattern_periods` repeat PRBS13Q.

More: [`model/prbs-pattern.md`](model/prbs-pattern.md).

---

## 2. `drv`: the driver

**What it does.** The driver turns the data into the electrical signal that modulates the laser: one constant drive level per symbol (NRZ hold). In real hardware this is a DAC or a PAM-4 driver IC. Its imperfections show up as timing errors of the edges, called **jitter**.

**In Lightstone.**
- Symbol k becomes the normalized drive k/3: 0, ⅓, ⅔, 1 (NRZ: 0, 1). The VCSEL maps it to optical power.
- The waveform is the ideal rectangular drive, represented by its **exact spectrum** on the grid, X(f) = S(f) · T · sinc(fT) · e^(−jπfT). Here S is the spectrum of the symbol levels (`eq:tx-nrz`). This is the band-limited (alias-free) version of the continuous waveform, so the results do not depend on the oversampling. Rise times come from the VCSEL, the fiber and the receiver.
- Optional transmitter jitter, all zero by default (`rj_rms`, `dcd`, `sj_amplitude`, `sj_frequency`, `cdr_corner`):
  - duty-cycle distortion and sinusoidal jitter shift the edges exactly in the spectrum;
  - random jitter becomes a noise term (slope noise).

  See [`model/driver.md`](model/driver.md).

**Signal model.** Input `in`: data. Output `out`: an **electrical** signal of quantity *drive* (dimensionless). Its base spectrum is the drive spectrum, with no filter and gain 1. Without jitter it has no noise terms. It also records the alphabet size so the VCSEL can report its levels. The driver is linear.

**Numbers.** The drive has mean 0.500 and runs from −0.095 to 1.095. The 9.5 % overshoot at each edge is the Gibbs ripple of the band-limited rectangle; the VCSEL response removes it. σ = 0 (no jitter).

**Look at it.**
```python
d = g.output("drv")                 # 'noiseless', 'signal', 'sigma': 131 056 samples each
d["noiseless"][:32]                 # the first two symbols (16 samples each)
g.params("drv")                     # jitter parameters, all 0
```

**Not modeled.** No driver bandwidth, no nonlinearity of the levels, and no level mismatch. The PAM-4 levels are exactly equally spaced.

---

## 3. `vcsel`: the laser

**What it does.** A vertical-cavity surface-emitting laser converts the drive current into light at 850 nm. The optical power follows the current, but only up to the laser's modulation bandwidth. The laser also adds intensity noise (RIN). Its light leaves the chip in a few transverse modes, which matters for how it couples into the fiber.

**In Lightstone.**
- **Levels** (`eq:tx-levels`). Average power 0 dBm and outer extinction ratio 4 dB give P₀ = 2P̄/(ER + 1) and P₃ = 2P̄·ER/(ER + 1), with the inner levels equally spaced: 0.569, 0.856, 1.144 and 1.431 mW. OMA_outer = P₃ − P₀ = 0.861 mW.
- **Response** (`eq:vcsel-response`). A second-order low-pass on the power, H_V(f) = 1/(1 − (f/f_r)² + j·2ζ·f/f_r), with resonance f_r = 28 GHz and damping ζ = 0.7. The −3 dB bandwidth is 28.3 GHz.
- **Clipping.** The power is set to 0 W where the filtered waveform would go negative. With the defaults this never happens, but it makes the node formally nonlinear.
- **RIN** (`eq:vcsel-rin`), −140 dB/Hz. Gaussian power noise whose strength follows the power, RIN · P(t)². The VCSEL draws one realization with its own seed.
- **Source properties** that later nodes need:
  - wavelength 850 nm and RMS spectral width 0.4 nm, for chromatic dispersion in the fiber;
  - the near field, an incoherent mix of the 4 lowest Laguerre–Gauss mode groups with waist 4 µm, for the launch;
  - the polarization, unpolarized.

**Signal model.**
- **Input `in`.** The drive, with any noise terms it carries (jitter). These are converted into optical power noise with (P₃ − P₀) · H_V(f).
- **Output `out`.** An **optical** signal with one contribution. Its source part holds:
  - the spectrum of the transmitted power P(t);
  - the spectrum of the RIN realization;
  - the spectrum of P(t)², for the RIN variance;
  - the source properties above.
- **Propagation state.** *Free space* (offset 0, tilt 0, power factor 1): the light has not reached a fiber yet.
- **No modes yet.** The output has no mode bins. Which fiber modes get the light depends on the fiber, so the launch is computed there.

**Numbers.** Average power 1.000 mW (0.00 dBm), levels as above, VCSEL bandwidth 28.3 GHz.

**Look at it.**
```python
v = g.report("vcsel")
v["levels"]                         # [0.000569, 0.000856, 0.001144, 0.001431] W
v["vcsel_bandwidth"] / 1e9          # 28.3 GHz
v["power"]                          # transmitted power waveform [W]
g.output("vcsel")["average_power"]  # 0.001 W
```

**Not modeled.**
- Small-signal model only: no thermal rollover, gain compression or level-dependent bandwidth.
- No chirp.
- RIN is white, with no relaxation-oscillation peak.
- The transverse modes are equally excited and do not depend on the current.

More: [`model/vcsel.md`](model/vcsel.md), the launch in [`model/connector.md`](model/connector.md).

---

## 4. `el0`: the connector (launch)

**What it does.** The connector holds the fiber end in front of the laser (or of another fiber). Its alignment decides which fiber modes the light enters. A beam that hits the core center excites mainly the low-order modes. An offset or tilted beam fills higher-order modes, which travel at different speeds. The launch therefore sets how much modal dispersion the signal will see.

**In Lightstone.** Parameters: radial offset `offset` (default 0), tilt `tilt` (0) and insertion loss `loss_db` (0). In the reference link the connector is ideal.

**Signal model.**
- **Before the first fiber** (the case here). The connector does not compute anything yet. It writes its offset and tilt into the contribution's free-space state, where the last connector before the fiber wins, and multiplies the power factor by its loss.
- **When the launch is computed.** The overlap integrals that give the power per mode need the fiber's modes, so the next fiber node computes them.
- **Between two fibers.** A connector may only add loss there. An offset or tilt between fibers would redistribute power between modes (mode coupling), which is not modeled, so the engine reports an error.

**Numbers.** 1.000 mW in, 1.000 mW out.

**Look at it.**
```python
g.params("el0")                     # {'loss_db': 0.0, 'offset': 0.0, 'tilt': 0.0}
g.set("el0", offset=5e-6)           # 5 µm offset: try it, then reset
g.set("el0", offset=0.0)
```

**Not modeled.**
- Butt coupling only: no Fresnel reflection, air gap or lens.
- Tilt and offset act in one plane.
- Fiber-to-fiber offsets are not allowed (above).

More: [`model/connector.md`](model/connector.md).

---

## 5. `el1`: the multimode fiber (100 m OM4)

**What it does.** The fiber carries the light in many guided **modes**. A graded-index core (refractive index falling almost parabolically from the center) makes them arrive at nearly the same time, but not exactly. The remaining differential mode delay (DMD) smears the symbols into each other: intersymbol interference. Each mode also spreads by chromatic dispersion, because the laser emits a range of wavelengths. The glass attenuates the light a little.

**In Lightstone.** The node is the `mmf` preset of type `om4`:

- **Geometry.** 25 µm core radius, NA 0.20, power-law index profile with α = 2.0777.
- **Material.** A germania-doped core in fused-silica cladding, both with Sellmeier dispersion, so the index difference changes with wavelength (profile dispersion).
- **Loss.** 3.5 dB/km, the cabled limit. 100 m therefore lose 0.35 dB.
- **Calibration.** It is a *minimum-spec* OM4: its effective modal bandwidth with the default launch is at the OM4 minimum, 4700 MHz·km.

On its first run the node does three things:

1. **Mode solver.** It solves the scalar mode equation for all guided LP modes by finite volumes, 600 points per core radius. For every mode it computes the group delay and the chromatic delay slope from five neighboring wavelengths. The result is cached: other spans with the same geometry, and later runs, reuse it.
2. **Launch.** It computes the overlap of the VCSEL near field (with the connector's offset and tilt) with every mode. This gives the launched power per mode.
3. **Propagation.** It adds this span's delay and delay slope to every mode, relative to LP01, and applies the attenuation.

The fiber is **linear in power**. VCSEL light is incoherent over the delay differences between modes (coherence time ≈ 1.4 ps, delay spread ≈ 5 ps), so the modes add as powers at the detector and do not interfere.

**Signal model.**
- **Input.** The free-space contribution from the connector.
- **Output.** The same contribution, now *guided*. It holds a reference to the mode set and one **mode bin** per LP mode (90 bins at the default resolution `mode`). Each bin has a power weight, an accumulated delay and an accumulated delay slope.
- **No waveform yet.** The fiber is just this table. Its effect on the waveform is evaluated only at the photodiode, as one transfer function:

$$H(f) = \sum_m w_m\,e^{-j2\pi f(\tau_m - \bar\tau)}\,e^{-2\pi^2 f^2\sigma_m^2},\qquad \sigma_m = \Big|\frac{d\tau_m}{d\lambda}\Big|\,\sigma_\lambda$$

A second fiber span would continue the same mode bins: it adds delays and losses and requires the same geometry.

**Numbers.**

| Quantity | Value |
|---|---|
| V-number | 37.0 |
| Guided LP modes / spatial modes | 90 / 171 |
| Launched power that is guided | 99.999 % |
| Power in mode groups 1–6 | 90 % (centered beam) |
| Relative delays of groups 1–8 after 100 m | 0 … 15 ps (higher groups arrive later: over-compensated profile) |
| EMB, this launch | 4710 MHz·km (OM4 minimum 4700) |
| OFL bandwidth | 1454 MHz·km (see the note below) |
| Transmission | 0.9226 (0.35 dB) |
| Output power | 0.923 mW (−0.35 dBm) |

The OFL bandwidth is below the nominal OM4 value (3500 MHz·km) on purpose. The preset is calibrated to the EMB with the laser launch, which is what laser links depend on. A fiber cannot sit at both minimums in this model ([`model/mmf.md`](model/mmf.md)).

**Look at it.**
```python
f = g.report("el1")
f["modes"], f["v_number"]                       # 90, 36.96
f["emb_length"] / 1e9, f["ofl_bandwidth_length"] / 1e9   # 4710, 1454 MHz·km
g.output("el1")["average_power"]                # 0.000923 W
modes = lightstone.simulate().modes             # per mode: (l, m), group, n_eff, delay, slope, launched power
g.set("el1", type="om3")                        # switch the preset (OM3 at 100 m: try the SER)
g.set("el1", type="om4")
```

In the browser, selecting the fiber shows the facet with the launch beam, the guided intensity, the mode delays and the transfer function.

**Not modeled.**
- Scalar LP modes only: no polarization or vector effects.
- No mode coupling along the fiber, no bends, and the same loss for every mode.
- The index profile is an ideal power law: no central dip or trench.
- Chromatic dispersion of the launched modes is ≈ 6 % below the IEEE 802.3 value, which makes TDECQ at 100 m 0.1–0.3 dB optimistic (V14b).
- The preset is calibrated, not measured DMD data.

More: [`model/mmf.md`](model/mmf.md) (the presets) and [`model/custom-fiber.md`](model/custom-fiber.md) (the fiber model).

---

## 6. `pin`: the photodiode

**What it does.** A PIN photodiode converts optical power into current, I = R · P, with responsivity R ≈ 0.5 A/W at 850 nm. Because the photons arrive at random, the current has shot noise.

**In Lightstone.**
- Square-law detection in the incoherent limit: the detected power is the sum of the mode powers.
- All light is collected by default (`detector_radius: null`, a lens-coupled receiver). With a finite radius, each mode contributes the fraction of its power that falls on the detector. If significantly excited modes are then partially coherent, a warning appears, because modal noise is not modeled.
- Shot noise with strength 2q · I(t), generated with the receiver's seed.

**Signal model.** This is where the optical description becomes a waveform description:

1. For each contribution, the photodiode builds the fiber transfer function H(f) from the mode bins (the formula above). It multiplies the source's power spectrum by it and sums all contributions. The result is the spectrum of the detected power.
2. The output is an **electrical** signal of quantity *current*: base spectrum = detected power, gain = R.
3. It carries these noise terms:
   - **RIN**: the source's realization, now with filter H(f) and gain R; strength RIN · P_tx(t)².
   - **Shot**: generated here, strength 2qR · P_det(t). It needs the detected power waveform, so the photodiode computes it once.
   - **Jitter**, if the driver has any: like RIN, with the drive's filter × VCSEL × fiber.

From here on, every component only edits this description.

**Numbers.**

| Quantity | Value |
|---|---|
| Detected power | 0.923 mW (−0.35 dBm) |
| Mean photocurrent | 461 µA (254 … 668 µA) |
| RMS delay spread of the channel | 5.14 ps |
| Mean delay | 493.7 ns (100 m at group index 1.48) |
| Fiber (optical) −3 dB bandwidth | 33.0 GHz |

**Look at it.**
```python
p = g.report("pin")
p["channels"][0]                    # transmission, mean_delay, rms_delay_spread
p["p_det"]                          # detected power waveform [W]
i = g.output("pin")                 # current: noiseless, signal, sigma
```

Before the receive filter, σ(t) contains noise up to f_s/2 = 425 GHz. It therefore depends on the oversampling and only becomes meaningful after `rxf`.

**Not modeled.** A GaAs detector's cutoff above ≈ 870 nm (the responsivity scales with λ, constant quantum efficiency); no transit-time or RC bandwidth of the diode itself (the receive filter stands for the whole receiver bandwidth); no dark current.

More: [`model/pin.md`](model/pin.md).

---

## 7. `tia`: the transimpedance amplifier

**What it does.** The TIA converts the small photocurrent into a voltage, V = Z · I. Its input transistor adds thermal (Johnson) noise, usually given as an input-referred current noise density.

**In Lightstone.**
- Transimpedance Z = 1000 Ω.
- Input-referred thermal noise density i_n = 15 pA/√Hz, white, generated with the receiver's seed.
- Optional output clipping (`clip: [low, high]`), off by default. With clipping the TIA becomes nonlinear and the SER must be counted ([`tutorial-component-graph.md`](tutorial-component-graph.md) Section 7.4).

**Signal model.** The TIA multiplies the gain of the signal and of every noise term by Z, and switches the quantity to *voltage*. It adds one **thermal** term: constant strength i_n², gain Z. No waveform is computed. Without clipping the TIA is linear and `gaussian_exact` stays true.

**Numbers.** Mean output 461 mV (254 … 668 mV). As at the photodiode, σ before the receive filter (14.7 mV RMS) still includes noise up to 425 GHz.

**Look at it.**
```python
g.params("tia")                     # transimpedance, noise_density, thermal_noise, noise_seed
v = g.output("tia")["noiseless"]
```

**Not modeled.** Only white noise; real TIAs have noise that rises with frequency. No soft compression (only hard clipping) and no offset.

More: [`model/tia.md`](model/tia.md).

---

## 8. `rxf`: the receive filter (Bessel–Thomson)

**What it does.** The receiver's bandwidth limits how fast the signal can change, and how much noise gets through. IEEE 802.3 defines the reference receiver for these links as a 4th-order Bessel–Thomson response at half the symbol rate. Lightstone uses the same filter to stand for the bandwidth of the whole receiver.

**In Lightstone.**
- H_BT(f) = 105 / (s⁴ + 10s³ + 45s² + 105s + 105), with s scaled so that f_3dB = 0.5 × baud = 26.6 GHz (`f3db_rel`, `eq:rx-bessel-thomson`).
- The node reports the noise bandwidth B_n = ∫|H|² df = 27.8 GHz. White noise of PSD S has variance S · B_n after the filter.

**Signal model.** The filter multiplies H_BT into the signal's filter **and** into the filter of every noise term. Terms that shared a filter keep sharing it, so the engine can combine their realizations. Nothing else changes. This is the general rule for any linear electrical component, including custom ones.

**Numbers.** After the filter the noise is final, apart from the FFE. Its RMS over the record is 7.26 mV, made up of:

| Source | σ (RMS) | Share of the variance |
|---|---|---|
| RIN | 6.51 mV | 80 % |
| Thermal | 2.50 mV (= Z · i_n · √B_n) | 12 % |
| Shot | 2.03 mV | 8 % |
| Total | 7.26 mV | |

σ(t) varies between 4.5 and 9.8 mV along the waveform, because RIN and shot noise follow the signal level.

The level spacing is about 96 mV, far larger than the noise. Yet before equalization the eye is **closed**: the eye heights at the 10⁻³ contour are −43, −45 and −53 mV, and the SER is 0.0995. The fiber's intersymbol interference, not noise, closes the eye at 100 m.

**Look at it.**
```python
g.report("rxf")["noise_bandwidth"] / 1e9        # 27.8 GHz
y = g.output("rxf")                             # the waveform a reference receiver sees
g.add("pre", "pam4_analyzer").wire("rxf", "pre").wire("src", "pre.ref")
g.run(); g.report("pre")["ser"]                 # 0.0995 before the FFE
g.remove("pre")
```

**Not modeled.** An ideal filter shape. A real receiver's frequency response would replace it (for example with a custom component, [`tutorial-component-graph.md`](tutorial-component-graph.md) Section 7).

More: [`model/bessel-thomson.md`](model/bessel-thomson.md).

---

## 9. `ffe`: the receiver equalizer

**What it does.** A feed-forward equalizer (FFE) is a short filter with taps one symbol apart. It subtracts scaled copies of the neighboring symbols from each symbol and so undoes most of the intersymbol interference. 100 m PAM-4 links rely on it. The IEEE TDECQ measurement also includes a 5-tap FFE in its reference receiver.

**In Lightstone.**
- 5 taps, main cursor in the middle (`n_taps`; the lane setting `rx.ffe_taps`).
- **Adaptation.** The taps are chosen once by constrained minimum mean-square error (`eq:ffe-mmse`). They minimize the distance of the equalized samples to the ideal PAM-4 levels, plus the noise the taps amplify (`noise_aware`, default true), with the taps summing to 1.
  - It uses the noiseless waveform and the transmitted symbols (input `ref`), like an ideal training sequence.
  - It searches 32 sampling phases and takes the center of the best range.
- After adaptation the taps are **fixed**. The FFE is then a linear filter, H_FFE(f) = Σ c_j e^(−j2πf(j−m)T).

**Signal model.**
- **Inputs.** `in` (electrical) and `ref` (the symbols, a monitor input).
- **During adaptation.** The FFE materializes the noiseless input and computes the noise autocorrelation from the noise terms.
- **Output.** Like the receive filter, it multiplies H_FFE into the signal and every noise term. σ(t) therefore stays exact, and the SER stays semi-analytic.

**Numbers.**

| Quantity | Value |
|---|---|
| Taps | 0.067, −0.344, 1.495, −0.287, 0.069 |
| Design phase | 0.53 UI |
| Residual mean-square error | 1.2 × 10⁻⁴ V² (ISI + noise) |
| Noise after the FFE | 10.5 mV RMS (7.26 mV before: noise enhancement × 1.45) |

The main tap above 1 and the negative neighbors subtract the energy that leaked into the adjacent symbols. The price is more noise, but the eye opens far more than the noise grows (Section 11).

**Look at it.**
```python
f = g.report("ffe")
f["taps"], f["phase"], f["mse"]
g.set("ffe", n_taps=9); g.run()          # more taps: SER 9.1e-08
g.set("ffe", n_taps=5); g.run()
lightstone.Graph.reference(lane_config={"rx": {"ffe_taps": 0}})   # the lane without an FFE
```

**Not modeled.**
- Ideal training: the taps are designed on the noiseless waveform.
- No adaptation dynamics and no decision-feedback equalizer (DFE).
- Clock recovery is ideal: the analyzer picks the best sampling phase.

More: [`model/ffe.md`](model/ffe.md).

---

## 10. `rx`: the receiver output

**What it does.** This is the end of the analog receiver: the point where a real receiver would sample the signal and decide on symbols.

**In Lightstone.**
- The receiver output makes **no decision**: it does not slice and outputs no symbols.
- It is where the lazy description becomes explicit waveforms (`eq:materialize`):
  - the **noiseless** waveform, the inverse FFT of base × filter × gain;
  - one **noisy** realization, with every noise term drawn and filtered;
  - **σ(t)** from the noise terms;
  - `gaussian_exact`.
- If the noise has passed a nonlinear block, it warns that the semi-analytic SER is not valid.
- Thresholds, sampling phase and errors are computed by the analysis (Section 11).

**Signal model.** Input `in` is a monitor input (it only observes). The node has no outputs. Its result is a report holding the three waveforms of 131 056 samples. `Graph.receiver()` collects it together with the transmitted symbols.

**Numbers.** Mean 461 mV, σ 10.5 mV RMS, Gaussian and exact.

**Look at it.**
```python
rx = g.receiver()                   # the only receiver_output in the graph
rx.noiseless, rx.signal, rx.sigma   # numpy arrays
a = rx.analyze()                    # PAM-4 metrics, as the analyzer node below
```

More: [`model/receiver-output.md`](model/receiver-output.md).

---

## 11. Measuring the result: analyzer, eye, TDECQ

The reference graph ends at `rx`. The measurements are separate **analysis nodes**, which you add like instruments. Each observes a signal (`in`) and the transmitted symbols (`ref`), and changes nothing:

```python
g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")
g.add("eye", "eye_diagram").wire("ffe", "eye").wire("src", "eye.ref")
g.add("tq", "tdecq").wire("el1", "tq").wire("src", "tq.ref")      # optical: the fiber output
g.run()
```

### 11.1 `pam4_analyzer`: SER, levels, eye

How it works:

1. It aligns the waveform with the symbols by cross-correlation.
2. It searches the best sampling phase in steps of 1/64 UI.
3. It sets the thresholds halfway between the level means.
4. It computes the **semi-analytic SER**: for each of the 8191 noiseless samples y_n with its own σ_n, the Gaussian probability of falling beyond a threshold (`eq:pam4-ser-semi-analytic`):

$$\mathrm{SER} = \frac1N\sum_n\left[Q\!\left(\frac{y_n - \theta_{k_n-1}}{\sigma_n}\right) + Q\!\left(\frac{\theta_{k_n} - y_n}{\sigma_n}\right)\right]$$

This includes the ISI exactly and reaches SERs far below 1/8191. The eye heights and widths are measured on the noisy realization, at the 10⁻³ probability contour.

| At the FFE output | Value |
|---|---|
| SER / BER | 1.9 × 10⁻⁷ / 9.3 × 10⁻⁸ |
| Level means | 264.5, 395.7, 526.9, 658.0 mV |
| Thresholds | 330.1, 461.3, 592.5 mV |
| Eye heights (10⁻³) | 78.1, 70.8, 61.3 mV |
| Eye widths | 0.23, 0.22, 0.22 UI |
| Worst level-pair Q | 5.35 |

For comparison, at the receive filter output (before the FFE): SER 0.0995, eye heights −43 to −53 mV, Q 1.5.

### 11.2 `eye_diagram`

It shows a 2-UI density of the noisy waveform at the analyzer's sampling phase (600 × 400 histogram bins). In the browser the node draws it in scope colors.

### 11.3 `tdecq`: the IEEE transmitter metric

TDECQ (transmitter and dispersion eye closure, quaternary) asks how much Gaussian noise could be added to the optical signal before the SER reaches 4.8 × 10⁻⁴, compared with an ideal transmitter. It works on the **optical** signal at the fiber output:

1. It collects all light.
2. It applies the transmitter noise (RIN, and jitter if any) but no receiver noise.
3. It filters with the reference receiver (Bessel–Thomson, 0.5 × baud).
4. It equalizes with its own 5-tap reference FFE.
5. It measures in two time windows at fixed thresholds.

| Quantity | Value |
|---|---|
| TDECQ | 3.26 dB |
| C_eq (noise enhancement of the reference FFE) | 1.87 dB |
| OMA_outer at the fiber output | 0.788 mW |
| Reference FFE taps | 0.062, −0.323, 1.485, −0.299, 0.076 |

The reference FFE taps are close to the link FFE's taps, as they should be: both solve almost the same problem.

More: [`model/pam4-analysis.md`](model/pam4-analysis.md), [`model/tdecq.md`](model/tdecq.md).

---

## 12. The whole chain in one line

Everything from the drive to the receiver output is linear, apart from the VCSEL's 0 W clip, which never acts. The engine therefore never builds the per-mode waveforms. The receiver waveform is one inverse FFT of one product:

$$y(t) = \mathcal{F}^{-1}\Big\{\hat x_\text{drive}(f)\cdot (P_3 - P_0)\,H_V(f)\cdot \underbrace{\sum_m w_m\,e^{-j2\pi f\tau_m}e^{-2\pi^2f^2\sigma_m^2}}_{\text{fiber, 90 mode bins}}\cdot R\cdot Z\cdot H_\text{BT}(f)\cdot H_\text{FFE}(f)\Big\} + \text{DC}$$

Every noise term follows the same chain from its point of origin, and σ(t) is computed exactly from it.

**Where the time goes.** The first run takes ≈ 0.4 s, almost all of it the mode solver and launch. Changing the fiber length reruns the fiber and everything after it (≈ 0.1 s). Changing the FFE reruns only `ffe`, `rx` and any analyzer behind them.

---

## 13. What the reference link does not model (summary)

| Area | Not modeled | Where it matters |
|---|---|---|
| Transmitter | VCSEL nonlinearity and chirp, RIN spectrum shape, current-dependent transverse modes, driver bandwidth | High drive, long links with strong dispersion |
| Launch | Fresnel reflections, lenses, fiber-to-fiber offsets (mode coupling) | Patch cords, multiple connectors |
| Fiber | Mode coupling, bends, mode-dependent loss, measured DMD profiles; dispersion ≈ 6 % low | Real installed fiber, worst-case budgets |
| Detection | Modal noise with a partial detector, diode bandwidth | Bare detectors close to the fiber |
| Receiver | Frequency-dependent TIA noise, real filter shape, soft compression | Detailed receiver design |
| Equalizer | Adaptation from noisy data, DFE | Comparison with real silicon |
| Clock | Clock recovery is ideal (jitter can be added at the driver and the analyzer) | Jitter tolerance tests |

Validations V1–V16 show where the model has been checked against analytic results, the NumPy reference implementation and the IEEE 802.3 link models (docs/validation.md, Section 1).

---

## 14. Try it

```python
g = lightstone.Graph.reference()
g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")
for L in (50, 100, 150):
    g.set("el1", length=L).run()
    print(L, g.report("ana")["ser"])                # 8.3e-08, 1.9e-07, 5.2e-06
g.set("el1", length=100).set("el0", offset=8e-6).run()
print(g.report("el1")["emb_length"] / 1e9)          # 3073 MHz·km: an 8 µm offset fills higher modes
g.set("el0", offset=0.0).set("drv", rj_rms=0.4e-12).run()
print(g.report("ana")["ser"])                       # 5.7e-07 with 0.4 ps transmitter RJ
```

In the browser the same graph opens with **Reference link**. Select any node to see its parameters, its outputs and its model document; move a slider and only the affected nodes recompute.

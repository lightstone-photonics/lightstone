# Lightstone – Validation

What has been checked, against what, and how well. Every test runs in CI: the Rust tests with `cargo test -p lightstone-core -p lightstone-analysis --release`, the Python tests with `pytest tests/python` (after `maturin develop` in `crates/lightstone-py`). Equations and the validation of each component are also in its model document in [`model/`](model/). The architecture is described in [`architecture.md`](architecture.md).

The references are analytic results, an independent NumPy implementation (`reference/lightstone_ref`), and public IEEE 802.3 material: the 10G Ethernet link model spreadsheet (802.3ae), the fiber emulation of 802.3db, the effective-bandwidth table of 802.3cm, and the TDECQ method of 802.3bs/cd. Lightstone is *compared with* these; it is not certified or endorsed by IEEE or TIA.

---

## 1. Validation suite

| ID | Component | Test | Reference | Tolerance |
|---|---|---|---|---|
| V1 | Mode solver | Step-index fiber, V = 4.5 | Analytical LP eigenvalue equation (Bessel) | Δn_eff < 10⁻⁶ |
| V2 | Mode solver | Near-infinite parabolic profile | Analytical mode-group spacing and degeneracy | Δn_eff < 10⁻⁵ |
| V3 | Mode solver | Orthonormality | ∬ ψ_m ψ_n* = δ_mn | < 10⁻⁶ |
| V4 | Coupling | Coaxial Gaussian on parabolic-fiber fundamental mode | Power efficiency (2w₀w₁/(w₀² + w₁²))² | < 0.1 % |
| V5 | Coupling | Power conservation | Σ\|c_m\|² ≤ 1; equals 1 for a fully guided input | < 10⁻⁶ |
| V6 | Coupling | Offset +Δx vs. −Δx | Identical \|c_m\|² | < 10⁻¹² of the largest mode power |
| V7 | Propagation | Two-mode link with known Δτ | Double peak separation | < 0.1 ps |
| V8 | Propagation | OFL bandwidth and EMB of the default OM4 model | OM4 spec: OFL ≥ 3500, EMB ≥ 4700 MHz·km | Both met |
| V9 | Receiver | Zero optical input | σ² = i_n² · B_n | < 1 % |
| V10 | Receiver | Constant optical input | Shot noise σ² = 2qIB_n added | < 1 % |
| V11 | Analysis | Noise-free back-to-back PAM-4 | Analytical EH, SER = 0 | Exact levels |
| V12 | Analysis | Back-to-back with known Gaussian noise | Analytical PAM-4 SER | < 5 % relative |
| V13 | Analysis | TDECQ of an ideal signal | ≈ 0 dB | < 0.1 dB |
| V14a | System | NRZ eye closure (VECP) vs. length (0–300 m), 10.3125 GBd, OM3-class fiber, two index profiles | IEEE 802.3ae 10G link model (`10GEPBud3_1_16a.xls`), given Lightstone's component values | ≤ 0.5 dB |
| V14b | System | PAM-4 TDECQ at 53.125 GBd over worst-case OM4 (50 and 100 m, two index profiles) | IEEE 802.3db fiber emulation (Table 167-12 bandwidths and their derivation) | ≤ 0.5 dB |
| V14c | System | NRZ eye closure with DCD (5 and 10 ps), 0–300 m, as V14a | Gaussian eye model of the 10G link model with the physical pulse widths T ± DCD | ≤ 0.5 dB; DCD increment ≤ 0.02 dB |
| V16a | Transmitter | Random-jitter slope noise σ(t), RJ 0.3 ps at 53.125 GBd, with and without DCD | Monte Carlo of exact random edge shifts | < 3 % per phase |
| V16b | System | NRZ bathtub with RJ and DCD (fast link) | Dual-Dirac model, TJ = DJ + 2·Q·σ_RJ | eye width within 0.5 σ_RJ, 10⁻³ … 10⁻¹² |
| V17a | WDM | SWDM4 (4 VCSELs, mux, 100 m OM5, demux) with ideal filters | Each single-wavelength link | < 10⁻⁹ (waveforms, σ, realization) |
| V17b | WDM | Crosstalk at a demux port (isolation 15 dB) | Σⱼ T(λⱼ)·yⱼ of the single-wavelength waveforms | < 10⁻⁹ |
| V19 | Analysis | TDECQ: ISI-free PAM-4 with Gaussian noise; reference link 0–150 m, jitter | Analytic TDECQ; independent implementation of the IEEE 802.3 method (`reference/lightstone_ref`) | Analytic < 0.02 dB; same mechanics < 0.02 dB; vs. full method −0.02 … +0.15 dB |
| V18 | Fiber | OM3/OM4 effective bandwidth 844–918 nm (presets, modes at each λ, same dispersion term) | IEEE 802.3cm table (TIA worst-case EMB + 10G-model dispersion) | 850 nm within 1 %; gap pinned: OM4 +20…28 %, OM3 +6…12 % at 918 nm (documented, not a pass criterion) |
| S1/S2 | Analysis | Electrical / optical spectrum analyzer | Parseval (signal variance, mean σ²), Bessel–Thomson −3 dB point; OSA total power and line shape | 10⁻⁹ / 10⁻³ |
| M1 | Measured data | Simulated reference-receiver waveform resampled like a scope capture (23 / 40 samples per UI, 1–3 periods, CSV / `.npy`) | Its own TDECQ | < 0.01 dB (whole periods), < 0.05 dB (1.3 periods) |
| V15 | Cross-impl. | Rust vs. NumPy reference, all stages | Identical outputs | n_eff < 10⁻¹²; delays 10⁻⁶ of spread; waveforms < 10⁻⁷ relative |

## 2. V14a: NRZ against the IEEE 802.3ae link model

**Reference.** The 10G Ethernet link model spreadsheet `10GEPBud3_1_16a.xls` (Hanson, Cunningham, Dawe, Dolfi; aligned to 802.3ae D3.2/3.3), public at the [802.3ae serial PMD ad hoc](https://www.ieee802.org/3/ae/public/adhoc/serial_pmd/documents/10GEPBud3_1_16a.xls). It is the only IEEE 802.3 link model spreadsheet that is public. The later PAM-4 multimode models (802.3cd/cm/db) were private tools (e.g. a compiled MATLAB model, [castro_3cm_02a_1118](https://www.ieee802.org/3/cm/public/November18/castro_3cm_02a_1118.pdf)); their comparison is V14b.

**What is compared.** The link model's ISI penalty P_isi, which its notes define as the vertical eye closure penalty (VECP): the closure of the worst-case NRZ eye at its centre, relative to the full swing, without noise. The model treats every element as a Gaussian with a 10–90 % rise time (T = 480 ns·MHz / B), adds them in quadrature to T_c, and evaluates the eye as 2·erf(T_b/(2√2 σ)) − 1 with σ = T_c/2.563. Noise penalties (RIN, MPN, modal and reflection noise) and their interaction terms are not part of V14a.

**How.**
1. `scripts/extract_ieee_10g_link_model.py` copies the computed cells of the five 850 nm sheets into `tests/python/data/ieee_10g_link_model.json`. `tests/python/ieee_link_model.py` re-implements the equations. They reproduce all 110 rows: T_e and T_c to 10⁻⁷, P_isi to 2 × 10⁻³ dB (the sheet's σ factor is 2.563 to four digits).
2. Lightstone simulates a 10GBASE-SR-like link: NRZ (PRBS13), 10.3125 GBd, 840 nm, 0.29 nm RMS spectral width, a critically damped 10 GHz VCSEL, no noise, a 4th-order Bessel–Thomson receiver at 8.25 GHz. The fiber is OM3-class: two graded-index profiles on either side of the optimum (α = 1.9708 and 2.1256, germania-doped core) whose computed EMB with the VCSEL launch is 2071 and 2070 MHz·km.
3. The link model gets Lightstone's own component values: the back-to-back 10–90 % rise time measured on the simulated waveform (67.9 ps, VCSEL and receiver together), the computed EMB, the launch-weighted RMS chromatic dispersion of the guided modes (97 ps/(nm·km)) and the source width. There is no duty-cycle distortion on either side.
4. Lightstone's VECP is the worst-case eye at the best sampling instant over the settled swing.

**Result** (16 samples/UI; `pytest -s tests/python/test_v14_ieee_link_model.py` prints the table):

| Length | VECP α = 1.9708 | VECP α = 2.1256 | Link model | Δ (α = 1.9708 / 2.1256) |
|---|---|---|---|---|
| 0 m | 0.575 dB | 0.575 dB | 0.627 dB | −0.05 / −0.05 dB |
| 100 m | 0.795 dB | 0.801 dB | 0.808 dB | −0.01 / −0.01 dB |
| 200 m | 1.435 dB | 1.502 dB | 1.387 / 1.386 dB | +0.05 / +0.12 dB |
| 300 m | 2.460 dB | 2.584 dB | 2.398 dB | +0.06 / +0.19 dB |

Both profiles stay within 0.2 dB of the link model up to 300 m (target ≤ 0.5 dB). The remaining differences are expected:
- At 0 m the Gaussian approximation of the VCSEL and receiver responses overestimates the closure by 0.05 dB.
- At long lengths the real modal impulse response is not Gaussian. Two fibers with the same EMB give different eyes (0.12 dB apart at 300 m), which the link model cannot distinguish by construction.
- Lightstone's launched modes have D ≈ −97 ps/(nm·km) at 840 nm (germania-doped core). The sheets use the 802.3ae worst-case fiber formula (−117.8 ps/(nm·km)). V14a gives the link model Lightstone's value; with the sheet's value the link model would show ≈ 0.08 dB more closure at 300 m.
- Numbers since v0.12 (germania-doped core, profiles re-chosen for the same EMB). With the v0.11 pure-silica model the differences to the link model were within 0.01 dB of these (max. Δ 0.18 dB).

For reference, the published OM3 sheet (`850S2000`, with 6 ps duty-cycle distortion and its own rise times) gives P_isi = 0.75 dB at 2 m and 3.02 dB at 300 m.

## 3. V14b: PAM-4 TDECQ against the IEEE 802.3db fiber emulation

**Reference.** No PAM-4 multimode link model is public. What 802.3db (100 Gb/s per lane over MMF) publishes is how the worst-case OM4 channel enters the transmitter test: TDECQ is measured through a **fiber emulation filter** of bandwidth f_A, 18.0 GHz for 100 m and 33.6 GHz for 50 m (Table 167-12, quoted in [murty_3db_01a_092321](https://www.ieee802.org/3/db/public/September-09-September-29-2021/murty_3db_01a_092321.pdf)). The baseline proposal ([murty_3db_adhoc_01a_121720](https://www.ieee802.org/3/db/public/adhoc/presentations/murty_3db_adhoc_01a_121720.pdf)) derives the filter with the 10GE link model method ([king_3cm_adhoc_01_062818](https://www.ieee802.org/3/cm/public/adhoc/king_3cm_adhoc_01_062818.pdf)):
- a Gaussian fiber with BW_eff = (EMB⁻² + BW_cd⁻²)^(−½);
- BW_cd = 1.87 × 10⁵/Δλ / √(D² + E²), D = λ/4 · S₀ (1 − (U₀/λ)⁴), E = 0.7 S₀ Δλ;
- EMB = 4700 MHz·km, U₀ = 1328 nm, S₀ = 0.093477 ps/(nm²·km);
- worst-case sources 844 nm / 0.6 nm RMS (100 m) and 842 nm / 0.65 nm (50 m).

**How.**
1. The re-implemented method reproduces the published numbers: f_A = 18.22 / 34.18 GHz (published 18.0 / 33.6) and the fiber + 26.5625 GHz Bessel–Thomson reference responses 15.16 / 21.19 GHz (published 15.0 / 21.0), within 1.7 %.
2. Lightstone simulates PAM-4 at 53.125 GBd with the default VCSEL (RIN off) over OM4-class fiber with a germania-doped core on both sides of the optimum (α = 2.0119 and 2.0802, EMB 4616–4790 MHz·km) at the worst-case wavelength and width, and computes TDECQ with the same implementation for every case (5-tap reference FFE; 802.3db uses 9 taps).
3. Three comparisons:
   - **Shape:** Lightstone's fiber vs. a Gaussian filter with the same −3 dB bandwidth.
   - **Worst case, matched:** Lightstone vs. the Table 167-12 filter, with the source width scaled so that D·Δλ equals the standard's (the chromatic term depends only on this product).
   - **Worst case, unmatched:** the same with Lightstone's own dispersion.

**Result** (back-to-back TDECQ 1.10 dB; `pytest -s tests/python/test_v14b_pam4_link_model.py` prints the table):

| Profile, length | Fiber f_3dB | TDECQ: Lightstone / same-bandwidth Gaussian | TDECQ matched / Table 167-12 | Unmatched Δ |
|---|---|---|---|---|
| α = 2.0119, 50 m | 35.5 GHz | 1.641 / 1.638 dB | 1.690 / 1.706 dB (Δ −0.02) | −0.07 dB |
| α = 2.0119, 100 m | 19.0 GHz | 3.239 / 3.243 dB | 3.457 / 3.543 dB (Δ −0.09) | −0.30 dB |
| α = 2.0802, 50 m | 34.3 GHz | 1.687 / 1.679 dB | 1.736 / 1.706 dB (Δ +0.03) | −0.02 dB |
| α = 2.0802, 100 m | 18.0 GHz | 3.455 / 3.535 dB | 3.690 / 3.543 dB (Δ +0.15) | −0.09 dB |

Findings:
- **The standard's Gaussian fiber emulation is a faithful stand-in** for a mode-resolved OM4 channel at 53 GBd: within 0.08 dB of TDECQ.
- **With matched dispersion, Lightstone agrees with 802.3db within 0.15 dB** (target ≤ 0.5 dB). The measured TDECQ of a 100 m SR transmitter in [ghiasi_802.3db_01_092321](https://www.ieee802.org/3/db/public/September-09-September-29-2021/ghiasi_802.3db_01_092321.pdf) (about 3.5–4.3 dB, real VCSEL, 9 taps) is in the same range; this is not a quantitative check, since the transmitter differs.
- **Dispersion gap, found by V14b and mostly closed in v0.12.** With the pure-silica fiber of v0.11 the launched modes had D ≈ −87 ps/(nm·km) at 844 nm against the 802.3 MMF value of −101, and the 100 m TDECQ was 0.4–0.6 dB optimistic. v0.12 models the core as germania-doped silica (Fleming 1984, `docs/model/custom-fiber.md`): D ≈ −95.5 ps/(nm·km), and the unmatched gap at 100 m is now 0.09–0.30 dB. The remaining 6 % may come from real fibers' higher or non-uniform GeO₂ content.

---

## 4. Results by test

| ID | Where | Result |
|---|---|---|
| V1 | `tests/python` | Pass. Δn_eff ≤ 5.5 × 10⁻⁹ (LP01/11/21/02) |
| V2 | `lightstone-core` modes | Pass (groups 1–4 < 10⁻⁵; group degeneracy ⌈g/2⌉) |
| V3 | `lightstone-core` modes | Pass |
| V4 | `lightstone-core` coupling | Pass. Max. relative error 3.6 × 10⁻⁵ |
| V5, V6 | `lightstone-core` coupling | Pass |
| V7 | `lightstone-core` link | Pass |
| V8 | `lightstone-core` link | Pass: the OM4 preset's EMB with the default launch is 4710 MHz·km (minimum 4700; v0.13). Earlier defaults: v0.12 OFL 3966 / EMB 6850, v0.11 3590 / 6565 MHz·km |
| V9, V10 | `lightstone-core` rx | Pass (< 1 %) |
| V11, V12 | `lightstone-analysis` pam4 | Pass |
| V13 | `lightstone-analysis` tdecq | Pass (< 0.1 dB) |
| V19 | `tests/python/test_v19_tdecq.py` | Pass. No public IEEE TDECQ code or waveforms exist; the oracle is an independent NumPy implementation of the standard's text (symbol-agnostic histograms, crossing-time center with histogram shift, taps optimized for minimum TDECQ, OMA_outer per 121.8.4). Analytic ISI-free cases within 0.02 dB for both; Lightstone is 0.02–0.09 dB conservative on the reference link (0–150 m, jitter). V19 found and fixed three deviations: OMA_outer from mid-symbol settled runs (now 121.8.4, was 0.10–0.13 dB low), a 0.031 UI histogram window sampled at 3 points (now 0.04 UI, 16 points), and an eye center taken only from the MMSE optimum (now also the crossing-time center; a flat eye with symbol-constant noise read 0.5 dB high) (`docs/model/tdecq.md`) |
| V14a | `tests/python/test_v14_ieee_link_model.py` | Pass (v0.11). The re-implemented link model equations reproduce the five 850 nm sheets (T_e, T_c to 10⁻⁷, P_isi to 2 × 10⁻³ dB). Lightstone vs. the link model at 0–300 m: max. \|Δ\| 0.06 dB (α = 1.917) and 0.18 dB (α = 2.07), see Section 2 |
| V14b | `tests/python/test_v14b_pam4_link_model.py` | Pass (v0.12). The 802.3db method reproduces Table 167-12 (18.0 / 33.6 GHz) and the baseline reference responses (15.0 / 21.0 GHz) within 1.7 %. Lightstone's mode-resolved fiber and a Gaussian of the same bandwidth give the same TDECQ within 0.08 dB. Worst case vs. Table 167-12: within 0.15 dB with the chromatic dispersion matched; 0.09–0.30 dB less TDECQ at 100 m with Lightstone's own dispersion (germania-doped core; pure silica: 0.4–0.6 dB) (Section 3) |
| V15 | `tests/python` | Pass, with the tolerances in Section 1. The original 10⁻⁹ target was not reachable: finite-difference derivatives of n_eff(λ) amplify the ~10⁻¹³ eigenvalue differences between the two solvers |

---

## 5. Known limitations

- TDECQ uses MMSE taps and symbol-aware histograms; V19 sizes the deviations against an independent implementation of the IEEE method (0.02–0.09 dB conservative). Not compared with a commercial scope or measured TDECQ.
- α is a calibrated parameter, not a measured profile. Profile dispersion is modeled since v0.12 (germania-doped core).
- PRBS13Q seed and bit order have not been verified bit-for-bit against Clause 120.5.11.2.1.
- SER, EH and EW are for an unequalized slicer. At ≥ 100 m the default link needs equalization, which matches practice for 100G/lane SR.
- Fiber presets are single fibers with ideal power-law profiles, calibrated at 850 nm. Away from 850 nm they are optimistic against the TIA worst-case bandwidth (V18: OM4 +24 % at 918 nm), so WDM results at 880–940 nm over OM3/OM4 are optimistic. Import of measured DMD data is planned.
- Chromatic dispersion of the launched modes is about 6 % below the IEEE 802.3 MMF value (V14b), which makes the 100 m TDECQ 0.1–0.3 dB optimistic.
- No comparison with measured links yet. If you have measured eye diagrams, SER or TDECQ of a multimode link, please open an issue.

---

## 6. Browser performance

Measured in headless Chromium in a development VM (software rendering), not on a laptop: a fiber-length change ≈ 95–100 ms engine time (≈ 125 ms until painted), a connector-offset change ≈ 150 ms (includes re-coupling); TDECQ runs only on slider release (≈ 70 ms).

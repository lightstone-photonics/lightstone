# Lightstone – Architecture

How Lightstone is built: the reference link, the code structure, the physical model, the analysis layer, the model documentation, lanes, and the component graph with its engine and browser editor. Validation tests and results are in [`validation.md`](validation.md).

Companion documents: [`signal-spec.md`](signal-spec.md) (signal definitions), [`tutorial-reference-link.md`](tutorial-reference-link.md) (the reference graph node by node), [`tutorial-signals-and-math.md`](tutorial-signals-and-math.md) (introduction), [`tutorial-component-graph.md`](tutorial-component-graph.md) (Python graph API and custom components), [`model/`](model/) (per-component math, limits, validation), [`references.yaml`](references.yaml).

**Scope:** 850–950 nm VCSELs, multimode fiber (OM3/OM4/OM5 presets and custom fibers), WDM over multimode fiber, PIN/TIA receivers with FFE, PAM-4 and NRZ analysis including TDECQ. Single-mode, amplified, coherent and nonlinear links are not covered yet.

---

## 1. Reference link

| Block | Key parameters (defaults) |
|---|---|
| Transmitter | 53.125 GBd PAM-4, PRBS13Q, Gray coding (NRZ with PRBS13 as an option, `levels: 2`, since v0.11) |
| VCSEL | 850 nm, RMS spectral width 0.4 nm, 2nd-order response (f_r, damping), RIN −140 dB/Hz, average power, ER/OMA |
| Connector | Radial offset Δr, tilt θ |
| Fiber | OM4, 50 µm core, NA 0.20, graded index (α ≈ 2), length 0–150 m |
| Receiver | PIN responsivity, finite detector area, TIA noise 15 pA/√Hz, 4th-order Bessel-Thomson filter (f_3dB = 0.5 × baud), 5-tap adaptive FFE (since v0.15, `rx.ffe_taps`; 0 = none) |
| Analysis | Eye density, EH/EW, per-level SER, TDECQ (5-tap FFE reference equalizer) |

---

## 2. Code structure

```
lightstone/
├── crates/
│   ├── lightstone-core/       # Physics and graph engine
│   │     signal, component,   #   signal model and component interface
│   │     components, graph    #   built-in nodes, JSON graphs, evaluation, caching
│   │     tx, modes, coupling, #   validated physics used by the nodes
│   │     fiber, rx, dsp, …
│   │     system, link         #   lanes as preset graphs; Simulator / SystemSimulator APIs
│   ├── lightstone-analysis/   # Alignment, eye density, EH/EW, SER/BER, TDECQ; G2 test
│   ├── lightstone-py/         # PyO3 bindings and Python API (maturin)
│   └── lightstone-wasm/       # wasm-bindgen bindings for the browser
├── reference/                 # NumPy/SciPy reference implementation (test oracle, V15)
├── tests/python/              # Python validation (V1, V15, R1 golden snapshot) and API tests
├── notebooks/                 # Validation report, sweeps, custom component, signal tutorial (generated)
├── examples/                  # Example graphs (graphs/) and a Python custom component (components/)
├── scripts/                   # Reference link checker
├── web/                       # Svelte 5 browser app; e2e/ browser smoke test
└── docs/
    ├── architecture.md        # This document
    ├── validation.md          # Validation suite and results
    ├── signal-spec.md         # Signal definitions
    ├── tutorial-signals-and-math.md
    ├── model/                 # One Markdown + LaTeX file per component
    └── references.yaml        # Standards, datasheets and papers cited by components
```

Rules:

- `lightstone-core` and `lightstone-analysis` contain no I/O, rendering or binding code.
- Bindings are thin wrappers: no physics in `lightstone-py` or `lightstone-wasm`.
- All internal quantities are SI units (m, s, Hz, W, A, V). Unit conversion (dB/km, ps/m, nm) only happens at the API boundary.
- Core numerics use `f64`. `f32` is only allowed for rendering buffers.
- Every simulation is a graph of components (Section 7). The engine caches each node's outputs, keyed by its kind, parameters, the simulation settings and its inputs' keys, so a change recomputes only the nodes downstream of it. Expensive shared results (mode solutions, launch coupling, OFL bandwidth) are cached separately and reused across nodes and runs. Example on the reference link:

| Parameter changed | Nodes recomputed |
|---|---|
| Fiber profile α | Fiber (new mode solve and coupling) and everything downstream |
| Connector offset / tilt | Connector, fiber (new coupling, cached mode solve), receiver chain |
| Fiber length | Fiber (cached coupling), PIN, TIA, filter, receiver output: 5 of 9 nodes |
| RX noise / filter | The receiver nodes from the changed one onward |

---

## 3. Physical model

### 3.1 Transmitter (intensity modulation)

1. PRBS13Q → Gray-coded PAM-4 symbols s ∈ {0, 1, 2, 3}.
2. Map to optical power levels from average power P_avg and extinction ratio ER (levels equally spaced in power).
3. Apply the VCSEL small-signal response (2nd-order: resonance frequency f_r, damping γ) to the power waveform.
4. Add RIN as level-dependent Gaussian noise on optical power.

Modulation acts on **power**, not field amplitude. Power is non-negative at every step.

### 3.2 Fiber modes

- Scalar LP modes of a graded-index α-profile: n(r) = n₁·√(1 − 2Δ(r/a)^α) in the core, n₂ in the cladding.
- Solved numerically: a radial finite-difference eigenproblem per azimuthal order l.
- Output per mode: (l, m), n_eff, group delay τ_m/L, chromatic delay slope dτ_m/dλ, field ψ_m(r, φ).
- Modes are labeled with the principal mode group g = 2m + l − 1 (reporting only; propagation is per mode).
- **Differential mode delay (DMD)** comes from the deviation of α from its optimum. The default fiber is the minimum-spec OM4 preset (v0.13, α = 2.0777): EMB with the default launch ≈ 4710 MHz·km (OM4 min 4700). OM3 and OM5 presets and a custom fiber exist (`docs/model/mmf.md`).
- **Materials** (v0.12): fused-silica cladding, germania-doped core (GeO₂ fraction from the NA at 850 nm, Fleming 1984). Δ changes with λ (profile dispersion), so the optimum α at 850 nm is ≈ 2.03–2.04. `core_material: "fixed_delta"` keeps the earlier pure-silica, fixed-Δ model.

### 3.3 Launch and connector coupling

- VCSEL emission modeled as an incoherent, equal-power mix of the lowest Q Laguerre–Gauss mode groups (default Q = 4, w = 4 µm). Q = 1 is a single-mode Gaussian beam.
- Coupling coefficients via the 2D overlap integral:
  c_m = ∬ E_in(x − Δx, y − Δy) · ψ_m*(x, y) dx dy
- Tilt adds a phase factor exp(i·k·n·θ·x) to E_in.
- Power per mode: P_m = |c_m|². Unguided power: 1 − Σ P_m (radiated, counted as loss).
- Launch condition is reported as encircled flux for comparison with IEC 61280-4-1.

### 3.4 Propagation (incoherent, power-based)

Because the VCSEL coherence time (~ps) is shorter than the modal delay spread, intermodal interference averages out. Lightstone propagates **power per LP mode** (one linear filter on power, H(f) = Σ_m w_m e^{−j2πfτ_mL} e^{−2π²f²σ_m²}):

P_m(t, L) = P_m,launch · 10^(−α·L/10) · [h_m ∗ P_tx](t − τ_m·L)

where h_m is the chromatic-dispersion impulse response of mode m, derived from the source spectrum and dτ_g/dλ. This also captures the interaction between modal and chromatic dispersion.

A coherent mode (per-mode complex envelopes, partial detector illumination) for modal-noise studies is not implemented; the signal format reserves it (`signal-spec.md`, Section 9).

### 3.5 Receiver

1. Detected photocurrent: I(t) = R · Σ_g η_g · P_g(t, L), where η_g is the fraction of group power falling on the detector area.
2. Noise, level-dependent where applicable:
   - Thermal (TIA): σ² = i_n² · B_n
   - Shot: σ² = 2q · I · B_n
   - RIN: generated at the transmitter, filtered by fiber and receiver
3. 4th-order Bessel-Thomson receive filter. B_n is the noise bandwidth of this filter.
4. The analytic per-sample σ(t) used for the semi-analytic SER is exact for the simulated noise processes (time-varying white noise through linear filters).

### 3.6 Sampling

- Integer oversampling: 16 samples/symbol by default (850 GS/s at 53.125 GBd); the browser uses 8. The NRZ drive is generated band-limited from its exact spectrum, so 8/16/32 samples per UI agree to < 0.002 dB TDECQ.
- Sub-sample delays applied as linear phase in the frequency domain.
- Pattern length: an integer number of PRBS13Q periods, so circular FFT convolution is exact.

---

## 4. Analysis layer

| Output | Definition |
|---|---|
| Eye density | 2-UI accumulation matrix (default 400 × 600), interpolated between samples, normalized |
| Level statistics | Mean μ_k and σ_k per PAM-4 level at the optimal sampling phase |
| EH₀₋₂ | Vertical opening of each sub-eye at a 10⁻³ probability contour |
| EW₀₋₂ | Horizontal opening of each sub-eye at the slicer threshold |
| SER | Sum over the 3 slicers of Gaussian tail probabilities using μ_k, σ_k (level-dependent, no double-counting of noise) |
| BER | SER / 2 (Gray coding approximation) |
| TDECQ | IEEE 802.3-style: 5-tap T-spaced FFE reference equalizer, find the added noise that gives SER = 4.8 × 10⁻⁴ |

---

## 5. Model documentation and references

Every component explains its own math and cites its sources, in the browser, the Python docs and the notebooks alike.

### 5.1 Single source of truth

- **Rule: one file per component, and it describes only what that component does to its input.** One component kind has one doc in `docs/model/`, and that doc covers what the component does with the signal it receives and what it hands on. It does not cover what happens before or after it in the link. Material that belongs to another component (for example RIN in the pattern source, or receiver clock jitter in the driver) goes into that component's doc, and a short link points there. The editor's inspector shows exactly this doc for the selected node (`DOC` in `web/src/lib/graph/catalog.js`), and a web unit test checks that every kind has its own doc and every doc belongs to a kind. Example components follow the same rule, and their doc may also explain how to write one (`rust-example.md`, and `example-component.md` for Python; Python-only components have no editor kind and are marked `python_only: true`).
- Written in Markdown with LaTeX equations; the file name is the doc id (e.g. `vcsel.md`, `driver.md`, `prbs-pattern.md`, `custom-fiber.md`, `tdecq.md`).
- Every symbol in a formula is described next to it: what it is, its unit, and its role.
- Each equation has a stable ID (e.g. `eq:coupling-overlap`). The implementing Rust function references that ID in its doc comment, and the validation test that checks it references it too. This keeps docs, code and tests linked.
- The same files are rendered in the web app, in the Python documentation, and embedded in notebooks. Math is never written twice.
- Each file has the same structure:
  1. **What it models** – one paragraph in plain language
  2. **Equations** – the model as implemented, with symbols and SI units
  3. **Assumptions & limits** – what is neglected and when the model breaks down
  4. **Parameters** – name, symbol, unit, default, valid range
  5. **Validation** – which V-tests cover it, with current status
  6. **References** – IDs from `references.yaml`
- **Figures** go into `docs/model/img/` as SVG and are linked as `![alt text](img/name.svg)` with an italic caption below. Style: textbook line art (serif type, italic symbols, black on white, one accent color), so they read the same on GitHub and in the dark browser panel. Schematics show structure and geometry; plots show computed results and are generated from the simulator by `scripts/make_figures.py`, so they cannot drift from the code. Every figure has alt text and an SVG `<title>`/`<desc>` for screen readers. CI regenerates them and warns (non-blocking) if they differ from the committed files.

### 5.2 References

`references.yaml` holds every cited source:

```yaml
- id: ieee-802.3-cl138
  title: "IEEE 802.3, Clause 138 – 50GBASE-SR / 100GBASE-SR2 / 200GBASE-SR4"
  type: standard
  url: https://standards.ieee.org/ieee/802.3/
  access: free            # free | paywalled | registration
  used_by: [tdecq, bessel-thomson]
- id: tia-492aaad
  title: "TIA-492AAAD – OM4 fiber specification"
  type: standard
  access: paywalled
  used_by: [multimode-fiber]
```

Rules:

- Link to official sources only: standards bodies, vendor datasheet pages, DOIs. Do not host copies of standards or datasheets (licensing).
- Mark access status. IEEE 802.3 is available free through the IEEE GET program; TIA and IEC documents are paywalled, so link to their catalog page and summarize the relevant numbers in the component doc.
- Vendor datasheets: store URL and retrieval date. Default parameters taken from a datasheet cite it, so users can see where each default comes from.
- CI runs a link checker on `references.yaml` (weekly, non-blocking) to catch dead links.

### 5.3 In the browser

- Each component has an **"i" / "Math"** panel that opens its documentation next to the simulation.
- Equations rendered with KaTeX (fast, no server, works offline).
- **Live values:** key equations show the current parameter values and intermediate results (e.g. V-number, number of mode groups, coupled power per group, noise σ) next to the symbolic form.
- A **References** list per component, with access badges (free / paywalled).
- A **Limits** box per component, stating what the model does not cover, to stop the demo from being trusted beyond its validity.
- Accessibility: equations rendered with KaTeX's MathML output for screen readers; panels keyboard-navigable.

---

## 6. Lanes and systems

A **lane** is a preset chain of components (transmitter, connector, fiber spans, attenuators, receiver). Lanes predate the general graph (Section 7) and are now built on it: the Python `Simulator`, `System` and `lane` APIs create the corresponding graph. In the browser, lanes are edited as frames in the graph editor.

### 6.1 Data model

A **system** consists of independent **lanes** (e.g. parallel VCSEL links as in SR4). Each lane is a chain:

```
VCSEL transmitter → [element]* → PIN/TIA receiver
element ∈ { connector, fiber, attenuator }
```

```json
{ "lanes": [ { "name": "Lane 1", "seed": 0,
    "tx": {…}, "vcsel": {…}, "source": { "beam_radius": 4e-6, "vcsel_mode_groups": 4 },
    "elements": [ { "kind": "connector", "offset": 0, "tilt": 0, "loss_db": 0 },
                  { "kind": "fiber", "length": 100, … },
                  { "kind": "attenuator", "loss_db": 3 } ],
    "rx": {…} } ] }
```

The old single-link `LinkConfig` maps to a one-lane system (connector + fiber). `Simulator` stays as a thin wrapper, so the existing API and all validation tests (V1–V15) keep running unchanged against the new engine.

### 6.2 Physics of a chain

The lane accumulates a **modal path**. For each LP mode it tracks power, total group delay and total chromatic delay slope:

- **Launch**: the first connector before the first fiber defines offset and tilt of the VCSEL beam. Mode powers come from the overlap integrals (Section 3.3). Without a connector the launch is ideal (centered, no tilt).
- **Fiber span**: delay τ_m += (τ_m/L)·L and slope += (dτ_m/dλ)·L; power × attenuation.
- **Attenuator / connector after a fiber**: power × 10^(−loss/10).
- **Receiver**: detector-area fraction from the last span's mode fields, then H(f) = Σ_m w_m e^{−j2πf(τ_m−τ̄)} e^{−2π²f²σ_m²} with σ_m = |slope_m|·σ_λ.

Validation rules (shown in the editor):
- All fiber spans in a lane must share core radius, NA and solver grid. Modes are mapped by their (l, m) label, and modes not guided in a later span count as loss (approximation for similar fibers).
- A connector after a fiber may only add loss. Offset and tilt between fibers would redistribute power between modes (a mode-coupling matrix), which is not modeled. The editor shows an error.
- A lane without fiber is a back-to-back link.

Required tests:
- **R1**: the default single-lane system equals the pre-refactor results (golden snapshot). Measured agreement ≈ 3 × 10⁻¹² (rounding); test tolerance 10⁻⁹.
- **R2**: span concatenation, 60 m + ideal connector + 40 m = 100 m.
- **R3**: an attenuator of X dB equals the fiber's extra loss of X dB.
- **R4**: lanes are independent (changing lane 2 leaves lane 1 bit-identical).

---

## 7. Component graph and signal model

The lane chain (Section 6) generalizes to a graph: components are placed on a canvas, their ports are wired together, and one output may feed several inputs. This makes Lightstone a simulation framework rather than a single-link tool. The signal definitions, the linear/nonlinear distinction and the noise handling are specified in [`signal-spec.md`](signal-spec.md).

### 7.1 Goals

- **Grid editor:** drag components from a palette onto a canvas, connect ports with wires, and select a node to see its parameters and math panel. Zoom into nodes as today.
- **Unified signal model:** typed ports (`Data`, `Electrical`, `Optical`) on one global time grid (signal-spec, Sections 1–4).
- **Polarization in the signal format from the start:** every optical signal carries a polarization state per mode bin: `Power` (unpolarized) by default at no cost, or Stokes vectors, with Mueller matrices for polarization-aware components. Jones vectors are used at the `Field` level (signal-spec, Section 8).
- **Phase in the signal format from the start:** an optical signal has a representation level `Power` (default) / `Stokes` / `Field`. `Field` carries complex envelopes (phase) per mode bin and polarization, plus a coherence descriptor per source. The engine uses the lowest level each wire needs, so current graphs never pay for it. Coherent and single-mode components later live in the same `Optical` type (signal-spec, Section 9).
- **Fan-out:** one output can drive several inputs. Optical fan-out to probes and analyzers is an ideal tap by default, with an optional **realistic** mode (tap ratio, insertion loss). Fan-out to other optical components needs an explicit splitter, which the editor offers to insert (signal-spec, Section 7).
- **Custom components** in Python or Rust (Section 7.4).
- **Reactivity:** nice to have, not required. Per-node caching recomputes only what is downstream of a change; large changes may use an explicit **Run** button with progress.

### 7.2 Components

The current physics is split into nodes:

| Category | Components |
|---|---|
| Sources | PRBS13Q / pattern generator (`Data`) |
| Transmitter | Driver (`Data → Electrical`), VCSEL (`Electrical → Optical`) |
| Optical | Connector / launch, fiber span, attenuator, splitter, combiner |
| Receiver | PIN photodiode (`Optical → Electrical`), TIA, Bessel–Thomson filter, FFE equalizer, receiver output |
| Analysis (sinks) | Eye diagram, TDECQ, scope, spectrum, power meter, mode-power view |
| Custom | Python or Rust component |

Each component declares its ports, parameters, a model document (`docs/model/*.md`) and whether it is `Linear(H)` or `Nonlinear` (signal-spec, Section 6).

### 7.3 Engine

- The graph must be acyclic for now; it is evaluated in topological order. Loops are planned and must not be ruled out by any design change (decision in Section 7.8).
- Optical signals stay **lazy** (source + accumulated per-mode-bin transfer function) through linear components and are **materialized** only where a nonlinear or custom component needs waveforms.
- **Mode resolution** is a simulation setting: `ModeGroup` (default) or `Mode`. Components do not depend on it.
- Noise terms are carried along with their accumulated filters. After a nonlinear block, analyses switch from the semi-analytic SER to a Monte Carlo SER and say so.
- Results are cached per node and output, keyed by the node's parameters and its inputs' cache keys.
- Graphs are saved as JSON (nodes, parameters, wires, and an optional `layout` with editor node positions that the engine ignores). The JSON is the same format for Python, the browser and later shareable links.

### 7.4 Custom components

One interface for both languages: declared ports, parameters, a documentation string (Markdown + LaTeX for the math panel) and a `process(inputs, params)` function. Optionally a linear declaration with H(f), which keeps signals lazy.

- **Python:** works natively in the `lightstone` package (the Rust engine calls the Python function, NumPy arrays in and out). Not in the browser yet (that would need Pyodide, ≈ 10 MB extra download).
- **Rust:** a crate implementing the component trait, compiled together with the engine. Example and step-by-step guide: `crates/lightstone-core/src/example.rs` (kind `rust_example`, `docs/model/rust-example.md`). This is the route for performance-critical components and for the official library. Rust components cannot be written in the browser.
- Custom components show an **"unvalidated"** badge.
- Code runs locally in the user's browser or Python process. Only when a loaded system contains custom code from someone else does the browser ask for confirmation before running it.

### 7.5 Required tests

- **G1:** the reference link built as a graph reproduces the current results (golden snapshot, Section 6.2 R1).
- **G2:** mode-group vs. per-mode resolution on the reference link; the TDECQ and SER difference is reported, with a stated limit.
- **G3:** splitter energy conservation: output powers sum to (1 − excess loss) × input.
- **G4:** an ideal tap leaves the through signal bit-identical; a realistic tap reduces it by exactly its insertion loss.
- **G5:** a custom Python component declared `Linear(H)` gives the same result as the equivalent built-in component.
- **G6:** a nonlinear block (TIA clipping) switches the SER to Monte Carlo, and with clipping disabled it matches the semi-analytic SER.
- **F1–F4:** phase and coherence tests (signal-spec, Section 9.7). F1: sources emitting at `Field` give the same results as `Power` for current components (to rounding, ≤ 10⁻¹²).
- **P1–P4:** polarization tests (signal-spec, Section 8.6). P1: the reference link gives bit-identical results at the `Power` and `Stokes` levels.

### 7.6 Graph engine (v0.7)

`lightstone-core` now runs every simulation on the graph engine:

| Module | Content |
|---|---|
| `signal` | Settings and time grid; `Data`, lazy `Electrical` with symbolic noise terms, `Optical` with per-source contributions, mode bins, `Power`/`Stokes`/`Field` levels, coherence descriptor and reserved backward wave |
| `component` | Port types, `Component` trait, linearity declaration, reports, shared caches (modes, coupling, OFL) |
| `components` | `prbs13q`, `driver`, `vcsel`, `connector`, `mmf`, `custom_fiber`, `attenuator`, `splitter`, `combiner`, `probe`, `power_meter`, `pin`, `tia`, `bessel_thomson`, `receiver_output` |
| `graph` | JSON graph format, registry (open for custom components), validation, topological evaluation, per-node caching |
| `system` | Lanes as a preset graph (`lane_graph`); `LaneEngine`, `SystemSimulator` and `Simulator` keep their APIs |

Python, WebAssembly and the browser are unchanged and run on the new engine. Interactive speed is unchanged: a fiber-length change takes ≈ 88 ms engine time in headless Chromium (before: ≈ 85–100 ms).

**Test results:**

| Test | Result |
|---|---|
| G1 | Golden snapshot: noiseless waveform and σ(t) agree to ≈ 3 × 10⁻¹²; the noisy waveform to ≈ 5 × 10⁻¹⁰ (noise terms are now filtered per term instead of summed first; rounding only, same random streams) |
| G2 | See table below |
| G3 | Splitter: Σ outputs = (1 − excess loss) × input to 10⁻¹² |
| G4 | Ideal probe bit-identical; realistic probe exact to 10⁻¹² |
| P1 | `Stokes` bit-identical to `Power` |
| F1 | `Field` equal to `Power` to ≤ 10⁻¹² (rounding of the Jones decomposition) |
| F4 | Field → Stokes conversion exact for linear, circular, elliptical and partially polarized light |
| B1 | Backward waves enabled (all empty) gives identical results |
| Other | Graph validation (types, unconnected inputs, loops, optical fan-out), caching (a length change recomputes 5 of 9 nodes), incoherent combination of two VCSELs, partial-coherence warning, materialized mode bins = lazy detection |

All earlier tests (V1–V15, R1–R4) pass unchanged: 58 core, 12 + 1 analysis and 17 Python tests; clippy and fmt are clean.

**G2: mode group vs. per mode** (reference link, RIN off so that only the modal effect is compared):

| Case | TDECQ per mode / per group | SER per mode / per group | RMS delay spread [ps] |
|---|---|---|---|
| 50 m, centered | 1.289 / 1.289 dB | 5.5 × 10⁻⁹ / 5.5 × 10⁻⁹ | 1.93 / 1.93 |
| 100 m, centered | 1.879 / 1.879 dB | 1.85 × 10⁻² / 1.85 × 10⁻² | 3.87 / 3.86 |
| 150 m, centered | 2.989 / 2.989 dB | 1.20 × 10⁻¹ / 1.20 × 10⁻¹ | 5.80 / 5.79 |
| 100 m, 8 µm offset | 1.983 / 1.984 dB | 3.73 × 10⁻² / 3.76 × 10⁻² | 4.58 / 4.12 |

The mode-group approximation changes TDECQ by ≤ 0.001 dB here. With an offset launch, the delay spread inside groups matters more (RMS spread −10 %), but the effect on TDECQ and SER stays small. The test limit is |ΔTDECQ| < 0.3 dB.

**Design decisions (v0.7):**
- **Resolution:** new graphs default to `ModeGroup` (as specified). The lane presets behind the existing APIs, the browser and the validation tests use `Mode`, so all validated results stay exactly as before.
- **`Field` level:** a mode bin holds up to three mutually incoherent Jones vectors (polarized part plus two orthogonal vectors for the unpolarized part). This represents partially polarized light exactly. Detection uses the incoherent limit (diagonal detector overlap); cross terms are reserved.
- **Linearity declaration:** declared by every component, and since v0.8 the engine applies declared transfer functions itself (Section 7.7).
- **Materialized optical signals:** available via `Contribution::materialize`; built-in components only consume lazy optical signals so far.
- **`receiver_output`** (called `decision` until v0.12; it makes no decision) materializes waveforms and σ(t). The actual PAM-4 analysis and TDECQ stay in `lightstone-analysis`; analyzer nodes run inside the graph since v0.8 (Section 7.7).
- **Seeds:** components without an explicit seed derive one from the graph seed and their node id, so two receivers in one graph get independent noise. Lane presets pass explicit seeds that reproduce the old random streams.

### 7.7 Engine-applied transfer functions, custom components, analysis nodes (v0.8)

**Engine** (`lightstone-core`):

| Item | Implementation |
|---|---|
| Engine-applied H(f) | `Component::transfer` returns H(f) on the grid. For a `Linear` component with one electrical or optical input and output, the engine applies it and does not call `process`. Electrical: H is appended to the signal and every noise term, so σ(t) stays exact. Optical: H acts on power, common to all mode bins (`h_extra`); per-mode-bin H(f) is still open. H must have a real impulse response (H(−f) = H(f)\*, checked to 10⁻⁹; the Nyquist FFT bin is exempt because filters are complex at +fs/2). |
| Automatic materialization | Inputs of nonlinear custom components are materialized (noiseless and noisy waveform, σ(t), `gaussian_exact`). Outputs are wrapped as `ElectricalSignal::from_waveforms`: the noise is carried as one explicit realization (`NoiseKind::Nonlinear`) that does not enter σ(t), and `gaussian_exact` becomes false. If no noise reached the block, the output is deterministic and stays exact. |
| TIA clipping | `tia.clip = [low, high]` \[V\] makes the TIA nonlinear: noiseless and noisy waveform are clipped after the thermal noise is added (signal-spec 6.3, case B). The TIA warns how often it clipped. |
| Decision | Warns when `gaussian_exact` is false: use the counted (Monte Carlo) SER, with its resolution 1/N_sym. |
| Custom components | `lightstone-core::custom`: `CustomSpec` (ports, linear flag, domain, doc) and a `CustomBehavior` trait (`transfer`, `impulse` or `process`), registered per graph engine. Time-domain linear components return an impulse response, a continuous h(t) \[1/s\] sampled on the grid or FIR taps; `transfer_from_impulse` converts it to H(f) (`eq:custom-impulse`, below). A nonlinear component described in the frequency domain is rejected. Built-in kinds cannot be replaced. Nonlinear custom components with optical ports are rejected: they would need materialized optical signals downstream, which built-in components do not consume yet. |
| Time-domain linear components | `eq:custom-impulse`: on the periodic grid the DFT of the sampled impulse response is exactly the transfer function of the same circular convolution, H\[k\] = Σₙ g\[n\] e^{−j2πkn/N}, with g\[n\] = h(tₙ)·Δt for a continuous h(t) (tₙ signed, negative times wrapped to the end of the period) and g\[i·s\] = cᵢ for taps (s = 1 or samples per symbol). A real h gives H(−f) = H(f)\* by construction. Warnings at the node: h(t) not decayed within half a period (wraps around), or a sampled h(t) not band-limited to fs/2 (\|H(fs/2)\| > 10⁻³ of the peak: aliases; typically a jump at t = 0). |
| Analysis nodes | `lightstone-analysis::nodes` (the core cannot depend on the analysis crate, so these kinds are added to the registry by `nodes::register`; `nodes::registry()` is core plus analysis, used by the Python `Graph`). `pam4_analyzer`, `eye_diagram`: inputs `in` (electrical) and `ref` (transmitted symbols); semi-analytic SER while `gaussian_exact` holds, counted SER otherwise. `tdecq`: inputs `in` (optical) and `ref`; forms the reference-receiver waveform itself. Results are `Report::Data` (JSON). Model doc: `docs/model/receiver-output.md` |
| Receiver FFE | Node `ffe` (`docs/model/ffe.md`): T-spaced, inputs `in` and `ref`, output `out`. Fixed taps, or adapted once by constrained MMSE (Σc = 1) on the noiseless waveform, optionally including the noise autocorrelation (`noise_aware`, default). Then a fixed linear filter: σ(t) stays exact. Its H(f) depends on the input it was adapted to, so for loops (14.9) it would need to be held fixed |
| Built-in kinds | `Registry::register_builtin` marks kinds that custom components cannot replace (core and analysis kinds) |
| Engine API | `GraphEngine::node_ports` (ports for editors and bindings), `clear_cache` (a re-registered kind's code is not part of the cache key). |

**Python** (`lightstone`):
- `Graph`: build (`add`, `wire`, `chain`, `insert`, `replace`, `set`, `remove`), `Graph.reference(lane)` for the lane preset, JSON round trip, `validate`, `run` (cached), `report`, `output`, `power`, `warnings`, `computed_last_run`.
- `receiver("rx")` returns a `ReceiverResult`; `analyze()` uses the semi-analytic SER while `gaussian_exact` holds and the counted SER otherwise, and reports `ser_method` and `ser_resolution`.
- Custom components: `Component(kind, inputs, outputs, linear=…, domain=…, function=…)` or the `@lightstone.component` decorator. `linear` and `domain` are required, with no defaults, so it is always explicit what the function receives. Linear components are described in the frequency domain (H(f)) or the time domain (impulse response h(t), or FIR taps with `tap_spacing` `"sample"` or `"symbol"`). Nonlinear components are always time domain: they get `Waveform` objects (noiseless, noisy, σ) and return waveforms. Python exceptions are reported at the node with their traceback.

**Test results:**

| Test | Result |
|---|---|
| G5 | A Python Bessel–Thomson filter declared linear equals the built-in filter (noiseless, σ(t), noisy) to ≤ 10⁻¹²; a Python H = T on optical power equals the built-in attenuator to ≤ 10⁻¹² (receiver and power meter). The same holds for Rust custom components. |
| G6 | 50 m, i_n = 120 pA/√Hz, 4 pattern periods (32 764 symbols): semi-analytic SER 3.60 × 10⁻², Monte Carlo SER through a TIA clipping at ±1 kV 3.73 × 10⁻² (1.3 σ_stat; six seeds average 3.54 × 10⁻²). Clipping at 0.30…0.60 V raises it to 6.6 × 10⁻². A Python clipper after a linear TIA is bit-identical to the built-in clipping TIA. |
| Time domain | A sampled Gaussian h(t) gives the periodized analytic H(f) to 10⁻¹²; taps \[1\] give the identity, \[0, 1\] a one-sample delay, symbol-spaced taps Σcᵢe^{−j2πf·iT}. In the graph, a Gaussian described by h(t) equals the same filter described by H(f) (noiseless, σ(t), noisy) to ≤ 10⁻¹², in Rust and Python. A symbol-spaced 3-tap FFE (−0.15, 1.3, −0.15) lowers the 100 m SER from 3.3 × 10⁻² to 6.5 × 10⁻⁶. Warnings for wrap-around and aliasing are tested; missing or contradictory `linear`/`domain`/`taps` arguments are rejected. |
| Analysis nodes | On the reference link, `pam4_analyzer` and `eye_diagram` give results identical to the analysis functions on the lane results (SER, eye heights, eye counts), and `tdecq` agrees to 10⁻⁹ dB (2.854 dB at 100 m, 8 samples/UI). After TIA clipping the analyzer reports the counted SER and warns. |
| FFE | E1: the noise-free design inverts a known ISI channel [1, 0.3]/1.3 (taps to 10⁻³). E2: its 5-tap noise-free design equals the TDECQ reference equalizer (10⁻⁹); with strong noise, the noise-aware design has a lower noise gain. E3: at 100 m a 5-tap FFE lowers the SER from 3.1 × 10⁻² to 8.9 × 10⁻⁸ (9 taps: 6.7 × 10⁻⁸) and opens all eyes, with σ(t) exact; fixed taps [1] are bit-identical. At 16 samples/UI: 3.3 × 10⁻² → 9.0 × 10⁻⁸, eye heights 85/76/66 mV instead of −19/−22/−30 mV. |
| Other | Wide-limit clipping reproduces the linear receiver to 2 × 10⁻¹⁰ (the difference sits in the Nyquist FFT bin only; elsewhere 4 × 10⁻¹⁴). Transfer-function checks, port rules, Python error reporting, caching (custom re-registration recomputes all nodes), JSON round trip. |

All earlier tests pass unchanged: 65 core, 22 analysis (14 unit, 6 analysis-node, G2, G6) and 32 Python tests; clippy (including the Python bindings) and fmt are clean. Python, WebAssembly and the browser still run the lane presets, so interactive results are unchanged.

**Still open:**
- Built-in linear components still apply their effect in `process`; moving them onto declared transfer functions does not change results, and is a prerequisite for loops (Section 7.8).
- Per-mode-bin optical H(f) and nonlinear optical components (materialized optical signals, signal-spec 6.3 case C).
- A `docs/model/` file and math-panel entry for custom components (shown as "unvalidated").

### 7.8 Loops (design decision)

The engine does not evaluate loops yet, but nothing in the design may rule them out. Lab setups with fiber in a loop, ring resonators, loop mirrors, cavities and multiple reflections all need them. This section fixes how loops will work, so that changes made now keep that path open.

**Three kinds of loop:**

| Kind | Examples | How it is simulated |
|---|---|---|
| Recirculating loop (time-gated) | Fiber loop with switches: the signal passes the same span N times in sequence, never overlapping itself | Unrolled: N copies of the round trip in a chain. Possible today (N fiber nodes; mode solve and coupling are cached). A `loop` node that unrolls a round-trip subgraph N times is a convenience, useful once amplifiers or mode coupling between spans exist |
| Linear feedback (steady state) | Fiber ring resonator, Sagnac loop / loop mirror, Fabry–Pérot cavity, double reflections (MPI) | Solved per frequency. At the `Power` level for incoherent light (loop much longer than the coherence length): a geometric series, e.g. T/(1 − R·H_k(f)) per mode bin. At the `Field` level with complex transfer functions or S-matrices (signal-spec 11.2) |
| Nonlinear feedback (steady state) | Loop with a saturating or clipping element, amplified loop below lasing | Periodic steady state by fixed-point iteration around the loop until the waveforms stop changing. Converges when the loop loses more than it gains |

**How the engine will handle them.** A cycle in the graph will be allowed. The engine finds the strongly connected components (groups of nodes that feed back into each other) and treats each as one unit: linear units by a per-frequency solve, others by fixed-point iteration. The graph of these units is acyclic by definition, so evaluation order, per-node caching (one cache key per unit) and the semi-analytic noise of everything outside the loop stay as they are. The JSON graph format needs no change: wires that close a cycle can already be written, and no extra subcircuit markup is needed.

**Accepted limit: transients.** The periodic time grid (signal-spec 1) describes steady states only. A loop filling up, a laser switching on, clock recovery or an AGC locking in need time-stepped simulation. That would be a separate engine, not an extension of this one. A loop with gain ≥ 1 has no steady state and is reported as an error.

**Prerequisites** (to be done before loops, and kept in mind for all changes until then):

1. **Per-mode-bin transfer functions in the lazy optical signal.** Light that has gone around a loop is a sum over round trips per mode bin, not one weight, delay and slope. The lazy form needs a general H_k(f) per mode bin alongside (w_k, τ_k, D_k) (signal-spec 3.3, 6.1).
2. **Merging contributions of the same source.** Light returning to a coupler meets itself. Contributions from the same source in the same mode basis must merge into one (their per-mode-bin H add), instead of accumulating one contribution per round trip as the combiner does today.
3. **Declared linear maps for built-in components.** To set up the loop equation, the engine needs every linear component's effect as a (per-mode-bin) transfer function, not only its result. Built-in linear components move from applying their effect in `process` to declaring it (`Component::transfer`, extended to per-mode-bin and multi-port maps).
4. **Engine:** strongly connected components, per-unit cache keys, the per-frequency solve and the fixed-point iteration (with a convergence limit and a clear error when it fails).
5. **Editor:** layout and wiring must allow backward wires from the start. A strictly left-to-right layered layout would build the no-loop assumption into the UI.

Until the engine supports loops, the cycle error stays, and its message should say that loops are planned.

**Required tests (when implemented):**
- **L1:** an unrolled recirculating loop of N round trips equals the equivalent single link (checked now with a graph of N spans: 3 × (50 m + 1 dB) equals 150 m + 3 dB to 8 × 10⁻¹⁶).
- **L2:** an incoherent power ring matches the analytic geometric series (transmission, ripple period = 1/loop delay).
- **L3:** fixed-point iteration of a nonlinear loop converges, and equals the linear solve when the nonlinearity is inactive.
- **L4:** a loop with gain ≥ 1 is reported as having no steady state.
- **B2–B4** (signal-spec 11.3) for loops at the `Field` level.

---

### 7.9 Browser graph editor (v0.10)

The browser app is one **graph editor** (full window width). The earlier lane-based system view is retired in the browser; its parts moved into the editor: component symbols, zone markers (as frames), the fiber facet and the fiber component view. Lanes remain in the core and the Python API (`System`, `lane`), where they are presets of the graph.

| Part | Implementation |
|---|---|
| WASM bindings | `lightstone-wasm::Graph`: core and analysis kinds (`nodes::registry`), `run` (cached evaluation; compact per-node results as JSON), `eye(node)` (eye densities), `preview(node, port)` (32 symbols of a port's signal), `fiber_view(node)` and `facet(node, …)` (fiber component view and facet images from a fiber node's result: index profile, mode delays and powers at the span output, transfer function, launch beam, guided intensity, mode fields), `ports`, `validate`, `kinds` (ports and default parameters of every kind), `lane_graph` (reference link), `version`. The former `System` class is removed |
| Canvas | SvelteFlow 1.7.1 (`@xyflow/svelte`, MIT). Nodes show a schematic symbol per kind (style of the former system view), label, id and live result; ports colored by signal type (optical, electrical, data; hollow = monitor input). Problems are marked red |
| Frames | Named, resizable areas behind the nodes (`layout.frames`): presets and graphs without a layout get the zones Transmitter, Channel, Receiver front end, Data recovery. Dragging a frame moves the nodes inside it; **Duplicate** copies a frame with its nodes and inner wires (new ids, explicit noise seeds cleared), so a lane is copied in one step. "+ Frame" frames the selected nodes, "Zone frames" recreates the zones |
| Editing | Palette with symbols (click to add after the selected node, auto-wired where possible; analyzers get the pattern source at `ref`), drag-to-wire with type checking, splitter insertion on optical fan-out (signal-spec 7), Delete, rename, auto layout (layered, wrapped rows, loops tolerated) |
| Node values | After each run the nodes show their results: e.g. fiber EMB, received power; measuring nodes (power meter, PAM-4/NRZ analyzer, TDECQ) as an instrument display with one large reading; the analyzer SER, BER, Q-factor, eye heights and widths (an SER of 0 is shown as below the method's floor, `< 2.3e-19`, or as `0 (no noise)`); the receiver output's mean and RMS noise; the eye diagram node a thumbnail of its eye with range and sampling phase |
| Node size | Any node can be resized by dragging its corners when selected; the eye thumbnail and the instrument displays grow with it. Sizes are saved in `layout.sizes` (`{ id: [w, h] }`, only for resized nodes; the engine ignores `layout`); *Reset node size* in the inspector restores the natural size |
| Inspector | Resizable. Parameters (sliders, switches, ratios, taps, all parameters as JSON), wiring by select boxes (keyboard alternative), live values, eye density, signal previews (SVG/PNG export), for fiber and connector nodes the facet (launch beam, guided intensity, mode fields), power per mode group and the four fiber plots; the model document with figures and math |
| Runs | **Run** records a run: the graph as run, all node results, the value at every output port, eyes and signal previews (optional). **Live update** (on by default) re-runs on every edit; off, edits only mark the results as stale. Runs are listed with label, time and key metrics; ticked runs are compared (metrics that differ). Save all runs as JSON, export CSV (one row per run), open saved results, load a run's graph back. CSV and comparison show the report metrics; **include all port values** adds every output port. The inspector lists each output's value; hovering a wire shows the value at its source |
| Sweeps | One node parameter over a linear or logarithmic grid; every point is a run (overrides + metrics, eyes optional) in the same list, plotted against the parameter (metric selectable, log y). Runs in a separate engine in the worker, so the editor's cache is untouched |
| Autosave | The current graph is kept in the browser (localStorage) and restored on reload. Runs are kept until the page is reloaded; save them as JSON |
| Files | Graph JSON (with `layout.positions` and `layout.frames`), results JSON (study, below), CSV, PNG/SVG |

**Results file** (`format: "lightstone-study"`, `version: 1`): `{ format, version, created, app: { name, engine }, runs: [ run ] }` with `run = { id, label, note, created, graph, overrides, ok, issues, warnings, ms, reports, ports, eyes?, previews?, sweep? }`. `reports` are the compact node results (same as the editor shows; instruments and nodes with a report). `ports` summarizes every output port independently of the instruments placed, `ports[node][port]` = optical `{ type, average_power }` [W]; electrical `{ type, quantity, mean, swing, noise_rms, gaussian_exact }` in the signal's unit (`swing`: peak-to-peak of the noiseless waveform; `noise_rms`: RMS of σ(t), or of the explicit realization after a nonlinear block); data `{ type, symbols, levels }`. Runs saved before `ports` existed have none (still version 1). `eyes` maps an `eye_diagram` node to its counts (row-major, `reports[node].height` × `width`), `sweep = { id, param, index, value, unit, scale }` marks sweep points. The graph alone reproduces a run exactly (fixed seeds); the file is for comparing, reporting and archiving. CSV: `run, label, created, ok`, the override columns, then one column per scalar metric (`node.key`, arrays as `key[i]`); with port values, also `node/port.field` (e.g. `rxf/out.swing`). The power meter stays the explicit measurement point: its report is in `reports` and in the default columns.

**Tests:**
- `web/test/*.test.mjs` (`npm test`): port compatibility, editing, optical fan-out and splitter insertion, placement, layered layout with loops and wrapping, auto-wiring, zone frames and membership, frame duplication (ids, wires, seeds), runs and studies (overrides, grids, metric flattening, CSV, parse, compare), port values (recorded, kept out of the default columns, text).
- `web/e2e/graph.mjs` (headless Chromium, Playwright, about 80 checks): reference link with 4 zone frames; analyzer and FFE (SER 1.0 × 10⁻¹ → 1.9 × 10⁻⁷); OM4 preset, type selector, automatic conversion to a custom fiber and back; partial recomputation; fiber inspector (facet drawn, guided intensity, 4 plots); dragging a frame moves its nodes; splitter insertion; error marking and keyboard delete; eye and model doc; values on the nodes (eye thumbnail, analyzer instrument display, decision mean and σ); resizing the eye node by dragging (saved in the graph JSON); save/reopen; Run, live update off, results JSON with graph, label and eye, port values of all nodes with outputs, CSV with and without port values, output values in the inspector and on wire hover, PNG; comparison; a 4-point length sweep (SER rises 6.4 × 10⁻⁷ → 8.1 × 10⁻⁶ at 8 samples/UI with the FFE); autosave restore; duplicating the Transmitter frame computes only the 3 copies; switching the pattern source to NRZ.
- Rust: default parameters for every kind, layout changes recompute nothing.
- CI (`web` job) runs the unit tests, the build and the browser test.

**Limits and open points:**
- Undo/redo (v0.17): snapshots of the graph document, recorded with the autosave; repeated edits of one control within 400 ms (a slider drag) are one step; up to 100 steps; not persisted across reloads (`web/src/lib/graph/history.js`).
- Custom components (Python) cannot be defined in the browser (it would need Pyodide). A graph that uses custom kinds reports them as unknown. Components written in Rust are compiled into the engine and appear in the editor like built-ins (example: `rust_example`, palette group *Custom*, `docs/model/rust-example.md`).
- Frames are visual groups; they are not yet collapsible subsystem blocks. A duplicated frame is a copy, not a linked instance.
- Sweeps vary one parameter; multi-parameter grids and sweeps of frame-wide parameters are open. Sweep points run one after another in the worker.
- Wiring by drag needs a pointer; the inspector's input selects are the keyboard alternative.
- The system view's probe previews along the wires and the per-lane summary row are replaced by the inspector's signal previews and the nodes' badges.

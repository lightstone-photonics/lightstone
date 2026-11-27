# Tutorial: The Component Graph in Python

This tutorial shows how to build, run and extend simulations with `lightstone.Graph`. It starts with the reference link, explains what nodes, ports and wires are, and ends with your own components written in Python. All numbers below come from running the code with the default settings.

Each node of the reference link is explained in [`tutorial-reference-link.md`](tutorial-reference-link.md). Background on signals, noise and the linear/nonlinear distinction is in [`tutorial-signals-and-math.md`](tutorial-signals-and-math.md) (Sections 7 and 9). The exact definitions are in [`signal-spec.md`](signal-spec.md) and docs/architecture.md, Section 7.

**Setup.** The package is not on PyPI yet. Build it once from the repository:

```bash
python -m venv .venv && .venv/bin/pip install numpy maturin
cd crates/lightstone-py && ../../.venv/bin/maturin develop --release
```

---

## 1. Why a graph?

Lightstone has three Python entry points. They all run on the same engine:

| API | What you describe | Use it for |
|---|---|---|
| `Simulator` | One fixed link: TX → connector → fiber → RX, set by parameters | Parameter sweeps on the reference link |
| `System` | Lanes: TX → a chain of connectors, fibers, attenuators → RX | Several parallel links, extra elements |
| `Graph` | Any arrangement of components and wires | Everything else: splitters, power meters, your own components, nonlinear blocks |

`Simulator` and `System` are built on top of `Graph`. Internally every lane becomes a graph, so `Graph` is the general tool and the other two are shortcuts.

---

## 2. Nodes, ports, wires

A graph has three ingredients:

- A **node** is one component, for example a fiber. It has an `id` you choose (`"fib"`), a `kind` that says what it is (`"mmf"`, a multimode fiber), and `params` (`{"type": "om4", "length": 100}`).
- A **port** is a named input or output of a node. Every port has a **type**: `data` (symbols), `electrical` (current, voltage or the normalized drive) or `optical`.
- A **wire** connects an output port to an input port, written as `"node.port"`: `"fib.out" → "pin.in"`.

The reference link as a graph:

```
src ──data──▶ drv ──drive──▶ vcsel ──optical──▶ el0 ──▶ el1 ──optical──▶ pin ──current──▶ tia ──voltage──▶ rxf ──▶ ffe ──▶ rx
prbs13q       driver         vcsel              connector mmf (OM4)      pin               tia               bessel_thomson ffe  receiver_output
 └──────────────────────────────── data (transmitted symbols, for adaptation) ────────────────────────────────────────▶ ffe.ref
```

Since v0.15 the reference link ends with a 5-tap receiver equalizer (`ffe`, Section 6.2), as the 802.3 reference receiver does. `rx.ffe_taps: 0` in a lane configuration removes it.

Rules the engine checks before running:

| Rule | Why |
|---|---|
| Port types must match (no data into an optical input) | A fiber cannot do anything with symbols |
| Every input is connected exactly once | A component needs all its inputs |
| No loops (yet) | Nodes are evaluated in order, from sources to sinks. Loops are planned (docs/architecture.md, 7.8). A fiber loop with N round trips can already be built as N fiber spans in a row |
| An optical output feeds at most one component. Exception: **monitor** inputs such as `power_meter` | Light cannot be copied. To send it to two places, use a `splitter` (which divides the power) or a `probe` |

Electrical and data outputs may feed as many inputs as you like: copying a voltage has no physical cost.

---

## 3. Your first graph: the reference link

`Graph.reference()` builds the reference link as a graph:

```python
import numpy as np
import lightstone

g = lightstone.Graph.reference()
print(g.nodes())
# ['src', 'drv', 'vcsel', 'el0', 'el1', 'pin', 'tia', 'rxf', 'ffe', 'rx']
```

The ids are fixed by the preset. `el0`, `el1`, … are the lane **el**ements in order: here `el0` is the connector and `el1` the 100 m fiber.

Run it and analyze the receiver:

```python
rx = g.run().receiver("rx")
a = rx.analyze()
print(a["ser"], a["ser_method"])
# 1.9e-07  semi_analytic
```

- `run()` evaluates all nodes (≈ 0.5 s the first time, mostly the fiber mode solver).
- `receiver("rx")` collects the waveforms from the `receiver_output` node, plus the transmitted symbols for comparison. The receiver output makes no decision: it is where the signal becomes explicit waveforms and σ(t). Thresholds and SER come from the analysis. (Until v0.12 the kind was called `decision` and the node `dec`; graphs saved with the old kind still load.)
- `analyze()` returns the PAM-4 metrics: SER, eye heights, eye widths and so on (as for `Simulator`).

The reference fiber is a minimum-spec OM4 (EMB 4700 MHz·km, the worst fiber that still counts as OM4). At 100 m its eye is closed by intersymbol interference *before* the equalizer (SER 0.0995, eye heights ≈ −45 mV); the 5-tap FFE opens it (eye heights 78, 71, 61 mV). This is the same result `Simulator` gives.

---

## 4. Changing parameters: only what's affected is recomputed

```python
print(g.params("el1")["length"])      # 100.0
g.set("el1", length=70.0)
rx = g.run().receiver()
print(rx.analyze()["ser"])            # 9.0e-08
print(g.computed_last_run)            # 6
```

Only 6 of 10 nodes ran again (fiber, pin, tia, rxf, ffe, rx), in ≈ 0.1 s. Each node's result is cached and keyed by its parameters and its inputs. A node whose parameters and inputs did not change is not recomputed. Running again without a change computes 0 nodes.

`set` merges parameters: `g.set("el1", length=70.0)` changes only the length. Parameter names are those in the model documents ([`model/`](model/)), e.g. `length`, `attenuation_db_per_km` for the fiber, `noise_density` for the TIA. `g.params(id)` shows a node's current parameters.

Simulation-wide settings (symbol rate, samples per UI, number of pattern periods, seed) are in `g.settings`. Pass them when you create a graph: `lightstone.Graph(settings={"sps": 8})`.

---

## 5. Building a graph yourself

The same link (without the equalizer), built from scratch, with a power meter at the fiber output:

```python
g = lightstone.Graph(settings={"sps": 8})
g.add("src", "prbs13q").add("drv", "driver").add("vcsel", "vcsel")
g.add("con", "connector", offset=3e-6)
g.add("fib", "mmf", type="om4", length=100.0)
g.add("pin", "pin").add("tia", "tia").add("rxf", "bessel_thomson").add("rx", "receiver_output")
g.chain("src", "drv", "vcsel", "con", "fib", "pin", "tia", "rxf", "rx")

g.add("pm", "power_meter").wire("fib", "pm")
g.run()
print(g.power("pm"))                  # 0.00092: average optical power after the fiber [W]
```

| Method | Does |
|---|---|
| `add(id, kind, **params)` | Add a node |
| `wire("a.out", "b.in")` | Connect two ports. A bare id means port `out` (source) or `in` (target): `wire("fib", "pm")` |
| `chain(a, b, c, …)` | `a.out → b.in`, `b.out → c.in`, … |
| `insert(before, id, kind, **params)` | Put a node with ports `in`/`out` into the wire that feeds `before`: `insert("pin", "att", "attenuator", loss_db=3)` places an attenuator in front of the photodiode |
| `replace(id, kind, **params)` | Change a node's kind, keep its wires |
| `set(id, **params)` | Update parameters |
| `remove(id)` | Remove a node and its wires |
| `unwire("b.in")` | Remove the wire into an input |

All of them return the graph, so calls can be chained.

**Which kinds exist?** `lightstone.builtin_kinds()` lists them: `prbs13q`, `driver`, `vcsel`, `connector`, `mmf` and `custom_fiber` (Section 5.1), `attenuator`, `splitter`, `combiner`, `probe`, `power_meter`, `pin`, `tia`, `bessel_thomson`, `ffe`, `receiver_output`, the WDM components `wdm_mux`, `wdm_demux` and `bandpass_filter` ([`model/wdm-mux.md`](model/wdm-mux.md), [`wdm-demux.md`](model/wdm-demux.md), [`bandpass-filter.md`](model/bandpass-filter.md)), and the analysis nodes `pam4_analyzer`, `eye_diagram`, `tdecq` (Section 6.1), `optical_spectrum_analyzer` and `electrical_spectrum_analyzer` ([`model/optical-spectrum-analyzer.md`](model/optical-spectrum-analyzer.md), [`electrical-spectrum-analyzer.md`](model/electrical-spectrum-analyzer.md)), and `rust_example`, an example component written in Rust ([`model/rust-example.md`](model/rust-example.md)).

**Which ports does a node have?** `g.ports()` shows every node's ports and types:

```python
g.ports()["pin"]
# {'kind': 'pin', 'linear': True,
#  'inputs':  [{'name': 'in',  'type': 'optical', 'monitor': False}],
#  'outputs': [{'name': 'out', 'type': 'electrical (current)', 'monitor': False}]}
```

Most components have one input `in` and one output `out`. Exceptions: `splitter` (`out0`, `out1`, …), `combiner` (`in0`, `in1`, …), `probe` (`out` and `tap`), and the sinks `power_meter` and `receiver_output` (no outputs).

### 5.1 Fibers: presets and custom

There are two fiber kinds:

- **`mmf`** (*Multimode* in the browser): a standard multimode fiber, `type` = `"om3"`, `"om4"` or `"om5"`. Each preset is a **minimum-spec** fiber: its effective modal bandwidth at 850 nm with the default VCSEL launch sits at the type's minimum (OM3 2000, OM4 and OM5 4700 MHz·km), the worst fiber that still carries the name. OM5 also keeps its bandwidth up to 953 nm. Only `length`, `attenuation_db_per_km` (≤ 3.5 dB/km, the cabled limit) and `extra_loss_db` belong to it.
- **`custom_fiber`** (*Custom*): every parameter is free: core radius, NA, profile (graded with α, or step), core glass, attenuation, solver grid. `lightstone.fiber_presets()` gives each preset's full set of custom parameters as a starting point.

```python
g = lightstone.Graph.reference()                  # el1 is an mmf, type "om4"
g.set("el1", type="om3")                          # switch the preset
g.set("el1", profile={"kind": "graded", "alpha": 2.05})
# UserWarning: node 'el1': converted the OM3 preset to a custom fiber (physics changed: profile)
```

Changing the physics of a preset turns it into a custom fiber with the preset's values, so an `"om4"` node always means the calibrated OM4. The browser does the same and offers *Back to OM3*. Graphs saved with the old kind `fiber` load as `custom_fiber`. The model and its calibration: [`model/mmf.md`](model/mmf.md).

**Mistakes** raise a `GraphError` that names the node:

```python
g.add("att", "attenuator")     # not wired
g.run()
# GraphError: att: input 'in' is not connected
```

```python
g.remove("att")
g.add("att2", "attenuator").wire("fib", "att2")   # fib.out already feeds pin
g.run()
# GraphError: fib: optical output 'out' feeds 2 components; light cannot be copied.
#             Insert a splitter (or a probe for analyzers).
```

---

## 6. Looking inside: reports and outputs

Besides its output signals, a node can produce a **report** with side information:

| Node kind | `g.report(id)` contains |
|---|---|
| `vcsel` | Power levels, transmitted power waveform, VCSEL bandwidth |
| `mmf`, `custom_fiber` | Number of modes, V-number; at the first span also launch efficiency, EMB and OFL bandwidth |
| `pin` | Detected optical power waveform; per source: transmission, mean delay, RMS delay spread |
| `bessel_thomson` | Noise bandwidth |
| `power_meter` | Average power |
| `receiver_output` | Noiseless and noisy waveform, σ(t), `gaussian_exact` |
| `ffe` | Taps, main cursor, design phase and MSE |
| `pam4_analyzer`, `eye_diagram`, `tdecq` | Analysis results (Section 6.1) |

```python
g.report("el1")["emb_length"] / 1e9     # 4710 MHz·km (reference link: OM4 preset)
g.report("pin")["channels"][0]["rms_delay_spread"]   # 3.6e-12 s at 70 m
```

`g.output(id, port)` gives the value on any output port: the symbols of a data port, explicit waveforms of an electrical port, the average power of an optical port. This works like an oscilloscope probe on any wire:

```python
v = g.output("tia")           # TIA output, before the receive filter
v["noiseless"], v["signal"], v["sigma"]
```

`g.warnings()` lists the warnings of the last run as (node, message).

### 6.1 Analysis nodes

The analysis can also be part of the graph itself. Analysis nodes are sinks: they observe a signal and produce a report, without changing anything. Each needs two inputs: the signal (`in`) and the transmitted symbols from the pattern source (`ref`), which it uses to align and to count errors.

| Kind | Input `in` | Report (`g.report(id)`) |
|---|---|---|
| `pam4_analyzer` | electrical | `type: "pam4_analysis"`: SER, BER, eye heights and widths, levels, thresholds, `q_min`, `ser_method`, `ser_resolution`, `ser_floor` (an SER of 0 means below it), `noise_rms` (0: noise-free signal) |
| `eye_diagram` | electrical | `type: "eye_diagram"`: `counts` as a [height, width] array, voltage range, sampling phase |
| `tdecq` | optical | `type: "tdecq"`: TDECQ in dB, equalizer taps, OMA, thresholds |

```python
g = lightstone.Graph.reference()
g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")
g.add("eye", "eye_diagram").wire("ffe", "eye").wire("src", "eye.ref")
g.add("tq", "tdecq").wire("el1", "tq").wire("src", "tq.ref")      # optical: at the fiber output
g.run()
print(g.report("ana")["ser"], g.report("ana")["ser_method"])      # 1.9e-07 semi_analytic
print(g.report("tq")["tdecq_db"])                                 # 3.26
counts = g.report("eye")["counts"]                                # shape (400, 600)
```

Wiring the same output into several analyzers is fine. Electrical signals can be copied, and analysis inputs only observe, so even the optical `el1.out` can feed both the photodiode and `tdecq`.

The results are the same as `rx.analyze()` and `res.tdecq()`, but now they are part of the graph: they are cached like any other node, saved with the graph's JSON, and the browser editor will be able to show them. Like `analyze()`, `pam4_analyzer` switches to the counted SER after a nonlinear block.

### 6.2 The receiver equalizer (`ffe`)

At 100 m the eye before the equalizer is closed (eye heights −43 to −53 mV, SER 0.10). Real receivers for these links use a **feed-forward equalizer** (FFE): a short filter with symbol-spaced taps that undoes most of the intersymbol interference. The reference graph has one, node `ffe` between `rxf` and `rx`:

```python
g = lightstone.Graph.reference()
g.add("pre", "pam4_analyzer").wire("rxf", "pre").wire("src", "pre.ref")    # before the FFE
g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")    # after it
g.run()
g.report("pre")["ser"], g.report("ana")["ser"]   # 0.0995, 1.9e-07
g.report("ana")["eye_height"]                    # 78 / 71 / 61 mV
g.report("ffe")["taps"]                          # [0.067, -0.344, 1.495, -0.287, 0.069]
```

`g.set("ffe", n_taps=9)` gives SER 9.1 × 10⁻⁸; the examples below keep 5 taps.

- **Adaptation.** Without `taps`, the node chooses them itself (`adapted: True` in its report): it finds the taps that bring the received samples closest to the ideal PAM-4 levels, the minimum mean-square error. By default (`noise_aware=True`) it also accounts for the noise the taps amplify. It uses the known symbols (`ref`), like the training sequence in a real receiver. The taps sum to 1, so the levels keep their scale.
- **Fixed taps.** `ffe` with `taps=[…]` uses your taps instead; `main` sets which one is the main cursor.
- **Still linear.** After adapting, the taps are fixed, so the equalizer is a linear filter. The noise stays exactly described, and the SER stays semi-analytic. That is how it can show an SER of 2 × 10⁻⁷ from 8191 symbols.
- **Without an equalizer.** `Graph.reference(lane_config={"rx": {"ffe_taps": 0}})` (or `simulate({"rx.ffe_taps": 0})`) builds the lane without it, with `rxf` wired straight to `rx`. The NRZ validations V14a, V14c and V16b do that: they compare the unequalized eye with the IEEE link model. In your own graph, `g.insert("ana", "ffe2", "ffe", n_taps=5).wire("src", "ffe2.ref")` puts an FFE in front of a single node.

The physics, the design equation and the limits (ideal training, no DFE) are in [`model/ffe.md`](model/ffe.md). Section 7.2 shows how to write an equalizer with your own taps as a custom component.

### 6.3 NRZ instead of PAM-4

With `levels=2` the pattern source sends NRZ: one PRBS13 period (8191 bits) instead of PRBS13Q. The driver, the VCSEL, the analyzer, the eye diagram and the FFE follow automatically. The VCSEL keeps the same outer power levels, so average power, extinction ratio and OMA_outer do not change. The reference lane at 25.78 GBd (`tx.levels` in a lane configuration sets the source):

```python
g = lightstone.Graph.reference(lane_config={"tx": {"levels": 2, "baud": 25.78125e9, "sps": 8}})
g.add("ana", "pam4_analyzer").wire("rxf", "ana").wire("src", "ana.ref")
g.run()
g.report("vcsel")["levels"]                  # [0.000569, 0.001431] W: P₀ and P₃ of the PAM-4 default
a = g.report("ana")
a["thresholds"], a["eye_height"], a["ber"]   # one threshold 0.461 V, one eye 0.284 V, BER 0.0
a["ser_floor"], a["q_min"]                  # 2.3e-19: the 0 means SER < 2.3e-19 (Q ≈ 11)
```

For NRZ the analyzer reports two levels and one eye, and BER equals SER (PAM-4: SER/2 with Gray coding). TDECQ is defined for PAM-4 only: a `tdecq` node with an NRZ reference stops the run with `GraphError: tq: TDECQ is defined for PAM-4 only ...`. In a graph built by hand, set it on the source, `g.set("src", levels=2)`; in the browser, the pattern source has a *Modulation* selector. Validation V14a uses this NRZ path to compare the eye closure with the IEEE 802.3ae link model (docs/validation.md, Section 2).

### 6.4 Transmitter and receiver jitter

The driver can move the edges of the drive: random jitter `rj_rms`, duty-cycle distortion `dcd` (peak-to-peak) and sinusoidal jitter `sj_amplitude` at `sj_frequency`, all in seconds. Continuing the graph of 6.2 with a `tdecq` node on the fiber output:

```python
g.add("tq", "tdecq").wire("el1", "tq").wire("src", "tq.ref")
g.run()                                       # SER 1.9e-7, TDECQ 3.26 dB
g.set("drv", rj_rms=0.4e-12, dcd=1e-12)
g.run()
g.report("ana")["ser"], g.report("tq")["tdecq_db"]   # 6.0e-7, 3.55 dB
g.set("ana", clock_rj_rms_ui=0.01)            # receiver clock jitter (analyzer option, UI RMS)
g.run()
g.report("ana")["ser"]                        # 6.7e-7
```

- **Random jitter stays semi-analytic.** It is carried as a noise term whose strength follows the slope of the drive at each edge, so σ(t) stays exact and the SER can still show 10⁻⁷ from 8191 symbols. The approximation is first order: keep `rj_rms` well below the edge rise time (the driver warns above 0.05 UI).
- **DCD and SJ are exact edge shifts.** SJ is referenced to a clock recovery with corner `cdr_corner` (default 4 MHz), and its frequency is rounded to the record's frequency grid (6.49 MHz for one pattern period at 53.125 GBd). The driver warns when it rounds.
- **TDECQ** includes transmitter jitter, as in the standard, but not the receiver clock jitter.

The model, its limits and validations V16a, V16b and V14c are in [`model/driver.md`](model/driver.md). With `lightstone.simulate`, the same parameters are `tx.jitter.rj_rms` and so on.

---

## 7. Your own components

You can add components written in Python. Every custom component answers two questions, and you always state both explicitly:

| | `domain="frequency"` | `domain="time"` |
|---|---|---|
| `linear=True` | `function(f, ctx)` returns the transfer function **H(f)** (7.1) | `function(t, ctx)` returns the **impulse response h(t)**, or you give **FIR taps** (7.2) |
| `linear=False` | not possible: a nonlinear component has no H(f) | `function(inputs, ctx)` works on **waveforms** (7.3) |

- **Linear** means the output is a fixed filter applied to the input. Lightstone then applies the filter itself and keeps track of the noise exactly. **Nonlinear** means the output depends on the signal level (clipping, saturation, a decision).
- **Domain** says how you describe the component: by its frequency response or by what it does in time.

A linear filter can be described either way: H(f) and h(t) are a Fourier pair. Pick whichever you have. A formula in f (an RC low-pass) suits `frequency`. A measured pulse response, a pulse shape or equalizer taps suit `time`. The result is the same (Section 7.2).

There are no defaults: leaving out `linear` or `domain` is an error. This keeps it unambiguous whether a function receives frequencies or times.

The examples in this section use the reference link **without** its built-in FFE, so the effect of each component on the 100 m signal is visible directly (before equalization the SER is 0.10):

```python
NO_FFE = {"rx": {"ffe_taps": 0}}      # lane configuration: rxf wired straight to rx
```

### 7.1 Linear, frequency domain: describe H(f)

Example: a first-order RC low-pass, $H(f) = 1/(1 + jf/f_c)$:

```python
@lightstone.component("rc_filter",
                      inputs={"in": "electrical"}, outputs={"out": "electrical"},
                      linear=True, domain="frequency")
def rc(f, ctx):
    """First-order RC low-pass."""
    return 1 / (1 + 1j * f / ctx.params.get("fc", 30e9))

g = lightstone.Graph.reference(lane_config=NO_FFE)
g.insert("rx", "rc", "rc_filter", fc=25e9)     # between rxf and rx
rx = g.run().receiver()
print(rx.analyze()["ser"], rx.ser_method)       # 0.231  semi_analytic
```

What happens:

- `@lightstone.component(...)` registers a new kind, `"rc_filter"`. From then on every graph can use it by name.
- Your function receives the frequencies `f` of the simulation grid in Hz (a NumPy array that includes negative frequencies) and the context `ctx`. It returns H at those frequencies: an array, or a single number for a flat gain.
- `ctx.params` holds the node's parameters (here `fc`). `ctx` also has `baud`, `sps`, `sample_rate`, `n`, `n_symbols`, `seed` and `time`.
- Lightstone multiplies the signal **and every noise term** by H. It therefore still knows exactly how much noise is left, and the precise semi-analytic SER stays valid (`ser_method` is still `semi_analytic`).

The extra filter lowers the bandwidth, so the SER rises from 0.10 to 0.23. With `g.set("rc", fc=15e9)` it rises to 0.29. Only `rc` and `rx` are recomputed.

Two rules for H(f):

1. **Real impulse response:** H(−f) must equal the complex conjugate of H(f). Every formula written with `1j * f`, like the one above, satisfies this automatically. Something like a constant `1 + 0.1j` does not, and is rejected.
2. **Ports:** exactly one input and one output, both electrical or both optical. On optical ports H acts on the optical **power** and is the same for all fiber modes (an attenuator is `return 10 ** (-loss_db / 10)`).

### 7.2 Linear, time domain: impulse response or FIR taps

The **impulse response** h(t) is the output for an infinitely short input pulse of unit area. It describes a linear filter completely, just like H(f). Lightstone turns it into H(f) for you.

**A continuous impulse response.** Example: a Gaussian pulse response with RMS width σ:

```python
@lightstone.component("gauss_pulse",
                      inputs={"in": "electrical"}, outputs={"out": "electrical"},
                      linear=True, domain="time")
def gauss(t, ctx):
    """Gaussian impulse response, area 1 (DC gain 1)."""
    s = ctx.params.get("sigma", 8e-12)
    return np.exp(-t**2 / (2 * s**2)) / (np.sqrt(2 * np.pi) * s)

g = lightstone.Graph.reference(lane_config=NO_FFE)
g.insert("rx", "gp", "gauss_pulse", sigma=8e-12)
print(g.run().receiver().analyze()["ser"])      # 0.289, semi_analytic
```

- Your function receives the times `t` [s] of one pattern period, **including negative times**: the grid is periodic, so the second half of the samples stands for t < 0. That is why a centered (non-causal) pulse like this Gaussian works.
- It returns h(t) in **1/s**. The area ∫h dt is the DC gain, so a filter that passes DC unchanged has area 1, as here.
- Signals stay lazy and the noise stays exact, just as with H(f). Described in the frequency domain, $H(f) = e^{-2\pi^2\sigma^2 f^2}$, the same filter gives results equal to within 10⁻¹².

**FIR taps.** A discrete filter, typically an equalizer, is a list of dimensionless taps c₀, c₁, …. Tap i delays the signal by i taps and weights it with cᵢ, and the sum of the taps is the DC gain. Since there is no function, use `Component` with `taps=` and say how far apart the taps are with `tap_spacing`: `"symbol"` (one unit interval, as in the IEEE reference equalizer) or `"sample"`:

```python
ffe = lightstone.Component("ffe3",
                           inputs={"in": "electrical"}, outputs={"out": "electrical"},
                           linear=True, domain="time",
                           taps=lambda ctx: ctx.params["taps"], tap_spacing="symbol")

g = lightstone.Graph.reference(lane_config=NO_FFE)
g.use(ffe).insert("rx", "fir", "ffe3", taps=[-0.15, 1.3, -0.15])
a = g.run().receiver().analyze()
print(a["ser"], a["eye_height"])                # 6.3e-04, eye heights ≈ +8…19 mV
```

This fixed 3-tap feed-forward equalizer opens the 100 m eye that is closed without equalization: the SER drops from 0.10 to 6.3 × 10⁻⁴, and the eye heights go from about −45 mV to +8…19 mV. Three fixed taps are not enough for a worst-case OM4 fiber at 100 m; the adaptive 5-tap `ffe` of the reference link (Section 6.2) reaches 2 × 10⁻⁷. `taps` can be a fixed list or, as here, a function of `ctx`, so the taps become node parameters. The first tap has no delay, so this equalizer delays the signal by one symbol. The analysis finds the delay by itself.

**What to watch out for.** Lightstone samples h(t) on the simulation grid. That is exact as long as two conditions hold, and it warns at the node when they don't:

| Condition | Warning | What to do |
|---|---|---|
| h(t) has decayed within half a pattern period (± 77 ns) | "has not decayed …: it wraps around" | Use a faster response or more `pattern_periods` |
| h(t) is smooth on the sample scale (band-limited to fs/2) | "not band-limited … aliases" | Typical cause: a jump, e.g. an RC response `exp(−t/τ)` starting abruptly at t = 0. Describe it in the frequency domain instead, or use more samples per symbol |

The RC low-pass from 7.1 is an example of the second case. In the time domain its impulse response jumps at t = 0, so Lightstone warns that sampling it aliases (|H(fs/2)| ≈ 9 % of the peak at 8 samples per symbol). For such filters, the frequency domain is the better choice. FIR taps are discrete by definition and never alias.

### 7.3 Nonlinear components: work on waveforms

If the output depends on the signal level, there is no H(f) and no impulse response. Your function then works on explicit waveforms, so the domain is always `"time"`:

```python
@lightstone.component("limiter",
                      inputs={"in": "voltage"}, outputs={"out": "voltage"},
                      linear=False, domain="time")
def limiter(inputs, ctx):
    """Hard limiter between low and high [V]."""
    lo, hi = ctx.params.get("low", 0.30), ctx.params.get("high", 0.60)
    return {"out": inputs["in"].map(lambda v: np.clip(v, lo, hi))}

g = lightstone.Graph.reference(lane_config=NO_FFE)
g.insert("rxf", "lim", "limiter")               # between tia and rxf
a = g.run().receiver().analyze()
print(a["ser"], a["ser_method"], a["ser_resolution"])
# 0.110  monte_carlo  0.000122
```

What happens:

- `inputs` is a dict: input port name → `Waveform`. A `Waveform` holds the **noiseless** waveform (`.noiseless`), one **noisy** realization (`.noisy`), and the analytic noise `.sigma`.
- You return a dict: output port name → `Waveform`. `x.map(f)` applies `f` to both the noiseless and the noisy waveform, which is what a memoryless nonlinearity needs. You can also return a tuple `(noiseless, noisy)` of arrays.
- Lightstone produces these waveforms for you before calling your function, and turns your result back into a signal afterwards.

**Why `monte_carlo`?** The noise has gone through the limiter, and clipping cuts off its tails. It is no longer Gaussian, so σ(t) no longer describes it, and the semi-analytic SER would be wrong. Lightstone notices this and switches automatically: `analyze()` now **counts** the actual errors in the noisy waveform. Counting has a limit. With 8191 symbols the smallest nonzero SER it can show is 1/8191 ≈ 1.2 × 10⁻⁴ (`ser_resolution`). For lower SERs, raise `pattern_periods` in the settings. The `receiver_output` node also warns about it:

```python
g.warnings()
# [('rx', 'Noise has passed a nonlinear block, so σ(t) does not describe it and the
#          semi-analytic SER is not valid. Use the counted (Monte Carlo) SER; it resolves
#          SER ≳ 1.2e-4 with 8191 symbols.')]
```

Rule of thumb: declare a component linear whenever it really is. It is faster, and the SER stays precise.

### 7.4 The built-in alternative: TIA clipping

The TIA has clipping built in, which gives exactly the same result as the limiter above:

```python
g.remove("lim").wire("tia", "rxf").set("tia", clip=[0.30, 0.60])
g.run().receiver().analyze()["ser"]        # 0.110, monte_carlo
g.warnings()                               # also: ('tia', 'TIA output clipped in 23.939 % of samples.')
```

`set("tia", clip=None)` turns it off again.

### 7.5 Notes on custom components

- **Per-graph components.** Instead of the global decorator, create a `lightstone.Component(kind, inputs=…, outputs=…, linear=…, domain=…, function=…)` (or `taps=…, tap_spacing=…`) and pass it to one graph: `lightstone.Graph.reference(components=[my_comp])` or `g.use(my_comp)`.
- **Port types** for `inputs`/`outputs`: `data`, `electrical` (any), `current`, `voltage`, `drive`, `optical`.
- **Errors in your code** stop the run with a `GraphError` that names the node and includes the Python traceback.
- **Caching.** Lightstone cannot see when your function's code changes. Whenever a component kind is registered with a graph (the first time it is used, or after `use()` with a new version), that graph's cache is cleared and every node runs again once. That is why inserting `rc` above recomputed all 10 nodes. Changing only its parameters afterwards recomputes just the downstream nodes.
- **Validation status.** Custom components are your own physics: Lightstone does not validate them.
- **A complete example in a file.** `examples/components/example_component.py` (gain and delay, $H(f) = 1$ by default), its model doc [`model/example-component.md`](model/example-component.md) (writing, loading and wiring it) and the notebook `notebooks/custom_component.ipynb` (inserted into the reference graph; gain, delay, and a CTLE variant that replaces the FFE). The same block written in Rust, `rust_example` ([`model/rust-example.md`](model/rust-example.md), with a guide to writing Rust components), is compiled into the engine and also appears in the browser editor (palette group *Custom*).
- **Not supported yet:** nonlinear components with optical ports (put them after the photodiode, or make them linear), and optical H(f) that differs per fiber mode.

---

## 8. Saving and loading

A graph is a plain JSON document: settings, nodes with parameters, wires.

```python
open("link.json", "w").write(g.to_json())
g2 = lightstone.Graph.from_json(open("link.json").read())
```

```json
{
  "settings": { "baud": 53125000000.0, "sps": 16, "pattern_periods": 1, "resolution": "mode", ... },
  "nodes": [
    { "id": "src", "kind": "prbs13q", "params": { "seed": 2731 } },
    { "id": "drv", "kind": "driver", "params": {} },
    ...
  ],
  "wires": [ { "from": "src.out", "to": "drv.in" }, ... ]
}
```

The browser's graph editor reads and writes the same format (Section 8.1). An optional `layout` entry stores the node positions of the editor; the engine ignores it, and Python keeps it when saving. Large noise seeds (above 2⁵³ − 1, as in the reference link) appear as strings, `"seed": "5355905856850362371"`, so that browser and Python draw the same noise; you can give seeds as numbers or strings. A saved graph that uses custom kinds only stores their names. The components themselves must be defined (decorator or `components=[…]`) before the graph runs.

### 8.1 The same graph in the browser

The browser app (`web/`, see the README) is a graph editor running the same engine in WebAssembly:

- **☰ Menu**: *Save graph as…*, *Load graph…* (a JSON file, also one saved in Python with `g.to_json()`), *New empty graph*, and **Examples**: the start page (the reference link with an eye diagram and a PAM-4 analyzer, `examples/graphs/pam4-100m-om4.json`; shown to a new visitor), the bare reference link as the engine generates it, and an SWDM4 WDM link (4 channels 850–940 nm over 100 m OM5, with an optical spectrum analyzer and an eye diagram and SER per channel; `examples/graphs/swdm4-wdm-link.json`). The current graph is autosaved in the browser.
- **↶ Undo / ↷ Redo** (Ctrl+Z, Ctrl+Shift+Z or Ctrl+Y) step through every edit, including loading a graph; dragging one slider counts as one step.
- **Add** components from the palette. A new node is placed after the selected one and wired to it where possible.
- **Wire** by dragging from an output port (right side of a node) to an input port (left side). Port colors show the signal type: optical, electrical, data. Incompatible ports do not connect. A second wire from an optical output offers to insert a splitter.
- **Select** a node to edit its parameters, wire its inputs from select boxes, see its results (SER, TDECQ, the eye of an `eye_diagram` node, the signal at each output) and its model document.
- **Results on the canvas.** Measuring nodes (power meter, analyzer, TDECQ) show an instrument display, spectrum analyzers their trace, and the eye diagram node its eye. Drag a selected node's corners to make it larger: the display, trace or eye grows with it, and the size is saved with the graph.
- **Frames** (Transmitter, Channel, …) group nodes; dragging a frame moves its nodes, and **Duplicate** copies a frame with its nodes, e.g. to add a lane.
- **Run** records the results; a **sweep** varies one parameter and records every point. Save the runs as JSON or CSV.
- Every run also records the value at each **output port**: average power of optical outputs; mean, swing and RMS noise of electrical outputs; the symbol count of data outputs. The inspector lists them under *Outputs*, and hovering a wire shows the value at its source. CSV and the run comparison show the instrument results; tick **include all port values** to add every port. A power meter is still the explicit measurement point, but you no longer need one just to see a power level.
- **Save graph as…** writes the graph, including node positions, sizes and frames, so that `lightstone.Graph.from_json(...)` can run it in Python. In Chromium-based browsers a save dialog asks for folder and name; elsewhere only the name is asked for and the file goes to the download folder (browsers do not let a page choose the folder there).

A results file from the browser can be read in Python with the standard library (format in docs/architecture.md, Section 7.9):

```python
import json
study = json.load(open("lightstone-results.json"))
for run in study["runs"]:
    print(run["label"], run["reports"]["ana"]["ser"])        # node results as shown in the editor
    print(run["ports"]["rxf"]["out"]["swing"])                # value at an output port [V]
g = lightstone.Graph(study["runs"][0]["graph"])               # the exact graph of a run
```

---

## 9. Cheat sheet

```python
g = lightstone.Graph.reference()               # reference link (src … el0, el1 … rxf, ffe, rx)
g = lightstone.Graph(settings={"sps": 8})      # empty graph
g.add(id, kind, **params); g.chain(a, b, c); g.wire("a.out", "b.in")
g.insert("pin", "att", "attenuator", loss_db=3); g.replace(id, kind); g.set(id, **params); g.remove(id)
g.validate()                                   # [] if the graph can run
g.run()                                        # cached evaluation; g.computed_last_run
rx = g.receiver("rx"); a = rx.analyze()       # a["ser"], a["ser_method"], a["ser_resolution"]
g.report(id); g.output(id, port); g.power(pm_id); g.warnings(); g.ports()
g.to_json(); lightstone.Graph.from_json(text)

g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")   # analysis nodes (6.1)
g.set("ffe", n_taps=9)                                                    # the reference FFE (6.2); rx.ffe_taps: 0 removes it

# linear and domain are always required
@lightstone.component(kind, inputs={...}, outputs={...}, linear=True, domain="frequency")
def h(f, ctx): return ...                      # H(f); engine-applied, SER stays semi-analytic

@lightstone.component(kind, inputs={...}, outputs={...}, linear=True, domain="time")
def h_t(t, ctx): return ...                    # h(t) [1/s], t includes negative times; ∫h dt = DC gain

lightstone.Component(kind, inputs={...}, outputs={...}, linear=True, domain="time",
                     taps=[c0, c1, ...], tap_spacing="symbol")   # FIR ("symbol" or "sample")

@lightstone.component(kind, inputs={...}, outputs={...}, linear=False, domain="time")
def proc(inputs, ctx): return {"out": inputs["in"].map(fn)}   # waveforms, SER becomes Monte Carlo
```

| Term | Meaning |
|---|---|
| Node / kind / params | One component / its type / its settings |
| Port | Named input or output with a signal type (data, electrical, optical) |
| Wire | Connection `node.port → node.port` |
| Monitor input | Input that only observes (power meter, receiver output); optical fan-out to it is allowed |
| Report | Side information a node produces (e.g. EMB of a fiber, SER of an analyzer) |
| Analysis node | Sink that turns a signal into diagnostics (`pam4_analyzer`, `eye_diagram`, `tdecq`); needs the transmitted symbols at `ref` |
| FFE | Feed-forward equalizer: symbol-spaced taps that undo intersymbol interference |
| Linear component | Output = fixed filter of the input; Lightstone applies it itself |
| Domain | How a component is described: `frequency` (H(f)) or `time` (impulse response, taps, waveforms) |
| Impulse response h(t) | Output for a unit-area input pulse; the time-domain description of a linear filter, Fourier pair of H(f) |
| FIR taps | Discrete impulse response: weights cᵢ at delays i·spacing |
| Nonlinear component | Output depends on the signal level; works on explicit waveforms |
| `gaussian_exact` | True while all blocks after the noise sources are linear; then σ(t) is exact |
| Semi-analytic / Monte Carlo SER | Computed from σ(t) (precise, any SER) / counted errors (needed after nonlinear blocks, SER ≥ 1/N_sym) |

Further reading: [`architecture.md`](architecture.md), Section 7 (graph engine, status in 7.7), [`signal-spec.md`](signal-spec.md) Section 6 (linear and nonlinear components), and `tests/python/test_graph.py` for more examples.

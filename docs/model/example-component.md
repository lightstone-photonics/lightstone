---
component: example_component
title: Example custom component (gain and delay)
python_only: true
validation: []
references: []
---

## What it models

An electrical gain and delay block, written as an example of a custom component. It multiplies the electrical signal at its input by a constant gain and delays it by a fixed time. With the default parameters it passes its input unchanged, $H(f) = 1$. It is not a built-in kind: it lives in `examples/components/example_component.py` and has to be loaded (see below). The notebook `notebooks/custom_component.ipynb` uses it. The browser cannot run Python, so it is not in the editor; the same block written in Rust, which is, is `rust-example.md`.

## Equations

**Transfer function** (`eq:example-component`):

$$y(t) = g\thinspace x(t - \tau),\qquad H(f) = g\thinspace e^{-j2\pi f\tau}$$

Here $x(t)$ is the input and $y(t)$ the output waveform at time $t$ (s), both in the unit of the wire (V after the TIA, A after the photodiode). $g$ is the dimensionless gain (`gain`, default 1), $\tau$ the delay in s (`delay`, default 0), $H(f)$ the transfer function at frequency $f$ (Hz) and $j$ the imaginary unit.

The block is linear, so Lightstone applies $H(f)$ to the signal and to every noise term. The noise stays exactly Gaussian: a gain scales the signal and the noise by the same factor and leaves the SER unchanged, and a delay only moves the waveform in time (the analysis re-aligns).

| Parameter | Symbol | Unit | Default |
|---|---|---|---|
| `gain` | $g$ | — | 1 |
| `delay` | $\tau$ | s | 0 |

Ports: input `in` and output `out`, both electrical (any quantity).

## How to create and load a custom component

**1. Write it.** A custom component is a `lightstone.Component` with ports, `linear` and `domain`, and a function. For a linear component described in the frequency domain the function receives the grid frequencies `f` (Hz, FFT order, negative frequencies included) and a context `ctx`, and returns $H(f)$. This is the whole example (`examples/components/example_component.py`):

```python
import numpy as np
import lightstone

def transfer(f, ctx):
    gain = float(ctx.params.get("gain", 1.0))     # node parameter, dimensionless
    delay = float(ctx.params.get("delay", 0.0))   # node parameter, s
    return gain * np.exp(-2j * np.pi * f * delay)

example_component = lightstone.Component(
    "example_component",
    inputs={"in": "electrical"}, outputs={"out": "electrical"},
    linear=True, domain="frequency", function=transfer,
)
```

- `ctx.params` holds the node's parameters from the graph; `ctx` also has `baud`, `sps`, `sample_rate`, `n`, `n_symbols`, `seed` and `time`.
- $H(-f)$ must be the complex conjugate of $H(f)$ (a real impulse response). Formulas in `1j * f` satisfy this.
- Other forms: an impulse response h(t) or FIR taps (`linear=True, domain="time"`), or a function on waveforms for nonlinear blocks (`linear=False, domain="time"`). See [`tutorial-component-graph.md`](../tutorial-component-graph.md), Section 7.

**2. Load it.** Import the file, then give the component to a graph:

```python
import importlib.util
spec = importlib.util.spec_from_file_location("example_component", "examples/components/example_component.py")
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

g = lightstone.Graph.reference(components=[mod.example_component])   # or g.use(mod.example_component)
```

If the file is on `sys.path`, `from example_component import example_component` does the same. `lightstone.register(mod.example_component)` makes the kind available to every graph instead of one.

**3. Wire it in.** `insert(before, id, kind, **params)` puts a node with ports `in`/`out` into the wire that feeds input `before`:

```python
g.insert("ffe", "ex", "example_component")    # rxf → ex → ffe
g.run()                                       # H = 1: receiver output bit-identical to the reference graph
g.set("ex", gain=0.5).run()                   # only ex, ffe and rx are recomputed
```

Without `insert`, use `add` and `wire`: `g.unwire("ffe.in").add("ex", "example_component").wire("rxf", "ex").wire("ex", "ffe")`.

**4. Save and reload.** A saved graph (`g.to_json()`) stores only the kind name `example_component`. Load the component again (step 2) before running a graph read with `Graph.from_json(text, components=[...])`.

## Assumptions & limits

- Electrical only, flat gain and pure delay: no bandwidth limit, no noise of its own, no saturation.
- A delay that is not a whole number of samples is applied as a linear phase on the periodic grid (band-limited interpolation, exact for the periodic signal).
- Custom components are not validated by Lightstone. Python components do not run in the browser editor (that needs Pyodide, docs/architecture.md, 7.4).

## Validation

- With the defaults, the receiver output of the reference graph with the block inserted is bit-identical to the reference graph (`notebooks/custom_component.ipynb`, step 2).
- `tests/python/test_graph.py`: it gives the same receiver output as the Rust example `rust_example` (`rust-example.md`), with defaults and with gain and delay set.

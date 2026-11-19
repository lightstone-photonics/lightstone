---
component: rust_example
title: Example component in Rust (gain and delay)
validation: []
references: []
---

## What it models

An electrical gain and delay block, written in Rust as a template for your own components. It multiplies the electrical signal at its input by a constant gain and delays it by a fixed time. With the default parameters it passes its input unchanged, $H(f) = 1$. Code: `crates/lightstone-core/src/example.rs`, node kind `rust_example`. Because it is compiled into the engine, it is available everywhere: in Python (`g.add("ex", "rust_example")`), in lanes and in the browser editor (palette group *Custom*). The same block written in Python is `example-component.md`.

## Equations

**Transfer function** (`eq:rust-example`):

$$y(t) = g\,x(t - \tau),\qquad H(f) = g\,e^{-j2\pi f\tau}$$

Here $x(t)$ is the input and $y(t)$ the output waveform at time $t$ (s), both in the unit of the wire (V after the TIA, A after the photodiode). $g$ is the dimensionless gain (`gain`, default 1), $\tau$ the delay in s (`delay`, default 0), $H(f)$ the transfer function at frequency $f$ (Hz) and $j$ the imaginary unit.

The block is linear, so the engine applies $H(f)$ to the signal and to every noise term. The noise stays exactly Gaussian: a gain scales the signal and the noise by the same factor and leaves the SER unchanged, and a delay only moves the waveform in time (the analysis re-aligns).

| Parameter | Symbol | Unit | Default | Editor slider |
|---|---|---|---|---|
| `gain` | $g$ | — | 1 | 0 … 2 |
| `delay` | $\tau$ | s | 0 | 0 … 20 ps |

Ports: input `in` and output `out`, both electrical (any quantity; the output keeps the input's quantity).

## How to create a component in Rust

A component is a Rust type that implements the `Component` trait (`crates/lightstone-core/src/component.rs`). Built-in and custom components implement the same trait. The steps below follow `example.rs`; copy it and rename.

**1. Parameters.** A serde struct with `#[serde(default)]` and a `Default` impl. The node's `params` from the graph JSON are deserialized into it, missing fields take the defaults, and the editor shows the defaults for a new node.

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RustExampleParams {
    pub gain: f64,   // dimensionless
    pub delay: f64,  // s
}
impl Default for RustExampleParams {
    fn default() -> Self { Self { gain: 1.0, delay: 0.0 } }
}
```

**2. The component and its constructor.** It holds the parsed parameters and checks them; an `Err(String)` shows up as an issue at the node.

```rust
pub struct RustExample(pub RustExampleParams);

impl RustExample {
    pub fn new(params: &serde_json::Value) -> Result<Self, String> { /* parse, validate */ }
    pub fn response(&self, f: f64) -> Complex64 {
        Complex64::from_polar(self.0.gain, -2.0 * PI * f * self.0.delay)
    }
}
```

**3. Implement `Component`.**

```rust
impl Component for RustExample {
    fn inputs(&self) -> Vec<PortSpec> { vec![PortSpec::new("in", PortType::Electrical(None))] }
    fn outputs(&self) -> Vec<PortSpec> { vec![PortSpec::new("out", PortType::Electrical(None))] }
    fn linearity(&self) -> Linearity { Linearity::Linear }
    fn transfer(&self, ctx: &mut ProcessCtx) -> Option<Result<Transfer, String>> {
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        let h = (0..n).map(|k| self.response(freq(k, n, fs))).collect();
        Some(Ok(Transfer { h: Arc::new(h), warnings: vec![] }))
    }
    fn process(&self, _: &mut ProcessCtx, _: &[Signal]) -> Result<Output, String> {
        Err("applied through its transfer function".into())
    }
}
```

- **Ports**: `PortType::Data` (symbols), `PortType::Electrical(None)` (any quantity) or `Electrical(Some(Quantity::Voltage))` etc., and `PortType::Optical`. `PortSpec::monitor(...)` declares an input that only observes.
- **Linear with `transfer`** (as here): one electrical or optical input and one output of the same kind. Return H(f) on the simulation grid in FFT bin order (`dsp::freq(k, n, fs)`); the engine checks $H(-f) = H(f)^*$, applies it to the signal and all noise terms, and does not call `process`. `ctx.grid` has `baud`, `sps`, `n_symbols`, `n()` and `fs()`. Warnings in `Transfer::warnings` appear at the node.
- **Everything else** (several ports, reports, nonlinear blocks) implements `process(ctx, inputs)`: `inputs` holds one `Signal` per declared input in order (`inputs[0].as_electrical()?`, `.as_data()?`), and it returns an `Output` with one signal per declared output (`Output::signals(vec![...])`), an optional `report` and `warnings`. See `Bt` (Bessel–Thomson filter) or `Attenuator` in `components.rs`. Return `Linearity::Nonlinear` when the output depends on the signal level.

**4. Register the kind.** In `register_builtins` (`components.rs`):

```rust
r.register_builtin(crate::example::KIND, |p, _| Ok(Box::new(crate::example::RustExample::new(p)?)));
```

and its default parameters in `default_params` (same file), so editors can create the node:

```rust
crate::example::KIND => v(serde_json::to_value(crate::example::RustExampleParams::default())),
```

The factory receives the node's `params` and a `NodeInfo` (node id and graph settings, e.g. for a default noise seed). Built-in kinds cannot be replaced by Python custom components. Code outside this repository can instead build its own registry, `let mut r = lightstone_analysis::nodes::registry(); r.register("my_kind", |p, _| ...)`, and run graphs with `GraphEngine::with_registry(graph, r)`.

**5. Show it in the browser editor.** In `web/src/lib/graph/catalog.js`: the kind in a palette group of `GROUPS` (here *Custom*), its name in `LABEL` (and optionally `PALETTE_LABEL`), the id prefix in `PREFIX`, the doc id in `DOC`, sliders in `SPECS` (path, label, unit, min, max, step, digits, scale; display = SI value / scale) and a node summary in `badge()`. A symbol goes into `GraphSymbol.svelte`; without one the node shows a box with "?".

**6. Document and test it.** One doc in `docs/model/<doc id>.md` with front matter (`component`, `title`, `validation`, `references`), equations with stable IDs that the code comments reference (here `eq:rust-example`); `web/test/docs.test.mjs` checks that every kind has its doc. Unit tests go next to the code (`#[cfg(test)]` in `example.rs`).

**7. Build.** `cargo test -p lightstone-core --release`; for Python `(cd crates/lightstone-py && maturin develop --release)`; for the browser `web/scripts/build-wasm.sh`, then `npm run build` in `web/`.

## Assumptions & limits

- Electrical only, flat gain and pure delay: no bandwidth limit, no noise of its own, no saturation.
- A delay that is not a whole number of samples is applied as a linear phase on the periodic grid (band-limited interpolation, exact for the periodic signal). A delay longer than half the pattern period wraps around; the node warns.

## Validation

- Unit tests in `example.rs`: the defaults give $H(f) = 1$; $H(f) = g\,e^{-j2\pi f\tau}$ and $H(-f) = H(f)^*$ to 10⁻¹⁵; invalid parameters are rejected; inserted into the reference link between the receive filter and the FFE, the defaults leave the FFE output bit-identical, and gain 0.5 halves the noiseless waveform and σ(t) to 10⁻¹².
- `tests/python/test_graph.py`: the Rust example and the Python example component (`example-component.md`) give the same receiver output.

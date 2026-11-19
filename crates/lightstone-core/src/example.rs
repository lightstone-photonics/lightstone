//! Example component written in Rust (node kind `rust_example`, `docs/model/rust-example.md`):
//! an electrical gain and delay block,
//!
//! `eq:rust-example` — H(f) = g · exp(−j·2π·f·τ)
//!
//! With the defaults (g = 1, τ = 0 s) H(f) = 1: the block passes its input unchanged. It is a
//! template for your own components: parameters (serde struct with defaults), ports,
//! linearity, the transfer function, registration and tests. The model doc walks through each
//! step.

use crate::component::{
    Component, Linearity, Output, PortSpec, PortType, ProcessCtx, Signal, Transfer,
};
use crate::dsp::freq;
use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::sync::Arc;

/// Node kind of the example.
pub const KIND: &str = "rust_example";

/// Parameters of the node (`params` in the graph JSON). `#[serde(default)]`: missing fields
/// take the values of [`Default`], so `{}` is a valid node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RustExampleParams {
    /// Gain g (dimensionless).
    pub gain: f64,
    /// Delay τ \[s\].
    pub delay: f64,
}

impl Default for RustExampleParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            delay: 0.0,
        }
    }
}

/// The component: holds its parsed parameters. One instance per node and parameter set.
pub struct RustExample(pub RustExampleParams);

impl RustExample {
    /// Build from the node's JSON parameters (called by the registry).
    pub fn new(params: &serde_json::Value) -> Result<Self, String> {
        let p: RustExampleParams = if params.is_null() {
            RustExampleParams::default()
        } else {
            serde_json::from_value(params.clone())
                .map_err(|e| format!("invalid parameters: {e}"))?
        };
        if !p.gain.is_finite() || !p.delay.is_finite() {
            return Err("gain and delay must be finite numbers".into());
        }
        Ok(Self(p))
    }

    /// H(f) at frequency `f` \[Hz\] (`eq:rust-example`).
    pub fn response(&self, f: f64) -> Complex64 {
        Complex64::from_polar(self.0.gain, -2.0 * PI * f * self.0.delay)
    }
}

impl Component for RustExample {
    /// One electrical input `in` (any quantity: current, voltage or drive).
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", PortType::Electrical(None))]
    }
    /// One electrical output `out`; `None` keeps the input's quantity.
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", PortType::Electrical(None))]
    }
    /// Linear: the engine applies H(f) to the signal and to every noise term, so σ(t) stays
    /// exact and the semi-analytic SER stays valid.
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    /// H(f) on the simulation grid, in FFT bin order (`dsp::freq`). The engine checks
    /// H(−f) = H(f)* and applies it; `process` is then not called.
    fn transfer(&self, ctx: &mut ProcessCtx) -> Option<Result<Transfer, String>> {
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        let h: Vec<Complex64> = (0..n).map(|k| self.response(freq(k, n, fs))).collect();
        let mut warnings = Vec::new();
        let period = ctx.grid.n_symbols as f64 / ctx.grid.baud;
        if self.0.delay.abs() > 0.5 * period {
            warnings.push(format!(
                "Delay {:.3} ns exceeds half the pattern period ({:.3} ns); the periodic signal \
                 wraps around.",
                self.0.delay * 1e9,
                0.5 * period * 1e9
            ));
        }
        Some(Ok(Transfer {
            h: Arc::new(h),
            warnings,
        }))
    }
    /// Not used: a linear component that returns a transfer function is applied by the engine.
    fn process(&self, _: &mut ProcessCtx, _: &[Signal]) -> Result<Output, String> {
        Err("rust_example is applied through its transfer function".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphConfig, GraphEngine, NodeConfig, WireConfig};
    use crate::signal::{Level, Resolution};
    use crate::system::{lane_graph, Element, LaneConfig};

    /// Reference link (fast settings), optionally with a `rust_example` node `ex` between the
    /// receive filter `rxf` and the FFE.
    fn reference(example: Option<serde_json::Value>) -> GraphEngine {
        let mut lane = LaneConfig::default();
        lane.tx.sps = 8;
        for e in lane.elements.iter_mut() {
            if let Element::Fiber(f) = e {
                f.grid_points_per_radius = 300;
            }
        }
        let mut g: GraphConfig = lane_graph(&lane, Resolution::Mode, Level::Power);
        if let Some(params) = example {
            g.nodes.push(NodeConfig {
                id: "ex".into(),
                kind: KIND.into(),
                params,
            });
            for w in g.wires.iter_mut().filter(|w| w.to == "ffe.in") {
                w.from = "ex.out".into();
            }
            g.wires.push(WireConfig {
                from: "rxf.out".into(),
                to: "ex.in".into(),
            });
        }
        let mut e = GraphEngine::new(g);
        e.run().expect("graph runs");
        e
    }

    /// Noiseless waveform and σ(t) at `node.out`.
    fn wave(e: &mut GraphEngine, node: &str) -> (Vec<f64>, Vec<f64>) {
        let s = e
            .output(node, "out")
            .unwrap()
            .as_electrical()
            .unwrap()
            .clone();
        let st = &e.graph().settings;
        let fs = st.baud * st.sps as f64;
        let sp = e.spectral();
        (s.noiseless(&sp), s.noise_sigma(&sp, fs))
    }

    #[test]
    fn defaults_give_unity_transfer() {
        let c = RustExample::new(&serde_json::json!({})).unwrap();
        for f in [0.0, 1e9, -3e10, 5e10] {
            assert_eq!(c.response(f), Complex64::new(1.0, 0.0));
        }
    }

    #[test]
    fn transfer_is_gain_and_delay_phase() {
        let (g, tau, f) = (0.7, 4.7e-12, 2.5e10);
        let c = RustExample::new(&serde_json::json!({"gain": g, "delay": tau})).unwrap();
        let h = c.response(f);
        let want = Complex64::new(0.0, -2.0 * PI * f * tau).exp() * g;
        assert!((h - want).norm() < 1e-15);
        assert!((c.response(-f) - h.conj()).norm() < 1e-15);
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        assert!(RustExample::new(&serde_json::json!({"gain": "x"})).is_err());
        assert!(RustExample::new(&serde_json::json!({"gain": f64::NAN})).is_err());
    }

    /// In the reference graph: H = 1 is bit-identical; gain 0.5 halves signal and noise.
    #[test]
    fn in_the_reference_graph() {
        let mut base = reference(None);
        let mut unity = reference(Some(serde_json::json!({})));
        assert_eq!(wave(&mut base, "ffe"), wave(&mut unity, "ffe"));
        let mut half = reference(Some(serde_json::json!({"gain": 0.5})));
        let ((y0, s0), (y1, s1)) = (wave(&mut base, "rxf"), wave(&mut half, "ex"));
        for i in 0..y0.len() {
            assert!((y1[i] - 0.5 * y0[i]).abs() <= 1e-12 * y0[i].abs().max(1e-3));
            assert!((s1[i] - 0.5 * s0[i]).abs() <= 1e-12 * s0[i].abs().max(1e-9));
        }
    }
}

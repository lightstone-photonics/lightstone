//! Graph nodes built on the analysis layer (docs/architecture.md, 7.7): analyzers that turn
//! signals into diagnostics inside the graph, and the adaptive receiver FFE.
//!
//! | kind | inputs → outputs | report |
//! |---|---|---|
//! | `pam4_analyzer` | `in` (electrical), `ref` (data) | PAM-4 metrics; semi-analytic or counted SER |
//! | `eye_diagram` | `in` (electrical), `ref` (data) | 2-UI eye density |
//! | `tdecq` | `in` (optical), `ref` (data) | TDECQ through the reference receiver |
//! | `ffe` | `in` (electrical), `ref` (data) → `out` (electrical) | taps, cursor, phase, MSE |
//!
//! `ref` is the transmitted symbol stream (the pattern source), used for alignment and as the
//! decision reference. All inputs are monitor inputs except the FFE signal path.
//!
//! The core crate cannot depend on this crate, so these kinds are added to a registry with
//! [`register`]; [`registry`] returns the core built-ins plus these nodes.

use crate::eye_density;
use crate::pam4::{analyze_levels, ber_from_ser, AnalysisOptions};
use crate::tdecq::{tdecq, TdecqOptions};
use lightstone_core::component::*;
use lightstone_core::dsp::BesselThomson4;
pub use lightstone_core::ffe::{design_ffe, FfeDesign, FfeParams};
use lightstone_core::graph::Registry;
use num_complex::Complex64;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Register the analysis nodes as built-in kinds.
pub fn register(r: &mut Registry) {
    r.register_builtin("pam4_analyzer", |p, _| Ok(Box::new(Pam4Node(parse(p)?))));
    r.register_builtin("eye_diagram", |p, _| Ok(Box::new(EyeNode(parse(p)?))));
    r.register_builtin("tdecq", |p, _| Ok(Box::new(TdecqNode(parse(p)?))));
    r.register_builtin("optical_spectrum_analyzer", |p, _| {
        Ok(Box::new(crate::spectrum::Osa::new(parse(p)?)?))
    });
    r.register_builtin("electrical_spectrum_analyzer", |p, _| {
        Ok(Box::new(crate::spectrum::Esa::new(parse(p)?)?))
    });
}

/// Default parameters of a built-in kind (core or analysis), as JSON.
pub fn default_params(kind: &str) -> Option<serde_json::Value> {
    let v = |x: serde_json::Result<serde_json::Value>| x.expect("serializable");
    match kind {
        "pam4_analyzer" => Some(v(serde_json::to_value(AnalysisOptions::default()))),
        "eye_diagram" => Some(v(serde_json::to_value(EyeParams::default()))),
        "tdecq" => Some(v(serde_json::to_value(TdecqOptions::default()))),
        "optical_spectrum_analyzer" => Some(v(serde_json::to_value(
            crate::spectrum::OsaParams::default(),
        ))),
        "electrical_spectrum_analyzer" => Some(v(serde_json::to_value(
            crate::spectrum::EsaParams::default(),
        ))),
        _ => lightstone_core::components::default_params(kind),
    }
}

/// Core built-ins plus the analysis nodes.
pub fn registry() -> Registry {
    let mut r = Registry::builtin();
    register(&mut r);
    r
}

fn parse<T: DeserializeOwned + Default>(v: &serde_json::Value) -> Result<T, String> {
    if v.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid parameters: {e}"))
}

fn data_report(kind: &str, value: serde_json::Value) -> Output {
    Output {
        report: Some(Report::Data {
            kind: kind.into(),
            value: Arc::new(value),
        }),
        ..Default::default()
    }
}

/// Reference symbols and their alphabet size (2 = NRZ, 4 = PAM-4).
fn symbols(s: &Signal) -> Result<(&[u8], u8), String> {
    let d = s.as_data()?;
    if d.levels != 4 && d.levels != 2 {
        return Err("the reference must be a PAM-4 or NRZ symbol stream".into());
    }
    Ok((&d.symbols, d.levels))
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn analyzer_inputs(ty: PortType) -> Vec<PortSpec> {
    vec![
        PortSpec::monitor("in", ty),
        PortSpec::monitor("ref", PortType::Data),
    ]
}

fn json<T: Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("serializable")
}

// ---------------------------------------------------------------------------------------------
// PAM-4 analyzer and eye diagram
// ---------------------------------------------------------------------------------------------

struct Pam4Node(AnalysisOptions);

impl Component for Pam4Node {
    fn inputs(&self) -> Vec<PortSpec> {
        analyzer_inputs(PortType::Electrical(None))
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Nonlinear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let e = inputs[0].as_electrical()?;
        let (sym, levels) = symbols(&inputs[1])?;
        let m = e.materialize(&ctx.spectral, ctx.grid.fs());
        let sps = ctx.grid.sps;
        // Semi-analytic SER while σ(t) is exact, counted (Monte Carlo) SER otherwise.
        let mut out = if m.gaussian_exact {
            let a = analyze_levels(
                &m.signal,
                Some((&m.noiseless, &m.sigma)),
                sym,
                levels,
                sps,
                &self.0,
            );
            let mut v = json(&a);
            v["ser_method"] = "semi_analytic".into();
            v["ser_resolution"] = serde_json::Value::Null;
            // Smallest SER the semi-analytic method shows (a 0 means below it), and the RMS
            // of σ(t): 0 means a noise-free signal, where SER 0 is exact.
            v["ser_floor"] = crate::pam4::ser_floor().into();
            v["noise_rms"] = rms(&m.sigma).into();
            data_report("pam4_analysis", v)
        } else {
            let mut a = analyze_levels(&m.signal, None, sym, levels, sps, &self.0);
            a.ser = Some(a.ser_counted);
            a.ber = ber_from_ser(a.ser_counted, levels);
            let mut v = json(&a);
            v["ser_method"] = "monte_carlo".into();
            v["ser_resolution"] = (1.0 / sym.len() as f64).into();
            v["ser_floor"] = (1.0 / sym.len() as f64).into();
            v["noise_rms"] = rms(&m.sigma).into();
            data_report("pam4_analysis", v)
        };
        if !m.gaussian_exact {
            out.warnings.push(format!(
                "Counted (Monte Carlo) SER: noise has passed a nonlinear block. It resolves \
                 SER ≳ {:.1e} with {} symbols.",
                1.0 / sym.len() as f64,
                sym.len()
            ));
        }
        Ok(out)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct EyeParams {
    width: usize,
    height: usize,
    /// Vertical range \[low, high\]; `None` = waveform range with 5 % margin.
    v_range: Option<[f64; 2]>,
    phases_per_ui: usize,
}

impl Default for EyeParams {
    fn default() -> Self {
        Self {
            width: 600,
            height: 400,
            v_range: None,
            phases_per_ui: AnalysisOptions::default().phases_per_ui,
        }
    }
}

struct EyeNode(EyeParams);

impl Component for EyeNode {
    fn inputs(&self) -> Vec<PortSpec> {
        analyzer_inputs(PortType::Electrical(None))
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Nonlinear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let p = &self.0;
        if p.width < 2 || p.height < 2 {
            return Err("eye width and height must be ≥ 2".into());
        }
        let e = inputs[0].as_electrical()?;
        let (sym, levels) = symbols(&inputs[1])?;
        let m = e.materialize(&ctx.spectral, ctx.grid.fs());
        let sps = ctx.grid.sps;
        let opts = AnalysisOptions {
            phases_per_ui: p.phases_per_ui,
            ..Default::default()
        };
        let model = m
            .gaussian_exact
            .then_some((m.noiseless.as_slice(), m.sigma.as_slice()));
        let a = analyze_levels(&m.signal, model, sym, levels, sps, &opts);
        let eye = eye_density(
            &m.signal,
            sps,
            a.delay,
            a.phase,
            p.width,
            p.height,
            p.v_range.map(|r| (r[0], r[1])),
        );
        let mut v = json(&eye);
        v["delay"] = a.delay.into();
        v["phase"] = a.phase.into();
        Ok(data_report("eye_diagram", v))
    }
}

// ---------------------------------------------------------------------------------------------
// TDECQ
// ---------------------------------------------------------------------------------------------

/// TDECQ of an optical signal: detected power (all light collected, incl. RIN, no receiver
/// noise) through the reference receiver (4th-order Bessel–Thomson at `ref_f3db_rel`·baud).
struct TdecqNode(TdecqOptions);

impl Component for TdecqNode {
    fn inputs(&self) -> Vec<PortSpec> {
        analyzer_inputs(PortType::Optical)
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Nonlinear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let s = &inputs[0].as_optical()?.forward;
        let (sym, levels) = symbols(&inputs[1])?;
        if levels != 4 {
            return Err(
                "TDECQ is defined for PAM-4 only; the reference is an NRZ stream \
                        (use pam4_analyzer for NRZ eye and SER)"
                    .into(),
            );
        }
        let grid = ctx.grid;
        let (n, fs) = (grid.n(), grid.fs());
        let bt = BesselThomson4::new(self.0.ref_f3db_rel * grid.baud);
        let mut spec = vec![Complex64::new(0.0, 0.0); n];
        for c in &s.contributions {
            let h = c.transfer(None, &grid);
            let e = c.source.emitted_spec();
            for k in 0..n {
                spec[k] += e[k] * h[k];
            }
        }
        for (k, x) in spec.iter_mut().enumerate() {
            *x *= bt.response(lightstone_core::dsp::freq(k, n, fs));
        }
        let w = ctx.spectral.inverse_real(spec);
        let r = tdecq(&w, sym, grid.sps, grid.baud, &self.0);
        Ok(data_report("tdecq", json(&r)))
    }
}

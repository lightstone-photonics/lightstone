//! Custom components (docs/architecture.md, Section 7.4).
//!
//! A custom component declares its ports, whether it is linear, and a behavior supplied by a
//! binding (Python) or by Rust code:
//!
//! * **Linear, frequency domain:** one electrical or optical input and one output of the same
//!   kind. The behavior returns H(f) on the simulation grid; the engine applies it and signals
//!   stay lazy ([`Component::transfer`]).
//! * **Linear, time domain:** same ports. The behavior returns an impulse response
//!   ([`ImpulseResponse`]): a continuous h(t) evaluated on the grid, or FIR taps. The engine
//!   converts it to H(f) on the periodic grid (exact, `eq:custom-impulse`) and applies it as
//!   above.
//! * **Nonlinear** (time domain only): the engine materializes the inputs into explicit waveforms
//!   ([`Waveform`]), the behavior computes explicit output waveforms, and the outputs are
//!   wrapped as signals again. Noise that passes the block is carried as an explicit
//!   realization, and `gaussian_exact` becomes false (signal-spec 6).
//!
//! Nonlinear custom components with optical ports would need materialized optical signals
//! downstream (signal-spec 3.3), which the built-in components do not consume yet. They are
//! rejected at registration.

use crate::component::*;
use crate::components::default_seed;
use crate::dsp::{freq, time, Spectral};
use crate::graph::{NodeInfo, Registry};
use crate::signal::{DataSignal, ElectricalSignal, Quantity, TimeGrid};
use num_complex::Complex64;
use std::rc::Rc;
use std::sync::Arc;

/// Seed XOR for the default seed of custom components.
const CUSTOM_SEED_XOR: u64 = 0x4355_0000_0000_0003;

/// Declaration of a custom component kind.
#[derive(Clone, Debug)]
pub struct CustomSpec {
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub linear: bool,
    /// Domain in which the behavior describes the component. Nonlinear components are always
    /// `Time`.
    pub domain: Domain,
    /// Documentation for the math panel (Markdown + LaTeX).
    pub doc: String,
}

/// Port type from its declaration name: `data`, `electrical` (any quantity), `current`,
/// `voltage`, `drive` or `optical`.
pub fn port_type_from_name(s: &str) -> Result<PortType, String> {
    Ok(match s {
        "data" => PortType::Data,
        "electrical" => PortType::Electrical(None),
        "current" => PortType::Electrical(Some(Quantity::Current)),
        "voltage" => PortType::Electrical(Some(Quantity::Voltage)),
        "drive" => PortType::Electrical(Some(Quantity::Dimensionless)),
        "optical" => PortType::Optical,
        _ => {
            return Err(format!(
                "unknown port type '{s}' (data, electrical, current, voltage, drive, optical)"
            ))
        }
    })
}

#[derive(serde::Deserialize)]
struct PortDecl {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

#[derive(serde::Deserialize)]
struct SpecDecl {
    inputs: Vec<PortDecl>,
    outputs: Vec<PortDecl>,
    linear: bool,
    domain: String,
    #[serde(default)]
    doc: String,
}

impl CustomSpec {
    /// Declaration from JSON (shared by the Python and WASM bindings):
    /// `{inputs: [{name, type}], outputs: [...], linear, domain: "time" | "frequency", doc?}`.
    pub fn from_json(json: &str) -> Result<CustomSpec, String> {
        let d: SpecDecl =
            serde_json::from_str(json).map_err(|e| format!("invalid component spec: {e}"))?;
        let ports = |v: &[PortDecl]| -> Result<Vec<PortSpec>, String> {
            v.iter()
                .map(|p| Ok(PortSpec::new(&p.name, port_type_from_name(&p.ty)?)))
                .collect()
        };
        let domain = match d.domain.as_str() {
            "time" => Domain::Time,
            "frequency" => Domain::Frequency,
            o => return Err(format!("domain must be 'time' or 'frequency', not '{o}'")),
        };
        Ok(CustomSpec {
            inputs: ports(&d.inputs)?,
            outputs: ports(&d.outputs)?,
            linear: d.linear,
            domain,
            doc: d.doc,
        })
    }
}

/// Domain in which a custom component is described.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    /// H(f) (linear components only).
    Frequency,
    /// Impulse response (linear) or explicit waveforms (nonlinear).
    Time,
}

/// Spacing of FIR taps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapSpacing {
    /// One tap per sample (1/fs).
    Sample,
    /// One tap per symbol (T = 1/baud), e.g. a T-spaced feed-forward equalizer.
    Symbol,
}

/// Impulse response of a linear time-domain component.
#[derive(Clone, Debug, PartialEq)]
pub enum ImpulseResponse {
    /// Continuous impulse response h(t) \[1/s\] at the times `t` passed to
    /// [`CustomBehavior::impulse`]. ∫h dt is the DC gain.
    Continuous(Vec<f64>),
    /// Dimensionless FIR taps c₀, c₁, …: tap i at delay i·spacing, Σc is the DC gain.
    Taps { taps: Vec<f64>, spacing: TapSpacing },
}

/// Relative level of h(t) in the outer 10 % of the window above which it counts as not
/// decayed (it wraps around the periodic grid).
const DECAY_TOL: f64 = 1e-6;

/// |H(fs/2)| relative to the peak of |H| above which a sampled continuous h(t) counts as not
/// band-limited: aliasing then changes H noticeably (≳ 0.1 %).
const ALIAS_TOL: f64 = 1e-3;

/// H(f) on the grid from an impulse response (`eq:custom-impulse`).
///
/// The grid is periodic, so the discrete impulse response g\[n\] and H\[k\] = DFT(g) describe
/// the same circular convolution exactly:
/// * continuous h(t): g\[n\] = h(tₙ)·Δt with tₙ = [`time`]`(n)` (negative times wrapped to
///   the end of the period). h must be band-limited to fs/2, otherwise it aliases.
/// * taps: g\[i·s\] = cᵢ with stride s = 1 (sample spacing) or samples per symbol.
///
/// Returns H and warnings. Real g gives H(−f) = H(f)* by construction.
pub fn transfer_from_impulse(
    ir: &ImpulseResponse,
    grid: &TimeGrid,
    sp: &Spectral,
) -> Result<(Vec<Complex64>, Vec<String>), String> {
    let n = grid.n();
    let mut warnings = Vec::new();
    let continuous = matches!(ir, ImpulseResponse::Continuous(_));
    let g = match ir {
        ImpulseResponse::Continuous(h) => {
            if h.len() != n {
                return Err(format!(
                    "impulse response has {} samples, the simulation grid has {n}",
                    h.len()
                ));
            }
            if h.iter().any(|x| !x.is_finite()) {
                return Err("impulse response contains NaN or infinite values".into());
            }
            let peak = h.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            let edge = (0..n)
                .filter(|&k| time(k, n, 1.0).abs() >= 0.45 * n as f64)
                .fold(0.0f64, |m, k| m.max(h[k].abs()));
            if peak > 0.0 && edge > DECAY_TOL * peak {
                warnings.push(format!(
                    "The impulse response has not decayed within ±{:.1} ns (half the \
                     pattern period): |h| at the window edges is {:.1e} of its peak. It wraps \
                     around the periodic grid; use more pattern periods or a shorter response.",
                    0.5 * n as f64 / grid.fs() * 1e9,
                    edge / peak
                ));
            }
            let dt = 1.0 / grid.fs();
            h.iter().map(|x| x * dt).collect::<Vec<f64>>()
        }
        ImpulseResponse::Taps { taps, spacing } => {
            if taps.is_empty() {
                return Err("FIR taps must not be empty".into());
            }
            if taps.iter().any(|x| !x.is_finite()) {
                return Err("FIR taps contain NaN or infinite values".into());
            }
            let stride = match spacing {
                TapSpacing::Sample => 1,
                TapSpacing::Symbol => grid.sps,
            };
            if (taps.len() - 1) * stride >= n {
                return Err(format!(
                    "{} taps do not fit into one pattern period ({n} samples)",
                    taps.len()
                ));
            }
            let mut g = vec![0.0; n];
            for (i, c) in taps.iter().enumerate() {
                g[i * stride] = *c;
            }
            g
        }
    };
    let h = sp.forward(&g);
    // A sampled h(t) is only exact if h is band-limited to fs/2; otherwise the spectrum
    // aliases (sampled H = Σₘ H(f − m·fs)). Taps are discrete by definition and not checked.
    if continuous && n.is_multiple_of(2) {
        let peak = h.iter().fold(0.0f64, |m, c| m.max(c.norm()));
        let nyq = h[n / 2].norm();
        if peak > 0.0 && nyq > ALIAS_TOL * peak {
            warnings.push(format!(
                "The impulse response is not band-limited to fs/2 = {:.0} GHz (|H(fs/2)| is \
                 {:.1e} of its peak), so sampling it aliases. A jump in h(t), e.g. at t = 0, \
                 causes this. Describe the component in the frequency domain, or use more \
                 samples per symbol.",
                0.5 * grid.fs() / 1e9,
                nyq / peak
            ));
        }
    }
    Ok((h, warnings))
}

/// Context handed to the behavior.
pub struct CustomCtx<'a> {
    pub node: &'a str,
    pub params: &'a serde_json::Value,
    pub grid: TimeGrid,
    /// Seed for components that draw random numbers (derived from the graph seed and node id).
    pub seed: u64,
}

/// Explicit waveform on a port of a nonlinear custom component.
#[derive(Clone, Debug, PartialEq)]
pub enum Waveform {
    /// Symbol stream (one value per symbol).
    Data { symbols: Vec<u8>, levels: u8 },
    /// Noiseless and noisy waveform on the simulation grid. `sigma` and `gaussian_exact`
    /// describe the noise of inputs and are ignored on outputs.
    Electrical {
        quantity: Quantity,
        noiseless: Vec<f64>,
        noisy: Vec<f64>,
        sigma: Vec<f64>,
        gaussian_exact: bool,
    },
}

/// Behavior of a custom component.
pub trait CustomBehavior {
    /// H(f) of a linear frequency-domain component at the frequencies `f` \[Hz\] (FFT bin
    /// order).
    fn transfer(&self, _ctx: &CustomCtx, _f: &[f64]) -> Result<Vec<Complex64>, String> {
        Err("this component provides no transfer function".into())
    }
    /// Impulse response of a linear time-domain component. `t` \[s\] are the signed sample
    /// times of one period ([`time`]); a [`ImpulseResponse::Continuous`] response is
    /// evaluated at them, taps ignore them.
    fn impulse(&self, _ctx: &CustomCtx, _t: &[f64]) -> Result<ImpulseResponse, String> {
        Err("this component provides no impulse response".into())
    }
    /// Output waveforms of a nonlinear component, one per declared output.
    fn process(&self, _ctx: &CustomCtx, _inputs: Vec<Waveform>) -> Result<Vec<Waveform>, String> {
        Err("this component provides no process function".into())
    }
}

/// Check a declaration: unique port names, and the port rules of linear and nonlinear
/// components.
pub fn validate_spec(spec: &CustomSpec) -> Result<(), String> {
    for ports in [&spec.inputs, &spec.outputs] {
        for (i, p) in ports.iter().enumerate() {
            if p.name.is_empty() || p.name.contains('.') {
                return Err(format!(
                    "port name '{}' must be non-empty and must not contain '.'",
                    p.name
                ));
            }
            if ports[..i].iter().any(|q| q.name == p.name) {
                return Err(format!("duplicate port name '{}'", p.name));
            }
        }
    }
    let kind = |t: &PortType| match t {
        PortType::Data => 0,
        PortType::Electrical(_) => 1,
        PortType::Optical => 2,
    };
    if !spec.linear && spec.domain == Domain::Frequency {
        return Err(
            "a nonlinear component has no transfer function; describe it in the \
                    time domain (it works on explicit waveforms)"
                .into(),
        );
    }
    if spec.linear {
        let ok = spec.inputs.len() == 1
            && spec.outputs.len() == 1
            && kind(&spec.inputs[0].ty) == kind(&spec.outputs[0].ty)
            && kind(&spec.inputs[0].ty) != 0;
        if !ok {
            return Err(
                "a linear component needs exactly one input and one output, both \
                        electrical or both optical"
                    .into(),
            );
        }
    } else if spec
        .inputs
        .iter()
        .chain(&spec.outputs)
        .any(|p| p.ty == PortType::Optical)
    {
        return Err(
            "nonlinear custom components with optical ports are not supported yet \
                    (they need materialized optical signals). Declare the component linear \
                    with a transfer function H(f), or place it after the photodiode."
                .into(),
        );
    }
    Ok(())
}

/// Register a custom component kind. Built-in kinds cannot be replaced.
pub fn register_custom(
    registry: &mut Registry,
    kind: &str,
    spec: CustomSpec,
    behavior: Rc<dyn CustomBehavior>,
) -> Result<(), String> {
    if kind.is_empty() {
        return Err("component kind must be non-empty".into());
    }
    if registry.is_builtin(kind) || Registry::builtin().is_builtin(kind) {
        return Err(format!("'{kind}' is a built-in component kind"));
    }
    validate_spec(&spec)?;
    let spec = Arc::new(spec);
    registry.register(kind, move |params, info: &NodeInfo| {
        Ok(Box::new(Custom {
            spec: spec.clone(),
            behavior: behavior.clone(),
            params: params.clone(),
            seed: default_seed(info, CUSTOM_SEED_XOR),
        }))
    });
    Ok(())
}

struct Custom {
    spec: Arc<CustomSpec>,
    behavior: Rc<dyn CustomBehavior>,
    params: serde_json::Value,
    seed: u64,
}

impl Custom {
    fn ctx<'a>(&'a self, pctx: &'a ProcessCtx) -> CustomCtx<'a> {
        CustomCtx {
            node: pctx.node,
            params: &self.params,
            grid: pctx.grid,
            seed: self.seed,
        }
    }
}

impl Component for Custom {
    fn inputs(&self) -> Vec<PortSpec> {
        self.spec.inputs.clone()
    }
    fn outputs(&self) -> Vec<PortSpec> {
        self.spec.outputs.clone()
    }
    fn linearity(&self) -> Linearity {
        if self.spec.linear {
            Linearity::Linear
        } else {
            Linearity::Nonlinear
        }
    }

    fn transfer(&self, ctx: &mut ProcessCtx) -> Option<Result<Transfer, String>> {
        if !self.spec.linear {
            return None;
        }
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        let result = match self.spec.domain {
            Domain::Frequency => {
                let f: Vec<f64> = (0..n).map(|k| freq(k, n, fs)).collect();
                self.behavior
                    .transfer(&self.ctx(ctx), &f)
                    .map(|h| (h, Vec::new()))
            }
            Domain::Time => {
                let t: Vec<f64> = (0..n).map(|k| time(k, n, fs)).collect();
                self.behavior
                    .impulse(&self.ctx(ctx), &t)
                    .and_then(|ir| transfer_from_impulse(&ir, &ctx.grid, &ctx.spectral))
            }
        };
        Some(result.and_then(|(h, warnings)| {
            check_transfer(&h, n)?;
            Ok(Transfer {
                h: Arc::new(h),
                warnings,
            })
        }))
    }

    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        if self.spec.linear {
            return Err(
                "linear custom components are applied through their transfer function".into(),
            );
        }
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        // Automatic materialization of the inputs.
        let mut wf = Vec::with_capacity(inputs.len());
        for s in inputs {
            wf.push(match s {
                Signal::Data(d) => Waveform::Data {
                    symbols: d.symbols.as_ref().clone(),
                    levels: d.levels,
                },
                Signal::Electrical(e) => {
                    let m = e.materialize(&ctx.spectral, fs);
                    Waveform::Electrical {
                        quantity: e.quantity,
                        noiseless: m.noiseless,
                        noisy: m.signal,
                        sigma: m.sigma,
                        gaussian_exact: m.gaussian_exact,
                    }
                }
                Signal::Optical(_) => {
                    return Err(
                        "optical inputs cannot be materialized for custom components yet".into(),
                    )
                }
            });
        }
        let outs = self.behavior.process(&self.ctx(ctx), wf)?;
        if outs.len() != self.spec.outputs.len() {
            return Err(format!(
                "process returned {} outputs, the component declares {}",
                outs.len(),
                self.spec.outputs.len()
            ));
        }
        let mut signals = Vec::with_capacity(outs.len());
        for (w, port) in outs.into_iter().zip(&self.spec.outputs) {
            signals.push(match (w, port.ty) {
                (Waveform::Data { symbols, levels }, PortType::Data) => {
                    if symbols.len() != ctx.grid.n_symbols {
                        return Err(format!(
                            "output '{}' has {} symbols, the grid has {}",
                            port.name,
                            symbols.len(),
                            ctx.grid.n_symbols
                        ));
                    }
                    if levels == 0 || symbols.iter().any(|s| *s >= levels) {
                        return Err(format!(
                            "output '{}': symbols must be < the alphabet size {levels}",
                            port.name
                        ));
                    }
                    Signal::Data(Arc::new(DataSignal {
                        symbols: Arc::new(symbols),
                        levels,
                    }))
                }
                (
                    Waveform::Electrical {
                        quantity,
                        noiseless,
                        noisy,
                        ..
                    },
                    PortType::Electrical(q),
                ) => {
                    if noiseless.len() != n || noisy.len() != n {
                        return Err(format!(
                            "output '{}' must have {n} samples (noiseless and noisy)",
                            port.name
                        ));
                    }
                    if noiseless.iter().chain(&noisy).any(|x| !x.is_finite()) {
                        return Err(format!(
                            "output '{}' contains NaN or infinite values",
                            port.name
                        ));
                    }
                    Signal::Electrical(Arc::new(ElectricalSignal::from_waveforms(
                        q.unwrap_or(quantity),
                        &noiseless,
                        &noisy,
                        &ctx.spectral,
                    )))
                }
                _ => {
                    return Err(format!(
                        "output '{}' does not match its declared type {}",
                        port.name,
                        port.ty.name()
                    ))
                }
            });
        }
        Ok(Output::signals(signals))
    }
}

//! Component interface for the graph engine (docs/architecture.md, Section 7, signal-spec 6).

use crate::config::LaunchConfig;
use crate::coupling::{couple_launch, Coupling, DEFAULT_ANGULAR_POINTS};
use crate::dsp::Spectral;
use crate::fiber::{self, FiberChannel};
use crate::modes::{self, ModeSet, SolverParams};
use crate::signal::{
    DataSignal, ElectricalSignal, MaterializedElectrical, OpticalWave, Quantity, Settings,
    Spectrum, TimeGrid,
};
use crate::tx::TxOutput;
use num_complex::Complex64;
use std::sync::Arc;

/// Signal type of a port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortType {
    Data,
    /// Electrical; `Some(q)` restricts the quantity (current, voltage, …).
    Electrical(Option<Quantity>),
    Optical,
}

impl PortType {
    pub fn name(&self) -> String {
        match self {
            PortType::Data => "data".into(),
            PortType::Electrical(None) => "electrical".into(),
            PortType::Electrical(Some(q)) => format!("electrical ({q:?})").to_lowercase(),
            PortType::Optical => "optical".into(),
        }
    }

    /// Can an output of type `self` drive an input of type `to`?
    pub fn connects_to(&self, to: &PortType) -> bool {
        match (self, to) {
            (PortType::Data, PortType::Data) | (PortType::Optical, PortType::Optical) => true,
            (PortType::Electrical(a), PortType::Electrical(b)) => match (a, b) {
                (_, None) => true,
                (Some(x), Some(y)) => x == y,
                (None, Some(_)) => false,
            },
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PortSpec {
    pub name: String,
    pub ty: PortType,
    /// Input that only observes (analyzers, meters): optical fan-out to it is an ideal tap.
    pub monitor: bool,
}

impl PortSpec {
    pub fn new(name: &str, ty: PortType) -> Self {
        Self {
            name: name.into(),
            ty,
            monitor: false,
        }
    }

    pub fn monitor(name: &str, ty: PortType) -> Self {
        Self {
            name: name.into(),
            ty,
            monitor: true,
        }
    }
}

/// Linearity declaration (signal-spec 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linearity {
    /// Output is a fixed transfer function of the input; signals stay lazy.
    Linear,
    /// Output depends on signal level; inputs must be materialized.
    Nonlinear,
}

/// Value on a wire.
#[derive(Clone, Debug)]
pub enum Signal {
    Data(Arc<DataSignal>),
    Electrical(Arc<ElectricalSignal>),
    Optical(Arc<OpticalWave>),
}

impl Signal {
    pub fn as_data(&self) -> Result<&Arc<DataSignal>, String> {
        match self {
            Signal::Data(d) => Ok(d),
            _ => Err("expected a data signal".into()),
        }
    }
    pub fn as_electrical(&self) -> Result<&Arc<ElectricalSignal>, String> {
        match self {
            Signal::Electrical(e) => Ok(e),
            _ => Err("expected an electrical signal".into()),
        }
    }
    pub fn as_optical(&self) -> Result<&Arc<OpticalWave>, String> {
        match self {
            Signal::Optical(o) => Ok(o),
            _ => Err("expected an optical signal".into()),
        }
    }
}

/// Launch coupling result reported by the fiber span where a contribution enters the fiber.
#[derive(Clone, Debug)]
pub struct LaunchReport {
    /// Launch beam and connector offset/tilt that produced the coupling.
    pub launch: LaunchConfig,
    pub modes: Arc<ModeSet>,
    pub coupling: Arc<Coupling>,
    /// Modal bandwidth·length for this launch (≈ EMB) \[Hz·m\].
    pub emb_length: Option<f64>,
    /// Overfilled-launch bandwidth·length of the span's fiber \[Hz·m\].
    pub ofl_bandwidth_length: Option<f64>,
}

/// Accumulated delay of one mode at the detector (for mode reports).
#[derive(Clone, Debug)]
pub struct PathEntry {
    pub l: u32,
    pub m: u32,
    pub group: u32,
    /// Accumulated relative delay of the mode's mode bin \[s\].
    pub delay: f64,
}

#[derive(Clone, Debug)]
pub struct DetectionReport {
    /// Power-domain channel per source contribution.
    pub channels: Vec<FiberChannel>,
    /// Detected optical power before any filter \[W\].
    pub p_det: Arc<Vec<f64>>,
    /// Per-mode path delays of the first contribution (empty in free space).
    pub path: Vec<PathEntry>,
}

/// Side information a component reports in addition to its output signals.
#[derive(Clone, Debug)]
pub enum Report {
    Transmitter {
        tx: Arc<TxOutput>,
    },
    FiberSpan {
        basis: Arc<ModeSet>,
        launch: Option<Arc<LaunchReport>>,
    },
    Detection(Arc<DetectionReport>),
    Filter {
        noise_bandwidth: f64,
    },
    Receiver(Arc<MaterializedElectrical>),
    PowerMeter {
        average_power: f64,
    },
    /// Structured result of an analyzer or adaptive node (e.g. `pam4_analyzer`, `ffe`),
    /// as JSON. `kind` names the result type.
    Data {
        kind: String,
        value: Arc<serde_json::Value>,
    },
}

/// Result of processing one node.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub signals: Vec<Signal>,
    pub report: Option<Report>,
    pub warnings: Vec<String>,
}

impl Output {
    pub fn signals(signals: Vec<Signal>) -> Self {
        Self {
            signals,
            ..Default::default()
        }
    }
}

const CACHE_SIZE: usize = 8;

type CouplingKey = (SolverParams, LaunchConfig);
/// Coupling and the launch's modal bandwidth·length.
type CouplingValue = (Arc<Coupling>, Option<f64>);

fn lru_get<K: PartialEq, V: Clone>(v: &mut Vec<(K, V)>, k: &K) -> Option<V> {
    let pos = v.iter().position(|(kk, _)| kk == k)?;
    let e = v.remove(pos);
    let out = e.1.clone();
    v.push(e);
    Some(out)
}

fn lru_put<K, V>(v: &mut Vec<(K, V)>, k: K, val: V) {
    if v.len() >= CACHE_SIZE {
        v.remove(0);
    }
    v.push((k, val));
}

/// Expensive results shared by all nodes and kept across runs.
#[derive(Default)]
pub struct SharedCache {
    spectral: Option<Spectral>,
    modes: Vec<(SolverParams, Arc<ModeSet>)>,
    couplings: Vec<(CouplingKey, CouplingValue)>,
    ofl: Vec<(SolverParams, Option<f64>)>,
}

impl SharedCache {
    pub fn spectral(&mut self, n: usize) -> Spectral {
        match &self.spectral {
            Some(s) if s.len() == n => s.clone(),
            _ => {
                let s = Spectral::new(n);
                self.spectral = Some(s.clone());
                s
            }
        }
    }

    pub fn modes(&mut self, sp: &SolverParams) -> Arc<ModeSet> {
        if let Some(m) = lru_get(&mut self.modes, sp) {
            return m;
        }
        let m = Arc::new(modes::solve(sp));
        lru_put(&mut self.modes, sp.clone(), m.clone());
        m
    }

    /// Launch coupling and the launch's modal bandwidth·length (≈ EMB).
    pub fn coupling(
        &mut self,
        set: &ModeSet,
        sp: &SolverParams,
        launch: &LaunchConfig,
    ) -> (Arc<Coupling>, Option<f64>) {
        let key = (sp.clone(), launch.clone());
        if let Some(c) = lru_get(&mut self.couplings, &key) {
            return c;
        }
        let c = Arc::new(couple_launch(set, launch, DEFAULT_ANGULAR_POINTS));
        let w: Vec<f64> = c.modes.iter().map(|m| m.power).collect();
        let emb = fiber::modal_bandwidth_length(set, &w);
        lru_put(&mut self.couplings, key, (c.clone(), emb));
        (c, emb)
    }

    pub fn ofl(&mut self, set: &ModeSet, sp: &SolverParams) -> Option<f64> {
        if let Some(b) = lru_get(&mut self.ofl, sp) {
            return b;
        }
        let b = fiber::modal_bandwidth_length(set, &fiber::overfilled_weights(set));
        lru_put(&mut self.ofl, sp.clone(), b);
        b
    }
}

/// Context passed to [`Component::process`].
pub struct ProcessCtx<'a> {
    pub node: &'a str,
    pub settings: &'a Settings,
    pub grid: TimeGrid,
    pub spectral: Spectral,
    pub shared: &'a mut SharedCache,
}

/// A graph component. Built-in and custom components implement the same trait.
pub trait Component {
    fn inputs(&self) -> Vec<PortSpec>;
    fn outputs(&self) -> Vec<PortSpec>;
    fn linearity(&self) -> Linearity;
    /// Compute output signals from input signals (one per declared input, in order).
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String>;
    /// Transfer function H(f) on the simulation grid (FFT bin order, [`crate::dsp::freq`]).
    ///
    /// A `Linear` component with one electrical or optical input and one output of the same
    /// kind may return `Some`: the engine then applies H(f) itself and does not call
    /// [`Component::process`]. Electrical signals and all their noise terms get H appended;
    /// optical signals get it as a filter on power common to all mode bins (`h_extra`). Signals
    /// stay lazy either way.
    fn transfer(&self, _ctx: &mut ProcessCtx) -> Option<Result<Transfer, String>> {
        None
    }
}

/// Transfer function returned by [`Component::transfer`], with warnings for the node.
#[derive(Clone, Debug)]
pub struct Transfer {
    pub h: Spectrum,
    pub warnings: Vec<String>,
}

/// Relative tolerance of the conjugate-symmetry check in [`check_transfer`].
const HERMITIAN_TOL: f64 = 1e-9;

/// Check that H(f) has the grid length, is finite and has a real impulse response
/// (H(−f) = H(f)*). The Nyquist FFT bin of even grids is mapped to +fs/2 and not checked.
pub fn check_transfer(h: &[Complex64], n: usize) -> Result<(), String> {
    if h.len() != n {
        return Err(format!(
            "transfer function has {} points, the simulation grid has {n}",
            h.len()
        ));
    }
    if h.iter().any(|c| !c.re.is_finite() || !c.im.is_finite()) {
        return Err("transfer function contains NaN or infinite values".into());
    }
    let scale = h.iter().map(|c| c.norm()).fold(0.0, f64::max).max(1e-300);
    for k in 0..=n / 2 {
        if n.is_multiple_of(2) && k == n / 2 {
            continue;
        }
        if (h[k] - h[(n - k) % n].conj()).norm() > HERMITIAN_TOL * scale {
            return Err(format!(
                "transfer function must satisfy H(−f) = H(f)* (real impulse response); \
                 violated at FFT bin {k}"
            ));
        }
    }
    Ok(())
}

/// Apply a linear transfer function to a signal (engine side of [`Component::transfer`]).
pub fn apply_transfer(input: &Signal, h: &Spectrum, out: &PortSpec) -> Result<Signal, String> {
    match (input, out.ty) {
        (Signal::Electrical(e), PortType::Electrical(q)) => {
            check_transfer(h, e.base.len())?;
            let mut y = e.apply_filter(h);
            if let Some(q) = q {
                y.quantity = q;
            }
            Ok(Signal::Electrical(Arc::new(y)))
        }
        (Signal::Optical(w), PortType::Optical) => {
            let mut s = w.forward.as_ref().clone();
            for c in s.contributions.iter_mut() {
                check_transfer(h, c.source.power_spec.len())?;
                c.apply_filter(h);
            }
            Ok(Signal::Optical(Arc::new(OpticalWave::forward(s))))
        }
        _ => Err(
            "a transfer function needs one electrical input and output, or one optical input \
             and output"
                .into(),
        ),
    }
}

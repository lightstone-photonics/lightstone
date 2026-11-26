//! PyO3 bindings. Thin wrappers only: configuration crosses the boundary as JSON, waveforms
//! as NumPy arrays. The Python-facing API lives in `python/lightstone/__init__.py`.

use lightstone_analysis as ana;
use lightstone_core::component::{Report, Signal};
use lightstone_core::config::{FiberConfig, LaunchConfig, LinkConfig};
use lightstone_core::custom::{
    register_custom, CustomBehavior, CustomCtx, CustomSpec, ImpulseResponse, TapSpacing, Waveform,
};
use lightstone_core::graph::{GraphConfig, GraphEngine};
use lightstone_core::signal::{Level, Quantity, Resolution};
use lightstone_core::system::{lane_graph, LaneConfig};
use lightstone_core::{coupling, fiber, modes, Simulator, SystemConfig, SystemSimulator};
use num_complex::Complex64;
use numpy::{IntoPyArray, PyReadonlyArray1};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::rc::Rc;

fn parse_config(json: &str) -> PyResult<LinkConfig> {
    serde_json::from_str(json).map_err(|e| PyValueError::new_err(format!("invalid config: {e}")))
}

fn to_json<T: serde::Serialize>(v: &T) -> PyResult<String> {
    serde_json::to_string(v).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Default link configuration as JSON.
#[pyfunction]
fn default_config_json() -> PyResult<String> {
    to_json(&LinkConfig::default())
}

/// Cached link simulator.
#[pyclass(name = "_Simulator", unsendable)]
struct PySimulator {
    inner: Simulator,
}

#[pymethods]
impl PySimulator {
    #[new]
    fn new(config_json: &str) -> PyResult<Self> {
        Ok(Self {
            inner: Simulator::new(parse_config(config_json)?),
        })
    }

    fn set_config_json(&mut self, config_json: &str) -> PyResult<()> {
        self.inner.set_config(parse_config(config_json)?);
        Ok(())
    }

    fn config_json(&self) -> PyResult<String> {
        to_json(self.inner.config())
    }

    /// Run the link. Returns a dict of NumPy arrays plus JSON metadata.
    fn run<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        result_dict(py, self.inner.run())
    }
}

fn result_dict<'py>(
    py: Python<'py>,
    r: lightstone_core::LinkResult,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("sample_rate", r.sample_rate)?;
    d.set_item("baud", r.baud)?;
    d.set_item("sps", r.sps)?;
    d.set_item("tx_levels", r.tx_levels.to_vec())?;
    d.set_item("symbols", r.symbols.into_pyarray(py))?;
    d.set_item("tx_power", r.tx_power.into_pyarray(py))?;
    d.set_item("rx_optical", r.rx_optical.into_pyarray(py))?;
    d.set_item("rx_signal", r.rx.signal.into_pyarray(py))?;
    d.set_item("rx_noiseless", r.rx.noiseless.into_pyarray(py))?;
    d.set_item("rx_sigma", r.rx.sigma.into_pyarray(py))?;
    d.set_item("reference_optical", r.rx.reference_optical.into_pyarray(py))?;
    d.set_item("modes_json", to_json(&r.modes)?)?;
    d.set_item("metrics_json", to_json(&r.metrics)?)?;
    Ok(d)
}

/// Default system configuration (one lane) as JSON.
#[pyfunction]
fn default_system_json() -> PyResult<String> {
    to_json(&SystemConfig::default())
}

/// Multi-lane system simulator.
#[pyclass(name = "_System", unsendable)]
struct PySystem {
    inner: SystemSimulator,
}

#[pymethods]
impl PySystem {
    #[new]
    fn new(config_json: &str) -> PyResult<Self> {
        let cfg: SystemConfig = serde_json::from_str(config_json)
            .map_err(|e| PyValueError::new_err(format!("invalid system config: {e}")))?;
        Ok(Self {
            inner: SystemSimulator::new(cfg),
        })
    }

    fn set_config_json(&mut self, config_json: &str) -> PyResult<()> {
        let cfg: SystemConfig = serde_json::from_str(config_json)
            .map_err(|e| PyValueError::new_err(format!("invalid system config: {e}")))?;
        self.inner.set_config(cfg);
        Ok(())
    }

    fn config_json(&self) -> PyResult<String> {
        to_json(self.inner.config())
    }

    /// Validation issues as JSON list.
    fn validate_json(&self) -> PyResult<String> {
        to_json(&self.inner.validate())
    }

    fn run_lane<'py>(&mut self, py: Python<'py>, lane: usize) -> PyResult<Bound<'py, PyDict>> {
        let r = self
            .inner
            .run_lane(lane)
            .map_err(|e| PyValueError::new_err(format!("invalid system: {e}")))?;
        result_dict(py, r)
    }
}

/// Solve fiber modes. Returns (radial grid [m], list of mode dicts with fields as arrays,
/// metadata dict).
#[pyfunction]
fn solve_modes<'py>(
    py: Python<'py>,
    fiber_json: &str,
    wavelength: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let f: FiberConfig = serde_json::from_str(fiber_json)
        .map_err(|e| PyValueError::new_err(format!("invalid fiber config: {e}")))?;
    let set = modes::solve(&modes::SolverParams::from_fiber(&f, wavelength));
    let d = PyDict::new(py);
    d.set_item("wavelength", set.wavelength)?;
    d.set_item("core_radius", set.core_radius)?;
    d.set_item("n_core", set.n_core)?;
    d.set_item("n_clad", set.n_clad)?;
    d.set_item("v_number", set.v_number)?;
    d.set_item("dr", set.dr)?;
    d.set_item(
        "ofl_bandwidth_length",
        fiber::modal_bandwidth_length(&set, &fiber::overfilled_weights(&set)),
    )?;
    let list = pyo3::types::PyList::empty(py);
    for m in &set.modes {
        let md = PyDict::new(py);
        md.set_item("l", m.l)?;
        md.set_item("m", m.m)?;
        md.set_item("group", m.group)?;
        md.set_item("n_eff", m.n_eff)?;
        md.set_item("b", m.b)?;
        md.set_item("group_delay", m.group_delay)?;
        md.set_item("chromatic_slope", m.chromatic_slope)?;
        md.set_item("field", m.field.clone().into_pyarray(py))?;
        list.append(md)?;
    }
    d.set_item("modes", list)?;
    d.set_item("r", set.r.clone().into_pyarray(py))?;
    Ok(d)
}

/// Launch coupling: mode powers for a launch configuration.
#[pyfunction]
fn couple<'py>(
    py: Python<'py>,
    fiber_json: &str,
    launch_json: &str,
    wavelength: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let f: FiberConfig = serde_json::from_str(fiber_json)
        .map_err(|e| PyValueError::new_err(format!("invalid fiber config: {e}")))?;
    let l: LaunchConfig = serde_json::from_str(launch_json)
        .map_err(|e| PyValueError::new_err(format!("invalid launch config: {e}")))?;
    let set = modes::solve(&modes::SolverParams::from_fiber(&f, wavelength));
    let c = coupling::couple_launch(&set, &l, coupling::DEFAULT_ANGULAR_POINTS);
    let d = PyDict::new(py);
    d.set_item("guided_fraction", c.guided_fraction)?;
    d.set_item("domain_fraction", c.domain_fraction)?;
    d.set_item(
        "l",
        c.modes
            .iter()
            .map(|m| m.l)
            .collect::<Vec<_>>()
            .into_pyarray(py),
    )?;
    d.set_item(
        "m",
        c.modes
            .iter()
            .map(|m| m.m)
            .collect::<Vec<_>>()
            .into_pyarray(py),
    )?;
    d.set_item(
        "power",
        c.modes
            .iter()
            .map(|m| m.power)
            .collect::<Vec<_>>()
            .into_pyarray(py),
    )?;
    let w: Vec<f64> = c.modes.iter().map(|m| m.power).collect();
    d.set_item("emb_length", fiber::modal_bandwidth_length(&set, &w))?;
    d.set_item(
        "encircled_flux_4p5um",
        coupling::encircled_flux(&set, &c, 4.5e-6),
    )?;
    d.set_item(
        "encircled_flux_19um",
        coupling::encircled_flux(&set, &c, 19e-6),
    )?;
    Ok(d)
}

/// PAM-4 or NRZ analysis (`levels` 4 or 2; `None` = inferred from the symbols). Returns JSON.
#[pyfunction]
#[pyo3(signature = (noisy, symbols, sps, noiseless=None, sigma=None, options_json=None, levels=None))]
#[allow(clippy::too_many_arguments)]
fn analyze(
    noisy: PyReadonlyArray1<f64>,
    symbols: PyReadonlyArray1<u8>,
    sps: usize,
    noiseless: Option<PyReadonlyArray1<f64>>,
    sigma: Option<PyReadonlyArray1<f64>>,
    options_json: Option<&str>,
    levels: Option<u8>,
) -> PyResult<String> {
    let opts: ana::AnalysisOptions = match options_json {
        Some(j) => serde_json::from_str(j).map_err(|e| PyValueError::new_err(e.to_string()))?,
        None => Default::default(),
    };
    let x = noisy.as_slice()?;
    let s = symbols.as_slice()?;
    if x.len() != s.len() * sps {
        return Err(PyValueError::new_err(
            "len(noisy) must equal len(symbols) * sps",
        ));
    }
    let model = match (&noiseless, &sigma) {
        (Some(a), Some(b)) => Some((a.as_slice()?, b.as_slice()?)),
        (None, None) => None,
        _ => {
            return Err(PyValueError::new_err(
                "give both noiseless and sigma, or neither",
            ))
        }
    };
    if let Some((a, b)) = model {
        if a.len() != x.len() || b.len() != x.len() {
            return Err(PyValueError::new_err("noiseless/sigma length mismatch"));
        }
    }
    let m = levels.unwrap_or_else(|| ana::infer_levels(s));
    if m != 2 && m != 4 {
        return Err(PyValueError::new_err("levels must be 2 (NRZ) or 4 (PAM-4)"));
    }
    if s.iter().any(|&k| k >= m) {
        return Err(PyValueError::new_err(format!(
            "symbols must be below levels = {m}"
        )));
    }
    to_json(&ana::analyze_levels(x, model, s, m, sps, &opts))
}

/// Symbols of a named test pattern: "prbs13q" (8191 symbols; PRBS13 for levels = 2) or
/// "ssprq" (65 535 PAM-4 symbols; unverified against the standard, docs/model/prbs-pattern.md).
#[pyfunction]
#[pyo3(signature = (name="prbs13q", levels=4, seed=lightstone_core::prbs::PRBS13_DEFAULT_SEED))]
fn test_pattern<'py>(
    py: Python<'py>,
    name: &str,
    levels: u8,
    seed: u16,
) -> PyResult<Bound<'py, numpy::PyArray1<u8>>> {
    if seed & 0x1FFF == 0 {
        return Err(PyValueError::new_err(
            "PRBS13 seed must be nonzero (13 bits)",
        ));
    }
    let v =
        lightstone_core::prbs::named_pattern(name, levels, seed).map_err(PyValueError::new_err)?;
    Ok(v.into_pyarray(py))
}

/// TDECQ of a reference-receiver-filtered optical waveform. Returns JSON.
#[pyfunction]
#[pyo3(signature = (waveform, symbols, sps, baud, options_json=None))]
fn tdecq(
    waveform: PyReadonlyArray1<f64>,
    symbols: PyReadonlyArray1<u8>,
    sps: usize,
    baud: f64,
    options_json: Option<&str>,
) -> PyResult<String> {
    let opts: ana::TdecqOptions = match options_json {
        Some(j) => serde_json::from_str(j).map_err(|e| PyValueError::new_err(e.to_string()))?,
        None => Default::default(),
    };
    let x = waveform.as_slice()?;
    let s = symbols.as_slice()?;
    if x.len() != s.len() * sps {
        return Err(PyValueError::new_err(
            "len(waveform) must equal len(symbols) * sps",
        ));
    }
    if ana::infer_levels(s) != 4 {
        return Err(PyValueError::new_err(
            "TDECQ is defined for PAM-4 only (the symbols are NRZ)",
        ));
    }
    opts.validate().map_err(PyValueError::new_err)?;
    to_json(&ana::tdecq(x, s, sps, baud, &opts))
}

/// Eye density. Returns (counts as 2D-reshapable 1D array, metadata JSON without counts).
#[pyfunction]
#[pyo3(signature = (samples, sps, delay, center_phase, width=600, height=400, v_range=None))]
#[allow(clippy::too_many_arguments)]
fn eye_density<'py>(
    py: Python<'py>,
    samples: PyReadonlyArray1<f64>,
    sps: usize,
    delay: usize,
    center_phase: f64,
    width: usize,
    height: usize,
    v_range: Option<(f64, f64)>,
) -> PyResult<(Bound<'py, numpy::PyArray1<u32>>, String)> {
    let x = samples.as_slice()?;
    if x.len() % sps != 0 {
        return Err(PyValueError::new_err(
            "len(samples) must be a multiple of sps",
        ));
    }
    let mut e = ana::eye_density(x, sps, delay, center_phase, width, height, v_range);
    let counts = std::mem::take(&mut e.counts);
    Ok((counts.into_pyarray(py), to_json(&e)?))
}

// ---------------------------------------------------------------------------------------------
// Component graph and custom Python components (docs/architecture.md, 7.4 and 7.7)
// ---------------------------------------------------------------------------------------------

fn parse_graph(json: &str) -> PyResult<GraphConfig> {
    serde_json::from_str(json).map_err(|e| PyValueError::new_err(format!("invalid graph: {e}")))
}

fn quantity_name(q: Quantity) -> &'static str {
    match q {
        Quantity::Dimensionless => "drive",
        Quantity::Current => "current",
        Quantity::Voltage => "voltage",
    }
}

fn parse_spec(json: &str) -> PyResult<CustomSpec> {
    CustomSpec::from_json(json).map_err(PyValueError::new_err)
}

/// Calls the Python adapter object (`lightstone.Component`): `_transfer(ctx, f)` returns
/// `(re, im)`, `_impulse(ctx, t)` returns `("continuous", h)` or `("taps", c, spacing)`,
/// `_process(ctx, inputs)` returns a list of output tuples.
struct PyBehavior {
    kind: String,
    obj: Py<PyAny>,
}

fn ctx_dict<'py>(py: Python<'py>, c: &CustomCtx) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("node", c.node)?;
    d.set_item(
        "params_json",
        serde_json::to_string(c.params).unwrap_or_default(),
    )?;
    d.set_item("baud", c.grid.baud)?;
    d.set_item("sps", c.grid.sps)?;
    d.set_item("n_symbols", c.grid.n_symbols)?;
    d.set_item("n", c.grid.n())?;
    d.set_item("sample_rate", c.grid.fs())?;
    d.set_item("seed", c.seed)?;
    Ok(d)
}

impl PyBehavior {
    fn err(&self, e: PyErr) -> String {
        format!("Python component '{}': {e}", self.kind)
    }
}

impl CustomBehavior for PyBehavior {
    fn transfer(&self, ctx: &CustomCtx, f: &[f64]) -> Result<Vec<Complex64>, String> {
        Python::attach(|py| -> PyResult<Vec<Complex64>> {
            let r = self.obj.call_method1(
                py,
                "_transfer",
                (ctx_dict(py, ctx)?, f.to_vec().into_pyarray(py)),
            )?;
            let (re, im): (PyReadonlyArray1<f64>, PyReadonlyArray1<f64>) = r.extract(py)?;
            let (re, im) = (re.as_slice()?, im.as_slice()?);
            if re.len() != im.len() {
                return Err(PyValueError::new_err(
                    "real and imaginary parts differ in length",
                ));
            }
            Ok(re
                .iter()
                .zip(im)
                .map(|(a, b)| Complex64::new(*a, *b))
                .collect())
        })
        .map_err(|e| self.err(e))
    }

    fn impulse(&self, ctx: &CustomCtx, t: &[f64]) -> Result<ImpulseResponse, String> {
        Python::attach(|py| -> PyResult<ImpulseResponse> {
            let r = self.obj.call_method1(
                py,
                "_impulse",
                (ctx_dict(py, ctx)?, t.to_vec().into_pyarray(py)),
            )?;
            let (kind, values, spacing): (String, PyReadonlyArray1<f64>, Option<String>) =
                r.extract(py)?;
            let values = values.as_slice()?.to_vec();
            Ok(match (kind.as_str(), spacing.as_deref()) {
                ("continuous", None) => ImpulseResponse::Continuous(values),
                ("taps", Some(s)) => ImpulseResponse::Taps {
                    taps: values,
                    spacing: match s {
                        "sample" => TapSpacing::Sample,
                        "symbol" => TapSpacing::Symbol,
                        o => {
                            return Err(PyValueError::new_err(format!(
                                "tap_spacing must be 'sample' or 'symbol', not '{o}'"
                            )))
                        }
                    },
                },
                _ => return Err(PyValueError::new_err("invalid impulse response")),
            })
        })
        .map_err(|e| self.err(e))
    }

    fn process(&self, ctx: &CustomCtx, inputs: Vec<Waveform>) -> Result<Vec<Waveform>, String> {
        let quantity = inputs
            .iter()
            .find_map(|w| match w {
                Waveform::Electrical { quantity, .. } => Some(*quantity),
                _ => None,
            })
            .unwrap_or(Quantity::Voltage);
        Python::attach(|py| -> PyResult<Vec<Waveform>> {
            let list = PyList::empty(py);
            for w in inputs {
                match w {
                    Waveform::Data { symbols, levels } => {
                        list.append(("data", symbols.into_pyarray(py), levels))?
                    }
                    Waveform::Electrical {
                        quantity,
                        noiseless,
                        noisy,
                        sigma,
                        gaussian_exact,
                    } => list.append((
                        quantity_name(quantity),
                        noiseless.into_pyarray(py),
                        noisy.into_pyarray(py),
                        sigma.into_pyarray(py),
                        gaussian_exact,
                    ))?,
                }
            }
            let r = self
                .obj
                .call_method1(py, "_process", (ctx_dict(py, ctx)?, list))?;
            let outs: Vec<(String, Bound<PyAny>, Bound<PyAny>)> = r.extract(py)?;
            outs.into_iter()
                .map(|(kind, a, b)| match kind.as_str() {
                    "data" => Ok(Waveform::Data {
                        symbols: a.extract::<PyReadonlyArray1<u8>>()?.as_slice()?.to_vec(),
                        levels: b.extract()?,
                    }),
                    "electrical" => Ok(Waveform::Electrical {
                        quantity,
                        noiseless: a.extract::<PyReadonlyArray1<f64>>()?.as_slice()?.to_vec(),
                        noisy: b.extract::<PyReadonlyArray1<f64>>()?.as_slice()?.to_vec(),
                        sigma: vec![],
                        gaussian_exact: false,
                    }),
                    _ => Err(PyValueError::new_err(format!(
                        "unknown output kind '{kind}'"
                    ))),
                })
                .collect()
        })
        .map_err(|e| self.err(e))
    }
}

/// Component graph engine with per-node caching.
#[pyclass(name = "_Graph", unsendable)]
struct PyGraph {
    inner: GraphEngine,
}

fn report_dict<'py>(py: Python<'py>, r: &Report) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match r {
        Report::Transmitter { tx } => {
            d.set_item("type", "transmitter")?;
            d.set_item("levels", tx.levels.to_vec())?;
            d.set_item("power", tx.power.clone().into_pyarray(py))?;
            d.set_item("vcsel_bandwidth", tx.vcsel_bandwidth)?;
        }
        Report::FiberSpan { basis, launch } => {
            d.set_item("type", "fiber_span")?;
            d.set_item("modes", basis.modes.len())?;
            d.set_item("v_number", basis.v_number)?;
            if let Some(l) = launch {
                d.set_item("guided_fraction", l.coupling.guided_fraction)?;
                d.set_item("emb_length", l.emb_length)?;
                d.set_item("ofl_bandwidth_length", l.ofl_bandwidth_length)?;
            }
        }
        Report::Detection(det) => {
            d.set_item("type", "detection")?;
            d.set_item("p_det", det.p_det.as_ref().clone().into_pyarray(py))?;
            let ch = PyList::empty(py);
            for c in &det.channels {
                let cd = PyDict::new(py);
                cd.set_item("transmission", c.transmission)?;
                cd.set_item("mean_delay", c.mean_delay)?;
                cd.set_item("rms_delay_spread", c.rms_delay_spread)?;
                ch.append(cd)?;
            }
            d.set_item("channels", ch)?;
        }
        Report::Filter { noise_bandwidth } => {
            d.set_item("type", "filter")?;
            d.set_item("noise_bandwidth", noise_bandwidth)?;
        }
        Report::Receiver(m) => {
            d.set_item("type", "receiver")?;
            d.set_item("noiseless", m.noiseless.clone().into_pyarray(py))?;
            d.set_item("signal", m.signal.clone().into_pyarray(py))?;
            d.set_item("sigma", m.sigma.clone().into_pyarray(py))?;
            d.set_item("gaussian_exact", m.gaussian_exact)?;
        }
        Report::PowerMeter { average_power } => {
            d.set_item("type", "power_meter")?;
            d.set_item("average_power", average_power)?;
        }
        Report::Data { kind, value } => {
            d.set_item("type", kind)?;
            d.set_item("json", to_json(value.as_ref())?)?;
        }
    }
    Ok(d)
}

#[pymethods]
impl PyGraph {
    #[new]
    fn new(graph_json: &str) -> PyResult<Self> {
        Ok(Self {
            inner: GraphEngine::with_registry(parse_graph(graph_json)?, ana::nodes::registry()),
        })
    }

    /// Replace the graph; cached results of unchanged nodes are reused.
    fn set_graph_json(&mut self, graph_json: &str) -> PyResult<()> {
        let g = parse_graph(graph_json)?;
        if *self.inner.graph() != g {
            self.inner.set_graph(g);
        }
        Ok(())
    }

    fn graph_json(&self) -> PyResult<String> {
        to_json(self.inner.graph())
    }

    /// Register (or replace) a custom component kind. Clears the node cache, because the
    /// component's code is not part of the cache key.
    fn register(&mut self, kind: &str, spec_json: &str, obj: Py<PyAny>) -> PyResult<()> {
        let spec = parse_spec(spec_json)?;
        let behavior = Rc::new(PyBehavior {
            kind: kind.to_string(),
            obj,
        });
        register_custom(self.inner.registry_mut(), kind, spec, behavior)
            .map_err(PyValueError::new_err)?;
        self.inner.clear_cache();
        Ok(())
    }

    fn validate_json(&self) -> PyResult<String> {
        to_json(&self.inner.validate())
    }

    fn ports_json(&self) -> PyResult<String> {
        to_json(&self.inner.node_ports())
    }

    /// Run the graph. Returns `None`, or the list of issues as JSON if it failed.
    fn run(&mut self) -> PyResult<Option<String>> {
        match self.inner.run() {
            Ok(()) => Ok(None),
            Err(e) => Ok(Some(to_json(&e.0)?)),
        }
    }

    fn computed_last_run(&self) -> usize {
        self.inner.computed_last_run()
    }

    fn warnings(&self) -> Vec<(String, String)> {
        self.inner.warnings()
    }

    fn report<'py>(&self, py: Python<'py>, node: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.inner
            .report(node)
            .map(|r| report_dict(py, r))
            .transpose()
    }

    /// Value of an output port from the last run: symbols for data, explicit waveforms for
    /// electrical, average power for optical.
    fn output<'py>(
        &mut self,
        py: Python<'py>,
        node: &str,
        port: &str,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(sig) = self.inner.output(node, port).cloned() else {
            return Ok(None);
        };
        let d = PyDict::new(py);
        match sig {
            Signal::Data(x) => {
                d.set_item("type", "data")?;
                d.set_item("symbols", x.symbols.as_ref().clone().into_pyarray(py))?;
                d.set_item("levels", x.levels)?;
            }
            Signal::Electrical(e) => {
                let fs = self.inner.grid().fs();
                let m = e.materialize(&self.inner.spectral(), fs);
                d.set_item("type", "electrical")?;
                d.set_item("quantity", quantity_name(e.quantity))?;
                d.set_item("noiseless", m.noiseless.into_pyarray(py))?;
                d.set_item("signal", m.signal.into_pyarray(py))?;
                d.set_item("sigma", m.sigma.into_pyarray(py))?;
                d.set_item("gaussian_exact", m.gaussian_exact)?;
            }
            Signal::Optical(w) => {
                d.set_item("type", "optical")?;
                d.set_item("average_power", w.forward.average_power())?;
                d.set_item("contributions", w.forward.contributions.len())?;
            }
        }
        Ok(Some(d))
    }
}

/// Built-in component kinds (core components and analysis nodes).
#[pyfunction]
fn builtin_kinds() -> Vec<String> {
    ana::nodes::registry().builtin_kinds()
}

/// Multimode fiber presets as JSON: `{ version, types: { om3: <custom_fiber params>, … } }`.
#[pyfunction]
fn fiber_presets_json() -> String {
    use lightstone_core::config::{MmfType, MMF_PRESET_VERSION};
    let types: serde_json::Map<String, serde_json::Value> = MmfType::ALL
        .iter()
        .map(|t| {
            (
                serde_json::to_value(t)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                serde_json::to_value(t.config()).unwrap(),
            )
        })
        .collect();
    serde_json::json!({ "version": MMF_PRESET_VERSION, "types": types }).to_string()
}

/// Preset graph of a lane (reference link) as JSON.
#[pyfunction]
#[pyo3(signature = (lane_json=None, resolution="mode", level="power"))]
fn lane_graph_json(lane_json: Option<&str>, resolution: &str, level: &str) -> PyResult<String> {
    let lane: LaneConfig = match lane_json {
        Some(j) => serde_json::from_str(j)
            .map_err(|e| PyValueError::new_err(format!("invalid lane config: {e}")))?,
        None => LaneConfig::default(),
    };
    let res: Resolution = serde_json::from_value(serde_json::Value::String(resolution.into()))
        .map_err(|_| PyValueError::new_err("resolution must be 'mode' or 'mode_group'"))?;
    let lvl: Level = serde_json::from_value(serde_json::Value::String(level.into()))
        .map_err(|_| PyValueError::new_err("level must be 'power', 'stokes' or 'field'"))?;
    to_json(&lane_graph(&lane, res, lvl))
}

#[pymodule]
fn _lightstone(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(default_config_json, m)?)?;
    m.add_function(wrap_pyfunction!(fiber_presets_json, m)?)?;
    m.add_function(wrap_pyfunction!(solve_modes, m)?)?;
    m.add_function(wrap_pyfunction!(couple, m)?)?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(tdecq, m)?)?;
    m.add_function(wrap_pyfunction!(test_pattern, m)?)?;
    m.add_function(wrap_pyfunction!(eye_density, m)?)?;
    m.add_class::<PySimulator>()?;
    m.add_class::<PySystem>()?;
    m.add_class::<PyGraph>()?;
    m.add_function(wrap_pyfunction!(default_system_json, m)?)?;
    m.add_function(wrap_pyfunction!(builtin_kinds, m)?)?;
    m.add_function(wrap_pyfunction!(lane_graph_json, m)?)?;
    Ok(())
}

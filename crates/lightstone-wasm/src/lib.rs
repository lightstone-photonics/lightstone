//! wasm-bindgen bindings for the browser graph editor. Thin wrappers: graphs and results
//! cross as JSON, eye densities and images as typed arrays.

use lightstone_analysis as ana;
use lightstone_core::coupling::{lg_field, lg_modes};
use lightstone_core::system::LaneConfig;
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Fiber transfer-function plot: points and maximum frequency \[Hz\].
const FIBER_POINTS: usize = 200;
const FIBER_FMAX: f64 = 100e9;

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn err<E: std::fmt::Display>(e: E) -> JsError {
    JsError::new(&e.to_string())
}

/// n×n facet image over [−extent, extent]² \[m\]: `kind = 0` VCSEL launch intensity
/// (incoherent LG mixture, offset applied), `kind = 1` guided near-field intensity
/// (incoherent sum of `powers` per mode of `set`), `kind = 2` field ψ of mode `mode_index`
/// (cos orientation, signed). Missing inputs give a blank image.
fn facet_image(
    kind: u32,
    n: usize,
    extent: f64,
    mode_index: usize,
    launch: Option<&lightstone_core::config::LaunchConfig>,
    set: Option<&lightstone_core::modes::ModeSet>,
    powers: Option<&[f64]>,
) -> Vec<f32> {
    let mut img = vec![0f32; n * n];
    let coord = |i: usize| -extent + (i as f64 + 0.5) * 2.0 * extent / n as f64;
    let dr = set.map_or(1.0, |s| s.dr);
    let radial = |field: &[f64], r: f64| -> f64 {
        // Linear interpolation on the cell-centered grid.
        let x = r / dr - 0.5;
        if x <= 0.0 {
            return field[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= field.len() {
            return 0.0;
        }
        let u = x - i as f64;
        field[i] * (1.0 - u) + field[i + 1] * u
    };
    match (kind, launch, set, powers) {
        (0, Some(launch), _, _) => {
            let lgs = lg_modes(launch.vcsel_mode_groups);
            for iy in 0..n {
                for ix in 0..n {
                    let (x, y) = (coord(ix) - launch.offset, coord(iy));
                    let rho = (x * x + y * y).sqrt();
                    let th = y.atan2(x);
                    let v: f64 = lgs
                        .iter()
                        .map(|&m| lg_field(m, launch.beam_radius, rho, th).powi(2))
                        .sum::<f64>()
                        / lgs.len() as f64;
                    img[iy * n + ix] = v as f32;
                }
            }
        }
        (1, _, Some(set), Some(powers)) => {
            for iy in 0..n {
                for ix in 0..n {
                    let r = (coord(ix).powi(2) + coord(iy).powi(2)).sqrt();
                    let mut v = 0.0;
                    for (md, &p) in set.modes.iter().zip(powers) {
                        if p < 1e-9 {
                            continue;
                        }
                        let f = radial(&md.field, r);
                        v += p * f * f * if md.l == 0 { 1.0 } else { 0.5 };
                    }
                    img[iy * n + ix] = v as f32;
                }
            }
        }
        (2, _, Some(set), _) => {
            if let Some(md) = set.modes.get(mode_index) {
                for iy in 0..n {
                    for ix in 0..n {
                        let (x, y) = (coord(ix), coord(iy));
                        let r = (x * x + y * y).sqrt();
                        let phi = y.atan2(x);
                        img[iy * n + ix] =
                            (radial(&md.field, r) * (md.l as f64 * phi).cos()) as f32;
                    }
                }
            }
        }
        _ => {}
    }
    img
}

// ---------------------------------------------------------------------------------------------
// Component graph (docs/architecture.md, 7.9: browser graph editor)
// ---------------------------------------------------------------------------------------------

use lightstone_core::component::{Report, Signal};
use lightstone_core::graph::{GraphConfig, GraphEngine, NodeInfo};
use lightstone_core::modes::ModeSet;
use lightstone_core::signal::{Level, Propagation, Resolution, Settings};
use std::collections::HashMap;
use std::sync::Arc;

/// Symbols in a signal preview window.
const PREVIEW_SYMBOLS: usize = 32;
/// First symbol of the preview window.
const PREVIEW_START: usize = 100;

fn graph_from(json: &str) -> Result<GraphConfig, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("invalid graph: {e}")))
}

/// Compact, JSON-friendly summary of a node report (large arrays are left out; eye densities
/// are available through [`Graph::eye`]).
fn report_summary(r: &Report) -> serde_json::Value {
    use serde_json::json;
    match r {
        Report::Transmitter { tx } => json!({
            "type": "transmitter",
            "levels": tx.levels,
            "average_power": tx.power.iter().sum::<f64>() / tx.power.len().max(1) as f64,
            "vcsel_bandwidth": tx.vcsel_bandwidth,
        }),
        Report::FiberSpan { basis, launch } => json!({
            "type": "fiber_span",
            "modes": basis.modes.len(),
            "v_number": basis.v_number,
            "guided_fraction": launch.as_ref().map(|l| l.coupling.guided_fraction),
            "emb_length": launch.as_ref().and_then(|l| l.emb_length),
            "ofl_bandwidth_length": launch.as_ref().and_then(|l| l.ofl_bandwidth_length),
        }),
        Report::Detection(d) => json!({
            "type": "detection",
            "average_power": d.p_det.iter().sum::<f64>() / d.p_det.len().max(1) as f64,
            "channels": d.channels.iter().map(|c| json!({
                "transmission": c.transmission,
                "rms_delay_spread": c.rms_delay_spread,
                "mean_delay": c.mean_delay,
            })).collect::<Vec<_>>(),
        }),
        Report::Filter { noise_bandwidth } => {
            json!({ "type": "filter", "noise_bandwidth": noise_bandwidth })
        }
        Report::Receiver(m) => {
            let n = m.sigma.len().max(1) as f64;
            json!({
                "type": "receiver",
                "gaussian_exact": m.gaussian_exact,
                "mean": m.noiseless.iter().sum::<f64>() / n,
                "sigma_rms": (m.sigma.iter().map(|s| s * s).sum::<f64>() / n).sqrt(),
            })
        }
        Report::PowerMeter { average_power } => {
            json!({ "type": "power_meter", "average_power": average_power })
        }
        Report::Data { kind, value } => {
            let mut v = value.as_ref().clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("counts");
                o.insert("type".into(), kind.clone().into());
            }
            v
        }
    }
}

/// Compact summary of an output port value (results `ports` section):
/// optical → average power \[W\]; electrical → mean, swing (peak-to-peak of the noiseless
/// waveform) and RMS noise in the signal's unit; data → symbol count and alphabet size.
fn port_summary(sig: &Signal, sp: &lightstone_core::dsp::Spectral, fs: f64) -> serde_json::Value {
    use serde_json::json;
    let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt();
    match sig {
        Signal::Data(d) => json!({
            "type": "data",
            "symbols": d.symbols.len(),
            "levels": d.levels,
        }),
        Signal::Optical(w) => json!({
            "type": "optical",
            "average_power": w.forward.average_power(),
        }),
        Signal::Electrical(e) => {
            let x = e.noiseless(sp);
            let mean = x.iter().sum::<f64>() / x.len().max(1) as f64;
            let (lo, hi) = x
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                    (a.min(v), b.max(v))
                });
            // σ(t) is exact while the path is linear; after a nonlinear block only the
            // explicit realization is known, so its RMS is reported instead.
            let noise_rms = if e.gaussian_exact {
                rms(&e.noise_sigma(sp, fs))
            } else {
                rms(&e.noise_realization(sp, fs))
            };
            json!({
                "type": "electrical",
                "quantity": e.quantity,
                "mean": mean,
                "swing": if x.is_empty() { 0.0 } else { hi - lo },
                "noise_rms": noise_rms,
                "gaussian_exact": e.gaussian_exact,
            })
        }
    }
}

#[derive(Serialize)]
struct KindInfo {
    kind: String,
    linear: bool,
    inputs: Vec<lightstone_core::graph::PortInfo>,
    outputs: Vec<lightstone_core::graph::PortInfo>,
    params: serde_json::Value,
}

/// (mode index in the basis, power fraction, accumulated relative delay \[s\], mode group).
type ModePower = (usize, f64, f64, u32);

/// Browser-side component graph handle (core and analysis kinds).
#[wasm_bindgen]
pub struct Graph {
    engine: GraphEngine,
    eyes: HashMap<String, Vec<u32>>,
    /// Port summaries per node, with the node's result key they were computed for.
    ports: HashMap<String, (u64, serde_json::Value)>,
}

#[wasm_bindgen]
impl Graph {
    #[wasm_bindgen(constructor)]
    pub fn new(graph_json: &str) -> Result<Graph, JsError> {
        Ok(Graph {
            engine: GraphEngine::with_registry(graph_from(graph_json)?, ana::nodes::registry()),
            eyes: HashMap::new(),
            ports: HashMap::new(),
        })
    }

    /// Replace the graph; unchanged nodes keep their cached results.
    pub fn set_graph(&mut self, graph_json: &str) -> Result<(), JsError> {
        let g = graph_from(graph_json)?;
        if *self.engine.graph() != g {
            self.engine.set_graph(g);
        }
        Ok(())
    }

    /// Multimode fiber presets: `{ version, types: { om3: <custom_fiber params>, … } }`.
    pub fn fiber_presets() -> String {
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

    /// Engine version (crate version), recorded in saved results.
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Built-in kinds with their ports (for default parameters) and default parameters.
    pub fn kinds() -> String {
        let reg = ana::nodes::registry();
        let settings = Settings::default();
        let info = |p: &lightstone_core::component::PortSpec| lightstone_core::graph::PortInfo {
            name: p.name.clone(),
            ty: p.ty.name(),
            monitor: p.monitor,
        };
        let kinds: Vec<KindInfo> = reg
            .builtin_kinds()
            .into_iter()
            .filter_map(|k| {
                let params = ana::nodes::default_params(&k)?;
                let c = reg
                    .create(
                        &k,
                        &params,
                        &NodeInfo {
                            id: "n",
                            settings: &settings,
                        },
                    )
                    .ok()?;
                Some(KindInfo {
                    linear: c.linearity() == lightstone_core::component::Linearity::Linear,
                    inputs: c.inputs().iter().map(info).collect(),
                    outputs: c.outputs().iter().map(info).collect(),
                    kind: k,
                    params,
                })
            })
            .collect();
        serde_json::to_string(&kinds).unwrap()
    }

    /// Ports of every node of the current graph (they can depend on parameters, e.g. the
    /// number of splitter outputs).
    pub fn ports(&self) -> String {
        serde_json::to_string(&self.engine.node_ports()).unwrap()
    }

    /// Structural problems as JSON `[{node, wire, message}]`.
    pub fn validate(&self) -> String {
        serde_json::to_string(&self.engine.validate()).unwrap()
    }

    /// Evaluate the graph. Returns JSON `{ ok, issues, computed, ms, warnings, reports, ports }`
    /// (`ports`: per node, per output port, see [`port_summary`]); on failure `ok` is false
    /// and `issues` lists the problems.
    pub fn run(&mut self) -> String {
        let t0 = now_ms();
        let res = self.engine.run();
        let ms = now_ms() - t0;
        let issues = match &res {
            Ok(()) => vec![],
            Err(e) => e.0.clone(),
        };
        let mut reports = serde_json::Map::new();
        self.eyes.clear();
        if res.is_ok() {
            for n in &self.engine.graph().nodes {
                if let Some(r) = self.engine.report(&n.id) {
                    reports.insert(n.id.clone(), report_summary(r));
                    if let Report::Data { kind, value } = r {
                        if kind == "eye_diagram" {
                            let counts: Vec<u32> =
                                serde_json::from_value(value["counts"].clone()).unwrap_or_default();
                            self.eyes.insert(n.id.clone(), counts);
                        }
                    }
                }
            }
        }
        let mut ports = serde_json::Map::new();
        if res.is_ok() {
            let sp = self.engine.spectral();
            let fs = self.engine.grid().fs();
            let mut fresh = HashMap::new();
            for n in &self.engine.graph().nodes {
                let Some(key) = self.engine.result_key(&n.id) else {
                    continue;
                };
                let summary = match self.ports.remove(&n.id) {
                    Some((k, v)) if k == key => v,
                    _ => serde_json::Value::Object(
                        self.engine
                            .outputs(&n.id)
                            .into_iter()
                            .map(|(p, s)| (p.to_string(), port_summary(s, &sp, fs)))
                            .collect(),
                    ),
                };
                ports.insert(n.id.clone(), summary.clone());
                fresh.insert(n.id.clone(), (key, summary));
            }
            self.ports = fresh;
        }
        let warnings: Vec<serde_json::Value> = self
            .engine
            .warnings()
            .into_iter()
            .map(|(node, message)| serde_json::json!({ "node": node, "message": message }))
            .collect();
        serde_json::json!({
            "ok": res.is_ok(),
            "issues": issues,
            "computed": self.engine.computed_last_run(),
            "ms": ms,
            "warnings": warnings,
            "reports": reports,
            "ports": ports,
        })
        .to_string()
    }

    /// Eye density of an `eye_diagram` node from the last run (row-major, row 0 = top).
    pub fn eye(&self, node: &str) -> Vec<u32> {
        self.eyes.get(node).cloned().unwrap_or_default()
    }

    /// Signal at an output port over a window of 32 symbols (from symbol 100), as JSON:
    /// electrical → noiseless and noisy waveform and σ; optical → detected power (all light,
    /// incl. RIN) \[W\]; data → symbols. `null` if the port has no value.
    pub fn preview(&mut self, node: &str, port: &str) -> String {
        let Some(sig) = self.engine.output(node, port).cloned() else {
            return "null".into();
        };
        let grid = self.engine.grid();
        let (n, sps) = (grid.n(), grid.sps);
        let len = (PREVIEW_SYMBOLS * sps).min(n);
        let start = (PREVIEW_START * sps) % n;
        let window =
            |x: &[f64]| -> Vec<f32> { (0..len).map(|i| x[(start + i) % n] as f32).collect() };
        let sp = self.engine.spectral();
        let v = match sig {
            Signal::Data(d) => serde_json::json!({
                "type": "data",
                "symbols": (0..PREVIEW_SYMBOLS).map(|k| d.symbols[(PREVIEW_START + k) % d.symbols.len()]).collect::<Vec<_>>(),
            }),
            Signal::Electrical(e) => {
                let m = e.materialize(&sp, grid.fs());
                serde_json::json!({
                    "type": "electrical",
                    "quantity": format!("{:?}", e.quantity).to_lowercase(),
                    "sps": sps,
                    "noiseless": window(&m.noiseless),
                    "signal": window(&m.signal),
                    "sigma": window(&m.sigma),
                    "gaussian_exact": m.gaussian_exact,
                })
            }
            Signal::Optical(w) => {
                let mut spec = vec![num_complex::Complex64::new(0.0, 0.0); n];
                for c in &w.forward.contributions {
                    let h = c.transfer(None, &grid);
                    let e = c.source.emitted_spec();
                    for k in 0..n {
                        spec[k] += e[k] * h[k];
                    }
                }
                let p = sp.inverse_real(spec);
                serde_json::json!({
                    "type": "optical",
                    "sps": sps,
                    "power": window(&p),
                    "average_power": w.forward.average_power(),
                    "contributions": w.forward.contributions.len(),
                })
            }
        };
        v.to_string()
    }

    /// Mode powers (normalized to 1) of the optical signal at `node.out`, per mode of the
    /// signal's basis, with the bins' accumulated delays: (basis, [(mode index, power,
    /// delay, group)]). `None` unless the signal is guided.
    fn mode_powers(&self, node: &str) -> Option<(Arc<ModeSet>, Vec<ModePower>)> {
        let Some(Signal::Optical(w)) = self.engine.output(node, "out") else {
            return None;
        };
        let c = w.forward.contributions.first()?;
        let Propagation::Guided {
            basis, mode_bins, ..
        } = &c.prop
        else {
            return None;
        };
        let index: HashMap<(u32, u32), usize> = basis
            .modes
            .iter()
            .enumerate()
            .map(|(k, m)| ((m.l, m.m), k))
            .collect();
        let total: f64 = mode_bins
            .iter()
            .map(|b| b.state.power())
            .sum::<f64>()
            .max(1e-300);
        let mut out = Vec::new();
        for b in mode_bins {
            for mm in &b.members {
                if let Some(&k) = index.get(&(mm.l, mm.m)) {
                    out.push((k, b.state.power() * mm.frac / total, b.delay, b.group));
                }
            }
        }
        Some((basis.clone(), out))
    }

    /// Fiber view of a `fiber` node (index profile, mode delays and powers at its output,
    /// transfer function from the source to this point, launch metrics) as JSON in the shape
    /// of the system view's fiber summary: `{ fiber, modes, group_power, metrics }`.
    pub fn fiber_view(&mut self, node: &str) -> String {
        let Some(Report::FiberSpan { basis, launch }) = self.engine.report(node).cloned() else {
            return "null".into();
        };
        let Some((set, powers)) = self.mode_powers(node) else {
            return "null".into();
        };
        let Some(Signal::Optical(w)) = self.engine.output(node, "out").cloned() else {
            return "null".into();
        };
        let c = &w.forward.contributions[0];
        let ch = c.channel(None);
        let h0 = ch.response(0.0).norm().max(1e-300);
        let modal = lightstone_core::fiber::FiberChannel {
            terms: ch
                .terms
                .iter()
                .map(|t| lightstone_core::fiber::ChannelTerm {
                    sigma: 0.0,
                    ..t.clone()
                })
                .collect(),
            ..ch.clone()
        };
        let freq: Vec<f64> = (0..FIBER_POINTS)
            .map(|i| FIBER_FMAX * i as f64 / (FIBER_POINTS - 1) as f64)
            .collect();
        // Full fiber parameters (presets resolved), so `mmf` and `custom_fiber` look alike.
        let params = self
            .engine
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .and_then(|n| lightstone_core::components::fiber_config(&n.kind, &n.params))
            .map(|c| serde_json::to_value(c).expect("serializable"))
            .unwrap_or_default();
        let group_of: HashMap<(u32, u32), u32> = powers
            .iter()
            .map(|&(k, _, _, g)| ((set.modes[k].l, set.modes[k].m), g))
            .collect();
        let max_group = powers.iter().map(|p| p.3).max().unwrap_or(0) as usize;
        let mut group_power = vec![0.0; max_group];
        for &(_, p, _, g) in &powers {
            group_power[g as usize - 1] += p;
        }
        let modes: Vec<serde_json::Value> = powers
            .iter()
            .map(|&(k, p, d, g)| {
                serde_json::json!({ "index": k, "l": set.modes[k].l, "m": set.modes[k].m, "group": g,
                                    "launched_power": p, "path_delay": d })
            })
            .collect();
        let length = params["length"].as_f64().unwrap_or(0.0);
        let alpha = params["profile"]["alpha"].as_f64();
        let fiber = serde_json::json!({
            "element": node,
            "span_length": length,
            "n_core": basis.n_core,
            "n_clad": basis.n_clad,
            "core_radius": basis.core_radius,
            "alpha": alpha,
            "length": length,
            "freq": freq,
            "mag": freq.iter().map(|&f| ch.response(f).norm() / h0).collect::<Vec<_>>(),
            "mag_modal": freq.iter().map(|&f| modal.response(f).norm() / h0).collect::<Vec<_>>(),
            "terms": ch.terms.iter().map(|t| serde_json::json!({
                "delay": t.delay, "weight": t.weight, "sigma": t.sigma,
                "group": group_of.get(&(t.l, t.m)).copied().unwrap_or(0),
            })).collect::<Vec<_>>(),
        });
        serde_json::json!({
            "fiber": fiber,
            "modes": modes,
            "group_power": group_power,
            "first_span": launch.is_some(),
            "metrics": {
                "v_number": basis.v_number,
                "modes": basis.modes.len(),
                "guided_fraction": launch.as_ref().map(|l| l.coupling.guided_fraction),
                "emb_length": launch.as_ref().and_then(|l| l.emb_length),
                "ofl_bandwidth_length": launch.as_ref().and_then(|l| l.ofl_bandwidth_length),
                "rms_delay_spread": ch.rms_delay_spread,
                "transmission": ch.transmission,
                "offset": launch.as_ref().map(|l| l.launch.offset),
            },
        })
        .to_string()
    }

    /// Facet image of a `fiber` node (see the system view's `render_facet` for `kind`):
    /// launch beam (first span only), guided intensity at the span's output, or a mode field.
    pub fn facet(
        &mut self,
        node: &str,
        kind: u32,
        n: usize,
        extent: f64,
        mode_index: usize,
    ) -> Vec<f32> {
        let Some(Report::FiberSpan { launch, .. }) = self.engine.report(node).cloned() else {
            return vec![0.0; n * n];
        };
        let Some((set, powers)) = self.mode_powers(node) else {
            return vec![0.0; n * n];
        };
        let mut per_mode = vec![0.0; set.modes.len()];
        for &(k, p, _, _) in &powers {
            per_mode[k] += p;
        }
        facet_image(
            kind,
            n,
            extent,
            mode_index,
            launch.as_ref().map(|l| &l.launch),
            Some(&set),
            Some(&per_mode),
        )
    }

    /// Preset: the reference link as a graph (lane preset, 8 samples/UI for the browser).
    /// `lane_json` converts a lane of the system view instead (empty string = default lane).
    pub fn lane_graph(lane_json: &str) -> Result<String, JsError> {
        let mut lane: LaneConfig = if lane_json.is_empty() {
            LaneConfig::default()
        } else {
            serde_json::from_str(lane_json).map_err(err)?
        };
        lane.tx.sps = 8;
        let g = lightstone_core::system::lane_graph(&lane, Resolution::Mode, Level::Power);
        Ok(serde_json::to_string(&g).unwrap())
    }
}

//! Composable systems: independent lanes, each a chain
//! VCSEL transmitter → [connector | fiber | attenuator]* → PIN/TIA receiver.
//!
//! `eq:system-modal-path` — Along a lane each LP mode m accumulates
//! power P_m (launch × losses), relative group delay Σ_spans (τ_m − τ_ref)·L and chromatic
//! delay slope Σ_spans (dτ_m/dλ)·L. The receiver sees
//! H(f) = Σ_m w_m e^{−j2πf(τ_m − τ̄)} e^{−2π²f²σ_m²}, σ_m = |slope_m|·σ_λ, w_m = P_m·η_m.
//!
//! Each lane runs on the graph engine as a preset graph ([`lane_graph`]).
//! See docs/architecture.md, Sections 6–7, for scope and validation rules.

use crate::component::{DetectionReport, LaunchReport, Report, Signal};
use crate::components::{VcselParams, RX_SEED_XOR, TX_SEED_XOR};
use crate::config::{FiberConfig, LaunchConfig, LinkConfig, RxConfig, TxConfig, VcselConfig};
use crate::coupling::{encircled_flux, Coupling};
use crate::fiber::FiberChannel;
use crate::graph::{GraphConfig, GraphEngine, GraphError};
use crate::link::{LinkMetrics, LinkResult, ModeReport};
use crate::modes::ModeSet;
use crate::rx::RxOutput;
use crate::signal::{Level, NoiseKind, Resolution, Settings};
use crate::units::w_to_dbm;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

/// VCSEL near-field (launch beam) parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    pub beam_radius: f64,
    pub vcsel_mode_groups: u32,
}

impl Default for SourceConfig {
    fn default() -> Self {
        let l = LaunchConfig::default();
        Self {
            beam_radius: l.beam_radius,
            vcsel_mode_groups: l.vcsel_mode_groups,
        }
    }
}

/// Connector: before the first fiber it defines the launch (offset, tilt); after a fiber
/// only its loss is modeled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectorConfig {
    /// Radial offset \[m\].
    pub offset: f64,
    /// Tilt in air \[rad\].
    pub tilt: f64,
    /// Insertion loss \[dB\].
    pub loss_db: f64,
}

/// Lumped optical attenuator.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttenuatorConfig {
    /// Loss \[dB\].
    pub loss_db: f64,
}

/// Chain element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Element {
    Connector(ConnectorConfig),
    Fiber(FiberConfig),
    Attenuator(AttenuatorConfig),
}

/// One lane: transmitter, chain, receiver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaneConfig {
    pub name: String,
    /// Seed for this lane's noise sources.
    pub seed: u64,
    pub tx: TxConfig,
    pub vcsel: VcselConfig,
    pub source: SourceConfig,
    pub elements: Vec<Element>,
    pub rx: RxConfig,
}

impl Default for LaneConfig {
    fn default() -> Self {
        LinkConfig::default().into()
    }
}

impl From<LinkConfig> for LaneConfig {
    fn from(c: LinkConfig) -> Self {
        Self {
            name: "Lane 1".into(),
            seed: c.seed,
            tx: c.tx,
            vcsel: c.vcsel,
            source: SourceConfig {
                beam_radius: c.launch.beam_radius,
                vcsel_mode_groups: c.launch.vcsel_mode_groups,
            },
            elements: vec![
                Element::Connector(ConnectorConfig {
                    offset: c.launch.offset,
                    tilt: c.launch.tilt,
                    loss_db: 0.0,
                }),
                Element::Fiber(c.fiber),
            ],
            rx: c.rx,
        }
    }
}

/// A system of independent lanes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    pub lanes: Vec<LaneConfig>,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            lanes: vec![LaneConfig::default()],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A validation finding, attached to a lane and optionally an element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub lane: usize,
    pub element: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

impl LaneConfig {
    /// Index of the first fiber element.
    pub fn first_fiber(&self) -> Option<usize> {
        self.elements
            .iter()
            .position(|e| matches!(e, Element::Fiber(_)))
    }

    /// Launch from the source and the connector before the first fiber (if any).
    pub fn launch(&self) -> LaunchConfig {
        let limit = self.first_fiber().unwrap_or(self.elements.len());
        let (offset, tilt) = self.elements[..limit]
            .iter()
            .rev()
            .find_map(|e| match e {
                Element::Connector(c) => Some((c.offset, c.tilt)),
                _ => None,
            })
            .unwrap_or((0.0, 0.0));
        LaunchConfig {
            beam_radius: self.source.beam_radius,
            vcsel_mode_groups: self.source.vcsel_mode_groups,
            offset,
            tilt,
        }
    }

    pub fn fibers(&self) -> impl Iterator<Item = (usize, &FiberConfig)> {
        self.elements
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Element::Fiber(f) => Some((i, f)),
                _ => None,
            })
    }

    /// Validate this lane (index `lane` is used in the reported issues).
    pub fn validate(&self, lane: usize) -> Vec<Issue> {
        let mut out = Vec::new();
        let mut push = |element: Option<usize>, severity, message: String| {
            out.push(Issue {
                lane,
                element,
                severity,
                message,
            })
        };
        let first = self.first_fiber();
        let mut connectors_before = 0;
        for (i, e) in self.elements.iter().enumerate() {
            match e {
                Element::Connector(c) => {
                    let after_fiber = first.is_some_and(|f| i > f);
                    if after_fiber && (c.offset != 0.0 || c.tilt != 0.0) {
                        push(
                            Some(i),
                            Severity::Error,
                            "Offset/tilt between fibers redistributes power between modes \
                             (mode coupling), which is not modeled yet. Set offset and tilt \
                             to 0 or move the connector before the first fiber."
                                .into(),
                        );
                    }
                    if !after_fiber {
                        connectors_before += 1;
                    }
                    if c.loss_db < 0.0 {
                        push(
                            Some(i),
                            Severity::Error,
                            "Connector loss must be ≥ 0 dB.".into(),
                        );
                    }
                }
                Element::Attenuator(a) => {
                    if a.loss_db < 0.0 {
                        push(
                            Some(i),
                            Severity::Error,
                            "Attenuation must be ≥ 0 dB.".into(),
                        );
                    }
                }
                Element::Fiber(f) => {
                    if f.length < 0.0 {
                        push(
                            Some(i),
                            Severity::Error,
                            "Fiber length must be ≥ 0 m.".into(),
                        );
                    }
                }
            }
        }
        if connectors_before > 1 {
            push(
                None,
                Severity::Warning,
                "Several connectors before the first fiber: only the last one defines the \
                 launch offset/tilt; all add their loss."
                    .into(),
            );
        }
        if let Some(f0) = first {
            let Element::Fiber(ref a) = self.elements[f0] else {
                unreachable!()
            };
            for (i, b) in self.fibers().skip(1) {
                if b.core_radius != a.core_radius
                    || b.na != a.na
                    || b.grid_points_per_radius != a.grid_points_per_radius
                    || b.domain_factor != a.domain_factor
                {
                    push(
                        Some(i),
                        Severity::Error,
                        "All fiber spans in a lane must share core radius, NA and solver grid \
                         (modes are mapped by their LP label)."
                            .into(),
                    );
                }
            }
        } else {
            push(
                None,
                Severity::Warning,
                "No fiber in this lane: back-to-back link.".into(),
            );
        }
        out
    }
}

impl SystemConfig {
    pub fn validate(&self) -> Vec<Issue> {
        let mut v: Vec<Issue> = self
            .lanes
            .iter()
            .enumerate()
            .flat_map(|(i, l)| l.validate(i))
            .collect();
        if self.lanes.is_empty() {
            v.push(Issue {
                lane: 0,
                element: None,
                severity: Severity::Error,
                message: "The system has no lanes.".into(),
            });
        }
        v
    }

    pub fn has_errors(&self) -> bool {
        self.validate()
            .iter()
            .any(|i| i.severity == Severity::Error)
    }
}

/// Node id of lane element `i` in the lane's preset graph.
pub fn element_node(i: usize) -> String {
    format!("el{i}")
}

/// Preset graph of a lane: PRBS13Q → driver → VCSEL → elements → PIN → TIA →
/// Bessel–Thomson → FFE (`rx.ffe_taps` > 0) → receiver output. Seeds reproduce the pre-graph engine bit for bit.
pub fn lane_graph(cfg: &LaneConfig, resolution: Resolution, level: Level) -> GraphConfig {
    let mut g = GraphConfig {
        settings: Settings {
            baud: cfg.tx.baud,
            sps: cfg.tx.sps,
            pattern_periods: cfg.tx.pattern_periods,
            pattern_length: crate::signal::PATTERN_PERIOD,
            resolution,
            level,
            seed: cfg.seed,
            backward_waves: false,
        },
        ..Default::default()
    };
    let vcsel = VcselParams {
        p_avg_dbm: cfg.tx.p_avg_dbm,
        er_db: cfg.tx.er_db,
        vcsel: cfg.vcsel.clone(),
        beam_radius: cfg.source.beam_radius,
        vcsel_mode_groups: cfg.source.vcsel_mode_groups,
        rin_enabled: cfg.rx.rin_noise,
        seed: Some(cfg.seed ^ TX_SEED_XOR),
        stokes: [1.0, 0.0, 0.0, 0.0],
    };
    let rx_seed = cfg.seed ^ RX_SEED_XOR;
    g.node(
        "src",
        "prbs13q",
        json!({ "seed": cfg.tx.prbs_seed, "levels": cfg.tx.levels }),
    )
    .node(
        "drv",
        "driver",
        serde_json::to_value(crate::components::DriverParams {
            jitter: cfg.tx.jitter.clone(),
            seed: Some(cfg.seed ^ crate::components::JITTER_SEED_XOR),
        })
        .expect("serializable"),
    )
    .node(
        "vcsel",
        "vcsel",
        serde_json::to_value(&vcsel).expect("serializable"),
    )
    .wire("src.out", "drv.in")
    .wire("drv.out", "vcsel.in");
    let mut prev = "vcsel".to_string();
    for (i, e) in cfg.elements.iter().enumerate() {
        let (kind, params) = match e {
            Element::Connector(c) => ("connector", serde_json::to_value(c)),
            // A fiber with preset physics becomes a standard `mmf` node, others a custom fiber.
            Element::Fiber(f) => match crate::config::MmfType::matching(f).filter(|_| {
                (0.0..=crate::config::MMF_MAX_ATTENUATION_DB_PER_KM)
                    .contains(&f.attenuation_db_per_km)
            }) {
                Some(t) => (
                    "mmf",
                    Ok(json!({ "type": t, "length": f.length,
                               "attenuation_db_per_km": f.attenuation_db_per_km,
                               "extra_loss_db": f.extra_loss_db })),
                ),
                None => ("custom_fiber", serde_json::to_value(f)),
            },
            Element::Attenuator(a) => ("attenuator", serde_json::to_value(a)),
        };
        let id = element_node(i);
        g.node(&id, kind, params.expect("serializable"))
            .wire(&format!("{prev}.out"), &format!("{id}.in"));
        prev = id;
    }
    let rx = &cfg.rx;
    g.node(
        "pin",
        "pin",
        json!({ "responsivity": rx.responsivity, "detector_radius": rx.detector_radius,
                "shot_noise": rx.shot_noise, "noise_seed": crate::seed::to_json(rx_seed) }),
    )
    .node(
        "tia",
        "tia",
        json!({ "transimpedance": rx.transimpedance, "noise_density": rx.noise_density,
                "thermal_noise": rx.thermal_noise, "noise_seed": crate::seed::to_json(rx_seed) }),
    )
    .node(
        "rxf",
        "bessel_thomson",
        json!({ "f3db_rel": rx.bt_f3db_rel, "enabled": rx.filter_enabled }),
    )
    .wire(&format!("{prev}.out"), "pin.in")
    .wire("pin.out", "tia.in")
    .wire("tia.out", "rxf.in");
    let last = if rx.ffe_taps > 0 {
        // Receiver equalizer, adapted on the transmitted symbols (`docs/model/ffe.md`).
        g.node("ffe", "ffe", json!({ "n_taps": rx.ffe_taps }))
            .wire("rxf.out", "ffe.in")
            .wire("src.out", "ffe.ref");
        "ffe"
    } else {
        "rxf"
    };
    g.node("rx", "receiver_output", json!({}))
        .wire(&format!("{last}.out"), "rx.in");
    g
}

/// Simulation engine for one lane: builds the lane's preset graph and runs it on the graph
/// engine (with per-node caching).
pub struct LaneEngine {
    cfg: LaneConfig,
    resolution: Resolution,
    level: Level,
    engine: GraphEngine,
}

impl LaneEngine {
    /// Lane engine at `Mode` resolution and `Power` level (the validated reference path).
    pub fn new(cfg: LaneConfig) -> Self {
        Self::with_representation(cfg, Resolution::Mode, Level::Power)
    }

    pub fn with_representation(cfg: LaneConfig, resolution: Resolution, level: Level) -> Self {
        let g = lane_graph(&cfg, resolution, level);
        Self {
            cfg,
            resolution,
            level,
            engine: GraphEngine::new(g),
        }
    }

    pub fn config(&self) -> &LaneConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: LaneConfig) {
        self.cfg = cfg;
    }

    /// The underlying graph engine (after the last run).
    pub fn graph_engine(&self) -> &GraphEngine {
        &self.engine
    }

    fn sync(&mut self) -> Result<(), GraphError> {
        let g = lane_graph(&self.cfg, self.resolution, self.level);
        if *self.engine.graph() != g {
            self.engine.set_graph(g);
        }
        self.engine.run()
    }

    fn fiber_report(&self, element: usize) -> Option<(&Arc<ModeSet>, Option<&Arc<LaunchReport>>)> {
        match self.engine.report(&element_node(element))? {
            Report::FiberSpan { basis, launch } => Some((basis, launch.as_ref())),
            _ => None,
        }
    }

    fn launch_report(&self) -> Option<Arc<LaunchReport>> {
        let first = self.cfg.first_fiber()?;
        self.fiber_report(first).and_then(|(_, l)| l.cloned())
    }

    fn detection(&self) -> Option<Arc<DetectionReport>> {
        match self.engine.report("pin")? {
            Report::Detection(d) => Some(d.clone()),
            _ => None,
        }
    }

    /// Mode set of the first fiber span (labels for the whole lane).
    pub fn mode_set(&mut self) -> Option<Arc<ModeSet>> {
        self.sync().ok()?;
        self.launch_report().map(|l| l.modes.clone())
    }

    /// Mode set of the fiber at element index `element`.
    pub fn mode_set_of(&mut self, element: usize) -> Option<Arc<ModeSet>> {
        self.sync().ok()?;
        self.fiber_report(element).map(|(b, _)| b.clone())
    }

    /// Launch coupling into the first fiber.
    pub fn coupling(&mut self) -> Option<Arc<Coupling>> {
        self.sync().ok()?;
        self.launch_report().map(|l| l.coupling.clone())
    }

    /// Power-domain channel from source to detector.
    pub fn channel(&mut self) -> FiberChannel {
        self.sync().expect("valid lane");
        self.detection().expect("detection report").channels[0].clone()
    }

    /// Run the lane; panics if the lane is invalid (see [`LaneEngine::try_run`]).
    pub fn run(&mut self) -> LinkResult {
        self.try_run()
            .unwrap_or_else(|e| panic!("invalid lane: {e}"))
    }

    /// Run the lane and assemble the link result.
    pub fn try_run(&mut self) -> Result<LinkResult, GraphError> {
        self.sync()?;
        let e = &self.engine;
        let grid = e.grid();
        let fs = grid.fs();
        let missing = |what: &str| {
            GraphError(vec![crate::graph::GraphIssue {
                node: None,
                wire: None,
                message: format!("lane graph produced no {what}"),
            }])
        };
        let symbols = match e.output("src", "out") {
            Some(Signal::Data(d)) => d.symbols.as_ref().clone(),
            _ => return Err(missing("symbols")),
        };
        let tx = match e.report("vcsel") {
            Some(Report::Transmitter { tx }) => tx.clone(),
            _ => return Err(missing("transmitter report")),
        };
        let det = self
            .detection()
            .ok_or_else(|| missing("detection report"))?;
        let bn = match e.report("rxf") {
            Some(Report::Filter { noise_bandwidth }) => *noise_bandwidth,
            _ => return Err(missing("filter report")),
        };
        let rec = match e.report("rx") {
            Some(Report::Receiver(m)) => m.clone(),
            _ => return Err(missing("receiver report")),
        };
        let filtered = match e.output("rxf", "out") {
            Some(Signal::Electrical(x)) => x.clone(),
            _ => return Err(missing("receiver signal")),
        };
        let launch = self.launch_report();
        let mut spectral = self.engine.spectral();
        let sp = &mut spectral;

        // Reference-receiver optical waveform: detected power (incl. transmitter noise: RIN and
        // jitter) through the receive filter, without receiver noise. Terms are added relative
        // to the signal gain (RIN has the signal's gain, jitter its own).
        let mut reference_optical = filtered.noiseless_unscaled(sp);
        for (i, t) in filtered.noise.iter().enumerate() {
            if matches!(t.kind, NoiseKind::Rin | NoiseKind::Jitter) {
                if let Some(r) = filtered.given_term_unscaled(i, sp) {
                    let g = t.gain / filtered.gain;
                    reference_optical
                        .iter_mut()
                        .zip(&r)
                        .for_each(|(a, b)| *a += if t.kind == NoiseKind::Rin { *b } else { g * b });
                }
            }
        }
        let rx = &self.cfg.rx;
        let s_th = if rx.thermal_noise {
            rx.noise_density.powi(2)
        } else {
            0.0
        };
        let rxo = RxOutput {
            signal: rec.signal.clone(),
            noiseless: rec.noiseless.clone(),
            sigma: rec.sigma.clone(),
            reference_optical,
            noise_bandwidth: bn,
            thermal_sigma: rx.transimpedance * (s_th * bn).sqrt(),
        };

        let ch = &det.channels[0];
        let (mode_reports, v_number, n_lp, n_sp, guided, ef45, ef19, emb, ofl) = match &launch {
            Some(l) => {
                let set = &l.modes;
                let cpl = &l.coupling;
                let lp01 = set.modes.first().map_or(0.0, |m| m.group_delay);
                let path: HashMap<(u32, u32), f64> =
                    det.path.iter().map(|p| ((p.l, p.m), p.delay)).collect();
                let lp01_path = set
                    .modes
                    .first()
                    .and_then(|m| path.get(&(m.l, m.m)))
                    .copied()
                    .unwrap_or(0.0);
                let reports = set
                    .modes
                    .iter()
                    .zip(&cpl.modes)
                    .map(|(m, c)| ModeReport {
                        l: m.l,
                        m: m.m,
                        group: m.group,
                        n_eff: m.n_eff,
                        relative_delay: m.group_delay - lp01,
                        path_delay: path.get(&(m.l, m.m)).copied().unwrap_or(0.0) - lp01_path,
                        chromatic_slope: m.chromatic_slope,
                        launched_power: c.power,
                    })
                    .collect();
                (
                    reports,
                    set.v_number,
                    set.modes.len(),
                    set.spatial_mode_count(),
                    cpl.guided_fraction,
                    encircled_flux(set, cpl, 4.5e-6),
                    encircled_flux(set, cpl, 19e-6),
                    l.emb_length,
                    l.ofl_bandwidth_length,
                )
            }
            None => (Vec::new(), 0.0, 0, 0, 1.0, f64::NAN, f64::NAN, None, None),
        };

        let avg = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
        let metrics = LinkMetrics {
            v_number,
            n_lp_modes: n_lp,
            n_spatial_modes: n_sp,
            guided_fraction: guided,
            encircled_flux_4p5um: ef45,
            encircled_flux_19um: ef19,
            ofl_bandwidth_length: ofl,
            emb_length: emb,
            channel_bandwidth: ch.bandwidth(),
            rms_delay_spread: ch.rms_delay_spread,
            mean_delay: ch.mean_delay,
            transmission: ch.transmission,
            tx_avg_power_dbm: w_to_dbm(avg(&tx.power)),
            rx_avg_power_dbm: w_to_dbm(avg(&det.p_det).max(1e-30)),
            tx_oma_outer: tx.levels[tx.levels.len() - 1] - tx.levels[0],
            vcsel_bandwidth: tx.vcsel_bandwidth,
            rx_noise_bandwidth: bn,
            rx_thermal_sigma: rxo.thermal_sigma,
        };

        Ok(LinkResult {
            sample_rate: fs,
            baud: grid.baud,
            sps: grid.sps,
            symbols,
            tx_levels: tx.levels.clone(),
            tx_power: tx.power.clone(),
            rx_optical: det.p_det.as_ref().clone(),
            rx: rxo,
            modes: mode_reports,
            metrics,
        })
    }
}

/// Error returned when simulating an invalid system.
#[derive(Clone, Debug)]
pub struct InvalidSystem(pub Vec<Issue>);

impl std::fmt::Display for InvalidSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msgs: Vec<String> = self
            .0
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| match i.element {
                Some(e) => format!("lane {} element {}: {}", i.lane + 1, e + 1, i.message),
                None => format!("lane {}: {}", i.lane + 1, i.message),
            })
            .collect();
        write!(f, "{}", msgs.join("; "))
    }
}

impl std::error::Error for InvalidSystem {}

/// Multi-lane simulator. Each lane has its own engine and caches, so editing one lane never
/// recomputes (or changes) another.
pub struct SystemSimulator {
    cfg: SystemConfig,
    lanes: Vec<LaneEngine>,
}

impl SystemSimulator {
    pub fn new(cfg: SystemConfig) -> Self {
        let lanes = cfg.lanes.iter().cloned().map(LaneEngine::new).collect();
        Self { cfg, lanes }
    }

    pub fn config(&self) -> &SystemConfig {
        &self.cfg
    }

    /// Replace the configuration, keeping engines (and their caches) of existing lanes.
    pub fn set_config(&mut self, cfg: SystemConfig) {
        self.lanes.truncate(cfg.lanes.len());
        for (i, l) in cfg.lanes.iter().enumerate() {
            if i < self.lanes.len() {
                self.lanes[i].set_config(l.clone());
            } else {
                self.lanes.push(LaneEngine::new(l.clone()));
            }
        }
        self.cfg = cfg;
    }

    pub fn validate(&self) -> Vec<Issue> {
        self.cfg.validate()
    }

    pub fn lane(&mut self, i: usize) -> Option<&mut LaneEngine> {
        self.lanes.get_mut(i)
    }

    /// Run one lane (errors if the system is invalid).
    pub fn run_lane(&mut self, i: usize) -> Result<LinkResult, InvalidSystem> {
        let issues = self
            .cfg
            .lanes
            .get(i)
            .map(|l| l.validate(i))
            .unwrap_or_else(|| {
                vec![Issue {
                    lane: i,
                    element: None,
                    severity: Severity::Error,
                    message: "No such lane.".into(),
                }]
            });
        if issues.iter().any(|x| x.severity == Severity::Error) {
            return Err(InvalidSystem(issues));
        }
        self.lanes[i].try_run().map_err(|e| {
            InvalidSystem(
                e.0.into_iter()
                    .map(|g| Issue {
                        lane: i,
                        element: g
                            .node
                            .as_deref()
                            .and_then(|n| n.strip_prefix("el"))
                            .and_then(|k| k.parse().ok()),
                        severity: Severity::Error,
                        message: g.message,
                    })
                    .collect(),
            )
        })
    }

    /// Run all lanes.
    pub fn run(&mut self) -> Result<Vec<LinkResult>, InvalidSystem> {
        let issues = self.validate();
        if issues.iter().any(|x| x.severity == Severity::Error) {
            return Err(InvalidSystem(issues));
        }
        (0..self.lanes.len()).map(|i| self.run_lane(i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::db_to_lin;

    fn rel_max(a: &[f64], b: &[f64]) -> f64 {
        let scale = b.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        a.iter()
            .zip(b)
            .fold(0.0f64, |m, (x, y)| m.max((x - y).abs()))
            / scale
    }

    fn fiber(len: f64) -> Element {
        Element::Fiber(FiberConfig {
            length: len,
            grid_points_per_radius: 300,
            ..FiberConfig::default()
        })
    }

    fn lane(elements: Vec<Element>) -> LaneConfig {
        LaneConfig {
            elements,
            ..LaneConfig::default()
        }
    }

    #[test]
    fn serde_roundtrip() {
        let s = SystemConfig::default();
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"kind\":\"fiber\""), "{j}");
        let back: SystemConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
        // Minimal JSON with defaults.
        let m: SystemConfig = serde_json::from_str(
            r#"{"lanes":[{"elements":[{"kind":"connector","offset":3e-6},{"kind":"fiber","length":50},{"kind":"attenuator","loss_db":3}]}]}"#,
        )
        .unwrap();
        assert_eq!(m.lanes[0].elements.len(), 3);
        assert_eq!(m.lanes[0].launch().offset, 3e-6);
    }

    /// R2: 60 m + ideal connector + 40 m equals 100 m.
    #[test]
    fn r2_span_concatenation() {
        let mut a = LaneEngine::new(lane(vec![
            Element::Connector(ConnectorConfig::default()),
            fiber(100.0),
        ]));
        let mut b = LaneEngine::new(lane(vec![
            Element::Connector(ConnectorConfig::default()),
            fiber(60.0),
            Element::Connector(ConnectorConfig::default()),
            fiber(40.0),
        ]));
        let (ra, rb) = (a.run(), b.run());
        assert!(rel_max(&rb.rx.noiseless, &ra.rx.noiseless) < 1e-9);
        assert!(rel_max(&rb.rx.sigma, &ra.rx.sigma) < 1e-9);
        assert!((ra.metrics.rms_delay_spread - rb.metrics.rms_delay_spread).abs() < 1e-18);
    }

    /// R3: an attenuator of X dB equals the fiber's extra loss of X dB.
    #[test]
    fn r3_attenuator_equals_extra_loss() {
        let mut f = FiberConfig {
            length: 80.0,
            grid_points_per_radius: 300,
            ..FiberConfig::default()
        };
        let a = LaneEngine::new(lane(vec![
            Element::Fiber(f.clone()),
            Element::Attenuator(AttenuatorConfig { loss_db: 3.0 }),
        ]))
        .run();
        f.extra_loss_db = 3.0;
        let b = LaneEngine::new(lane(vec![Element::Fiber(f)])).run();
        assert!(rel_max(&a.rx.noiseless, &b.rx.noiseless) < 1e-12);
        assert!((a.metrics.transmission / b.metrics.transmission - 1.0).abs() < 1e-12);
    }

    /// R4: lanes are independent.
    #[test]
    fn r4_lane_independence() {
        let base = SystemConfig {
            lanes: vec![lane(vec![fiber(50.0)]), lane(vec![fiber(120.0)])],
        };
        let mut sim = SystemSimulator::new(base.clone());
        let r1 = sim.run_lane(0).unwrap();
        let mut changed = base;
        changed.lanes[1].elements = vec![fiber(10.0)];
        changed.lanes[1].seed = 99;
        sim.set_config(changed);
        let r1b = sim.run_lane(0).unwrap();
        assert_eq!(r1.rx.signal, r1b.rx.signal);
        let r2 = sim.run_lane(1).unwrap();
        assert_ne!(r2.rx.signal, r1.rx.signal);
    }

    #[test]
    fn back_to_back_lane() {
        let mut e = LaneEngine::new(lane(vec![Element::Attenuator(AttenuatorConfig {
            loss_db: 1.0,
        })]));
        let r = e.run();
        assert!((r.metrics.transmission - db_to_lin(-1.0)).abs() < 1e-15);
        assert!(r.modes.is_empty());
        assert!(r.rx.signal.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn validation_rules() {
        let bad = lane(vec![
            fiber(50.0),
            Element::Connector(ConnectorConfig {
                offset: 2e-6,
                ..Default::default()
            }),
            fiber(50.0),
        ]);
        let issues = bad.validate(0);
        assert!(issues
            .iter()
            .any(|i| i.severity == Severity::Error && i.element == Some(1)));
        let other = FiberConfig {
            core_radius: 31.25e-6,
            ..FiberConfig::default()
        };
        let mixed = lane(vec![fiber(50.0), Element::Fiber(other)]);
        assert!(mixed
            .validate(0)
            .iter()
            .any(|i| i.severity == Severity::Error && i.element == Some(1)));
        let sys = SystemConfig { lanes: vec![bad] };
        assert!(sys.has_errors());
        assert!(SystemSimulator::new(sys).run().is_err());
        assert!(!SystemConfig::default().has_errors());
    }
}

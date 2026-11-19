//! Built-in graph components. The physics is the validated code of the `tx`, `modes`,
//! `coupling`, `fiber` and `rx` modules, arranged as nodes.
//!
//! | kind | inputs → outputs |
//! |---|---|
//! | `prbs13q` | → `out` (data) |
//! | `driver` | `in` (data) → `out` (electrical, normalized drive 0…1) |
//! | `vcsel` | `in` (drive) → `out` (optical) |
//! | `connector`, `attenuator`, `fiber` | `in` → `out` (optical) |
//! | `splitter` | `in` → `out0` … `outN−1` (optical) |
//! | `combiner` | `in0` … `inN−1` → `out` (optical) |
//! | `bandpass_filter` | `in` → `out` (optical, wavelength-selective) |
//! | `wdm_mux`, `wdm_demux` | `in0` … → `out` / `in` → `out0` … (optical, per-channel passbands) |
//! | `probe` | `in` → `out`, `tap` (optical; ideal or realistic tap) |
//! | `power_meter` | `in` (optical, monitor) |
//! | `pin` | `in` (optical) → `out` (current) |
//! | `tia` | `in` (current) → `out` (voltage); nonlinear with `clip` |
//! | `bessel_thomson` | `in` → `out` (electrical) |
//! | `ffe` | `in` (electrical), `ref` (data) → `out` (electrical): receiver equalizer |
//! | `receiver_output` | `in` (electrical): materializes waveforms and σ(t) (formerly `decision`) |
//!
//! Custom kinds are registered through [`crate::custom`].

use crate::component::*;
use crate::config::{
    FiberConfig, JitterConfig, LaunchConfig, MmfType, TxConfig, VcselConfig,
    MMF_MAX_ATTENUATION_DB_PER_KM,
};
use crate::dsp::{freq, BesselThomson4};
use crate::graph::{NodeInfo, Registry};
use crate::modes::{ModeSet, SolverParams};
use crate::prbs::PRBS13_DEFAULT_SEED;
use crate::rng::Rng;
use crate::signal::*;
use crate::system::{AttenuatorConfig, ConnectorConfig};
use crate::tx::{self, TxOutput, VcselResponse};
use crate::units::{attenuation_transmission, db_to_lin, dbm_to_w, Q_E};
use num_complex::Complex64;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Seed XOR used by the transmitter noise stream (unchanged from the pre-graph engine).
pub const TX_SEED_XOR: u64 = 0x7458_0000_0000_0001;
/// Seed XOR used by the driver's random-jitter stream.
pub const JITTER_SEED_XOR: u64 = 0x4a54_0000_0000_0003;
/// Seed XOR used by the receiver noise stream.
pub const RX_SEED_XOR: u64 = 0x5258_0000_0000_0002;
/// Random-stream order of receiver noise terms sharing one seed.
const THERMAL_ORDER: u32 = 0;
const SHOT_ORDER: u32 = 1;

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub(crate) fn default_seed(info: &NodeInfo, xor: u64) -> u64 {
    info.settings.seed ^ xor ^ fnv(info.id)
}

pub(crate) fn parse<T: DeserializeOwned + Default>(v: &serde_json::Value) -> Result<T, String> {
    if v.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid parameters: {e}"))
}

pub(crate) fn optical() -> PortType {
    PortType::Optical
}

pub(crate) fn one_optical(sig: OpticalSignal) -> Output {
    Output::signals(vec![Signal::Optical(Arc::new(OpticalWave::forward(sig)))])
}

fn one_electrical(e: ElectricalSignal) -> Output {
    Output::signals(vec![Signal::Electrical(Arc::new(e))])
}

/// Default parameters of a built-in kind, as JSON (for editors: new nodes start with
/// explicit values). `None` for unknown kinds.
pub fn default_params(kind: &str) -> Option<serde_json::Value> {
    let v = |x: serde_json::Result<serde_json::Value>| x.expect("serializable");
    Some(match kind {
        "prbs13q" => v(serde_json::to_value(PrbsParams::default())),
        "power_meter" | "receiver_output" => serde_json::json!({}),
        "driver" => v(serde_json::to_value(DriverParams::default())),
        "vcsel" => v(serde_json::to_value(VcselParams::default())),
        "connector" => v(serde_json::to_value(ConnectorConfig::default())),
        "attenuator" => v(serde_json::to_value(AttenuatorConfig::default())),
        "custom_fiber" => v(serde_json::to_value(FiberConfig::default())),
        "mmf" => v(serde_json::to_value(MmfParams::default())),
        "splitter" => v(serde_json::to_value(SplitterParams::default())),
        "combiner" => v(serde_json::to_value(CombinerParams::default())),
        "probe" => v(serde_json::to_value(ProbeParams::default())),
        "pin" => v(serde_json::to_value(PinParams::default())),
        "tia" => v(serde_json::to_value(TiaParams::default())),
        "bessel_thomson" => v(serde_json::to_value(BtParams::default())),
        "ffe" => v(serde_json::to_value(crate::ffe::FfeParams::default())),
        "bandpass_filter" => v(serde_json::to_value(crate::wdm::BandpassParams::default())),
        "wdm_mux" | "wdm_demux" => v(serde_json::to_value(crate::wdm::WdmParams::default())),
        crate::example::KIND => v(serde_json::to_value(
            crate::example::RustExampleParams::default(),
        )),
        _ => return None,
    })
}

/// Full fiber parameters (a `custom_fiber` configuration) of a fiber node of kind `mmf` or
/// `custom_fiber`; `None` for other kinds or invalid parameters.
pub fn fiber_config(kind: &str, params: &serde_json::Value) -> Option<FiberConfig> {
    match crate::graph::canonical_kind(kind) {
        "mmf" => MmfParams::fiber(params).ok(),
        "custom_fiber" => serde_json::from_value(params.clone()).ok(),
        _ => None,
    }
}

/// Register all built-in components.
pub fn register_builtins(r: &mut Registry) {
    r.register_builtin("prbs13q", |p, i| Ok(Box::new(Prbs::new(p, i)?)));
    r.register_builtin("driver", |p, i| Ok(Box::new(Driver::new(p, i)?)));
    r.register_builtin("vcsel", |p, i| Ok(Box::new(Vcsel::new(p, i)?)));
    r.register_builtin("connector", |p, _| Ok(Box::new(Connector(parse(p)?))));
    r.register_builtin("attenuator", |p, _| Ok(Box::new(Attenuator(parse(p)?))));
    r.register_builtin("custom_fiber", |p, _| Ok(Box::new(Fiber(parse(p)?))));
    r.register_builtin("mmf", |p, _| Ok(Box::new(Fiber(MmfParams::fiber(p)?))));
    r.register_builtin("splitter", |p, _| Ok(Box::new(Splitter::new(p)?)));
    r.register_builtin("combiner", |p, _| Ok(Box::new(Combiner::new(p)?)));
    r.register_builtin("probe", |p, _| Ok(Box::new(Probe(parse(p)?))));
    r.register_builtin("power_meter", |_, _| Ok(Box::new(PowerMeter)));
    r.register_builtin("pin", |p, i| Ok(Box::new(Pin::new(p, i)?)));
    r.register_builtin("tia", |p, i| Ok(Box::new(Tia::new(p, i)?)));
    r.register_builtin("bessel_thomson", |p, _| Ok(Box::new(Bt(parse(p)?))));
    r.register_builtin("ffe", |p, _| Ok(Box::new(crate::ffe::Ffe::new(parse(p)?)?)));
    r.register_builtin("bandpass_filter", |p, _| {
        Ok(Box::new(crate::wdm::Bandpass::new(p)?))
    });
    r.register_builtin("wdm_mux", |p, _| Ok(Box::new(crate::wdm::WdmMux::new(p)?)));
    r.register_builtin("wdm_demux", |p, _| {
        Ok(Box::new(crate::wdm::WdmDemux::new(p)?))
    });
    r.register_builtin("receiver_output", |_, _| Ok(Box::new(ReceiverOutput)));
    // Example component written in Rust (docs/model/rust-example.md).
    r.register_builtin(crate::example::KIND, |p, _| {
        Ok(Box::new(crate::example::RustExample::new(p)?))
    });
}

// ---------------------------------------------------------------------------------------------
// Pattern and driver
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct PrbsParams {
    seed: Option<u16>,
    /// Alphabet size: 4 = PAM-4 (PRBS13Q), 2 = NRZ (PRBS13).
    levels: u8,
}

impl Default for PrbsParams {
    fn default() -> Self {
        Self {
            seed: None,
            levels: 4,
        }
    }
}

/// Pattern source (node kind `prbs13q`): PRBS13Q for PAM-4 or PRBS13 for NRZ.
struct Prbs {
    seed: u16,
    levels: u8,
}

impl Prbs {
    fn new(p: &serde_json::Value, _: &NodeInfo) -> Result<Self, String> {
        let p: PrbsParams = parse(p)?;
        let seed = p.seed.unwrap_or(PRBS13_DEFAULT_SEED);
        if seed & 0x1FFF == 0 {
            return Err("PRBS13 seed must be nonzero (13 bits).".into());
        }
        crate::prbs::pattern(p.levels, seed)?;
        Ok(Self {
            seed,
            levels: p.levels,
        })
    }
}

impl Component for Prbs {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", PortType::Data)]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, _: &[Signal]) -> Result<Output, String> {
        let one = crate::prbs::pattern(self.levels, self.seed)?;
        let periods = ctx.grid.n_symbols / PATTERN_PERIOD;
        let mut s = Vec::with_capacity(ctx.grid.n_symbols);
        for _ in 0..periods {
            s.extend_from_slice(&one);
        }
        Ok(Output::signals(vec![Signal::Data(Arc::new(DataSignal {
            symbols: Arc::new(s),
            levels: self.levels,
        }))]))
    }
}

/// Driver parameters (node kind `driver`): transmitter jitter and the seed of its random
/// part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverParams {
    #[serde(flatten)]
    pub jitter: JitterConfig,
    pub seed: Option<u64>,
}

/// RJ above this fraction of a UI makes the first-order slope-noise model questionable.
const RJ_WARN_UI: f64 = 0.05;

/// Driver: maps the symbols to the normalized drive levels k/(M−1) (PAM-4: 0, ⅓, ⅔, 1;
/// NRZ: 0, 1) as a band-limited NRZ-hold waveform (`eq:tx-nrz`), with optional transmitter
/// jitter (`eq:tx-jitter`): deterministic edge shifts (DCD, SJ) in the spectrum
/// (`eq:tx-edge-shift`) and random jitter as an equivalent slope-noise term.
struct Driver {
    p: DriverParams,
    seed: u64,
}

impl Driver {
    fn new(p: &serde_json::Value, info: &NodeInfo) -> Result<Self, String> {
        let p: DriverParams = parse(p)?;
        let j = &p.jitter;
        for (name, v) in [
            ("rj_rms", j.rj_rms),
            ("dcd", j.dcd),
            ("sj_amplitude", j.sj_amplitude),
            ("cdr_corner", j.cdr_corner),
        ] {
            if !(v >= 0.0 && v.is_finite()) {
                return Err(format!("{name} must be ≥ 0."));
            }
        }
        if j.sj_amplitude > 0.0 && !(j.sj_frequency > 0.0 && j.sj_frequency.is_finite()) {
            return Err("sj_frequency must be > 0.".into());
        }
        let seed = p
            .seed
            .unwrap_or_else(|| default_seed(info, JITTER_SEED_XOR));
        Ok(Self { p, seed })
    }
}

/// Deterministic jitter of a pattern: edge shifts τₙ \[UI\] at every symbol boundary, the SJ
/// frequency actually used (rounded to the record's frequency grid) and warnings.
#[derive(Clone, Debug)]
pub struct DeterministicJitter {
    pub tau: Vec<f64>,
    pub sj_frequency: Option<f64>,
    pub warnings: Vec<String>,
}

/// Edge shifts of the configured DCD and SJ (`eq:tx-jitter`).
pub fn deterministic_jitter(
    j: &JitterConfig,
    symbols: &[u8],
    baud: f64,
) -> Result<DeterministicJitter, String> {
    let ns = symbols.len();
    let mut tau = vec![0.0; ns];
    let mut warnings = Vec::new();
    if j.dcd > 0.0 {
        let half = 0.5 * j.dcd * baud;
        for (i, t) in tau.iter_mut().enumerate() {
            let prev = symbols[(i + ns - 1) % ns];
            *t = match symbols[i].cmp(&prev) {
                std::cmp::Ordering::Greater => -half,
                std::cmp::Ordering::Less => half,
                std::cmp::Ordering::Equal => 0.0,
            };
        }
    }
    let mut f_used = None;
    if j.sj_amplitude > 0.0 {
        let f_rec = baud / ns as f64;
        let bins = (j.sj_frequency / f_rec).round().max(1.0);
        let f = bins * f_rec;
        if f >= 0.5 * baud {
            return Err("sj_frequency must be below half the symbol rate.".into());
        }
        if (f - j.sj_frequency).abs() > 1e-3 * j.sj_frequency {
            warnings.push(format!(
                "SJ frequency rounded to {:.4} MHz (multiple of the record's frequency \
                 resolution {:.4} MHz; more pattern periods give a finer grid).",
                f * 1e-6,
                f_rec * 1e-6
            ));
        }
        // Jitter relative to the recovered clock: × jf/(f_c + jf).
        let h = if j.cdr_corner > 0.0 {
            Complex64::new(0.0, f) / Complex64::new(j.cdr_corner, f)
        } else {
            Complex64::new(1.0, 0.0)
        };
        let a = j.sj_amplitude * baud * h.norm();
        let ph = h.arg();
        for (i, t) in tau.iter_mut().enumerate() {
            *t += a * (2.0 * std::f64::consts::PI * bins * i as f64 / ns as f64 + ph).sin();
        }
        f_used = Some(f);
    }
    Ok(DeterministicJitter {
        tau,
        sj_frequency: f_used,
        warnings,
    })
}

impl Component for Driver {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", PortType::Data)]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new(
            "out",
            PortType::Electrical(Some(Quantity::Dimensionless)),
        )]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let d = inputs[0].as_data()?;
        if d.symbols.len() != ctx.grid.n_symbols {
            return Err("symbol count does not match the simulation grid".into());
        }
        if !(d.levels == 2 || d.levels == 4) {
            return Err(format!(
                "the driver maps NRZ (2) or PAM-4 (4) symbols, got {} levels",
                d.levels
            ));
        }
        let levels = tx::drive_levels(d.levels);
        let (sps, baud) = (ctx.grid.sps, ctx.grid.baud);
        let j = &self.p.jitter;
        let mut warnings = Vec::new();
        let (base, tau) = if j.dcd > 0.0 || j.sj_amplitude > 0.0 {
            let dj = deterministic_jitter(j, &d.symbols, baud)?;
            warnings.extend(dj.warnings);
            let tau = dj.tau;
            // Edges must keep their order: consecutive edge times strictly increasing.
            let ed = tx::edges(&d.symbols, &levels, sps, &tau);
            let ns = d.symbols.len() as f64;
            for (a, b) in ed.iter().zip(ed.iter().cycle().skip(1)) {
                let ta = a.symbol as f64 + tau[a.symbol];
                let mut tb = b.symbol as f64 + tau[b.symbol];
                if b.symbol <= a.symbol {
                    tb += ns;
                }
                if tb <= ta {
                    return Err(
                        "Deterministic jitter larger than the edge spacing: edges would cross. \
                         Reduce dcd or sj_amplitude."
                            .into(),
                    );
                }
            }
            (
                tx::shifted_nrz_spectrum(&d.symbols, &levels, sps, &tau, &ctx.spectral),
                tau,
            )
        } else {
            (
                tx::nrz_spectrum(&d.symbols, &levels, sps),
                vec![0.0; d.symbols.len()],
            )
        };
        let mut noise = Vec::new();
        if j.rj_rms > 0.0 {
            // Equivalent slope noise: an edge Δ shifted by a Gaussian τ adds −Δ·τ·δ(t − tₑ),
            // a band-limited impulse at the edge's exact position. As per-sample white noise:
            // weight a = Δ·σ_RJ/Δt, strength S = 2a²/fs (variance a² for an impulse on the
            // grid); both trains are placed at the fractional positions spectrally.
            let fs = ctx.grid.fs();
            let sigma_ui = j.rj_rms * baud;
            if sigma_ui > RJ_WARN_UI {
                warnings.push(format!(
                    "RJ of {:.3} UI RMS: the slope-noise model assumes jitter much smaller than \
                     the edge rise time.",
                    sigma_ui
                ));
            }
            let mut rng = Rng::new(self.seed);
            let ed = tx::edges(&d.symbols, &levels, sps, &tau);
            let mut w = Vec::with_capacity(ed.len());
            let mut s = Vec::with_capacity(ed.len());
            for e in &ed {
                let a = e.step * sigma_ui * sps as f64;
                w.push((e.position, -a * rng.normal()));
                s.push((e.position, 2.0 * a * a / fs));
            }
            let mut s_spec = tx::impulse_train_spectrum(&s, &ctx.spectral);
            // Keep the strength's DC exact (Σ S); its Nyquist bin is not used.
            s_spec[0] = Complex64::new(s.iter().map(|x| x.1).sum(), 0.0);
            noise.push(NoiseTerm {
                kind: NoiseKind::Jitter,
                strength: Strength::Varying {
                    spectrum: Arc::new(s_spec),
                    waveform: None,
                },
                filter: None,
                gain: 1.0,
                realization: Realization::Given(Arc::new(tx::impulse_train_spectrum(
                    &w,
                    &ctx.spectral,
                ))),
            });
        }
        let mut out = one_electrical(ElectricalSignal {
            quantity: Quantity::Dimensionless,
            base: Arc::new(base),
            filter: None,
            gain: 1.0,
            noise,
            gaussian_exact: true,
            symbol_levels: Some(d.levels),
        });
        out.warnings = warnings;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// VCSEL
// ---------------------------------------------------------------------------------------------

/// VCSEL parameters (node kind `vcsel`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VcselParams {
    pub p_avg_dbm: f64,
    pub er_db: f64,
    pub vcsel: VcselConfig,
    pub beam_radius: f64,
    pub vcsel_mode_groups: u32,
    pub rin_enabled: bool,
    pub seed: Option<u64>,
    /// Emitted normalized Stokes vector; default unpolarized.
    pub stokes: [f64; 4],
}

impl Default for VcselParams {
    fn default() -> Self {
        let t = TxConfig::default();
        let l = LaunchConfig::default();
        Self {
            p_avg_dbm: t.p_avg_dbm,
            er_db: t.er_db,
            vcsel: VcselConfig::default(),
            beam_radius: l.beam_radius,
            vcsel_mode_groups: l.vcsel_mode_groups,
            rin_enabled: true,
            seed: None,
            stokes: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

struct Vcsel {
    p: VcselParams,
    seed: u64,
}

impl Vcsel {
    fn new(p: &serde_json::Value, info: &NodeInfo) -> Result<Self, String> {
        let p: VcselParams = parse(p)?;
        if p.stokes[0].is_nan() || p.stokes[0] <= 0.0 {
            return Err("stokes[0] must be positive (normalized Stokes vector)".into());
        }
        let seed = p.seed.unwrap_or_else(|| default_seed(info, TX_SEED_XOR));
        Ok(Self { p, seed })
    }
}

impl Component for Vcsel {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new(
            "in",
            PortType::Electrical(Some(Quantity::Dimensionless)),
        )]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        // Output power is clipped at 0 W.
        Linearity::Nonlinear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let drive = inputs[0].as_electrical()?;
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        let sp = &ctx.spectral;
        // M levels with the same outer levels for NRZ and PAM-4; M only affects the report.
        let levels = tx::power_levels_m(
            dbm_to_w(self.p.p_avg_dbm),
            db_to_lin(self.p.er_db),
            drive.symbol_levels.unwrap_or(4),
        );
        let (p_lo, p_hi) = (levels[0], levels[levels.len() - 1]);
        // P(t) = P₀ + (P_{M−1} − P₀)·d(t), then the VCSEL response (eq:tx-levels,
        // eq:vcsel-response).
        let span = (p_hi - p_lo) * drive.gain;
        let mut spec: Vec<Complex64> = drive.spectrum_unscaled().iter().map(|c| c * span).collect();
        spec[0] += p_lo * n as f64;
        let resp = VcselResponse::from_config(&self.p.vcsel);
        let h_v: Vec<Complex64> = (0..n).map(|k| resp.response(freq(k, n, fs))).collect();
        for (c, h) in spec.iter_mut().zip(&h_v) {
            *c *= h;
        }
        let mut power = sp.inverse_real(spec);
        for p in power.iter_mut() {
            *p = p.max(0.0);
        }
        let rin_lin = if self.p.rin_enabled {
            db_to_lin(self.p.vcsel.rin_db_hz)
        } else {
            0.0
        };
        let rin: Vec<f64> = if self.p.rin_enabled {
            let mut rng = Rng::new(self.seed);
            let half = 0.5 * fs;
            power
                .iter()
                .map(|&p| (rin_lin * half).sqrt() * p * rng.normal())
                .collect()
        } else {
            vec![0.0; n]
        };
        let (power_spec, rin_spec) = sp.forward_pair(&power, &rin);
        let sq: Vec<f64> = power.iter().map(|p| p * p).collect();
        let power_sq_spec = sp.forward(&sq);
        let tx = Arc::new(TxOutput {
            symbols: vec![],
            levels,
            sample_rate: fs,
            power,
            rin,
            vcsel_bandwidth: resp.bandwidth_3db(),
        });
        let s0 = self.p.stokes[0];
        // Drive noise (transmitter jitter) becomes optical power noise: × span × H_vcsel.
        let mut tx_noise = Vec::new();
        let mut warnings = Vec::new();
        for t in &drive.noise {
            match &t.realization {
                Realization::Given(r) => {
                    let filter: Vec<Complex64> = match &t.filter {
                        Some(f) => f.iter().zip(&h_v).map(|(a, b)| a * b).collect(),
                        None => h_v.clone(),
                    };
                    tx_noise.push(SourceNoise {
                        kind: t.kind,
                        strength: t.strength.clone(),
                        filter: Arc::new(filter),
                        gain: t.gain * (p_hi - p_lo),
                        realization: r.clone(),
                    });
                }
                Realization::Generate { .. } => warnings.push(
                    "Drive noise generated before the VCSEL is not modeled and was ignored."
                        .to_string(),
                ),
            }
        }
        let source = Arc::new(SourceInfo {
            node: ctx.node.to_string(),
            tx: tx.clone(),
            power_spec: Arc::new(power_spec),
            rin_spec: Arc::new(rin_spec),
            power_sq_spec: Arc::new(power_sq_spec),
            rin_lin,
            wavelength: self.p.vcsel.wavelength,
            spectral_width_rms: self.p.vcsel.spectral_width_rms,
            spectrum_shape: SpectrumShape::Gaussian,
            beam_radius: self.p.beam_radius,
            vcsel_mode_groups: self.p.vcsel_mode_groups,
            stokes: [
                1.0,
                self.p.stokes[1] / s0,
                self.p.stokes[2] / s0,
                self.p.stokes[3] / s0,
            ],
            tx_noise,
        });
        let mut out = one_optical(OpticalSignal {
            level: ctx.settings.level,
            resolution: ctx.settings.resolution,
            contributions: vec![Contribution {
                source,
                prop: Propagation::FreeSpace {
                    offset: 0.0,
                    tilt: 0.0,
                    factor: 1.0,
                },
                h_extra: None,
            }],
        });
        out.report = Some(Report::Transmitter { tx });
        out.warnings = warnings;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// Optical path
// ---------------------------------------------------------------------------------------------

pub(crate) fn forward_of(s: &Signal) -> Result<&OpticalSignal, String> {
    Ok(&s.as_optical()?.forward)
}

struct Connector(ConnectorConfig);

impl Component for Connector {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let c = &self.0;
        if c.loss_db < 0.0 {
            return Err("Connector loss must be ≥ 0 dB.".into());
        }
        let t = db_to_lin(-c.loss_db);
        let mut s = forward_of(&inputs[0])?.clone();
        for k in s.contributions.iter_mut() {
            match &mut k.prop {
                Propagation::FreeSpace {
                    offset,
                    tilt,
                    factor,
                } => {
                    // The last connector before the fiber defines the launch.
                    *offset = c.offset;
                    *tilt = c.tilt;
                    *factor *= t;
                }
                Propagation::Guided { .. } => {
                    if c.offset != 0.0 || c.tilt != 0.0 {
                        return Err(
                            "Offset/tilt between fibers redistributes power between modes \
                                    (mode coupling), which is not modeled yet. Set offset and tilt \
                                    to 0 or move the connector before the first fiber."
                                .into(),
                        );
                    }
                    k.scale(t);
                }
            }
        }
        Ok(one_optical(s))
    }
}

struct Attenuator(AttenuatorConfig);

impl Component for Attenuator {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        if self.0.loss_db < 0.0 {
            return Err("Attenuation must be ≥ 0 dB.".into());
        }
        Ok(one_optical(
            forward_of(&inputs[0])?.scaled(db_to_lin(-self.0.loss_db)),
        ))
    }
}

/// Same fiber geometry and solver grid: modes can be mapped by their LP label.
fn same_geometry(a: &SolverParams, b: &SolverParams) -> bool {
    a.core_radius == b.core_radius
        && a.na == b.na
        && a.grid_points_per_radius == b.grid_points_per_radius
        && a.domain_factor == b.domain_factor
}

/// Mode bins after the launch at the chosen resolution.
fn launch_bins(
    set: &ModeSet,
    powers: &[f64],
    factor: f64,
    level: Level,
    resolution: Resolution,
    stokes: &[f64; 4],
) -> Vec<ModeBin> {
    match resolution {
        Resolution::Mode => set
            .modes
            .iter()
            .zip(powers)
            .map(|(m, &p)| ModeBin {
                group: m.group,
                members: vec![Member {
                    l: m.l,
                    m: m.m,
                    frac: 1.0,
                }],
                state: BinState::new(level, p * factor, stokes),
                delay: 0.0,
                slope: 0.0,
            })
            .collect(),
        Resolution::ModeGroup => {
            let mut groups: BTreeMap<u32, Vec<(u32, u32, f64)>> = BTreeMap::new();
            for (m, &p) in set.modes.iter().zip(powers) {
                groups.entry(m.group).or_default().push((m.l, m.m, p));
            }
            groups
                .into_iter()
                .map(|(g, ms)| {
                    let total: f64 = ms.iter().map(|x| x.2).sum();
                    let k = ms.len() as f64;
                    let members = ms
                        .iter()
                        .map(|&(l, m, p)| Member {
                            l,
                            m,
                            frac: if total > 0.0 { p / total } else { 1.0 / k },
                        })
                        .collect();
                    ModeBin {
                        group: g,
                        members,
                        state: BinState::new(level, total * factor, stokes),
                        delay: 0.0,
                        slope: 0.0,
                    }
                })
                .collect()
        }
    }
}

/// Add one span's delay, chromatic slope and loss to every mode bin (`eq:system-modal-path`).
fn propagate_span(
    mode_bins: &mut [ModeBin],
    set: &ModeSet,
    f: &FiberConfig,
    wavelength: f64,
    reference_delay: &mut f64,
) {
    let index: HashMap<(u32, u32), usize> = set
        .modes
        .iter()
        .enumerate()
        .map(|(k, m)| ((m.l, m.m), k))
        .collect();
    let tref = set.modes[0].group_delay;
    let t = attenuation_transmission(f.attenuation_at(wavelength), f.length)
        * db_to_lin(-f.extra_loss_db);
    for b in mode_bins.iter_mut() {
        let (mut d, mut s, mut lost) = (0.0, 0.0, 0.0);
        for mm in &b.members {
            match index.get(&(mm.l, mm.m)) {
                Some(&j) => {
                    let md = &set.modes[j];
                    d += mm.frac * ((md.group_delay - tref) * f.length);
                    s += mm.frac * (md.chromatic_slope * f.length);
                }
                // Not guided in this span: the member's power is lost.
                None => lost += mm.frac,
            }
        }
        b.delay += d;
        b.slope += s;
        if lost > 0.0 {
            let keep = 1.0 - lost;
            b.state.scale(t * keep);
            b.members.retain(|mm| index.contains_key(&(mm.l, mm.m)));
            if keep > 0.0 {
                b.members.iter_mut().for_each(|mm| mm.frac /= keep);
            }
        } else {
            b.state.scale(t);
        }
    }
    *reference_delay += tref * f.length;
}

/// Standard multimode fiber (node kind `mmf`): a preset type plus length and losses. The
/// physics (geometry, profile, material, solver) is fixed by the preset; to change it, use a
/// `custom_fiber` (the editor converts automatically).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MmfParams {
    #[serde(rename = "type")]
    pub fiber_type: MmfType,
    pub length: f64,
    pub attenuation_db_per_km: f64,
    pub extra_loss_db: f64,
}

impl Default for MmfParams {
    fn default() -> Self {
        let c = MmfType::Om4.config();
        Self {
            fiber_type: MmfType::Om4,
            length: c.length,
            attenuation_db_per_km: c.attenuation_db_per_km,
            extra_loss_db: c.extra_loss_db,
        }
    }
}

impl MmfParams {
    /// The fiber configuration of an `mmf` node's parameters.
    pub fn fiber(p: &serde_json::Value) -> Result<FiberConfig, String> {
        let m: MmfParams = serde_json::from_value(p.clone()).map_err(|e| {
            format!(
                "{e}. A multimode fiber (mmf) only takes type, length, attenuation_db_per_km \
                 and extra_loss_db; use a custom_fiber to change its physics"
            )
        })?;
        if !(0.0..=MMF_MAX_ATTENUATION_DB_PER_KM).contains(&m.attenuation_db_per_km) {
            return Err(format!(
                "{} attenuation must be 0 … {MMF_MAX_ATTENUATION_DB_PER_KM} dB/km (cabled \
                 limit); use a custom_fiber for other values",
                m.fiber_type.name()
            ));
        }
        Ok(FiberConfig {
            length: m.length,
            attenuation_db_per_km: m.attenuation_db_per_km,
            extra_loss_db: m.extra_loss_db,
            ..m.fiber_type.config()
        })
    }
}

/// Fiber span: `custom_fiber` (all parameters) and `mmf` (preset).
struct Fiber(FiberConfig);

impl Component for Fiber {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let f = &self.0;
        if f.length < 0.0 {
            return Err("Fiber length must be ≥ 0 m.".into());
        }
        let mut s = forward_of(&inputs[0])?.clone();
        let (level, resolution) = (s.level, s.resolution);
        let mut report: Option<Report> = None;
        for (ci, c) in s.contributions.iter_mut().enumerate() {
            let sp = SolverParams::from_fiber(f, c.source.wavelength);
            let set = ctx.shared.modes(&sp);
            let mut launch_report = None;
            let (mut mode_bins, mut reference_delay) = match &c.prop {
                Propagation::FreeSpace {
                    offset,
                    tilt,
                    factor,
                } => {
                    let launch = LaunchConfig {
                        beam_radius: c.source.beam_radius,
                        vcsel_mode_groups: c.source.vcsel_mode_groups,
                        offset: *offset,
                        tilt: *tilt,
                    };
                    let (cpl, emb) = ctx.shared.coupling(&set, &sp, &launch);
                    let ofl = ctx.shared.ofl(&set, &sp);
                    let powers: Vec<f64> = cpl.modes.iter().map(|m| m.power).collect();
                    let mode_bins =
                        launch_bins(&set, &powers, *factor, level, resolution, &c.source.stokes);
                    launch_report = Some(Arc::new(LaunchReport {
                        launch: launch.clone(),
                        modes: set.clone(),
                        coupling: cpl,
                        emb_length: emb,
                        ofl_bandwidth_length: ofl,
                    }));
                    (mode_bins, 0.0)
                }
                Propagation::Guided {
                    solver,
                    mode_bins,
                    reference_delay,
                    ..
                } => {
                    if !same_geometry(solver, &sp) {
                        return Err("All fiber spans in a path must share core radius, NA and \
                                    solver grid (modes are mapped by their LP label)."
                            .into());
                    }
                    (mode_bins.clone(), *reference_delay)
                }
            };
            propagate_span(
                &mut mode_bins,
                &set,
                f,
                c.source.wavelength,
                &mut reference_delay,
            );
            c.prop = Propagation::Guided {
                basis: set.clone(),
                solver: sp,
                mode_bins,
                reference_delay,
            };
            if ci == 0 {
                report = Some(Report::FiberSpan {
                    basis: set,
                    launch: launch_report,
                });
            }
        }
        let mut out = one_optical(s);
        out.report = report;
        Ok(out)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct SplitterParams {
    ratios: Vec<f64>,
    excess_loss_db: f64,
}

impl Default for SplitterParams {
    fn default() -> Self {
        Self {
            ratios: vec![0.5, 0.5],
            excess_loss_db: 0.0,
        }
    }
}

/// Power splitter (polarization-independent; S-matrix phases reserved for the `Field` level).
struct Splitter(SplitterParams);

impl Splitter {
    fn new(p: &serde_json::Value) -> Result<Self, String> {
        let p: SplitterParams = parse(p)?;
        if p.ratios.len() < 2 {
            return Err("A splitter needs at least two outputs.".into());
        }
        if p.ratios.iter().any(|r| *r < 0.0) || p.ratios.iter().sum::<f64>() > 1.0 + 1e-12 {
            return Err("Split ratios must be ≥ 0 and sum to at most 1.".into());
        }
        if p.excess_loss_db < 0.0 {
            return Err("Excess loss must be ≥ 0 dB.".into());
        }
        Ok(Self(p))
    }
}

impl Component for Splitter {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        (0..self.0.ratios.len())
            .map(|i| PortSpec::new(&format!("out{i}"), optical()))
            .collect()
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let s = forward_of(&inputs[0])?;
        let el = db_to_lin(-self.0.excess_loss_db);
        Ok(Output::signals(
            self.0
                .ratios
                .iter()
                .map(|r| Signal::Optical(Arc::new(OpticalWave::forward(s.scaled(r * el)))))
                .collect(),
        ))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct CombinerParams {
    inputs: usize,
    excess_loss_db: f64,
}

impl Default for CombinerParams {
    fn default() -> Self {
        Self {
            inputs: 2,
            excess_loss_db: 0.0,
        }
    }
}

/// Incoherent power combiner: contributions are kept separate and add at the detector.
struct Combiner(CombinerParams);

impl Combiner {
    fn new(p: &serde_json::Value) -> Result<Self, String> {
        let p: CombinerParams = parse(p)?;
        if p.inputs < 2 {
            return Err("A combiner needs at least two inputs.".into());
        }
        if p.excess_loss_db < 0.0 {
            return Err("Excess loss must be ≥ 0 dB.".into());
        }
        Ok(Self(p))
    }
}

impl Component for Combiner {
    fn inputs(&self) -> Vec<PortSpec> {
        (0..self.0.inputs)
            .map(|i| PortSpec::new(&format!("in{i}"), optical()))
            .collect()
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let el = db_to_lin(-self.0.excess_loss_db);
        let first = forward_of(&inputs[0])?;
        let mut out = OpticalSignal {
            level: first.level,
            resolution: first.resolution,
            contributions: vec![],
        };
        for i in inputs {
            out.contributions
                .extend(forward_of(i)?.scaled(el).contributions);
        }
        Ok(one_optical(out))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct ProbeParams {
    /// `false`: ideal, lossless tap. `true`: monitor tap with ratio and insertion loss.
    realistic: bool,
    tap_ratio: f64,
    insertion_loss_db: f64,
}

impl Default for ProbeParams {
    fn default() -> Self {
        Self {
            realistic: false,
            tap_ratio: 0.02,
            insertion_loss_db: 0.0,
        }
    }
}

/// Inline probe: `out` continues the signal, `tap` feeds analyzers (signal-spec 7).
struct Probe(ProbeParams);

impl Component for Probe {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![
            PortSpec::new("out", optical()),
            PortSpec::new("tap", optical()),
        ]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let wave = inputs[0].as_optical()?;
        if !self.0.realistic {
            // Ideal tap: the through signal is the input itself (bit-identical).
            return Ok(Output::signals(vec![
                Signal::Optical(wave.clone()),
                Signal::Optical(wave.clone()),
            ]));
        }
        let p = &self.0;
        if !(0.0..=1.0).contains(&p.tap_ratio) || p.insertion_loss_db < 0.0 {
            return Err("Tap ratio must be in [0, 1] and insertion loss ≥ 0 dB.".into());
        }
        let s = &wave.forward;
        let through = (1.0 - p.tap_ratio) * db_to_lin(-p.insertion_loss_db);
        Ok(Output::signals(vec![
            Signal::Optical(Arc::new(OpticalWave::forward(s.scaled(through)))),
            Signal::Optical(Arc::new(OpticalWave::forward(s.scaled(p.tap_ratio)))),
        ]))
    }
}

/// Average optical power (monitor input; does not affect the signal).
struct PowerMeter;

impl Component for PowerMeter {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::monitor("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        Ok(Output {
            report: Some(Report::PowerMeter {
                average_power: forward_of(&inputs[0])?.average_power(),
            }),
            ..Default::default()
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Receiver
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct PinParams {
    /// Responsivity at 850 nm \[A/W\].
    responsivity: f64,
    /// Wavelength dependence of the responsivity.
    responsivity_spectrum: ResponsivitySpectrum,
    detector_radius: Option<f64>,
    shot_noise: bool,
    noise_seed: Option<u64>,
}

impl Default for PinParams {
    fn default() -> Self {
        Self {
            responsivity: 0.5,
            responsivity_spectrum: ResponsivitySpectrum::ConstantQe,
            detector_radius: None,
            shot_noise: true,
            noise_seed: None,
        }
    }
}

/// How the photodiode responsivity depends on wavelength (`eq:rx-responsivity`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsivitySpectrum {
    /// Constant quantum efficiency: R(λ) = R₈₅₀·λ/850 nm (R = ηqλ/hc).
    #[default]
    ConstantQe,
    /// The same responsivity at every wavelength (versions ≤ 0.15).
    Flat,
}

impl ResponsivitySpectrum {
    /// R(λ)/R(850 nm); exactly 1 at 850 nm.
    pub fn relative(self, wavelength: f64) -> f64 {
        match self {
            Self::Flat => 1.0,
            Self::ConstantQe => wavelength / crate::config::REFERENCE_WAVELENGTH,
        }
    }
}

/// PIN photodiode: square-law detection in the incoherent limit, shot noise, RIN transfer.
struct Pin {
    p: PinParams,
    seed: u64,
}

impl Pin {
    fn new(p: &serde_json::Value, info: &NodeInfo) -> Result<Self, String> {
        let p: PinParams = parse(p)?;
        let seed = p
            .noise_seed
            .unwrap_or_else(|| default_seed(info, RX_SEED_XOR));
        Ok(Self { p, seed })
    }
}

/// Minimum relative mode-bin power for the partial-coherence validity check.
const COHERENCE_CHECK_REL: f64 = 0.01;
/// γ above which interference between mode bins is considered non-negligible.
const COHERENCE_CHECK_GAMMA: f64 = 0.1;

impl Component for Pin {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new(
            "out",
            PortType::Electrical(Some(Quantity::Current)),
        )]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let s = forward_of(&inputs[0])?;
        let grid = ctx.grid;
        let n = grid.n();
        let sp = &ctx.spectral;
        let r = self.p.responsivity;
        let rd = self.p.detector_radius;
        // `total`: spectrum of I(t)/R₈₅₀ (each source weighted by its relative responsivity);
        // `power`: spectrum of the detected optical power, if it differs.
        let mut total = vec![Complex64::new(0.0, 0.0); n];
        let mut power: Option<Vec<Complex64>> = None;
        let mut channels = Vec::new();
        let mut noise = Vec::new();
        let mut warnings = Vec::new();
        for c in &s.contributions {
            let ch = c.channel(rd);
            let mut h = crate::link::channel_spectrum(&ch, n, grid.fs());
            if let Some(x) = &c.h_extra {
                h = h.iter().zip(x.iter()).map(|(a, b)| a * b).collect();
            }
            let rel = self.p.responsivity_spectrum.relative(c.source.wavelength);
            if rel != 1.0 && power.is_none() {
                power = Some(total.clone());
            }
            if let Some(pw) = power.as_mut() {
                for (t, (p, hh)) in pw.iter_mut().zip(c.source.power_spec.iter().zip(&h)) {
                    *t += p * hh;
                }
            }
            for (t, (p, hh)) in total.iter_mut().zip(c.source.power_spec.iter().zip(&h)) {
                *t += if rel == 1.0 { p * hh } else { p * hh * rel };
            }
            let rc = r * rel;
            for sn in &c.source.tx_noise {
                noise.push(NoiseTerm {
                    kind: sn.kind,
                    strength: sn.strength.clone(),
                    filter: Some(Arc::new(
                        sn.filter.iter().zip(&h).map(|(a, b)| a * b).collect(),
                    )),
                    gain: sn.gain * rc,
                    realization: Realization::Given(sn.realization.clone()),
                });
            }
            if c.source.rin_lin > 0.0 {
                let rl = c.source.rin_lin;
                noise.push(NoiseTerm {
                    kind: NoiseKind::Rin,
                    strength: Strength::Varying {
                        spectrum: Arc::new(c.source.power_sq_spec.iter().map(|x| x * rl).collect()),
                        waveform: None,
                    },
                    filter: Some(Arc::new(h)),
                    gain: rc,
                    realization: Realization::Given(c.source.rin_spec.clone()),
                });
            }
            // Validity check of the incoherent model (signal-spec 9.4).
            if rd.is_some() && warnings.is_empty() {
                let big = ch.terms.iter().map(|t| t.weight).fold(0.0, f64::max);
                let sig: Vec<_> = ch
                    .terms
                    .iter()
                    .filter(|t| t.weight >= COHERENCE_CHECK_REL * big)
                    .collect();
                let mut worst: f64 = 0.0;
                for (i, a) in sig.iter().enumerate() {
                    for b in &sig[i + 1..] {
                        worst = worst.max(c.source.gamma(a.delay - b.delay));
                    }
                }
                if worst > COHERENCE_CHECK_GAMMA {
                    warnings.push(format!(
                        "The detector does not collect all light, and significantly excited \
                         modes are partially coherent (γ up to {worst:.2}). Interference between \
                         them (modal noise) is not modeled, so the power model may be optimistic."
                    ));
                }
            }
            channels.push(ch);
        }
        let i_rel = sp.inverse_real(total.clone());
        let p_det = match power {
            Some(pw) => sp.inverse_real(pw),
            None => i_rel.clone(),
        };
        if self.p.shot_noise {
            // S = 2q·I(t) = 2q·R₈₅₀·(I/R₈₅₀).
            let k = 2.0 * Q_E * r;
            noise.push(NoiseTerm {
                kind: NoiseKind::Shot,
                strength: Strength::Varying {
                    spectrum: Arc::new(total.iter().map(|x| x * k).collect()),
                    waveform: Some(Arc::new(i_rel.iter().map(|p| k * p).collect())),
                },
                filter: None,
                gain: 1.0,
                realization: Realization::Generate {
                    seed: self.seed,
                    order: SHOT_ORDER,
                },
            });
        }
        let path = match s.contributions.first().map(|c| &c.prop) {
            Some(Propagation::Guided { mode_bins, .. }) => mode_bins
                .iter()
                .flat_map(|b| {
                    b.members.iter().map(move |mm| PathEntry {
                        l: mm.l,
                        m: mm.m,
                        group: b.group,
                        delay: b.delay,
                    })
                })
                .collect(),
            _ => vec![],
        };
        let mut out = one_electrical(ElectricalSignal {
            quantity: Quantity::Current,
            base: Arc::new(total),
            filter: None,
            gain: r,
            noise,
            gaussian_exact: true,
            symbol_levels: None,
        });
        out.report = Some(Report::Detection(Arc::new(DetectionReport {
            channels,
            p_det: Arc::new(p_det),
            path,
        })));
        out.warnings = warnings;
        Ok(out)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct TiaParams {
    transimpedance: f64,
    noise_density: f64,
    thermal_noise: bool,
    noise_seed: Option<u64>,
    /// Output limits \[low, high\] \[V\]. Makes the TIA nonlinear (`eq:tia-clip`).
    clip: Option<[f64; 2]>,
}

impl Default for TiaParams {
    fn default() -> Self {
        Self {
            transimpedance: 1000.0,
            noise_density: 15e-12,
            thermal_noise: true,
            noise_seed: None,
            clip: None,
        }
    }
}

/// Transimpedance amplifier with input-referred thermal noise and optional output clipping.
struct Tia {
    p: TiaParams,
    seed: u64,
}

impl Tia {
    fn new(p: &serde_json::Value, info: &NodeInfo) -> Result<Self, String> {
        let p: TiaParams = parse(p)?;
        if let Some([lo, hi]) = p.clip {
            if lo.is_nan() || hi.is_nan() || lo >= hi {
                return Err("TIA clip limits must satisfy low < high.".into());
            }
        }
        let seed = p
            .noise_seed
            .unwrap_or_else(|| default_seed(info, RX_SEED_XOR));
        Ok(Self { p, seed })
    }
}

impl Component for Tia {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new(
            "in",
            PortType::Electrical(Some(Quantity::Current)),
        )]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new(
            "out",
            PortType::Electrical(Some(Quantity::Voltage)),
        )]
    }
    fn linearity(&self) -> Linearity {
        if self.p.clip.is_some() {
            Linearity::Nonlinear
        } else {
            Linearity::Linear
        }
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let z = self.p.transimpedance;
        let mut e = inputs[0].as_electrical()?.scale(z);
        e.quantity = Quantity::Voltage;
        if self.p.thermal_noise {
            e.noise.push(NoiseTerm {
                kind: NoiseKind::Thermal,
                strength: Strength::Constant(self.p.noise_density * self.p.noise_density),
                filter: None,
                gain: z,
                realization: Realization::Generate {
                    seed: self.seed,
                    order: THERMAL_ORDER,
                },
            });
        }
        let Some([lo, hi]) = self.p.clip else {
            return Ok(one_electrical(e));
        };
        // Nonlinear: materialize, then clip the noiseless and the noisy waveform. Noise
        // generated before the clipper is no longer Gaussian (signal-spec 6.3, case B).
        let m = e.materialize(&ctx.spectral, ctx.grid.fs());
        let clip = |v: &[f64]| -> Vec<f64> { v.iter().map(|x| x.clamp(lo, hi)).collect() };
        let (y0, y) = (clip(&m.noiseless), clip(&m.signal));
        let clipped = m.signal.iter().filter(|x| **x < lo || **x > hi).count();
        let mut out = one_electrical(ElectricalSignal::from_waveforms(
            Quantity::Voltage,
            &y0,
            &y,
            &ctx.spectral,
        ));
        if clipped > 0 {
            out.warnings.push(format!(
                "TIA output clipped in {:.3} % of samples.",
                100.0 * clipped as f64 / m.signal.len() as f64
            ));
        }
        Ok(out)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct BtParams {
    /// −3 dB frequency relative to the symbol rate.
    f3db_rel: f64,
    enabled: bool,
}

impl Default for BtParams {
    fn default() -> Self {
        Self {
            f3db_rel: 0.5,
            enabled: true,
        }
    }
}

/// 4th-order Bessel–Thomson receive filter (`eq:rx-bessel-thomson`).
struct Bt(BtParams);

impl Component for Bt {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", PortType::Electrical(None))]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", PortType::Electrical(None))]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let e = inputs[0].as_electrical()?;
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        if !self.0.enabled {
            let mut out = Output::signals(vec![inputs[0].clone()]);
            out.report = Some(Report::Filter {
                noise_bandwidth: 0.5 * fs,
            });
            return Ok(out);
        }
        let bt = BesselThomson4::new(self.0.f3db_rel * ctx.grid.baud);
        let h: Spectrum = Arc::new((0..n).map(|k| bt.response(freq(k, n, fs))).collect());
        let bn = noise_bandwidth(&Some(h.clone()), fs, n);
        let mut out = one_electrical(e.apply_filter(&h));
        out.report = Some(Report::Filter {
            noise_bandwidth: bn,
        });
        Ok(out)
    }
}

/// Receiver output (kind `receiver_output`, formerly `decision`): materializes the receiver
/// waveforms and σ(t) for analysis. It is not a slicer: thresholds and errors are computed
/// by the analysis (`pam4_analyzer`).
struct ReceiverOutput;

impl Component for ReceiverOutput {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::monitor("in", PortType::Electrical(None))]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Nonlinear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let e = inputs[0].as_electrical()?;
        let m = e.materialize(&ctx.spectral, ctx.grid.fs());
        let mut warnings = Vec::new();
        if !m.gaussian_exact {
            warnings.push(format!(
                "Noise has passed a nonlinear block, so σ(t) does not describe it and the \
                 semi-analytic SER is not valid. Use the counted (Monte Carlo) SER; it resolves \
                 SER ≳ {:.1e} with {} symbols.",
                1.0 / ctx.grid.n_symbols as f64,
                ctx.grid.n_symbols
            ));
        }
        Ok(Output {
            report: Some(Report::Receiver(Arc::new(m))),
            warnings,
            ..Default::default()
        })
    }
}

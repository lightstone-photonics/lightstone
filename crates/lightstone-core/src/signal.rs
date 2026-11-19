//! Signals flowing between graph components. Definitions: `docs/signal-spec.md`.
//!
//! * [`DataSignal`] – symbol stream
//! * [`ElectricalSignal`] – lazy waveform (base spectrum × filter × gain) plus symbolic noise
//! * [`OpticalWave`] – forward (and reserved backward) optical signal; per source a
//!   [`Contribution`] in free space or guided in a mode basis, with per-mode-bin state at the
//!   `Power`, `Stokes` or `Field` representation level.

use crate::dsp::Spectral;
use crate::fiber::{ChannelTerm, FiberChannel};
use crate::link::channel_spectrum;
use crate::modes::{power_within, ModeSet, SolverParams};
use crate::rng::Rng;
use crate::tx::TxOutput;
use crate::units::C0;
use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::PI;
use std::sync::Arc;

/// Spectrum on the simulation grid (FFT bin order).
pub type Spectrum = Arc<Vec<Complex64>>;

/// Symbols in one PRBS13Q period.
pub const PATTERN_PERIOD: usize = 8191;

// ---------------------------------------------------------------------------------------------
// Settings and time grid
// ---------------------------------------------------------------------------------------------

/// Mode-dimension resolution of optical signals (signal-spec 3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// One mode bin per principal mode group.
    ModeGroup,
    /// One mode bin per LP mode.
    Mode,
}

/// Representation level of optical signals (signal-spec 9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Incoherent, unpolarized power (S₀ only).
    Power,
    /// Incoherent, partially polarized power (Stokes vector).
    Stokes,
    /// Complex envelopes (mutually incoherent Jones vectors) – carries phase.
    Field,
}

/// Simulation-wide settings (one per graph).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Symbol rate \[Bd\].
    pub baud: f64,
    /// Samples per symbol.
    pub sps: usize,
    /// Number of PRBS13Q periods (8191 symbols each).
    pub pattern_periods: usize,
    pub resolution: Resolution,
    /// Level at which sources emit.
    pub level: Level,
    /// Base seed for components without an explicit seed.
    pub seed: u64,
    /// Reserved: propagate backward waves (signal-spec 11.1). No current component reflects.
    pub backward_waves: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            baud: 53.125e9,
            sps: 16,
            pattern_periods: 1,
            resolution: Resolution::ModeGroup,
            level: Level::Power,
            seed: 0,
            backward_waves: false,
        }
    }
}

/// Common periodic time grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeGrid {
    pub baud: f64,
    pub sps: usize,
    pub n_symbols: usize,
}

impl TimeGrid {
    pub fn new(s: &Settings) -> Self {
        Self {
            baud: s.baud,
            sps: s.sps.max(1),
            n_symbols: PATTERN_PERIOD * s.pattern_periods.max(1),
        }
    }

    /// Number of samples.
    pub fn n(&self) -> usize {
        self.n_symbols * self.sps
    }

    /// Sample rate \[Hz\].
    pub fn fs(&self) -> f64 {
        self.baud * self.sps as f64
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn mul(a: &[Complex64], b: &[Complex64]) -> Vec<Complex64> {
    a.iter().zip(b).map(|(x, y)| x * y).collect()
}

fn filtered(spec: &[Complex64], filter: &Option<Spectrum>) -> Vec<Complex64> {
    match filter {
        Some(h) => mul(spec, h),
        None => spec.to_vec(),
    }
}

fn ones(n: usize) -> Vec<Complex64> {
    vec![Complex64::new(1.0, 0.0); n]
}

/// Discrete noise bandwidth 0.5·fs·mean|H|² of a filter (identity: fs/2) \[Hz\].
pub fn noise_bandwidth(filter: &Option<Spectrum>, fs: f64, n: usize) -> f64 {
    match filter {
        Some(h) => 0.5 * fs * h.iter().map(|c| c.norm_sqr()).sum::<f64>() / n as f64,
        None => 0.5 * fs,
    }
}

// ---------------------------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------------------------

/// Symbol stream.
#[derive(Clone, Debug)]
pub struct DataSignal {
    pub symbols: Arc<Vec<u8>>,
    /// Alphabet size (4 for PAM-4).
    pub levels: u8,
}

// ---------------------------------------------------------------------------------------------
// Electrical
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    /// Normalized drive signal (0…1).
    Dimensionless,
    /// Current \[A\].
    Current,
    /// Voltage \[V\].
    Voltage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseKind {
    Thermal,
    Shot,
    Rin,
    /// Transmitter random jitter as equivalent slope noise (`eq:tx-jitter`).
    Jitter,
    Custom,
    /// Noise that has passed a nonlinear block: an explicit realization only. Its
    /// distribution is unknown, so it does not contribute to σ(t).
    Nonlinear,
}

/// Strength of a white Gaussian noise source: one-sided PSD at the point of generation.
#[derive(Clone, Debug)]
pub enum Strength {
    /// Constant PSD S \[unit²/Hz\].
    Constant(f64),
    /// Time-varying PSD S(t): spectrum of S(t) (for the variance) and, if the realization is
    /// generated by the engine, the waveform S(t).
    Varying {
        spectrum: Spectrum,
        waveform: Option<Arc<Vec<f64>>>,
    },
}

/// How the random realization of a noise term is obtained.
#[derive(Clone, Debug)]
pub enum Realization {
    /// Generated on materialization. Terms with the same seed draw from one random stream in
    /// ascending `order`.
    Generate { seed: u64, order: u32 },
    /// Already generated upstream (e.g. RIN at the transmitter): spectrum of the realization
    /// at the point of generation.
    Given(Spectrum),
}

/// One noise source with the filter and gain it has passed since generation (signal-spec 5).
#[derive(Clone, Debug)]
pub struct NoiseTerm {
    pub kind: NoiseKind,
    pub strength: Strength,
    pub filter: Option<Spectrum>,
    pub gain: f64,
    pub realization: Realization,
}

/// Lazy electrical signal: waveform = gain · IFFT(base · filter), plus symbolic noise.
#[derive(Clone, Debug)]
pub struct ElectricalSignal {
    pub quantity: Quantity,
    pub base: Spectrum,
    pub filter: Option<Spectrum>,
    pub gain: f64,
    pub noise: Vec<NoiseTerm>,
    /// True while every block since the noise sources was linear (σ(t) exact).
    pub gaussian_exact: bool,
    /// Alphabet size M of the symbol drive this signal carries (set by the driver: 2 for
    /// NRZ, 4 for PAM-4; kept through linear blocks), `None` if unknown. Informational: the
    /// transmitter uses it to report its M power levels.
    pub symbol_levels: Option<u8>,
}

/// Explicit receiver waveforms.
#[derive(Clone, Debug)]
pub struct MaterializedElectrical {
    pub noiseless: Vec<f64>,
    pub signal: Vec<f64>,
    pub sigma: Vec<f64>,
    pub gaussian_exact: bool,
}

impl ElectricalSignal {
    /// Signal from explicit waveforms, e.g. the output of a nonlinear block (signal-spec 6.3).
    /// The noise is carried as one explicit realization `noisy − noiseless`. If that is
    /// nonzero, its statistics are unknown and `gaussian_exact` is false.
    pub fn from_waveforms(
        quantity: Quantity,
        noiseless: &[f64],
        noisy: &[f64],
        sp: &Spectral,
    ) -> Self {
        let residual: Vec<f64> = noisy.iter().zip(noiseless).map(|(a, b)| a - b).collect();
        let has_noise = residual.iter().any(|x| *x != 0.0);
        let (base, noise) = if has_noise {
            let (b, r) = sp.forward_pair(noiseless, &residual);
            let term = NoiseTerm {
                kind: NoiseKind::Nonlinear,
                strength: Strength::Constant(0.0),
                filter: None,
                gain: 1.0,
                realization: Realization::Given(Arc::new(r)),
            };
            (b, vec![term])
        } else {
            (sp.forward(noiseless), vec![])
        };
        Self {
            quantity,
            base: Arc::new(base),
            filter: None,
            gain: 1.0,
            noise,
            gaussian_exact: !has_noise,
            symbol_levels: None,
        }
    }

    /// Spectrum of the noiseless waveform without the scalar gain.
    pub fn spectrum_unscaled(&self) -> Vec<Complex64> {
        filtered(&self.base, &self.filter)
    }

    /// Apply a linear filter H(f) to the signal and all its noise terms. Terms that share a
    /// filter keep sharing one (needed to combine their realizations).
    pub fn apply_filter(&self, h: &Spectrum) -> Self {
        let mut memo: HashMap<usize, Spectrum> = HashMap::new();
        let mut get = |f: &Option<Spectrum>| -> Spectrum {
            let key = f.as_ref().map_or(0, |a| Arc::as_ptr(a) as usize);
            memo.entry(key)
                .or_insert_with(|| match f {
                    None => h.clone(),
                    Some(a) => Arc::new(mul(a, h)),
                })
                .clone()
        };
        let filter = Some(get(&self.filter));
        let noise = self
            .noise
            .iter()
            .map(|t| NoiseTerm {
                filter: Some(get(&t.filter)),
                ..t.clone()
            })
            .collect();
        Self {
            filter,
            noise,
            ..self.clone()
        }
    }

    /// Multiply the signal and all noise terms by a scalar gain.
    pub fn scale(&self, g: f64) -> Self {
        Self {
            gain: self.gain * g,
            noise: self
                .noise
                .iter()
                .map(|t| NoiseTerm {
                    gain: t.gain * g,
                    ..t.clone()
                })
                .collect(),
            ..self.clone()
        }
    }

    /// Noiseless waveform without the scalar gain.
    pub fn noiseless_unscaled(&self, sp: &Spectral) -> Vec<f64> {
        sp.inverse_real(self.spectrum_unscaled())
    }

    pub fn noiseless(&self, sp: &Spectral) -> Vec<f64> {
        let g = self.gain;
        self.noiseless_unscaled(sp).iter().map(|v| g * v).collect()
    }

    /// Realization of term `i` filtered but without its gain (only for `Given` terms).
    pub fn given_term_unscaled(&self, i: usize, sp: &Spectral) -> Option<Vec<f64>> {
        match &self.noise.get(i)?.realization {
            Realization::Given(g) => Some(sp.inverse_real(filtered(g, &self.noise[i].filter))),
            Realization::Generate { .. } => None,
        }
    }

    /// Sum of all noise realizations at this point.
    pub fn noise_realization(&self, sp: &Spectral, fs: f64) -> Vec<f64> {
        let n = sp.len();
        let half = 0.5 * fs;
        let len = self.noise.len();
        // 1) Pre-filter white realizations, in the random-stream order of each seed.
        let mut pre: Vec<Option<Vec<f64>>> = vec![None; len];
        let mut streams: BTreeMap<u64, Vec<(u32, usize)>> = BTreeMap::new();
        for (i, t) in self.noise.iter().enumerate() {
            if let Realization::Generate { seed, order } = t.realization {
                streams.entry(seed).or_default().push((order, i));
            }
        }
        for (seed, mut terms) in streams {
            terms.sort();
            let mut rng = Rng::new(seed);
            for (_, i) in terms {
                let x: Vec<f64> = match &self.noise[i].strength {
                    Strength::Constant(s) => {
                        let a = (s * half).sqrt();
                        (0..n).map(|_| a * rng.normal()).collect()
                    }
                    Strength::Varying {
                        waveform: Some(w), ..
                    } => w
                        .iter()
                        .map(|&s| (s.max(0.0) * half).sqrt() * rng.normal())
                        .collect(),
                    Strength::Varying { waveform: None, .. } => {
                        panic!("generated noise term needs its strength waveform")
                    }
                };
                pre[i] = Some(x);
            }
        }
        // 2) Terms sharing filter and gain are filtered together (one FFT pair per group).
        let same_filter = |a: &Option<Spectrum>, b: &Option<Spectrum>| match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            _ => false,
        };
        let mut out = vec![0.0; n];
        let mut done = vec![false; len];
        for i in 0..len {
            if done[i] {
                continue;
            }
            let t = &self.noise[i];
            let y = match &t.realization {
                Realization::Given(g) => {
                    done[i] = true;
                    sp.inverse_real(filtered(g, &t.filter))
                }
                Realization::Generate { .. } => {
                    let mut sum = vec![0.0; n];
                    for j in i..len {
                        let u = &self.noise[j];
                        if done[j]
                            || !matches!(u.realization, Realization::Generate { .. })
                            || !same_filter(&u.filter, &t.filter)
                            || u.gain != t.gain
                        {
                            continue;
                        }
                        for (s, x) in sum.iter_mut().zip(pre[j].as_ref().expect("generated")) {
                            *s += x;
                        }
                        done[j] = true;
                    }
                    let spec = sp.forward(&sum);
                    sp.inverse_real(filtered(&spec, &t.filter))
                }
            };
            for (o, v) in out.iter_mut().zip(&y) {
                *o += t.gain * v;
            }
        }
        out
    }

    /// Exact per-sample noise standard deviation (signal-spec 5):
    /// σ²(t) = Σ gain²·(fs/2)·(g² ∗ S)(t), g = IFFT(filter).
    pub fn noise_sigma(&self, sp: &Spectral, fs: f64) -> Vec<f64> {
        let n = sp.len();
        let half = 0.5 * fs;
        let mut var = vec![0.0; n];
        let mut varying: Vec<(&Spectrum, &Option<Spectrum>, f64)> = Vec::new();
        for t in &self.noise {
            match &t.strength {
                Strength::Constant(s) => {
                    let v = t.gain * t.gain * s * noise_bandwidth(&t.filter, fs, n);
                    var.iter_mut().for_each(|x| *x += v);
                }
                Strength::Varying { spectrum, .. } => varying.push((spectrum, &t.filter, t.gain)),
            }
        }
        let filt = |f: &Option<Spectrum>| f.as_ref().map_or_else(|| ones(n), |h| h.to_vec());
        for chunk in varying.chunks(2) {
            if chunk.len() == 2 {
                let (sa, fa, ga) = chunk[0];
                let (sb, fb, gb) = chunk[1];
                let (g1, g2) = sp.inverse_real_pair(&filt(fa), &filt(fb));
                let q1: Vec<f64> = g1.iter().map(|g| g * g).collect();
                let q2: Vec<f64> = g2.iter().map(|g| g * g).collect();
                let (q1s, q2s) = sp.forward_pair(&q1, &q2);
                let va_spec: Vec<Complex64> =
                    sa.iter().zip(&q1s).map(|(s, q)| s * q * half).collect();
                let vb_spec: Vec<Complex64> =
                    sb.iter().zip(&q2s).map(|(s, q)| s * q * half).collect();
                let (va, vb) = sp.inverse_real_pair(&va_spec, &vb_spec);
                for k in 0..n {
                    var[k] += (ga * ga * va[k]).max(0.0) + (gb * gb * vb[k]).max(0.0);
                }
            } else {
                let (sa, fa, ga) = chunk[0];
                let g1 = sp.inverse_real(filt(fa));
                let q1: Vec<f64> = g1.iter().map(|g| g * g).collect();
                let q1s = sp.forward(&q1);
                let va_spec: Vec<Complex64> =
                    sa.iter().zip(&q1s).map(|(s, q)| s * q * half).collect();
                let va = sp.inverse_real(va_spec);
                for k in 0..n {
                    var[k] += (ga * ga * va[k]).max(0.0);
                }
            }
        }
        var.iter().map(|v| v.sqrt()).collect()
    }

    /// Explicit noiseless waveform, noisy waveform and σ(t).
    pub fn materialize(&self, sp: &Spectral, fs: f64) -> MaterializedElectrical {
        let noiseless = self.noiseless(sp);
        let noise = self.noise_realization(sp, fs);
        let signal = noiseless.iter().zip(&noise).map(|(a, b)| a + b).collect();
        MaterializedElectrical {
            noiseless,
            signal,
            sigma: self.noise_sigma(sp, fs),
            gaussian_exact: self.gaussian_exact,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Optical: polarization and phase states
// ---------------------------------------------------------------------------------------------

/// Stokes vector of a Jones vector (E_x, E_y): S₂ = 2Re(E_x E_y*), S₃ = −2Im(E_x E_y*).
pub fn jones_to_stokes(e: &[Complex64; 2]) -> [f64; 4] {
    let (x, y) = (e[0], e[1]);
    let c = x * y.conj();
    [
        x.norm_sqr() + y.norm_sqr(),
        x.norm_sqr() - y.norm_sqr(),
        2.0 * c.re,
        -2.0 * c.im,
    ]
}

/// Decompose light of `power` with normalized Stokes vector `s` (s₀ = 1) into mutually
/// incoherent Jones vectors: the polarized part (one vector) plus, for the unpolarized part,
/// two orthogonal vectors of equal power.
pub fn jones_decomposition(power: f64, s: &[f64; 4]) -> Vec<[Complex64; 2]> {
    let s0 = if s[0] > 0.0 { s[0] } else { 1.0 };
    let n = [s[1] / s0, s[2] / s0, s[3] / s0];
    let dop = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().min(1.0);
    let p_pol = dop * power;
    let p_un = power - p_pol;
    let mut out = Vec::with_capacity(3);
    if p_pol > 0.0 {
        let u = [n[0] / dop, n[1] / dop, n[2] / dop];
        let a = p_pol.sqrt();
        let ex = ((1.0 + u[0]) / 2.0).max(0.0).sqrt() * a;
        let ey = ((1.0 - u[0]) / 2.0).max(0.0).sqrt() * a;
        let delta = u[2].atan2(u[1]);
        out.push([Complex64::new(ex, 0.0), Complex64::from_polar(ey, delta)]);
    }
    if p_un > 0.0 {
        let a = (p_un / 2.0).sqrt();
        out.push([Complex64::new(a, 0.0), Complex64::new(0.0, 0.0)]);
        out.push([Complex64::new(0.0, 0.0), Complex64::new(a, 0.0)]);
    }
    out
}

/// State of one mode bin at the signal's representation level.
#[derive(Clone, Debug, PartialEq)]
pub enum BinState {
    Power(f64),
    Stokes([f64; 4]),
    /// Mutually incoherent Jones vectors (complex envelopes relative to the carrier) \[√W\].
    Field(Vec<[Complex64; 2]>),
}

impl BinState {
    /// Mode bin with total `power` and emitted normalized Stokes vector `stokes`.
    pub fn new(level: Level, power: f64, stokes: &[f64; 4]) -> Self {
        match level {
            Level::Power => BinState::Power(power),
            Level::Stokes => BinState::Stokes([
                power * stokes[0],
                power * stokes[1],
                power * stokes[2],
                power * stokes[3],
            ]),
            Level::Field => BinState::Field(jones_decomposition(power, stokes)),
        }
    }

    /// Total power S₀ \[W\] (relative units in lazy weights).
    pub fn power(&self) -> f64 {
        match self {
            BinState::Power(p) => *p,
            BinState::Stokes(s) => s[0],
            BinState::Field(c) => c.iter().map(|e| e[0].norm_sqr() + e[1].norm_sqr()).sum(),
        }
    }

    pub fn stokes(&self) -> [f64; 4] {
        match self {
            BinState::Power(p) => [*p, 0.0, 0.0, 0.0],
            BinState::Stokes(s) => *s,
            BinState::Field(c) => c.iter().map(jones_to_stokes).fold([0.0; 4], |a, b| {
                [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
            }),
        }
    }

    /// Polarization-independent power transmission `t`.
    pub fn scale(&mut self, t: f64) {
        match self {
            BinState::Power(p) => *p *= t,
            BinState::Stokes(s) => s.iter_mut().for_each(|x| *x *= t),
            BinState::Field(c) => {
                let a = t.sqrt();
                for e in c.iter_mut() {
                    e[0] *= a;
                    e[1] *= a;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Optical: sources, mode bins, contributions
// ---------------------------------------------------------------------------------------------

/// Spectral shape of a source, for the coherence descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpectrumShape {
    Gaussian,
}

/// Source waveforms and properties shared by every wire the source feeds.
#[derive(Clone, Debug)]
pub struct SourceInfo {
    /// Node id of the source.
    pub node: String,
    pub tx: Arc<TxOutput>,
    /// Spectrum of P_tx(t).
    pub power_spec: Spectrum,
    /// Spectrum of the RIN realization (zeros if disabled).
    pub rin_spec: Spectrum,
    /// Spectrum of P_tx(t)² (for the RIN variance).
    pub power_sq_spec: Spectrum,
    /// Linear RIN \[1/Hz\]; 0 if RIN is disabled.
    pub rin_lin: f64,
    /// Center wavelength \[m\].
    pub wavelength: f64,
    /// RMS spectral width \[m\].
    pub spectral_width_rms: f64,
    pub spectrum_shape: SpectrumShape,
    /// VCSEL near field (launch beam).
    pub beam_radius: f64,
    pub vcsel_mode_groups: u32,
    /// Emitted normalized Stokes vector (S₀ = 1).
    pub stokes: [f64; 4],
    /// Noise of the drive (e.g. transmitter jitter) as optical power noise at the source.
    pub tx_noise: Vec<SourceNoise>,
}

/// A drive noise term converted to optical power at the source: the PIN emits it like RIN,
/// with `filter` (drive filters × VCSEL response) times the channel.
#[derive(Clone, Debug)]
pub struct SourceNoise {
    pub kind: NoiseKind,
    pub strength: Strength,
    pub filter: Spectrum,
    /// Power per drive unit \[W\].
    pub gain: f64,
    /// Realization at the point of generation (unfiltered).
    pub realization: Spectrum,
}

impl SourceInfo {
    /// Spectrum of the emitted power including transmitter noise (RIN, drive noise): what a
    /// noise-free reference receiver sees before the channel.
    pub fn emitted_spec(&self) -> Vec<Complex64> {
        let mut s: Vec<Complex64> = self
            .power_spec
            .iter()
            .zip(self.rin_spec.iter())
            .map(|(p, r)| p + r)
            .collect();
        for t in &self.tx_noise {
            for ((o, r), h) in s.iter_mut().zip(t.realization.iter()).zip(t.filter.iter()) {
                *o += r * h * t.gain;
            }
        }
        s
    }
}

impl SourceInfo {
    /// RMS optical-frequency width σ_ν \[Hz\].
    pub fn sigma_nu(&self) -> f64 {
        C0 * self.spectral_width_rms / (self.wavelength * self.wavelength)
    }

    /// Degree of coherence γ(Δτ) (signal-spec 9.4). Gaussian spectrum:
    /// γ = exp(−2π²σ_ν²Δτ²).
    pub fn gamma(&self, dt: f64) -> f64 {
        match self.spectrum_shape {
            SpectrumShape::Gaussian => {
                let s = self.sigma_nu();
                (-2.0 * PI * PI * s * s * dt * dt).exp()
            }
        }
    }

    /// Coherence time (γ = 1/e) \[s\].
    pub fn coherence_time(&self) -> f64 {
        1.0 / (2f64.sqrt() * PI * self.sigma_nu().max(1e-300))
    }
}

/// A mode of the basis that belongs to a mode bin, with its share of the mode bin's power.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub l: u32,
    pub m: u32,
    pub frac: f64,
}

/// One slot in the mode dimension (a mode or a mode group). Not an FFT bin: see
/// signal-spec 3.3 for the naming convention.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeBin {
    /// Principal mode group of the mode bin (for a mode: its group).
    pub group: u32,
    pub members: Vec<Member>,
    pub state: BinState,
    /// Accumulated relative group delay \[s\].
    pub delay: f64,
    /// Accumulated chromatic delay slope \[s/m\].
    pub slope: f64,
}

/// Where a contribution currently is.
#[derive(Clone, Debug)]
pub enum Propagation {
    /// Before the first fiber: launch offset/tilt (last connector wins) and lumped power factor.
    FreeSpace { offset: f64, tilt: f64, factor: f64 },
    /// Guided in a fiber; `basis` is the current span's mode set.
    Guided {
        basis: Arc<ModeSet>,
        solver: SolverParams,
        mode_bins: Vec<ModeBin>,
        /// Σ over spans of τ_ref·L \[s\].
        reference_delay: f64,
    },
}

/// One source's share of an optical signal (lazy form).
#[derive(Clone, Debug)]
pub struct Contribution {
    pub source: Arc<SourceInfo>,
    pub prop: Propagation,
    /// Extra linear filter common to all mode bins (custom linear components).
    pub h_extra: Option<Spectrum>,
}

/// Explicit per-mode-bin power waveforms of a contribution (signal-spec 3.3).
#[derive(Clone, Debug)]
pub struct MaterializedOptical {
    pub mode_bins: Vec<Vec<f64>>,
}

impl Contribution {
    /// Polarization-independent power transmission `t`.
    pub fn scale(&mut self, t: f64) {
        match &mut self.prop {
            Propagation::FreeSpace { factor, .. } => *factor *= t,
            Propagation::Guided { mode_bins, .. } => {
                mode_bins.iter_mut().for_each(|b| b.state.scale(t))
            }
        }
    }

    /// Total power transmission from the source to this point (all light collected),
    /// including the DC gain of `h_extra`.
    pub fn transmission(&self) -> f64 {
        let t: f64 = match &self.prop {
            Propagation::FreeSpace { factor, .. } => *factor,
            Propagation::Guided { mode_bins, .. } => {
                mode_bins.iter().map(|b| b.state.power()).sum()
            }
        };
        t * self.h_extra.as_ref().map_or(1.0, |h| h[0].re)
    }

    /// Append a linear filter on optical power, common to all mode bins (engine-applied H(f) of
    /// a linear component, signal-spec 6.1).
    pub fn apply_filter(&mut self, h: &Spectrum) {
        self.h_extra = Some(match &self.h_extra {
            None => h.clone(),
            Some(x) => Arc::new(mul(x, h)),
        });
    }

    /// Detector fraction of every mode bin for a centered detector of radius `rd`.
    fn detector_fractions(&self, rd: Option<f64>) -> Vec<f64> {
        let Propagation::Guided {
            basis, mode_bins, ..
        } = &self.prop
        else {
            return vec![1.0];
        };
        let Some(rd) = rd else {
            return vec![1.0; mode_bins.len()];
        };
        let index: HashMap<(u32, u32), usize> = basis
            .modes
            .iter()
            .enumerate()
            .map(|(k, m)| ((m.l, m.m), k))
            .collect();
        mode_bins
            .iter()
            .map(|b| {
                b.members
                    .iter()
                    .map(|mm| {
                        index
                            .get(&(mm.l, mm.m))
                            .map_or(0.0, |&j| mm.frac * power_within(basis, &basis.modes[j], rd))
                    })
                    .sum()
            })
            .collect()
    }

    /// Power-domain channel from the source to a detector of radius `rd` (`None` = all light),
    /// in the incoherent limit (signal-spec 9.4).
    pub fn channel(&self, rd: Option<f64>) -> FiberChannel {
        match &self.prop {
            Propagation::FreeSpace { factor, .. } => FiberChannel {
                terms: vec![ChannelTerm {
                    l: 0,
                    m: 0,
                    weight: *factor,
                    delay: 0.0,
                    sigma: 0.0,
                }],
                mean_delay: 0.0,
                rms_delay_spread: 0.0,
                transmission: *factor,
            },
            Propagation::Guided {
                mode_bins,
                reference_delay,
                ..
            } => {
                let eta = self.detector_fractions(rd);
                let w: Vec<f64> = mode_bins
                    .iter()
                    .zip(&eta)
                    .map(|(b, e)| {
                        if rd.is_some() {
                            b.state.power() * e
                        } else {
                            b.state.power()
                        }
                    })
                    .collect();
                let total: f64 = w.iter().sum();
                let mean = if total > 0.0 {
                    w.iter()
                        .zip(mode_bins)
                        .map(|(w, b)| w * b.delay)
                        .sum::<f64>()
                        / total
                } else {
                    0.0
                };
                let var = if total > 0.0 {
                    w.iter()
                        .zip(mode_bins)
                        .map(|(w, b)| w * (b.delay - mean).powi(2))
                        .sum::<f64>()
                        / total
                } else {
                    0.0
                };
                let sw = self.source.spectral_width_rms;
                let terms = mode_bins
                    .iter()
                    .zip(&w)
                    .filter(|(_, &w)| w > 0.0)
                    .map(|(b, &w)| ChannelTerm {
                        l: b.members.first().map_or(0, |m| m.l),
                        m: b.members.first().map_or(0, |m| m.m),
                        weight: w,
                        delay: b.delay - mean,
                        sigma: b.slope.abs() * sw,
                    })
                    .collect();
                FiberChannel {
                    terms,
                    mean_delay: reference_delay + mean,
                    rms_delay_spread: var.sqrt(),
                    transmission: total,
                }
            }
        }
    }

    /// Channel transfer function on the grid (including `h_extra`).
    pub fn transfer(&self, rd: Option<f64>, grid: &TimeGrid) -> Vec<Complex64> {
        let h = channel_spectrum(&self.channel(rd), grid.n(), grid.fs());
        match &self.h_extra {
            Some(x) => mul(&h, x),
            None => h,
        }
    }

    /// Explicit power waveform of every mode bin (all light collected).
    pub fn materialize(&self, grid: &TimeGrid, sp: &Spectral) -> MaterializedOptical {
        let ch = self.channel(None);
        let mode_bins = ch
            .terms
            .iter()
            .map(|t| {
                let one = FiberChannel {
                    terms: vec![t.clone()],
                    ..ch.clone()
                };
                let mut h = channel_spectrum(&one, grid.n(), grid.fs());
                if let Some(x) = &self.h_extra {
                    h = mul(&h, x);
                }
                sp.inverse_real(mul(&self.source.power_spec, &h))
            })
            .collect();
        MaterializedOptical { mode_bins }
    }
}

/// Optical signal on one wire in one direction.
#[derive(Clone, Debug)]
pub struct OpticalSignal {
    pub level: Level,
    pub resolution: Resolution,
    pub contributions: Vec<Contribution>,
}

impl OpticalSignal {
    pub fn scaled(&self, t: f64) -> Self {
        let mut s = self.clone();
        s.contributions.iter_mut().for_each(|c| c.scale(t));
        s
    }

    /// Average optical power \[W\] (all light collected).
    pub fn average_power(&self) -> f64 {
        self.contributions
            .iter()
            .map(|c| {
                c.source.power_spec[0].re / c.source.power_spec.len() as f64 * c.transmission()
            })
            .sum()
    }
}

/// Optical port value: forward wave and reserved backward wave (signal-spec 11.1).
#[derive(Clone, Debug)]
pub struct OpticalWave {
    pub forward: Arc<OpticalSignal>,
    /// Always `None` with current components.
    pub backward: Option<Arc<OpticalSignal>>,
}

impl OpticalWave {
    pub fn forward(s: OpticalSignal) -> Self {
        Self {
            forward: Arc::new(s),
            backward: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64; 4], b: &[f64; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12)
    }

    /// F4: Field → Stokes matches the analytic Stokes vector.
    #[test]
    fn f4_field_to_stokes() {
        let cases: [[f64; 4]; 5] = [
            [1.0, 1.0, 0.0, 0.0], // horizontal
            [1.0, 0.0, 1.0, 0.0], // +45°
            [1.0, 0.0, 0.0, 1.0], // right circular
            [1.0, 0.3, 0.4, 0.0], // partially polarized linear
            [1.0, 0.5, 0.5, 0.5], // elliptical, partially polarized
        ];
        for s in cases {
            let st = BinState::new(Level::Field, 2.0, &s).stokes();
            let exact = [2.0 * s[0], 2.0 * s[1], 2.0 * s[2], 2.0 * s[3]];
            assert!(close(&st, &exact), "{s:?}: {st:?}");
        }
        // Fully elliptical (DOP = 1).
        let e = [1.0, 0.6, 0.0, 0.8];
        assert!(close(&BinState::new(Level::Field, 1.0, &e).stokes(), &e));
    }

    #[test]
    fn unpolarized_states_have_equal_power() {
        let s = [1.0, 0.0, 0.0, 0.0];
        for level in [Level::Power, Level::Stokes, Level::Field] {
            let mut b = BinState::new(level, 0.7, &s);
            b.scale(0.5);
            assert!((b.power() - 0.35).abs() < 1e-15, "{level:?}");
        }
    }

    #[test]
    fn coherence_numbers() {
        let src = SourceInfo {
            node: "v".into(),
            tx: Arc::new(TxOutput {
                symbols: vec![],
                levels: vec![0.0; 4],
                sample_rate: 1.0,
                power: vec![],
                rin: vec![],
                vcsel_bandwidth: 0.0,
            }),
            power_spec: Arc::new(vec![]),
            rin_spec: Arc::new(vec![]),
            power_sq_spec: Arc::new(vec![]),
            rin_lin: 0.0,
            wavelength: 850e-9,
            spectral_width_rms: 0.4e-9,
            spectrum_shape: SpectrumShape::Gaussian,
            beam_radius: 4e-6,
            vcsel_mode_groups: 4,
            stokes: [1.0, 0.0, 0.0, 0.0],
            tx_noise: vec![],
        };
        // Tutorial values: γ(1 ps) ≈ 0.58, γ(4 ps) ≈ 2e-4, τ_c ≈ 1.36 ps.
        assert!((src.gamma(1e-12) - 0.58).abs() < 0.01);
        assert!(src.gamma(4e-12) < 3e-4);
        assert!((src.coherence_time() - 1.356e-12).abs() < 0.01e-12);
    }
}

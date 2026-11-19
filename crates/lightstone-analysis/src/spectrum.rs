//! Spectrum analyzers (`docs/model/optical-spectrum-analyzer.md`, `docs/model/electrical-spectrum-analyzer.md`).
//!
//! `eq:osa-model-spectrum` — Optical spectrum analyzer: the *model spectrum* of an optical
//! wire. Every source contribution is a Gaussian line with its centre wavelength λ_c, RMS width
//! σ_λ and its average power P at this point; the analyzer shows the power within a resolution
//! bandwidth (RBW) around each wavelength,
//! P_RBW(λ) = Σ_c P_c·[Φ((λ + RBW/2 − λ_c)/σ_c) − Φ((λ − RBW/2 − λ_c)/σ_c)].
//! No VCSEL mode structure and no modulation sidebands (±R_s/2 ≈ ±0.07 nm at 53 GBd, small
//! against σ_λ = 0.4 nm).
//!
//! `eq:esa-psd` — Electrical spectrum analyzer: one-sided power spectral densities of an
//! electrical wire. Signal: the periodogram of the noiseless (periodic) waveform,
//! S_x(f_k) = 2|X_k|²/(N²·Δf) (k > 0). Noise: from the noise terms, time-averaged,
//! S_n(f) = Σ g²·S̄·|H(f)|² (an explicit realization enters through its periodogram). Both are
//! averaged over display bins of width RBW = f_stop/points. ∫S_n df = mean σ²(t) and
//! ∫S_x df = variance of the noiseless waveform (Parseval).

use crate::stats::q_func;
use lightstone_core::component::*;
use lightstone_core::dsp::freq;
use lightstone_core::signal::{Quantity, Realization, Strength};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn dbm(p: f64) -> f64 {
    10.0 * (p.max(1e-30) / 1e-3).log10()
}

fn data(kind: &str, v: serde_json::Value) -> Output {
    Output {
        report: Some(Report::Data {
            kind: kind.into(),
            value: Arc::new(v),
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Optical spectrum analyzer
// ---------------------------------------------------------------------------------------------

/// Parameters of `optical_spectrum_analyzer`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OsaParams {
    /// Start and stop wavelength \[m\].
    pub start: f64,
    pub stop: f64,
    /// Number of trace points.
    pub points: usize,
    /// Resolution bandwidth \[m\].
    pub rbw: f64,
}

impl Default for OsaParams {
    fn default() -> Self {
        Self {
            start: 820e-9,
            stop: 970e-9,
            points: 751,
            rbw: 0.1e-9,
        }
    }
}

pub(crate) struct Osa(pub OsaParams);

impl Osa {
    pub(crate) fn new(p: OsaParams) -> Result<Self, String> {
        if !(p.start > 0.0 && p.stop > p.start) {
            return Err("The span needs 0 < start < stop.".into());
        }
        if !(2..=20_000).contains(&p.points) {
            return Err("points must be 2 … 20000.".into());
        }
        if p.rbw.is_nan() || p.rbw <= 0.0 {
            return Err("The resolution bandwidth must be > 0.".into());
        }
        Ok(Self(p))
    }
}

/// Power of a Gaussian line (centre `c`, RMS `s`, power `p`) within `[a, b]`.
fn line_power(p: f64, c: f64, s: f64, a: f64, b: f64) -> f64 {
    if s <= 0.0 {
        return if (a..b).contains(&c) { p } else { 0.0 };
    }
    p * (q_func((a - c) / s) - q_func((b - c) / s))
}

impl Component for Osa {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::monitor("in", PortType::Optical)]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let s = &inputs[0].as_optical()?.forward;
        let p = &self.0;
        let lines: Vec<(String, f64, f64, f64)> = s
            .contributions
            .iter()
            .map(|c| {
                let pw =
                    c.source.power_spec[0].re / c.source.power_spec.len() as f64 * c.transmission();
                (
                    c.source.node.clone(),
                    c.source.wavelength,
                    c.source.spectral_width_rms,
                    pw,
                )
            })
            .collect();
        let step = (p.stop - p.start) / (p.points - 1) as f64;
        let wl: Vec<f64> = (0..p.points).map(|i| p.start + i as f64 * step).collect();
        let trace: Vec<f64> = wl
            .iter()
            .map(|&l| {
                let (a, b) = (l - 0.5 * p.rbw, l + 0.5 * p.rbw);
                dbm(lines.iter().map(|x| line_power(x.3, x.1, x.2, a, b)).sum())
            })
            .collect();
        let (pk_i, pk) =
            trace.iter().enumerate().fold(
                (0, f64::NEG_INFINITY),
                |m, (i, &v)| if v > m.1 { (i, v) } else { m },
            );
        let total: f64 = lines.iter().map(|x| x.3).sum();
        let sources: Vec<_> = lines
            .iter()
            .map(|(n, l, w, pw)| {
                serde_json::json!({ "source": n, "wavelength": l, "spectral_width_rms": w,
                                    "power": pw, "power_dbm": dbm(*pw) })
            })
            .collect();
        Ok(data(
            "optical_spectrum",
            serde_json::json!({
                "wavelength": wl, "power_dbm": trace, "rbw": p.rbw,
                "total_power": total, "total_power_dbm": dbm(total),
                "peak": { "wavelength": wl[pk_i], "power_dbm": pk },
                "sources": sources,
                "model": "Gaussian line per source (centre wavelength, RMS width, average power); \
                          no mode structure or modulation sidebands",
            }),
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Electrical spectrum analyzer
// ---------------------------------------------------------------------------------------------

/// Parameters of `electrical_spectrum_analyzer`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EsaParams {
    /// Stop frequency relative to the symbol rate (start is 0).
    pub stop_rel: f64,
    /// Number of display bins; RBW = stop / points.
    pub points: usize,
}

impl Default for EsaParams {
    fn default() -> Self {
        Self {
            stop_rel: 1.5,
            points: 400,
        }
    }
}

pub(crate) struct Esa(pub EsaParams);

impl Esa {
    pub(crate) fn new(p: EsaParams) -> Result<Self, String> {
        if p.stop_rel.is_nan() || p.stop_rel <= 0.0 {
            return Err("stop_rel must be > 0.".into());
        }
        if !(2..=20_000).contains(&p.points) {
            return Err("points must be 2 … 20000.".into());
        }
        Ok(Self(p))
    }
}

/// One-sided PSDs (signal, noise) per FFT bin k = 0 … n/2 \[unit²/Hz\] and the DC value.
pub fn electrical_psd(
    e: &lightstone_core::signal::ElectricalSignal,
    fs: f64,
) -> (Vec<f64>, Vec<f64>, f64) {
    let base = e.spectrum_unscaled();
    let n = base.len();
    let df = fs / n as f64;
    let nf = n as f64;
    let half = n / 2;
    let one_sided = |k: usize| if k == 0 || 2 * k == n { 1.0 } else { 2.0 };
    let sig: Vec<f64> = (0..=half)
        .map(|k| {
            if k == 0 {
                0.0
            } else {
                one_sided(k) * (e.gain * base[k]).norm_sqr() / (nf * nf * df)
            }
        })
        .collect();
    let mut noise = vec![0.0; half + 1];
    for t in &e.noise {
        let h2 = |k: usize| t.filter.as_ref().map_or(1.0, |h| h[k].norm_sqr());
        let s_mean = match &t.strength {
            Strength::Constant(s) => *s,
            Strength::Varying { spectrum, .. } => spectrum[0].re / nf,
        };
        if s_mean > 0.0 {
            for (k, v) in noise.iter_mut().enumerate() {
                // One-sided PSD; the Nyquist bin is counted once.
                let w = if 2 * k == n || k == 0 { 0.5 } else { 1.0 };
                *v += w * t.gain * t.gain * s_mean * h2(k);
            }
        } else if let Realization::Given(g) = &t.realization {
            for (k, v) in noise.iter_mut().enumerate() {
                *v += one_sided(k) * t.gain * t.gain * g[k].norm_sqr() * h2(k) / (nf * nf * df);
            }
        }
    }
    (sig, noise, e.gain * base[0].re / nf)
}

impl Component for Esa {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::monitor("in", PortType::Electrical(None))]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let e = inputs[0].as_electrical()?;
        let (n, fs) = (ctx.grid.n(), ctx.grid.fs());
        let (sig, noise, dc) = electrical_psd(e, fs);
        let df = fs / n as f64;
        let stop = (self.0.stop_rel * ctx.grid.baud).min(0.5 * fs);
        let kmax = ((stop / df).floor() as usize).min(n / 2);
        let points = self.0.points.min(kmax.max(1));
        let rbw = kmax as f64 * df / points as f64;
        let (mut f, mut s_db, mut n_db) = (vec![], vec![], vec![]);
        let db = |x: f64| 10.0 * x.max(1e-300).log10();
        for i in 0..points {
            let (a, b) = (1 + i * kmax / points, 1 + (i + 1) * kmax / points);
            let cnt = (b - a).max(1) as f64;
            let ms = sig[a..b.min(sig.len())].iter().sum::<f64>() / cnt;
            let mn = noise[a..b.min(noise.len())].iter().sum::<f64>() / cnt;
            f.push(0.5 * (freq(a, n, fs) + freq(b - 1, n, fs)));
            s_db.push(db(ms));
            n_db.push(db(mn));
        }
        let signal_var: f64 = sig.iter().sum::<f64>() * df;
        let noise_var: f64 = noise.iter().sum::<f64>() * df;
        let unit = match e.quantity {
            Quantity::Voltage => "V",
            Quantity::Current => "A",
            Quantity::Dimensionless => "1",
        };
        Ok(data(
            "electrical_spectrum",
            serde_json::json!({
                "frequency": f, "signal_db": s_db, "noise_db": n_db, "rbw": rbw,
                "unit": unit, "dc": dc,
                "signal_rms": signal_var.sqrt(), "noise_rms": noise_var.sqrt(),
                "snr_db": 10.0 * (signal_var / noise_var.max(1e-300)).log10(),
                "density_unit": format!("{unit}²/Hz"),
            }),
        ))
    }
}

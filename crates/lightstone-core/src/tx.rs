//! PAM-4 / NRZ intensity-modulated VCSEL transmitter.
//!
//! `eq:tx-levels` — M equally spaced power levels (M = 4 for PAM-4, 2 for NRZ) from average
//! power and extinction ratio: P₀ = 2P̄/(ER+1), P_{M−1} = 2P̄·ER/(ER+1),
//! P_k = P₀ + k(P_{M−1} − P₀)/(M−1), OMA_outer = P_{M−1} − P₀.
//!
//! `eq:vcsel-response` — Small-signal VCSEL intensity response (2nd order):
//! H(f) = 1 / (1 − (f/f_r)² + j·2ζ·f/f_r).
//!
//! `eq:vcsel-rin` — RIN as Gaussian power noise with one-sided PSD RIN·P(t)².

use crate::config::{TxConfig, VcselConfig};
use crate::dsp::{self, Spectral};
use crate::prbs::pattern;
use crate::rng::Rng;
use crate::units::{db_to_lin, dbm_to_w};
use num_complex::Complex64;

/// The four optical power levels \[W\].
pub fn power_levels(p_avg: f64, er_lin: f64) -> [f64; 4] {
    let p0 = 2.0 * p_avg / (er_lin + 1.0);
    let p3 = 2.0 * p_avg * er_lin / (er_lin + 1.0);
    let step = (p3 - p0) / 3.0;
    [p0, p0 + step, p0 + 2.0 * step, p3]
}

/// The `m` equally spaced optical power levels \[W\] (`m` = 2 or 4) with the same outer levels
/// for every `m`: P₀ = 2P̄/(ER+1), P_{m−1} = 2P̄·ER/(ER+1). For `m` = 4 this is exactly
/// [`power_levels`].
pub fn power_levels_m(p_avg: f64, er_lin: f64, m: u8) -> Vec<f64> {
    if m == 4 {
        return power_levels(p_avg, er_lin).to_vec();
    }
    let p0 = 2.0 * p_avg / (er_lin + 1.0);
    let p_top = 2.0 * p_avg * er_lin / (er_lin + 1.0);
    let m = m.max(2) as usize;
    (0..m)
        .map(|k| {
            if k + 1 == m {
                p_top
            } else {
                p0 + k as f64 * (p_top - p0) / (m - 1) as f64
            }
        })
        .collect()
}

/// Normalized drive levels k/(m−1) for an `m`-level alphabet (exactly 0, ⅓, ⅔, 1 for PAM-4).
pub fn drive_levels(m: u8) -> Vec<f64> {
    if m == 4 {
        return vec![0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    }
    let m = m.max(2) as usize;
    (0..m).map(|k| k as f64 / (m - 1) as f64).collect()
}

/// Symbol sequence: `pattern_periods` repetitions of the pattern (PRBS13Q for PAM-4, PRBS13
/// for NRZ). Panics if `cfg.levels` is not 2 or 4.
pub fn symbols(cfg: &TxConfig) -> Vec<u8> {
    let one = pattern(cfg.levels, cfg.prbs_seed).expect("TxConfig.levels");
    let mut s = Vec::with_capacity(one.len() * cfg.pattern_periods.max(1));
    for _ in 0..cfg.pattern_periods.max(1) {
        s.extend_from_slice(&one);
    }
    s
}

/// Rectangular (NRZ-hold) waveform of the levels, `sps` samples per symbol.
pub fn nrz_waveform(symbols: &[u8], levels: &[f64], sps: usize) -> Vec<f64> {
    let mut w = Vec::with_capacity(symbols.len() * sps);
    for &s in symbols {
        w.extend(std::iter::repeat_n(levels[s as usize], sps));
    }
    w
}

/// 2nd-order VCSEL intensity response.
#[derive(Clone, Copy, Debug)]
pub struct VcselResponse {
    pub resonance_freq: f64,
    pub damping: f64,
}

impl VcselResponse {
    pub fn from_config(v: &VcselConfig) -> Self {
        Self {
            resonance_freq: v.resonance_freq,
            damping: v.damping,
        }
    }

    pub fn response(&self, f: f64) -> Complex64 {
        let x = f / self.resonance_freq;
        Complex64::new(1.0, 0.0) / Complex64::new(1.0 - x * x, 2.0 * self.damping * x)
    }

    /// Electrical-convention −3 dB bandwidth (|H|² = ½) \[Hz\].
    pub fn bandwidth_3db(&self) -> f64 {
        dsp::bandwidth(
            |f| self.response(f),
            1.0 / 2f64.sqrt(),
            20.0 * self.resonance_freq,
        )
        .unwrap_or(f64::INFINITY)
    }
}

/// Transmitter output.
#[derive(Clone, Debug)]
pub struct TxOutput {
    pub symbols: Vec<u8>,
    /// Optical power levels P₀ … P_{M−1} \[W\] (M = 2 or 4).
    pub levels: Vec<f64>,
    pub sample_rate: f64,
    /// Noiseless optical power after the VCSEL response \[W\].
    pub power: Vec<f64>,
    /// RIN noise component of the optical power \[W\] (zero if disabled).
    pub rin: Vec<f64>,
    pub vcsel_bandwidth: f64,
}

/// Spectrum of the band-limited NRZ drive on the `n = symbols·sps` grid. `eq:tx-nrz`
///
/// The drive is the continuous-time NRZ waveform Σₙ P_{kₙ} rect((t − nT)/T − ½). Its
/// spectrum is evaluated exactly at the FFT bins, X(f) = S(f)·T·sinc(fT)·e^{−jπfT}, where S
/// is the (periodic) symbol spectrum. This is the alias-free sampled version of the
/// continuous waveform (components above fs/2 are dropped), so results are independent of
/// the oversampling factor up to the negligible out-of-band energy after the VCSEL filter.
pub fn nrz_spectrum(symbols: &[u8], levels: &[f64], sps: usize) -> Vec<Complex64> {
    let ns = symbols.len();
    let n = ns * sps;
    let mut s: Vec<Complex64> = symbols
        .iter()
        .map(|&k| Complex64::new(levels[k as usize], 0.0))
        .collect();
    rustfft::FftPlanner::new()
        .plan_fft_forward(ns)
        .process(&mut s);
    let mut x = Vec::with_capacity(n);
    for k in 0..n {
        // Frequency in units of the baud rate: f·T = kk/ns (signed FFT bin index kk).
        let ft = dsp::freq(k, n, n as f64) / ns as f64;
        let sinc = if ft == 0.0 {
            1.0
        } else {
            (std::f64::consts::PI * ft).sin() / (std::f64::consts::PI * ft)
        };
        let ph = Complex64::from_polar(1.0, -std::f64::consts::PI * ft);
        x.push(s[k % ns] * (sps as f64 * sinc) * ph);
    }
    x
}

/// One level step of the drive: index of the symbol it starts, step Δ = L_n − L_{n−1}, and
/// its position in samples (n·sps + τₙ·sps, not wrapped).
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub symbol: usize,
    pub step: f64,
    pub position: f64,
}

/// The level steps of the periodic NRZ drive, each shifted by `tau_ui[n]` UI (one entry per
/// symbol boundary n, i.e. the edge into symbol n). Boundaries without a level change are
/// skipped.
pub fn edges(symbols: &[u8], levels: &[f64], sps: usize, tau_ui: &[f64]) -> Vec<Edge> {
    let ns = symbols.len();
    let mut out = Vec::new();
    for i in 0..ns {
        let step = levels[symbols[i] as usize] - levels[symbols[(i + ns - 1) % ns] as usize];
        if step == 0.0 {
            continue;
        }
        out.push(Edge {
            symbol: i,
            step,
            position: (i as f64 + tau_ui[i]) * sps as f64,
        });
    }
    out
}

/// Spectrum of a periodic train of band-limited impulses Σₑ wₑ·δ(t − pₑ·Δt) on the `n`-sample
/// grid (positions `pₑ` in samples, any real value): Xₖ = Σₑ wₑ·e^{−jθₖpₑ}, θₖ = 2πk̃/n.
/// Integer parts by FFT, fractional parts by a Taylor series (|θₖrₑ| ≤ π/2, truncated
/// below 10⁻¹⁷). The Nyquist bin is set to zero.
pub fn impulse_train_spectrum(impulses: &[(f64, f64)], sp: &Spectral) -> Vec<Complex64> {
    let n = sp.len();
    let split: Vec<(usize, f64, f64)> = impulses
        .iter()
        .map(|&(p, w)| {
            let q = p.round();
            (q.rem_euclid(n as f64) as usize % n, p - q, w)
        })
        .collect();
    let r_max = split.iter().map(|e| e.1.abs()).fold(0.0, f64::max);
    let x = std::f64::consts::PI * r_max;
    let mut orders = 1;
    let mut bound = 1.0;
    while orders < 40 {
        bound *= x / orders as f64;
        if bound < 1e-17 {
            break;
        }
        orders += 1;
    }
    let train = |m: usize| -> Vec<f64> {
        let mut a = vec![0.0; n];
        for &(q, r, w) in &split {
            a[q] += w * r.powi(m as i32);
        }
        a
    };
    let mut series: Vec<Vec<Complex64>> = Vec::with_capacity(orders);
    let mut m = 0;
    while m < orders {
        if m + 1 < orders {
            let (a, b) = sp.forward_pair(&train(m), &train(m + 1));
            series.push(a);
            series.push(b);
            m += 2;
        } else {
            series.push(sp.forward(&train(m)));
            m += 1;
        }
    }
    let mut out = vec![Complex64::new(0.0, 0.0); n];
    for (k, o) in out.iter_mut().enumerate() {
        let kk = dsp::freq(k, n, n as f64);
        if 2.0 * kk.abs() >= n as f64 {
            continue;
        }
        let theta = 2.0 * std::f64::consts::PI * kk / n as f64;
        let mut acc = Complex64::new(0.0, 0.0);
        let mut c = Complex64::new(1.0, 0.0);
        for (mm, a) in series.iter().enumerate() {
            if mm > 0 {
                c *= Complex64::new(0.0, -theta) / mm as f64;
            }
            acc += c * a[k];
        }
        *o = acc;
    }
    out
}

/// Spectrum of the band-limited NRZ drive whose edges are shifted by `tau_ui[n]` UI.
/// `eq:tx-edge-shift`
///
/// The drive is written as steps, x(t) = L₀ + Σₑ Δₑ·u(t − tₑ). On the periodic grid (FFT
/// convention of [`nrz_spectrum`]) a step contributes Δₑ·e^{−jθₖpₑ}/(jθₖ), θₖ = 2πk̃/n (k̃ the
/// signed bin), pₑ its position in samples: the spectrum of an impulse train
/// ([`impulse_train_spectrum`]) divided by jθₖ. The result is alias-free like
/// [`nrz_spectrum`], which it equals for τ ≡ 0. The DC bin is the exact mean,
/// sps·(Σ Lₙ − Σₑ Δₑτₑ).
pub fn shifted_nrz_spectrum(
    symbols: &[u8],
    levels: &[f64],
    sps: usize,
    tau_ui: &[f64],
    sp: &Spectral,
) -> Vec<Complex64> {
    let ns = symbols.len();
    let n = ns * sps;
    assert_eq!(tau_ui.len(), ns);
    assert_eq!(sp.len(), n);
    let ed = edges(symbols, levels, sps, tau_ui);
    let imp: Vec<(f64, f64)> = ed.iter().map(|e| (e.position, e.step)).collect();
    let mut out = impulse_train_spectrum(&imp, sp);
    for (k, o) in out.iter_mut().enumerate().skip(1) {
        let kk = dsp::freq(k, n, n as f64);
        *o /= Complex64::new(0.0, 2.0 * std::f64::consts::PI * kk / n as f64);
    }
    let sum_l: f64 = symbols.iter().map(|&s| levels[s as usize]).sum();
    let sum_dt: f64 = ed.iter().map(|e| e.step * tau_ui[e.symbol]).sum();
    out[0] = Complex64::new(sps as f64 * (sum_l - sum_dt), 0.0);
    out
}

/// Generate the transmitter waveform.
pub fn transmit(
    tx: &TxConfig,
    vc: &VcselConfig,
    spectral: &Spectral,
    rin_enabled: bool,
    rng: &mut Rng,
) -> TxOutput {
    let syms = symbols(tx);
    let levels = power_levels_m(dbm_to_w(tx.p_avg_dbm), db_to_lin(tx.er_db), tx.levels);
    let fs = tx.baud * tx.sps as f64;
    let resp = VcselResponse::from_config(vc);
    let mut spec = nrz_spectrum(&syms, &levels, tx.sps);
    let n = spec.len();
    for (k, c) in spec.iter_mut().enumerate() {
        *c *= resp.response(dsp::freq(k, n, fs));
    }
    let mut power = spectral.inverse_real(spec);
    // Physical constraint: optical power cannot be negative.
    for p in power.iter_mut() {
        *p = p.max(0.0);
    }
    let rin = if rin_enabled {
        let rin_lin = db_to_lin(vc.rin_db_hz);
        let half_fs = 0.5 * fs;
        power
            .iter()
            .map(|&p| (rin_lin * half_fs).sqrt() * p * rng.normal())
            .collect()
    } else {
        vec![0.0; power.len()]
    };
    TxOutput {
        symbols: syms,
        levels,
        sample_rate: fs,
        power,
        rin,
        vcsel_bandwidth: resp.bandwidth_3db(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nrz_spectrum_dc_and_levels() {
        let syms: Vec<u8> = (0..64).map(|i| ((i * 5 + i / 7) % 4) as u8).collect();
        let lv = power_levels(1e-3, 3.0);
        let sps = 16;
        let sp = Spectral::new(syms.len() * sps);
        let w = sp.inverse_real(nrz_spectrum(&syms, &lv, sps));
        // DC preserved and mid-symbol values close to the levels (Gibbs ripple only).
        let mean = w.iter().sum::<f64>() / w.len() as f64;
        let exact = syms.iter().map(|&s| lv[s as usize]).sum::<f64>() / syms.len() as f64;
        assert!((mean - exact).abs() < 1e-15);
        for (n, &s) in syms.iter().enumerate() {
            let v = w[n * sps + sps / 2];
            assert!(
                (v - lv[s as usize]).abs() < 0.15 * (lv[3] - lv[0]),
                "symbol {n}"
            );
        }
    }

    fn test_symbols() -> Vec<u8> {
        (0..257).map(|i| ((i * 5 + i / 7) % 4) as u8).collect()
    }

    fn max_rel_diff(a: &[Complex64], b: &[Complex64]) -> f64 {
        let scale = b.iter().map(|c| c.norm()).fold(0.0, f64::max);
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).norm())
            .fold(0.0, f64::max)
            / scale
    }

    /// Unshifted edges reproduce the rectangular-pulse spectrum.
    #[test]
    fn shifted_spectrum_zero_shift_equals_nrz() {
        let syms = test_symbols();
        let lv = drive_levels(4);
        let sps = 16;
        let sp = Spectral::new(syms.len() * sps);
        let a = shifted_nrz_spectrum(&syms, &lv, sps, &vec![0.0; syms.len()], &sp);
        let b = nrz_spectrum(&syms, &lv, sps);
        assert!(max_rel_diff(&a, &b) < 1e-12, "{}", max_rel_diff(&a, &b));
    }

    /// Shifting every edge by the same τ (fractional sample) is a pure delay: e^{−j2πfτ}.
    #[test]
    fn shifted_spectrum_uniform_shift_is_delay() {
        let syms = test_symbols();
        let lv = drive_levels(4);
        let sps = 8;
        let n = syms.len() * sps;
        let sp = Spectral::new(n);
        for tau in [0.013, 0.37, -0.81, 2.29] {
            let a = shifted_nrz_spectrum(&syms, &lv, sps, &vec![tau; syms.len()], &sp);
            let b: Vec<Complex64> = nrz_spectrum(&syms, &lv, sps)
                .iter()
                .enumerate()
                .map(|(k, c)| {
                    let kk = dsp::freq(k, n, n as f64);
                    if 2.0 * kk.abs() >= n as f64 {
                        return Complex64::new(0.0, 0.0);
                    }
                    let th = -2.0 * std::f64::consts::PI * kk * tau * sps as f64 / n as f64;
                    c * Complex64::from_polar(1.0, th)
                })
                .collect();
            assert!(
                max_rel_diff(&a, &b) < 1e-12,
                "τ {tau}: {}",
                max_rel_diff(&a, &b)
            );
        }
    }

    /// Duty-cycle distortion: rising edges early, falling edges late by `d/2` lengthens every
    /// high pulse by d; the waveform mean is exact and the mid-level crossings move.
    #[test]
    fn shifted_spectrum_dcd_crossings() {
        let syms: Vec<u8> = (0..64)
            .map(|i| (i % 3 == 0) as u8 ^ (i % 5 == 0) as u8)
            .collect();
        let lv = drive_levels(2);
        let sps = 64;
        let d = 0.2;
        let tau: Vec<f64> = (0..syms.len())
            .map(|i| {
                let prev = syms[(i + syms.len() - 1) % syms.len()];
                match syms[i].cmp(&prev) {
                    std::cmp::Ordering::Greater => -d / 2.0,
                    std::cmp::Ordering::Less => d / 2.0,
                    _ => 0.0,
                }
            })
            .collect();
        let sp = Spectral::new(syms.len() * sps);
        let w = sp.inverse_real(shifted_nrz_spectrum(&syms, &lv, sps, &tau, &sp));
        let ones = syms.iter().filter(|&&s| s == 1).count() as f64;
        let rises = edges(&syms, &lv, sps, &tau)
            .iter()
            .filter(|e| e.step > 0.0)
            .count() as f64;
        let mean = w.iter().sum::<f64>() / w.len() as f64;
        assert!((mean - (ones + rises * d) / syms.len() as f64).abs() < 1e-14);
        // The band-limited step crosses ½ exactly at its (shifted) position.
        for e in edges(&syms, &lv, sps, &tau).iter().filter(|e| e.symbol > 0) {
            let p = e.symbol as f64 * sps as f64 + tau[e.symbol] * sps as f64;
            let j = p.floor() as usize;
            let (a, b) = (w[j] - 0.5, w[j + 1] - 0.5);
            let x = j as f64 + a / (a - b);
            assert!(
                (x - p).abs() < 0.05 * sps as f64,
                "edge {}: {x} vs {p}",
                e.symbol
            );
        }
    }

    #[test]
    fn levels_match_er_and_average() {
        let l = power_levels(1e-3, 4.0);
        assert!(((l[0] + l[1] + l[2] + l[3]) / 4.0 - 1e-3).abs() < 1e-15);
        assert!((l[3] / l[0] - 4.0).abs() < 1e-12);
        assert!(((l[2] - l[1]) - (l[1] - l[0])).abs() < 1e-15);
    }

    #[test]
    fn vcsel_bandwidth_reasonable() {
        let r = VcselResponse {
            resonance_freq: 18e9,
            damping: 0.55,
        };
        let b = r.bandwidth_3db();
        assert!(b > 18e9 && b < 30e9, "{b}");
        assert!((r.response(0.0).re - 1.0).abs() < 1e-15);
    }
}

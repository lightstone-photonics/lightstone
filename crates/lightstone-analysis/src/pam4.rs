//! PAM-4 and NRZ eye metrics (unequalized receiver). The alphabet size M (4, or 2 for NRZ)
//! is given ([`analyze_levels`]) or inferred from the symbols ([`analyze`]); for M = 2 there
//! is one threshold and the ¼ below becomes ½.
//!
//! `eq:pam4-ser-semi-analytic` — With noiseless samples y₀ₙ and per-sample noise σₙ:
//! SER = (1/N) Σₙ [Q((y₀ₙ − θ_{k−1})/σₙ) + Q((θ_k − y₀ₙ)/σₙ)], k = symbol of n
//! (terms for non-existent thresholds omitted). This includes ISI exactly and assumes
//! Gaussian noise; it is the primary SER estimate.
//!
//! `eq:pam4-ser-gaussian` — Level-Gaussian estimate from empirical μ_k, σ_k of the noisy
//! waveform (ISI lumped into σ): SER = ¼ Σ_k [Q((μ_k − θ_{k−1})/σ_k) + Q((θ_k − μ_k)/σ_k)].
//!
//! `eq:pam4-eye-height` — EH_k = q_p(level k+1) − q_{1−p}(level k) of the noisy samples at
//! the optimal phase (p = 10⁻³ by default). EW_k is the contiguous phase range around the
//! optimum where q_p(level k+1) > θ_k > q_{1−p}(level k). BER ≈ SER/2 (Gray-coded PAM-4) or
//! BER = SER (NRZ).

use crate::align::{align, Sampler};
use crate::stats::{mean, q_func, quantile, std_dev};
use num_complex::Complex64;
use rustfft::FftPlanner;
use serde::{Deserialize, Serialize};

/// Options for [`analyze`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisOptions {
    /// Phase steps per UI for the sampling-phase search.
    pub phases_per_ui: usize,
    /// Probability for the eye-height contour.
    pub eye_probability: f64,
    /// Fixed sample delay; if `None`, found by cross-correlation.
    pub delay: Option<usize>,
    /// RMS random jitter of the receiver's sampling clock \[UI\] (`eq:rx-clock-jitter`):
    /// adds (dy₀/dt·σ)² to σ²(t) of the semi-analytic SER. Not in the noisy waveform, so
    /// eye height/width and the counted SER do not include it.
    pub clock_rj_rms_ui: f64,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            phases_per_ui: 64,
            eye_probability: 1e-3,
            delay: None,
            clock_rj_rms_ui: 0.0,
        }
    }
}

/// σ_tot(t) = √(σ(t)² + (y₀′(t)·σ_clk)²) with the periodic derivative y₀′ \[per UI\] of the
/// noiseless waveform (spectral, alias-free) and the clock jitter σ_clk \[UI\].
pub fn with_clock_jitter(y0: &[f64], sigma: &[f64], sps: usize, rj_ui: f64) -> Vec<f64> {
    let n = y0.len();
    let mut buf: Vec<Complex64> = y0.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    let mut planner = FftPlanner::new();
    planner.plan_fft_forward(n).process(&mut buf);
    let ns = n as f64 / sps as f64;
    for (k, c) in buf.iter_mut().enumerate() {
        let kk = if 2 * k < n {
            k as f64
        } else {
            k as f64 - n as f64
        };
        if 2 * k == n {
            *c = Complex64::new(0.0, 0.0);
            continue;
        }
        // d/d(t/T): × j2π·(f·T), f·T = kk/ns.
        *c *= Complex64::new(0.0, 2.0 * std::f64::consts::PI * kk / ns / n as f64);
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    sigma
        .iter()
        .zip(&buf)
        .map(|(s, d)| (s * s + (d.re * rj_ui).powi(2)).sqrt())
        .collect()
}

/// PAM-M analysis result (M = 4, or 2 for NRZ; level arrays have M entries, threshold and
/// eye arrays M − 1). Amplitudes are in the units of the input waveform.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pam4Analysis {
    /// Integer alignment delay \[samples\].
    pub delay: usize,
    /// Optimal sampling phase \[UI\] relative to the aligned symbol start.
    pub phase: f64,
    /// Level means and standard deviations at the optimal phase (noisy waveform).
    pub level_mean: Vec<f64>,
    pub level_std: Vec<f64>,
    /// Decision thresholds (midpoints of adjacent level means).
    pub thresholds: Vec<f64>,
    /// Eye heights at the configured probability contour (negative = closed).
    pub eye_height: Vec<f64>,
    /// Eye widths \[UI\].
    pub eye_width: Vec<f64>,
    /// Semi-analytic SER (requires noiseless waveform and σ), else `None`.
    pub ser: Option<f64>,
    /// Level-Gaussian SER from the noisy waveform statistics.
    pub ser_gaussian: f64,
    /// Counted symbol errors on the noisy waveform / number of symbols.
    pub ser_counted: f64,
    pub symbol_errors: usize,
    /// BER (from `ser` if available, else `ser_gaussian`): SER/2 for Gray-coded PAM-4, SER
    /// for NRZ ([`ber_from_ser`]).
    pub ber: f64,
    /// Minimum level Q-factor (μ_{k+1} − μ_k)/(σ_k + σ_{k+1}).
    pub q_min: f64,
}

fn group(values: &[f64], symbols: &[u8], m: usize) -> Vec<Vec<f64>> {
    let mut g: Vec<Vec<f64>> = vec![Vec::new(); m];
    for (&v, &k) in values.iter().zip(symbols) {
        g[k as usize].push(v);
    }
    g
}

/// Arguments of Q above this are treated as 0 in the semi-analytic SER (Q(9) ≈ 1.1 × 10⁻¹⁹).
pub const Q_CUTOFF: f64 = 9.0;

/// Smallest nonzero semi-analytic SER: with the cutoff, a reported SER of 0 means
/// SER < 2·Q(9) ≈ 2.3 × 10⁻¹⁹ (at most two thresholds per symbol), unless σ = 0.
pub fn ser_floor() -> f64 {
    2.0 * q_func(Q_CUTOFF)
}

/// Q(x) with a cheap cutoff at [`Q_CUTOFF`].
#[inline]
fn q_fast(x: f64) -> f64 {
    if x > Q_CUTOFF {
        0.0
    } else {
        q_func(x)
    }
}

fn semi_analytic_ser(y: &[f64], s: &[f64], symbols: &[u8], th: &[f64]) -> f64 {
    let top = th.len();
    let mut acc = 0.0;
    for ((&y, &s), &k) in y.iter().zip(s).zip(symbols) {
        let k = k as usize;
        if s <= 0.0 {
            if k > 0 && y <= th[k - 1] {
                acc += 1.0;
            }
            if k < top && y >= th[k] {
                acc += 1.0;
            }
            continue;
        }
        if k > 0 {
            acc += q_fast((y - th[k - 1]) / s);
        }
        if k < top {
            acc += q_fast((th[k] - y) / s);
        }
    }
    acc / symbols.len() as f64
}

fn thresholds_from_means(m: &[f64]) -> Vec<f64> {
    m.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect()
}

fn level_means(groups: &[Vec<f64>]) -> Vec<f64> {
    groups.iter().map(|g| mean(g)).collect()
}

fn level_stds(groups: &[Vec<f64>]) -> Vec<f64> {
    groups.iter().map(|g| std_dev(g)).collect()
}

fn gaussian_ser(mu: &[f64], sd: &[f64], th: &[f64]) -> f64 {
    let m = mu.len();
    let mut acc = 0.0;
    for k in 0..m {
        let s = sd[k].max(1e-300);
        if k > 0 {
            acc += q_func((mu[k] - th[k - 1]) / s);
        }
        if k + 1 < m {
            acc += q_func((th[k] - mu[k]) / s);
        }
    }
    acc / m as f64
}

/// (lower-edge quantile q_p, upper-edge quantile q_{1−p}) per level.
fn level_edges(groups: &mut [Vec<f64>], p: f64) -> (Vec<f64>, Vec<f64>) {
    let m = groups.len();
    let mut lo = vec![0.0; m];
    let mut hi = vec![0.0; m];
    for k in 0..m {
        lo[k] = quantile(&mut groups[k], p);
        hi[k] = quantile(&mut groups[k], 1.0 - p);
    }
    (lo, hi)
}

/// Alphabet size of a symbol stream: 2 if it only contains 0 and 1 (NRZ), else 4 (PAM-4).
pub fn infer_levels(symbols: &[u8]) -> u8 {
    if symbols.iter().all(|&s| s <= 1) {
        2
    } else {
        4
    }
}

/// BER from SER: SER/2 for Gray-coded PAM-4 (a symbol error flips one of two bits), SER for
/// NRZ.
pub fn ber_from_ser(ser: f64, levels: u8) -> f64 {
    if levels == 2 {
        ser
    } else {
        0.5 * ser
    }
}

/// Analyze a PAM-4 or NRZ waveform; the alphabet size is inferred from the symbols
/// ([`infer_levels`]).
///
/// * `noisy` – received waveform (what a scope would capture)
/// * `model` – optional (noiseless waveform, per-sample σ) for semi-analytic SER
pub fn analyze(
    noisy: &[f64],
    model: Option<(&[f64], &[f64])>,
    symbols: &[u8],
    sps: usize,
    opts: &AnalysisOptions,
) -> Pam4Analysis {
    analyze_levels(noisy, model, symbols, infer_levels(symbols), sps, opts)
}

/// [`analyze`] with an explicit alphabet size `levels` (2 = NRZ, 4 = PAM-4). All symbols
/// must be below `levels`.
pub fn analyze_levels(
    noisy: &[f64],
    model: Option<(&[f64], &[f64])>,
    symbols: &[u8],
    levels: u8,
    sps: usize,
    opts: &AnalysisOptions,
) -> Pam4Analysis {
    let m = levels.max(2) as usize;
    let group = |v: &[f64], s: &[u8]| group(v, s, m);
    let sigma_clk: Option<Vec<f64>> = match model {
        Some((y0, sg)) if opts.clock_rj_rms_ui > 0.0 => {
            Some(with_clock_jitter(y0, sg, sps, opts.clock_rj_rms_ui))
        }
        _ => None,
    };
    let model = match (&sigma_clk, model) {
        (Some(s), Some((y0, _))) => Some((y0, s.as_slice())),
        (_, m) => m,
    };
    let delay = opts
        .delay
        .unwrap_or_else(|| align(model.map_or(noisy, |m| m.0), symbols, sps));
    let s_noisy = Sampler::new(noisy, sps, delay);
    let s_model =
        model.map(|(y0, sg)| (Sampler::new(y0, sps, delay), Sampler::new(sg, sps, delay)));
    let np = opts.phases_per_ui.max(1);
    let phases: Vec<f64> = (0..np).map(|i| i as f64 / np as f64).collect();

    // Phase search: minimize semi-analytic SER if possible, else level-Gaussian SER.
    // Coarse-to-fine: every 4th phase, then all phases within ±3 of the coarse optimum.
    let score_at = |ph: f64| -> (f64, f64) {
        if let Some((y0, sg)) = &s_model {
            let y = y0.at_phase(ph);
            let g = group(&y, symbols);
            let th = thresholds_from_means(&level_means(&g));
            let ser = semi_analytic_ser(&y, &sg.at_phase(ph), symbols, &th);
            // Tie-break on the noiseless inner-eye opening (when SER underflows to 0).
            let open = (0..m - 1)
                .map(|k| {
                    let lo = g[k].iter().cloned().fold(f64::MIN, f64::max);
                    let hi = g[k + 1].iter().cloned().fold(f64::MAX, f64::min);
                    hi - lo
                })
                .fold(f64::INFINITY, f64::min);
            (ser, open)
        } else {
            let g = group(&s_noisy.at_phase(ph), symbols);
            let mu = level_means(&g);
            let th = thresholds_from_means(&mu);
            (gaussian_ser(&mu, &level_stds(&g), &th), 0.0)
        }
    };
    let better = |a: (f64, f64), b: (f64, f64)| a.0 < b.0 || (a.0 == b.0 && a.1 > b.1);
    let coarse = if np >= 16 { 4 } else { 1 };
    let mut best = (0usize, (f64::INFINITY, f64::NEG_INFINITY));
    let mut seen = vec![false; np];
    for i in (0..np).step_by(coarse) {
        seen[i] = true;
        let s = score_at(phases[i]);
        if better(s, best.1) {
            best = (i, s);
        }
    }
    if coarse > 1 {
        let c = best.0 as i64;
        for d in -(coarse as i64 - 1)..=(coarse as i64 - 1) {
            let i = (c + d).rem_euclid(np as i64) as usize;
            if seen[i] {
                continue;
            }
            seen[i] = true;
            let s = score_at(phases[i]);
            if better(s, best.1) {
                best = (i, s);
            }
        }
    }
    let phase = phases[best.0];

    let y_opt = s_noisy.at_phase(phase);
    let mut groups = group(&y_opt, symbols);
    let level_mean = level_means(&groups);
    let level_std = level_stds(&groups);
    let thresholds = match &s_model {
        Some((y0, _)) => thresholds_from_means(&level_means(&group(&y0.at_phase(phase), symbols))),
        None => thresholds_from_means(&level_mean),
    };
    let p = opts.eye_probability;
    let (lo, hi) = level_edges(&mut groups, p);
    let eye_height: Vec<f64> = (0..m - 1).map(|k| lo[k + 1] - hi[k]).collect();

    // Eye widths: walk left/right from the optimum while the eye stays open at θ_k.
    type Edges = (Vec<f64>, Vec<f64>);
    let mut open_cache: Vec<Option<Edges>> = vec![None; 2 * np + 1];
    let mut edges_at = |step: i64| -> Edges {
        let idx = (step + np as i64) as usize;
        if let Some(e) = &open_cache[idx] {
            return e.clone();
        }
        let ph = phase + step as f64 / np as f64;
        let mut g = group(&s_noisy.at_phase(ph), symbols);
        let e = level_edges(&mut g, p);
        open_cache[idx] = Some(e.clone());
        e
    };
    let mut eye_width = vec![0.0; m - 1];
    for (k, ew) in eye_width.iter_mut().enumerate() {
        let open = |e: &Edges| e.0[k + 1] > thresholds[k] && e.1[k] < thresholds[k];
        if !open(&edges_at(0)) {
            continue;
        }
        let mut count = 1;
        for dir in [-1i64, 1] {
            for step in 1..np as i64 {
                if count >= np {
                    break;
                }
                if open(&edges_at(dir * step)) {
                    count += 1;
                } else {
                    break;
                }
            }
        }
        *ew = count.min(np) as f64 / np as f64;
    }

    // Counted errors on the noisy waveform.
    let errors = y_opt
        .iter()
        .zip(symbols)
        .filter(|(&y, &k)| thresholds.iter().filter(|&&t| y > t).count() != k as usize)
        .count();
    let ser = s_model.as_ref().map(|(y0, sg)| {
        semi_analytic_ser(
            &y0.at_phase(phase),
            &sg.at_phase(phase),
            symbols,
            &thresholds,
        )
    });
    let ser_gaussian = gaussian_ser(&level_mean, &level_std, &thresholds);
    let q_min = (0..m - 1)
        .map(|k| (level_mean[k + 1] - level_mean[k]) / (level_std[k] + level_std[k + 1]))
        .fold(f64::INFINITY, f64::min);

    Pam4Analysis {
        delay,
        phase,
        level_mean,
        level_std,
        thresholds,
        eye_height,
        eye_width,
        ser,
        ser_gaussian,
        ser_counted: errors as f64 / symbols.len() as f64,
        symbol_errors: errors,
        ber: ber_from_ser(ser.unwrap_or(ser_gaussian), levels),
        q_min,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightstone_core::prbs::prbs13q;
    use lightstone_core::rng::Rng;

    fn ideal(levels: &[f64; 4], periods: usize, sps: usize) -> (Vec<u8>, Vec<f64>) {
        let one = prbs13q(lightstone_core::prbs::PRBS13_DEFAULT_SEED);
        let syms: Vec<u8> = (0..periods).flat_map(|_| one.iter().cloned()).collect();
        let w = syms
            .iter()
            .flat_map(|&s| std::iter::repeat_n(levels[s as usize], sps))
            .collect();
        (syms, w)
    }

    /// Clock jitter adds |dy₀/dt|·σ_clk: exact for a sine (derivative per UI 2πc·cos).
    #[test]
    fn clock_jitter_slope_noise() {
        let (ns, sps, c) = (64usize, 16usize, 5.0);
        let n = ns * sps;
        let y: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * c * i as f64 / n as f64).sin())
            .collect();
        // c cycles per record of ns UI: d/d(t/T) = 2π·c/ns·cos.
        let s0 = vec![0.01; n];
        let rj = 0.02;
        let s = with_clock_jitter(&y, &s0, sps, rj);
        for (i, v) in s.iter().enumerate() {
            let d = 2.0 * std::f64::consts::PI * c / ns as f64
                * (2.0 * std::f64::consts::PI * c * i as f64 / n as f64).cos();
            let e = (0.01f64.powi(2) + (d * rj).powi(2)).sqrt();
            assert!((v - e).abs() < 1e-12, "{i}: {v} vs {e}");
        }
    }

    /// V11: noise-free back-to-back PAM-4 — exact levels, EH = level spacing, SER = 0.
    #[test]
    fn v11_noise_free() {
        let lv = [0.1, 0.4, 0.7, 1.0];
        let (syms, w) = ideal(&lv, 1, 8);
        let sigma = vec![0.0; w.len()];
        let a = analyze(
            &w,
            Some((&w, &sigma)),
            &syms,
            8,
            &AnalysisOptions::default(),
        );
        for k in 0..4 {
            assert!((a.level_mean[k] - lv[k]).abs() < 1e-12);
            assert!(a.level_std[k] < 1e-12);
        }
        for k in 0..3 {
            assert!((a.eye_height[k] - 0.3).abs() < 1e-12, "{:?}", a.eye_height);
            // NRZ-hold through cubic interpolation: open except within ~1 sample of edges.
            assert!(a.eye_width[k] > 0.7, "{:?}", a.eye_width);
        }
        assert_eq!(a.symbol_errors, 0);
        assert_eq!(a.ser, Some(0.0));
    }

    /// V12: back-to-back with known Gaussian noise — analytic PAM-4 SER
    /// SER = (3/2)·Q(d/(2σ)) for level spacing d.
    #[test]
    fn v12_gaussian_noise_ser() {
        let d = 0.3;
        let lv = [0.1, 0.4, 0.7, 1.0];
        let sps = 2;
        let (syms, w) = ideal(&lv, 64, sps);
        let sig = 0.06;
        let mut rng = Rng::new(3);
        let noisy: Vec<f64> = w.iter().map(|v| v + sig * rng.normal()).collect();
        let sigma = vec![sig; w.len()];
        let opts = AnalysisOptions {
            delay: Some(0),
            // Only exact sample positions: cubic interpolation of an NRZ-hold (non-band-
            // limited) waveform overshoots at fractional phases.
            phases_per_ui: 2,
            ..AnalysisOptions::default()
        };
        let a = analyze(&noisy, Some((&w, &sigma)), &syms, sps, &opts);
        let exact = 1.5 * q_func(d / (2.0 * sig));
        let ser = a.ser.unwrap();
        // Exact up to PRBS13Q level imbalance (2047 zeros vs 2048 others): ~4e-5 relative.
        assert!(
            ((ser - exact) / exact).abs() < 1e-4,
            "semi {ser} vs {exact}"
        );
        assert!(
            ((a.ser_counted - exact) / exact).abs() < 0.05,
            "counted {} vs {exact}",
            a.ser_counted
        );
        assert!(
            ((a.ser_gaussian - exact) / exact).abs() < 0.05,
            "gaussian {} vs {exact}",
            a.ser_gaussian
        );
    }
}

#[cfg(test)]
mod nrz_tests {
    use super::*;
    use lightstone_core::prbs::{pattern, PRBS13_DEFAULT_SEED};
    use lightstone_core::rng::Rng;

    fn ideal_nrz(lv: [f64; 2], periods: usize, sps: usize) -> (Vec<u8>, Vec<f64>) {
        let one = pattern(2, PRBS13_DEFAULT_SEED).unwrap();
        let syms: Vec<u8> = (0..periods).flat_map(|_| one.iter().cloned()).collect();
        let w = syms
            .iter()
            .flat_map(|&s| std::iter::repeat_n(lv[s as usize], sps))
            .collect();
        (syms, w)
    }

    /// NRZ noise-free back-to-back: two levels, one eye of the full swing, SER = 0.
    #[test]
    fn nrz_noise_free() {
        let (syms, w) = ideal_nrz([0.2, 1.0], 1, 8);
        assert_eq!(infer_levels(&syms), 2);
        let sigma = vec![0.0; w.len()];
        let a = analyze(
            &w,
            Some((&w, &sigma)),
            &syms,
            8,
            &AnalysisOptions::default(),
        );
        assert_eq!(a.level_mean.len(), 2);
        assert_eq!(a.thresholds.len(), 1);
        assert!((a.level_mean[0] - 0.2).abs() < 1e-12 && (a.level_mean[1] - 1.0).abs() < 1e-12);
        assert!((a.thresholds[0] - 0.6).abs() < 1e-12);
        assert!((a.eye_height[0] - 0.8).abs() < 1e-12, "{:?}", a.eye_height);
        assert_eq!(a.ser, Some(0.0));
    }

    /// NRZ with Gaussian noise: SER = Q(d/(2σ)) and BER = SER (no Gray coding).
    #[test]
    fn nrz_gaussian_noise_ser() {
        let d = 0.8;
        let sps = 2;
        let (syms, w) = ideal_nrz([0.2, 1.0], 64, sps);
        let sig = 0.16;
        let mut rng = Rng::new(3);
        let noisy: Vec<f64> = w.iter().map(|&x| x + sig * rng.normal()).collect();
        let sigma = vec![sig; w.len()];
        let opts = AnalysisOptions {
            delay: Some(0),
            phases_per_ui: 2,
            ..Default::default()
        };
        let a = analyze_levels(&noisy, Some((&w, &sigma)), &syms, 2, sps, &opts);
        let exact = q_func(d / (2.0 * sig));
        let ser = a.ser.unwrap();
        assert!((ser - exact).abs() / exact < 1e-3, "{ser} vs {exact}");
        assert_eq!(a.ber, ser);
        // Counted errors agree statistically (~0.6 % of 524k symbols).
        assert!(
            (a.ser_counted - exact).abs() / exact < 0.05,
            "{}",
            a.ser_counted
        );
    }
}

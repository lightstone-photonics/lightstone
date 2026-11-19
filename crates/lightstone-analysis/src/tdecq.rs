//! TDECQ (transmitter and dispersion eye closure, quaternary), IEEE 802.3-style.
//!
//! `eq:tdecq` — Following the IEEE 802.3 definition (Clauses 121/138), as summarized in
//! IEEE 802.3cd contributions:
//!
//! TDECQ = 10·log₁₀( (OMA_outer / (6·Q_t)) / (σ_G / C_eq) )
//!
//! * the waveform is observed through the reference receiver (4th-order Bessel–Thomson,
//!   f₃dB = 0.5 × baud) — the input is expected to be already filtered;
//! * a 5-tap, T-spaced FFE with Σcᵢ = 1 equalizes the waveform; C_eq is its noise enhancement
//!   for noise shaped by the reference receiver;
//! * σ_G is the largest Gaussian noise (at the equalizer output) for which the SER at both
//!   time slices (eye center ± 0.05 UI, histogram width 0.04 UI) is ≤ 4.8 × 10⁻⁴, using fixed
//!   thresholds P_ave − OMA/3, P_ave, P_ave + OMA/3;
//! * Q_t = 3.414 (1.5·Q(Q_t) = 4.8 × 10⁻⁴).
//!
//! Implementation notes (deviations, documented and sized in docs/model/tdecq.md, V19):
//! * FFE taps are the constrained least-squares (MMSE) solution, not the TDECQ-minimizing taps.
//! * Symbol-aware histograms (the known pattern assigns each sample to its level); identical to
//!   the standard's symbol-agnostic histograms while no sample lies beyond its threshold.
//! * The eye-center phase is the one minimizing TDECQ, searched near the MMSE optimum and near
//!   the standard's center (mean P_ave crossing + 0.5 UI); the standard (D3.4+) allows the
//!   0.1 UI histogram pair to be shifted.
//! * OMA_outer per 121.8.4 (central 2 UI of a run of 7 threes and of 6 zeros, as in PRBS13Q);
//!   patterns without such runs fall back to settled runs at mid-symbol.
//! * Cross-checked against an independent implementation of the standard's text
//!   (reference/lightstone_ref, V19); not against a commercial scope.

use crate::align::{align, Sampler};
use crate::stats::{q_func, q_inv};
use lightstone_core::dsp::BesselThomson4;
use lightstone_core::ffe::{measure_oma, plateau_center, solve_dense};
use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Target SER for TDECQ.
pub const SER_TARGET: f64 = 4.8e-4;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TdecqOptions {
    pub phases_per_ui: usize,
    /// Reference receiver −3 dB frequency relative to baud (for C_eq).
    pub ref_f3db_rel: f64,
    /// Fixed alignment delay; `None` = cross-correlation.
    pub delay: Option<usize>,
    /// Histogram bins across the waveform range.
    pub histogram_bins: usize,
    /// OMA_outer measured separately (e.g. per 121.8.4); `None` = settled runs of the pattern.
    pub oma_outer: Option<f64>,
}

impl Default for TdecqOptions {
    fn default() -> Self {
        Self {
            phases_per_ui: 64,
            ref_f3db_rel: 0.5,
            delay: None,
            histogram_bins: 1000,
            oma_outer: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TdecqResult {
    /// TDECQ in dB; `None` if the eye cannot reach the target SER even without added noise.
    pub tdecq_db: Option<f64>,
    /// 10·log₁₀(C_eq) \[dB\].
    pub ceq_db: f64,
    pub taps: [f64; 5],
    pub oma_outer: f64,
    pub p_ave: f64,
    pub thresholds: [f64; 3],
    /// σ_G at the equalizer output.
    pub sigma_g: f64,
    pub phase: f64,
    pub delay: usize,
}

/// Q_t such that 1.5·Q(Q_t) = SER_TARGET (≈ 3.414).
pub fn q_t() -> f64 {
    q_inv(SER_TARGET / 1.5)
}

/// Constrained LS FFE: minimize Σ(Σⱼ cⱼ y_{n+2−j} − tₙ)² subject to Σcⱼ = 1.
/// Returns (taps, mean squared error).
fn design_ffe(y: &[f64], t: &[f64]) -> ([f64; 5], f64) {
    let n = y.len();
    let at = |i: i64| y[i.rem_euclid(n as i64) as usize];
    let mut r = vec![vec![0.0; 6]; 6];
    let mut p = vec![0.0; 6];
    for k in 0..n as i64 {
        let v: [f64; 5] = std::array::from_fn(|j| at(k + 2 - j as i64));
        for i in 0..5 {
            for j in 0..5 {
                r[i][j] += v[i] * v[j];
            }
            p[i] += v[i] * t[k as usize];
        }
    }
    for i in 0..5 {
        r[i][5] = 1.0;
        r[5][i] = 1.0;
    }
    p[5] = 1.0;
    let x = solve_dense(r, p);
    let c: [f64; 5] = std::array::from_fn(|i| x[i]);
    let mut mse = 0.0;
    for k in 0..n as i64 {
        let e: f64 = (0..5).map(|j| c[j] * at(k + 2 - j as i64)).sum::<f64>() - t[k as usize];
        mse += e * e;
    }
    (c, mse / n as f64)
}

/// Noise enhancement of the FFE for noise shaped by the reference receiver.
fn noise_enhancement(c: &[f64; 5], baud: f64, f3db_rel: f64) -> f64 {
    let bt = BesselThomson4::new(f3db_rel * baud);
    let t = 1.0 / baud;
    let fmax = 6.0 * baud;
    let m = 6000;
    let df = fmax / m as f64;
    let (mut num, mut den) = (0.0, 0.0);
    for i in 0..m {
        let f = (i as f64 + 0.5) * df;
        let hr = bt.response(f).norm_sqr();
        let mut hf = Complex64::new(0.0, 0.0);
        for (j, cj) in c.iter().enumerate() {
            hf += Complex64::from_polar(*cj, -2.0 * PI * f * (j as f64 - 2.0) * t);
        }
        num += hr * hf.norm_sqr();
        den += hr;
    }
    (num / den).sqrt()
}

/// Per-level histograms of equalized values at one time slice.
struct SliceHist {
    lo: f64,
    bin_width: f64,
    counts: [Vec<u32>; 4],
    total: usize,
}

impl SliceHist {
    fn new(values: &[(u8, f64)], lo: f64, hi: f64, n_bins: usize) -> Self {
        let bin_width = (hi - lo) / n_bins as f64;
        let mut counts: [Vec<u32>; 4] = std::array::from_fn(|_| vec![0u32; n_bins]);
        for &(k, v) in values {
            let i = (((v - lo) / bin_width).floor() as i64).clamp(0, n_bins as i64 - 1) as usize;
            counts[k as usize][i] += 1;
        }
        Self {
            lo,
            bin_width,
            counts,
            total: values.len(),
        }
    }

    fn ser(&self, sigma: f64, th: &[f64; 3]) -> f64 {
        let mut acc = 0.0;
        for k in 0..4 {
            for (i, &c) in self.counts[k].iter().enumerate() {
                if c == 0 {
                    continue;
                }
                let v = self.lo + (i as f64 + 0.5) * self.bin_width;
                let mut p = 0.0;
                if sigma == 0.0 {
                    if k > 0 && v <= th[k - 1] {
                        p += 1.0;
                    }
                    if k < 3 && v >= th[k] {
                        p += 1.0;
                    }
                } else {
                    if k > 0 {
                        p += q_func((v - th[k - 1]) / sigma);
                    }
                    if k < 3 {
                        p += q_func((th[k] - v) / sigma);
                    }
                }
                acc += c as f64 * p;
            }
        }
        acc / self.total as f64
    }
}

/// OMA_outer per IEEE 802.3 121.8.4: P3 is the mean over the central 2 UI of a run of exactly
/// 7 threes, P0 the same for a run of exactly 6 zeros (all such runs averaged). `None` if the
/// pattern has no such runs.
fn oma_outer_121(waveform: &[f64], symbols: &[u8], sps: usize, delay: usize) -> Option<f64> {
    let n = symbols.len();
    let nw = waveform.len();
    let level = |k: u8, run: usize| -> Option<f64> {
        let (mut acc, mut cnt) = (0.0, 0usize);
        for s in 0..n {
            if symbols[s] != k || symbols[(s + n - 1) % n] == k {
                continue;
            }
            let len = (0..=run).take_while(|&d| symbols[(s + d) % n] == k).count();
            if len != run {
                continue;
            }
            // Samples with time (in UI from the run start) in [a, a + 2), a = (run − 2)/2.
            let a = (run - 2) as f64 / 2.0;
            let i0 = (a * sps as f64 - 1e-9).ceil() as usize;
            let i1 = ((a + 2.0) * sps as f64 - 1e-9).ceil() as usize;
            for i in i0..i1 {
                acc += waveform[(s * sps + delay + i) % nw];
                cnt += 1;
            }
        }
        (cnt > 0).then(|| acc / cnt as f64)
    };
    Some(level(3, 7)? - level(0, 6)?)
}

/// Mean P_ave crossing phase of the waveform \[UI, circular mean\], relative to the symbols.
fn crossing_phase(waveform: &[f64], sps: usize, delay: usize, p_ave: f64) -> f64 {
    let n = waveform.len();
    let (mut cs, mut sn) = (0.0, 0.0);
    for i in 0..n {
        let (a, b) = (waveform[i] - p_ave, waveform[(i + 1) % n] - p_ave);
        if (a < 0.0) != (b < 0.0) {
            let t = i as f64 + a / (a - b) - delay as f64;
            let ph = 2.0 * PI * t.rem_euclid(sps as f64) / sps as f64;
            cs += ph.cos();
            sn += ph.sin();
        }
    }
    (sn.atan2(cs) / (2.0 * PI)).rem_euclid(1.0)
}

/// Compute TDECQ of an optical waveform already filtered by the reference receiver.
pub fn tdecq(
    waveform: &[f64],
    symbols: &[u8],
    sps: usize,
    baud: f64,
    opts: &TdecqOptions,
) -> TdecqResult {
    let delay = opts.delay.unwrap_or_else(|| align(waveform, symbols, sps));
    let s = Sampler::new(waveform, sps, delay);
    let n = symbols.len();
    let p_ave = waveform.iter().sum::<f64>() / waveform.len() as f64;
    let oma = opts
        .oma_outer
        .or_else(|| oma_outer_121(waveform, symbols, sps, delay))
        .unwrap_or_else(|| measure_oma(&s, symbols, 4));
    let thresholds = [p_ave - oma / 3.0, p_ave, p_ave + oma / 3.0];
    let target: Vec<f64> = symbols
        .iter()
        .map(|&k| p_ave + oma * (k as f64 - 1.5) / 3.0)
        .collect();
    let np = opts.phases_per_ui.max(16);
    let qt = q_t();

    // 1) MMSE phase search (cheap).
    let mut mses = Vec::with_capacity(np);
    let mut per_phase: Vec<[f64; 5]> = Vec::with_capacity(np);
    for i in 0..np {
        let ph = i as f64 / np as f64;
        let y = s.at_phase(ph);
        let (c, mse) = design_ffe(&y, &target);
        per_phase.push(c);
        mses.push(mse);
    }
    let center = plateau_center(&mses, 1e-9 * oma * oma);
    // The standard's center: mean P_ave crossing + 0.5 UI (robust where the MMSE optimum is not).
    let cross = crossing_phase(waveform, sps, delay, p_ave);
    let center_x = (((cross + 0.5) * np as f64).round() as usize) % np;

    // 2) Full TDECQ for phases near both centers.
    let slice_off = 0.05;
    // Histogram window 0.04 UI, uniform in time: 16 midpoints (converged to ~0.003 dB, V19).
    let window: Vec<f64> = (0..16)
        .map(|i| ((i as f64 + 0.5) / 16.0 - 0.5) * 0.04)
        .collect();
    // ±1/16 UI around each center.
    let search = (np / 16).max(2) as i64;
    let mut candidates: Vec<usize> = Vec::new();
    for c in [center, center_x] {
        for di in -search..=search {
            let idx = (c as i64 + di).rem_euclid(np as i64) as usize;
            if !candidates.contains(&idx) {
                candidates.push(idx);
            }
        }
    }
    let mut best: Option<TdecqResult> = None;
    for idx in candidates {
        let ph = idx as f64 / np as f64;
        let c = per_phase[idx];
        let ceq = noise_enhancement(&c, baud, opts.ref_f3db_rel);
        let mut hists = Vec::with_capacity(2);
        for side in [-slice_off, slice_off] {
            let mut vals = Vec::with_capacity(n * window.len());
            for w in &window {
                let y = s.at_phase(ph + side + w);
                for k in 0..n {
                    let e: f64 = (0..5).map(|j| c[j] * y[(k + n + 2 - j) % n]).sum();
                    vals.push((symbols[k], e));
                }
            }
            let lo = vals.iter().map(|v| v.1).fold(f64::INFINITY, f64::min);
            let hi = vals.iter().map(|v| v.1).fold(f64::NEG_INFINITY, f64::max);
            let pad = 1e-9 * (hi - lo).abs().max(oma.abs()).max(1e-300);
            hists.push(SliceHist::new(
                &vals,
                lo - pad,
                hi + pad,
                opts.histogram_bins,
            ));
        }
        let ser_at = |sg: f64| {
            hists
                .iter()
                .map(|h| h.ser(sg, &thresholds))
                .fold(0.0, f64::max)
        };
        let (tdecq_db, sigma_g) = if oma <= 0.0 || ser_at(0.0) > SER_TARGET {
            (None, 0.0)
        } else {
            let (mut lo, mut hi) = (0.0, oma);
            // 40 halvings: σ_G resolved to 1e-12 of OMA.
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if ser_at(mid) <= SER_TARGET {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let sg = lo;
            let r = (oma / (6.0 * qt)) / (sg / ceq);
            (Some(10.0 * r.log10()), sg)
        };
        let res = TdecqResult {
            tdecq_db,
            ceq_db: 10.0 * ceq.log10(),
            taps: c,
            oma_outer: oma,
            p_ave,
            thresholds,
            sigma_g,
            phase: ph,
            delay,
        };
        let better = match (&best, res.tdecq_db) {
            (None, _) => true,
            (Some(b), Some(t)) => b.tdecq_db.is_none_or(|bt| t < bt),
            (Some(_), None) => false,
        };
        if better {
            best = Some(res);
        }
    }
    best.expect("at least one phase evaluated")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightstone_core::prbs::{prbs13q, PRBS13_DEFAULT_SEED};

    #[test]
    fn qt_value() {
        assert!((q_t() - 3.414).abs() < 1e-3, "{}", q_t());
    }

    /// V13: TDECQ of an ideal (ISI-free) PAM-4 signal is 0 dB.
    #[test]
    fn v13_ideal_signal() {
        let syms = prbs13q(PRBS13_DEFAULT_SEED);
        let lv = [0.2e-3, 0.6e-3, 1.0e-3, 1.4e-3];
        let sps = 8;
        let w: Vec<f64> = syms
            .iter()
            .flat_map(|&s| std::iter::repeat_n(lv[s as usize], sps))
            .collect();
        let r = tdecq(
            &w,
            &syms,
            sps,
            53.125e9,
            &TdecqOptions {
                delay: Some(0),
                ..TdecqOptions::default()
            },
        );
        let t = r.tdecq_db.unwrap();
        assert!(t.abs() < 0.1, "TDECQ {t} dB, taps {:?}", r.taps);
        assert!((r.oma_outer - 1.2e-3).abs() < 1e-12);
    }

    #[test]
    fn isi_increases_tdecq() {
        use lightstone_core::dsp::Spectral;
        let syms = prbs13q(PRBS13_DEFAULT_SEED);
        let lv = [0.2e-3, 0.6e-3, 1.0e-3, 1.4e-3];
        let sps = 16;
        let baud = 53.125e9;
        let w: Vec<f64> = syms
            .iter()
            .flat_map(|&s| std::iter::repeat_n(lv[s as usize], sps))
            .collect();
        let sp = Spectral::new(w.len());
        let mut last = -1.0;
        for f3 in [0.75, 0.5, 0.35] {
            let bt = BesselThomson4::new(f3 * baud);
            let y = sp.filter(&w, baud * sps as f64, |f| bt.response(f));
            let t = tdecq(&y, &syms, sps, baud, &TdecqOptions::default())
                .tdecq_db
                .unwrap();
            assert!(t > last, "f3 {f3}: {t} <= {last}");
            last = t;
        }
    }
}

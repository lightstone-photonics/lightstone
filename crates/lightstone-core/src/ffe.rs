//! Receiver feed-forward equalizer (node kind `ffe`, `docs/model/ffe.md`): T-spaced taps,
//! fixed or adapted once by constrained MMSE on the noiseless waveform (optionally including
//! the noise autocorrelation). After adaptation it is a fixed linear filter, so σ(t) stays
//! exact. Lives in the core (not the analysis crate) because the reference lane uses it.

// Index loops mirror the equations (taps i, j).
#![allow(clippy::needless_range_loop)]

use crate::align::Sampler;
use crate::component::*;
use crate::dsp::Spectral;
use crate::signal::{ElectricalSignal, Spectrum, Strength};
use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Reference symbols and their alphabet size (2 = NRZ, 4 = PAM-4).
fn symbols(s: &Signal) -> Result<(&[u8], u8), String> {
    let d = s.as_data()?;
    if d.levels != 4 && d.levels != 2 {
        return Err("the reference must be a PAM-4 or NRZ symbol stream".into());
    }
    Ok((&d.symbols, d.levels))
}

fn json<T: Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("serializable")
}

/// Solve the small dense system A x = b (Gaussian elimination with partial pivoting).
pub fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n)
            .max_by(|&i, &j| a[i][c].abs().partial_cmp(&a[j][c].abs()).unwrap())
            .unwrap();
        a.swap(c, p);
        b.swap(c, p);
        let d = a[c][c];
        for r in c + 1..n {
            let f = a[r][c] / d;
            for k in c..n {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s -= a[r][k] * x[k];
        }
        x[r] = s / a[r][r];
    }
    x
}

/// Measure OMA_outer from settled runs of the outer levels (0 and `levels` − 1) at
/// mid-symbol.
pub fn measure_oma(s: &Sampler, symbols: &[u8], levels: u8) -> f64 {
    let n = symbols.len();
    let top = levels.max(2) - 1;
    for half in [3usize, 2, 1, 0] {
        let mut acc = [(0.0, 0usize); 2];
        for i in 0..n {
            let k = symbols[i];
            if k != 0 && k != top {
                continue;
            }
            let settled =
                (1..=half).all(|d| symbols[(i + n - d) % n] == k && symbols[(i + d) % n] == k);
            if settled {
                let slot = if k == top { 1 } else { 0 };
                acc[slot].0 += s.symbol(i, 0.5);
                acc[slot].1 += 1;
            }
        }
        if acc[0].1 >= 4 && acc[1].1 >= 4 {
            return acc[1].0 / acc[1].1 as f64 - acc[0].0 / acc[0].1 as f64;
        }
    }
    0.0
}

/// Index at the middle of the longest circular run of values within `tol` of the minimum.
pub fn plateau_center(v: &[f64], tol: f64) -> usize {
    let n = v.len();
    let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
    let ok: Vec<bool> = v.iter().map(|&x| x <= min + tol).collect();
    if ok.iter().all(|&b| b) {
        return n / 2;
    }
    // Start scanning after a non-ok index so runs do not wrap.
    let start = ok.iter().position(|&b| !b).unwrap();
    let (mut best_len, mut best_mid, mut run_start, mut run_len) = (0, 0, 0, 0);
    for s in 1..=n {
        let i = (start + s) % n;
        if ok[i] {
            if run_len == 0 {
                run_start = s;
            }
            run_len += 1;
            if run_len > best_len {
                best_len = run_len;
                best_mid = (start + run_start + (run_len - 1) / 2) % n;
            }
        } else {
            run_len = 0;
        }
    }
    best_mid
}

/// Parameters of the receiver FFE (node kind `ffe`, `docs/model/ffe.md`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct FfeParams {
    /// Number of T-spaced taps.
    pub n_taps: usize,
    /// Index of the main cursor (default: the middle tap).
    pub main: Option<usize>,
    /// Fixed taps; `None` = adapt (constrained MMSE).
    pub taps: Option<Vec<f64>>,
    /// Include the receiver noise in the MMSE design (noise enhancement).
    pub noise_aware: bool,
    /// Sampling phases per UI searched during adaptation.
    pub phases_per_ui: usize,
}

impl Default for FfeParams {
    fn default() -> Self {
        Self {
            n_taps: 5,
            main: None,
            taps: None,
            noise_aware: true,
            phases_per_ui: 32,
        }
    }
}

/// Result of an FFE design.
#[derive(Clone, Debug, Serialize)]
pub struct FfeDesign {
    pub taps: Vec<f64>,
    pub main: usize,
    /// Sampling phase \[UI\] the taps were designed for (`None` for fixed taps).
    pub phase: Option<f64>,
    pub delay: Option<usize>,
    /// Mean squared error at the design phase: ISI + noise (if noise-aware) \[unit²\].
    pub mse: Option<f64>,
    pub adapted: bool,
    pub noise_aware: bool,
}

/// Constrained MMSE taps (`eq:ffe-mmse`): minimize
/// (1/N) Σₙ (Σⱼ cⱼ y_{n+m−j} − tₙ)² + cᵀ R c subject to Σcⱼ = 1, where y are the noiseless
/// symbol-rate samples, t the target levels, m the main cursor and R the noise
/// autocorrelation at T-spaced lags (zero for a noise-free design). Returns (taps, MSE).
pub fn design_ffe(
    y: &[f64],
    target: &[f64],
    n_taps: usize,
    main: usize,
    noise_acf: &[f64],
) -> (Vec<f64>, f64) {
    let n = y.len();
    let at = |i: i64| y[i.rem_euclid(n as i64) as usize];
    let l = n_taps;
    let mut a = vec![vec![0.0; l + 1]; l + 1];
    let mut b = vec![0.0; l + 1];
    for k in 0..n as i64 {
        let v: Vec<f64> = (0..l).map(|j| at(k + main as i64 - j as i64)).collect();
        for i in 0..l {
            for j in 0..l {
                a[i][j] += v[i] * v[j];
            }
            b[i] += v[i] * target[k as usize];
        }
    }
    let nf = n as f64;
    for i in 0..l {
        for j in 0..l {
            a[i][j] = a[i][j] / nf + noise_acf.get(i.abs_diff(j)).copied().unwrap_or(0.0);
        }
        b[i] /= nf;
        a[i][l] = 1.0;
        a[l][i] = 1.0;
    }
    b[l] = 1.0;
    let x = solve_dense(a, b);
    let c: Vec<f64> = x[..l].to_vec();
    let mut mse = 0.0;
    for k in 0..n as i64 {
        let e: f64 = (0..l)
            .map(|j| c[j] * at(k + main as i64 - j as i64))
            .sum::<f64>()
            - target[k as usize];
        mse += e * e;
    }
    let noise: f64 = (0..l)
        .flat_map(|i| (0..l).map(move |j| (i, j)))
        .map(|(i, j)| c[i] * c[j] * noise_acf.get(i.abs_diff(j)).copied().unwrap_or(0.0))
        .sum();
    (c, mse / nf + noise)
}

/// Time-averaged noise autocorrelation of an electrical signal at lags of `stride` samples,
/// for lags 0 … `lags` − 1: R(τ) = Σ gain²·(fs/2)·S̄·(g ⋆ g)(τ), g = IFFT(filter).
/// Noise after a nonlinear block (explicit realization only) is estimated from the
/// realization itself.
fn noise_acf(e: &ElectricalSignal, sp: &Spectral, fs: f64, stride: usize, lags: usize) -> Vec<f64> {
    let n = sp.len();
    let half = 0.5 * fs;
    let mut psd = vec![Complex64::new(0.0, 0.0); n];
    for t in &e.noise {
        let s_mean = match &t.strength {
            Strength::Constant(s) => *s,
            Strength::Varying { spectrum, .. } => spectrum[0].re / n as f64,
        };
        let h2 = |k: usize| t.filter.as_ref().map_or(1.0, |h| h[k].norm_sqr());
        if s_mean > 0.0 {
            for (k, p) in psd.iter_mut().enumerate() {
                *p += t.gain * t.gain * half * s_mean * h2(k);
            }
        } else if let crate::signal::Realization::Given(g) = &t.realization {
            // Explicit realization (e.g. after a nonlinear block): its periodogram.
            for (k, p) in psd.iter_mut().enumerate() {
                *p += t.gain * t.gain * g[k].norm_sqr() * h2(k) / n as f64;
            }
        }
    }
    let r = sp.inverse_real(psd);
    (0..lags).map(|i| r[(i * stride) % n]).collect()
}

/// H(f) of T-spaced taps with main cursor `main`: tap j at delay (j − main)·T.
fn taps_transfer(taps: &[f64], main: usize, sps: usize, sp: &Spectral) -> Spectrum {
    let n = sp.len();
    let mut g = vec![0.0; n];
    for (j, c) in taps.iter().enumerate() {
        let off = (j as i64 - main as i64) * sps as i64;
        g[off.rem_euclid(n as i64) as usize] += c;
    }
    Arc::new(sp.forward(&g))
}

pub(crate) struct Ffe(FfeParams);

impl Ffe {
    pub(crate) fn new(p: FfeParams) -> Result<Self, String> {
        let l = p.taps.as_ref().map_or(p.n_taps, Vec::len);
        if l == 0 {
            return Err("the FFE needs at least one tap".into());
        }
        if let Some(t) = &p.taps {
            if t.iter().any(|x| !x.is_finite()) {
                return Err("FFE taps contain NaN or infinite values".into());
            }
        }
        if p.main.is_some_and(|m| m >= l) {
            return Err(format!(
                "main cursor index must be < the number of taps ({l})"
            ));
        }
        if p.phases_per_ui == 0 {
            return Err("phases_per_ui must be ≥ 1".into());
        }
        Ok(Self(p))
    }
}

impl Component for Ffe {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![
            PortSpec::new("in", PortType::Electrical(None)),
            PortSpec::monitor("ref", PortType::Data),
        ]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", PortType::Electrical(None))]
    }
    fn linearity(&self) -> Linearity {
        // Once adapted, the taps are fixed: the FFE is a linear filter (noise stays exact).
        Linearity::Linear
    }
    fn process(&self, ctx: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let p = &self.0;
        let e = inputs[0].as_electrical()?;
        let (sym, levels) = symbols(&inputs[1])?;
        let (sps, fs) = (ctx.grid.sps, ctx.grid.fs());
        let sp = ctx.spectral.clone();
        let l = p.taps.as_ref().map_or(p.n_taps, Vec::len);
        let main = p.main.unwrap_or(l / 2);
        if (l - 1) * sps >= sp.len() {
            return Err(format!("{l} taps do not fit into one pattern period"));
        }
        let design = match &p.taps {
            Some(t) => FfeDesign {
                taps: t.clone(),
                main,
                phase: None,
                delay: None,
                mse: None,
                adapted: false,
                noise_aware: false,
            },
            None => {
                // Adaptation on the noiseless waveform (plus the noise statistics if
                // noise-aware), targeting the ideal PAM-4 grid at the input's mean and OMA.
                let y0 = e.noiseless(&sp);
                let delay = crate::align::align(&y0, sym, sps);
                let s = Sampler::new(&y0, sps, delay);
                let mean = y0.iter().sum::<f64>() / y0.len() as f64;
                let oma = measure_oma(&s, sym, levels);
                if oma <= 0.0 {
                    return Err("cannot adapt the FFE: the input has no signal swing".into());
                }
                // Ideal grid: mean + OMA·(k − (M−1)/2)/(M−1) (PAM-4: (k − 1.5)/3).
                let (mid, steps) = (0.5 * (levels - 1) as f64, (levels - 1) as f64);
                let target: Vec<f64> = sym
                    .iter()
                    .map(|&k| mean + oma * (k as f64 - mid) / steps)
                    .collect();
                let acf = if p.noise_aware {
                    noise_acf(e, &sp, fs, sps, l)
                } else {
                    vec![]
                };
                let np = p.phases_per_ui;
                let per_phase: Vec<(Vec<f64>, f64)> = (0..np)
                    .map(|i| design_ffe(&s.at_phase(i as f64 / np as f64), &target, l, main, &acf))
                    .collect();
                let mses: Vec<f64> = per_phase.iter().map(|x| x.1).collect();
                let best = plateau_center(&mses, 1e-9 * oma * oma);
                let (taps, mse) = per_phase[best].clone();
                FfeDesign {
                    taps,
                    main,
                    phase: Some(best as f64 / np as f64),
                    delay: Some(delay),
                    mse: Some(mse),
                    adapted: true,
                    noise_aware: p.noise_aware,
                }
            }
        };
        let h = taps_transfer(&design.taps, main, sps, &sp);
        let mut out = Output::signals(vec![Signal::Electrical(Arc::new(e.apply_filter(&h)))]);
        out.report = Some(Report::Data {
            kind: "ffe".into(),
            value: Arc::new(json(&design)),
        });
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E1: for a symbol-spaced channel h = [1, a]/(1 + a) (DC gain 1), the noise-free MMSE
    /// FFE converges to the truncated inverse cⱼ = (1 + a)(−a)ʲ (main cursor 0).
    #[test]
    fn e1_mmse_ffe_inverts_known_isi() {
        let sym = crate::prbs::prbs13q(crate::prbs::PRBS13_DEFAULT_SEED);
        let a = 0.3;
        let lv = |k: u8| k as f64 / 3.0;
        let n = sym.len();
        let y: Vec<f64> = (0..n)
            .map(|i| (lv(sym[i]) + a * lv(sym[(i + n - 1) % n])) / (1.0 + a))
            .collect();
        let mean = y.iter().sum::<f64>() / n as f64;
        let target: Vec<f64> = sym.iter().map(|&k| mean + (k as f64 - 1.5) / 3.0).collect();
        let (c, mse) = design_ffe(&y, &target, 9, 0, &[]);
        for (j, cj) in c.iter().enumerate() {
            let inv = (1.0 + a) * (-a).powi(j as i32);
            assert!((cj - inv).abs() < 1e-3, "tap {j}: {cj} vs {inv}");
        }
        assert!((c.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert!(mse < 1e-6, "residual ISI {mse:e}");
    }

    /// Noise-aware design trades ISI for noise: with strong noise the taps stay closer to the
    /// identity (smaller noise gain Σc²) than the noise-free design.
    #[test]
    fn noise_aware_design_limits_noise_enhancement() {
        let sym = crate::prbs::prbs13q(crate::prbs::PRBS13_DEFAULT_SEED);
        let n = sym.len();
        let lv = |k: u8| k as f64 / 3.0;
        let y: Vec<f64> = (0..n)
            .map(|i| 0.6 * lv(sym[i]) + 0.2 * lv(sym[(i + n - 1) % n]) + 0.2 * lv(sym[(i + 1) % n]))
            .collect();
        let mean = y.iter().sum::<f64>() / n as f64;
        let target: Vec<f64> = sym.iter().map(|&k| mean + (k as f64 - 1.5) / 3.0).collect();
        let (c0, _) = design_ffe(&y, &target, 7, 3, &[]);
        let (c1, _) = design_ffe(&y, &target, 7, 3, &[0.05]); // white noise, σ² = 0.05
        let gain = |c: &[f64]| c.iter().map(|x| x * x).sum::<f64>();
        assert!(
            gain(&c1) < 0.7 * gain(&c0),
            "{} vs {}",
            gain(&c1),
            gain(&c0)
        );
    }
}

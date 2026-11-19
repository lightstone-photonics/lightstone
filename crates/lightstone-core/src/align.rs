//! Symbol alignment and fractional-phase sampling.

use num_complex::Complex64;
use rustfft::FftPlanner;

/// Find the integer delay `d` (in samples, 0 ≤ d < N) that best aligns `samples` to the known
/// symbol sequence: samples\[i\] ≈ template\[i − d\] (circular cross-correlation).
pub fn align(samples: &[f64], symbols: &[u8], sps: usize) -> usize {
    let n = samples.len();
    assert_eq!(n, symbols.len() * sps, "samples must be symbols × sps long");
    let mut planner = FftPlanner::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let mean_y = samples.iter().sum::<f64>() / n as f64;
    let mut x: Vec<Complex64> = symbols
        .iter()
        .flat_map(|&s| std::iter::repeat_n(Complex64::new(s as f64 - 1.5, 0.0), sps))
        .collect();
    let mut y: Vec<Complex64> = samples
        .iter()
        .map(|&v| Complex64::new(v - mean_y, 0.0))
        .collect();
    fwd.process(&mut x);
    fwd.process(&mut y);
    let mut c: Vec<Complex64> = x.iter().zip(&y).map(|(a, b)| a.conj() * b).collect();
    inv.process(&mut c);
    let (best, _) =
        c.iter().enumerate().fold(
            (0, f64::MIN),
            |(bi, bv), (i, v)| {
                if v.re > bv {
                    (i, v.re)
                } else {
                    (bi, bv)
                }
            },
        );
    best
}

/// Samples a periodic waveform at fractional positions relative to aligned symbols.
///
/// Position of symbol `n` at phase `ph` (in UI, 0 ≤ ph < 1 is inside the symbol):
/// t = n·sps + delay + ph·sps. Values use Catmull–Rom cubic interpolation.
/// Phases within [−PAD_UI, 1 + PAD_UI] are served from a wrap-padded copy (no modulo in the
/// inner loop).
#[derive(Clone)]
pub struct Sampler {
    ext: Vec<f64>,
    pad: usize,
    n: usize,
    pub sps: usize,
    pub delay: usize,
}

/// Phase range (in UI) supported on either side of the symbol.
pub const PAD_UI: usize = 3;

impl Sampler {
    pub fn new(samples: &[f64], sps: usize, delay: usize) -> Self {
        let n = samples.len();
        let pad = (PAD_UI + 1) * sps + 4;
        let mut ext = Vec::with_capacity(n + 2 * pad);
        for i in 0..pad {
            ext.push(samples[(n - (pad - i) % n) % n]);
        }
        ext.extend_from_slice(samples);
        for i in 0..pad {
            ext.push(samples[i % n]);
        }
        Self {
            ext,
            pad,
            n,
            sps,
            delay: delay % n,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn n_symbols(&self) -> usize {
        self.n / self.sps
    }

    /// Raw sample `i` (periodic).
    pub fn sample(&self, i: usize) -> f64 {
        self.ext[self.pad + i % self.n]
    }

    /// Cubic interpolation at base sample `base` (0 ≤ base < N) plus offset `off` (samples,
    /// |off| ≤ PAD_UI·sps).
    #[inline]
    fn interp(&self, base: usize, off: f64) -> f64 {
        let fl = off.floor();
        let u = off - fl;
        let i = (self.pad as i64 + base as i64 + fl as i64) as usize;
        let (p0, p1, p2, p3) = (
            self.ext[i - 1],
            self.ext[i],
            self.ext[i + 1],
            self.ext[i + 2],
        );
        let u2 = u * u;
        let u3 = u2 * u;
        0.5 * (2.0 * p1
            + (-p0 + p2) * u
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3)
    }

    /// Linear interpolation (cheaper, used for display).
    #[inline]
    pub fn symbol_linear(&self, n: usize, ph: f64) -> f64 {
        let base = (n * self.sps + self.delay) % self.n;
        let off = ph * self.sps as f64;
        let fl = off.floor();
        let u = off - fl;
        let i = (self.pad as i64 + base as i64 + fl as i64) as usize;
        self.ext[i] + u * (self.ext[i + 1] - self.ext[i])
    }

    /// Value of symbol `n` at phase `ph` \[UI\], −PAD_UI ≤ ph ≤ 1 + PAD_UI.
    #[inline]
    pub fn symbol(&self, n: usize, ph: f64) -> f64 {
        debug_assert!(ph >= -(PAD_UI as f64) && ph <= 1.0 + PAD_UI as f64);
        let base = (n * self.sps + self.delay) % self.n;
        self.interp(base, ph * self.sps as f64)
    }

    /// All symbols at one phase.
    pub fn at_phase(&self, ph: f64) -> Vec<f64> {
        (0..self.n_symbols()).map(|k| self.symbol(k, ph)).collect()
    }
}

/// Apply a T-spaced 5-tap FFE (main tap index 2) at the full sample rate:
/// y\[i\] = Σⱼ cⱼ x\[i − (j − 2)·sps\]. `eq:ffe`
pub fn apply_ffe(samples: &[f64], sps: usize, taps: &[f64; 5]) -> Vec<f64> {
    let n = samples.len() as i64;
    (0..n)
        .map(|i| {
            taps.iter()
                .enumerate()
                .map(|(j, c)| {
                    let k = (i - (j as i64 - 2) * sps as i64).rem_euclid(n);
                    c * samples[k as usize]
                })
                .sum()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_delay() {
        let syms: Vec<u8> = (0..500).map(|i| ((i * 7 + i / 3) % 4) as u8).collect();
        let sps = 8;
        let base: Vec<f64> = syms
            .iter()
            .flat_map(|&s| std::iter::repeat_n(s as f64, sps))
            .collect();
        let n = base.len();
        for d in [0usize, 5, 123, n - 3] {
            let shifted: Vec<f64> = (0..n).map(|i| base[(i + n - d) % n]).collect();
            assert_eq!(align(&shifted, &syms, sps), d);
        }
    }

    #[test]
    fn cubic_exact_on_samples_and_periodic() {
        let x: Vec<f64> = (0..64).map(|i| (i as f64 * 0.3).sin()).collect();
        let s = Sampler::new(&x, 4, 5);
        for k in 0..16 {
            for j in -8i64..12 {
                let ph = j as f64 / 4.0;
                let idx = ((k as i64 * 4 + 5 + j).rem_euclid(64)) as usize;
                assert!((s.symbol(k, ph) - x[idx]).abs() < 1e-15);
                assert!((s.symbol_linear(k, ph) - x[idx]).abs() < 1e-15);
            }
        }
    }

    #[test]
    fn ffe_identity_and_shift() {
        let x: Vec<f64> = (0..40).map(|i| i as f64).collect();
        assert_eq!(apply_ffe(&x, 4, &[0.0, 0.0, 1.0, 0.0, 0.0]), x);
        let y = apply_ffe(&x, 4, &[0.0, 1.0, 0.0, 0.0, 0.0]);
        // tap j=1 → x[i + 4]
        assert_eq!(y[0], 4.0);
        assert_eq!(y[39], 3.0);
    }
}

//! Frequency-domain helpers for periodic real waveforms.

use num_complex::Complex64;
use rustfft::{Fft, FftPlanner};
use std::f64::consts::PI;
use std::sync::Arc;

/// Forward/inverse FFT pair for a fixed length.
#[derive(Clone)]
pub struct Spectral {
    n: usize,
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
}

impl std::fmt::Debug for Spectral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Spectral({})", self.n)
    }
}

impl Spectral {
    pub fn new(n: usize) -> Self {
        let mut p = FftPlanner::new();
        Self {
            n,
            fwd: p.plan_fft_forward(n),
            inv: p.plan_fft_inverse(n),
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// FFT of a real signal.
    pub fn forward(&self, x: &[f64]) -> Vec<Complex64> {
        assert_eq!(x.len(), self.n);
        let mut buf: Vec<Complex64> = x.iter().map(|&v| Complex64::new(v, 0.0)).collect();
        self.fwd.process(&mut buf);
        buf
    }

    /// Inverse FFT, returning the real part (normalized by 1/N).
    pub fn inverse_real(&self, mut spec: Vec<Complex64>) -> Vec<f64> {
        assert_eq!(spec.len(), self.n);
        self.inv.process(&mut spec);
        let s = 1.0 / self.n as f64;
        spec.iter().map(|c| c.re * s).collect()
    }

    /// Two real signals from one forward FFT: returns (fft(a), fft(b)).
    pub fn forward_pair(&self, a: &[f64], b: &[f64]) -> (Vec<Complex64>, Vec<Complex64>) {
        assert_eq!(a.len(), self.n);
        assert_eq!(b.len(), self.n);
        let mut buf: Vec<Complex64> = a
            .iter()
            .zip(b)
            .map(|(&x, &y)| Complex64::new(x, y))
            .collect();
        self.fwd.process(&mut buf);
        let n = self.n;
        let mut fa = Vec::with_capacity(n);
        let mut fb = Vec::with_capacity(n);
        for k in 0..n {
            let x = buf[k];
            let xc = buf[(n - k) % n].conj();
            fa.push((x + xc) * 0.5);
            fb.push((x - xc) * Complex64::new(0.0, -0.5));
        }
        (fa, fb)
    }

    /// Two real signals from one inverse FFT: given Hermitian spectra A and B,
    /// returns (ifft(A), ifft(B)) using ifft(A + jB) = a + jb.
    pub fn inverse_real_pair(&self, a: &[Complex64], b: &[Complex64]) -> (Vec<f64>, Vec<f64>) {
        assert_eq!(a.len(), self.n);
        assert_eq!(b.len(), self.n);
        let j = Complex64::new(0.0, 1.0);
        let mut buf: Vec<Complex64> = a.iter().zip(b).map(|(x, y)| x + j * y).collect();
        self.inv.process(&mut buf);
        let s = 1.0 / self.n as f64;
        (
            buf.iter().map(|c| c.re * s).collect(),
            buf.iter().map(|c| c.im * s).collect(),
        )
    }

    /// Apply a transfer function H(f) (circular convolution).
    pub fn filter<F: Fn(f64) -> Complex64>(&self, x: &[f64], fs: f64, h: F) -> Vec<f64> {
        let mut spec = self.forward(x);
        for (k, c) in spec.iter_mut().enumerate() {
            *c *= h(freq(k, self.n, fs));
        }
        self.inverse_real(spec)
    }
}

/// Signed frequency of FFT bin `k` for length `n` at sample rate `fs`.
/// The Nyquist FFT bin of even-length transforms is mapped to +fs/2.
pub fn freq(k: usize, n: usize, fs: f64) -> f64 {
    signed_index(k, n) * fs / n as f64
}

/// Signed time of sample `k` of a periodic grid of length `n` at sample rate `fs`: samples in
/// the second half stand for negative times (the period wraps around). The sample at n/2 of
/// even-length grids is mapped to +n/(2fs), matching [`freq`].
pub fn time(k: usize, n: usize, fs: f64) -> f64 {
    signed_index(k, n) / fs
}

fn signed_index(k: usize, n: usize) -> f64 {
    if k <= n / 2 {
        k as f64
    } else {
        k as f64 - n as f64
    }
}

/// Normalized 4th-order Bessel polynomial transfer function,
/// H(s) = 105 / (s⁴ + 10 s³ + 45 s² + 105 s + 105), unit DC gain.
fn bessel4_norm(s: Complex64) -> Complex64 {
    let s2 = s * s;
    let den = s2 * s2 + 10.0 * s2 * s + 45.0 * s2 + 105.0 * s + 105.0;
    Complex64::new(105.0, 0.0) / den
}

/// Normalized frequency ω at which |H| of the 4th-order Bessel filter is −3 dB.
fn bessel4_w3db() -> f64 {
    // Bisection on |H(jω)|² = 1/2. Root is near 2.1139.
    let (mut lo, mut hi) = (1.0, 4.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        let g = bessel4_norm(Complex64::new(0.0, mid)).norm_sqr();
        if g > 0.5 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// 4th-order Bessel–Thomson low-pass with −3 dB frequency `f3db` \[Hz\].
/// `eq:rx-bessel-thomson`
#[derive(Clone, Copy, Debug)]
pub struct BesselThomson4 {
    pub f3db: f64,
    w3: f64,
}

impl BesselThomson4 {
    pub fn new(f3db: f64) -> Self {
        Self {
            f3db,
            w3: bessel4_w3db(),
        }
    }

    pub fn response(&self, f: f64) -> Complex64 {
        bessel4_norm(Complex64::new(0.0, self.w3 * f / self.f3db))
    }

    /// Group delay at DC \[s\] (the normalized polynomial has unit delay).
    pub fn dc_group_delay(&self) -> f64 {
        self.w3 / (2.0 * PI * self.f3db)
    }

    /// Continuous one-sided noise bandwidth ∫₀^∞ |H(f)|² df \[Hz\].
    pub fn noise_bandwidth(&self) -> f64 {
        // Integrate numerically to 40·f3db (|H|² ~ f⁻⁸ beyond → negligible tail).
        let fmax = 40.0 * self.f3db;
        let n = 400_000;
        let df = fmax / n as f64;
        let mut acc = 0.0;
        for i in 0..n {
            let f = (i as f64 + 0.5) * df;
            acc += self.response(f).norm_sqr();
        }
        acc * df
    }
}

/// Discrete equivalent noise bandwidth of a filter on an `n`-point grid at rate `fs`:
/// white noise with per-sample variance S·fs/2 has output variance S·B. \[Hz\]
pub fn discrete_noise_bandwidth<F: Fn(f64) -> Complex64>(n: usize, fs: f64, h: F) -> f64 {
    let mut acc = 0.0;
    for k in 0..n {
        acc += h(freq(k, n, fs)).norm_sqr();
    }
    0.5 * fs * acc / n as f64
}

/// Find the lowest frequency where |H(f)/H(0)| drops to `level` (e.g. 0.5 for optical −3 dB,
/// 1/√2 for electrical −3 dB). Returns `None` if not reached below `fmax`.
pub fn bandwidth<F: Fn(f64) -> Complex64>(h: F, level: f64, fmax: f64) -> Option<f64> {
    let h0 = h(0.0).norm();
    if h0 == 0.0 {
        return None;
    }
    // Coarse scan then bisection. Callers choose `fmax` ≈ 50/(delay spread), so the step is
    // ≈ 1/40 of the fastest ripple period of |H| and no crossing is skipped.
    let steps = 2_000;
    let df = fmax / steps as f64;
    let mut prev = 0.0;
    for i in 1..=steps {
        let f = i as f64 * df;
        if h(f).norm() / h0 <= level {
            let (mut lo, mut hi) = (prev, f);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if h(mid).norm() / h0 <= level {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(0.5 * (lo + hi));
        }
        prev = f;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt4_is_3db_at_f3db() {
        let bt = BesselThomson4::new(26.5625e9);
        let g = bt.response(26.5625e9).norm_sqr();
        assert!((g - 0.5).abs() < 1e-12);
        assert!((bt.response(0.0).re - 1.0).abs() < 1e-15);
        // Known value of normalized −3 dB frequency for 4th-order Bessel.
        assert!((bt.w3 - 2.1139).abs() < 1e-3);
    }

    #[test]
    fn pair_transforms_match_single() {
        let n = 1000;
        let sp = Spectral::new(n);
        let a: Vec<f64> = (0..n).map(|i| ((i * 7) % 13) as f64 - 6.0).collect();
        let b: Vec<f64> = (0..n).map(|i| ((i as f64) * 0.01).sin()).collect();
        let (fa, fb) = sp.forward_pair(&a, &b);
        let (ga, gb) = (sp.forward(&a), sp.forward(&b));
        for k in 0..n {
            assert!((fa[k] - ga[k]).norm() < 1e-9);
            assert!((fb[k] - gb[k]).norm() < 1e-9);
        }
        let (ra, rb) = sp.inverse_real_pair(&fa, &fb);
        for i in 0..n {
            assert!((ra[i] - a[i]).abs() < 1e-10 && (rb[i] - b[i]).abs() < 1e-10);
        }
    }

    #[test]
    fn filter_identity() {
        let sp = Spectral::new(1000);
        let x: Vec<f64> = (0..1000).map(|i| ((i * 7) % 13) as f64).collect();
        let y = sp.filter(&x, 1.0, |_| Complex64::new(1.0, 0.0));
        for (a, b) in x.iter().zip(&y) {
            assert!((a - b).abs() < 1e-10);
        }
    }

    #[test]
    fn bandwidth_first_order() {
        let fc = 10e9;
        let h = |f: f64| Complex64::new(1.0, 0.0) / Complex64::new(1.0, f / fc);
        let b = bandwidth(h, 1.0 / 2f64.sqrt(), 100e9).unwrap();
        assert!((b - fc).abs() / fc < 1e-9);
    }
}

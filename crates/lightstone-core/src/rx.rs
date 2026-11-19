//! PIN photodiode + TIA receiver.
//!
//! `eq:rx-photocurrent` — I(t) = R · P_det(t).
//!
//! `eq:rx-noise` — Noise sources (one-sided PSDs, added before the receive filter):
//! thermal S_th = i_n², shot S_sh = 2q·I(t), RIN S_RIN = RIN·(R·P(t))² (generated at the
//! transmitter). After the receive filter the variance is σ² = S · B_n with B_n the noise
//! bandwidth of the filter; for RIN, B_RIN = ∫|H_ch(f)·H_rx(f)|²df / |H_ch(0)|² because RIN
//! also passes through the fiber channel.
//!
//! `eq:rx-bessel-thomson` — Receive filter: 4th-order Bessel–Thomson low-pass with
//! f₃dB = bt_f3db_rel · baud (IEEE reference receiver uses 0.5 × baud).

use crate::config::{RxConfig, VcselConfig};
use crate::dsp::{freq, BesselThomson4, Spectral};
use crate::rng::Rng;
use crate::units::{db_to_lin, Q_E};
use num_complex::Complex64;

/// Receiver output waveforms.
#[derive(Clone, Debug)]
pub struct RxOutput {
    /// Noisy TIA output \[V\].
    pub signal: Vec<f64>,
    /// Noiseless TIA output \[V\].
    pub noiseless: Vec<f64>,
    /// Analytic per-sample noise standard deviation at the output \[V\].
    pub sigma: Vec<f64>,
    /// Reference-receiver optical waveform: detected optical power (incl. RIN) through the
    /// receive filter, without receiver noise \[W\]. Used for TDECQ.
    pub reference_optical: Vec<f64>,
    /// Noise bandwidth used for the analytic σ \[Hz\].
    pub noise_bandwidth: f64,
    /// Thermal-only output noise σ \[V\].
    pub thermal_sigma: f64,
}

/// Receive-filter transfer function (identity if disabled).
#[derive(Clone, Copy, Debug)]
pub struct RxFilter {
    bt: Option<BesselThomson4>,
}

impl RxFilter {
    pub fn new(cfg: &RxConfig, baud: f64) -> Self {
        Self {
            bt: cfg
                .filter_enabled
                .then(|| BesselThomson4::new(cfg.bt_f3db_rel * baud)),
        }
    }

    pub fn response(&self, f: f64) -> Complex64 {
        self.bt.map_or(Complex64::new(1.0, 0.0), |b| b.response(f))
    }

    pub fn bessel_thomson(&self) -> Option<BesselThomson4> {
        self.bt
    }
}

/// Source of RIN for the exact analytic variance: RIN noise is generated at the transmitter as
/// white noise with per-sample variance RIN·P_tx(t)²·fs/2 and passes through the channel.
pub struct RinSource<'a> {
    /// Spectrum of P_tx(t)².
    pub p_tx_sq_spec: &'a [Complex64],
    /// Channel transfer function (power domain, includes losses).
    pub channel: &'a [Complex64],
}

/// Run the receiver.
///
/// * `p_spec` – spectrum of the noiseless optical power at the detector
/// * `rin_spec` – spectrum of the RIN component of the optical power at the detector
/// * `rin_source` – needed for the exact analytic RIN variance (if `None`, RIN is assumed to
///   reach the detector unfiltered: σ² = RIN·I(t)²·B_n)
///
/// Analytic σ is exact for the simulated noise processes (white Gaussian sources with
/// signal-dependent variance, linearly filtered):
/// σ²(t) = i_n²·B_n + 2q·(fs/2)·(g_rx² ∗ I_det)(t) + RIN·R²·(fs/2)·(g_tot² ∗ P_tx²)(t),
/// with g_rx, g_tot the discrete impulse responses of the receive filter and channel×filter.
///
/// Returns the receiver output and the (unfiltered) detected optical power \[W\].
#[allow(clippy::too_many_arguments)]
pub fn receive(
    cfg: &RxConfig,
    vcsel: &VcselConfig,
    p_spec: &[Complex64],
    rin_spec: &[Complex64],
    rin_source: Option<RinSource>,
    fs: f64,
    baud: f64,
    spectral: &Spectral,
    rng: &mut Rng,
) -> (RxOutput, Vec<f64>) {
    let n = p_spec.len();
    let filt = RxFilter::new(cfg, baud);
    let hrx: Vec<Complex64> = (0..n).map(|k| filt.response(freq(k, n, fs))).collect();
    let bn = 0.5 * fs * hrx.iter().map(|h| h.norm_sqr()).sum::<f64>() / n as f64;
    let half_fs = 0.5 * fs;
    let r = cfg.responsivity;
    let z = cfg.transimpedance;

    // Noiseless detected power (unfiltered) and its filtered version from one inverse FFT.
    let p_spec_f: Vec<Complex64> = p_spec.iter().zip(&hrx).map(|(a, h)| a * h).collect();
    let (p_det, p_f) = spectral.inverse_real_pair(p_spec, &p_spec_f);

    // Receiver noise current (white, per-sample variance S·fs/2) before the filter.
    let mut noise = vec![0.0; n];
    if cfg.thermal_noise {
        let s = cfg.noise_density * half_fs.sqrt();
        for v in noise.iter_mut() {
            *v += s * rng.normal();
        }
    }
    if cfg.shot_noise {
        for (v, &p) in noise.iter_mut().zip(&p_det) {
            let i = (r * p).max(0.0);
            *v += (2.0 * Q_E * i * half_fs).sqrt() * rng.normal();
        }
    }
    let noise_spec_f: Vec<Complex64> = spectral
        .forward(&noise)
        .iter()
        .zip(&hrx)
        .map(|(a, h)| a * h)
        .collect();
    let rin_spec_f: Vec<Complex64> = if cfg.rin_noise {
        rin_spec.iter().zip(&hrx).map(|(a, h)| a * h).collect()
    } else {
        vec![Complex64::new(0.0, 0.0); n]
    };
    let (rin_f, noise_f) = spectral.inverse_real_pair(&rin_spec_f, &noise_spec_f);

    // Reference optical waveform (with RIN, no receiver noise).
    let reference_optical: Vec<f64> = p_f.iter().zip(&rin_f).map(|(a, b)| a + b).collect();
    let noiseless: Vec<f64> = p_f.iter().map(|p| z * r * p).collect();
    let signal: Vec<f64> = reference_optical
        .iter()
        .zip(&noise_f)
        .map(|(p, i)| z * (r * p + i))
        .collect();

    // Analytic σ per sample.
    let rin_lin = if cfg.rin_noise {
        db_to_lin(vcsel.rin_db_hz)
    } else {
        0.0
    };
    let s_th = if cfg.thermal_noise {
        cfg.noise_density.powi(2)
    } else {
        0.0
    };
    // Exact time-varying variances of the filtered signal-dependent noise (see doc comment).
    let need_shot = cfg.shot_noise;
    let need_rin = cfg.rin_noise && rin_lin > 0.0;
    let (var_shot, var_rin): (Vec<f64>, Vec<f64>) =
        if need_shot || (need_rin && rin_source.is_some()) {
            let htot: Vec<Complex64> = match &rin_source {
                Some(src) => src.channel.iter().zip(&hrx).map(|(a, b)| a * b).collect(),
                None => hrx.clone(),
            };
            let (g_rx, g_tot) = spectral.inverse_real_pair(&hrx, &htot);
            let g_rx2: Vec<f64> = g_rx.iter().map(|g| g * g).collect();
            let g_tot2: Vec<f64> = g_tot.iter().map(|g| g * g).collect();
            let (grx2_spec, gtot2_spec) = spectral.forward_pair(&g_rx2, &g_tot2);
            // Shot: 2q·(fs/2)·(g_rx² ∗ R·P_det). P_det ≥ 0 physically, so FFT(I_det) = R·p_spec.
            let shot_spec: Vec<Complex64> = if need_shot {
                p_spec
                    .iter()
                    .zip(&grx2_spec)
                    .map(|(p, g)| p * g * (2.0 * Q_E * r * half_fs))
                    .collect()
            } else {
                vec![Complex64::new(0.0, 0.0); n]
            };
            let rin_var_spec: Vec<Complex64> = match (&rin_source, need_rin) {
                (Some(src), true) => src
                    .p_tx_sq_spec
                    .iter()
                    .zip(&gtot2_spec)
                    .map(|(p, g)| p * g * (rin_lin * r * r * half_fs))
                    .collect(),
                _ => vec![Complex64::new(0.0, 0.0); n],
            };
            spectral.inverse_real_pair(&shot_spec, &rin_var_spec)
        } else {
            (vec![0.0; n], vec![0.0; n])
        };
    let sigma: Vec<f64> = (0..n)
        .map(|k| {
            let rin = if need_rin && rin_source.is_none() {
                let i = (r * p_f[k]).max(0.0);
                rin_lin * i * i * bn
            } else {
                var_rin[k]
            };
            z * (s_th * bn + var_shot[k].max(0.0) + rin.max(0.0)).sqrt()
        })
        .collect();

    let out = RxOutput {
        signal,
        noiseless,
        sigma,
        reference_optical,
        noise_bandwidth: bn,
        thermal_sigma: z * (s_th * bn).sqrt(),
    };
    (out, p_det)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cfg: &RxConfig, p: f64, seed: u64) -> (RxOutput, Vec<f64>) {
        let n = 1 << 21;
        let fs = 850e9;
        let sp = Spectral::new(n);
        let pd = sp.forward(&vec![p; n]);
        let rin = vec![Complex64::new(0.0, 0.0); n];
        let mut rng = Rng::new(seed);
        let (out, _) = receive(
            cfg,
            &VcselConfig::default(),
            &pd,
            &rin,
            None,
            fs,
            53.125e9,
            &sp,
            &mut rng,
        );
        let noise: Vec<f64> = out
            .signal
            .iter()
            .zip(&out.noiseless)
            .map(|(a, b)| a - b)
            .collect();
        (out, noise)
    }

    fn var(x: &[f64]) -> f64 {
        let m = x.iter().sum::<f64>() / x.len() as f64;
        x.iter().map(|v| (v - m).powi(2)).sum::<f64>() / x.len() as f64
    }

    /// V9: zero optical input — output variance equals i_n²·B_n (·Z²).
    #[test]
    fn v9_thermal_noise_calibration() {
        let cfg = RxConfig {
            shot_noise: false,
            rin_noise: false,
            ..RxConfig::default()
        };
        let (out, noise) = run(&cfg, 0.0, 7);
        let expected = (cfg.transimpedance * cfg.noise_density).powi(2) * out.noise_bandwidth;
        let v = var(&noise);
        assert!(
            ((v - expected) / expected).abs() < 0.01,
            "{v} vs {expected}"
        );
        // Discrete noise bandwidth agrees with the continuous integral of the BT filter.
        let bt = BesselThomson4::new(0.5 * 53.125e9);
        let bc = bt.noise_bandwidth();
        assert!(((out.noise_bandwidth - bc) / bc).abs() < 1e-3);
    }

    /// V10: constant optical input — shot noise 2qI·B_n adds to the thermal floor.
    #[test]
    fn v10_shot_noise() {
        let p = 1e-3;
        let cfg = RxConfig {
            rin_noise: false,
            ..RxConfig::default()
        };
        let (out, noise) = run(&cfg, p, 11);
        let i = cfg.responsivity * p;
        let expected = cfg.transimpedance.powi(2)
            * (cfg.noise_density.powi(2) + 2.0 * Q_E * i)
            * out.noise_bandwidth;
        let v = var(&noise);
        assert!(
            ((v - expected) / expected).abs() < 0.01,
            "{v} vs {expected}"
        );
        assert!(((out.sigma[0].powi(2) - expected) / expected).abs() < 1e-9);
    }
}

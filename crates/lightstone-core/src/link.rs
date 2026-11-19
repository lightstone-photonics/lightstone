//! End-to-end link simulation with stage caching (engine: [`crate::system::LaneEngine`]).
//!
//! Stage graph (each stage is recomputed only if its inputs changed):
//!
//! | Stage    | Inputs                                         |
//! |----------|------------------------------------------------|
//! | TX       | tx, vcsel, rx.rin_noise, seed                  |
//! | Modes    | fiber geometry/profile/grid, wavelength        |
//! | Coupling | modes, launch                                  |
//! | Channel  | modes, coupling, fiber length/loss, σ_λ, detector radius (cheap, always) |
//! | RX       | everything above + rx (always)                 |

use crate::config::LinkConfig;
use crate::coupling::Coupling;
use crate::dsp::freq;
#[cfg(test)]
use crate::dsp::Spectral;
use crate::fiber::FiberChannel;
use crate::modes::ModeSet;
use crate::rx::RxOutput;
use crate::system::LaneEngine;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::sync::Arc;

/// Per-mode summary for reporting.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ModeReport {
    pub l: u32,
    pub m: u32,
    pub group: u32,
    pub n_eff: f64,
    /// Group delay relative to LP01 in the first fiber span \[s/m\].
    pub relative_delay: f64,
    /// Delay relative to LP01 accumulated over all spans of the lane \[s\].
    pub path_delay: f64,
    /// Chromatic delay slope \[s/m²\].
    pub chromatic_slope: f64,
    /// Launched power fraction.
    pub launched_power: f64,
}

/// Scalar link metrics (SI units).
#[derive(Clone, Debug, serde::Serialize)]
pub struct LinkMetrics {
    pub v_number: f64,
    pub n_lp_modes: usize,
    pub n_spatial_modes: usize,
    pub guided_fraction: f64,
    /// Encircled flux of the launched near field at 4.5 µm and 19 µm radius.
    pub encircled_flux_4p5um: f64,
    pub encircled_flux_19um: f64,
    /// Overfilled-launch modal bandwidth·length \[Hz·m\].
    pub ofl_bandwidth_length: Option<f64>,
    /// Modal bandwidth·length for the actual launch (≈ EMB) \[Hz·m\].
    pub emb_length: Option<f64>,
    /// Optical −3 dB bandwidth of the fiber channel at the configured length,
    /// including chromatic dispersion \[Hz\].
    pub channel_bandwidth: Option<f64>,
    pub rms_delay_spread: f64,
    pub mean_delay: f64,
    /// Source-to-detector power transmission (coupling × attenuation × detector).
    pub transmission: f64,
    pub tx_avg_power_dbm: f64,
    pub rx_avg_power_dbm: f64,
    pub tx_oma_outer: f64,
    pub vcsel_bandwidth: f64,
    pub rx_noise_bandwidth: f64,
    pub rx_thermal_sigma: f64,
}

/// Simulation output.
#[derive(Clone, Debug)]
pub struct LinkResult {
    pub sample_rate: f64,
    pub baud: f64,
    pub sps: usize,
    pub symbols: Vec<u8>,
    pub tx_levels: Vec<f64>,
    /// Transmitted optical power (noiseless) \[W\].
    pub tx_power: Vec<f64>,
    /// Detected optical power before the receive filter (noiseless) \[W\].
    pub rx_optical: Vec<f64>,
    pub rx: RxOutput,
    pub modes: Vec<ModeReport>,
    pub metrics: LinkMetrics,
}

/// Cached single-link simulator. Thin wrapper over a one-lane [`LaneEngine`]
/// (connector + fiber), kept for the original `LinkConfig` API.
pub struct Simulator {
    cfg: LinkConfig,
    engine: LaneEngine,
}

impl Simulator {
    pub fn new(cfg: LinkConfig) -> Self {
        let engine = LaneEngine::new(cfg.clone().into());
        Self { cfg, engine }
    }

    pub fn config(&self) -> &LinkConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: LinkConfig) {
        self.engine.set_config(cfg.clone().into());
        self.cfg = cfg;
    }

    /// Mode set for the current fiber (solved on demand, cached).
    pub fn mode_set(&mut self) -> Arc<ModeSet> {
        self.engine
            .mode_set()
            .expect("a LinkConfig always has a fiber")
    }

    /// Launch coupling for the current configuration (cached).
    pub fn coupling(&mut self) -> Arc<Coupling> {
        self.engine
            .coupling()
            .expect("a LinkConfig always has a fiber")
    }

    /// Fiber channel for the current configuration.
    pub fn channel(&mut self) -> FiberChannel {
        self.engine.channel()
    }

    /// Run the full link.
    pub fn run(&mut self) -> LinkResult {
        self.engine.run()
    }
}

/// Evaluate the channel transfer function on the FFT grid using phasor recurrences
/// (exact up to rounding; resynchronized periodically).
pub fn channel_spectrum(ch: &FiberChannel, n: usize, fs: f64) -> Vec<Complex64> {
    let df = fs / n as f64;
    let half = n / 2;
    let mut pos = vec![Complex64::new(0.0, 0.0); half + 1];
    let total: f64 = ch.terms.iter().map(|t| t.weight).sum();
    const RESYNC: usize = 2048;
    for t in ch.terms.iter().filter(|t| t.weight > 1e-12 * total) {
        let a = 2.0 * PI * PI * df * df * t.sigma * t.sigma;
        let rot = Complex64::from_polar(1.0, -2.0 * PI * df * t.delay);
        let mut k = 0;
        while k <= half {
            // Exact values at block start.
            let f = k as f64 * df;
            let mut ph = Complex64::from_polar(1.0, -2.0 * PI * f * t.delay);
            let mut g = (-a * (k * k) as f64).exp();
            let mut gr = (-a * (2 * k + 1) as f64).exp();
            let g2 = (-2.0 * a).exp();
            let end = (k + RESYNC).min(half + 1);
            if g == 0.0 {
                break;
            }
            for p in pos.iter_mut().take(end).skip(k) {
                *p += ph * (t.weight * g);
                ph *= rot;
                g *= gr;
                gr *= g2;
            }
            k = end;
        }
    }
    let mut h = vec![Complex64::new(0.0, 0.0); n];
    for (k, hk) in h.iter_mut().enumerate() {
        let f = freq(k, n, fs);
        let idx = (f / df).round() as i64;
        *hk = if idx >= 0 {
            pos[idx as usize]
        } else {
            pos[(-idx) as usize].conj()
        };
    }
    h
}

/// Convenience: run a configuration once.
pub fn simulate(cfg: &LinkConfig) -> LinkResult {
    Simulator::new(cfg.clone()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fiber::ChannelTerm;

    #[test]
    fn channel_spectrum_matches_direct() {
        let ch = FiberChannel {
            terms: vec![
                ChannelTerm {
                    l: 0,
                    m: 1,
                    weight: 0.6,
                    delay: -3.3e-12,
                    sigma: 2e-12,
                },
                ChannelTerm {
                    l: 1,
                    m: 1,
                    weight: 0.3,
                    delay: 7.1e-12,
                    sigma: 0.0,
                },
            ],
            mean_delay: 0.0,
            rms_delay_spread: 0.0,
            transmission: 0.9,
        };
        let n = 16 * 8191;
        let fs = 850e9;
        let h = channel_spectrum(&ch, n, fs);
        for k in [0usize, 1, 17, 5000, 40_000, 65_000, n - 1, n - 4000] {
            let d = ch.response(freq(k, n, fs));
            assert!((h[k] - d).norm() < 1e-9, "FFT bin {k}: {} vs {}", h[k], d);
        }
    }

    /// V7: two-mode link with known Δτ — detected pulse shows two peaks Δτ·L apart.
    #[test]
    fn v7_two_mode_delay() {
        let n = 16 * 8191;
        let fs = 850e9;
        let dt_per_m = 0.12345e-12;
        let l = 100.0;
        let ch = FiberChannel {
            terms: vec![
                ChannelTerm {
                    l: 0,
                    m: 1,
                    weight: 0.5,
                    delay: -0.5 * dt_per_m * l,
                    sigma: 0.0,
                },
                ChannelTerm {
                    l: 1,
                    m: 1,
                    weight: 0.5,
                    delay: 0.5 * dt_per_m * l,
                    sigma: 0.0,
                },
            ],
            mean_delay: 0.0,
            rms_delay_spread: 0.0,
            transmission: 1.0,
        };
        // Gaussian input pulse (σ = 2.5 ps) centred at sample 1000.
        let ts = 1.0 / fs;
        let sig = 2.5e-12;
        let t0 = 1000.0 * ts;
        let x: Vec<f64> = (0..n)
            .map(|i| (-((i as f64 * ts - t0).powi(2)) / (2.0 * sig * sig)).exp())
            .collect();
        let sp = Spectral::new(n);
        let h = channel_spectrum(&ch, n, fs);
        let xs = sp.forward(&x);
        let y = sp.inverse_real(xs.iter().zip(&h).map(|(a, b)| a * b).collect());
        // Find the two local maxima and refine with a log-parabola (exact for Gaussians).
        let mut peaks = Vec::new();
        for i in 900..1100 {
            if y[i] > y[i - 1] && y[i] >= y[i + 1] && y[i] > 0.1 {
                let (a, b, c) = (y[i - 1].ln(), y[i].ln(), y[i + 1].ln());
                let off = 0.5 * (a - c) / (a - 2.0 * b + c);
                peaks.push((i as f64 + off) * ts);
            }
        }
        assert_eq!(peaks.len(), 2, "{peaks:?}");
        let sep = peaks[1] - peaks[0];
        let exact = dt_per_m * l;
        assert!((sep - exact).abs() < 0.1e-12, "sep {sep:e} vs {exact:e}");
        // Peaks overlap slightly; require 0.1 ps (spec), and in practice much better.
    }

    #[test]
    fn default_link_runs() {
        let mut sim = Simulator::new(LinkConfig::default());
        let r = sim.run();
        assert_eq!(r.rx.signal.len(), 8191 * 16);
        let m = &r.metrics;
        assert!(m.guided_fraction > 0.9 && m.guided_fraction <= 1.0);
        assert!(m.transmission < 1.0);
        assert!(r.rx.signal.iter().all(|v| v.is_finite()));
        // V8: the default fiber is the minimum-spec OM4 preset: EMB with the default VCSEL
        // launch at the OM4 minimum of 4700 MHz·km (within 1 %). The OFL bandwidth is lower
        // (≈ 1450 MHz·km): the presets are calibrated to the laser-launch bandwidth that IEEE
        // link budgets use, not to the overfilled launch.
        let emb = m.emb_length.unwrap();
        assert!((4700e9..4747e9).contains(&emb), "EMB {emb:e}");
        assert!(m.ofl_bandwidth_length.unwrap() < emb);
        // Second run with only length changed must reuse caches and still work.
        let mut c = sim.config().clone();
        c.fiber.length = 50.0;
        sim.set_config(c);
        let r2 = sim.run();
        assert!(r2.metrics.rms_delay_spread < r.metrics.rms_delay_spread);
    }
}

//! Incoherent (power-based) multimode propagation.
//!
//! `eq:fiber-power-propagation` — The VCSEL coherence time (~ps) is shorter than the
//! differential mode delay, so intermodal interference averages out at the detector and the
//! fiber acts as a linear filter on optical power:
//!
//! P_det(t) = Σ_m w_m · (h_m ∗ P_tx)(t − (τ_m − τ_ref)L),
//! w_m = P_m,launch · η_m · T_att · T_extra,
//! H_m(f) = exp(−j2πf(τ_m − τ_ref)L) · exp(−2π²f²σ_m²),  σ_m = |dτ_m/dλ| · L · σ_λ.
//!
//! The Gaussian factor is the chromatic broadening for a Gaussian source spectrum of RMS
//! width σ_λ (modal–chromatic interaction is captured because dτ/dλ is per mode).
//! τ_ref is the power-weighted mean delay, reported separately as `mean_delay`.

use crate::config::FiberConfig;
use crate::dsp;
use crate::modes::{power_within, ModeSet};
use crate::units::{attenuation_transmission, db_to_lin};
use num_complex::Complex64;
use std::f64::consts::PI;

/// Per-mode channel term.
#[derive(Clone, Debug)]
pub struct ChannelTerm {
    pub l: u32,
    pub m: u32,
    /// Power weight at the detector (launch × detector fraction × losses).
    pub weight: f64,
    /// Delay relative to τ_ref \[s\].
    pub delay: f64,
    /// RMS chromatic broadening \[s\].
    pub sigma: f64,
}

/// Linear power-domain channel of a fiber link.
#[derive(Clone, Debug)]
pub struct FiberChannel {
    pub terms: Vec<ChannelTerm>,
    /// Power-weighted mean absolute delay τ_ref·L \[s\].
    pub mean_delay: f64,
    /// RMS spread of modal delays (power-weighted) \[s\].
    pub rms_delay_spread: f64,
    /// Σ weights = total power transmission from source to detector.
    pub transmission: f64,
}

impl FiberChannel {
    pub fn response(&self, f: f64) -> Complex64 {
        let mut h = Complex64::new(0.0, 0.0);
        for t in &self.terms {
            let g = (-2.0 * PI * PI * f * f * t.sigma * t.sigma).exp();
            h += Complex64::from_polar(t.weight * g, -2.0 * PI * f * t.delay);
        }
        h
    }

    /// Optical (−3 dB optical ⇔ |H|/|H(0)| = ½) bandwidth of the whole channel \[Hz\].
    pub fn bandwidth(&self) -> Option<f64> {
        let max_spread = self
            .terms
            .iter()
            .map(|t| t.delay.abs() + 3.0 * t.sigma)
            .fold(0.0, f64::max);
        let fmax = if max_spread > 0.0 {
            50.0 / max_spread
        } else {
            1e15
        };
        dsp::bandwidth(|f| self.response(f), 0.5, fmax)
    }
}

/// Inputs that select which modes reach the detector.
pub struct ChannelInputs<'a> {
    pub modes: &'a ModeSet,
    pub launch: &'a [f64],
    pub fiber: &'a FiberConfig,
    pub spectral_width_rms: f64,
    pub detector_radius: Option<f64>,
}

/// Build the power-domain channel for fiber length `fiber.length`.
pub fn channel(inp: &ChannelInputs) -> FiberChannel {
    let l = inp.fiber.length;
    let t_loss = attenuation_transmission(inp.fiber.attenuation_db_per_km, l)
        * db_to_lin(-inp.fiber.extra_loss_db);
    let mut raw = Vec::with_capacity(inp.modes.modes.len());
    for (md, &p) in inp.modes.modes.iter().zip(inp.launch) {
        let eta = inp
            .detector_radius
            .map_or(1.0, |rd| power_within(inp.modes, md, rd));
        raw.push((md, p * eta * t_loss));
    }
    let total: f64 = raw.iter().map(|(_, w)| w).sum();
    let mean_tau = if total > 0.0 {
        raw.iter().map(|(m, w)| m.group_delay * w).sum::<f64>() / total
    } else {
        0.0
    };
    let var = if total > 0.0 {
        raw.iter()
            .map(|(m, w)| w * ((m.group_delay - mean_tau) * l).powi(2))
            .sum::<f64>()
            / total
    } else {
        0.0
    };
    let terms = raw
        .iter()
        .filter(|(_, w)| *w > 0.0)
        .map(|(m, w)| ChannelTerm {
            l: m.l,
            m: m.m,
            weight: *w,
            delay: (m.group_delay - mean_tau) * l,
            sigma: m.chromatic_slope.abs() * l * inp.spectral_width_rms,
        })
        .collect();
    FiberChannel {
        terms,
        mean_delay: mean_tau * l,
        rms_delay_spread: var.sqrt(),
        transmission: total,
    }
}

/// Modal bandwidth–length product \[Hz·m\] for the given mode power weights
/// (chromatic dispersion excluded). `eq:fiber-modal-bandwidth`
///
/// With launch weights from the actual source this approximates the effective modal
/// bandwidth (EMB); with [`overfilled_weights`] it gives the overfilled-launch (OFL)
/// bandwidth.
pub fn modal_bandwidth_length(modes: &ModeSet, weights: &[f64]) -> Option<f64> {
    let ref_len = 1000.0;
    let fiber = FiberConfig {
        length: ref_len,
        attenuation_db_per_km: 0.0,
        extra_loss_db: 0.0,
        ..FiberConfig::default()
    };
    let ch = channel(&ChannelInputs {
        modes,
        launch: weights,
        fiber: &fiber,
        spectral_width_rms: 0.0,
        detector_radius: None,
    });
    ch.bandwidth().map(|b| b * ref_len)
}

/// Overfilled launch: equal power in every guided spatial mode (both orientations).
pub fn overfilled_weights(modes: &ModeSet) -> Vec<f64> {
    let n = modes.spatial_mode_count() as f64;
    modes
        .modes
        .iter()
        .map(|m| m.orientations() as f64 / n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two equal impulses Δτ apart: |H| = |cos(πfΔτ)| = ½ at f = 1/(3Δτ).
    #[test]
    fn two_impulse_bandwidth() {
        let dt = 50e-12;
        let ch = FiberChannel {
            terms: vec![
                ChannelTerm {
                    l: 0,
                    m: 1,
                    weight: 0.5,
                    delay: -dt / 2.0,
                    sigma: 0.0,
                },
                ChannelTerm {
                    l: 1,
                    m: 1,
                    weight: 0.5,
                    delay: dt / 2.0,
                    sigma: 0.0,
                },
            ],
            mean_delay: 0.0,
            rms_delay_spread: dt / 2.0,
            transmission: 1.0,
        };
        let b = ch.bandwidth().unwrap();
        let exact = 1.0 / (3.0 * dt);
        assert!(((b - exact) / exact).abs() < 1e-9, "{b} vs {exact}");
    }
}

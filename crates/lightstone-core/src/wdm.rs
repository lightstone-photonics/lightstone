//! Wavelength-selective components (`docs/model/bandpass-filter.md`, `wdm-mux.md`, `wdm-demux.md`): optical bandpass filter, WDM
//! multiplexer and demultiplexer.
//!
//! `eq:wdm-passband` — Power transmission of a passband with centre λ_c, −3 dB full width w,
//! super-Gaussian order n, insertion loss IL and isolation ISO:
//! T(λ) = 10^(−IL/10) · max(exp(−ln2·|2(λ − λ_c)/w|^(2n)), 10^(−ISO/10)).
//! n = 1 is a Gaussian, larger n a flatter top with steeper edges. The isolation is a floor
//! relative to the passband peak: the leakage of out-of-band channels (crosstalk).
//!
//! Each source contribution is weighted by T at its own centre wavelength. The filter does
//! not reshape a source's spectrum; a warning appears when the transmission changes by more
//! than 1 dB within ±2 RMS spectral widths. All three components are linear, so σ(t) and the
//! semi-analytic SER stay exact.

use crate::component::*;
use crate::components::{forward_of, one_optical, optical, parse};
use crate::signal::{OpticalSignal, OpticalWave};
use crate::units::db_to_lin;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// SWDM4 channel plan: 850, 880, 910, 940 nm (100G-SWDM4 MSA, 30 nm spacing).
pub const SWDM4_CHANNELS: [f64; 4] = [850e-9, 880e-9, 910e-9, 940e-9];

/// Transmission change within ±2σ_λ of a source above which a warning is given \[dB\].
const SPECTRAL_WARN_DB: f64 = 1.0;
/// A source more than this far below a port's peak is reported as outside the passband \[dB\].
const OUT_OF_BAND_DB: f64 = 3.0;

/// One passband (`eq:wdm-passband`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Passband {
    pub center: f64,
    pub width: f64,
    pub order: u32,
    pub insertion_loss_db: f64,
    pub isolation_db: Option<f64>,
}

impl Passband {
    /// Power transmission at wavelength `l` \[m\].
    pub fn transmission(&self, l: f64) -> f64 {
        let x = (2.0 * (l - self.center) / self.width).abs();
        let shape = (-std::f64::consts::LN_2 * x.powi(2 * self.order as i32)).exp();
        let floor = self.isolation_db.map_or(0.0, |i| db_to_lin(-i));
        db_to_lin(-self.insertion_loss_db) * shape.max(floor)
    }

    fn check(&self) -> Result<(), String> {
        if !(self.center > 0.0 && self.width > 0.0) {
            return Err("Passband centre and width must be > 0.".into());
        }
        if !(1..=10).contains(&self.order) {
            return Err("Passband order must be 1 … 10 (1 = Gaussian).".into());
        }
        if self.insertion_loss_db < 0.0 {
            return Err("Insertion loss must be ≥ 0 dB.".into());
        }
        if self.isolation_db.is_some_and(|i| i <= 0.0) {
            return Err("Isolation must be > 0 dB (or null for none).".into());
        }
        Ok(())
    }
}

/// Weight every contribution of `s` by the passband at its wavelength; returns the filtered
/// signal and one report row per contribution. Warnings go to `warnings`.
fn filter(
    s: &OpticalSignal,
    pb: &Passband,
    port: &str,
    warnings: &mut Vec<String>,
) -> (OpticalSignal, Vec<serde_json::Value>) {
    let mut out = s.clone();
    let mut rows = Vec::new();
    for c in out.contributions.iter_mut() {
        let (l, sw) = (c.source.wavelength, c.source.spectral_width_rms);
        let t = pb.transmission(l);
        let t_db = 10.0 * t.max(1e-300).log10();
        let peak = db_to_lin(-pb.insertion_loss_db);
        if sw > 0.0 && t > 0.0 {
            let worst = [l - 2.0 * sw, l + 2.0 * sw]
                .iter()
                .map(|&x| (10.0 * (pb.transmission(x) / t).log10()).abs())
                .fold(0.0, f64::max);
            if worst > SPECTRAL_WARN_DB {
                warnings.push(format!(
                    "{port}: the transmission changes by {worst:.1} dB within ±2 RMS widths of \
                     source '{}' ({:.1} nm ± {:.2} nm). The filter weights the source at its \
                     centre wavelength and does not reshape its spectrum, so the result is \
                     approximate.",
                    c.source.node,
                    l * 1e9,
                    sw * 1e9
                ));
            }
        }
        rows.push(serde_json::json!({
            "source": c.source.node, "wavelength": l,
            "transmission": t, "transmission_db": t_db,
            "in_band": t >= peak * db_to_lin(-OUT_OF_BAND_DB),
        }));
        if t != 1.0 {
            c.scale(t);
        }
    }
    (out, rows)
}

fn data_report(kind: &str, value: serde_json::Value) -> Option<Report> {
    Some(Report::Data {
        kind: kind.into(),
        value: Arc::new(value),
    })
}

// ---------------------------------------------------------------------------------------------
// Bandpass filter
// ---------------------------------------------------------------------------------------------

/// Parameters of `bandpass_filter`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BandpassParams {
    /// Centre wavelength \[m\].
    pub center: f64,
    /// −3 dB full width \[m\].
    pub width: f64,
    /// Super-Gaussian order (1 = Gaussian).
    pub order: u32,
    pub insertion_loss_db: f64,
    /// Out-of-band suppression relative to the peak \[dB\]; `None` = no floor.
    pub isolation_db: Option<f64>,
}

impl Default for BandpassParams {
    fn default() -> Self {
        Self {
            center: 850e-9,
            width: 22e-9,
            order: 3,
            insertion_loss_db: 0.5,
            isolation_db: Some(30.0),
        }
    }
}

impl BandpassParams {
    fn passband(&self) -> Passband {
        Passband {
            center: self.center,
            width: self.width,
            order: self.order,
            insertion_loss_db: self.insertion_loss_db,
            isolation_db: self.isolation_db,
        }
    }
}

pub(crate) struct Bandpass(Passband);

impl Bandpass {
    pub(crate) fn new(p: &serde_json::Value) -> Result<Self, String> {
        let pb = parse::<BandpassParams>(p)?.passband();
        pb.check()?;
        Ok(Self(pb))
    }
}

impl Component for Bandpass {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let mut warnings = Vec::new();
        let (s, rows) = filter(forward_of(&inputs[0])?, &self.0, "out", &mut warnings);
        let mut out = one_optical(s);
        out.report = data_report(
            "wdm",
            serde_json::json!({ "ports": [{
            "port": "out", "center": self.0.center, "sources": rows }] }),
        );
        out.warnings = warnings;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// WDM multiplexer and demultiplexer
// ---------------------------------------------------------------------------------------------

/// Parameters of `wdm_mux` and `wdm_demux`: one passband per channel, sharing shape and loss.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WdmParams {
    /// Channel centre wavelengths \[m\], one port each.
    pub channels: Vec<f64>,
    /// −3 dB full width of every passband \[m\].
    pub width: f64,
    /// Super-Gaussian order (1 = Gaussian).
    pub order: u32,
    pub insertion_loss_db: f64,
    /// Out-of-band suppression relative to the peak \[dB\] (crosstalk); `None` = no floor.
    pub isolation_db: Option<f64>,
}

impl Default for WdmParams {
    fn default() -> Self {
        Self {
            channels: SWDM4_CHANNELS.to_vec(),
            width: 22e-9,
            order: 3,
            insertion_loss_db: 1.0,
            isolation_db: Some(30.0),
        }
    }
}

impl WdmParams {
    fn passbands(&self) -> Result<Vec<Passband>, String> {
        if self.channels.is_empty() {
            return Err("A WDM component needs at least one channel.".into());
        }
        self.channels
            .iter()
            .map(|&c| {
                let pb = Passband {
                    center: c,
                    width: self.width,
                    order: self.order,
                    insertion_loss_db: self.insertion_loss_db,
                    isolation_db: self.isolation_db,
                };
                pb.check().map(|_| pb)
            })
            .collect()
    }
}

/// `wdm_mux`: N inputs (`in0` …), each through its channel's passband, combined
/// incoherently on `out`.
pub(crate) struct WdmMux(Vec<Passband>);

impl WdmMux {
    pub(crate) fn new(p: &serde_json::Value) -> Result<Self, String> {
        Ok(Self(parse::<WdmParams>(p)?.passbands()?))
    }
}

impl Component for WdmMux {
    fn inputs(&self) -> Vec<PortSpec> {
        (0..self.0.len())
            .map(|i| PortSpec::new(&format!("in{i}"), optical()))
            .collect()
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("out", optical())]
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let first = forward_of(&inputs[0])?;
        let mut out = OpticalSignal {
            level: first.level,
            resolution: first.resolution,
            contributions: vec![],
        };
        let mut warnings = Vec::new();
        let mut ports = Vec::new();
        for (i, (sig, pb)) in inputs.iter().zip(&self.0).enumerate() {
            let port = format!("in{i}");
            let (s, rows) = filter(forward_of(sig)?, pb, &port, &mut warnings);
            for r in &rows {
                if r["in_band"] == false {
                    warnings.push(format!(
                        "{port}: source '{}' at {:.1} nm is outside the port's passband \
                         ({:.1} nm ± {:.1} nm): {:.1} dB.",
                        r["source"].as_str().unwrap_or("?"),
                        r["wavelength"].as_f64().unwrap_or(0.0) * 1e9,
                        pb.center * 1e9,
                        pb.width * 0.5e9,
                        r["transmission_db"].as_f64().unwrap_or(0.0)
                    ));
                }
            }
            ports.push(serde_json::json!({ "port": port, "center": pb.center, "sources": rows }));
            out.contributions.extend(s.contributions);
        }
        let mut o = one_optical(out);
        o.report = data_report("wdm", serde_json::json!({ "ports": ports }));
        o.warnings = warnings;
        Ok(o)
    }
}

/// `wdm_demux`: one input, N outputs (`out0` …); every output carries all contributions,
/// each weighted by that channel's passband at its wavelength (leakage = crosstalk).
pub(crate) struct WdmDemux(Vec<Passband>);

impl WdmDemux {
    pub(crate) fn new(p: &serde_json::Value) -> Result<Self, String> {
        Ok(Self(parse::<WdmParams>(p)?.passbands()?))
    }
}

impl Component for WdmDemux {
    fn inputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::new("in", optical())]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        (0..self.0.len())
            .map(|i| PortSpec::new(&format!("out{i}"), optical()))
            .collect()
    }
    fn linearity(&self) -> Linearity {
        Linearity::Linear
    }
    fn process(&self, _: &mut ProcessCtx, inputs: &[Signal]) -> Result<Output, String> {
        let s = forward_of(&inputs[0])?;
        let mut warnings = Vec::new();
        let mut ports = Vec::new();
        let mut signals = Vec::new();
        for (i, pb) in self.0.iter().enumerate() {
            let port = format!("out{i}");
            let (f, rows) = filter(s, pb, &port, &mut warnings);
            ports.push(serde_json::json!({ "port": port, "center": pb.center, "sources": rows }));
            signals.push(Signal::Optical(Arc::new(OpticalWave::forward(f))));
        }
        let mut o = Output::signals(signals);
        o.report = data_report("wdm", serde_json::json!({ "ports": ports }));
        o.warnings = warnings;
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(iso: Option<f64>) -> Passband {
        Passband {
            center: 880e-9,
            width: 22e-9,
            order: 3,
            insertion_loss_db: 1.0,
            isolation_db: iso,
        }
    }

    /// Peak = insertion loss, −3 dB at ±w/2, the floor at the isolation.
    #[test]
    fn passband_shape() {
        let p = pb(Some(30.0));
        let db = |l: f64| 10.0 * p.transmission(l).log10();
        assert!((db(880e-9) + 1.0).abs() < 1e-12);
        assert!((db(891e-9) + 1.0 + 10.0 * 2f64.log10()).abs() < 1e-9);
        assert!((db(869e-9) - db(891e-9)).abs() < 1e-9);
        assert!((db(850e-9) + 31.0).abs() < 1e-9, "{}", db(850e-9));
        assert!((db(910e-9) + 31.0).abs() < 1e-9);
        let open = pb(None);
        assert!(open.transmission(850e-9) < 1e-50);
        // Order 1 is a Gaussian in wavelength.
        let g = Passband {
            order: 1,
            ..pb(None)
        };
        let x = 5e-9;
        let e = (-std::f64::consts::LN_2 * (2.0 * x / 22e-9f64).powi(2)).exp() * db_to_lin(-1.0);
        assert!((g.transmission(880e-9 + x) / e - 1.0).abs() < 1e-9);
    }
}

//! Physical constants and unit conversions. Internal unit system is SI.

/// Speed of light in vacuum \[m/s\].
pub const C0: f64 = 299_792_458.0;
/// Elementary charge \[C\].
pub const Q_E: f64 = 1.602_176_634e-19;
/// Boltzmann constant \[J/K\].
pub const K_B: f64 = 1.380_649e-23;

/// dBm → W.
pub fn dbm_to_w(dbm: f64) -> f64 {
    1e-3 * 10f64.powf(dbm / 10.0)
}

/// W → dBm.
pub fn w_to_dbm(w: f64) -> f64 {
    10.0 * (w / 1e-3).log10()
}

/// dB → linear power ratio.
pub fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 10.0)
}

/// Linear power ratio → dB.
pub fn lin_to_db(lin: f64) -> f64 {
    10.0 * lin.log10()
}

/// Power attenuation in dB/km → power transmission over length `l` \[m\].
/// `eq:fiber-attenuation`
pub fn attenuation_transmission(db_per_km: f64, l: f64) -> f64 {
    10f64.powf(-db_per_km * l / 1000.0 / 10.0)
}

/// Fiber chromatic dispersion parameter in ps/(nm·km) → s/m² .
pub fn ps_nm_km_to_si(d: f64) -> f64 {
    d * 1e-12 / (1e-9 * 1e3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbm_roundtrip() {
        for v in [-20.0, -3.0, 0.0, 5.5] {
            assert!((w_to_dbm(dbm_to_w(v)) - v).abs() < 1e-12);
        }
        assert!((dbm_to_w(0.0) - 1e-3).abs() < 1e-18);
    }

    #[test]
    fn attenuation() {
        // 3 dB/km over 1 km halves the power (to ~0.5012).
        let t = attenuation_transmission(3.0, 1000.0);
        assert!((t - 10f64.powf(-0.3)).abs() < 1e-15);
    }
}

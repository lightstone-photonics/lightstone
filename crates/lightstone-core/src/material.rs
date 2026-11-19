//! Material dispersion of fused silica and germania-doped silica.
//!
//! `eq:material-sellmeier` — Malitson (1965) three-term Sellmeier equation for fused silica,
//! valid 0.21–3.71 µm:
//! n² − 1 = Σ Bᵢ λ² / (λ² − Cᵢ²), λ in µm.
//!
//! `eq:material-germania` — GeO₂–SiO₂ glass with GeO₂ mole fraction x (Fleming 1984): the
//! Sellmeier coefficients are interpolated linearly between fused silica (Malitson) and
//! GeO₂ glass, Bᵢ(x) = Bᵢ,SiO₂ + x (Bᵢ,GeO₂ − Bᵢ,SiO₂), likewise Cᵢ(x).

const B: [f64; 3] = [0.696_166_3, 0.407_942_6, 0.897_479_4];
const C: [f64; 3] = [0.068_404_3, 0.116_241_4, 9.896_161];

/// Sellmeier coefficients of GeO₂ glass (Fleming 1984), C in µm.
const B_GE: [f64; 3] = [0.806_866_42, 0.718_158_48, 0.854_168_31];
const C_GE: [f64; 3] = [0.068_972_606, 0.153_966_05, 11.841_931];

/// Wavelength at which a fiber's numerical aperture is specified for germania-doped cores
/// \[m\] (the NA fixes the GeO₂ fraction at the core centre).
pub const NA_REFERENCE_WAVELENGTH: f64 = 850e-9;

/// Refractive index of fused silica at vacuum wavelength `lambda` \[m\].
pub fn silica_index(lambda: f64) -> f64 {
    germania_silica_index(lambda, 0.0)
}

/// Refractive index of GeO₂–SiO₂ glass with GeO₂ mole fraction `x` (0 … 1) at vacuum
/// wavelength `lambda` \[m\] (`eq:material-germania`).
pub fn germania_silica_index(lambda: f64, x: f64) -> f64 {
    let l2 = (lambda * 1e6).powi(2);
    let mut s = 1.0;
    for i in 0..3 {
        let b = B[i] + x * (B_GE[i] - B[i]);
        let c = C[i] + x * (C_GE[i] - C[i]);
        s += b * l2 / (l2 - c * c);
    }
    s.sqrt()
}

/// GeO₂ mole fraction of a core whose numerical aperture against a fused-silica cladding is
/// `na` at [`NA_REFERENCE_WAVELENGTH`]: √(n_core² − n_clad²) = NA (bisection).
pub fn germania_fraction_for_na(na: f64) -> f64 {
    let lam = NA_REFERENCE_WAVELENGTH;
    let n2 = silica_index(lam);
    let na_of = |x: f64| {
        (germania_silica_index(lam, x).powi(2) - n2 * n2)
            .max(0.0)
            .sqrt()
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if na_of(mid) < na {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Group index N = n − λ dn/dλ of fused silica.
pub fn silica_group_index(lambda: f64) -> f64 {
    let d = 1e-10;
    let dn = (silica_index(lambda + d) - silica_index(lambda - d)) / (2.0 * d);
    silica_index(lambda) - lambda * dn
}

/// Material dispersion parameter D = −(λ/c) d²n/dλ² \[s/m²\].
pub fn silica_dispersion(lambda: f64) -> f64 {
    let d = 1e-9;
    let n2 = (silica_index(lambda + d) - 2.0 * silica_index(lambda) + silica_index(lambda - d))
        / (d * d);
    -lambda / crate::units::C0 * n2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        // Malitson: n(0.85 µm) ≈ 1.4525, n(1.55 µm) ≈ 1.4440.
        assert!((silica_index(850e-9) - 1.4525).abs() < 2e-4);
        assert!((silica_index(1550e-9) - 1.4440).abs() < 2e-4);
        // D(850 nm) ≈ −85 ps/(nm·km); zero dispersion near 1.27 µm.
        let d850 = silica_dispersion(850e-9) / crate::units::ps_nm_km_to_si(1.0);
        assert!((d850 + 85.0).abs() < 5.0, "D850 = {d850}");
        let d1270 = silica_dispersion(1270e-9) / crate::units::ps_nm_km_to_si(1.0);
        assert!(d1270.abs() < 3.0, "D1270 = {d1270}");
    }

    /// GeO₂ glass and doped silica: Fleming's n(GeO₂, 0.85 µm) ≈ 1.597; NA 0.2 at 850 nm needs
    /// ≈ 9 mol% GeO₂; the doped core has more dispersion and a longer zero-dispersion
    /// wavelength than silica.
    #[test]
    fn germania_doped_silica() {
        assert!((germania_silica_index(850e-9, 1.0) - 1.597).abs() < 1e-3);
        assert_eq!(germania_silica_index(850e-9, 0.0), silica_index(850e-9));
        let x = germania_fraction_for_na(0.2);
        assert!((x - 0.0917).abs() < 2e-3, "x = {x}");
        let na = (germania_silica_index(850e-9, x).powi(2) - silica_index(850e-9).powi(2)).sqrt();
        assert!((na - 0.2).abs() < 1e-12);
        let d = |lam: f64, x: f64| {
            let h = 1e-9;
            let n2 = (germania_silica_index(lam + h, x) - 2.0 * germania_silica_index(lam, x)
                + germania_silica_index(lam - h, x))
                / (h * h);
            -lam / crate::units::C0 * n2 / crate::units::ps_nm_km_to_si(1.0)
        };
        assert!(
            d(844e-9, x) < d(844e-9, 0.0) - 8.0,
            "{} vs {}",
            d(844e-9, x),
            d(844e-9, 0.0)
        );
        assert!(
            d(1300e-9, x) < 0.0 && d(1300e-9, 0.0) > 0.0,
            "zero dispersion moves above 1.3 µm"
        );
    }
}

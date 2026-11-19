//! Launch coupling into fiber modes.
//!
//! `eq:coupling-overlap` — c_m = ∬ E_in(x, y) ψ_m(x, y) dx dy (modes are real),
//! with the unit-power Gaussian input
//! E_in = √(2/(πw₀²)) · exp(−((x−Δx)² + y²)/w₀²) · exp(j k₀ sinθ x).
//!
//! Quadrature: polar grid of the mode solver (midpoint rule in r) and uniform M-point
//! rule in φ (spectrally accurate for periodic integrands). For each radius, the azimuthal
//! Fourier coefficients of E_in are computed with an FFT; the radial integral is a dot
//! product with F_lm(r).
//!
//! `eq:coupling-power` — P_lm = |c_lm,cos|² + |c_lm,sin|²; unguided fraction 1 − Σ P_lm.

use crate::config::LaunchConfig;
use crate::modes::ModeSet;
use num_complex::Complex64;
use rustfft::FftPlanner;
use std::f64::consts::PI;

/// Default number of azimuthal quadrature points. Exact (alias-free) for azimuthal content
/// up to order M − l_max; sufficient for offsets/tilts within the core (see coupling tests).
pub const DEFAULT_ANGULAR_POINTS: usize = 256;

/// Coupling result for one LP_lm mode.
#[derive(Clone, Debug)]
pub struct ModeCoupling {
    pub l: u32,
    pub m: u32,
    /// Coefficient onto F(r)cos(lφ).
    pub c_cos: Complex64,
    /// Coefficient onto F(r)sin(lφ) (zero for l = 0).
    pub c_sin: Complex64,
    /// Launched power fraction (both orientations).
    pub power: f64,
}

#[derive(Clone, Debug)]
pub struct Coupling {
    /// Same order as `ModeSet::modes`.
    pub modes: Vec<ModeCoupling>,
    /// Σ P_lm (fraction of input power that is guided).
    pub guided_fraction: f64,
    /// Fraction of the input power inside the computational domain.
    pub domain_fraction: f64,
}

/// Generic coupling of an arbitrary input field E(r, φ) sampled on the solver radial grid.
/// `field(r, phi)` must be evaluated in fiber-facet coordinates; `input_power` is the exact
/// total power of the input field (if known).
///
/// Overlaps are normalized by the input power integrated with the same quadrature, so that
/// Σ|c|² ≤ 1 holds exactly (discrete Bessel inequality; the modes are orthonormal in the
/// discrete inner product) and the O(h²) quadrature error of the input norm cancels. Input
/// power outside the computational domain (if `input_power` is given) is counted as unguided.
pub fn couple_field<F: Fn(f64, f64) -> Complex64>(
    set: &ModeSet,
    field: F,
    angular_points: usize,
    input_power: Option<f64>,
) -> Coupling {
    couple_fields(
        set,
        1,
        |r, phi, out| out[0] = field(r, phi),
        |_| true,
        angular_points,
        &[input_power],
    )
    .pop()
    .unwrap()
}

/// Batched version of [`couple_field`] for `n_fields` input fields evaluated together.
/// `eval(r, φ, out)` writes all field values at one point; `ring_active(r)` may return
/// `false` for radii where every field is negligible (the ring is then skipped).
pub fn couple_fields<E, A>(
    set: &ModeSet,
    n_fields: usize,
    eval: E,
    ring_active: A,
    angular_points: usize,
    input_powers: &[Option<f64>],
) -> Vec<Coupling>
where
    E: Fn(f64, f64, &mut [Complex64]),
    A: Fn(f64) -> bool,
{
    let m_ang = angular_points;
    let l_max = set.modes.iter().map(|m| m.l).max().unwrap_or(0) as usize;
    assert!(
        2 * l_max < m_ang,
        "angular quadrature too coarse for l_max = {l_max}"
    );
    assert_eq!(input_powers.len(), n_fields);
    let fft = FftPlanner::new().plan_fft_forward(m_ang);
    let dphi = 2.0 * PI / m_ang as f64;
    let nr = set.r.len();
    let zero = Complex64::new(0.0, 0.0);
    // Per field: a[l][i] = A_l(r_i) = ∫ E(r_i, φ) e^{-j l φ} dφ and neg[l][i] = A_{-l}(r_i)
    // (FFT bin M − l). For l > 0: ∫E cos(lφ) = (A_l + A_-l)/2, ∫E sin(lφ) = j(A_l − A_-l)/2.
    let mut a = vec![vec![vec![zero; nr]; l_max + 1]; n_fields];
    let mut neg = vec![vec![vec![zero; nr]; l_max + 1]; n_fields];
    let mut bufs = vec![vec![zero; m_ang]; n_fields];
    let mut vals = vec![zero; n_fields];
    let mut p_disc = vec![0.0; n_fields];
    for (i, &r) in set.r.iter().enumerate() {
        if !ring_active(r) {
            continue;
        }
        let mut ring = vec![0.0; n_fields];
        #[allow(clippy::needless_range_loop)] // j indexes every field buffer
        for j in 0..m_ang {
            eval(r, j as f64 * dphi, &mut vals);
            for f in 0..n_fields {
                bufs[f][j] = vals[f];
                ring[f] += vals[f].norm_sqr();
            }
        }
        for f in 0..n_fields {
            p_disc[f] += ring[f] * dphi * r * set.dr;
            fft.process(&mut bufs[f]);
            for l in 0..=l_max {
                a[f][l][i] = bufs[f][l] * dphi;
                neg[f][l][i] = bufs[f][(m_ang - l) % m_ang] * dphi;
            }
        }
    }

    (0..n_fields)
        .map(|f| {
            // Fraction of the input inside the domain (≤ 1; quadrature excess clamps to 1).
            let domain_fraction = input_powers[f].map_or(1.0, |p| (p_disc[f] / p).min(1.0));
            let amp_scale = if p_disc[f] > 0.0 {
                (domain_fraction / p_disc[f]).sqrt()
            } else {
                0.0
            };
            let mut out = Vec::with_capacity(set.modes.len());
            let mut total = 0.0;
            for md in &set.modes {
                let l = md.l as usize;
                let mut cc = zero;
                let mut cs = zero;
                for i in 0..nr {
                    let w = md.field[i] * set.r[i] * set.dr;
                    if l == 0 {
                        cc += a[f][0][i] * w;
                    } else {
                        cc += (a[f][l][i] + neg[f][l][i]) * 0.5 * w;
                        cs += Complex64::new(0.0, 1.0) * (a[f][l][i] - neg[f][l][i]) * 0.5 * w;
                    }
                }
                cc *= amp_scale;
                cs *= amp_scale;
                let power = cc.norm_sqr() + cs.norm_sqr();
                total += power;
                out.push(ModeCoupling {
                    l: md.l,
                    m: md.m,
                    c_cos: cc,
                    c_sin: cs,
                    power,
                });
            }
            Coupling {
                modes: out,
                guided_fraction: total,
                domain_fraction,
            }
        })
        .collect()
}

/// Generalized Laguerre polynomial L_p^l(x).
fn laguerre(p: u32, l: u32, x: f64) -> f64 {
    let l = l as f64;
    if p == 0 {
        return 1.0;
    }
    let (mut a, mut b) = (1.0, 1.0 + l - x);
    for k in 1..p {
        let k = k as f64;
        let c = ((2.0 * k + 1.0 + l - x) * b - (k + l) * a) / (k + 1.0);
        a = b;
        b = c;
    }
    b
}

fn factorial(n: u32) -> f64 {
    (1..=n).map(|k| k as f64).product()
}

/// One real Laguerre–Gauss VCSEL transverse mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LgMode {
    pub p: u32,
    pub l: u32,
    /// `false` = cos(lθ), `true` = sin(lθ).
    pub sin: bool,
}

/// All real LG modes of the lowest `groups` mode groups (2p + l = q − 1, q = 1..=groups).
pub fn lg_modes(groups: u32) -> Vec<LgMode> {
    assert!(groups <= 30, "at most 30 VCSEL mode groups supported");
    let mut v = Vec::new();
    for q in 0..groups.max(1) {
        for l in (0..=q).rev() {
            if (q - l) % 2 != 0 {
                continue;
            }
            let p = (q - l) / 2;
            v.push(LgMode { p, l, sin: false });
            if l > 0 {
                v.push(LgMode { p, l, sin: true });
            }
        }
    }
    v
}

/// Unit-power real LG field at beam coordinates (ρ, θ). `eq:launch-lg`
pub fn lg_field(md: LgMode, w: f64, rho: f64, theta: f64) -> f64 {
    let l = md.l;
    let mut c = (2.0 * factorial(md.p) / (PI * factorial(md.p + l))).sqrt() / w;
    if l > 0 {
        c *= 2f64.sqrt();
    }
    let s = 2f64.sqrt() * rho / w;
    let ang = if l == 0 {
        1.0
    } else if md.sin {
        (l as f64 * theta).sin()
    } else {
        (l as f64 * theta).cos()
    };
    c * s.powi(l as i32) * laguerre(md.p, l, s * s) * (-(rho * rho) / (w * w)).exp() * ang
}

/// Couple the multimode VCSEL launch (incoherent equal-power mixture of LG modes, see
/// [`LaunchConfig`]) into the fiber modes. `eq:launch-incoherent`
pub fn couple_launch(set: &ModeSet, launch: &LaunchConfig, angular_points: usize) -> Coupling {
    let lgs = lg_modes(launch.vcsel_mode_groups);
    let w = launch.beam_radius;
    let k0 = 2.0 * PI / set.wavelength;
    let kx = k0 * launch.tilt.sin();
    let dx = launch.offset;
    let share = 1.0 / lgs.len() as f64;
    // Rings farther than this from the beam center carry < e^-40 of any LG mode's field.
    let reach = w * ((2.0 * launch.vcsel_mode_groups.max(1) as f64).sqrt() + 6.5);
    let l_max = lgs.iter().map(|m| m.l).max().unwrap_or(0) as usize;
    // Per-mode constant prefactors (see `lg_field`).
    let consts: Vec<f64> = lgs
        .iter()
        .map(|m| {
            let mut c = (2.0 * factorial(m.p) / (PI * factorial(m.p + m.l))).sqrt() / w;
            if m.l > 0 {
                c *= 2f64.sqrt();
            }
            c
        })
        .collect();
    let results = couple_fields(
        set,
        lgs.len(),
        |r, phi, out| {
            let (s, co) = phi.sin_cos();
            let x = r * co;
            let y = r * s;
            let (bx, by) = (x - dx, y);
            let rho2 = bx * bx + by * by;
            let rho = rho2.sqrt();
            let (ct, st) = if rho > 0.0 {
                (bx / rho, by / rho)
            } else {
                (1.0, 0.0)
            };
            let env = (-rho2 / (w * w)).exp();
            let sn = 2f64.sqrt() * rho / w;
            let lag_x = sn * sn;
            // cos(lθ), sin(lθ) and s^l by recurrence.
            let mut cl = [0.0f64; 32];
            let mut sl = [0.0f64; 32];
            let mut pw = [0.0f64; 32];
            cl[0] = 1.0;
            pw[0] = 1.0;
            for l in 1..=l_max {
                cl[l] = cl[l - 1] * ct - sl[l - 1] * st;
                sl[l] = sl[l - 1] * ct + cl[l - 1] * st;
                pw[l] = pw[l - 1] * sn;
            }
            let tilt = Complex64::from_polar(1.0, kx * x);
            for ((o, &lg), &c) in out.iter_mut().zip(&lgs).zip(&consts) {
                let l = lg.l as usize;
                let ang = if lg.sin { sl[l] } else { cl[l] };
                *o = tilt * (c * pw[l] * laguerre(lg.p, lg.l, lag_x) * env * ang);
            }
        },
        |r| (r - dx.abs()).abs() <= reach,
        angular_points,
        &vec![Some(1.0); lgs.len()],
    );
    let mut acc = results[0].clone();
    for m in acc.modes.iter_mut() {
        m.power = 0.0;
        if lgs.len() > 1 {
            // Coefficients are not meaningful for an incoherent mixture.
            m.c_cos = Complex64::new(f64::NAN, f64::NAN);
            m.c_sin = Complex64::new(f64::NAN, f64::NAN);
        }
    }
    acc.guided_fraction = 0.0;
    acc.domain_fraction = 0.0;
    for c in &results {
        for (am, cm) in acc.modes.iter_mut().zip(&c.modes) {
            am.power += share * cm.power;
        }
        acc.guided_fraction += share * c.guided_fraction;
        acc.domain_fraction += share * c.domain_fraction;
    }
    acc
}

/// Couple the Gaussian launch beam described by `launch` into the modes of `set`.
pub fn couple_gaussian(set: &ModeSet, launch: &LaunchConfig, angular_points: usize) -> Coupling {
    let w = launch.beam_radius;
    let amp = (2.0 / (PI * w * w)).sqrt();
    let k0 = 2.0 * PI / set.wavelength;
    let kx = k0 * launch.tilt.sin();
    let dx = launch.offset;
    couple_field(
        set,
        |r, phi| {
            let (s, c) = phi.sin_cos();
            let x = r * c;
            let y = r * s;
            let mag = amp * (-((x - dx).powi(2) + y * y) / (w * w)).exp();
            Complex64::from_polar(mag, kx * x)
        },
        angular_points,
        Some(1.0),
    )
}

/// Encircled flux EF(ρ) of the launched guided near field (incoherent mode-power sum).
/// Returns the fraction of guided power within radius `rho`.
pub fn encircled_flux(set: &ModeSet, coupling: &Coupling, rho: f64) -> f64 {
    let mut inside = 0.0;
    let mut total = 0.0;
    for (md, c) in set.modes.iter().zip(&coupling.modes) {
        inside += c.power * crate::modes::power_within(set, md, rho);
        total += c.power;
    }
    if total > 0.0 {
        inside / total
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;
    use crate::modes::{solve, SolverParams};

    fn om4_parabolic() -> ModeSet {
        solve(&SolverParams {
            core_material: crate::config::CoreMaterial::default(),
            core_radius: 25e-6,
            na: 0.2,
            profile: Profile::Graded { alpha: 2.0 },
            wavelength: 850e-9,
            grid_points_per_radius: 400,
            domain_factor: 2.0,
        })
    }

    /// V4: coaxial Gaussian onto the LP01 mode of a parabolic fiber.
    /// η = (2 w₀ w₁ / (w₀² + w₁²))², w₁² = 2a²/V.
    #[test]
    fn v4_gaussian_to_lp01() {
        let set = om4_parabolic();
        let w1 = (2.0 * set.core_radius.powi(2) / set.v_number).sqrt();
        for w0 in [3e-6, 5e-6, w1, 9e-6] {
            let c = couple_gaussian(
                &set,
                &LaunchConfig {
                    vcsel_mode_groups: 1,
                    beam_radius: w0,
                    offset: 0.0,
                    tilt: 0.0,
                },
                DEFAULT_ANGULAR_POINTS,
            );
            let eta = c.modes[0].power;
            let exact = (2.0 * w0 * w1 / (w0 * w0 + w1 * w1)).powi(2);
            assert!(
                ((eta - exact) / exact).abs() < 1e-3,
                "w0 {w0}: {eta} vs {exact}"
            );
        }
    }

    /// V5: power conservation — Σ|c|² ≤ 1, and = 1 for an input that is itself a mode.
    #[test]
    fn v5_power_conservation() {
        let set = om4_parabolic();
        for (off, tilt) in [(0.0, 0.0), (10e-6, 0.0), (5e-6, 0.05), (20e-6, 0.1)] {
            let c = couple_gaussian(
                &set,
                &LaunchConfig {
                    vcsel_mode_groups: 1,
                    beam_radius: 6e-6,
                    offset: off,
                    tilt,
                },
                DEFAULT_ANGULAR_POINTS,
            );
            assert!(c.guided_fraction <= 1.0 + 1e-9, "{}", c.guided_fraction);
            assert!(c.guided_fraction > 0.5);
        }
        // Input equal to LP21 (cos orientation): all power must land in that mode.
        let idx = set.modes.iter().position(|m| m.l == 2 && m.m == 1).unwrap();
        let md = &set.modes[idx];
        let r = set.r.clone();
        let dr = set.dr;
        let f = md.field.clone();
        let c = couple_field(
            &set,
            |rr, phi| {
                let i = ((rr / dr) - 0.5).round() as usize;
                debug_assert!((r[i] - rr).abs() < 1e-12);
                Complex64::new(f[i] * (2.0 * phi).cos(), 0.0)
            },
            DEFAULT_ANGULAR_POINTS,
            None,
        );
        assert!(
            (c.guided_fraction - 1.0).abs() < 1e-6,
            "{}",
            c.guided_fraction
        );
        assert!((c.modes[idx].power - 1.0).abs() < 1e-6);
    }

    /// V6: offset +Δx and −Δx give identical mode powers.
    #[test]
    fn v6_offset_symmetry() {
        let set = om4_parabolic();
        let mk = |off: f64| {
            couple_gaussian(
                &set,
                &LaunchConfig {
                    vcsel_mode_groups: 1,
                    beam_radius: 6e-6,
                    offset: off,
                    tilt: 0.0,
                },
                DEFAULT_ANGULAR_POINTS,
            )
        };
        let p = mk(8e-6);
        let n = mk(-8e-6);
        let pmax = p.modes.iter().map(|m| m.power).fold(0.0, f64::max);
        for (a, b) in p.modes.iter().zip(&n.modes) {
            // Identical up to floating-point summation order: 1e-12 of the largest mode power.
            assert!(
                (a.power - b.power).abs() < 1e-12 * pmax,
                "LP{}{}: {} vs {}",
                a.l,
                a.m,
                a.power,
                b.power
            );
        }
        assert!((p.guided_fraction - n.guided_fraction).abs() < 1e-12);
    }

    #[test]
    fn offset_excites_higher_order_modes() {
        let set = om4_parabolic();
        let mut prev_lp01 = f64::INFINITY;
        for off in [0.0, 5e-6, 10e-6, 15e-6] {
            let c = couple_gaussian(
                &set,
                &LaunchConfig {
                    vcsel_mode_groups: 1,
                    beam_radius: 6e-6,
                    offset: off,
                    tilt: 0.0,
                },
                DEFAULT_ANGULAR_POINTS,
            );
            assert!(c.modes[0].power < prev_lp01);
            prev_lp01 = c.modes[0].power;
        }
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;
    use crate::config::Profile;
    use crate::modes::{solve, SolverParams};

    fn set() -> ModeSet {
        solve(&SolverParams {
            core_material: crate::config::CoreMaterial::default(),
            core_radius: 25e-6,
            na: 0.2,
            profile: Profile::Graded { alpha: 2.0 },
            wavelength: 850e-9,
            grid_points_per_radius: 400,
            domain_factor: 2.0,
        })
    }

    #[test]
    fn lg_mode_enumeration() {
        let m = lg_modes(4);
        // Groups 1..4 contain 1, 2, 3, 4 real spatial modes.
        assert_eq!(m.len(), 10);
        assert!(m.contains(&LgMode {
            p: 1,
            l: 1,
            sin: true
        }));
        assert!(m.contains(&LgMode {
            p: 0,
            l: 3,
            sin: false
        }));
    }

    #[test]
    fn lg_modes_are_unit_power() {
        let s = set();
        for lg in lg_modes(5) {
            let c = couple_field(
                &s,
                |r, phi| Complex64::new(lg_field(lg, 5e-6, r, phi), 0.0),
                DEFAULT_ANGULAR_POINTS,
                Some(1.0),
            );
            // Discrete power of the analytic unit-power field ≈ 1.
            assert!(
                (c.domain_fraction - 1.0).abs() < 1e-6,
                "{lg:?}: {}",
                c.domain_fraction
            );
        }
    }

    #[test]
    fn single_group_launch_equals_gaussian() {
        let s = set();
        let l = LaunchConfig {
            beam_radius: 5e-6,
            vcsel_mode_groups: 1,
            offset: 7e-6,
            tilt: 0.02,
        };
        let a = couple_launch(&s, &l, DEFAULT_ANGULAR_POINTS);
        let b = couple_gaussian(&s, &l, DEFAULT_ANGULAR_POINTS);
        for (x, y) in a.modes.iter().zip(&b.modes) {
            assert!((x.power - y.power).abs() < 1e-12);
        }
    }

    #[test]
    fn multimode_launch_conserves_power() {
        let s = set();
        for off in [0.0, 10e-6, 20e-6] {
            let c = couple_launch(
                &s,
                &LaunchConfig {
                    offset: off,
                    ..LaunchConfig::default()
                },
                DEFAULT_ANGULAR_POINTS,
            );
            assert!(c.guided_fraction <= 1.0 + 1e-12);
            assert!(c.guided_fraction > 0.3);
        }
    }
}

#[cfg(test)]
mod quadrature_tests {
    use super::*;
    use crate::config::Profile;
    use crate::modes::{solve, SolverParams};

    /// The default angular resolution is converged for large offsets and tilts.
    #[test]
    fn angular_quadrature_converged() {
        let set = solve(&SolverParams {
            core_material: crate::config::CoreMaterial::default(),
            core_radius: 25e-6,
            na: 0.2,
            profile: Profile::Graded { alpha: 2.0 },
            wavelength: 850e-9,
            grid_points_per_radius: 300,
            domain_factor: 2.0,
        });
        let l = LaunchConfig {
            offset: 20e-6,
            tilt: 0.15,
            ..LaunchConfig::default()
        };
        let a = couple_launch(&set, &l, DEFAULT_ANGULAR_POINTS);
        let b = couple_launch(&set, &l, 4 * DEFAULT_ANGULAR_POINTS);
        for (x, y) in a.modes.iter().zip(&b.modes) {
            assert!((x.power - y.power).abs() < 1e-10, "LP{}{}", x.l, x.m);
        }
    }
}

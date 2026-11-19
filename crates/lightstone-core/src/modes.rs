//! Scalar LP mode solver for circularly symmetric multimode fibers.
//!
//! `eq:modes-radial` — For ψ = F(r)·{cos, sin}(lφ) the scalar wave equation becomes
//! F'' + F'/r + (k²n²(r) − l²/r²) F = β² F.
//!
//! In normalized radius x = r/a with eigenvalue λ = a²(β² − k²n₂²):
//! F'' + F'/x − l² F/x² + V² g(x) F = λ F, g(x) = (n²(x) − n₂²)/(n₁² − n₂²).
//!
//! Discretization: cell-centered finite volumes x_i = (i + ½)h in flux-conservative form,
//! symmetrized with √x_i, giving a real symmetric tridiagonal matrix. Guided modes are the
//! eigenvalues λ > 0. Eigenvalues are found by Sturm-sequence bisection, eigenvectors by
//! inverse iteration. The core–cladding interface lies on a cell face (second-order
//! accuracy for step profiles). Dirichlet boundary at x = domain_factor.
//!
//! Dispersion: the cladding n₂(λ) is fused silica (Sellmeier). For a germania-doped core
//! (default) n₁(λ) is GeO₂–SiO₂ glass whose GeO₂ fraction gives the NA at 850 nm, and the
//! profile is n²(r, λ) = n₂²(λ) + (n₁²(λ) − n₂²(λ))·g(r/a): Δ changes with λ (profile
//! dispersion). With `fixed_delta`, Δ = NA²/(2n₁²) is held constant and n₁ = n₂/√(1 − 2Δ).
//! Group delay and chromatic delay slope per mode come from 5-point finite differences of
//! n_eff(λ).

use crate::config::{CoreMaterial, FiberConfig, Profile};
use crate::material::{germania_fraction_for_na, germania_silica_index, silica_index};
use crate::units::C0;
use std::f64::consts::PI;

/// Wavelength step for finite-difference derivatives [m]. 4 nm balances truncation error
/// (O(δλ⁴), negligible) against amplification of eigenvalue round-off in d²n/dλ².
const D_LAMBDA: f64 = 4e-9;

/// One guided LP_lm mode.
#[derive(Clone, Debug)]
pub struct Mode {
    /// Azimuthal order.
    pub l: u32,
    /// Radial order (1-based).
    pub m: u32,
    /// Principal mode group number g = 2m + l − 1 (meaningful for near-parabolic profiles).
    pub group: u32,
    /// Effective index at the center wavelength.
    pub n_eff: f64,
    /// Normalized propagation constant b = (n_eff² − n₂²)/(n₁² − n₂²).
    pub b: f64,
    /// Group delay per unit length τ/L \[s/m\].
    pub group_delay: f64,
    /// Chromatic delay slope per unit length d(τ/L)/dλ \[s/m²\].
    pub chromatic_slope: f64,
    /// Radial field F(r_i), normalized so that the full mode (including cos/sin(lφ))
    /// carries unit power: 2π∫F²r dr = 1 (l = 0) or π∫F²r dr = 1 (l > 0). \[1/m\]
    pub field: Vec<f64>,
}

impl Mode {
    /// Number of degenerate spatial orientations (cos and sin) for this mode.
    pub fn orientations(&self) -> u32 {
        if self.l == 0 {
            1
        } else {
            2
        }
    }
}

/// Result of a mode solve.
#[derive(Clone, Debug)]
pub struct ModeSet {
    pub wavelength: f64,
    pub core_radius: f64,
    pub n_core: f64,
    pub n_clad: f64,
    pub v_number: f64,
    /// Radial grid r_i \[m\] (cell centers).
    pub r: Vec<f64>,
    /// Radial cell width \[m\].
    pub dr: f64,
    pub modes: Vec<Mode>,
}

impl ModeSet {
    /// Total number of spatial modes counting cos/sin orientations (not polarization).
    pub fn spatial_mode_count(&self) -> usize {
        self.modes.iter().map(|m| m.orientations() as usize).sum()
    }
}

/// Mode-solver inputs, independent of fiber length/attenuation.
#[derive(Clone, Debug, PartialEq)]
pub struct SolverParams {
    pub core_radius: f64,
    pub na: f64,
    pub profile: Profile,
    pub core_material: CoreMaterial,
    pub wavelength: f64,
    pub grid_points_per_radius: usize,
    pub domain_factor: f64,
}

impl SolverParams {
    pub fn from_fiber(f: &FiberConfig, wavelength: f64) -> Self {
        Self {
            core_radius: f.core_radius,
            na: f.na,
            profile: f.profile,
            core_material: f.core_material,
            wavelength,
            grid_points_per_radius: f.grid_points_per_radius,
            domain_factor: f.domain_factor,
        }
    }
}

/// Normalized profile g(x) ∈ [0, 1].
fn profile_g(profile: Profile, x: f64) -> f64 {
    if x >= 1.0 {
        return 0.0;
    }
    match profile {
        Profile::Step => 1.0,
        Profile::Graded { alpha } => 1.0 - x.powf(alpha),
    }
}

/// Symmetric tridiagonal matrix (diagonal `d`, off-diagonal `e`, len n−1).
struct Tridiag {
    d: Vec<f64>,
    e: Vec<f64>,
}

impl Tridiag {
    fn n(&self) -> usize {
        self.d.len()
    }

    /// Number of eigenvalues strictly less than `x` (Sturm sequence / LDLᵀ inertia).
    fn count_less(&self, x: f64) -> usize {
        let mut count = 0;
        let mut q = self.d[0] - x;
        if q < 0.0 {
            count += 1;
        }
        for i in 1..self.n() {
            let qq = if q.abs() < 1e-300 { 1e-300 } else { q };
            q = self.d[i] - x - self.e[i - 1] * self.e[i - 1] / qq;
            if q < 0.0 {
                count += 1;
            }
        }
        count
    }

    /// Gershgorin upper bound of the spectrum.
    fn upper_bound(&self) -> f64 {
        let n = self.n();
        let mut ub = f64::MIN;
        for i in 0..n {
            let mut r = 0.0;
            if i > 0 {
                r += self.e[i - 1].abs();
            }
            if i + 1 < n {
                r += self.e[i].abs();
            }
            ub = ub.max(self.d[i] + r);
        }
        ub
    }

    /// k-th largest eigenvalue (k = 0 is the largest), searched in [lo, hi].
    fn kth_largest(&self, k: usize, mut lo: f64, mut hi: f64) -> f64 {
        let n = self.n();
        // e_k (descending) is the (n-1-k)-th smallest: count_less(x) <= n-1-k  ⇔  x <= e_k.
        let target = n - 1 - k;
        for _ in 0..300 {
            let mid = 0.5 * (lo + hi);
            if mid <= lo || mid >= hi {
                break;
            }
            if self.count_less(mid) <= target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Eigenvector for eigenvalue `ev` by inverse iteration (partial-pivoting LU).
    fn eigenvector(&self, ev: f64) -> Vec<f64> {
        let n = self.n();
        let scale = self.upper_bound().abs().max(1.0);
        // Tiny shift keeps the factorization nonsingular.
        let shift = ev + 1e-13 * scale;
        let mut v: Vec<f64> = (0..n)
            .map(|i| 1.0 + 0.1 * ((i as f64) * 0.618_034).sin())
            .collect();
        for _ in 0..4 {
            v = solve_tridiag_pivot(&self.d, &self.e, shift, &v);
            let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
        v
    }
}

/// Solve (T − σI) x = b for symmetric tridiagonal T with partial pivoting (LAPACK dgtsv style).
fn solve_tridiag_pivot(d: &[f64], e: &[f64], sigma: f64, b: &[f64]) -> Vec<f64> {
    let n = d.len();
    // Working arrays: dl (sub), dd (diag), du (super), du2 (second super from pivoting).
    let mut dl: Vec<f64> = e.to_vec();
    let mut dd: Vec<f64> = d.iter().map(|x| x - sigma).collect();
    let mut du: Vec<f64> = e.to_vec();
    let mut du2 = vec![0.0; n.saturating_sub(2)];
    let mut x = b.to_vec();
    for i in 0..n - 1 {
        if dd[i].abs() >= dl[i].abs() {
            // No row interchange.
            let piv = if dd[i] == 0.0 { 1e-300 } else { dd[i] };
            let fact = dl[i] / piv;
            dd[i + 1] -= fact * du[i];
            x[i + 1] -= fact * x[i];
            dl[i] = 0.0;
        } else {
            // Interchange rows i and i+1.
            let fact = dd[i] / dl[i];
            dd[i] = dl[i];
            let temp = dd[i + 1];
            dd[i + 1] = du[i] - fact * temp;
            if i + 1 < n - 1 {
                dl[i] = du[i + 1];
                du[i + 1] = -fact * dl[i];
            }
            du[i] = temp;
            x.swap(i, i + 1);
            x[i + 1] -= fact * x[i];
            if i + 1 < n - 1 {
                du2[i] = dl[i];
            }
            dl[i] = 0.0;
        }
    }
    // Back substitution with up to two super-diagonals.
    let fix = |v: f64| if v == 0.0 { 1e-300 } else { v };
    x[n - 1] /= fix(dd[n - 1]);
    if n > 1 {
        x[n - 2] = (x[n - 2] - du[n - 2] * x[n - 1]) / fix(dd[n - 2]);
    }
    for i in (0..n.saturating_sub(2)).rev() {
        x[i] = (x[i] - du[i] * x[i + 1] - du2[i] * x[i + 2]) / fix(dd[i]);
    }
    x
}

/// Build the normalized operator for azimuthal order `l`.
fn build_operator(profile: Profile, v2: f64, l: u32, h: f64, n: usize) -> Tridiag {
    let l2 = (l * l) as f64;
    let mut d = Vec::with_capacity(n);
    let mut e = Vec::with_capacity(n.saturating_sub(1));
    for i in 0..n {
        let xi = (i as f64 + 0.5) * h;
        let xm = i as f64 * h; // face i-1/2
        let xp = (i as f64 + 1.0) * h; // face i+1/2
        let g = profile_g(profile, xi);
        d.push(-(xp + xm) / (xi * h * h) + v2 * g - l2 / (xi * xi));
        if i + 1 < n {
            let xn = (i as f64 + 1.5) * h;
            e.push(xp / (h * h * (xi * xn).sqrt()));
        }
    }
    Tridiag { d, e }
}

/// Core-centre and cladding indices (n₁, n₂) at wavelength `lambda`.
fn indices(p: &SolverParams, lambda: f64) -> (f64, f64) {
    if p.core_material == CoreMaterial::GermaniaDoped {
        let x = germania_fraction_for_na(p.na);
        return (germania_silica_index(lambda, x), silica_index(lambda));
    }
    // Fixed Δ from the NA at the source wavelength.
    let n2_ref = silica_index(p.wavelength);
    let n1_ref = (n2_ref * n2_ref + p.na * p.na).sqrt();
    let delta = p.na * p.na / (2.0 * n1_ref * n1_ref);
    let n2 = silica_index(lambda);
    let n1 = n2 / (1.0 - 2.0 * delta).sqrt();
    (n1, n2)
}

/// Guided eigenvalues (descending) and optionally eigenvectors for one l at one wavelength.
struct LSolve {
    eigenvalues: Vec<f64>,
    vectors: Vec<Vec<f64>>,
}

fn solve_l(
    profile: Profile,
    v2: f64,
    l: u32,
    h: f64,
    n: usize,
    want_vectors: bool,
    max_modes: Option<usize>,
) -> LSolve {
    let t = build_operator(profile, v2, l, h, n);
    let ub = t.upper_bound();
    let n_guided = n - t.count_less(0.0);
    let n_take = max_modes.map_or(n_guided, |m| m.min(n_guided));
    let mut eigenvalues = Vec::with_capacity(n_take);
    let mut vectors = Vec::new();
    let mut hi = ub;
    for k in 0..n_take {
        let ev = t.kth_largest(k, 0.0, hi);
        hi = ev + 1e-9 * ev.abs().max(1.0); // eigenvalues are strictly descending
        eigenvalues.push(ev);
        if want_vectors {
            vectors.push(t.eigenvector(ev));
        }
    }
    LSolve {
        eigenvalues,
        vectors,
    }
}

/// Solve all guided LP modes, including group delay and chromatic slope.
pub fn solve(p: &SolverParams) -> ModeSet {
    let a = p.core_radius;
    let h = 1.0 / p.grid_points_per_radius as f64;
    let n = (p.domain_factor * p.grid_points_per_radius as f64).round() as usize;
    let lambdas: [f64; 5] = [-2.0, -1.0, 0.0, 1.0, 2.0].map(|j| p.wavelength + j * D_LAMBDA);

    let k_of = |lam: f64| 2.0 * PI / lam;
    let geom: Vec<(f64, f64, f64)> = lambdas
        .iter()
        .map(|&lam| {
            let (n1, n2) = indices(p, lam);
            let v2 = (k_of(lam) * a).powi(2) * (n1 * n1 - n2 * n2);
            (n1, n2, v2)
        })
        .collect();
    let (n1c, n2c, v2c) = geom[2];
    let kc = k_of(p.wavelength);

    let mut modes = Vec::new();
    let mut l = 0u32;
    loop {
        // Center wavelength: eigenvalues + vectors.
        let center = solve_l(p.profile, v2c, l, h, n, true, None);
        if center.eigenvalues.is_empty() {
            break;
        }
        // Neighbor wavelengths: only eigenvalues; keep modes guided at all five.
        let others: Vec<LSolve> = [0usize, 1, 3, 4]
            .iter()
            .map(|&j| {
                solve_l(
                    p.profile,
                    geom[j].2,
                    l,
                    h,
                    n,
                    false,
                    Some(center.eigenvalues.len()),
                )
            })
            .collect();
        let count = others
            .iter()
            .map(|s| s.eigenvalues.len())
            .min()
            .unwrap_or(0)
            .min(center.eigenvalues.len());

        for mi in 0..count {
            let neff_at = |j: usize, ev: f64| {
                let (_, n2, _) = geom[j];
                let k = k_of(lambdas[j]);
                (n2 * n2 + ev / (k * a).powi(2)).sqrt()
            };
            let ne = [
                neff_at(0, others[0].eigenvalues[mi]),
                neff_at(1, others[1].eigenvalues[mi]),
                neff_at(2, center.eigenvalues[mi]),
                neff_at(3, others[2].eigenvalues[mi]),
                neff_at(4, others[3].eigenvalues[mi]),
            ];
            let dn = (ne[0] - 8.0 * ne[1] + 8.0 * ne[3] - ne[4]) / (12.0 * D_LAMBDA);
            let d2n = (-ne[0] + 16.0 * ne[1] - 30.0 * ne[2] + 16.0 * ne[3] - ne[4])
                / (12.0 * D_LAMBDA * D_LAMBDA);
            let n_eff = ne[2];
            let group_delay = (n_eff - p.wavelength * dn) / C0;
            let chromatic_slope = -p.wavelength * d2n / C0;

            // Field: G_i = sqrt(x_i) F_i is the unit-norm eigenvector (Σ G² = 1).
            let g = &center.vectors[mi];
            let ang = if l == 0 { 2.0 * PI } else { PI };
            // Physical normalization: ang · Σ F_i² r_i dr = 1 with r = a x, dr = a h.
            // Σ F_i² x_i h a² = Σ G_i² h a² ⇒ F_i = G_i / (sqrt(x_i) · sqrt(ang h) · a).
            let norm = 1.0 / ((ang * h).sqrt() * a);
            let mut field: Vec<f64> = g
                .iter()
                .enumerate()
                .map(|(i, &gi)| gi / ((i as f64 + 0.5) * h).sqrt() * norm)
                .collect();
            // Sign convention: first significant lobe positive.
            let fmax = field.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            if let Some(first) = field.iter().find(|v| v.abs() > 1e-3 * fmax) {
                if *first < 0.0 {
                    field.iter_mut().for_each(|v| *v = -*v);
                }
            }
            let b = center.eigenvalues[mi] / v2c;
            modes.push(Mode {
                l,
                m: mi as u32 + 1,
                group: 2 * (mi as u32 + 1) + l - 1,
                n_eff,
                b,
                group_delay,
                chromatic_slope,
                field,
            });
        }
        l += 1;
    }
    modes.sort_by(|x, y| y.n_eff.partial_cmp(&x.n_eff).unwrap());

    let r: Vec<f64> = (0..n).map(|i| (i as f64 + 0.5) * h * a).collect();
    let _ = kc;
    ModeSet {
        wavelength: p.wavelength,
        core_radius: a,
        n_core: n1c,
        n_clad: n2c,
        v_number: v2c.sqrt(),
        r,
        dr: h * a,
        modes,
    }
}

/// Radial inner product ∫ F₁ F₂ r dr on the solver grid.
pub fn radial_overlap(set: &ModeSet, f1: &[f64], f2: &[f64]) -> f64 {
    f1.iter()
        .zip(f2)
        .zip(&set.r)
        .map(|((a, b), r)| a * b * r)
        .sum::<f64>()
        * set.dr
}

/// Fraction of a mode's power inside radius `rd` (centered circular detector).
pub fn power_within(set: &ModeSet, mode: &Mode, rd: f64) -> f64 {
    let total = radial_overlap(set, &mode.field, &mode.field);
    let mut acc = 0.0;
    for (i, (&f, &r)) in mode.field.iter().zip(&set.r).enumerate() {
        let lo = i as f64 * set.dr;
        let hi = lo + set.dr;
        if hi <= rd {
            acc += f * f * r * set.dr;
        } else if lo < rd {
            acc += f * f * r * (rd - lo);
        } else {
            break;
        }
    }
    acc / total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn om4(alpha: f64, ppr: usize) -> SolverParams {
        SolverParams {
            core_material: crate::config::CoreMaterial::default(),
            core_radius: 25e-6,
            na: 0.2,
            profile: Profile::Graded { alpha },
            wavelength: 850e-9,
            grid_points_per_radius: ppr,
            domain_factor: 2.0,
        }
    }

    #[test]
    fn tridiag_solver_matches_dense() {
        // 1D Laplacian eigenvalues: 2 - 2cos(kπ/(n+1)).
        let n = 50;
        let t = Tridiag {
            d: vec![2.0; n],
            e: vec![-1.0; n - 1],
        };
        for k in 0..5 {
            let ev = t.kth_largest(k, -1.0, t.upper_bound());
            let exact = 2.0 - 2.0 * ((n - k) as f64 * PI / (n as f64 + 1.0)).cos();
            assert!((ev - exact).abs() < 1e-12, "{k}: {ev} vs {exact}");
            let v = t.eigenvector(ev);
            // Residual check.
            let mut res: f64 = 0.0;
            for i in 0..n {
                let mut tv = t.d[i] * v[i];
                if i > 0 {
                    tv += t.e[i - 1] * v[i - 1];
                }
                if i + 1 < n {
                    tv += t.e[i] * v[i + 1];
                }
                res = res.max((tv - ev * v[i]).abs());
            }
            assert!(res < 1e-9, "residual {res}");
        }
    }

    /// V2: near-infinite parabolic profile — mode-group spacing and degeneracy.
    /// β² = k²n₁² − (2kn₁√(2Δ)/a)·g for g = 2m + l − 1.
    #[test]
    fn v2_parabolic_mode_groups() {
        let p = om4(2.0, 800);
        let set = solve(&p);
        let k = 2.0 * PI / p.wavelength;
        let n1 = set.n_core;
        let n2 = set.n_clad;
        let delta = (n1 * n1 - n2 * n2) / (2.0 * n1 * n1);
        for md in set.modes.iter().filter(|m| m.group <= 4) {
            let beta2 = k * k * n1 * n1
                - 2.0 * k * n1 * (2.0 * delta).sqrt() / p.core_radius * md.group as f64;
            let n_exact = beta2.sqrt() / k;
            let err = (md.n_eff - n_exact).abs();
            assert!(err < 1e-5, "LP{}{} g={} err {err:e}", md.l, md.m, md.group);
        }
        // Group g contains ceil(g/2) LP mode labels.
        for g in 1..=6 {
            let c = set.modes.iter().filter(|m| m.group == g).count();
            assert_eq!(c, (g as usize).div_ceil(2), "group {g}");
        }
    }

    /// V3: orthonormality of same-l modes on the solver grid.
    #[test]
    fn v3_orthonormality() {
        let set = solve(&om4(2.0, 400));
        for l in 0..4u32 {
            let ms: Vec<&Mode> = set.modes.iter().filter(|m| m.l == l).collect();
            let ang = if l == 0 { 2.0 * PI } else { PI };
            for (i, a) in ms.iter().enumerate() {
                for (j, b) in ms.iter().enumerate() {
                    let o = ang * radial_overlap(&set, &a.field, &b.field);
                    let expect = if i == j { 1.0 } else { 0.0 };
                    assert!((o - expect).abs() < 1e-6, "l={l} {i},{j}: {o}");
                }
            }
        }
    }

    #[test]
    fn om4_mode_count_and_v() {
        let set = solve(&om4(2.0, 400));
        // V = k a NA ≈ 36.96 at 850 nm.
        assert!((set.v_number - 36.96).abs() < 0.05, "V = {}", set.v_number);
        // Graded index: ~V²/4 modes counting both polarizations → ~V²/8 spatial modes.
        let spatial = set.spatial_mode_count() as f64;
        let expected = set.v_number.powi(2) / 8.0;
        assert!(
            (spatial / expected - 1.0).abs() < 0.15,
            "spatial modes {spatial}, expected ≈ {expected}"
        );
    }

    #[test]
    fn group_delay_physical() {
        let set = solve(&om4(2.0, 400));
        let lp01 = &set.modes[0];
        assert_eq!((lp01.l, lp01.m), (0, 1));
        // Group index ~1.47 and chromatic slope ~ material dispersion (−85…−110 ps/nm/km).
        let ng = lp01.group_delay * C0;
        assert!((ng - 1.47).abs() < 0.02, "ng {ng}");
        let d = lp01.chromatic_slope / crate::units::ps_nm_km_to_si(1.0);
        assert!(d < -70.0 && d > -120.0, "D {d}");
    }

    #[test]
    fn grid_convergence_of_delays() {
        // Differential group delays at the default resolution (600 points per radius) must
        // agree with a 2× finer grid to within 1 % of the delay spread. α = 2.10 lies ≈ 0.065
        // above the optimum of the germania-doped model (≈ 2.035), so the spread is typical.
        let a = solve(&om4(2.10, 600));
        let b = solve(&om4(2.10, 1200));
        let ref0a = a.modes[0].group_delay;
        let ref0b = b.modes[0].group_delay;
        let pairs: Vec<_> = a
            .modes
            .iter()
            .zip(&b.modes)
            .filter(|(m, _)| m.group <= 15)
            .collect();
        let spread = pairs
            .iter()
            .map(|(_, mb)| (mb.group_delay - ref0b).abs())
            .fold(0.0, f64::max);
        for (ma, mb) in pairs {
            assert_eq!((ma.l, ma.m), (mb.l, mb.m));
            let da = ma.group_delay - ref0a;
            let db = mb.group_delay - ref0b;
            assert!(
                (da - db).abs() < 0.01 * spread,
                "LP{}{}: {da:e} vs {db:e}",
                ma.l,
                ma.m
            );
        }
    }

    #[test]
    fn detector_fraction() {
        let set = solve(&om4(2.0, 400));
        let lp01 = &set.modes[0];
        assert!((power_within(&set, lp01, 1.0) - 1.0).abs() < 1e-9);
        let half = power_within(&set, lp01, 5e-6);
        assert!(half > 0.0 && half < 1.0);
    }
}

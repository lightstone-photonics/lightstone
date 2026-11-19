//! Gaussian tail functions and quantiles.

/// Gaussian tail probability Q(x) = ½·erfc(x/√2).
pub fn q_func(x: f64) -> f64 {
    0.5 * libm::erfc(x / std::f64::consts::SQRT_2)
}

/// Inverse of Q on (0, ½]: returns x with Q(x) = p (bisection, |error| < 1e-12).
pub fn q_inv(p: f64) -> f64 {
    assert!(p > 0.0 && p < 1.0);
    let (mut lo, mut hi) = (-40.0, 40.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if q_func(mid) > p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Empirical quantile (type-7 linear interpolation). Reorders `v`.
pub fn quantile(v: &mut [f64], q: f64) -> f64 {
    assert!(!v.is_empty());
    let n = v.len();
    let pos = q.clamp(0.0, 1.0) * (n - 1) as f64;
    let i = pos.floor() as usize;
    let frac = pos - i as f64;
    let (_, a, rest) = v.select_nth_unstable_by(i, |x, y| x.partial_cmp(y).unwrap());
    let a = *a;
    if frac == 0.0 || rest.is_empty() {
        return a;
    }
    let b = rest.iter().cloned().fold(f64::INFINITY, f64::min);
    a + frac * (b - a)
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

pub fn std_dev(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_values() {
        assert!((q_func(0.0) - 0.5).abs() < 1e-15);
        // Q(3) = 1.3498980316e-3
        assert!((q_func(3.0) - 1.349_898_031_630_094_6e-3).abs() < 1e-15);
        assert!((q_inv(q_func(3.414)) - 3.414).abs() < 1e-9);
    }

    #[test]
    fn quantiles() {
        let mut v: Vec<f64> = (0..101).map(|i| i as f64).collect();
        assert_eq!(quantile(&mut v, 0.5), 50.0);
        assert!((quantile(&mut v, 0.001) - 0.1).abs() < 1e-12);
    }
}

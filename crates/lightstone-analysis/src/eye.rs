//! 2-UI eye-diagram density rasterizer.
//!
//! Each symbol contributes one trace spanning 2 UI centered on the sampling phase. Traces are
//! drawn as connected line segments (consecutive columns are joined vertically), so steep
//! edges produce continuous traces like a persistence-mode oscilloscope.

use serde::{Deserialize, Serialize};

/// Accumulated eye density. `counts[row * width + col]`, row 0 is the top (v_max).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EyeDiagram {
    pub width: usize,
    pub height: usize,
    pub counts: Vec<u32>,
    pub v_min: f64,
    pub v_max: f64,
    /// Time span of the window \[UI\] (always 2) and phase of the window center \[UI\].
    pub span_ui: f64,
    pub center_phase: f64,
    pub max_count: u32,
}

impl EyeDiagram {
    /// Counts normalized to \[0, 1\] by the maximum.
    pub fn normalized(&self) -> Vec<f32> {
        let m = self.max_count.max(1) as f32;
        self.counts.iter().map(|&c| c as f32 / m).collect()
    }
}

/// Rasterize the eye.
///
/// * `delay`, `center_phase` – alignment from [`crate::analyze`] (window centered there)
/// * `range` – vertical range; `None` = min/max of the waveform with 5 % margin
pub fn eye_density(
    samples: &[f64],
    sps: usize,
    delay: usize,
    center_phase: f64,
    width: usize,
    height: usize,
    range: Option<(f64, f64)>,
) -> EyeDiagram {
    assert!(width >= 2 && height >= 2);
    let (v_min, v_max) = range.unwrap_or_else(|| {
        let lo = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let m = 0.05 * (hi - lo).max(1e-300);
        (lo - m, hi + m)
    });
    let center_phase = center_phase.rem_euclid(1.0);
    let n = samples.len();
    let mut counts = vec![0u32; width * height];
    let scale = height as f64 / (v_max - v_min);
    let h = height as i64;
    // Every symbol's window starts at an integer sample offset (k·sps + delay), so the
    // per-column integer offsets and interpolation weights are shared by all symbols.
    let pad = 3 * sps + 2;
    assert!(n >= pad + sps, "waveform too short for a 2-UI eye window");
    let mut ext = Vec::with_capacity(n + 2 * pad + sps);
    ext.extend_from_slice(&samples[n - pad..]);
    ext.extend_from_slice(samples);
    ext.extend_from_slice(&samples[..pad + sps]);
    let cols: Vec<(usize, f64)> = (0..width)
        .map(|c| {
            let ph = center_phase - 1.0 + 2.0 * (c as f64 + 0.5) / width as f64;
            let t = ph * sps as f64 + pad as f64;
            let i = t.floor();
            (i as usize, t - i)
        })
        .collect();
    let n_sym = n / sps;
    for k in 0..n_sym {
        let base = (k * sps + delay) % n;
        let seg = &ext[base..base + 2 * pad + sps];
        let mut prev: Option<i64> = None;
        for (col, &(io, u)) in cols.iter().enumerate() {
            let v = seg[io] + u * (seg[io + 1] - seg[io]);
            let r = ((v_max - v) * scale).floor() as i64;
            let (a, b) = match prev {
                // Fill the vertical gap (excluding the previous row, already counted).
                Some(p) if p < r - 1 => (p + 1, r),
                Some(p) if p > r + 1 => (r, p - 1),
                _ => (r, r),
            };
            for row in a.max(0)..=b.min(h - 1) {
                counts[row as usize * width + col] += 1;
            }
            prev = Some(r);
        }
    }
    let max_count = counts.iter().cloned().max().unwrap_or(0);
    EyeDiagram {
        width,
        height,
        counts,
        v_min,
        v_max,
        span_ui: 2.0,
        center_phase,
        max_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_signal_single_row() {
        let x = vec![0.5; 64 * 8];
        let e = eye_density(&x, 8, 0, 0.5, 40, 20, Some((0.0, 1.0)));
        let row = 10; // v = 0.5 → (1 − 0.5)/1 · 20 = 10
        let total: u32 = e.counts.iter().sum();
        let in_row: u32 = e.counts[row * 40..(row + 1) * 40].iter().sum();
        assert_eq!(total, in_row);
        assert_eq!(total, 64 * 40);
    }

    #[test]
    fn transitions_are_continuous() {
        // Alternating 0/1 symbols: every column between plateaus must be hit.
        let sps = 8;
        let x: Vec<f64> = (0..64 * sps).map(|i| ((i / sps) % 2) as f64).collect();
        let e = eye_density(&x, sps, 0, 0.5, 64, 32, Some((-0.1, 1.1)));
        for col in 0..64 {
            let hits: u32 = (0..32).map(|r| e.counts[r * 64 + col]).sum();
            assert!(hits > 0, "column {col} empty");
        }
    }
}

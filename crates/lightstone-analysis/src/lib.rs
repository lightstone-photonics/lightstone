//! # lightstone-analysis
//!
//! Turns waveforms into diagnostics: symbol alignment, eye-density rasterization,
//! PAM-4 level statistics, eye heights/widths, SER/BER estimates and TDECQ.
//!
//! All functions take plain slices so they work on simulated or measured data.

// Index loops mirror the equations (levels k, taps j) and are clearer than iterator chains.
#![allow(clippy::needless_range_loop)]

/// Symbol alignment and interpolation (moved to the core with the FFE; re-exported).
pub use lightstone_core::align;
pub mod eye;
pub mod nodes;
pub mod pam4;
pub mod spectrum;
pub mod stats;
pub mod tdecq;

pub use align::{align, Sampler};
pub use eye::{eye_density, EyeDiagram};
pub use pam4::{
    analyze, analyze_levels, ber_from_ser, infer_levels, AnalysisOptions, Pam4Analysis,
};
pub use tdecq::{tdecq, TdecqOptions, TdecqResult};

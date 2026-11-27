//! # lightstone-core
//!
//! Physics core of the Optical Link Simulator (Lightstone).
//!
//! Reference link: 850 nm VCSEL → connector → OM4 graded-index multimode fiber → PIN/TIA.
//!
//! ## Conventions
//! * All quantities are SI (m, s, Hz, W, A, V). Conversions from engineering units
//!   (dB/km, nm, dBm, ...) happen only in [`units`] helpers and at the API boundary.
//! * All numerics use `f64`.
//! * Waveforms are periodic (an integer number of pattern periods), so all filters are
//!   applied as exact circular convolutions in the frequency domain.
//! * Equation IDs (e.g. `eq:vcsel-response`) refer to `docs/model/*.md`.
//! * "Bin" is always qualified: a *mode bin* ([`signal::ModeBin`], `mode_bins`) is a slot of
//!   the mode dimension of an optical signal; an *FFT bin* is a slot of the frequency axis
//!   (index `k`, frequency [`dsp::freq`]). Histograms use *histogram bins*
//!   (signal-spec 3.3).

pub mod align;
pub mod component;
pub mod components;
pub mod config;
pub mod coupling;
pub mod custom;
pub mod dsp;
pub mod example;
pub mod ffe;
pub mod fiber;
pub mod graph;
pub mod link;
pub mod material;
pub mod modes;
pub mod prbs;
pub mod rng;
pub mod rx;
pub mod seed;
pub mod signal;
pub mod system;
pub mod tx;
pub mod units;
pub mod wdm;

pub use config::LinkConfig;
pub use graph::{GraphConfig, GraphEngine};
pub use link::{LinkResult, Simulator};
pub use system::{LaneConfig, SystemConfig, SystemSimulator};

#[cfg(test)]
mod graph_tests;

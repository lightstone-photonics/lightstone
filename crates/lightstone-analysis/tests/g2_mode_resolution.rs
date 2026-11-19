//! G2: mode-group vs. per-mode resolution on the reference link (docs/architecture.md, 7.5).
//! Reports the TDECQ and SER difference and checks it stays within the stated limit.

use lightstone_analysis::{analyze, tdecq, AnalysisOptions, TdecqOptions};
use lightstone_core::signal::{Level, Resolution};
use lightstone_core::system::{Element, LaneConfig, LaneEngine};

/// Limit on |ΔTDECQ| between resolutions [dB].
const TDECQ_LIMIT_DB: f64 = 0.3;

fn metrics(len: f64, offset: f64, res: Resolution) -> (f64, f64, f64) {
    let mut lane = LaneConfig::default();
    for e in lane.elements.iter_mut() {
        match e {
            Element::Fiber(f) => f.length = len,
            Element::Connector(c) => c.offset = offset,
            _ => {}
        }
    }
    // Noise-free TDECQ isolates the modal effect; RIN would add seed-dependent scatter.
    lane.rx.rin_noise = false;
    let r = LaneEngine::with_representation(lane, res, Level::Power).run();
    let a = analyze(
        &r.rx.signal,
        Some((&r.rx.noiseless, &r.rx.sigma)),
        &r.symbols,
        r.sps,
        &AnalysisOptions::default(),
    );
    let t = tdecq(
        &r.rx.reference_optical,
        &r.symbols,
        r.sps,
        r.baud,
        &TdecqOptions::default(),
    );
    (
        t.tdecq_db.unwrap_or(f64::NAN),
        a.ser.unwrap(),
        r.metrics.rms_delay_spread,
    )
}

#[test]
fn g2_mode_group_vs_mode() {
    println!("case            | TDECQ mode / group / Δ [dB] | SER mode / group      | RMS delay spread mode / group [ps]");
    for (len, off) in [(50.0, 0.0), (100.0, 0.0), (150.0, 0.0), (100.0, 8e-6)] {
        let (tm, sm, dm) = metrics(len, off, Resolution::Mode);
        let (tg, sg, dg) = metrics(len, off, Resolution::ModeGroup);
        println!(
            "{len:>4} m, {:>4.1} µm | {tm:.3} / {tg:.3} / {:+.3}       | {sm:.2e} / {sg:.2e} | {:.2} / {:.2}",
            off * 1e6, tg - tm, dm * 1e12, dg * 1e12
        );
        assert!(
            (tg - tm).abs() < TDECQ_LIMIT_DB,
            "{len} m, {off}: ΔTDECQ {}",
            tg - tm
        );
        assert!(
            dg <= dm * 1.0001,
            "group spread must not exceed per-mode spread"
        );
    }
}

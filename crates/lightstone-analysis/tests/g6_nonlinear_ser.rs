//! G6: a nonlinear block (TIA clipping) switches the SER to Monte Carlo; with clipping
//! effectively disabled (limits far outside the signal) the Monte Carlo SER matches the
//! semi-analytic SER of the linear receiver (docs/architecture.md, 7.5).

use lightstone_analysis::{analyze, AnalysisOptions};
use lightstone_core::component::{Report, Signal};
use lightstone_core::graph::GraphEngine;
use lightstone_core::signal::{Level, MaterializedElectrical, Resolution};
use lightstone_core::system::{lane_graph, Element, LaneConfig};
use serde_json::json;
use std::sync::Arc;

fn run(clip: Option<[f64; 2]>) -> (Arc<MaterializedElectrical>, Vec<u8>, usize) {
    let mut lane = LaneConfig::default();
    lane.tx.sps = 8;
    // 4 pattern periods (32 764 symbols) so that the counted SER is statistically useful.
    lane.tx.pattern_periods = 4;
    // Noise-limited operating point: 50 m and a noisy TIA give SER ≈ 10⁻².
    lane.rx.noise_density = 120e-12;
    for e in lane.elements.iter_mut() {
        if let Element::Fiber(f) = e {
            f.length = 50.0;
            f.grid_points_per_radius = 300;
        }
    }
    let mut g = lane_graph(&lane, Resolution::Mode, Level::Power);
    if let Some(c) = clip {
        let tia = g.nodes.iter_mut().find(|n| n.id == "tia").unwrap();
        tia.params["clip"] = json!(c);
    }
    let mut e = GraphEngine::new(g);
    e.run().unwrap();
    let rec = match e.report("rx") {
        Some(Report::Receiver(m)) => m.clone(),
        _ => panic!("no receiver report"),
    };
    let symbols = match e.output("src", "out") {
        Some(Signal::Data(d)) => d.symbols.as_ref().clone(),
        _ => panic!("no symbols"),
    };
    (rec, symbols, lane.tx.sps)
}

#[test]
fn g6_monte_carlo_ser_after_nonlinear_block() {
    let opts = AnalysisOptions::default();
    let (lin, symbols, sps) = run(None);
    assert!(lin.gaussian_exact);
    let a = analyze(
        &lin.signal,
        Some((&lin.noiseless, &lin.sigma)),
        &symbols,
        sps,
        &opts,
    );
    let semi = a.ser.unwrap();

    // Clipping "disabled": the block is nonlinear, so the SER must be counted.
    let (wide, _, _) = run(Some([-1e3, 1e3]));
    assert!(!wide.gaussian_exact);
    let mc = analyze(&wide.signal, None, &symbols, sps, &opts);
    let n = symbols.len() as f64;
    let stat = (semi * (1.0 - semi) / n).sqrt();
    println!(
        "semi-analytic SER {semi:.4e}, Monte Carlo SER {:.4e} ({} errors, ±{stat:.1e}); \
         counted on the linear receiver {:.4e}",
        mc.ser_counted, mc.symbol_errors, a.ser_counted
    );
    assert!(
        semi > 1e-3 && semi < 1e-1,
        "operating point SER {semi} not resolvable"
    );
    assert!(
        (mc.ser_counted - semi).abs() < 4.0 * stat,
        "Monte Carlo {} vs semi-analytic {semi} (σ_stat {stat:e})",
        mc.ser_counted
    );

    // Active clipping changes the result (here: compresses the outer eye).
    let (hard, _, _) = run(Some([0.30, 0.60]));
    assert!(!hard.gaussian_exact);
    let c = analyze(&hard.signal, None, &symbols, sps, &opts);
    println!(
        "clipped at 0.30…0.60 V: Monte Carlo SER {:.4e}",
        c.ser_counted
    );
    assert!(c.ser_counted != mc.ser_counted);
}

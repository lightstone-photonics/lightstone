//! Spectrum analyzers (docs/model/optical-spectrum-analyzer.md, electrical-spectrum-analyzer.md): the ESA's noise PSD integrates to
//! the mean σ², its signal PSD to the waveform variance (Parseval); the OSA shows each source
//! at its wavelength with its power.

use lightstone_analysis::nodes;
use lightstone_core::component::Report;
use lightstone_core::graph::GraphEngine;
use lightstone_core::signal::{Level, Resolution};
use lightstone_core::system::{lane_graph, LaneConfig};
use serde_json::{json, Value};

fn data(e: &GraphEngine, node: &str) -> Value {
    match e.report(node) {
        Some(Report::Data { value, .. }) => value.as_ref().clone(),
        other => panic!("{node}: no data report: {other:?}"),
    }
}

fn lane() -> LaneConfig {
    let mut l = LaneConfig::default();
    l.tx.sps = 8;
    l
}

#[test]
fn esa_integrates_to_signal_and_noise_power() {
    let mut g = lane_graph(&lane(), Resolution::Mode, Level::Power);
    // Full span so the integrals cover 0 … fs/2.
    g.node(
        "esa",
        "electrical_spectrum_analyzer",
        json!({ "stop_rel": 4.0, "points": 4000 }),
    )
    .wire("rxf.out", "esa.in");
    let mut e = GraphEngine::with_registry(g, nodes::registry());
    e.run().unwrap();
    let v = data(&e, "esa");
    let y = match e.output("rxf", "out") {
        Some(lightstone_core::component::Signal::Electrical(x)) => x.clone(),
        _ => panic!(),
    };
    let sp = e.spectral();
    let m = y.materialize(&sp, e.grid().fs());
    let mean = m.noiseless.iter().sum::<f64>() / m.noiseless.len() as f64;
    let var =
        m.noiseless.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / m.noiseless.len() as f64;
    let s2 = m.sigma.iter().map(|s| s * s).sum::<f64>() / m.sigma.len() as f64;
    let (sr, nr) = (
        v["signal_rms"].as_f64().unwrap(),
        v["noise_rms"].as_f64().unwrap(),
    );
    println!(
        "signal rms {sr:.6} vs {:.6}, noise rms {nr:.6} vs {:.6}, dc {}",
        var.sqrt(),
        s2.sqrt(),
        v["dc"]
    );
    assert!((sr / var.sqrt() - 1.0).abs() < 1e-9);
    assert!((nr / s2.sqrt() - 1.0).abs() < 1e-9);
    assert!((v["dc"].as_f64().unwrap() / mean - 1.0).abs() < 1e-12);
    assert_eq!(v["unit"], "V");
    // Thermal noise alone has the receive filter's shape: exactly −3.01 dB at f_3dB.
    let mut l = lane();
    l.rx.rin_noise = false;
    l.rx.shot_noise = false;
    let mut g = lane_graph(&l, Resolution::Mode, Level::Power);
    let f3 = 0.5 * 53.125e9;
    g.node(
        "esa",
        "electrical_spectrum_analyzer",
        json!({ "stop_rel": 4.0, "points": 4000 }),
    )
    .wire("rxf.out", "esa.in");
    let mut e = GraphEngine::with_registry(g, nodes::registry());
    e.run().unwrap();
    let v = data(&e, "esa");
    let f: Vec<f64> = serde_json::from_value(v["frequency"].clone()).unwrap();
    let nd: Vec<f64> = serde_json::from_value(v["noise_db"].clone()).unwrap();
    let i = f.iter().position(|&q| q >= f3).unwrap();
    let interp = nd[i - 1] + (nd[i] - nd[i - 1]) * (f3 - f[i - 1]) / (f[i] - f[i - 1]);
    let drop = nd[0] - interp;
    // nd[0] is the first display bin (≈ 53 MHz), where |H|² = 1 to 1e-4 dB.
    println!("thermal noise PSD at f_3dB: −{drop:.4} dB");
    assert!((drop - 3.0103).abs() < 0.01, "{drop}");
}

#[test]
fn osa_shows_each_source() {
    let mut g = lane_graph(&lane(), Resolution::Mode, Level::Power);
    g.node("osa", "optical_spectrum_analyzer", json!({}))
        .wire("el1.out", "osa.in");
    let mut e = GraphEngine::with_registry(g, nodes::registry());
    e.run().unwrap();
    let v = data(&e, "osa");
    let pw = match e.output("el1", "out") {
        Some(lightstone_core::component::Signal::Optical(o)) => o.forward.average_power(),
        _ => panic!(),
    };
    assert!((v["total_power"].as_f64().unwrap() / pw - 1.0).abs() < 1e-12);
    let peak = v["peak"]["wavelength"].as_f64().unwrap();
    assert!((peak - 850e-9).abs() <= 0.1e-9, "{peak}");
    // Peak power per RBW: P·(Φ(+½RBW/σ) − Φ(−½RBW/σ)) for σ = 0.4 nm, RBW = 0.1 nm.
    let frac = 1.0 - 2.0 * lightstone_analysis::stats::q_func(0.05 / 0.4);
    let want = 10.0 * (pw * frac / 1e-3).log10();
    let got = v["peak"]["power_dbm"].as_f64().unwrap();
    assert!((got - want).abs() < 1e-6, "{got} vs {want}");
    // The trace integrates to the total power (RBW / point spacing = 0.5).
    let tr: Vec<f64> = serde_json::from_value(v["power_dbm"].clone()).unwrap();
    let sum: f64 = tr.iter().map(|d| 1e-3 * 10f64.powf(d / 10.0)).sum::<f64>() * (0.2 / 0.1);
    assert!((sum / pw - 1.0).abs() < 1e-3, "{}", sum / pw);
}

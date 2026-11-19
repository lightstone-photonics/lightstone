//! Analyzer nodes and the receiver FFE in the graph (docs/architecture.md, 7.7).
//! Each analyzer must reproduce the analysis functions applied to the lane results.

use lightstone_analysis::nodes::{self, design_ffe};
use lightstone_analysis::{analyze, eye_density, tdecq, AnalysisOptions, TdecqOptions};
use lightstone_core::component::{PortSpec, PortType, Report};
use lightstone_core::custom::{register_custom, CustomBehavior, CustomSpec, Domain};
use lightstone_core::graph::{GraphConfig, GraphEngine};
use lightstone_core::signal::{Level, MaterializedElectrical, Resolution};
use lightstone_core::system::{lane_graph, Element, LaneConfig, LaneEngine};
use serde_json::{json, Value};
use std::rc::Rc;
use std::sync::Arc;

/// Lane without the default receiver FFE (these tests insert analyzers and FFEs themselves).
fn lane(len: f64) -> LaneConfig {
    let mut l = LaneConfig::default();
    l.tx.sps = 8;
    l.rx.ffe_taps = 0;
    for e in l.elements.iter_mut() {
        if let Element::Fiber(f) = e {
            f.length = len;
            f.grid_points_per_radius = 300;
        }
    }
    l
}

/// Lane graph plus analyzers: PAM-4 and eye on the receive filter output, TDECQ on the fiber
/// output (`el1`).
fn with_analyzers(l: &LaneConfig) -> GraphConfig {
    let mut g = lane_graph(l, Resolution::Mode, Level::Power);
    g.node("ana", "pam4_analyzer", json!({}))
        .node("eye", "eye_diagram", json!({}))
        .node("tq", "tdecq", json!({}))
        .wire("rxf.out", "ana.in")
        .wire("src.out", "ana.ref")
        .wire("rxf.out", "eye.in")
        .wire("src.out", "eye.ref")
        .wire("el1.out", "tq.in")
        .wire("src.out", "tq.ref");
    g
}

fn run(g: GraphConfig) -> GraphEngine {
    let mut e = GraphEngine::with_registry(g, nodes::registry());
    e.run().unwrap();
    e
}

fn data(e: &GraphEngine, node: &str) -> Value {
    match e.report(node) {
        Some(Report::Data { value, .. }) => value.as_ref().clone(),
        other => panic!("{node}: no data report: {other:?}"),
    }
}

fn rx(e: &GraphEngine) -> Arc<MaterializedElectrical> {
    match e.report("rx") {
        Some(Report::Receiver(m)) => m.clone(),
        _ => panic!("no receiver report"),
    }
}

fn ser(v: &Value) -> f64 {
    v["ser"].as_f64().unwrap()
}

/// The analyzer nodes reproduce analyze(), eye_density() and tdecq() on the lane results.
#[test]
fn analyzers_match_analysis_functions() {
    let l = lane(100.0);
    let e = run(with_analyzers(&l));
    let r = LaneEngine::new(l).run();
    let opts = AnalysisOptions::default();

    let a = analyze(
        &r.rx.signal,
        Some((&r.rx.noiseless, &r.rx.sigma)),
        &r.symbols,
        r.sps,
        &opts,
    );
    let v = data(&e, "ana");
    assert_eq!(v["ser_method"], "semi_analytic");
    assert_eq!(ser(&v), a.ser.unwrap());
    assert_eq!(v["eye_height"], json!(a.eye_height));
    assert_eq!(v["delay"], json!(a.delay));

    let eye = eye_density(&r.rx.signal, r.sps, a.delay, a.phase, 600, 400, None);
    let ev = data(&e, "eye");
    assert_eq!(ev["counts"], json!(eye.counts));
    assert_eq!(ev["max_count"], json!(eye.max_count));

    let t = tdecq(
        &r.rx.reference_optical,
        &r.symbols,
        r.sps,
        r.baud,
        &TdecqOptions::default(),
    );
    let tv = data(&e, "tq");
    let (a_db, b_db) = (tv["tdecq_db"].as_f64().unwrap(), t.tdecq_db.unwrap());
    println!("TDECQ node {a_db:.6} dB, lane {b_db:.6} dB");
    assert!((a_db - b_db).abs() < 1e-9, "{a_db} vs {b_db}");
}

/// After a nonlinear block the PAM-4 analyzer reports the counted (Monte Carlo) SER.
#[test]
fn analyzer_switches_to_monte_carlo() {
    let l = lane(100.0);
    let mut g = with_analyzers(&l);
    g.nodes.iter_mut().find(|n| n.id == "tia").unwrap().params["clip"] = json!([0.3, 0.6]);
    let e = run(g);
    let v = data(&e, "ana");
    assert_eq!(v["ser_method"], "monte_carlo");
    assert_eq!(ser(&v), v["ser_counted"].as_f64().unwrap());
    assert!(e
        .warnings()
        .iter()
        .any(|(n, w)| n == "ana" && w.contains("Monte Carlo")));
}

/// Lane graph with an FFE between the receive filter and the receiver output, plus an analyzer on
/// the FFE output.
fn with_ffe(l: &LaneConfig, params: Value) -> GraphEngine {
    let mut g = lane_graph(l, Resolution::Mode, Level::Power);
    let w = g.wires.iter_mut().find(|w| w.to == "rx.in").unwrap();
    w.from = "ffe.out".into();
    g.node("ffe", "ffe", params)
        .node("ana", "pam4_analyzer", json!({}))
        .wire("rxf.out", "ffe.in")
        .wire("src.out", "ffe.ref")
        .wire("ffe.out", "ana.in")
        .wire("src.out", "ana.ref");
    run(g)
}

/// The adaptive FFE opens the 100 m eye; it stays linear (σ(t) exact, semi-analytic SER).
#[test]
fn adaptive_ffe_opens_the_100m_eye() {
    let l = lane(100.0);
    let plain = data(&run(with_analyzers(&l)), "ana");
    let mut results = vec![];
    for (taps, noise_aware) in [(5, false), (5, true), (9, true)] {
        let e = with_ffe(&l, json!({ "n_taps": taps, "noise_aware": noise_aware }));
        let v = data(&e, "ana");
        let d = data(&e, "ffe");
        assert_eq!(v["ser_method"], "semi_analytic");
        assert!(rx(&e).gaussian_exact);
        let c: Vec<f64> = serde_json::from_value(d["taps"].clone()).unwrap();
        assert!((c.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        println!(
            "{taps} taps, noise-aware {noise_aware}: SER {:.3e} (unequalized {:.3e}), EH {}, taps {:?}",
            ser(&v),
            ser(&plain),
            v["eye_height"],
            c
        );
        results.push(ser(&v));
    }
    assert!(results.iter().all(|s| *s < ser(&plain) / 100.0));
    // Accounting for the noise does not make it worse.
    assert!(results[1] <= results[0] * 1.05, "{results:?}");
}

/// The default lane (`rx.ffe_taps` = 5) is the unequalized lane with a 5-tap FFE inserted
/// before the receiver output, bit for bit; its SER is far below the unequalized one.
#[test]
fn default_lane_has_a_5_tap_ffe() {
    let l0 = lane(100.0);
    let mut l5 = l0.clone();
    l5.rx.ffe_taps = 5;
    let g = lane_graph(&l5, Resolution::Mode, Level::Power);
    assert!(g.nodes.iter().any(|n| n.id == "ffe" && n.kind == "ffe"));
    let lane5 = LaneEngine::new(l5).run();
    let manual = rx(&with_ffe(&l0, json!({ "n_taps": 5 })));
    assert_eq!(lane5.rx.signal, manual.signal);
    assert_eq!(lane5.rx.sigma, manual.sigma);
    let unequalized = LaneEngine::new(l0).run();
    let a = |r: &lightstone_core::link::LinkResult| {
        analyze(
            &r.rx.signal,
            Some((&r.rx.noiseless, &r.rx.sigma)),
            &r.symbols,
            r.sps,
            &AnalysisOptions::default(),
        )
        .ser
        .unwrap()
    };
    let (s5, s0) = (a(&lane5), a(&unequalized));
    println!("SER with FFE {s5:.3e}, without {s0:.3e}");
    assert!(s5 < s0 / 1e4);
}

/// Fixed taps: [1] is the identity; the reference 5-tap design equals the TDECQ FFE design.
#[test]
fn fixed_taps_and_consistency_with_tdecq_ffe() {
    let l = lane(100.0);
    let base = rx(&run(with_analyzers(&l)));
    let one = rx(&with_ffe(&l, json!({ "taps": [1.0] })));
    assert_eq!(one.signal, base.signal);
    assert_eq!(one.sigma, base.sigma);

    // Same constrained MMSE problem as the TDECQ reference equalizer (5 taps, main cursor 2):
    // compare with the taps the TDECQ routine reports at its phase.
    let r = LaneEngine::new(l).run();
    let t = tdecq(
        &r.rx.reference_optical,
        &r.symbols,
        r.sps,
        r.baud,
        &TdecqOptions::default(),
    );
    let s = lightstone_analysis::Sampler::new(&r.rx.reference_optical, r.sps, t.delay);
    let y = s.at_phase(t.phase);
    let target: Vec<f64> = r
        .symbols
        .iter()
        .map(|&k| t.p_ave + t.oma_outer * (k as f64 - 1.5) / 3.0)
        .collect();
    let (c, _) = design_ffe(&y, &target, 5, 2, &[]);
    for (a, b) in c.iter().zip(&t.taps) {
        assert!((a - b).abs() < 1e-9, "{c:?} vs {:?}", t.taps);
    }
}

#[test]
fn node_checks() {
    // The reference input must be connected.
    let mut g = lane_graph(&lane(50.0), Resolution::Mode, Level::Power);
    g.node("ana", "pam4_analyzer", json!({}))
        .wire("rxf.out", "ana.in");
    let issues = GraphEngine::with_registry(g, nodes::registry()).validate();
    assert!(issues
        .iter()
        .any(|i| i.message.contains("'ref' is not connected")));
    // Bad FFE parameters.
    let mut g = GraphConfig::default();
    g.node("f", "ffe", json!({ "n_taps": 3, "main": 5 }));
    let issues = GraphEngine::with_registry(g, nodes::registry()).validate();
    assert!(issues[0].message.contains("main cursor"), "{issues:?}");
    // Analysis kinds are built-in: a custom component cannot take their name.
    struct Nop;
    impl CustomBehavior for Nop {}
    let el = PortType::Electrical(None);
    let spec = CustomSpec {
        inputs: vec![PortSpec::new("in", el)],
        outputs: vec![PortSpec::new("out", el)],
        linear: true,
        domain: Domain::Frequency,
        doc: String::new(),
    };
    let mut r = nodes::registry();
    let err = register_custom(&mut r, "ffe", spec, Rc::new(Nop)).unwrap_err();
    assert!(err.contains("built-in"));
}

/// Every built-in kind has default parameters that the engine accepts (editors start new
/// nodes with them), and editor layout data never triggers a recomputation.
#[test]
fn default_params_and_layout() {
    let reg = nodes::registry();
    let settings = lightstone_core::signal::Settings::default();
    for kind in reg.builtin_kinds() {
        let p = nodes::default_params(&kind).unwrap_or_else(|| panic!("{kind}: no defaults"));
        let info = lightstone_core::graph::NodeInfo {
            id: "n",
            settings: &settings,
        };
        assert!(
            reg.create(&kind, &p, &info).is_ok(),
            "{kind}: defaults rejected"
        );
    }
    assert!(nodes::default_params("teleporter").is_none());

    let mut g = with_analyzers(&lane(50.0));
    let mut e = GraphEngine::with_registry(g.clone(), nodes::registry());
    e.run().unwrap();
    g.layout = Some(json!({ "positions": { "src": [0, 0], "fib": [300, 40] } }));
    e.set_graph(g.clone());
    e.run().unwrap();
    assert_eq!(e.computed_last_run(), 0);
    let back: GraphConfig = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
    assert_eq!(back.layout, g.layout);
}

/// NRZ (`levels: 2`): the pattern is PRBS13, the VCSEL reports two power levels with the
/// same outer levels as PAM-4, the analyzer gives one eye with BER = SER, the FFE adapts,
/// and TDECQ refuses NRZ.
#[test]
fn nrz_link() {
    let mut lane = lane(100.0);
    lane.tx.levels = 2;
    lane.tx.baud = 10.3125e9;
    let mut g = lane_graph(&lane, Resolution::Mode, Level::Power);
    g.node("ana", "pam4_analyzer", json!({}))
        .wire("rxf.out", "ana.in")
        .wire("src.out", "ana.ref")
        .node("eq", "ffe", json!({}))
        .wire("rxf.out", "eq.in")
        .wire("src.out", "eq.ref")
        .node("ana2", "pam4_analyzer", json!({}))
        .wire("eq.out", "ana2.in")
        .wire("src.out", "ana2.ref");
    let e = run(g.clone());
    let Some(lightstone_core::component::Signal::Data(d)) = e.output("src", "out") else {
        panic!("no data")
    };
    assert_eq!(d.levels, 2);
    assert_eq!(
        &d.symbols[..8191],
        &lightstone_core::prbs::prbs13(8191, lane.tx.prbs_seed)[..]
    );
    let Some(Report::Transmitter { tx }) = e.report("vcsel") else {
        panic!("no tx report")
    };
    let pam = lightstone_core::tx::power_levels(1e-3, lightstone_core::units::db_to_lin(4.0));
    assert_eq!(tx.levels.len(), 2);
    assert_eq!(tx.levels[0], pam[0]);
    assert_eq!(tx.levels[1], pam[3]);
    let a = data(&e, "ana");
    assert_eq!(a["thresholds"].as_array().unwrap().len(), 1);
    let ser = a["ser"].as_f64().unwrap();
    assert_eq!(a["ber"].as_f64().unwrap(), ser);
    assert!(
        ser < 1e-9,
        "a 10 GBd NRZ link over 100 m OM4 is error free: SER {ser:e}"
    );
    // A 0 means "below the floor" when there is noise.
    let floor = a["ser_floor"].as_f64().unwrap();
    assert!(floor > 2e-19 && floor < 2.5e-19, "{floor:e}");
    assert!(a["noise_rms"].as_f64().unwrap() > 0.0);
    let eh = a["eye_height"][0].as_f64().unwrap();
    assert!(eh > 0.0, "open eye {eh}");
    assert!(data(&e, "eq")["adapted"].as_bool().unwrap());

    g.node("td", "tdecq", json!({}))
        .wire("el1.out", "td.in")
        .wire("src.out", "td.ref");
    let mut eng = GraphEngine::with_registry(g, nodes::registry());
    let err = eng.run().unwrap_err();
    assert!(
        err.0.iter().any(|i| i.message.contains("PAM-4 only")),
        "{:?}",
        err.0
    );
}

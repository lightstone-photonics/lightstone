//! Graph engine tests (docs/architecture.md, 7.5, signal-spec 8.6 / 9.7 / 11.3).

use crate::component::{Report, Signal};
use crate::graph::{GraphConfig, GraphEngine};
use crate::signal::{Level, Resolution};
use crate::system::{element_node, lane_graph, LaneConfig, LaneEngine};
use crate::units::db_to_lin;
use serde_json::json;

fn rel_max(a: &[f64], b: &[f64]) -> f64 {
    let s = b.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    a.iter()
        .zip(b)
        .fold(0.0f64, |m, (x, y)| m.max((x - y).abs()))
        / s
}

/// Reference lane at 8 samples/UI without the receiver FFE (engine-mechanics tests).
fn fast_lane() -> LaneConfig {
    let mut l = LaneConfig::default();
    l.tx.sps = 8;
    l.rx.ffe_taps = 0;
    for e in l.elements.iter_mut() {
        if let crate::system::Element::Fiber(f) = e {
            f.grid_points_per_radius = 300;
        }
    }
    l
}

/// Transmitter + connector + 100 m fiber, ending at node `fib.out`.
fn tx_fiber(g: &mut GraphConfig, fiber_params: serde_json::Value) {
    g.settings.sps = 8;
    g.node("src", "prbs13q", json!({}))
        .node("drv", "driver", json!({}))
        .node("vcsel", "vcsel", json!({}))
        .node("con", "connector", json!({}))
        .node("fib", "custom_fiber", fiber_params)
        .wire("src.out", "drv.in")
        .wire("drv.out", "vcsel.in")
        .wire("vcsel.out", "con.in")
        .wire("con.out", "fib.in");
}

fn receiver(g: &mut GraphConfig, from: &str) {
    g.node("pin", "pin", json!({}))
        .node("tia", "tia", json!({}))
        .node("rxf", "bessel_thomson", json!({}))
        .node("rx", "receiver_output", json!({}))
        .wire(from, "pin.in")
        .wire("pin.out", "tia.in")
        .wire("tia.out", "rxf.in")
        .wire("rxf.out", "rx.in");
}

fn fiber100() -> serde_json::Value {
    json!({ "length": 100.0, "grid_points_per_radius": 300 })
}

fn meter(e: &GraphEngine, node: &str) -> f64 {
    match e.report(node) {
        Some(Report::PowerMeter { average_power }) => *average_power,
        other => panic!("{node}: no power meter report: {other:?}"),
    }
}

fn receiver_out(e: &GraphEngine) -> std::sync::Arc<crate::signal::MaterializedElectrical> {
    match e.report("rx") {
        Some(Report::Receiver(m)) => m.clone(),
        other => panic!("no receiver report: {other:?}"),
    }
}

#[test]
fn graph_json_roundtrip() {
    let g = lane_graph(&LaneConfig::default(), Resolution::ModeGroup, Level::Stokes);
    let s = serde_json::to_string(&g).unwrap();
    let back: GraphConfig = serde_json::from_str(&s).unwrap();
    assert_eq!(g, back);
    assert!(s.contains("\"resolution\":\"mode_group\""), "{s}");
}

/// G3: splitter conserves energy (outputs = (1 − excess loss) × input).
#[test]
fn g3_splitter_energy() {
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    g.node("pm_in", "power_meter", json!({}))
        .node(
            "spl",
            "splitter",
            json!({ "ratios": [0.3, 0.6], "excess_loss_db": 0.5 }),
        )
        .node("pm0", "power_meter", json!({}))
        .node("pm1", "power_meter", json!({}))
        .wire("fib.out", "pm_in.in")
        .wire("fib.out", "spl.in")
        .wire("spl.out0", "pm0.in")
        .wire("spl.out1", "pm1.in");
    let mut e = GraphEngine::new(g);
    e.run().unwrap();
    let (pin, p0, p1) = (meter(&e, "pm_in"), meter(&e, "pm0"), meter(&e, "pm1"));
    let expect = 0.9 * db_to_lin(-0.5) * pin;
    assert!(
        ((p0 + p1) / expect - 1.0).abs() < 1e-12,
        "{p0} + {p1} vs {expect}"
    );
    assert!((p0 / p1 - 0.5).abs() < 1e-12);
}

/// G4: an ideal probe leaves the signal bit-identical; a realistic probe reduces it by
/// exactly its through transmission, and the tap carries the tap ratio.
#[test]
fn g4_probe() {
    let build = |probe: Option<serde_json::Value>| {
        let mut g = GraphConfig::default();
        tx_fiber(&mut g, fiber100());
        match probe {
            None => receiver(&mut g, "fib.out"),
            Some(p) => {
                g.node("prb", "probe", p)
                    .node("pm", "power_meter", json!({}))
                    .wire("fib.out", "prb.in")
                    .wire("prb.tap", "pm.in");
                receiver(&mut g, "prb.out");
            }
        }
        let mut e = GraphEngine::new(g);
        e.run().unwrap();
        e
    };
    let plain = build(None);
    let ideal = build(Some(json!({})));
    assert_eq!(receiver_out(&plain).signal, receiver_out(&ideal).signal);
    assert_eq!(receiver_out(&plain).sigma, receiver_out(&ideal).sigma);

    let real = build(Some(
        json!({ "realistic": true, "tap_ratio": 0.05, "insertion_loss_db": 0.3 }),
    ));
    let through = 0.95 * db_to_lin(-0.3);
    let det = |e: &GraphEngine| match e.report("pin") {
        Some(Report::Detection(d)) => d.channels[0].transmission,
        _ => panic!(),
    };
    assert!((det(&real) / det(&plain) / through - 1.0).abs() < 1e-12);
    let tap = meter(&real, "pm");
    let full = meter(&ideal, "pm");
    assert!((tap / full / 0.05 - 1.0).abs() < 1e-12);
}

/// P1 (`Stokes` = `Power`, bit-identical) and F1 (`Field` = `Power` to rounding).
#[test]
fn p1_f1_representation_levels() {
    let run = |level| LaneEngine::with_representation(fast_lane(), Resolution::Mode, level).run();
    let p = run(Level::Power);
    let s = run(Level::Stokes);
    let f = run(Level::Field);
    assert_eq!(p.rx.signal, s.rx.signal);
    assert_eq!(p.rx.sigma, s.rx.sigma);
    assert!(rel_max(&f.rx.noiseless, &p.rx.noiseless) < 1e-12);
    assert!(rel_max(&f.rx.sigma, &p.rx.sigma) < 1e-12);
    assert!(rel_max(&f.rx.signal, &p.rx.signal) < 1e-12);
}

/// B1: with backward waves enabled (all empty today) results are identical.
#[test]
fn b1_backward_waves_reserved() {
    let mut g = lane_graph(&fast_lane(), Resolution::Mode, Level::Power);
    let mut a = GraphEngine::new(g.clone());
    a.run().unwrap();
    g.settings.backward_waves = true;
    let mut b = GraphEngine::new(g);
    b.run().unwrap();
    assert_eq!(receiver_out(&a).signal, receiver_out(&b).signal);
    match a.output("pin", "out") {
        Some(Signal::Electrical(_)) => {}
        _ => panic!(),
    }
    if let Some(Signal::Optical(w)) = b.output(&element_node(1), "out") {
        assert!(w.backward.is_none());
    } else {
        panic!("fiber output missing");
    }
}

#[test]
fn validation_rules() {
    let issues = |g: GraphConfig| GraphEngine::new(g).validate();
    // Unknown kind.
    let mut g = GraphConfig::default();
    g.node("x", "teleporter", json!({}));
    assert!(issues(g)[0].message.contains("unknown component kind"));
    // Type mismatch: data into an optical input.
    let mut g = GraphConfig::default();
    g.node("src", "prbs13q", json!({}))
        .node("att", "attenuator", json!({}))
        .wire("src.out", "att.in");
    assert!(issues(g)
        .iter()
        .any(|i| i.message.contains("cannot connect data")));
    // Unconnected input.
    let mut g = GraphConfig::default();
    g.node("att", "attenuator", json!({}));
    assert!(issues(g)
        .iter()
        .any(|i| i.message.contains("not connected")));
    // Loop.
    let mut g = GraphConfig::default();
    g.node("a", "attenuator", json!({}))
        .node("b", "attenuator", json!({}))
        .wire("a.out", "b.in")
        .wire("b.out", "a.in");
    assert!(issues(g).iter().any(|i| i.message.contains("loop")));
    // Optical fan-out to two components needs a splitter …
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    g.node("a", "attenuator", json!({}))
        .node("b", "attenuator", json!({}))
        .node("pa", "power_meter", json!({}))
        .node("pb", "power_meter", json!({}))
        .wire("fib.out", "a.in")
        .wire("fib.out", "b.in")
        .wire("a.out", "pa.in")
        .wire("b.out", "pb.in");
    assert!(issues(g)
        .iter()
        .any(|i| i.message.contains("Insert a splitter")));
    // … but a monitor tap is fine, and electrical outputs fan out freely.
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    receiver(&mut g, "fib.out");
    g.node("pm", "power_meter", json!({}))
        .node("dec2", "receiver_output", json!({}))
        .wire("fib.out", "pm.in")
        .wire("rxf.out", "dec2.in");
    assert!(issues(g).is_empty());
}

#[test]
fn offset_connector_between_fibers_is_an_error() {
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    g.node("c2", "connector", json!({ "offset": 3e-6 }))
        .node("pm", "power_meter", json!({}))
        .wire("fib.out", "c2.in")
        .wire("c2.out", "pm.in");
    let err = GraphEngine::new(g).run().unwrap_err();
    assert!(err.to_string().contains("mode coupling"), "{err}");
}

/// Only nodes downstream of a change are recomputed.
#[test]
fn caching_recomputes_downstream_only() {
    let mut lane = fast_lane();
    let mut e = GraphEngine::new(lane_graph(&lane, Resolution::Mode, Level::Power));
    e.run().unwrap();
    let all = e.computed_last_run();
    assert_eq!(all, 9);
    e.run().unwrap();
    assert_eq!(e.computed_last_run(), 0);
    if let crate::system::Element::Fiber(f) = &mut lane.elements[1] {
        f.length = 50.0;
    }
    e.set_graph(lane_graph(&lane, Resolution::Mode, Level::Power));
    e.run().unwrap();
    // fiber, pin, tia, filter, receiver output
    assert_eq!(e.computed_last_run(), 5);
}

/// Two VCSELs combined into one fiber add incoherently at the detector.
#[test]
fn combiner_adds_sources_incoherently() {
    let base = |g: &mut GraphConfig, k: &str, wl: f64, seed: u64| {
        g.node(
            &format!("src{k}"),
            "prbs13q",
            json!({ "seed": 0x0AAB + seed }),
        )
        .node(&format!("drv{k}"), "driver", json!({}))
        .node(
            &format!("v{k}"),
            "vcsel",
            json!({ "vcsel": { "wavelength": wl }, "seed": seed }),
        )
        .wire(&format!("src{k}.out"), &format!("drv{k}.in"))
        .wire(&format!("drv{k}.out"), &format!("v{k}.in"));
    };
    let detected = |g: GraphConfig| {
        let mut e = GraphEngine::new(g);
        e.run().unwrap();
        match e.report("pin") {
            Some(Report::Detection(d)) => (
                d.channels.len(),
                d.p_det.iter().sum::<f64>() / d.p_det.len() as f64,
            ),
            _ => panic!(),
        }
    };
    let single = |k: &str, wl: f64, seed: u64| {
        let mut g = GraphConfig::default();
        g.settings.sps = 8;
        base(&mut g, k, wl, seed);
        g.node("fib", "custom_fiber", fiber100())
            .wire(&format!("v{k}.out"), "fib.in");
        receiver(&mut g, "fib.out");
        detected(g).1
    };
    let mut g = GraphConfig::default();
    g.settings.sps = 8;
    base(&mut g, "a", 850e-9, 1);
    base(&mut g, "b", 880e-9, 2);
    g.node("cmb", "combiner", json!({}))
        .node("fib", "custom_fiber", fiber100())
        .wire("va.out", "cmb.in0")
        .wire("vb.out", "cmb.in1")
        .wire("cmb.out", "fib.in");
    receiver(&mut g, "fib.out");
    let (n, both) = detected(g);
    assert_eq!(n, 2);
    let sum = single("a", 850e-9, 1) + single("b", 880e-9, 2);
    assert!((both / sum - 1.0).abs() < 1e-12, "{both} vs {sum}");
}

/// Partial-coherence validity check: warns only with a finite detector and coherent modes.
#[test]
fn coherence_check() {
    let warn = |rd: Option<f64>, width: f64| {
        let mut l = fast_lane();
        l.rx.detector_radius = rd;
        l.vcsel.spectral_width_rms = width;
        let mut e = LaneEngine::new(l);
        e.run();
        !e.graph_engine().warnings().is_empty()
    };
    assert!(!warn(None, 1e-15));
    assert!(warn(Some(15e-6), 1e-15));
}

/// Materialized per-mode-bin waveforms sum to the lazily computed detected power.
#[test]
fn materialize_matches_lazy() {
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    receiver(&mut g, "fib.out");
    let mut e = GraphEngine::new(g);
    e.run().unwrap();
    let grid = e.grid();
    let sp = e.spectral();
    let Some(Signal::Optical(w)) = e.output("fib", "out") else {
        panic!()
    };
    let m = w.forward.contributions[0].materialize(&grid, &sp);
    let mut sum = vec![0.0; grid.n()];
    for b in &m.mode_bins {
        sum.iter_mut().zip(b).for_each(|(s, x)| *s += x);
    }
    let Some(Report::Detection(d)) = e.report("pin") else {
        panic!()
    };
    assert!(rel_max(&sum, &d.p_det) < 1e-10);
}

/// Mode-group mode bins conserve power and give a similar delay spread.
#[test]
fn mode_group_resolution_conserves_power() {
    let m = LaneEngine::with_representation(fast_lane(), Resolution::Mode, Level::Power).run();
    let g = LaneEngine::with_representation(fast_lane(), Resolution::ModeGroup, Level::Power).run();
    assert!((m.metrics.transmission / g.metrics.transmission - 1.0).abs() < 1e-12);
    let (a, b) = (m.metrics.rms_delay_spread, g.metrics.rms_delay_spread);
    assert!(b <= a * 1.0001 && b > 0.5 * a, "mode {a:e} group {b:e}");
}

// ---------------------------------------------------------------------------------------------
// Step 2: engine-applied H(f), custom components, nonlinear blocks
// ---------------------------------------------------------------------------------------------

mod step2 {
    use super::*;
    use crate::component::{check_transfer, PortSpec, PortType};
    use crate::custom::{
        register_custom, transfer_from_impulse, CustomBehavior, CustomCtx, CustomSpec, Domain,
        ImpulseResponse, TapSpacing, Waveform,
    };
    use crate::dsp::BesselThomson4;
    use crate::signal::Quantity;
    use num_complex::Complex64;
    use std::rc::Rc;

    struct BtBehavior;
    impl CustomBehavior for BtBehavior {
        fn transfer(&self, ctx: &CustomCtx, f: &[f64]) -> Result<Vec<Complex64>, String> {
            let rel = ctx.params["f3db_rel"].as_f64().unwrap_or(0.5);
            let bt = BesselThomson4::new(rel * ctx.grid.baud);
            Ok(f.iter().map(|&x| bt.response(x)).collect())
        }
    }

    struct Gain(f64);
    impl CustomBehavior for Gain {
        fn transfer(&self, _: &CustomCtx, f: &[f64]) -> Result<Vec<Complex64>, String> {
            Ok(vec![Complex64::new(self.0, 0.0); f.len()])
        }
    }

    struct Clipper(f64, f64);
    impl CustomBehavior for Clipper {
        fn process(&self, _: &CustomCtx, inputs: Vec<Waveform>) -> Result<Vec<Waveform>, String> {
            let clip = |v: Vec<f64>| v.into_iter().map(|x| x.clamp(self.0, self.1)).collect();
            match inputs.into_iter().next() {
                Some(Waveform::Electrical {
                    quantity,
                    noiseless,
                    noisy,
                    sigma,
                    gaussian_exact,
                }) => Ok(vec![Waveform::Electrical {
                    quantity,
                    noiseless: clip(noiseless),
                    noisy: clip(noisy),
                    sigma,
                    gaussian_exact,
                }]),
                _ => Err("expected an electrical input".into()),
            }
        }
    }

    fn spec(ty: PortType, linear: bool) -> CustomSpec {
        CustomSpec {
            inputs: vec![PortSpec::new("in", ty)],
            outputs: vec![PortSpec::new("out", ty)],
            linear,
            domain: if linear {
                Domain::Frequency
            } else {
                Domain::Time
            },
            doc: String::new(),
        }
    }

    fn engine(g: GraphConfig) -> GraphEngine {
        let mut e = GraphEngine::new(g);
        let r = e.registry_mut();
        let el = PortType::Electrical(None);
        register_custom(r, "my_bt", spec(el, true), Rc::new(BtBehavior)).unwrap();
        register_custom(
            r,
            "my_att",
            spec(PortType::Optical, true),
            Rc::new(Gain(0.5)),
        )
        .unwrap();
        register_custom(r, "my_clip", spec(el, false), Rc::new(Clipper(0.3, 0.6))).unwrap();
        register_custom(r, "wide_clip", spec(el, false), Rc::new(Clipper(-1e3, 1e3))).unwrap();
        e.run().unwrap();
        e
    }

    /// Lane graph with node `id` replaced by `kind` (same id, same wires).
    fn lane_with(id: &str, kind: &str, params: serde_json::Value) -> GraphConfig {
        let mut g = lane_graph(&fast_lane(), Resolution::Mode, Level::Power);
        let n = g.nodes.iter_mut().find(|n| n.id == id).unwrap();
        n.kind = kind.into();
        n.params = params;
        g
    }

    /// Lane graph with an extra SISO node `id` of `kind` inserted before node `before`.
    fn lane_insert(before: &str, id: &str, kind: &str, params: serde_json::Value) -> GraphConfig {
        let mut g = lane_graph(&fast_lane(), Resolution::Mode, Level::Power);
        let w = g
            .wires
            .iter_mut()
            .find(|w| w.to == format!("{before}.in"))
            .unwrap();
        let from = std::mem::replace(&mut w.from, format!("{id}.out"));
        g.node(id, kind, params).wire(&from, &format!("{id}.in"));
        g
    }

    fn assert_close(a: &[f64], b: &[f64], tol: f64, what: &str) {
        let r = rel_max(a, b);
        assert!(r < tol, "{what}: relative difference {r:e}");
    }

    /// A linear custom electrical component with H(f) is applied by the engine and matches the
    /// built-in Bessel–Thomson filter, including the noise (σ and realization).
    #[test]
    fn linear_custom_electrical_matches_builtin() {
        let base = engine(lane_graph(&fast_lane(), Resolution::Mode, Level::Power));
        let custom = engine(lane_with("rxf", "my_bt", json!({ "f3db_rel": 0.5 })));
        let (a, b) = (receiver_out(&base), receiver_out(&custom));
        assert!(b.gaussian_exact);
        assert_close(&b.noiseless, &a.noiseless, 1e-12, "noiseless");
        assert_close(&b.sigma, &a.sigma, 1e-12, "sigma");
        assert_close(&b.signal, &a.signal, 1e-12, "signal");
    }

    /// A linear custom optical component (H = T) matches the built-in attenuator, at the
    /// receiver and at a power meter.
    #[test]
    fn linear_custom_optical_matches_attenuator() {
        let att = |kind: &str| {
            let mut g = lane_insert(
                "pin",
                "att",
                kind,
                json!({ "loss_db": 10.0 * 2f64.log10() }),
            );
            g.node("pm", "power_meter", json!({}))
                .wire("att.out", "pm.in");
            engine(g)
        };
        let (a, b) = (att("attenuator"), att("my_att"));
        let (ra, rb) = (receiver_out(&a), receiver_out(&b));
        assert_close(&rb.noiseless, &ra.noiseless, 1e-12, "noiseless");
        assert_close(&rb.sigma, &ra.sigma, 1e-12, "sigma");
        assert_close(&rb.signal, &ra.signal, 1e-12, "signal");
        assert!((meter(&b, "pm") / meter(&a, "pm") - 1.0).abs() < 1e-12);
    }

    /// TIA clipping: a nonlinear block. With limits far outside the signal it reproduces the
    /// linear TIA to rounding but flags the noise as non-Gaussian; with active limits the
    /// waveform is clipped. A custom nonlinear clipper after a linear TIA gives the same result
    /// as the built-in clipping TIA.
    #[test]
    fn tia_clipping_is_nonlinear() {
        let lin = engine(lane_graph(&fast_lane(), Resolution::Mode, Level::Power));
        let tia = |clip: [f64; 2]| {
            let mut g = lane_graph(&fast_lane(), Resolution::Mode, Level::Power);
            let n = g.nodes.iter_mut().find(|n| n.id == "tia").unwrap();
            n.params["clip"] = json!(clip);
            engine(g)
        };
        let wide = tia([-1e3, 1e3]);
        let (a, b) = (receiver_out(&lin), receiver_out(&wide));
        assert!(a.gaussian_exact && !b.gaussian_exact);
        // Equal to rounding except in the Nyquist FFT bin (filters are complex at +fs/2, and
        // materializing before the receive filter drops that FFT bin's imaginary part).
        assert_close(&b.noiseless, &a.noiseless, 1e-9, "noiseless");
        assert_close(&b.signal, &a.signal, 1e-9, "signal");
        assert!(wide
            .warnings()
            .iter()
            .any(|(n, w)| n == "rx" && w.contains("Monte Carlo")));
        assert!(lin.warnings().is_empty());

        let hard = tia([0.3, 0.6]);
        let custom = engine(lane_insert("rxf", "clp", "my_clip", json!({})));
        let (h, c) = (receiver_out(&hard), receiver_out(&custom));
        assert_eq!(h.signal, c.signal);
        assert_eq!(h.noiseless, c.noiseless);
        assert!(hard
            .warnings()
            .iter()
            .any(|(n, w)| n == "tia" && w.contains("clipped")));
        // Clipping limits the swing to 0.3 V; the Bessel–Thomson filter after it adds its
        // step-response overshoot (≈ 0.9 %).
        let swing = c.noiseless.iter().cloned().fold(f64::MIN, f64::max)
            - c.noiseless.iter().cloned().fold(f64::MAX, f64::min);
        assert!(swing < 0.3 * 1.02, "swing {swing}");

        let wide_custom = engine(lane_insert("rxf", "clp", "wide_clip", json!({})));
        assert_close(
            &receiver_out(&wide_custom).signal,
            &a.signal,
            1e-9,
            "wide custom",
        );
    }

    #[test]
    fn transfer_and_spec_checks() {
        let n = 8;
        let mut h = vec![Complex64::new(1.0, 0.0); n];
        assert!(check_transfer(&h, n).is_ok());
        h[1] = Complex64::new(1.0, 0.5); // H(−f) ≠ H(f)*
        assert!(check_transfer(&h, n)
            .unwrap_err()
            .contains("real impulse response"));
        assert!(check_transfer(&h[..4], n).is_err());

        let mut r = crate::graph::Registry::builtin();
        let opt = PortType::Optical;
        let err = register_custom(&mut r, "x", spec(opt, false), Rc::new(Gain(1.0)));
        assert!(err.unwrap_err().contains("not supported yet"));
        let err = register_custom(&mut r, "tia", spec(opt, true), Rc::new(Gain(1.0)));
        assert!(err.unwrap_err().contains("built-in"));
        let mixed = CustomSpec {
            inputs: vec![PortSpec::new("in", opt)],
            outputs: vec![PortSpec::new(
                "out",
                PortType::Electrical(Some(Quantity::Current)),
            )],
            linear: true,
            domain: Domain::Frequency,
            doc: String::new(),
        };
        assert!(register_custom(&mut r, "x", mixed, Rc::new(Gain(1.0))).is_err());
        // A nonlinear component cannot be described by H(f).
        let mut nl = spec(PortType::Electrical(None), false);
        nl.domain = Domain::Frequency;
        let err = register_custom(&mut r, "x", nl, Rc::new(Gain(1.0)));
        assert!(err.unwrap_err().contains("time domain"));

        // Wrong-length transfer function is reported at the node.
        struct Short;
        impl CustomBehavior for Short {
            fn transfer(&self, _: &CustomCtx, _: &[f64]) -> Result<Vec<Complex64>, String> {
                Ok(vec![Complex64::new(1.0, 0.0); 3])
            }
        }
        let mut e = GraphEngine::new(lane_with("rxf", "short", json!({})));
        let el = PortType::Electrical(None);
        register_custom(e.registry_mut(), "short", spec(el, true), Rc::new(Short)).unwrap();
        let err = e.run().unwrap_err();
        assert_eq!(err.0[0].node.as_deref(), Some("rxf"));
        assert!(err.to_string().contains("simulation grid"), "{err}");
    }

    // -- linear components in the time domain (impulse response, FIR taps) -----------------

    /// Gaussian impulse response, h(t) = exp(−t²/2σ²)/(√(2π)σ), ∫h dt = 1.
    struct GaussT;
    impl CustomBehavior for GaussT {
        fn impulse(&self, ctx: &CustomCtx, t: &[f64]) -> Result<ImpulseResponse, String> {
            let s = ctx.params["sigma"].as_f64().unwrap();
            let a = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * s);
            Ok(ImpulseResponse::Continuous(
                t.iter()
                    .map(|t| a * (-t * t / (2.0 * s * s)).exp())
                    .collect(),
            ))
        }
    }

    /// The same filter in the frequency domain, H(f) = exp(−2π²σ²f²).
    struct GaussF;
    impl CustomBehavior for GaussF {
        fn transfer(&self, ctx: &CustomCtx, f: &[f64]) -> Result<Vec<Complex64>, String> {
            let s = ctx.params["sigma"].as_f64().unwrap();
            let k = 2.0 * std::f64::consts::PI.powi(2) * s * s;
            Ok(f.iter()
                .map(|f| Complex64::new((-k * f * f).exp(), 0.0))
                .collect())
        }
    }

    struct Fir(Vec<f64>, TapSpacing);
    impl CustomBehavior for Fir {
        fn impulse(&self, _: &CustomCtx, _: &[f64]) -> Result<ImpulseResponse, String> {
            Ok(ImpulseResponse::Taps {
                taps: self.0.clone(),
                spacing: self.1,
            })
        }
    }

    fn time_spec() -> CustomSpec {
        let el = PortType::Electrical(None);
        CustomSpec {
            domain: Domain::Time,
            ..spec(el, true)
        }
    }

    fn grid8() -> (crate::signal::TimeGrid, crate::dsp::Spectral) {
        let s = crate::signal::Settings {
            sps: 8,
            ..Default::default()
        };
        let g = crate::signal::TimeGrid::new(&s);
        (g, crate::dsp::Spectral::new(g.n()))
    }

    /// `eq:custom-impulse`: a sampled Gaussian h(t) gives the analytic H(f); taps give their
    /// DFT (identity, one-sample delay, symbol-spaced FIR).
    #[test]
    fn impulse_to_transfer_is_exact() {
        let (g, sp) = grid8();
        let (n, fs) = (g.n(), g.fs());
        let sigma = 5e-12;
        let a = 1.0 / ((2.0 * std::f64::consts::PI).sqrt() * sigma);
        let h: Vec<f64> = (0..n)
            .map(|k| {
                let t = crate::dsp::time(k, n, fs);
                a * (-t * t / (2.0 * sigma * sigma)).exp()
            })
            .collect();
        let (hf, w) = transfer_from_impulse(&ImpulseResponse::Continuous(h), &g, &sp).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let kk = 2.0 * std::f64::consts::PI.powi(2) * sigma * sigma;
        // Sampling aliases: the DFT of the sampled h equals Σₘ H(f − m·fs) exactly.
        let err = (0..n)
            .map(|k| {
                let f = crate::dsp::freq(k, n, fs);
                let exact: f64 = (-2..=2)
                    .map(|m| (-kk * (f - m as f64 * fs).powi(2)).exp())
                    .sum();
                (hf[k] - Complex64::new(exact, 0.0)).norm()
            })
            .fold(0.0, f64::max);
        assert!(err < 1e-12, "Gaussian: max |ΔH| = {err:e}");

        let taps = |c: Vec<f64>, s| ImpulseResponse::Taps {
            taps: c,
            spacing: s,
        };
        let (one, _) =
            transfer_from_impulse(&taps(vec![1.0], TapSpacing::Sample), &g, &sp).unwrap();
        assert!(one
            .iter()
            .all(|h| (h - Complex64::new(1.0, 0.0)).norm() < 1e-15));
        // [0, 1] at sample spacing: one-sample delay, H = e^{−j2πk/N}.
        let (d, _) =
            transfer_from_impulse(&taps(vec![0.0, 1.0], TapSpacing::Sample), &g, &sp).unwrap();
        let w = 2.0 * std::f64::consts::PI / n as f64;
        assert!((0..n).all(|k| (d[k] - Complex64::from_polar(1.0, -w * k as f64)).norm() < 1e-12));
        // Symbol spacing: H(f) = Σ cᵢ e^{−j2πf·iT} (except the Nyquist FFT bin, where H is real).
        let c = vec![-0.1, 1.2, -0.1];
        let (hs, _) = transfer_from_impulse(&taps(c.clone(), TapSpacing::Symbol), &g, &sp).unwrap();
        let t_sym = 1.0 / g.baud;
        let err = (0..n)
            .filter(|&k| k != n / 2)
            .map(|k| {
                let f = crate::dsp::freq(k, n, fs);
                let exact: Complex64 = c
                    .iter()
                    .enumerate()
                    .map(|(i, ci)| {
                        ci * Complex64::from_polar(
                            1.0,
                            -2.0 * std::f64::consts::PI * f * i as f64 * t_sym,
                        )
                    })
                    .sum();
                (hs[k] - exact).norm()
            })
            .fold(0.0, f64::max);
        assert!(err < 1e-12, "symbol-spaced taps: {err:e}");
        assert!((hs[0].re - 1.0).abs() < 1e-15, "DC gain = Σc");
    }

    /// A time-domain component in the graph equals the same filter described in the frequency
    /// domain, including σ(t); noise stays exactly Gaussian.
    #[test]
    fn time_domain_component_matches_frequency_domain() {
        let run = |kind: &str| {
            let mut e = GraphEngine::new(lane_insert("rx", "g", kind, json!({ "sigma": 8e-12 })));
            let el = PortType::Electrical(None);
            let r = e.registry_mut();
            register_custom(r, "gauss_t", time_spec(), Rc::new(GaussT)).unwrap();
            register_custom(r, "gauss_f", spec(el, true), Rc::new(GaussF)).unwrap();
            e.run().unwrap();
            e
        };
        let (t, f) = (run("gauss_t"), run("gauss_f"));
        let (a, b) = (receiver_out(&t), receiver_out(&f));
        assert!(a.gaussian_exact);
        assert_close(&a.noiseless, &b.noiseless, 1e-12, "noiseless");
        assert_close(&a.sigma, &b.sigma, 1e-12, "sigma");
        assert_close(&a.signal, &b.signal, 1e-12, "signal");
        assert!(t.warnings().is_empty(), "{:?}", t.warnings());
    }

    #[test]
    fn impulse_response_checks() {
        let (g, sp) = grid8();
        let n = g.n();
        // Not decayed: exponential with a 1 µs time constant wraps around the 154 ns period.
        let h: Vec<f64> = (0..n)
            .map(|k| {
                let t = crate::dsp::time(k, n, g.fs());
                if t >= 0.0 {
                    (-t / 1e-6).exp() / 1e-6
                } else {
                    0.0
                }
            })
            .collect();
        let (_, w) = transfer_from_impulse(&ImpulseResponse::Continuous(h), &g, &sp).unwrap();
        assert!(w[0].contains("has not decayed"), "{w:?}");
        // A jump at t = 0 (one-sided exponential, τ = 20 ps) is not band-limited: aliasing.
        let h: Vec<f64> = (0..n)
            .map(|k| {
                let t = crate::dsp::time(k, n, g.fs());
                if t >= 0.0 {
                    (-t / 20e-12).exp() / 20e-12
                } else {
                    0.0
                }
            })
            .collect();
        let (_, w) = transfer_from_impulse(&ImpulseResponse::Continuous(h), &g, &sp).unwrap();
        assert!(w.len() == 1 && w[0].contains("aliases"), "{w:?}");
        let bad = |ir| transfer_from_impulse(&ir, &g, &sp).unwrap_err();
        assert!(bad(ImpulseResponse::Continuous(vec![1.0; 3])).contains("simulation grid"));
        assert!(bad(ImpulseResponse::Taps {
            taps: vec![],
            spacing: TapSpacing::Sample
        })
        .contains("empty"));
        assert!(bad(ImpulseResponse::Taps {
            taps: vec![f64::NAN],
            spacing: TapSpacing::Symbol
        })
        .contains("NaN"));
        assert!(bad(ImpulseResponse::Taps {
            taps: vec![0.1; 9000],
            spacing: TapSpacing::Symbol
        })
        .contains("do not fit"));

        // The warning reaches the node; a symbol-spaced FIR runs through the graph.
        struct Slow;
        impl CustomBehavior for Slow {
            fn impulse(&self, ctx: &CustomCtx, t: &[f64]) -> Result<ImpulseResponse, String> {
                let tau = ctx.params["tau"].as_f64().unwrap();
                Ok(ImpulseResponse::Continuous(
                    t.iter()
                        .map(|t| {
                            if *t >= 0.0 {
                                (-t / tau).exp() / tau
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                ))
            }
        }
        let mut e = GraphEngine::new(lane_insert("rx", "s", "slow", json!({ "tau": 1e-6 })));
        register_custom(e.registry_mut(), "slow", time_spec(), Rc::new(Slow)).unwrap();
        e.run().unwrap();
        assert!(e
            .warnings()
            .iter()
            .any(|(n, w)| n == "s" && w.contains("has not decayed")));

        let fir = Fir(vec![-0.1, 1.2, -0.1], TapSpacing::Symbol);
        let mut e = GraphEngine::new(lane_insert("rx", "fir", "fir3", json!({})));
        register_custom(e.registry_mut(), "fir3", time_spec(), Rc::new(fir)).unwrap();
        e.run().unwrap();
        assert!(receiver_out(&e).gaussian_exact);
    }
}

/// `outputs` lists every output port of a node; `result_key` changes with the node or
/// anything upstream and stays put for unaffected nodes.
#[test]
fn outputs_and_result_keys() {
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    g.node("spl", "splitter", json!({ "ratios": [0.5, 0.5] }))
        .wire("fib.out", "spl.in");
    let mut e = GraphEngine::new(g.clone());
    e.run().unwrap();
    let names: Vec<&str> = e.outputs("spl").into_iter().map(|(p, _)| p).collect();
    assert_eq!(names, ["out0", "out1"]);
    assert!(matches!(e.outputs("src")[0].1, Signal::Data(_)));
    assert!(e.outputs("nope").is_empty());
    let (k_src, k_spl) = (e.result_key("src").unwrap(), e.result_key("spl").unwrap());
    let mut g2 = g.clone();
    g2.nodes.iter_mut().find(|n| n.id == "fib").unwrap().params["length"] = json!(50.0);
    e.set_graph(g2);
    e.run().unwrap();
    assert_eq!(e.result_key("src"), Some(k_src), "upstream of the change");
    assert_ne!(e.result_key("spl"), Some(k_spl), "downstream of the change");
}

/// A kind renamed in v0.12 (`decision` → `receiver_output`) still loads and gives the same
/// results.
#[test]
fn renamed_kind_still_loads() {
    let mut g = GraphConfig::default();
    tx_fiber(&mut g, fiber100());
    receiver(&mut g, "fib.out");
    let mut old = g.clone();
    old.nodes.iter_mut().find(|n| n.id == "rx").unwrap().kind = "decision".into();
    let mut a = GraphEngine::new(g);
    let mut b = GraphEngine::new(old);
    a.run().unwrap();
    b.run().unwrap();
    assert_eq!(
        b.graph().nodes.iter().find(|n| n.id == "rx").unwrap().kind,
        "receiver_output"
    );
    match (a.report("rx"), b.report("rx")) {
        (Some(Report::Receiver(x)), Some(Report::Receiver(y))) => assert_eq!(x.signal, y.signal),
        _ => panic!("no receiver report"),
    }
}

/// Fiber presets: an `mmf` node equals a `custom_fiber` with the preset's values; the
/// calibration puts the EMB at the type minimum; the reference lane uses the OM4 preset; an
/// old `fiber` kind still loads as `custom_fiber`; physics parameters on `mmf` are rejected.
#[test]
fn mmf_presets() {
    use crate::config::{MmfType, MMF_MAX_ATTENUATION_DB_PER_KM};
    let run = |kind: &str, params: serde_json::Value| {
        let mut g = GraphConfig::default();
        g.settings.sps = 8;
        g.node("src", "prbs13q", json!({}))
            .node("drv", "driver", json!({}))
            .node("vcsel", "vcsel", json!({}))
            .node("con", "connector", json!({}))
            .node("fib", kind, params)
            .wire("src.out", "drv.in")
            .wire("drv.out", "vcsel.in")
            .wire("vcsel.out", "con.in")
            .wire("con.out", "fib.in");
        receiver(&mut g, "fib.out");
        let mut e = GraphEngine::new(g);
        e.run().map(|_| e)
    };
    for (t, min_emb) in [
        (MmfType::Om3, 2000e9),
        (MmfType::Om4, 4700e9),
        (MmfType::Om5, 4700e9),
    ] {
        let a = run("mmf", json!({ "type": t, "length": 50.0 })).unwrap();
        let mut c = t.config();
        c.length = 50.0;
        let b = run("custom_fiber", serde_json::to_value(&c).unwrap()).unwrap();
        let (Some(Report::Receiver(x)), Some(Report::Receiver(y))) =
            (a.report("rx"), b.report("rx"))
        else {
            panic!("no receiver")
        };
        assert_eq!(
            x.signal, y.signal,
            "{t:?}: preset = custom with the preset's values"
        );
        let Some(Report::FiberSpan {
            launch: Some(l), ..
        }) = a.report("fib")
        else {
            panic!("no launch report")
        };
        let emb = l.emb_length.unwrap();
        assert!(emb >= min_emb && emb < 1.02 * min_emb, "{t:?}: EMB {emb:e}");
    }
    assert!(run(
        "mmf",
        json!({ "type": "om4", "profile": { "kind": "step" } })
    )
    .is_err());
    assert!(run(
        "mmf",
        json!({ "attenuation_db_per_km": MMF_MAX_ATTENUATION_DB_PER_KM + 0.1 })
    )
    .is_err());
    assert!(run(
        "fiber",
        serde_json::to_value(crate::config::FiberConfig::default()).unwrap()
    )
    .is_ok());
    let g = lane_graph(&LaneConfig::default(), Resolution::Mode, Level::Power);
    let el1 = g.nodes.iter().find(|n| n.id == "el1").unwrap();
    assert_eq!(
        (el1.kind.as_str(), el1.params["type"].as_str()),
        ("mmf", Some("om4"))
    );
    let mut lane = LaneConfig::default();
    if let crate::system::Element::Fiber(f) = &mut lane.elements[1] {
        f.grid_points_per_radius = 300;
    }
    let g = lane_graph(&lane, Resolution::Mode, Level::Power);
    assert_eq!(
        g.nodes.iter().find(|n| n.id == "el1").unwrap().kind,
        "custom_fiber"
    );
}

// ---------------------------------------------------------------------------------------------
// Transmitter jitter (`docs/model/driver.md`, `eq:tx-jitter`)
// ---------------------------------------------------------------------------------------------

/// Back-to-back lane without RIN and receiver noise: only the driver's jitter is random.
fn jitter_b2b(driver: serde_json::Value) -> GraphEngine {
    let mut g = GraphConfig::default();
    g.settings.sps = 8;
    g.node("src", "prbs13q", json!({}))
        .node("drv", "driver", driver)
        .node("vcsel", "vcsel", json!({ "rin_enabled": false }))
        .node("pin", "pin", json!({ "shot_noise": false }))
        .node("tia", "tia", json!({ "thermal_noise": false }))
        .node("rxf", "bessel_thomson", json!({}))
        .node("rx", "receiver_output", json!({}))
        .wire("src.out", "drv.in")
        .wire("drv.out", "vcsel.in")
        .wire("vcsel.out", "pin.in")
        .wire("pin.out", "tia.in")
        .wire("tia.out", "rxf.in")
        .wire("rxf.out", "rx.in");
    let mut e = GraphEngine::new(g);
    e.run().expect("jitter lane runs");
    e
}

/// V16a: the random-jitter slope-noise model against exact random edge shifts. The receiver
/// σ(t) of RJ = 0.3 ps (0.016 UI) at 53.125 GBd must match the variance of the waveform over
/// Monte Carlo realizations in which every edge is shifted by an independent Gaussian τ
/// (exact edge-shift spectrum through the same VCSEL and receive filter), phase by phase
/// within the UI. Also with DCD (edges off the sample grid). The realization the engine
/// draws has the predicted variance.
#[test]
fn v16a_random_jitter_matches_edge_shifts() {
    use crate::config::VcselConfig;
    use crate::dsp::{freq, BesselThomson4, Spectral};
    use crate::prbs::{pattern, PRBS13_DEFAULT_SEED};
    use crate::rng::Rng;
    use crate::tx::{self, VcselResponse};
    use num_complex::Complex64;

    let rj = 0.3e-12;
    for dcd in [0.0, 2e-12] {
        let e = jitter_b2b(json!({ "rj_rms": rj, "dcd": dcd }));
        let m = receiver_out(&e);
        let grid = e.grid();
        let (n, sps, fs, baud) = (grid.n(), grid.sps, grid.fs(), grid.baud);
        let sp = Spectral::new(n);
        let syms = pattern(4, PRBS13_DEFAULT_SEED).unwrap();
        let lv = tx::drive_levels(4);
        let tau0 = crate::components::deterministic_jitter(
            &crate::config::JitterConfig {
                dcd,
                ..Default::default()
            },
            &syms,
            baud,
        )
        .unwrap()
        .tau;
        let vr = VcselResponse::from_config(&VcselConfig::default());
        let bt = BesselThomson4::new(0.5 * baud);
        let h: Vec<Complex64> = (0..n)
            .map(|k| vr.response(freq(k, n, fs)) * bt.response(freq(k, n, fs)))
            .collect();
        let through = |x: Vec<Complex64>| -> Vec<f64> {
            sp.inverse_real(x.iter().zip(&h).map(|(a, b)| a * b).collect())
        };
        let x0 = through(tx::shifted_nrz_spectrum(&syms, &lv, sps, &tau0, &sp));
        // Linear map of the engine's receiver waveform onto the filtered drive.
        let mx = x0.iter().sum::<f64>() / n as f64;
        let my = m.noiseless.iter().sum::<f64>() / n as f64;
        let cov: f64 = x0
            .iter()
            .zip(&m.noiseless)
            .map(|(a, b)| (a - mx) * (b - my))
            .sum();
        let var: f64 = x0.iter().map(|a| (a - mx) * (a - mx)).sum();
        let gain = cov / var;
        let fit = x0
            .iter()
            .zip(&m.noiseless)
            .map(|(a, b)| (b - my - gain * (a - mx)).abs())
            .fold(0.0, f64::max);
        assert!(
            fit < 1e-9 * gain,
            "receiver waveform is a linear map of the drive"
        );

        let k_runs = 24;
        let sigma_ui = rj * baud;
        let mut rng = Rng::new(11);
        let mut var_mc = vec![0.0; sps];
        for _ in 0..k_runs {
            let tau: Vec<f64> = tau0.iter().map(|t| t + sigma_ui * rng.normal()).collect();
            let x = through(tx::shifted_nrz_spectrum(&syms, &lv, sps, &tau, &sp));
            for (i, (a, b)) in x.iter().zip(&x0).enumerate() {
                var_mc[i % sps] += (a - b) * (a - b);
            }
        }
        let mut var_pred = vec![0.0; sps];
        for (i, s) in m.sigma.iter().enumerate() {
            var_pred[i % sps] += (s / gain).powi(2) * k_runs as f64;
        }
        for p in 0..sps {
            let r = var_mc[p] / var_pred[p];
            println!("dcd {dcd:e}: phase {p}/{sps}: MC/model {r:.4}");
            assert!(
                (r - 1.0).abs() < 0.03,
                "dcd {dcd}: phase {p}/{sps}: MC/model {r:.4}"
            );
        }
        // The engine's own realization has the predicted variance (averaged over seeds; one
        // realization of ≈ 6000 PAM-4 edges spreads by ≈ 2.5 %).
        let mut ratio = 0.0;
        let seeds = 6;
        for seed in 0..seeds {
            let e = jitter_b2b(json!({ "rj_rms": rj, "dcd": dcd, "seed": seed }));
            let m = receiver_out(&e);
            let dev: f64 = m
                .signal
                .iter()
                .zip(&m.noiseless)
                .map(|(a, b)| (a - b).powi(2))
                .sum();
            let pred: f64 = m.sigma.iter().map(|s| s * s).sum();
            ratio += dev / pred / seeds as f64;
        }
        println!("dcd {dcd:e}: realization/model {ratio:.4}");
        assert!((ratio - 1.0).abs() < 0.03, "realization/model {ratio}");
    }
}

/// Zero jitter parameters give the unjittered drive bit for bit; DCD moves the mean exactly.
#[test]
fn jitter_zero_is_identity_and_dcd_shifts_mean() {
    let a = receiver_out(&jitter_b2b(json!({})));
    let b = receiver_out(&jitter_b2b(
        json!({ "rj_rms": 0.0, "dcd": 0.0, "sj_amplitude": 0.0 }),
    ));
    assert_eq!(a.noiseless, b.noiseless);
    assert!(b.sigma.iter().all(|s| *s == 0.0));
    let c = jitter_b2b(json!({ "dcd": 4e-12, "sj_amplitude": 1e-12, "sj_frequency": 1e9 }));
    assert!(c
        .warnings()
        .iter()
        .any(|w| w.1.contains("SJ frequency rounded")),);
    let bad = {
        let mut g = GraphConfig::default();
        g.node("src", "prbs13q", json!({}))
            .node("drv", "driver", json!({ "dcd": 30e-12 }))
            .node("o", "receiver_output", json!({}))
            .wire("src.out", "drv.in")
            .wire("drv.out", "o.in");
        let mut e = GraphEngine::new(g);
        e.run()
    };
    assert!(bad.is_err(), "DCD above one UI must be rejected");
}

/// SJ: frequency on the record grid, amplitude reduced by the clock recovery's jitter transfer
/// |jf/(f_c + jf)|, DCD as ±DCD/2 by edge direction.
#[test]
fn sinusoidal_jitter_and_clock_recovery() {
    use crate::components::deterministic_jitter;
    use crate::config::JitterConfig;
    let syms = crate::prbs::pattern(4, crate::prbs::PRBS13_DEFAULT_SEED).unwrap();
    let baud = 53.125e9;
    let f_rec = baud / syms.len() as f64;
    for (f, fc) in [(4e6, 4e6), (100e6, 4e6), (100e6, 0.0)] {
        let j = JitterConfig {
            sj_amplitude: 1e-12,
            sj_frequency: f,
            cdr_corner: fc,
            ..Default::default()
        };
        let dj = deterministic_jitter(&j, &syms, baud).unwrap();
        let (tau, used) = (dj.tau, dj.sj_frequency.unwrap());
        assert!((used / f_rec - (f / f_rec).round().max(1.0)).abs() < 1e-9);
        let h = if fc > 0.0 {
            used / (fc * fc + used * used).sqrt()
        } else {
            1.0
        };
        let peak = tau.iter().fold(0.0f64, |m, t| m.max(t.abs()));
        let expect = 1e-12 * baud * h;
        assert!(
            (peak - expect).abs() < 1e-3 * expect,
            "{f}/{fc}: {peak} vs {expect}"
        );
    }
    let tau = deterministic_jitter(
        &JitterConfig {
            dcd: 4e-12,
            ..Default::default()
        },
        &syms,
        baud,
    )
    .unwrap()
    .tau;
    let half = 2e-12 * baud;
    for i in 1..syms.len() {
        let want = match syms[i].cmp(&syms[i - 1]) {
            std::cmp::Ordering::Greater => -half,
            std::cmp::Ordering::Less => half,
            std::cmp::Ordering::Equal => 0.0,
        };
        assert_eq!(tau[i], want);
    }
}

// ---------------------------------------------------------------------------------------------
// Wavelength dependence and WDM (docs/model/wdm-mux.md, wdm-demux.md)
// ---------------------------------------------------------------------------------------------

mod wdm {
    use super::*;
    use crate::wdm::{Passband, SWDM4_CHANNELS};

    /// Transmitter `i` (pattern, driver, VCSEL at `l`) with fixed seeds, ending at `vcsel{i}`.
    fn tx(g: &mut GraphConfig, i: usize, l: f64) {
        let (s, d, v) = (format!("src{i}"), format!("drv{i}"), format!("vcsel{i}"));
        let seed = [2731, 1, 77, 4001][i];
        g.node(&s, "prbs13q", json!({ "seed": seed }))
            .node(&d, "driver", json!({}))
            .node(
                &v,
                "vcsel",
                json!({ "seed": 100 + i, "vcsel": { "wavelength": l } }),
            )
            .wire(&format!("{s}.out"), &format!("{d}.in"))
            .wire(&format!("{d}.out"), &format!("{v}.in"));
    }

    /// Receiver chain after `from`, with fixed seeds.
    fn rx_chain(g: &mut GraphConfig, from: &str) {
        g.node("pin", "pin", json!({ "noise_seed": 9 }))
            .node("tia", "tia", json!({ "noise_seed": 9 }))
            .node("rxf", "bessel_thomson", json!({}))
            .node("rx", "receiver_output", json!({}))
            .wire(from, "pin.in")
            .wire("pin.out", "tia.in")
            .wire("tia.out", "rxf.in")
            .wire("rxf.out", "rx.in");
    }

    fn fiber_om5(g: &mut GraphConfig, from: &str) {
        g.node("con", "connector", json!({}))
            .node("fib", "mmf", json!({ "type": "om5", "length": 100.0 }))
            .wire(from, "con.in")
            .wire("con.out", "fib.in");
    }

    /// SWDM4 link: 4 transmitters → mux → OM5 → demux; receiver on channel `k`.
    fn swdm4(k: usize, wdm: serde_json::Value) -> GraphEngine {
        let mut g = GraphConfig::default();
        g.settings.sps = 8;
        for (i, &l) in SWDM4_CHANNELS.iter().enumerate() {
            tx(&mut g, i, l);
        }
        g.node("mux", "wdm_mux", wdm.clone());
        for i in 0..4 {
            g.wire(&format!("vcsel{i}.out"), &format!("mux.in{i}"));
        }
        fiber_om5(&mut g, "mux.out");
        g.node("demux", "wdm_demux", wdm)
            .wire("fib.out", "demux.in");
        rx_chain(&mut g, &format!("demux.out{k}"));
        let mut e = GraphEngine::new(g);
        e.run().expect("SWDM4 link runs");
        e
    }

    /// Single-wavelength link of channel `k` with the same seeds (no mux/demux).
    fn single(k: usize) -> GraphEngine {
        let mut g = GraphConfig::default();
        g.settings.sps = 8;
        tx(&mut g, k, SWDM4_CHANNELS[k]);
        fiber_om5(&mut g, &format!("vcsel{k}.out"));
        rx_chain(&mut g, "fib.out");
        let mut e = GraphEngine::new(g);
        e.run().unwrap();
        e
    }

    /// Attenuation spectrum: exactly the 850 nm value at 850 nm, the OM5 ratio 2.3/3.0 at
    /// 953 nm by construction, and a prediction for 1300 nm within 4 % of the cabled limit
    /// (1.5 of 3.0 dB/km). Flat keeps the old behavior.
    #[test]
    fn attenuation_spectrum() {
        let f = crate::config::FiberConfig {
            attenuation_db_per_km: 3.0,
            ..Default::default()
        };
        assert_eq!(f.attenuation_at(850e-9), 3.0);
        assert!((f.attenuation_at(953e-9) - 2.3).abs() < 1e-6);
        let a1300 = f.attenuation_at(1300e-9);
        assert!((a1300 / 1.5 - 1.0).abs() < 0.04, "{a1300}");
        assert!(
            f.attenuation_at(880e-9) < 3.0 && f.attenuation_at(940e-9) < f.attenuation_at(910e-9)
        );
        let flat = crate::config::FiberConfig {
            attenuation_spectrum: crate::config::AttenuationSpectrum::Flat,
            ..f
        };
        assert_eq!(flat.attenuation_at(940e-9), 3.0);
    }

    /// V17a: an ideal mux/demux (no loss, no isolation floor) leaves every channel equal to
    /// the single-wavelength link (the other channels leak by < 10⁻⁸⁰), and the received
    /// power and current follow the wavelength-dependent attenuation and responsivity.
    #[test]
    fn v17a_ideal_wdm_equals_single_links() {
        let ideal = json!({ "insertion_loss_db": 0.0, "isolation_db": null });
        for k in [0usize, 3] {
            let a = receiver_out(&swdm4(k, ideal.clone()));
            let s = single(k);
            let b = receiver_out(&s);
            assert_close(&a.noiseless, &b.noiseless, 1e-9, "noiseless");
            assert_close(&a.sigma, &b.sigma, 1e-9, "sigma");
            assert_close(&a.signal, &b.signal, 1e-9, "signal");
            // Mean current = R₈₅₀·λ/850 nm · P_det, P_det = launch × 10^(−α(λ)L/10).
            let l = SWDM4_CHANNELS[k];
            let p_det = match s.report("pin") {
                Some(Report::Detection(d)) => d.p_det.iter().sum::<f64>() / d.p_det.len() as f64,
                _ => panic!("no detection report"),
            };
            let i_mean = b.noiseless.iter().sum::<f64>() / b.noiseless.len() as f64 / 1000.0;
            let want = 0.5 * l / 850e-9 * p_det;
            assert!((i_mean / want - 1.0).abs() < 1e-9, "{i_mean} vs {want}");
            let f = crate::config::MmfType::Om5.config();
            let tr = match s.report("pin") {
                Some(Report::Detection(d)) => d.channels[0].transmission,
                _ => unreachable!(),
            };
            let guided = tr / crate::units::attenuation_transmission(f.attenuation_at(l), 100.0);
            assert!(guided > 0.999 && guided <= 1.0, "guided fraction {guided}");
        }
    }

    /// V17b: crosstalk is exact superposition. The noiseless waveform at demux output k is
    /// Σⱼ T_k(λⱼ)·yⱼ, yⱼ the single-wavelength waveform of channel j (same fiber, same
    /// receiver); a lower isolation raises the SER of the channel.
    #[test]
    fn v17b_crosstalk_is_weighted_superposition() {
        let k = 1;
        let wdm = json!({ "insertion_loss_db": 1.0, "isolation_db": 15.0 });
        let a = receiver_out(&swdm4(k, wdm));
        let singles: Vec<Vec<f64>> = (0..4)
            .map(|j| receiver_out(&single(j)).noiseless.clone())
            .collect();
        let pb = |c: f64| Passband {
            center: c,
            width: 22e-9,
            order: 3,
            insertion_loss_db: 1.0,
            isolation_db: Some(15.0),
        };
        let demux = pb(SWDM4_CHANNELS[k]);
        let mut want = vec![0.0; a.noiseless.len()];
        for (j, y) in singles.iter().enumerate() {
            let l = SWDM4_CHANNELS[j];
            // Through mux port j (in band) and demux port k.
            let t = pb(l).transmission(l) * demux.transmission(l);
            for (w, v) in want.iter_mut().zip(y) {
                *w += t * v;
            }
        }
        assert_close(&a.noiseless, &want, 1e-9, "superposition");
    }

    /// The bandpass filter: ideal → bit-identical; a source outside the band is attenuated
    /// to the isolation floor, and a too narrow filter warns about the source spectrum.
    #[test]
    fn bandpass_filter_and_warnings() {
        let build = |bp: serde_json::Value| {
            let mut g = GraphConfig::default();
            g.settings.sps = 8;
            tx(&mut g, 0, 850e-9);
            g.node("bp", "bandpass_filter", bp)
                .node("pm", "power_meter", json!({}))
                .wire("vcsel0.out", "bp.in")
                .wire("bp.out", "pm.in");
            let mut e = GraphEngine::new(g);
            e.run().unwrap();
            e
        };
        let ideal = build(json!({ "insertion_loss_db": 0.0, "isolation_db": null }));
        let p0 = meter(&ideal, "pm");
        assert!((p0 / 1e-3 - 1.0).abs() < 1e-3);
        let off =
            build(json!({ "center": 940e-9, "insertion_loss_db": 0.0, "isolation_db": 25.0 }));
        assert!((meter(&off, "pm") / p0 / db_to_lin(-25.0) - 1.0).abs() < 1e-9);
        let narrow = build(json!({ "width": 1e-9 }));
        assert!(narrow
            .warnings()
            .iter()
            .any(|(n, w)| n == "bp" && w.contains("does not reshape")));
        // Out-of-band sources at a mux port are reported.
        let mut g = GraphConfig::default();
        g.settings.sps = 8;
        tx(&mut g, 0, 850e-9);
        g.node("mux", "wdm_mux", json!({ "channels": [910e-9] }))
            .node("pm", "power_meter", json!({}))
            .wire("vcsel0.out", "mux.in0")
            .wire("mux.out", "pm.in");
        let mut e = GraphEngine::new(g);
        e.run().unwrap();
        assert!(e
            .warnings()
            .iter()
            .any(|(n, w)| n == "mux" && w.contains("outside the port's passband")));
    }

    fn assert_close(a: &[f64], b: &[f64], tol: f64, what: &str) {
        let r = rel_max(a, b);
        assert!(r < tol, "{what}: relative difference {r:e}");
    }
}

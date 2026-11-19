"""Graph engine step 2 (docs/architecture.md, Sections 7.5 and 7.7): Python graph API, custom Python
components (G5) and nonlinear blocks with Monte Carlo SER (G6)."""

import json

import numpy as np
import pytest

import lightstone


def rel_max(a, b):
    return np.max(np.abs(np.asarray(a) - np.asarray(b))) / np.max(np.abs(b))


def fast_lane(**params):
    """Reference lane at 8 samples/UI and a coarser mode-solver grid, without the receiver
    FFE (these tests insert their own components before the receiver output)."""
    els = [lightstone.connector(), lightstone.fiber(100.0, grid_points_per_radius=300)]
    return lightstone.lane(els, **{"tx.sps": 8, "rx.ffe_taps": 0, **params})


def bt_response(f, f3db):
    """4th-order Bessel–Thomson, written independently of the Rust code (eq:rx-bessel-thomson)."""

    def h(s):
        s2 = s * s
        return 105.0 / (s2 * s2 + 10.0 * s2 * s + 45.0 * s2 + 105.0 * s + 105.0)

    lo, hi = 1.0, 4.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if abs(h(1j * mid)) ** 2 > 0.5 else (lo, mid)
    return h(1j * 0.5 * (lo + hi) * f / f3db)


BT = lightstone.Component(
    "py_bessel_thomson",
    inputs={"in": "electrical"},
    outputs={"out": "electrical"},
    linear=True, domain="frequency", function=lambda f, ctx: bt_response(f, ctx.params.get("f3db_rel", 0.5) * ctx.baud),
)

ATT = lightstone.Component(
    "py_attenuator",
    inputs={"in": "optical"},
    outputs={"out": "optical"},
    linear=True, domain="frequency", function=lambda f, ctx: 10 ** (-ctx.params["loss_db"] / 10),
)


def clipper(lo, hi):
    return lightstone.Component(
        f"py_clip_{lo}_{hi}",
        inputs={"in": "voltage"},
        outputs={"out": "voltage"},
        linear=False, domain="time", function=lambda x, ctx: {"out": x["in"].map(lambda v: np.clip(v, lo, hi))},
    )


def receiver(g):
    return g.run().receiver("rx")


# -- graph API ----------------------------------------------------------------------------


def test_reference_graph_matches_simulator():
    """The lane preset graph through the Python graph API equals the Simulator result."""
    lane = fast_lane()
    rx = receiver(lightstone.Graph.reference(lane))
    sim = lightstone.System([lane]).run()[0]
    assert np.array_equal(rx.signal, sim.rx_signal)
    assert np.array_equal(rx.sigma, sim.rx_sigma)
    assert rx.gaussian_exact and rx.ser_method == "semi_analytic"
    a = rx.analyze()
    assert a["ser"] == pytest.approx(sim.analyze()["ser"], rel=1e-12)
    assert a["ser_resolution"] is None


def test_build_graph_by_hand_and_json_roundtrip():
    g = lightstone.Graph(settings={"sps": 8})
    g.add("src", "prbs13q").add("drv", "driver").add("vcsel", "vcsel").add("con", "connector")
    g.add("fib", "custom_fiber", length=100.0, grid_points_per_radius=300)
    g.add("pm", "power_meter")
    g.add("pin", "pin").add("tia", "tia").add("rxf", "bessel_thomson").add("rx", "receiver_output")
    g.chain("src", "drv", "vcsel", "con", "fib", "pin", "tia", "rxf", "rx")
    g.wire("fib", "pm")
    assert g.validate() == []
    rx = receiver(g)
    assert 0 < g.power("pm") < 1e-3
    assert g.report("fib")["type"] == "fiber_span"
    assert g.output("src")["symbols"].size == 8191
    assert g.output("tia")["quantity"] == "voltage"
    back = lightstone.Graph.from_json(g.to_json())
    assert np.array_equal(receiver(back).signal, rx.signal)
    assert json.loads(back.to_json()) == json.loads(g.to_json())


def test_caching_and_reregistration():
    g = lightstone.Graph.reference(fast_lane())
    g.run()
    assert g.computed_last_run == 9
    g.run()
    assert g.computed_last_run == 0
    g.set("el1", length=50.0).run()
    assert g.computed_last_run == 5  # fiber, pin, tia, filter, decision
    # Re-registering a custom kind (new code) recomputes everything.
    g.insert("pin", "att", "py_attenuator", loss_db=1.0).use(ATT).run()
    half = lightstone.Component("py_attenuator", inputs={"in": "optical"}, outputs={"out": "optical"}, linear=True, domain="frequency", function=lambda f, ctx: 0.5)
    g.use(half).run()
    assert g.computed_last_run == 10


def test_graph_errors():
    g = lightstone.Graph.reference(fast_lane())
    g.add("loose", "attenuator")
    with pytest.raises(lightstone.GraphError) as e:
        g.run()
    assert any(i["node"] == "loose" and "not connected" in i["message"] for i in e.value.issues)

    # A Python exception is reported at the node, with its traceback.
    def broken(f, ctx):
        raise ZeroDivisionError("boom")

    bad = lightstone.Component("py_broken", inputs={"in": "electrical"}, outputs={"out": "electrical"}, linear=True, domain="frequency", function=broken)
    g = lightstone.Graph.reference(fast_lane(), components=[bad]).replace("rxf", "py_broken")
    with pytest.raises(lightstone.GraphError) as e:
        g.run()
    assert e.value.issues[0]["node"] == "rxf"
    assert "ZeroDivisionError: boom" in str(e.value) and "Traceback" in str(e.value)

    # H(f) without a real impulse response is rejected.
    odd = lightstone.Component("py_odd", inputs={"in": "electrical"}, outputs={"out": "electrical"}, linear=True, domain="frequency", function=lambda f, ctx: 1 + 0.1j)
    g = lightstone.Graph.reference(fast_lane(), components=[odd]).replace("rxf", "py_odd")
    with pytest.raises(lightstone.GraphError, match="real impulse response"):
        g.run()

    # Nonlinear optical custom components are not supported yet.
    nl_opt = lightstone.Component("py_nl_opt", inputs={"in": "optical"}, outputs={"out": "optical"}, linear=False, domain="time", function=lambda x, ctx: x)
    g = lightstone.Graph.reference(fast_lane(), components=[nl_opt]).insert("pin", "x", "py_nl_opt")
    with pytest.raises(lightstone.GraphError, match="not supported yet"):
        g.run()

    with pytest.raises(ValueError, match="built-in"):
        lightstone.register(lightstone.Component("tia", inputs={"in": "current"}, outputs={"out": "voltage"}, linear=True, domain="frequency", function=lambda f, c: 1))


def test_component_decorator_registers_globally():
    @lightstone.component("py_gain_2", inputs={"in": "electrical"}, outputs={"out": "electrical"}, linear=True, domain="frequency")
    def gain(f, ctx):
        """Ideal gain of 2."""
        return 2.0

    assert gain.linear and gain.doc == "Ideal gain of 2."
    base = receiver(lightstone.Graph.reference(fast_lane(**{"rx.thermal_noise": False, "rx.shot_noise": False})))
    g = lightstone.Graph.reference(fast_lane(**{"rx.thermal_noise": False, "rx.shot_noise": False})).insert("rx", "g2", "py_gain_2")
    rx = receiver(g)
    assert rel_max(rx.noiseless, 2 * base.noiseless) < 1e-12
    assert rel_max(rx.sigma, 2 * base.sigma) < 1e-12


# -- G5 -----------------------------------------------------------------------------------


def test_g5_linear_python_filter_equals_builtin():
    """G5: a Python Bessel–Thomson declared linear equals the built-in filter (signal and
    the analytic noise σ(t), so the semi-analytic SER stays valid)."""
    a = receiver(lightstone.Graph.reference(fast_lane()))
    b = receiver(lightstone.Graph.reference(fast_lane(), components=[BT]).replace("rxf", "py_bessel_thomson", f3db_rel=0.5))
    assert b.gaussian_exact
    for k in ("noiseless", "sigma", "signal"):
        assert rel_max(getattr(b, k), getattr(a, k)) < 1e-12, k
    assert b.analyze()["ser"] == pytest.approx(a.analyze()["ser"], rel=1e-9)


def test_g5_linear_python_optical_equals_attenuator():
    """G5 (optical): a Python H(f) = T on optical power equals the built-in attenuator."""

    def build(kind):
        g = lightstone.Graph.reference(fast_lane(), components=[ATT]).insert("pin", "att", kind, loss_db=2.5)
        return g.add("pm", "power_meter").wire("att", "pm").run()

    a, b = build("attenuator"), build("py_attenuator")
    ra, rb = a.receiver(), b.receiver()
    for k in ("noiseless", "sigma", "signal"):
        assert rel_max(getattr(rb, k), getattr(ra, k)) < 1e-12, k
    assert b.power("pm") == pytest.approx(a.power("pm"), rel=1e-12)


# -- G6 -----------------------------------------------------------------------------------


def noisy_lane(**params):
    """Noise-limited operating point (SER ≈ 4·10⁻²) with 4 pattern periods."""
    els = [lightstone.connector(), lightstone.fiber(50.0, grid_points_per_radius=300)]
    return lightstone.lane(els, **{"tx.sps": 8, "tx.pattern_periods": 4, "rx.noise_density": 120e-12, "rx.ffe_taps": 0, **params})


def test_g6_clipping_switches_to_monte_carlo():
    """G6: TIA clipping makes the receiver nonlinear: the SER becomes a counted (Monte Carlo)
    estimate. With limits far outside the signal it reproduces the linear receiver, and the
    Monte Carlo SER matches the semi-analytic SER within its statistical error."""
    lin = receiver(lightstone.Graph.reference(noisy_lane()))
    a = lin.analyze()
    g = lightstone.Graph.reference(noisy_lane()).set("tia", clip=[-1e3, 1e3])
    wide = receiver(g)
    assert not wide.gaussian_exact and wide.ser_method == "monte_carlo"
    # Equal to rounding except in the Nyquist FFT bin: filters are complex at +fs/2, and
    # materializing before the receive filter drops that FFT bin's imaginary part (≈ 2e-10 here).
    assert rel_max(wide.signal, lin.signal) < 1e-9
    assert any(n == "rx" and "Monte Carlo" in w for n, w in g.warnings())
    m = wide.analyze()
    n = len(wide.symbols)
    assert m["ser"] == m["ser_counted"] and m["ser_resolution"] == pytest.approx(1 / n)
    stat = np.sqrt(a["ser"] * (1 - a["ser"]) / n)
    assert abs(m["ser"] - a["ser"]) < 4 * stat, (m["ser"], a["ser"], stat)

    # Active clipping: a Python clipper after a linear TIA equals the built-in clipping TIA.
    hard = receiver(lightstone.Graph.reference(noisy_lane()).set("tia", clip=[0.3, 0.6]))
    py = receiver(lightstone.Graph.reference(noisy_lane(), components=[clipper(0.3, 0.6)]).insert("rxf", "clp", "py_clip_0.3_0.6"))
    assert not py.gaussian_exact
    assert np.array_equal(hard.signal, py.signal)
    assert hard.analyze()["ser"] > m["ser"]


def test_nonlinear_block_without_noise_stays_exact():
    """Without noise before the nonlinear block, the output is deterministic and σ(t) of
    noise added afterwards is still exact."""
    lane = noisy_lane(**{"rx.thermal_noise": False, "rx.shot_noise": False, "rx.rin_noise": False})
    rx = receiver(lightstone.Graph.reference(lane).set("tia", clip=[0.3, 0.6]))
    assert rx.gaussian_exact
    assert np.array_equal(rx.signal, rx.noiseless)


# -- linear components in the time domain -------------------------------------------------


def gauss_pair(sigma_key="sigma"):
    """The same Gaussian filter described in the time and in the frequency domain."""
    t = lightstone.Component(
        "py_gauss_t",
        inputs={"in": "electrical"},
        outputs={"out": "electrical"},
        linear=True,
        domain="time",
        function=lambda t, ctx: np.exp(-t**2 / (2 * ctx.params[sigma_key] ** 2)) / (np.sqrt(2 * np.pi) * ctx.params[sigma_key]),
    )
    f = lightstone.Component(
        "py_gauss_f",
        inputs={"in": "electrical"},
        outputs={"out": "electrical"},
        linear=True,
        domain="frequency",
        function=lambda f, ctx: np.exp(-2 * np.pi**2 * ctx.params[sigma_key] ** 2 * f**2),
    )
    return t, f


def test_time_domain_impulse_response_equals_frequency_domain():
    """h(t) is converted to H(f) exactly: a Gaussian pulse response in the time domain equals
    the analytic Gaussian H(f), including σ(t) (signals stay lazy, SER semi-analytic)."""
    gt, gf = gauss_pair()
    rt = receiver(lightstone.Graph.reference(fast_lane(), components=[gt]).insert("rx", "g", "py_gauss_t", sigma=8e-12))
    g = lightstone.Graph.reference(fast_lane(), components=[gf]).insert("rx", "g", "py_gauss_f", sigma=8e-12)
    rf = receiver(g)
    assert rt.gaussian_exact and rt.ser_method == "semi_analytic"
    for k in ("noiseless", "sigma", "signal"):
        assert rel_max(getattr(rt, k), getattr(rf, k)) < 1e-12, k


def test_fir_taps():
    """FIR taps: [1] is the identity; a symbol-spaced 3-tap FFE (Σc = 1) opens the 100 m eye."""
    ident = lightstone.Component("py_ident", inputs={"in": "electrical"}, outputs={"out": "electrical"}, linear=True, domain="time", taps=[1.0], tap_spacing="sample")
    base = receiver(lightstone.Graph.reference(fast_lane()))
    one = receiver(lightstone.Graph.reference(fast_lane(), components=[ident]).insert("rx", "i", "py_ident"))
    assert rel_max(one.signal, base.signal) < 1e-12

    ffe = lightstone.Component(
        "py_ffe",
        inputs={"in": "electrical"},
        outputs={"out": "electrical"},
        linear=True,
        domain="time",
        taps=lambda ctx: ctx.params["taps"],
        tap_spacing="symbol",
    )
    eq = receiver(lightstone.Graph.reference(fast_lane(), components=[ffe]).insert("rx", "ffe", "py_ffe", taps=[-0.15, 1.3, -0.15]))
    a, b = base.analyze(), eq.analyze()
    assert eq.gaussian_exact
    assert b["ser"] < a["ser"] / 10, (a["ser"], b["ser"])


def test_time_domain_warnings():
    """A response with a jump at t = 0 is not band-limited (aliases); a slow one wraps around."""
    def one_sided(tau):
        return lambda t, ctx: np.where(t >= 0, np.exp(-np.abs(t) / tau) / tau, 0.0)

    for tau, text in ((20e-12, "aliases"), (1e-6, "has not decayed")):
        c = lightstone.Component("py_exp", inputs={"in": "electrical"}, outputs={"out": "electrical"}, linear=True, domain="time", function=one_sided(tau))
        g = lightstone.Graph.reference(fast_lane(), components=[c]).insert("rx", "x", "py_exp").run()
        assert any(n == "x" and text in w for n, w in g.warnings()), g.warnings()


def test_linear_and_domain_are_explicit():
    ports = dict(inputs={"in": "electrical"}, outputs={"out": "electrical"})
    with pytest.raises(TypeError):
        lightstone.Component("x", **ports, domain="time", function=lambda t, c: t)  # no linear
    with pytest.raises(TypeError):
        lightstone.Component("x", **ports, linear=True, function=lambda f, c: 1)  # no domain
    with pytest.raises(TypeError):

        @lightstone.component("x", **ports, linear=True)
        def h(f, ctx):
            return 1

    with pytest.raises(TypeError, match="True or False"):
        lightstone.Component("x", **ports, linear=1, domain="time", function=lambda t, c: t)
    with pytest.raises(ValueError, match="'time' or 'frequency'"):
        lightstone.Component("x", **ports, linear=True, domain="freq", function=lambda f, c: 1)
    with pytest.raises(ValueError, match="no transfer function"):
        lightstone.Component("x", **ports, linear=False, domain="frequency", function=lambda x, c: x)
    with pytest.raises(ValueError, match="tap_spacing"):
        lightstone.Component("x", **ports, linear=True, domain="time", taps=[1.0])
    with pytest.raises(ValueError, match="only possible"):
        lightstone.Component("x", **ports, linear=True, domain="frequency", taps=[1.0], tap_spacing="sample")
    with pytest.raises(ValueError, match="not both"):
        lightstone.Component("x", **ports, linear=True, domain="time", function=lambda t, c: t, taps=[1.0], tap_spacing="sample")


# -- analysis nodes and the receiver FFE ---------------------------------------------------


def analyzed(lane, ffe=None):
    """Reference graph with PAM-4, eye and TDECQ analyzers (and optionally an FFE)."""
    g = lightstone.Graph.reference(lane)
    sig = "rxf"
    if ffe is not None:
        g.insert("rx", "ffe", "ffe", **ffe).wire("src", "ffe.ref")
        sig = "ffe"
    g.add("ana", "pam4_analyzer").wire(sig, "ana").wire("src", "ana.ref")
    g.add("eye", "eye_diagram", width=300, height=200).wire(sig, "eye").wire("src", "eye.ref")
    g.add("tq", "tdecq").wire("el1", "tq").wire("src", "tq.ref")
    return g.run()


def test_analysis_nodes_match_python_analysis():
    lane = fast_lane()
    g = analyzed(lane)
    sim = lightstone.System([lane]).run()[0]
    a, ref = g.report("ana"), sim.analyze()
    assert a["type"] == "pam4_analysis" and a["ser_method"] == "semi_analytic"
    assert a["ser"] == ref["ser"] and a["eye_height"] == ref["eye_height"]
    eye = g.report("eye")
    counts, _ = sim.eye(300, 200, analysis=ref)
    assert eye["counts"].shape == (200, 300) and np.array_equal(eye["counts"], counts)
    assert g.report("tq")["tdecq_db"] == pytest.approx(sim.tdecq()["tdecq_db"], abs=1e-9)
    assert {"pam4_analyzer", "eye_diagram", "tdecq", "ffe"} <= set(lightstone.builtin_kinds())


def test_ffe_node_opens_the_eye():
    lane = fast_lane()
    plain = analyzed(lane).report("ana")["ser"]
    g = analyzed(lane, ffe={"n_taps": 5})
    a, f = g.report("ana"), g.report("ffe")
    assert f["adapted"] and sum(f["taps"]) == pytest.approx(1.0, abs=1e-12)
    assert a["ser_method"] == "semi_analytic"
    assert a["ser"] < plain / 100, (a["ser"], plain)
    assert min(a["eye_height"]) > 0
    # The decision node sees the equalized signal too.
    assert g.receiver().analyze()["ser"] == pytest.approx(a["ser"], rel=1e-12)


def test_renamed_kind_decision_still_loads():
    """Graphs saved with the old kind name `decision` (and node id `dec`) still run."""
    g = lightstone.Graph.reference()
    cfg = json.loads(g.to_json())
    for n in cfg["nodes"]:
        if n["kind"] == "receiver_output":
            n["kind"], n["id"] = "decision", "dec"
    for w in cfg["wires"]:
        w["to"] = w["to"].replace("rx.", "dec.") if w["to"].startswith("rx.") else w["to"]
    old = lightstone.Graph.from_json(json.dumps(cfg))
    a = old.run().receiver()          # finds the only receiver output by itself
    b = lightstone.Graph.reference().run().receiver("rx")
    assert a.analyze()["ser"] == b.analyze()["ser"]


def test_mmf_presets_and_conversion():
    """OM3/OM4/OM5 presets: the reference link uses OM4; setting a physics parameter on an
    `mmf` node converts it to a custom fiber with the preset's values (and warns)."""
    presets = lightstone.fiber_presets()
    assert set(presets["types"]) == {"om3", "om4", "om5"}
    g = lightstone.Graph.reference()
    assert g._node("el1")["kind"] == "mmf" and g.params("el1")["type"] == "om4"
    g.run()
    emb = {}
    for t in ("om3", "om4", "om5"):
        g.set("el1", type=t)
        g.run()
        emb[t] = g.report("el1")["emb_length"] / 1e9
    assert 2000 <= emb["om3"] < 2040 and 4700 <= emb["om4"] < 4750 and 4700 <= emb["om5"] < 4750
    g.set("el1", type="om4").run()
    ser = g.receiver().analyze()["ser"]
    with pytest.warns(UserWarning, match="converted the OM4 preset"):
        g.set("el1", grid_points_per_radius=600)
    assert g._node("el1")["kind"] == "custom_fiber"
    assert g.run().receiver().analyze()["ser"] == ser, "same physics, same result"
    with pytest.raises(lightstone.GraphError, match="custom_fiber"):
        bad = lightstone.Graph.reference()
        bad._node("el1")["params"]["profile"] = {"kind": "step"}
        bad.run()


def _python_example():
    """The Python example component (examples/components/example_component.py)."""
    import importlib.util
    from pathlib import Path

    path = Path(__file__).resolve().parents[2] / "examples" / "components" / "example_component.py"
    spec = importlib.util.spec_from_file_location("example_component", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.example_component


@pytest.mark.parametrize("params", [{}, {"gain": 0.6, "delay": 3.3e-12}])
def test_rust_example_matches_python_example(params):
    """The Rust example (built-in `rust_example`) and the Python example component are the same
    block: identical receiver output in the reference graph (docs/model/rust-example.md)."""
    assert "rust_example" in lightstone.builtin_kinds()
    out = {}
    for kind in ("rust_example", "example_component"):
        g = lightstone.Graph.reference(components=[_python_example()])
        g.insert("ffe", "ex", kind, **params).run()
        out[kind] = g.receiver()
    a, b = out["rust_example"], out["example_component"]
    assert rel_max(a.noiseless, b.noiseless) < 1e-12
    assert rel_max(a.sigma, b.sigma) < 1e-12
    assert a.analyze()["ser"] == pytest.approx(b.analyze()["ser"], rel=1e-9)

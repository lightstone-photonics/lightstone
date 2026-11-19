"""Validation tests that need SciPy or the NumPy reference (V1, V15) plus API checks.

Rust-side validation tests (V2–V13) live next to the code in crates/*/src.
"""

import math

import numpy as np
import pytest
from scipy.optimize import brentq
from scipy.special import jv, kv, jn_zeros

import lightstone
import lightstone_ref


# ------------------------------------------------------------------------------------------
# V1: step-index fiber, V = 4.5, against the analytic LP eigenvalue equation
# ------------------------------------------------------------------------------------------


def lp_analytic(V: float):
    """Solve u J_{l-1}(u)/J_l(u) = -w K_{l-1}(w)/K_l(w), w = sqrt(V² − u²) (weak guidance).
    Returns {(l, m): b} with b = 1 − u²/V²."""
    out = {}
    for l in range(0, 10):
        def f(u):
            w = math.sqrt(max(V * V - u * u, 1e-300))
            return u * jv(l - 1, u) / jv(l, u) + w * kv(l - 1, w) / kv(l, w)

        # Roots lie between consecutive zeros of J_{l-1} (cutoffs) and zeros of J_l.
        zl = [0.0] + list(jn_zeros(l, 20))
        found = 0
        for lo, hi in zip(zl[:-1], zl[1:]):
            lo_, hi_ = lo + 1e-9, min(hi, V) - 1e-9
            if lo_ >= hi_:
                continue
            try:
                u = brentq(f, lo_, hi_, xtol=1e-15, rtol=1e-15, maxiter=500)
            except ValueError:
                continue
            found += 1
            out[(l, found)] = 1 - (u / V) ** 2
        if found == 0:
            break
    return out


def test_v1_step_index_lp_modes():
    lam = 850e-9
    na = 0.1
    V = 4.5
    a = V * lam / (2 * math.pi * na)
    res = lightstone.solve_modes(
        {"core_radius": a, "na": na, "profile": {"kind": "step"}, "grid_points_per_radius": 800, "domain_factor": 8.0},
        lam,
    )
    analytic = lp_analytic(V)
    # V = 4.5 guides LP01, LP11, LP21, LP02 (LP31 cutoff 5.136).
    assert set(analytic) == {(0, 1), (1, 1), (2, 1), (0, 2)}
    got = {(m["l"], m["m"]): m for m in res["modes"]}
    assert set(got) == set(analytic)
    n1, n2 = res["n_core"], res["n_clad"]
    for key, b in analytic.items():
        n_exact = math.sqrt(n2**2 + b * (n1**2 - n2**2))
        err = abs(got[key]["n_eff"] - n_exact)
        assert err < 1e-6, f"LP{key}: Δn_eff = {err:.2e}"


# ------------------------------------------------------------------------------------------
# V15: Rust vs NumPy reference, all deterministic stages
# ------------------------------------------------------------------------------------------


@pytest.fixture(scope="module")
def cross():
    params = {
        "fiber.length": 120.0,
        "fiber.grid_points_per_radius": 300,
        "launch.offset": 4e-6,
        "launch.tilt": 0.02,
        # The NumPy reference models the chain up to the receive filter (no FFE).
        "rx.ffe_taps": 0,
    }
    cfg = lightstone.with_params(None, params)
    rust = lightstone.simulate(cfg)
    ref = lightstone_ref.simulate_noiseless(cfg)
    # Without RIN the reference-receiver waveform is deterministic and comparable.
    rust_no_rin = lightstone.simulate(lightstone.with_params(cfg, {"rx.rin_noise": False}))
    return cfg, rust, ref, rust_no_rin


def rel(a, b):
    a, b = np.asarray(a), np.asarray(b)
    return np.max(np.abs(a - b)) / np.max(np.abs(b))


def test_v15_transmitter(cross):
    _, rust, ref, _ = cross
    assert np.array_equal(rust.symbols, ref["symbols"])
    assert rel(rust.tx_power, ref["tx_power"]) < 1e-9


def test_v15_modes(cross):
    _, rust, ref, _ = cross
    rm = rust.modes
    fm = ref["modes"]["modes"]
    assert len(rm) == len(fm)
    lp01 = fm[0]["group_delay"]
    for a, b in zip(rm, fm):
        assert (a["l"], a["m"]) == (b["l"], b["m"])
        assert abs(a["n_eff"] - b["n_eff"]) < 1e-12  # independent eigen solvers
        # Relative delays to LP01: compare in absolute terms (s/m).
        # Delays/slopes are finite-difference derivatives of n_eff(λ); the ~1e-13 eigenvalue
        # agreement of the two solvers is amplified to ~1e-8 relative. Tolerance: 1e-6.
        spread = max(abs(m["group_delay"] - lp01) for m in fm)
        assert abs(a["relative_delay"] - (b["group_delay"] - lp01)) < 1e-6 * spread
        assert abs(a["chromatic_slope"] - b["chromatic_slope"]) < 1e-6 * abs(b["chromatic_slope"])


def test_v15_coupling(cross):
    _, rust, ref, _ = cross
    p_rust = np.array([m["launched_power"] for m in rust.modes])
    assert np.max(np.abs(p_rust - ref["launch_power"])) < 1e-9 * np.max(ref["launch_power"])
    assert abs(rust.metrics["guided_fraction"] - ref["guided_fraction"]) < 1e-9


def test_v15_channel_and_receiver(cross):
    _, rust, ref, rust_no_rin = cross
    # Inherits the ~1e-8 delay/slope agreement (see test_v15_modes).
    assert rel(rust.rx_optical, ref["rx_optical"]) < 1e-7
    assert rel(rust.rx_noiseless, ref["rx_noiseless"]) < 1e-7
    assert rel(rust_no_rin.reference_optical, ref["reference_optical"]) < 1e-7
    assert rel(rust.rx_sigma, ref["rx_sigma"]) < 1e-7


# ------------------------------------------------------------------------------------------
# API behavior
# ------------------------------------------------------------------------------------------


def test_with_params_rejects_unknown_keys():
    with pytest.raises(KeyError):
        lightstone.with_params(None, {"fiber.lenght": 1.0})


def test_simulator_cache_consistency():
    """A cached simulator gives the same result as a fresh one."""
    sim = lightstone.Simulator({"fiber.length": 80.0})
    sim.run()
    sim.set(fiber__length=40.0, launch__offset=2e-6)
    a = sim.run()
    b = lightstone.simulate(sim.config)
    assert np.array_equal(a.rx_signal, b.rx_signal)


@pytest.mark.parametrize("length", [0.0, 50.0, 150.0])
def test_noise_statistics_match_sigma(length):
    """Simulated noise (thermal + shot + RIN) matches the analytic σ used for the SER."""
    r = lightstone.simulate({"fiber.length": length, "tx.pattern_periods": 4})
    resid = r.rx_signal - r.rx_noiseless
    ratio = np.var(resid) / np.mean(r.rx_sigma**2)
    assert abs(ratio - 1) < 0.02, ratio


def test_tdecq_increases_with_length():
    sim = lightstone.Simulator()
    vals = []
    for L in (0.0, 100.0, 150.0):
        vals.append(sim.set(fiber__length=L).run().tdecq()["tdecq_db"])
    assert vals[0] < vals[1] < vals[2], vals


def test_eye_density_shape():
    r = lightstone.simulate({"fiber.length": 10.0})
    counts, meta = r.eye(width=120, height=80)
    assert counts.shape == (80, 120)
    assert counts.sum() > 0
    assert meta["v_min"] < meta["v_max"]

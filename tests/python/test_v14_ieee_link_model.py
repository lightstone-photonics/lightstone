"""V14a: NRZ eye closure vs. length against the IEEE 802.3ae 10G Ethernet link model.

1. The re-implemented link model equations (ieee_link_model.py) reproduce the computed cells
   of the five 850 nm sheets of 10GEPBud3_1_16a.xls (data/ieee_10g_link_model.json).
2. Lightstone simulates a 10GBASE-SR-like NRZ link (10.3125 GBd, 840 nm, OM3-class fiber
   with an effective modal bandwidth of ≈ 2000 MHz·km, two index profiles with the same EMB:
   under- and over-compensated). Its worst-case noiseless eye closure (VECP) at 0–300 m
   must agree with the link model's ISI penalty P_isi within 0.5 dB when the link model is
   given Lightstone's own component values: back-to-back rise time (measured on the
   simulated waveform), EMB (computed from the mode delays and the launch), chromatic
   dispersion (launch-weighted RMS of the mode dispersions) and source spectral width. No
   duty-cycle distortion, no noise (P_isi is a noise-free quantity).

The link model treats every element as a Gaussian; Lightstone propagates the guided modes
with their own delays (non-Gaussian impulse response), a 2nd-order VCSEL and a 4th-order
Bessel–Thomson receiver. The test therefore checks the fiber and eye model against the
standard's methodology, not the spreadsheet against itself.
"""

import json
import math
from pathlib import Path

import numpy as np
import pytest

import lightstone as ls
from ieee_link_model import RISE_PER_SIGMA, p_isi, sheet_row, tc

DATA = json.loads((Path(__file__).parent / "data" / "ieee_10g_link_model.json").read_text())

BAUD = 10.3125e9
SPS = 16
WAVELENGTH = 840e-9
WIDTH = 0.29e-9  # RMS spectral width as in the 850 nm sheets [m]
# Index profiles giving EMB ≈ 2000 MHz·km with the default VCSEL launch (both sides of the
# optimum α ≈ 2.035 of the germania-doped core).
PROFILES = {"under": 1.9708, "over": 2.1256}
LENGTHS = [0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0]
TOLERANCE_DB = 0.5


# ---- 1. re-implementation vs. the spreadsheet ----------------------------------------------

@pytest.mark.parametrize("sheet", sorted(DATA["sheets"]))
def test_v14a_equations_reproduce_spreadsheet(sheet):
    s = DATA["sheets"][sheet]
    for row in s["rows"]:
        mine = sheet_row(s["inputs"], row["length_km"])
        for k in ("d1l_ps_nm", "bw_cd_mhz", "eff_bwm_mhz", "te_ps", "tc_ps"):
            assert mine[k] == pytest.approx(row[k], rel=1e-7), (sheet, row["length_km"], k)
        # The sheet's σ factor is 2.563 to ≈ 4 digits; P_isi agrees to < 2·10⁻³ dB.
        assert mine["p_isi_db"] == pytest.approx(row["p_isi_db"], abs=2e-3), (sheet, row["length_km"])


def test_v14a_sheet_values():
    """Spot values of the OM3 (2000 MHz·km) sheet, as quoted in the docs."""
    rows = {round(r["length_km"] * 1000): r for r in DATA["sheets"]["850S2000"]["rows"]}
    assert rows[2]["p_isi_db"] == pytest.approx(0.750, abs=1e-3)
    assert rows[300]["p_isi_db"] == pytest.approx(3.018, abs=1e-3)


# ---- 2. Lightstone vs. the link model --------------------------------------------------------

def nrz_graph(alpha: float) -> ls.Graph:
    g = ls.Graph.reference(lane_config={"tx": {"sps": SPS, "baud": BAUD, "levels": 2}, "rx": {"ffe_taps": 0}})
    # VCSEL: critically damped 10 GHz response, no RIN; receiver noise off (P_isi is
    # noise-free); 4th-order Bessel–Thomson at 0.8 × baud = 8.25 GHz (the sheet's Rx BW).
    g.set("vcsel", rin_enabled=False,
          vcsel={"resonance_freq": 10e9, "damping": 1.0, "wavelength": WAVELENGTH,
                 "spectral_width_rms": WIDTH, "rin_db_hz": -140.0})
    # OM3-class fiber with a chosen profile: a custom fiber built from the OM4 preset.
    g.replace("el1", "custom_fiber", {**ls.fiber_presets()["types"]["om4"], "length": 100.0,
                                       "grid_points_per_radius": 300,
                                       "profile": {"kind": "graded", "alpha": alpha}})
    g.set("pin", shot_noise=False)
    g.set("tia", thermal_noise=False)
    g.set("rxf", f3db_rel=0.8)
    return g


def received(g: ls.Graph):
    """Noiseless receiver waveform, symbols, and alignment delay [samples]."""
    rx = g.receiver("rx")
    y, s = np.asarray(rx.noiseless), np.asarray(rx.symbols)
    return y, s, ls.analyze(y, s, SPS, levels=2)["delay"]


def vecp(y, s, delay) -> float:
    """Worst-case eye closure at the best sampling instant, relative to the settled swing [dB]."""
    n = len(s)
    best = (-math.inf, 0)
    for o in range(-SPS // 2, SPS + SPS // 2):
        v = y[(delay + np.arange(n) * SPS + o) % y.size]
        e = v[s == 1].min() - v[s == 0].max()
        if e > best[0]:
            best = (e, o)
    v = y[(delay + np.arange(n) * SPS + best[1]) % y.size]
    # Settled levels: centres of runs of ≥ 7 equal symbols.
    settled = np.ones(n, bool)
    for k in range(1, 4):
        settled &= (np.roll(s, k) == s) & (np.roll(s, -k) == s)
    swing = v[settled & (s == 1)].mean() - v[settled & (s == 0)].mean()
    return -10 * math.log10(best[0] / swing)


def rise_10_90(y, s, delay) -> float:
    """Mean 10–90 % rise time [s] of 0→1 edges between runs of ≥ 6 zeros and ≥ 6 ones."""
    n, ts = len(s), []
    for i in range(n):
        if s[i] != 1 or not all(s[(i - k) % n] == 0 for k in range(1, 7)):
            continue
        if not all(s[(i + k) % n] == 1 for k in range(6)):
            continue
        seg = y[(delay + (i - 6) * SPS + np.arange(12 * SPS)) % y.size]
        lo, hi = seg[SPS:2 * SPS].mean(), seg[-2 * SPS:-SPS].mean()
        u = (seg - lo) / (hi - lo)

        def cross(th):
            j = 3 * SPS + int(np.argmax(u[3 * SPS:] > th))
            return j - 1 + (th - u[j - 1]) / (u[j] - u[j - 1])

        ts.append((cross(0.9) - cross(0.1)) / (SPS * BAUD))
    assert len(ts) >= 2  # PRBS13 has two such edges
    return float(np.mean(ts))


def effective_dispersion(alpha: float) -> float:
    """Launch-weighted RMS chromatic dispersion of the guided modes [ps/(nm·km)]."""
    sim = ls.Simulator()
    sim.set(**{"vcsel.wavelength": WAVELENGTH, "tx.sps": 8, "fiber.grid_points_per_radius": 300,
               "fiber.profile": {"kind": "graded", "alpha": alpha}})
    r = sim.run()
    p = np.array([m["launched_power"] for m in r.modes])
    d = np.array([m["chromatic_slope"] for m in r.modes])  # [s/m²]
    return math.sqrt(np.sum(p * d**2) / np.sum(p)) * 1e6


@pytest.fixture(scope="module", params=sorted(PROFILES))
def comparison(request):
    alpha = PROFILES[request.param]
    g = nrz_graph(alpha)
    rows = []
    t_b2b = emb = None
    for length in LENGTHS:
        g.set("el1", length=length)
        g.run()
        y, s, d = received(g)
        if length == 0.0:
            t_b2b = rise_10_90(y, s, d) * 1e12  # [ps]: VCSEL + receiver, in quadrature
            emb = g.report("el1")["emb_length"] / 1e9  # [MHz·km]
        rows.append((length, vecp(y, s, d)))
    disp = effective_dispersion(alpha)
    model = [p_isi(tc(t_b2b, emb, L / 1e3, disp, WIDTH * 1e9, 0.0)[1], BAUD / 1e6) for L, _ in rows]
    return {"profile": request.param, "alpha": alpha, "t_b2b": t_b2b, "emb": emb, "dispersion": disp,
            "rows": [(L, v, m) for (L, v), m in zip(rows, model)]}


def test_v14a_lightstone_matches_link_model(comparison):
    c = comparison
    print(f"\nV14a {c['profile']}-compensated α = {c['alpha']}: back-to-back 10–90 % "
          f"{c['t_b2b']:.1f} ps, EMB {c['emb']:.0f} MHz·km, D_rms {c['dispersion']:.1f} ps/(nm·km)")
    for L, v, m in c["rows"]:
        print(f"  {L:5.0f} m  VECP Lightstone {v:6.3f} dB  link model {m:6.3f} dB  Δ {v - m:+.3f} dB")
    assert 1900 < c["emb"] < 2200, "OM3-class fiber"
    for L, v, m in c["rows"]:
        assert abs(v - m) <= TOLERANCE_DB, f"{L} m: {v:.3f} vs {m:.3f} dB"
    vs = [v for _, v, _ in c["rows"]]
    assert all(b > a for a, b in zip(vs, vs[1:])), "eye closure grows with length"


# ---- 3. V14c: duty-cycle distortion -------------------------------------------------------

DCD_PS = [5.0, 10.0]


@pytest.mark.parametrize("dcd_ps", DCD_PS)
def test_v14c_dcd_matches_link_model(dcd_ps):
    """V14c: duty-cycle distortion at the driver (rising edges DCD/2 early, falling edges DCD/2
    late: isolated 1s are T + DCD wide, isolated 0s T − DCD).

    The link model's P_isi(T_c, DCD) narrows *both* isolated pulses to T − DCD, a worst-case
    allowance. The same Gaussian eye model with the physical pulse widths gives the opening
    erf((T+DCD)/(2√2σ)) + erf((T−DCD)/(2√2σ)) − 1. Lightstone's VECP must agree with that
    within the V14a tolerance, its DCD increment within 0.02 dB, and stay below the
    spreadsheet's conservative value."""
    alpha = PROFILES["over"]
    g = nrz_graph(alpha)
    g.set("el1", length=0.0)
    g.run()
    y, s, d = received(g)
    t_b2b = rise_10_90(y, s, d) * 1e12
    emb = g.report("el1")["emb_length"] / 1e9
    disp = effective_dispersion(alpha)
    tb = 1e12 / BAUD
    print(f"\nV14c DCD {dcd_ps} ps ({dcd_ps * BAUD * 1e-12:.3f} UI)")
    for length in (0.0, 100.0, 200.0, 300.0):
        g.set("el1", length=length)
        vs = []
        for dcd in (0.0, dcd_ps):
            g.set("drv", dcd=dcd * 1e-12)
            g.run()
            vs.append(vecp(*received(g)))
        t_c = tc(t_b2b, emb, length / 1e3, disp, WIDTH * 1e9, 0.0)[1]
        sigma = t_c / RISE_PER_SIGMA
        erf = lambda w: math.erf(w / (2 * math.sqrt(2) * sigma))  # noqa: E731
        m = -10 * math.log10(erf(tb + dcd_ps) + erf(tb - dcd_ps) - 1)
        m0, ieee = p_isi(t_c, BAUD / 1e6), p_isi(t_c, BAUD / 1e6, dcd_ps)
        v0, v = vs
        print(f"  {length:5.0f} m  VECP {v:6.3f} dB (+{v - v0:.3f})  Gaussian model {m:6.3f} dB "
              f"(+{m - m0:.3f})  spreadsheet {ieee:6.3f} dB")
        assert abs(v - m) <= TOLERANCE_DB, f"{length} m: {v:.3f} vs {m:.3f} dB"
        assert abs((v - v0) - (m - m0)) <= 0.02, f"{length} m: DCD increment"
        assert v - v0 > 0 and v < ieee

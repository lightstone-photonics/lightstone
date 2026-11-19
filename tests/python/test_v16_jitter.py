"""V16b: transmitter and receiver jitter against the dual-Dirac model.

A 10.3125 GBd NRZ back-to-back link with fast components (VCSEL f_r = 50 GHz, receive filter
4 × baud, no RIN or receiver noise) so pattern-dependent jitter is negligible. The driver adds
random jitter (Gaussian, σ) and duty-cycle distortion (DCD, peak-to-peak). The semi-analytic
BER from the receiver σ(t) as a function of the sampling instant (bathtub) must agree with the
dual-Dirac model of edge shifts

    BER(t) = ¼·[Q((t + D/2)/σ) + Q((t − D/2)/σ) + Q((T − D/2 − t)/σ) + Q((T + D/2 − t)/σ)]

(transition density ½, rising and falling edges equally likely), i.e. its eye width at BER b
is T − TJ(b) with total jitter TJ = DJ + 2·Q⁻¹(·)·σ. With σ = 0.002 UI, DCD = 0.08 UI the
widths agree within 0.5 σ down to 10⁻¹²; the slope-noise model is first order, so the
deviation grows with Q·σ relative to the edge rise time. Random jitter of the receiver clock
(analysis option) adds to the semi-analytic SER.
"""

import math

import numpy as np
import pytest

import lightstone as ls
from scipy.special import erfc

BAUD = 10.3125e9
SPS = 64
UI = 1 / BAUD
RJ = 0.002  # [UI] (0.19 ps)
DCD = 0.08  # [UI] (7.8 ps)


def q(x):
    return 0.5 * math.erfc(x / math.sqrt(2))


def fast_nrz(rj_ui=RJ, dcd_ui=DCD) -> ls.Graph:
    g = ls.Graph.reference(lane_config={"tx": {"sps": SPS, "baud": BAUD, "levels": 2}, "rx": {"ffe_taps": 0}})
    g.set("vcsel", rin_enabled=False, vcsel={"resonance_freq": 50e9, "damping": 0.7})
    g.set("el1", length=0.0)
    g.set("pin", shot_noise=False)
    g.set("tia", thermal_noise=False)
    g.set("rxf", f3db_rel=4.0)
    g.set("drv", rj_rms=rj_ui * UI, dcd=dcd_ui * UI)
    return g


def bathtub(rx, sub=16):
    """Semi-analytic BER vs sampling instant [UI] (threshold at the level midpoint), on a grid
    `sub` times finer than the samples (y₀ and σ interpolated linearly)."""
    y, sg, s = np.asarray(rx.noiseless), np.asarray(rx.sigma), np.asarray(rx.symbols)
    d = ls.analyze(y, s, SPS, y, sg, levels=2)["delay"]
    n = len(s)
    base = d + np.arange(n) * SPS
    mid = y[(base + SPS // 2) % y.size]
    th = 0.5 * (mid[s == 1].mean() + mid[s == 0].mean())
    idx = np.arange(y.size + 1)
    yw, sw = np.append(y, y[0]), np.append(sg, sg[0])
    phases = np.arange(-SPS // 2 * sub, (SPS + SPS // 2) * sub) / sub
    ber = []
    for p in phases:
        x = (base + p) % y.size
        v, sv = np.interp(x, idx, yw), np.interp(x, idx, sw)
        z = np.where(s == 1, v - th, th - v) / np.maximum(sv, 1e-300)
        ber.append(np.mean(0.5 * erfc(np.minimum(z, 40) / math.sqrt(2))))
    return phases / SPS, np.array(ber)


def width_at(t, ber, b):
    """Eye width [UI] where the bathtub is below b (log-linear interpolation)."""
    lb = np.log10(np.maximum(ber, 1e-300))
    i0 = int(np.argmin(lb))
    left = next(i for i in range(i0, 0, -1) if lb[i - 1] >= math.log10(b))
    right = next(i for i in range(i0, len(lb) - 1) if lb[i + 1] >= math.log10(b))
    xl = t[left - 1] + (math.log10(b) - lb[left - 1]) / (lb[left] - lb[left - 1]) * (t[left] - t[left - 1])
    xr = t[right] + (math.log10(b) - lb[right]) / (lb[right + 1] - lb[right]) * (t[right + 1] - t[right])
    return xr - xl


def dual_dirac_width(b, sigma, dj):
    """Eye width [UI] of the dual-Dirac bathtub at BER b."""
    def ber(x):  # x: distance from the eye centre
        t = 0.5 + x
        return 0.25 * (q((t + dj / 2) / sigma) + q((t - dj / 2) / sigma)
                       + q((1 - dj / 2 - t) / sigma) + q((1 + dj / 2 - t) / sigma))
    lo, hi = 0.0, 0.5
    for _ in range(100):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if ber(mid) < b else (lo, mid)
    return 2 * lo


@pytest.fixture(scope="module")
def tx_jitter():
    g = fast_nrz()
    g.run()
    return bathtub(g.receiver("rx"))


@pytest.mark.parametrize("b", [1e-3, 1e-6, 1e-9, 1e-12])
def test_v16b_bathtub_matches_dual_dirac(tx_jitter, b):
    t, ber = tx_jitter
    w = width_at(t, ber, b)
    m = dual_dirac_width(b, RJ, DCD)
    print(f"\nBER {b:.0e}: eye width {w:.4f} UI, dual-Dirac {m:.4f} UI, TJ {1 - w:.4f} UI")
    # First-order slope noise on a finite edge: within 0.5 σ down to 1e-12.
    assert abs(w - m) < 0.5 * RJ


def test_v16b_pattern_jitter_negligible():
    """Without jitter the eye is open over (almost) the whole UI at 1e-12 — the fast link."""
    g = fast_nrz(rj_ui=1e-4, dcd_ui=0.0)
    g.run()
    t, ber = bathtub(g.receiver("rx"))
    assert width_at(t, ber, 1e-12) > 1 - 0.01


def test_v16b_clock_jitter_increases_ser():
    """Receiver clock RJ (analysis option) adds to the noise-limited SER; it is not in the
    waveform."""
    g = fast_nrz(rj_ui=0.03, dcd_ui=0.0)
    g.set("vcsel", vcsel={"resonance_freq": 8e9, "damping": 0.7}, p_avg_dbm=-16.0)
    g.set("rxf", f3db_rel=0.5)
    g.set("tia", thermal_noise=True)
    g.run()
    rx = g.receiver("rx")
    tx_only, with_clock = rx.analyze()["ser"], rx.analyze(clock_rj_rms_ui=0.03)["ser"]
    g.set("drv", rj_rms=0.0)
    g.run()
    none = g.receiver("rx").analyze()["ser"]
    print(f"\nSER no jitter {none:.3e}, TX RJ {tx_only:.3e}, TX + clock RJ {with_clock:.3e}")
    assert 0 < none < tx_only < with_clock

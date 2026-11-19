"""V14b: PAM-4 (53.125 GBd) TDECQ over worst-case OM4 against the IEEE 802.3db fiber emulation.

There is no public PAM-4 multimode link model spreadsheet. What 802.3db publishes is how the
worst-case OM4 channel enters TDECQ: a fiber emulation filter of bandwidth f_A (Table 167-12),
derived from EMB, chromatic dispersion and source spectral width with the 10GE link model
method (see ieee_link_model.py for the equations and sources).

1. The re-implemented method reproduces the published bandwidths: f_A = 18.0 GHz (100 m) and
   33.6 GHz (50 m), and the combined fiber + receiver reference responses 15.0 and 21.0 GHz
   of the baseline proposal, within 2.5 %.
2. Shape: TDECQ through Lightstone's mode-resolved fiber equals TDECQ through a Gaussian
   emulation filter of the same −3 dB bandwidth within 0.1 dB, so the standard's Gaussian
   fiber emulation is a faithful stand-in at 53 GBd (two OM4 profiles, 50 and 100 m).
3. Worst case: with the 802.3db conditions (EMB ≈ 4700 MHz·km, 844 nm and 0.6 nm RMS width
   at 100 m; 842 nm and 0.65 nm at 50 m), Lightstone's TDECQ agrees with TDECQ measured with
   the Table 167-12 emulation filter within 0.5 dB, when the chromatic dispersion is matched
   (see below).

Known difference: Lightstone's germania-doped core gives D ≈ −95.5 ps/(nm·km) for the launched
modes at 844 nm; 802.3db uses the measured MMF value D = −101 ps/(nm·km). Step 3 therefore
scales the source width by the ratio of the two (the chromatic term depends only on D·Δλ), and
also reports the unmatched result: 0.1–0.3 dB less TDECQ than the standard at 100 m (with the
pure-silica fiber of v0.11: 0.4–0.6 dB).

Same transmitter (default VCSEL, 0 dBm, ER 4 dB, RIN off) and the same TDECQ implementation
(5-tap reference FFE, 26.5625 GHz Bessel–Thomson reference receiver) on both sides; 802.3db
uses a 9-tap FFE, which is not modelled here.
"""

import math

import numpy as np
import pytest

import lightstone as ls
from ieee_link_model import (
    EMB_OM4,
    bessel_thomson4,
    f3e_of,
    fiber_emulation_bandwidth,
    gaussian_response,
    mmf_dispersion,
)

BAUD = 53.125e9
SPS = 16
RX_BT = 26.5625e9  # reference receiver (half the symbol rate)
# Published values (murty_3db_01a_092321: Table 167-12; murty_3db_adhoc_01a_121720: baseline).
PMD = {
    # length: (centre wavelength [nm], RMS width [nm], f_A [GHz], reference response [GHz])
    100.0: (844.0, 0.60, 18.0, 15.0),
    50.0: (842.0, 0.65, 33.6, 21.0),
}
# Index profiles with EMB ≈ 4700 MHz·km for the default VCSEL launch at 844 nm (both sides of
# the optimum α ≈ 2.035 of the germania-doped core).
PROFILES = {"under": 2.0119, "over": 2.0802}
TOL_DB = 0.5


# ---- 1. the standard's numbers ---------------------------------------------------------------

@pytest.mark.parametrize("length", sorted(PMD))
def test_v14b_method_reproduces_published_bandwidths(length):
    lam, width, fa_pub, ref_pub = PMD[length]
    fa = fiber_emulation_bandwidth(length, EMB_OM4, mmf_dispersion(lam), width)
    assert fa / 1e9 == pytest.approx(fa_pub, rel=0.025), "Table 167-12 fiber emulation bandwidth"
    combined = f3e_of(lambda f: gaussian_response(f, fa) * bessel_thomson4(f, RX_BT))
    assert combined / 1e9 == pytest.approx(ref_pub, rel=0.025), "fiber + receiver reference response"


# ---- Lightstone side -------------------------------------------------------------------------

def run(alpha: float, length: float, wavelength_nm: float, width_nm: float):
    s = ls.Simulator()
    s.set(**{"tx.sps": SPS, "tx.baud": BAUD, "vcsel.wavelength": wavelength_nm * 1e-9,
             "vcsel.spectral_width_rms": width_nm * 1e-9, "fiber.length": length,
             "fiber.grid_points_per_radius": 300, "fiber.profile": {"kind": "graded", "alpha": alpha},
             "rx.rin_noise": False})
    return s.run()


def channel_f3e(r) -> float:
    """Electrical −3 dB bandwidth of the simulated fiber channel: FFT(rx_optical)/FFT(tx_power)."""
    x, y = np.fft.rfft(r.tx_power), np.fft.rfft(r.rx_optical)
    f = np.fft.rfftfreq(r.tx_power.size, 1 / r.sample_rate)
    ok = (np.abs(x) > 1e-6 * np.abs(x).max()) & (f < 0.9 * BAUD)
    f, h2 = f[ok], (np.abs(y[ok] / x[ok]) / abs(y[0] / x[0])) ** 2
    i = int(np.argmax(h2 < 0.5))
    assert i > 0
    return f[i - 1] + (0.5 - h2[i - 1]) / (h2[i] - h2[i - 1]) * (f[i] - f[i - 1])


def launched_dispersion(r) -> float:
    """Launch-weighted RMS chromatic dispersion of the guided modes [ps/(nm·km)] (negative)."""
    p = np.array([m["launched_power"] for m in r.modes])
    d = np.array([m["chromatic_slope"] for m in r.modes])
    return -math.sqrt(np.sum(p * d * d) / np.sum(p)) * 1e6


def tdecq_emulated(b2b, f3e: float) -> float:
    """TDECQ of the back-to-back reference waveform through a Gaussian fiber emulation filter."""
    w = np.asarray(b2b.reference_optical)
    f = np.fft.rfftfreq(w.size, 1 / b2b.sample_rate)
    w = np.fft.irfft(np.fft.rfft(w) * gaussian_response(f, f3e), n=w.size)
    return ls.tdecq(w, b2b.symbols, SPS, BAUD)["tdecq_db"]


@pytest.fixture(scope="module", params=sorted(PROFILES))
def profile(request):
    alpha = PROFILES[request.param]
    rows = {}
    for length, (lam, width, fa_pub, _) in PMD.items():
        b2b = run(alpha, 0.0, lam, width)
        r = run(alpha, length, lam, width)
        d_ls = launched_dispersion(r)
        d_std = mmf_dispersion(lam)
        # Matched: same D·Δλ as the standard.
        rm = run(alpha, length, lam, width * d_std / d_ls)
        rows[length] = {
            "emb": r.metrics["emb_length"] / 1e9,
            "d_ls": d_ls, "d_std": d_std,
            "f3e": channel_f3e(r), "f3e_matched": channel_f3e(rm),
            "tdecq": r.tdecq()["tdecq_db"], "tdecq_matched": rm.tdecq()["tdecq_db"],
            "tdecq_same_bw": tdecq_emulated(b2b, channel_f3e(r)),
            "tdecq_table": tdecq_emulated(b2b, fa_pub * 1e9),
            "tdecq_b2b": b2b.tdecq()["tdecq_db"],
        }
    return request.param, alpha, rows


def _print(name, alpha, rows):
    print(f"\nV14b {name}-compensated α = {alpha}")
    for L, x in sorted(rows.items()):
        print(f"  {L:4.0f} m: EMB {x['emb']:.0f} MHz·km, D {x['d_ls']:.1f} (802.3: {x['d_std']:.1f}) ps/(nm·km), "
              f"fiber f3e {x['f3e']/1e9:.2f} GHz (matched {x['f3e_matched']/1e9:.2f}, Table 167-12 {PMD[L][2]})")
        print(f"         TDECQ b2b {x['tdecq_b2b']:.2f} | Lightstone {x['tdecq']:.3f}, Gaussian same f3e {x['tdecq_same_bw']:.3f} dB | "
              f"matched {x['tdecq_matched']:.3f} vs Table 167-12 {x['tdecq_table']:.3f} dB "
              f"(Δ {x['tdecq_matched'] - x['tdecq_table']:+.3f}; unmatched Δ {x['tdecq'] - x['tdecq_table']:+.3f})")


def test_v14b_gaussian_emulation_matches_mode_resolved_fiber(profile):
    name, alpha, rows = profile
    _print(name, alpha, rows)
    for L, x in rows.items():
        assert 4500 < x["emb"] < 4900, "worst-case OM4 (EMB ≈ 4700 MHz·km)"
        assert abs(x["tdecq"] - x["tdecq_same_bw"]) <= 0.1, f"{L} m"


def test_v14b_worst_case_tdecq_matches_802_3db(profile):
    _, _, rows = profile
    for L, x in rows.items():
        assert abs(x["tdecq_matched"] - x["tdecq_table"]) <= TOL_DB, f"{L} m"
        assert x["tdecq_matched"] > x["tdecq_b2b"], "the channel adds TDECQ"
    # The documented dispersion gap: unmatched Lightstone gives less TDECQ at 100 m.
    assert rows[100.0]["tdecq"] < rows[100.0]["tdecq_table"]

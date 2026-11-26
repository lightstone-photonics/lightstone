"""Measured-waveform import (lightstone.measured), the SSPRQ pattern and TDECQ's ffe_taps option.

The import is checked with simulated "captures": the reference link's reference-receiver
waveform, resampled to another sample rate and length, must give back the same TDECQ.
"""

import json
import math

import lightstone_ref as R
import numpy as np
import pytest

import lightstone

BAUD = 53.125e9


@pytest.fixture(scope="module")
def link():
    r = lightstone.simulate({"tx.sps": 16, "fiber.length": 50.0})
    ref = lightstone.tdecq(r.reference_optical, r.symbols, r.sps, r.baud)["tdecq_db"]
    return r, ref


def capture(x, sps_in, sps_out, periods=1.0):
    """A scope-like capture: x (one period at sps_in) resampled band-limited to sps_out samples
    per UI, repeated, and cut to `periods` periods."""
    n_sym = x.size // sps_in
    y = lightstone.measured._fft_resample(x, n_sym * sps_out)
    k = math.ceil(periods)
    return np.tile(y, k)[: int(round(periods * y.size))]


def test_ssprq_pattern():
    s = lightstone.test_pattern("ssprq")
    assert s.size == 65535 and set(np.unique(s)) == {0, 1, 2, 3}
    assert np.array_equal(s[16384:32768], 3 - s[:16384])
    assert lightstone.test_pattern("prbs13q").size == 8191
    with pytest.raises(ValueError):
        lightstone.test_pattern("ssprq", levels=2)


@pytest.mark.parametrize("sps_in, periods", [(23, 1.0), (40, 3.0)])
def test_import_whole_periods_reproduces_tdecq(link, sps_in, periods):
    r, ref = link
    cap = capture(r.reference_optical, r.sps, sps_in, periods)
    m = lightstone.measured_waveform(cap, BAUD * sps_in, BAUD, r.symbols, sps=16)
    assert "FFT" in m.info["method"]
    assert m.signal.size == r.symbols.size * 16
    t = m.tdecq()["tdecq_db"]
    print(f"\n{sps_in} samples/UI, {periods} periods: TDECQ {t:.4f} dB (direct {ref:.4f} dB)")
    assert t == pytest.approx(ref, abs=0.01)


def test_import_partial_capture_interpolates(link):
    r, ref = link
    cap = capture(r.reference_optical, r.sps, 64, 1.3)
    m = lightstone.measured_waveform(cap, BAUD * 64, BAUD, r.symbols, sps=16)
    assert "interpolation" in m.info["method"]
    assert m.tdecq()["tdecq_db"] == pytest.approx(ref, abs=0.05)
    with pytest.raises(ValueError, match="at least one period"):
        lightstone.measured_waveform(cap[: cap.size // 2], BAUD * 64, BAUD, r.symbols)


def test_reference_filter_matches_bessel_thomson():
    f = np.linspace(0, 200e9, 1001)
    a = lightstone.measured._bt4(f, 0.5 * BAUD)
    b = R.bt4_response(f, 0.5 * BAUD)
    assert np.max(np.abs(a - b)) < 1e-12


def test_read_waveform_csv(tmp_path, link):
    r, ref = link
    cap = capture(r.reference_optical, r.sps, 20, 1.0)
    fs = BAUD * 20
    p = tmp_path / "scope.csv"
    with open(p, "w") as fh:
        fh.write("# exported by a scope\ntime [s],power [W]\n")
        for i, v in enumerate(cap):
            fh.write(f"{i / fs:.12e},{v:.9e}\n")
    sym = tmp_path / "pattern.txt"
    sym.write_text("\n".join(str(int(k)) for k in r.symbols))
    m = lightstone.read_waveform(p, BAUD, symbols=sym, sps=16)
    assert m.info["sample_rate"] == pytest.approx(fs, rel=1e-6)
    assert m.tdecq()["tdecq_db"] == pytest.approx(ref, abs=0.01)
    np.save(tmp_path / "x.npy", cap)
    m2 = lightstone.read_waveform(tmp_path / "x.npy", BAUD, symbols=r.symbols, sample_rate=fs, sps=16)
    assert m2.tdecq()["tdecq_db"] == pytest.approx(ref, abs=0.01)


def test_tdecq_ffe_taps(link):
    r, _ = link
    t = {n: lightstone.tdecq(r.reference_optical, r.symbols, r.sps, r.baud, ffe_taps=n) for n in (1, 5, 9)}
    assert t[1]["taps"] == [1.0] and abs(t[1]["ceq_db"]) < 1e-9
    assert len(t[5]["taps"]) == 5 and len(t[9]["taps"]) == 9
    vals = [t[n]["tdecq_db"] for n in (1, 5, 9)]
    print("\n50 m TDECQ with 1 / 5 / 9 taps:", vals)
    assert vals[0] is None or vals[0] > vals[1]
    assert vals[2] <= vals[1] + 0.02
    with pytest.raises(ValueError, match="odd"):
        lightstone.tdecq(r.reference_optical, r.symbols, r.sps, r.baud, ffe_taps=4)


def test_ssprq_in_a_graph():
    g = lightstone.Graph.reference(lane_config={"tx": {"sps": 8}})
    g.settings["pattern_length"] = 65535
    g.set("src", pattern="ssprq").add("tq", "tdecq", ffe_taps=5).wire("el1", "tq").wire("src", "tq.ref").run()
    assert np.array_equal(g.output("src")["symbols"], lightstone.test_pattern("ssprq"))
    a = g.receiver().analyze()
    assert a["ser"] < 2.4e-4
    print(f"\nSSPRQ link: SER {a['ser']:.2e}, TDECQ {g.report('tq')['tdecq_db']:.2f} dB")
    g2 = lightstone.Graph.reference(lane_config={"tx": {"sps": 8}}).set("src", pattern="ssprq")
    with pytest.raises(lightstone.GraphError, match="pattern_length"):
        g2.run()
    json.dumps(g.config)  # still a plain JSON graph

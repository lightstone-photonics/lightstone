"""V19: TDECQ against an independent implementation of the IEEE 802.3 method.

No public IEEE reference code or reference waveforms exist; the standard specifies TDECQ in
text and commercial scopes implement it. The oracle is `lightstone_ref.TdecqReference`, written
from the text of P802.3bs D3.1 121.8.4/121.8.5.3 (king_03_0217_smf), the histogram shift of
king_3bs_01a_0917 and the Ceq relation of dawe_3cd_01b_0718 — not from the Rust code. It uses
symbol-agnostic histograms, the eye centre from the mean P_ave crossing time (pair shift
optimized), taps optimized for minimum TDECQ (Nelder–Mead), OMAouter per 121.8.4, band-limited
interpolation and Ceq from the noise autocorrelation.

1. Analytic: ISI-free PAM-4 with Gaussian noise of RMS σ_s (exact per-level quantiles) has
   TDECQ = 10·log₁₀(σ_ideal/√(σ_ideal² − σ_s²)), σ_ideal = OMA/(6·Q_t). Both within 0.02 dB.
2. Mechanics: with Lightstone's taps, phase and per-symbol histograms the oracle reproduces
   Lightstone's TDECQ within 0.02 dB (same Ceq within 1e-6 dB, same OMAouter).
3. Method: Lightstone (MMSE taps, phase search) is conservative against the full standard method
   by 0.0–0.15 dB on the reference link (0–150 m, jitter).
4. Closed eyes: when samples lie beyond their thresholds the standard's symbol-agnostic
   histograms still give a finite TDECQ; Lightstone reports None. Only far above TDECQ limits.
"""

import math

import lightstone_ref as R
import numpy as np
import pytest
from scipy.stats import norm

import lightstone as ls

BAUD = 53.125e9


def analytic_waveform(frac, sps=16, seed=1):
    """ISI-free PAM-4 (rectangular, 4 levels) with per-symbol noise σ_s = frac·σ_ideal whose
    per-level distribution is the exact Gaussian quantile set (4 pattern periods, so the set
    reaches ±3.7 σ_s and the truncated tail biases TDECQ by < 0.01 dB)."""
    syms = np.tile(R.prbs13q(), 4)
    lv = np.array([0.2e-3, 0.6e-3, 1.0e-3, 1.4e-3])
    oma = lv[3] - lv[0]
    sid = oma / (6 * R.TDECQ_QT)
    rng = np.random.default_rng(seed)
    z = np.zeros(syms.size)
    for k in range(4):
        idx = np.nonzero(syms == k)[0]
        q = norm.ppf((np.arange(idx.size) + 0.5) / idx.size)
        rng.shuffle(q)
        z[idx] = q
    return np.repeat(lv[syms] + frac * sid * z, sps), syms, sps, oma


@pytest.mark.parametrize("frac", [0.0, 0.3, 0.5, 0.7])
def test_v19_analytic_isi_free(frac):
    w, syms, sps, oma = analytic_waveform(frac)
    exact = 10 * math.log10(1 / math.sqrt(1 - frac**2))
    ours = ls.tdecq(w, syms, sps, BAUD, delay=0, oma_outer=oma)["tdecq_db"]
    # upsample_to=sps: no interpolation of the (not band-limited) rectangular waveform.
    ref = R.tdecq(w, syms, sps, BAUD, delay=0, oma=oma, taps="tdecq", upsample_to=sps)
    print(f"\nσs/σideal {frac}: exact {exact:.4f}  Lightstone {ours:.4f}  standard {ref['tdecq_db']:.4f} dB")
    assert ours == pytest.approx(exact, abs=0.02)
    assert ref["tdecq_db"] == pytest.approx(exact, abs=0.02)


CASES = {
    "0 m": dict(fiber__length=0.0),
    "100 m": dict(fiber__length=100.0),
    "150 m": dict(fiber__length=150.0),
    "100 m, RJ 0.01 UI": dict(fiber__length=100.0, tx__jitter__rj_rms=0.01 / BAUD),
}


@pytest.fixture(scope="module")
def links():
    out = {}
    for name, p in CASES.items():
        r = ls.Simulator().set(tx__sps=32, **p).run()
        ours = r.tdecq()
        ref = R.TdecqReference(r.reference_optical, r.symbols, r.sps, r.baud)
        std = ref.run(taps="tdecq", centre="shift")
        out[name] = (r, ours, ref, std)
    return out


def test_v19_same_mechanics(links):
    """Oracle evaluated at Lightstone's taps/phase with per-symbol histograms = Lightstone."""
    for name, (r, ours, ref, _) in links.items():
        assert ref.delay == ours["delay"], name
        assert ref.oma == pytest.approx(ours["oma_outer"], rel=1e-9), name
        c = np.array(ours["taps"])
        assert 10 * math.log10(ref.ceq(c)) == pytest.approx(ours["ceq_db"], abs=1e-6), name
        same = ref.tdecq_db(c, ours["phase"], symbolwise=True)
        print(f"\n{name:18s} Lightstone {ours['tdecq_db']:.4f}  oracle, same taps/phase {same:.4f} dB")
        assert same == pytest.approx(ours["tdecq_db"], abs=0.02), name


def test_v19_against_standard_method(links):
    print("\nV19  case                Lightstone  standard  Δ [dB]   taps (standard)")
    for name, (_, ours, _, std) in links.items():
        d = ours["tdecq_db"] - std["tdecq_db"]
        print(f"     {name:18s}  {ours['tdecq_db']:9.3f}  {std['tdecq_db']:8.3f}  {d:+.3f}   {np.round(std['taps'], 3)}")
        assert -0.02 <= d <= 0.15, name


def test_v19_closed_eye_behaviour():
    """Strong RIN puts samples beyond their thresholds: Lightstone reports a closed eye, the
    standard's symbol-agnostic histograms a finite (large) TDECQ."""
    r = ls.Simulator().set(tx__sps=32, fiber__length=100.0, vcsel__rin_db_hz=-135.0).run()
    assert r.tdecq()["tdecq_db"] is None
    std = R.tdecq(r.reference_optical, r.symbols, r.sps, r.baud, taps="mmse", centre="crossing")
    print(f"\nRIN −135 dB/Hz, 100 m: standard method {std['tdecq_db']:.2f} dB, Lightstone: closed")
    assert 6.0 < std["tdecq_db"] < math.inf

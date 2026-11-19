"""V18: effective bandwidth versus wavelength of the OM3/OM4 presets against IEEE 802.3cm.

Reference: J. King, "Channel wavelength ranges for 400GBASE-4.2: OM3 and OM4 effective
bandwidth, modal and chromatic dispersion included", 802.3cm ad hoc, 28 June 2018
(king_3cm_adhoc_01_062818), table on slide 5. It combines the TIA worst-case EMB(λ) with the
chromatic-dispersion bandwidth of the 10G link model for a 0.6 nm RMS source:
BW_eff = (EMB⁻² + BW_CD⁻²)^(−½), BW_CD = (1.87e5/U_w)·(D² + E²)^(−½),
D = (λ/4)·S₀·(1 − (U₀/λ)⁴), E = 0.7·S₀·U_w, U₀ = 1316 nm, S₀ = 0.10275 ps/(nm²·km).

Lightstone side: the preset's EMB at each wavelength (modes solved at λ, default VCSEL launch),
combined with the same BW_CD, so only the modal part is compared.

Result (a documented gap, docs/validation.md, Section 5): at 850 nm the presets match (they are
calibrated there). Towards longer wavelengths they are optimistic: OM4 BW_eff +24 % at 918 nm,
OM3 +9 %. Each preset is one fiber with an ideal power-law profile at the 850 nm minimum; the
TIA worst case is an envelope over all fibers that pass at 850 nm (DMD masks) and falls faster.
The planned fix is measured/worst-case DMD fiber import. This test pins the gap so changes to it
are noticed.
"""

import math

import pytest

import lightstone as ls

# λ [nm]: (OM3, OM4) BW_eff [MHz·km], king_3cm_adhoc_01_062818 slide 5.
TABLE = {
    844: (1591, 2398),
    850: (1665, 2531),
    863: (1553, 2399),
    874: (1460, 2246),
    888: (1351, 2044),
    900: (1267, 1887),
    916: (1169, 1704),
    918: (1159, 1683),
}


def bw_cd(l_nm: float) -> float:
    """Chromatic-dispersion bandwidth of the 10G link model [MHz·km] (0.6 nm RMS source)."""
    u0, s0, uw = 1316.0, 0.10275, 0.6
    d = (l_nm / 4) * s0 * (1 - (u0 / l_nm) ** 4)
    e = 0.7 * s0 * uw
    return 1.87e5 / uw / math.sqrt(d * d + e * e)


def emb(fiber_type: str, l_nm: float) -> float:
    """Lightstone EMB [MHz·km] of a preset at wavelength l_nm (default launch); inf if the modal
    response does not reach −3 dB within the search range (EMB far above 10 000 MHz·km)."""
    f = {f"fiber.{k}": v for k, v in ls.fiber_presets()["types"][fiber_type].items()}
    r = ls.simulate({**f, "vcsel.wavelength": l_nm * 1e-9, "tx.sps": 4})
    e = r.metrics["emb_length"]
    return math.inf if e is None else e / 1e9


@pytest.fixture(scope="module")
def rows():
    out = []
    for l, (t3, t4) in TABLE.items():
        e3, e4 = emb("om3", l), emb("om4", l)
        eff = lambda e: (e**-2 + bw_cd(l) ** -2) ** -0.5  # noqa: E731
        out.append((l, t3, eff(e3), t4, eff(e4)))
    return out


def test_v18_bw_cd_formula_reproduces_table_at_850():
    """The table's own construction: TIA EMB 2000/4700 MHz·km at 850 nm with BW_CD."""
    for e, t in ((2000.0, 1665), (4700.0, 2531)):
        assert (e**-2 + bw_cd(850) ** -2) ** -0.5 == pytest.approx(t, rel=0.002)


def test_v18_presets_against_802_3cm(rows):
    print("\nV18  λ [nm]   OM3 table / Lightstone      OM4 table / Lightstone   [MHz·km]")
    for l, t3, m3, t4, m4 in rows:
        print(f"     {l:4d}     {t3:5d} / {m3:5.0f} ({m3 / t3 - 1:+.1%})    {t4:5d} / {m4:5.0f} ({m4 / t4 - 1:+.1%})")
    by = {r[0]: r for r in rows}
    # Calibrated at 850 nm: agreement within 1 %.
    assert by[850][2] == pytest.approx(by[850][1], rel=0.01)
    assert by[850][4] == pytest.approx(by[850][3], rel=0.01)
    # Both fall with wavelength above 850 nm, like the table.
    for i in (2, 4):
        vals = [r[i] for r in rows if r[0] >= 850]
        assert all(b < a for a, b in zip(vals, vals[1:]))
    # The documented gap: optimistic at long wavelengths (pinned ranges).
    for l, t3, m3, t4, m4 in rows:
        if l >= 874:
            assert m3 > t3 and m4 > t4, l
    assert 0.06 < by[918][2] / by[918][1] - 1 < 0.12
    assert 0.20 < by[918][4] / by[918][3] - 1 < 0.28


def test_v18_om5_meets_953nm_minimum():
    """OM5 at 953 nm: TIA-492AAAE minimum EMB 2470 MHz·km. The preset meets it by far (one fiber
    under-compensated at 850 nm, optimum near 950 nm), so it is optimistic there in the same way."""
    e = emb("om5", 953)
    print(f"\nOM5 EMB at 953 nm: {e:.0f} MHz·km (minimum 2470)")
    assert e > 2470

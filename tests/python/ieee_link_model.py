"""Re-implementation of the ISI part of the IEEE 802.3ae 10G Ethernet link model (V14a).

The equations follow the link model spreadsheet 10GEPBud3_1_16a.xls (Hanson, Cunningham,
Dawe, Dolfi; https://www.ieee802.org/3/ae/public/adhoc/serial_pmd/documents/) and are
checked against its computed cells in test_v14_ieee_link_model.py. Units follow the sheet:
ps, nm, km, MHz.

Each element is a Gaussian with a 10–90 % rise time T = C/B (C = 0.48 for an optical
−3 dB bandwidth B, the Gaussian relation 2·1.2816·√(2 ln 2)/(2π) ≈ 0.480); rise times add in
quadrature. The ISI penalty P_isi (synonymous with the vertical eye closure penalty VECP,
sheet notes) is the closure of the worst-case NRZ eye at its centre for a Gaussian impulse
response, where an isolated one peaks at erf(T_b/(2√2 σ)) and the eye is
2·erf(T_b/(2√2 σ)) − 1 of the full swing, with σ = T_c/2.563 and T_b reduced by the
duty-cycle distortion.
"""

import math

C_GAUSS = 480.0  # [ns·MHz] 10–90 % rise time × optical −3 dB bandwidth (the sheet's C1)
C_CD = 0.187  # chromatic bandwidth: B_cd = 0.187 / (|D·L|·Δλ_rms)
RISE_PER_SIGMA = 2.563  # 10–90 % rise time of a Gaussian step = 2.563 σ


def dispersion(wavelength_nm: float, lambda0_nm: float, s0: float) -> float:
    """Chromatic dispersion D(λ) = S₀/4 · (λ − λ₀⁴/λ³) [ps/(nm·km)]."""
    return s0 / 4.0 * (wavelength_nm - lambda0_nm**4 / wavelength_nm**3)


def chromatic_bandwidth(d_ps_nm_km: float, length_km: float, width_rms_nm: float) -> float:
    """B_cd [MHz] = 0.187 / (|D·L|·Δλ_rms) (∞ without dispersion)."""
    x = abs(d_ps_nm_km * length_km) * width_rms_nm  # [ps]
    return math.inf if x == 0 else C_CD / (x * 1e-12) / 1e6


def rise_time(bandwidth_mhz: float, c_ns_mhz: float = C_GAUSS) -> float:
    """10–90 % rise time [ps] of a Gaussian element with bandwidth `bandwidth_mhz`."""
    return 0.0 if math.isinf(bandwidth_mhz) else c_ns_mhz * 1e3 / bandwidth_mhz


def tc(ts_ps: float, bwm_mhz_km: float, length_km: float, d_ps_nm_km: float, width_rms_nm: float,
       t_rx_ps: float, c1: float = C_GAUSS) -> tuple[float, float]:
    """(T_e, T_c) [ps]: transmitter + fiber, and including the receiver."""
    eff_bwm = bwm_mhz_km / length_km if length_km > 0 else math.inf
    bw_cd = chromatic_bandwidth(d_ps_nm_km, length_km, width_rms_nm)
    te = math.sqrt(ts_ps**2 + rise_time(eff_bwm, c1) ** 2 + rise_time(bw_cd, c1) ** 2)
    return te, math.sqrt(te**2 + t_rx_ps**2)


def eye_opening(tc_ps: float, baud_mbd: float, dcd_ps: float = 0.0) -> float:
    """Worst-case eye opening at the eye centre as a fraction of the full swing."""
    tb = 1e6 / baud_mbd  # [ps]
    sigma = tc_ps / RISE_PER_SIGMA
    return 2.0 * math.erf((tb - dcd_ps) / (2.0 * math.sqrt(2.0) * sigma)) - 1.0


def p_isi(tc_ps: float, baud_mbd: float, dcd_ps: float = 0.0) -> float:
    """ISI penalty (VECP) [dB] = −10·log₁₀(eye opening)."""
    return -10.0 * math.log10(eye_opening(tc_ps, baud_mbd, dcd_ps))


def sheet_row(inp: dict, length_km: float) -> dict:
    """The sheet's length-dependent columns from its inputs (keys as in the extracted JSON)."""
    d1 = inp["d1_ps_nm_km"]
    te, t_c = tc(inp["ts_10_90_ps"], inp["bwm_mhz_km"], length_km, d1, inp["spectral_width_rms_nm"],
                 rise_time(inp["rx_bw_mhz"], inp["c_rx_ns_mhz"]), inp["c1_ns_mhz"])
    return {
        "d1l_ps_nm": d1 * length_km,
        "bw_cd_mhz": chromatic_bandwidth(d1, length_km, inp["spectral_width_rms_nm"]),
        "eff_bwm_mhz": inp["bwm_mhz_km"] / length_km,
        "te_ps": te,
        "tc_ps": t_c,
        "p_isi_db": p_isi(t_c, inp["baud_mbd"], inp["dcd_ps"]),
    }


# ---- 802.3db (100 Gb/s per lane PAM-4 over MMF): fiber emulation for TDECQ (V14b) ----------
#
# 802.3db specifies TDECQ with a fiber emulation filter whose −3 dB bandwidth f_A represents
# the worst-case OM4 channel (Table 167-12: 18.0 GHz for 100 m, 33.6 GHz for 50 m, as quoted in
# murty_3db_01a_092321). The baseline proposal (murty_3db_adhoc_01a_121720) derives it from the
# 10GE link model method (king_3cm_adhoc_01_062818): the fiber is a Gaussian low pass whose
# optical bandwidth·length is BW_eff = (EMB⁻² + BW_cd⁻²)^(−½), with
# BW_cd = 1.87·10⁵/Δλ_rms / √(D² + E²) [MHz·km], D = λ/4·S₀·(1 − (U₀/λ)⁴), E = 0.7·S₀·Δλ_rms,
# EMB = 4700 MHz·km, U₀ = 1328 nm, S₀ = 0.093477 ps/(nm²·km) (abbott_3db_adhoc_01_080620).
# The same document gives the combined fiber + 26.5625 GHz Bessel–Thomson reference response
# (21.0 GHz at 50 m, 15.0 GHz at 100 m).

U0_DB = 1328.0  # [nm]
S0_DB = 0.093477  # [ps/(nm²·km)]
EMB_OM4 = 4700.0  # [MHz·km]


def mmf_dispersion(wavelength_nm: float, u0: float = U0_DB, s0: float = S0_DB) -> float:
    """MMF chromatic dispersion D [ps/(nm·km)] as used by 802.3."""
    return wavelength_nm / 4.0 * s0 * (1.0 - (u0 / wavelength_nm) ** 4)


def effective_bandwidth(emb: float, d: float, width_nm: float, s0: float = S0_DB) -> float:
    """BW_eff [MHz·km]: EMB and chromatic bandwidth combined (Gaussian, optical −3 dB)."""
    e = 0.7 * s0 * width_nm
    bw_cd = 1.87e5 / width_nm / math.sqrt(d * d + e * e)
    return (emb**-2 + bw_cd**-2) ** -0.5


def fiber_emulation_bandwidth(length_m: float, emb: float, d: float, width_nm: float) -> float:
    """Electrical −3 dB bandwidth (|H|² = ½) [Hz] of the Gaussian fiber of `length_m`.

    A Gaussian with optical bandwidth B_o (|H| = ½) has its electrical −3 dB point at B_o/√2.
    """
    b_o = effective_bandwidth(emb, d, width_nm) * 1e6 / (length_m / 1e3)
    return b_o / math.sqrt(2.0)


def gaussian_response(f, f3e: float):
    """Gaussian fiber emulation filter with |H|² = ½ at f3e (zero phase)."""
    import numpy as np

    return np.exp(-math.log(2.0) / 2.0 * (np.asarray(f) / f3e) ** 2)


def bessel_thomson4(f, f3e: float):
    """4th-order Bessel–Thomson response with |H|² = ½ at f3e."""
    import numpy as np

    w3 = _bt4_w3()
    s = 1j * w3 * np.asarray(f) / f3e
    return 105.0 / (s**4 + 10 * s**3 + 45 * s**2 + 105 * s + 105)


def _bt4_w3() -> float:
    lo, hi = 0.1, 10.0
    h = lambda w: abs(105.0 / ((1j * w) ** 4 + 10 * (1j * w) ** 3 + 45 * (1j * w) ** 2 + 105j * w + 105)) ** 2 - 0.5
    for _ in range(100):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if h(mid) > 0 else (lo, mid)
    return 0.5 * (lo + hi)


def f3e_of(h, fmax: float = 200e9) -> float:
    """Electrical −3 dB frequency of a response function h(f) (bisection)."""
    lo, hi = 1e6, fmax
    for _ in range(100):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if abs(h(mid)) ** 2 > 0.5 else (lo, mid)
    return 0.5 * (lo + hi)

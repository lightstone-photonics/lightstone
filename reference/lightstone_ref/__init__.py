"""NumPy/SciPy reference implementation of the deterministic Lightstone signal chain.

Independent code (different language, FFT library and eigen solver) implementing the same
discretizations as the Rust core. Used as a test oracle (validation test V15) and as a
readable specification. Noise generation is not mirrored; comparisons use noiseless paths.

Equation IDs refer to docs/model/*.md.
"""

from __future__ import annotations

import math

import numpy as np
from scipy.linalg import eigh_tridiagonal
from scipy.optimize import brentq

C0 = 299_792_458.0
Q_E = 1.602_176_634e-19
D_LAMBDA = 4e-9

# ---------------------------------------------------------------------------------------------
# Patterns and transmitter
# ---------------------------------------------------------------------------------------------


def prbs13(n: int, seed: int = 0x0AAB) -> np.ndarray:
    """PRBS13, polynomial 1 + x + x^2 + x^12 + x^13 (Fibonacci LFSR)."""
    s = seed & 0x1FFF
    out = np.empty(n, dtype=np.uint8)
    for i in range(n):
        b = ((s >> 12) ^ (s >> 11) ^ (s >> 1) ^ s) & 1
        out[i] = (s >> 12) & 1
        s = ((s << 1) | b) & 0x1FFF
    return out


GRAY = {(0, 0): 0, (0, 1): 1, (1, 1): 2, (1, 0): 3}


def prbs13q(seed: int = 0x0AAB) -> np.ndarray:
    bits = prbs13(2 * 8191, seed).reshape(-1, 2)
    return np.array([GRAY[(int(a), int(b))] for a, b in bits], dtype=np.uint8)


def power_levels(p_avg: float, er_lin: float) -> np.ndarray:
    """eq:tx-levels"""
    p0 = 2 * p_avg / (er_lin + 1)
    p3 = 2 * p_avg * er_lin / (er_lin + 1)
    return p0 + np.arange(4) * (p3 - p0) / 3


def freqs(n: int, fs: float) -> np.ndarray:
    """Signed FFT frequencies; Nyquist FFT bin of even n mapped to +fs/2 (as in Rust)."""
    k = np.arange(n)
    kk = np.where(k <= n // 2, k, k - n)
    return kk * fs / n


def vcsel_response(f, fr, zeta):
    """eq:vcsel-response"""
    x = f / fr
    return 1.0 / (1 - x * x + 2j * zeta * x)


def bessel4(s):
    return 105.0 / (s**4 + 10 * s**3 + 45 * s**2 + 105 * s + 105)


def _bessel4_w3db() -> float:
    lo, hi = 1.0, 4.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if abs(bessel4(1j * mid)) ** 2 > 0.5:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


W3DB = _bessel4_w3db()


def bt4_response(f, f3db):
    """eq:rx-bessel-thomson"""
    return bessel4(1j * W3DB * f / f3db)


def dbm_to_w(x):
    return 1e-3 * 10 ** (x / 10)


def nrz_spectrum(syms, lv, sps):
    """eq:tx-nrz — exact spectrum of the continuous NRZ drive on the FFT grid:
    X(f) = S(f)·sps·sinc(fT)·exp(−jπfT), S = DFT of the symbol levels (periodic)."""
    ns = syms.size
    n = ns * sps
    s = np.fft.fft(lv[syms])
    ft = freqs(n, n) / ns  # f·T
    return s[np.arange(n) % ns] * sps * np.sinc(ft) * np.exp(-1j * np.pi * ft)


def transmit(cfg: dict):
    """Noiseless transmitter waveform (RIN excluded)."""
    tx, vc = cfg["tx"], cfg["vcsel"]
    syms = np.tile(prbs13q(tx["prbs_seed"]), max(1, tx["pattern_periods"]))
    lv = power_levels(dbm_to_w(tx["p_avg_dbm"]), 10 ** (tx["er_db"] / 10))
    sps = tx["sps"]
    fs = tx["baud"] * sps
    x = nrz_spectrum(syms, lv, sps)
    h = vcsel_response(freqs(x.size, fs), vc["resonance_freq"], vc["damping"])
    p = np.fft.ifft(x * h).real
    return syms, lv, fs, np.maximum(p, 0.0)


# ---------------------------------------------------------------------------------------------
# Material and modes
# ---------------------------------------------------------------------------------------------

_B = (0.6961663, 0.4079426, 0.8974794)
_C = (0.0684043, 0.1162414, 9.896161)


_B_GE = (0.80686642, 0.71815848, 0.85416831)
_C_GE = (0.068972606, 0.15396605, 11.841931)
NA_REFERENCE_WAVELENGTH = 850e-9


def germania_silica_index(lam, x):
    """eq:material-germania (Fleming 1984): Sellmeier coefficients interpolated in x."""
    l2 = (lam * 1e6) ** 2
    s = 1.0
    for b, c, bg, cg in zip(_B, _C, _B_GE, _C_GE):
        bb = b + x * (bg - b)
        cc = c + x * (cg - c)
        s += bb * l2 / (l2 - cc * cc)
    return math.sqrt(s)


def silica_index(lam):
    """eq:material-sellmeier (Malitson 1965)"""
    return germania_silica_index(lam, 0.0)


def germania_fraction_for_na(na):
    """GeO2 mole fraction with sqrt(n_core^2 - n_clad^2) = NA at 850 nm (independent: brentq)."""
    lam = NA_REFERENCE_WAVELENGTH
    n2 = silica_index(lam)
    return brentq(lambda x: math.sqrt(max(germania_silica_index(lam, x) ** 2 - n2 * n2, 0.0)) - na, 0.0, 1.0, xtol=1e-15)


def _profile_g(profile: dict, x: np.ndarray) -> np.ndarray:
    if profile["kind"] == "step":
        g = np.ones_like(x)
    else:
        g = 1 - x ** profile["alpha"]
    return np.where(x >= 1, 0.0, g)


def _operator(profile, v2, l, h, n):
    """eq:modes-radial — cell-centered, flux-conservative, √x-symmetrized."""
    i = np.arange(n)
    xi = (i + 0.5) * h
    xm = i * h
    xp = (i + 1.0) * h
    d = -(xp + xm) / (xi * h * h) + v2 * _profile_g(profile, xi) - l * l / xi**2
    e = xp[:-1] / (h * h * np.sqrt(xi[:-1] * xi[1:]))
    return d, e


def _indices(fiber, lam_ref, lam):
    if fiber.get("core_material", "germania_doped") == "germania_doped":
        x = germania_fraction_for_na(fiber["na"])
        return germania_silica_index(lam, x), silica_index(lam)
    n2r = silica_index(lam_ref)
    n1r = math.sqrt(n2r**2 + fiber["na"] ** 2)
    delta = fiber["na"] ** 2 / (2 * n1r**2)
    n2 = silica_index(lam)
    return n2 / math.sqrt(1 - 2 * delta), n2


def solve_modes(fiber: dict, wavelength: float):
    """Guided LP modes with group delay and chromatic slope."""
    a = fiber["core_radius"]
    ppr = fiber["grid_points_per_radius"]
    h = 1.0 / ppr
    n = int(round(fiber["domain_factor"] * ppr))
    lams = [wavelength + j * D_LAMBDA for j in (-2, -1, 0, 1, 2)]
    geo = []
    for lam in lams:
        n1, n2 = _indices(fiber, wavelength, lam)
        k = 2 * math.pi / lam
        geo.append((n1, n2, (k * a) ** 2 * (n1 * n1 - n2 * n2), k))
    prof = fiber["profile"]
    modes = []
    l = 0
    while True:
        evs = []
        vecs = None
        for j, (_, _, v2, _) in enumerate(geo):
            d, e = _operator(prof, v2, l, h, n)
            if j == 2:
                w, v = eigh_tridiagonal(d, e, select="v", select_range=(0.0, np.inf))
                vecs = v[:, ::-1]
            else:
                w = eigh_tridiagonal(d, e, eigvals_only=True, select="v", select_range=(0.0, np.inf))
            evs.append(w[::-1])
        if evs[2].size == 0:
            break
        count = min(e.size for e in evs)
        for mi in range(count):
            ne = np.array([math.sqrt(geo[j][1] ** 2 + evs[j][mi] / (geo[j][3] * a) ** 2) for j in range(5)])
            dn = (ne[0] - 8 * ne[1] + 8 * ne[3] - ne[4]) / (12 * D_LAMBDA)
            d2n = (-ne[0] + 16 * ne[1] - 30 * ne[2] + 16 * ne[3] - ne[4]) / (12 * D_LAMBDA**2)
            g = vecs[:, mi]
            ang = 2 * math.pi if l == 0 else math.pi
            field = g / np.sqrt((np.arange(n) + 0.5) * h) / (math.sqrt(ang * h) * a)
            fmax = np.max(np.abs(field))
            first = field[np.argmax(np.abs(field) > 1e-3 * fmax)]
            if first < 0:
                field = -field
            modes.append(
                dict(
                    l=l,
                    m=mi + 1,
                    group=2 * (mi + 1) + l - 1,
                    n_eff=ne[2],
                    group_delay=(ne[2] - wavelength * dn) / C0,
                    chromatic_slope=-wavelength * d2n / C0,
                    field=field,
                )
            )
        l += 1
    modes.sort(key=lambda m: -m["n_eff"])
    r = (np.arange(n) + 0.5) * h * a
    return dict(modes=modes, r=r, dr=h * a, wavelength=wavelength, n_core=geo[2][0], n_clad=geo[2][1])


# ---------------------------------------------------------------------------------------------
# Coupling
# ---------------------------------------------------------------------------------------------


def _laguerre(p, l, x):
    if p == 0:
        return np.ones_like(x)
    a, b = np.ones_like(x), 1 + l - x
    for k in range(1, p):
        a, b = b, ((2 * k + 1 + l - x) * b - (k + l) * a) / (k + 1)
    return b


def lg_modes(groups: int):
    out = []
    for q in range(max(1, groups)):
        for l in range(q, -1, -1):
            if (q - l) % 2:
                continue
            p = (q - l) // 2
            out.append((p, l, False))
            if l > 0:
                out.append((p, l, True))
    return out


def lg_field(p, l, sin, w, rho, theta):
    """eq:launch-lg"""
    c = math.sqrt(2 * math.factorial(p) / (math.pi * math.factorial(p + l))) / w
    if l > 0:
        c *= math.sqrt(2)
    s = math.sqrt(2) * rho / w
    ang = 1.0 if l == 0 else (np.sin(l * theta) if sin else np.cos(l * theta))
    return c * s**l * _laguerre(p, l, s * s) * np.exp(-(rho**2) / w**2) * ang


def couple_launch(ms: dict, launch: dict, angular_points: int = 256) -> dict:
    """eq:coupling-overlap, eq:launch-incoherent — polar quadrature with azimuthal FFT."""
    r, dr = ms["r"], ms["dr"]
    m_ang = angular_points
    phi = np.arange(m_ang) * 2 * math.pi / m_ang
    dphi = 2 * math.pi / m_ang
    R, PHI = np.meshgrid(r, phi, indexing="ij")
    x, y = R * np.cos(PHI), R * np.sin(PHI)
    bx, by = x - launch["offset"], y
    rho, th = np.hypot(bx, by), np.arctan2(by, bx)
    kx = 2 * math.pi / ms["wavelength"] * math.sin(launch["tilt"])
    tilt = np.exp(1j * kx * x)
    lgs = lg_modes(launch["vcsel_mode_groups"])
    powers = np.zeros(len(ms["modes"]))
    guided = 0.0
    for p, l, s in lgs:
        E = tilt * lg_field(p, l, s, launch["beam_radius"], rho, th)
        p_disc = np.sum(np.sum(np.abs(E) ** 2, axis=1) * dphi * r * dr)
        A = np.fft.fft(E, axis=1) * dphi  # A[:, l] = ∫E e^{-jlφ}
        frac = min(p_disc / 1.0, 1.0)
        scale = math.sqrt(frac / p_disc)
        for i, md in enumerate(ms["modes"]):
            L = md["l"]
            wgt = md["field"] * r * dr
            if L == 0:
                cc = np.sum(A[:, 0] * wgt)
                cs = 0.0
            else:
                ap, an = A[:, L], A[:, (m_ang - L) % m_ang]
                cc = np.sum(0.5 * (ap + an) * wgt)
                cs = np.sum(0.5j * (ap - an) * wgt)
            pw = (abs(cc) ** 2 + abs(cs) ** 2) * scale**2
            powers[i] += pw / len(lgs)
            guided += pw / len(lgs)
    return dict(power=powers, guided_fraction=guided)


# ---------------------------------------------------------------------------------------------
# Fiber channel and receiver (noiseless)
# ---------------------------------------------------------------------------------------------


def channel_response(ms, launch_power, fiber, spectral_width_rms, f, detector_radius=None):
    """eq:fiber-power-propagation"""
    L = fiber["length"]
    t_loss = 10 ** (-fiber["attenuation_db_per_km"] * L / 1e4) * 10 ** (-fiber["extra_loss_db"] / 10)
    if detector_radius is not None:
        raise NotImplementedError("reference covers full-area detection only")
    w = np.asarray(launch_power) * t_loss
    tau = np.array([m["group_delay"] for m in ms["modes"]])
    slope = np.array([m["chromatic_slope"] for m in ms["modes"]])
    mean_tau = np.sum(w * tau) / np.sum(w)
    keep = w > 1e-12 * np.sum(w)
    H = np.zeros(f.shape, dtype=complex)
    for wi, ti, si in zip(w[keep], tau[keep], slope[keep]):
        sig = abs(si) * L * spectral_width_rms
        H += wi * np.exp(-2 * math.pi**2 * f**2 * sig**2) * np.exp(-2j * math.pi * f * (ti - mean_tau) * L)
    return H


def simulate_noiseless(cfg: dict) -> dict:
    """Noiseless link: TX power, detected power, filtered RX output, reference optical, σ."""
    syms, lv, fs, p_tx = transmit(cfg)
    ms = solve_modes(cfg["fiber"], cfg["vcsel"]["wavelength"])
    cpl = couple_launch(ms, cfg["launch"])
    f = freqs(p_tx.size, fs)
    H = channel_response(ms, cpl["power"], cfg["fiber"], cfg["vcsel"]["spectral_width_rms"], f, cfg["rx"]["detector_radius"])
    P = np.fft.fft(p_tx)
    p_det = np.fft.ifft(P * H).real
    rx = cfg["rx"]
    hrx = bt4_response(f, rx["bt_f3db_rel"] * cfg["tx"]["baud"]) if rx["filter_enabled"] else np.ones_like(f)
    p_f = np.fft.ifft(P * H * hrx).real
    bn = 0.5 * fs * np.mean(np.abs(hrx) ** 2)
    var = (rx["noise_density"] ** 2 if rx["thermal_noise"] else 0.0) * bn
    # eq:rx-noise — exact variance of filtered white noise with time-varying intensity:
    # var(t) = (fs/2) · (g² ∗ S)(t), g = discrete impulse response.
    def conv(a, g):
        return np.fft.ifft(np.fft.fft(a) * np.fft.fft(g * g)).real

    if rx["shot_noise"]:
        g_rx = np.fft.ifft(hrx).real
        var = var + 2 * Q_E * rx["responsivity"] * 0.5 * fs * conv(p_det, g_rx)
    if rx["rin_noise"]:
        g_tot = np.fft.ifft(H * hrx).real
        rin = 10 ** (cfg["vcsel"]["rin_db_hz"] / 10)
        var = var + rin * rx["responsivity"] ** 2 * 0.5 * fs * conv(p_tx**2, g_tot)
    var = np.maximum(var, 0.0)
    z = rx["transimpedance"]
    return dict(
        symbols=syms,
        tx_power=p_tx,
        rx_optical=p_det,
        rx_noiseless=z * rx["responsivity"] * p_f,
        reference_optical=p_f,
        rx_sigma=z * np.sqrt(var),
        modes=ms,
        launch_power=cpl["power"],
        guided_fraction=cpl["guided_fraction"],
    )


# ---------------------------------------------------------------------------------------------
# TDECQ — independent implementation of the IEEE 802.3 method (validation test V19)
# ---------------------------------------------------------------------------------------------
#
# Written from the text of IEEE P802.3bs D3.1 121.8.4/121.8.5.3 (king_03_0217_smf), the
# T-spaced centre-shift change (king_3bs_01a_0917) and the Ceq/K relation (dawe_3cd_01b_0718),
# not from the Rust code. Differences to the Rust implementation are deliberate options so
# each one can be measured:
#   * symbol-agnostic histograms (the standard) — Rust: per-symbol histograms;
#   * eye centre from the mean P_ave crossing time of the equalized eye (+ optional shift of the
#     0.1 UI histogram pair) — Rust: phase search around the MMSE optimum;
#   * taps optimized for the added noise (Nelder–Mead) — Rust: MMSE taps;
#   * OMAouter over the central 2 UI of a run of 7 threes / 6 zeros — Rust: mid-symbol samples
#     of settled runs;
#   * band-limited (FFT) interpolation, Ceq from the noise autocorrelation at symbol lags.

from scipy.optimize import minimize  # noqa: E402
from scipy.special import erfc  # noqa: E402

TDECQ_SER = 4.8e-4


def q_function(x):
    return 0.5 * erfc(np.asarray(x) / math.sqrt(2.0))


TDECQ_QT = brentq(lambda q: 1.5 * float(q_function(q)) - TDECQ_SER, 2.0, 5.0, xtol=1e-14)


def align_delay(w, syms, sps):
    """Integer sample delay d with w[i] ≈ template[i − d] (circular cross-correlation)."""
    x = np.repeat(syms.astype(float) - 1.5, sps)
    c = np.fft.ifft(np.conj(np.fft.fft(x)) * np.fft.fft(w - w.mean())).real
    return int(np.argmax(c))


def oma_outer(w, syms, sps, delay):
    """121.8.4: P3 = mean over the central 2 UI of a run of 7 threes, P0 the same for a run of
    6 zeros (runs exactly that long in the periodic pattern; all such runs are averaged)."""
    n = syms.size
    t = (np.arange(w.size) - delay) % w.size / sps  # time in UI from symbol 0

    def level(k, run):
        acc = []
        for s in range(n):
            if syms[s] != k or syms[(s - 1) % n] == k:
                continue
            ln = 0
            while syms[(s + ln) % n] == k and ln <= run:
                ln += 1
            if ln != run:
                continue
            a = (s + (run - 2) / 2) % n
            m = ((t - a) % n) < 2.0
            acc.append(w[m].mean())
        if not acc:
            raise ValueError(f"pattern has no run of exactly {run} symbols of level {k}")
        return float(np.mean(acc))

    return level(3, 7) - level(0, 6)


def noise_autocorrelation(baud, f3db_rel, lags=5, sps=256, n_ui=4096):
    """R(m)/R(0) at symbol lags m of white noise filtered by the BT4 reference receiver, from the
    sampled impulse response (time domain, span n_ui symbols)."""
    n = sps * n_ui
    f = freqs(n, baud * sps)
    h = np.fft.ifft(bt4_response(f, f3db_rel * baud)).real
    r = np.array([np.dot(h, np.roll(h, -m * sps)) for m in range(lags)])
    return r / r[0]


def ceq_from_autocorrelation(taps, rho):
    """Noise enhancement (RMS ratio) of a T-spaced FFE for noise with normalized
    autocorrelation rho at symbol lags."""
    c = np.asarray(taps, float)
    m = np.abs(np.arange(c.size)[:, None] - np.arange(c.size)[None, :])
    return math.sqrt(float(c @ rho[m] @ c))


class TdecqReference:
    """TDECQ of a reference-receiver-filtered waveform (len = symbols × sps), IEEE method.

    `centre`: "crossing" (standard D3.1) or "shift" (D3.4+: the 0.1 UI histogram pair may move).
    `taps`: "tdecq" (optimize taps for minimum TDECQ), "ser" (D3.1 literal: taps minimize the
    worst SER for noise added after the equalizer, i.e. maximize σ_G), "mmse", or 5 fixed taps.
    `histograms`: "agnostic" (standard) or "symbol" (per-level, as in the Rust code).
    """

    def __init__(self, w, syms, sps, baud, *, delay=None, oma=None, f3db_rel=0.5, upsample_to=1024, bins=2000):
        self.w = np.asarray(w, float)
        self.syms = np.asarray(syms, np.uint8)
        self.sps, self.baud, self.bins = sps, baud, bins
        self.n = self.syms.size
        self.delay = align_delay(self.w, self.syms, sps) if delay is None else int(delay)
        self.p_ave = float(self.w.mean())
        self.oma = oma_outer(self.w, self.syms, sps, self.delay) if oma is None else float(oma)
        self.th = self.p_ave + self.oma / 3 * np.array([-1.0, 0.0, 1.0])
        self.rho = noise_autocorrelation(baud, f3db_rel)
        # Band-limited upsampling (zero-padded FFT), aligned so index k·P + q is symbol k at q/P.
        u = max(1, -(-upsample_to // sps))
        nn = self.w.size
        if u == 1:
            self.up = np.roll(self.w, -self.delay)
        else:
            spec = np.fft.fft(np.roll(self.w, -self.delay))
            pad = np.zeros(nn * u, complex)
            h = nn // 2
            pad[:h] = spec[:h]
            pad[-h:] = spec[-h:]
            if nn % 2 == 0:  # split the Nyquist bin
                pad[h] = pad[-h] = 0.5 * spec[h]
            else:
                pad[h] = spec[h]
            self.up = np.fft.ifft(pad).real * u
        self.P = sps * u
        self.native = np.roll(self.w, -self.delay).reshape(self.n, sps)
        self._cols = {}

    # -- sampling ---------------------------------------------------------------------------

    def _col(self, q):
        """Unequalized symbol-rate samples at phase q/P (q may be outside 0..P−1)."""
        if q not in self._cols:
            idx = (np.arange(self.n) * self.P + q) % self.up.size
            self._cols[q] = self.up[idx]
        return self._cols[q]

    @staticmethod
    def _eq(x, c):
        """T-spaced FFE along axis 0: y_k = Σ_j c_j x_{k+2−j}."""
        return sum(cj * np.roll(x, j - 2, axis=0) for j, cj in enumerate(c))

    def crossing_phase(self, c):
        """Mean P_ave crossing time of the equalized eye [UI, circular mean]."""
        e = self._eq(self.native, c).ravel()
        d = e - self.p_ave
        nxt = np.roll(d, -1)
        i = np.nonzero((d < 0) != (nxt < 0))[0]
        t = i + d[i] / (d[i] - nxt[i])
        ph = (t % self.sps) / self.sps
        return float(np.angle(np.mean(np.exp(2j * np.pi * ph))) / (2 * np.pi)) % 1.0

    def _window(self, c, centre):
        qs = np.arange(math.ceil((centre - 0.02) * self.P - 1e-9), math.floor((centre + 0.02) * self.P + 1e-9) + 1)
        x = np.stack([self._col(int(q)) for q in qs], axis=1)
        return self._eq(x, c)

    # -- SER and σ_G ------------------------------------------------------------------------

    def _hist(self, v, symbolwise):
        lo, hi = v.min(), v.max()
        pad = 1e-9 * max(hi - lo, abs(self.oma))
        edges = np.linspace(lo - pad, hi + pad, self.bins + 1)
        y = 0.5 * (edges[1:] + edges[:-1])
        if not symbolwise:
            f, _ = np.histogram(v.ravel(), edges)
            return ("agnostic", y, f / v.size)
        out = []
        for k in range(4):
            f, _ = np.histogram(v[self.syms == k].ravel(), edges)
            out.append(f / v.size)
        return ("symbol", y, out)

    def _ser(self, h, sigma):
        kind, y, f = h
        if kind == "agnostic":
            # 121–3…121–8: Σ_i Cf(y_i)·Gth(y_i) = Σ_y f(y)·Q(|y − Pth|/σ_G), summed over thresholds.
            return float(sum(np.dot(f, q_function(np.abs(y - t) / sigma)) for t in self.th))
        s = 0.0
        for k in range(4):
            if k > 0:
                s += np.dot(f[k], q_function((y - self.th[k - 1]) / sigma))
            if k < 3:
                s += np.dot(f[k], q_function((self.th[k] - y) / sigma))
        return float(s)

    def sigma_g(self, c, centre, symbolwise=False):
        """Largest σ_G (at the equalizer output) with max(SER_L, SER_R) = 4.8e-4; 0 if none."""
        hs = [self._hist(self._window(c, centre + d), symbolwise) for d in (-0.05, 0.05)]

        def g(ls):
            s = math.exp(ls)
            return math.log(max(max(self._ser(h, s) for h in hs), 1e-300)) - math.log(TDECQ_SER)

        lo, hi = math.log(1e-7 * self.oma), math.log(self.oma)
        if g(lo) > 0:
            return 0.0
        return math.exp(brentq(g, lo, hi, xtol=1e-12))

    # -- taps and TDECQ ---------------------------------------------------------------------

    def ceq(self, c):
        return ceq_from_autocorrelation(c, self.rho)

    def tdecq_db(self, c, centre, symbolwise=False):
        sg = self.sigma_g(c, centre, symbolwise)
        if sg <= 0:
            return math.inf
        return 10 * math.log10(self.oma * self.ceq(c) / (6 * TDECQ_QT * sg))

    def mmse_taps(self, centre):
        """Least-squares taps (Σc = 1) to the ideal levels at phase `centre`."""
        y = self._col(int(round(centre * self.P)))
        tgt = self.p_ave + self.oma * (self.syms - 1.5) / 3
        cols = [np.roll(y, j - 2) for j in range(5)]
        a = np.stack([cols[j] - cols[2] for j in (0, 1, 3, 4)], axis=1)
        x, *_ = np.linalg.lstsq(a, tgt - cols[2], rcond=None)
        return np.insert(x, 2, 1 - x.sum())

    def run(self, taps="tdecq", centre="crossing", histograms="agnostic", main_min=None):
        symbolwise = histograms == "symbol"
        ident = np.array([0, 0, 1.0, 0, 0])
        c0 = self.mmse_taps((self.crossing_phase(ident) + 0.5) % 1.0)

        def unpack(x):
            c = np.insert(x[:4], 2, 1 - np.sum(x[:4]))
            ctr = (self.crossing_phase(c) + 0.5) % 1.0
            if centre == "shift":
                ctr += x[4]
            return c, ctr

        def objective(x, what):
            c, ctr = unpack(x)
            if centre == "shift" and abs(x[4]) > 0.25:
                return 1e3
            if main_min is not None and c[2] < main_min:
                return 1e3 + (main_min - c[2])
            if what == "tdecq":
                return self.tdecq_db(c, ctr, symbolwise)
            sg = self.sigma_g(c, ctr, symbolwise)
            return -sg / self.oma

        if isinstance(taps, str) and taps in ("tdecq", "ser"):
            x0 = np.delete(c0, 2)
            if centre == "shift":
                x0 = np.append(x0, 0.0)
            best = None
            for _ in range(3):  # restarts: the histogram objective is piecewise smooth
                # Initial simplex: 0.01 per tap and 0.01 UI shift.
                simplex = np.vstack([x0, x0 + 0.01 * np.eye(x0.size)])
                r = minimize(objective, x0, args=(taps,), method="Nelder-Mead",
                             options=dict(xatol=1e-6, fatol=1e-7, maxiter=4000, initial_simplex=simplex))
                if best is not None and r.fun >= best.fun - 1e-9:
                    break
                best, x0 = r, r.x
            c, ctr = unpack(best.x)
        else:
            c = c0 if isinstance(taps, str) else np.asarray(taps, float)
            ctr = (self.crossing_phase(c) + 0.5) % 1.0
            if centre == "shift":
                r = minimize(lambda s: self.tdecq_db(c, ctr + s[0], symbolwise) if abs(s[0]) < 0.25 else 1e3,
                             [0.0], method="Nelder-Mead",
                             options=dict(xatol=1e-5, fatol=1e-7, initial_simplex=[[0.0], [0.01]]))
                ctr += float(r.x[0])
        sg = self.sigma_g(c, ctr, symbolwise)
        ceq = self.ceq(c)
        t = math.inf if sg <= 0 else 10 * math.log10(self.oma * ceq / (6 * TDECQ_QT * sg))
        return dict(tdecq_db=t, ceq_db=10 * math.log10(ceq), taps=c, centre=ctr, sigma_g=sg,
                    oma_outer=self.oma, p_ave=self.p_ave, delay=self.delay)


def tdecq(w, syms, sps, baud, **kw):
    """Convenience wrapper: TdecqReference(w, syms, sps, baud, delay=, oma=, f3db_rel=).run(...)."""
    init = {k: kw.pop(k) for k in ("delay", "oma", "f3db_rel", "upsample_to", "bins") if k in kw}
    return TdecqReference(w, syms, sps, baud, **init).run(**kw)

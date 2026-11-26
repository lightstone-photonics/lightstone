"""Measured waveforms: load a scope capture and analyze it with Lightstone (TDECQ, PAM-4/NRZ
analysis, eye). See docs/measured-data.md.

Typical use (an averaged, pattern-locked capture of one SSPRQ period)::

    import lightstone
    m = lightstone.read_waveform("capture.csv", baud=53.125e9, pattern="ssprq")
    print(m.tdecq()["tdecq_db"], m.tdecq(ffe_taps=1)["tdecq_db"])
"""

from __future__ import annotations

import csv
from pathlib import Path

import numpy as np

from . import _lightstone

#: Relative mismatch between the capture length and a whole number of pattern periods
#: below which the capture is resampled band-limited (FFT), as a periodic signal.
PERIODIC_TOL = 1e-3


def test_pattern(name: str = "prbs13q", levels: int = 4, seed: int | None = None) -> np.ndarray:
    """Symbols of a named test pattern: ``"prbs13q"`` (8191 symbols; PRBS13 for ``levels=2``)
    or ``"ssprq"`` (65 535 PAM-4 symbols; construction not yet verified against the standard,
    see docs/model/prbs-pattern.md)."""
    args = (name, int(levels)) if seed is None else (name, int(levels), int(seed))
    return np.asarray(_lightstone.test_pattern(*args))


def _bt4(f, f3db):
    """4th-order Bessel–Thomson low-pass (normalized to −3 dB at f3db), as in the engine."""
    w3 = 2.113917674904216  # −3 dB frequency of 105/(s⁴+10s³+45s²+105s+105), rad/s
    s = 1j * w3 * f / f3db
    return 105.0 / (s**4 + 10 * s**3 + 45 * s**2 + 105 * s + 105)


def _fft_resample(x: np.ndarray, n: int) -> np.ndarray:
    """Band-limited resampling of one period of a periodic signal to n points."""
    m = x.size
    X = np.fft.rfft(x)
    k = min(X.size, n // 2 + 1)
    Y = np.zeros(n // 2 + 1, complex)
    Y[:k] = X[:k]
    if m % 2 == 0 and k == X.size:  # Nyquist bin of the input: split between ±fs/2
        Y[k - 1] *= 0.5 if n > m else 1.0
    return np.fft.irfft(Y, n) * (n / m)


class MeasuredWaveform:
    """A measured waveform on Lightstone's grid: ``signal`` has ``len(symbols) * sps`` samples,
    one (or more) pattern periods. Create it with :func:`measured_waveform` or
    :func:`read_waveform`."""

    def __init__(self, signal, symbols, sps, baud, info):
        self.signal = np.asarray(signal, dtype=np.float64)
        self.symbols = np.asarray(symbols, dtype=np.uint8)
        self.sps = int(sps)
        self.baud = float(baud)
        #: How the capture was mapped onto the grid (periods, method, filter, …).
        self.info = info

    @property
    def time(self) -> np.ndarray:
        return np.arange(self.signal.size) / (self.baud * self.sps)

    def tdecq(self, **options) -> dict:
        """TDECQ (docs/model/tdecq.md). The waveform must already be seen through the reference
        receiver (scope in TDECQ mode, or ``reference_filter=True`` at import). Options as for
        :func:`lightstone.tdecq`, e.g. ``ffe_taps=1`` (no equalizer) or ``oma_outer=…``."""
        from . import tdecq

        return tdecq(self.signal, self.symbols, self.sps, self.baud, **options)

    def analyze(self, **options) -> dict:
        """PAM-4/NRZ analysis (SER from the waveform itself: counted, since the noise of a
        measured, averaged waveform is unknown; eye heights and widths)."""
        from . import analyze

        return analyze(self.signal, self.symbols, self.sps, **options)

    def eye(self, width: int = 600, height: int = 400, analysis: dict | None = None, v_range=None):
        from . import eye_density

        a = analysis or self.analyze()
        return eye_density(self.signal, self.sps, a["delay"], a["phase"], width, height, v_range)


def measured_waveform(samples, sample_rate: float, baud: float, symbols="prbs13q", *, sps: int = 16,
                      reference_filter: bool = False, f3db_rel: float = 0.5, start: float = 0.0) -> MeasuredWaveform:
    """Map a captured waveform onto Lightstone's grid.

    ``samples``: the captured values (any unit: W, V, …), sampled at ``sample_rate`` [Hz]. The
    capture should contain one or more whole periods of the pattern, as an averaged,
    pattern-locked capture of a sampling scope does. ``symbols``: the transmitted symbol
    sequence of one period (array of 0…3, or a pattern name for :func:`test_pattern`).
    ``start`` [s]: time of the first sample of the first period to use.

    If the capture length is a whole number of periods (within ``PERIODIC_TOL``), all periods
    are resampled band-limited (FFT) and averaged. Otherwise one period from ``start`` is
    linearly interpolated (and ``info["method"]`` says so).

    ``reference_filter=True`` applies the TDECQ reference receiver (4th-order Bessel–Thomson
    at ``f3db_rel × baud``); leave it off if the scope already did.
    """
    x = np.asarray(samples, dtype=np.float64).ravel()
    sym = test_pattern(symbols) if isinstance(symbols, str) else np.asarray(symbols, dtype=np.uint8).ravel()
    if x.size < 4 or sym.size < 2:
        raise ValueError("need at least a few samples and symbols")
    if sample_rate <= 0 or baud <= 0:
        raise ValueError("sample_rate and baud must be positive")
    n_sym = sym.size
    period = n_sym / baud  # [s]
    n_grid = n_sym * int(sps)
    duration = x.size / sample_rate
    periods = duration / period
    k = round(periods)
    if start == 0.0 and k >= 1 and abs(periods - k) <= PERIODIC_TOL * periods:
        y = _fft_resample(x, k * n_grid).reshape(k, n_grid).mean(axis=0)
        method = f"band-limited (FFT), {k} period(s) averaged"
    else:
        if duration < start + period * (1 - 1e-9):
            raise ValueError(
                f"the capture is {duration * 1e9:.3f} ns long but one pattern period is "
                f"{period * 1e9:.3f} ns ({n_sym} symbols at {baud / 1e9:g} GBd): capture at least one period")
        t = start + np.arange(n_grid) / (baud * sps)
        y = np.interp(t, np.arange(x.size) / sample_rate, x)
        method = f"linear interpolation of one period from t = {start * 1e9:g} ns ({periods:.4f} periods captured)"
    if reference_filter:
        f = np.fft.rfftfreq(n_grid, 1 / (baud * sps))
        y = np.fft.irfft(np.fft.rfft(y) * _bt4(f, f3db_rel * baud), n_grid)
    info = {
        "samples": int(x.size),
        "sample_rate": float(sample_rate),
        "periods_captured": float(periods),
        "method": method,
        "reference_filter": f"Bessel–Thomson 4th order at {f3db_rel:g} × baud" if reference_filter else None,
    }
    return MeasuredWaveform(y, sym, sps, baud, info)


def _read_numbers(path: Path, column):
    with open(path, newline="") as fh:
        sample = fh.read(4096)
        fh.seek(0)
        try:
            dialect = csv.Sniffer().sniff(sample, delimiters=",;\t ")
        except csv.Error:
            dialect = csv.excel
        rows = []
        for r in csv.reader(fh, dialect):
            vals = []
            for c in r:
                c = c.strip()
                if not c:
                    continue
                try:
                    vals.append(float(c))
                except ValueError:
                    vals = None
                    break
            if vals:  # skip header and comment lines
                rows.append(vals)
    width = min(len(r) for r in rows)
    a = np.array([r[:width] for r in rows])
    if column is not None:
        return None, a[:, column]
    if width >= 2:  # time, value
        return a[:, 0], a[:, 1]
    return None, a[:, 0]


def read_waveform(path, baud: float, symbols="prbs13q", *, sample_rate: float | None = None, column: int | None = None,
                  dtype: str = "float64", **kw) -> MeasuredWaveform:
    """Read a capture from a file and map it onto the grid (:func:`measured_waveform`).

    - ``.csv`` / ``.txt``: one column of values (then ``sample_rate`` is required), or two
      columns time [s], value (the sample rate is taken from the time column). Header and
      comment lines are skipped; ``column`` selects one column of a wider file.
    - ``.npy``: a 1-D array of values (``sample_rate`` required).
    - other: raw binary values of ``dtype`` (``sample_rate`` required).

    ``symbols``: a pattern name, an array, or a path to a text file of symbols (0…3, one per
    line or separated by commas/spaces).
    """
    path = Path(path)
    if isinstance(symbols, (str, Path)) and Path(symbols).suffix and Path(symbols).exists():
        symbols = np.array([int(v) for v in Path(symbols).read_text().replace(",", " ").split()], dtype=np.uint8)
    t = None
    suf = path.suffix.lower()
    if suf in (".csv", ".txt"):
        t, x = _read_numbers(path, column)
    elif suf == ".npy":
        x = np.load(path)
    else:
        x = np.fromfile(path, dtype=dtype)
    if t is not None and sample_rate is None:
        dt = np.diff(t)
        if not np.all(dt > 0) or np.max(np.abs(dt - dt.mean())) > 1e-3 * dt.mean():
            raise ValueError("the time column is not uniformly spaced; resample it or pass sample_rate")
        sample_rate = 1.0 / dt.mean()
    if sample_rate is None:
        raise ValueError("sample_rate is needed for a file without a time column")
    return measured_waveform(x, sample_rate, baud, symbols, **kw)


__all__ = ["test_pattern", "MeasuredWaveform", "measured_waveform", "read_waveform", "PERIODIC_TOL"]

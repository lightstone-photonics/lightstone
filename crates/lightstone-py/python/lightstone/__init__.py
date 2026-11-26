"""Lightstone – Optical Link Simulator (Python API).

Reference link: 850 nm VCSEL → connector → OM4 multimode fiber → PIN/TIA → PAM-4 diagnostics.
All quantities are SI (m, s, Hz, W, A, V) unless a key name says otherwise
(``*_dbm``, ``*_db``, ``*_db_per_km``, ``*_db_hz``).

Example::

    import lightstone
    sim = lightstone.Simulator({"fiber.length": 70.0, "launch.offset": 3e-6})
    res = sim.run()
    print(res.metrics["emb_length"] / 1e9, "MHz·km")
    a = res.analyze()
    print(a["ser"], a["eye_height"])
    print(res.tdecq()["tdecq_db"])
"""

from __future__ import annotations

import copy
import json
import warnings
import traceback
from typing import Any, Callable, Mapping

import numpy as np

from . import _lightstone
from .measured import MeasuredWaveform, measured_waveform, read_waveform, test_pattern  # noqa: E402

__all__ = [
    "default_config",
    "with_params",
    "Simulator",
    "LinkResult",
    "simulate",
    "System",
    "SystemError",
    "default_system",
    "lane",
    "connector",
    "fiber",
    "attenuator",
    "solve_modes",
    "couple",
    "analyze",
    "tdecq",
    "eye_density",
    "Graph",
    "GraphError",
    "ReceiverResult",
    "Component",
    "Context",
    "Waveform",
    "component",
    "register",
    "builtin_kinds",
    "fiber_presets",
    "test_pattern",
    "MeasuredWaveform",
    "measured_waveform",
    "read_waveform",
]

__version__ = "0.1.0"


def default_config() -> dict:
    """Default link configuration as a nested dict."""
    return json.loads(_lightstone.default_config_json())


def with_params(config: Mapping[str, Any] | None = None, params: Mapping[str, Any] | None = None) -> dict:
    """Return a copy of ``config`` with dotted-path overrides applied.

    ``with_params(cfg, {"fiber.length": 50.0, "rx.detector_radius": 15e-6})``
    """
    cfg = copy.deepcopy(dict(config) if config is not None else default_config())
    for key, value in (params or {}).items():
        node = cfg
        parts = key.split(".")
        for p in parts[:-1]:
            if p not in node or not isinstance(node[p], dict):
                raise KeyError(f"unknown config section '{p}' in '{key}'")
            node = node[p]
        if parts[-1] not in node:
            raise KeyError(f"unknown config key '{key}'")
        node[parts[-1]] = value
    return cfg


def _normalize(config: Mapping[str, Any] | None) -> dict:
    if config is None:
        return default_config()
    if any("." in k for k in config):
        return with_params(None, config)
    return with_params(config)


class LinkResult:
    """Output of :meth:`Simulator.run`. Waveforms are NumPy arrays sampled at ``sample_rate``."""

    def __init__(self, raw: dict):
        self.sample_rate: float = raw["sample_rate"]
        self.baud: float = raw["baud"]
        self.sps: int = raw["sps"]
        self.tx_levels = np.asarray(raw["tx_levels"])
        self.symbols: np.ndarray = raw["symbols"]
        #: Transmitted optical power (noiseless) [W]
        self.tx_power: np.ndarray = raw["tx_power"]
        #: Detected optical power before the receive filter (noiseless) [W]
        self.rx_optical: np.ndarray = raw["rx_optical"]
        #: Noisy TIA output [V]
        self.rx_signal: np.ndarray = raw["rx_signal"]
        #: Noiseless TIA output [V]
        self.rx_noiseless: np.ndarray = raw["rx_noiseless"]
        #: Analytic noise σ per sample [V]
        self.rx_sigma: np.ndarray = raw["rx_sigma"]
        #: Reference-receiver optical waveform (incl. RIN, no RX noise) [W] — TDECQ input
        self.reference_optical: np.ndarray = raw["reference_optical"]
        self.modes: list[dict] = json.loads(raw["modes_json"])
        self.metrics: dict = json.loads(raw["metrics_json"])

    @property
    def time(self) -> np.ndarray:
        """Time axis [s]."""
        return np.arange(self.rx_signal.size) / self.sample_rate

    def analyze(self, **options) -> dict:
        """PAM-4 (or NRZ) metrics of the receiver output (semi-analytic SER, EH, EW, ...)."""
        return analyze(self.rx_signal, self.symbols, self.sps, self.rx_noiseless, self.rx_sigma, **options)

    def tdecq(self, **options) -> dict:
        """TDECQ of the reference-receiver optical waveform."""
        return tdecq(self.reference_optical, self.symbols, self.sps, self.baud, **options)

    def eye(self, width: int = 600, height: int = 400, analysis: dict | None = None, v_range=None):
        """Eye density of the receiver output: (counts[height, width], metadata)."""
        a = analysis or self.analyze()
        return eye_density(self.rx_signal, self.sps, a["delay"], a["phase"], width, height, v_range)


class Simulator:
    """Cached link simulator. Changing only the fiber length or receiver settings reuses the
    mode solution, coupling and transmitter waveform."""

    def __init__(self, config: Mapping[str, Any] | None = None):
        self._cfg = _normalize(config)
        self._sim = _lightstone._Simulator(json.dumps(self._cfg))

    @property
    def config(self) -> dict:
        return copy.deepcopy(self._cfg)

    @config.setter
    def config(self, config: Mapping[str, Any]) -> None:
        self._cfg = _normalize(config)
        self._sim.set_config_json(json.dumps(self._cfg))

    def set(self, **params: Any) -> "Simulator":
        """Update parameters with ``section__key=value`` keywords, e.g. ``fiber__length=50``."""
        self.config = with_params(self._cfg, {k.replace("__", "."): v for k, v in params.items()})
        return self

    def run(self) -> LinkResult:
        return LinkResult(self._sim.run())


def simulate(config: Mapping[str, Any] | None = None) -> LinkResult:
    """Run one link simulation."""
    return Simulator(config).run()


def default_system() -> dict:
    """Default system: one lane (connector + 100 m OM4)."""
    return json.loads(_lightstone.default_system_json())


def connector(offset: float = 0.0, tilt: float = 0.0, loss_db: float = 0.0) -> dict:
    """Connector element. Before the first fiber it sets the launch offset/tilt."""
    return {"kind": "connector", "offset": offset, "tilt": tilt, "loss_db": loss_db}


def fiber(length: float = 100.0, **params) -> dict:
    """OM4 fiber span (any FiberConfig key may be overridden)."""
    f = dict(default_config()["fiber"], length=length, **params)
    f["kind"] = "fiber"
    return f


def attenuator(loss_db: float) -> dict:
    """Lumped optical attenuator."""
    return {"kind": "attenuator", "loss_db": loss_db}


def lane(elements=None, name: str = "Lane", seed: int = 0, **params) -> dict:
    """A lane: VCSEL transmitter → elements → receiver. ``params`` use dotted paths into the
    lane, e.g. ``{"vcsel.wavelength": 910e-9}`` via ``lane(..., **{"vcsel.wavelength": 910e-9})``."""
    ln = default_system()["lanes"][0]
    ln["name"], ln["seed"] = name, seed
    if elements is not None:
        ln["elements"] = list(elements)
    return with_params(ln, params) if params else ln


class SystemError(ValueError):
    """Raised when simulating a system that has validation errors."""


class System:
    """Multi-lane system simulator (see docs/architecture.md, Section 6).

    Example::

        sys = lightstone.System([
            lightstone.lane([lightstone.connector(offset=3e-6), lightstone.fiber(60), lightstone.connector(loss_db=0.3), lightstone.fiber(40)], name="A"),
            lightstone.lane([lightstone.fiber(150), lightstone.attenuator(2.0)], name="B", seed=1),
        ])
        results = sys.run()          # list of LinkResult, one per lane
    """

    def __init__(self, lanes=None):
        cfg = {"lanes": list(lanes)} if lanes is not None else default_system()
        self._cfg = cfg
        self._sys = _lightstone._System(json.dumps(cfg))

    @property
    def config(self) -> dict:
        return copy.deepcopy(self._cfg)

    @config.setter
    def config(self, cfg: Mapping[str, Any]) -> None:
        self._cfg = copy.deepcopy(dict(cfg))
        self._sys.set_config_json(json.dumps(self._cfg))

    def validate(self) -> list[dict]:
        """Validation issues: [{lane, element, severity, message}]."""
        return json.loads(self._sys.validate_json())

    def run_lane(self, i: int) -> LinkResult:
        try:
            return LinkResult(self._sys.run_lane(int(i)))
        except ValueError as e:
            raise SystemError(str(e)) from None

    def run(self) -> list[LinkResult]:
        return [self.run_lane(i) for i in range(len(self._cfg["lanes"]))]


def solve_modes(fiber: Mapping[str, Any] | None = None, wavelength: float = 850e-9) -> dict:
    """Solve the LP modes of a fiber (defaults: OM4 model)."""
    f = with_params({"fiber": default_config()["fiber"]}, {f"fiber.{k}": v for k, v in (fiber or {}).items()})
    return _lightstone.solve_modes(json.dumps(f["fiber"]), wavelength)


def couple(
    fiber: Mapping[str, Any] | None = None,
    launch: Mapping[str, Any] | None = None,
    wavelength: float = 850e-9,
) -> dict:
    """Launch coupling into fiber modes."""
    base = default_config()
    f = with_params({"fiber": base["fiber"]}, {f"fiber.{k}": v for k, v in (fiber or {}).items()})
    l = with_params({"launch": base["launch"]}, {f"launch.{k}": v for k, v in (launch or {}).items()})
    return _lightstone.couple(json.dumps(f["fiber"]), json.dumps(l["launch"]), wavelength)


def _f64(x) -> np.ndarray:
    return np.ascontiguousarray(x, dtype=np.float64)


def analyze(noisy, symbols, sps: int, noiseless=None, sigma=None, levels: int | None = None, **options) -> dict:
    """PAM-4 or NRZ analysis of a waveform (``len(noisy) == len(symbols) * sps``).

    ``levels`` is 4 (PAM-4) or 2 (NRZ); by default it is inferred from the symbols (2 if they
    are all 0 or 1). For NRZ the level, threshold and eye lists have 2 and 1 entries, and
    BER = SER (PAM-4: SER/2).
    """
    return json.loads(
        _lightstone.analyze(
            _f64(noisy),
            np.ascontiguousarray(symbols, dtype=np.uint8),
            int(sps),
            None if noiseless is None else _f64(noiseless),
            None if sigma is None else _f64(sigma),
            json.dumps(options) if options else None,
            None if levels is None else int(levels),
        )
    )


def tdecq(waveform, symbols, sps: int, baud: float, **options) -> dict:
    """TDECQ (IEEE 802.3-style, see docs/model/tdecq.md) of a reference-receiver waveform.

    PAM-4 only; NRZ symbols raise ``ValueError``.
    """
    return json.loads(
        _lightstone.tdecq(
            _f64(waveform),
            np.ascontiguousarray(symbols, dtype=np.uint8),
            int(sps),
            float(baud),
            json.dumps(options) if options else None,
        )
    )


def eye_density(samples, sps: int, delay: int, center_phase: float, width: int = 600, height: int = 400, v_range=None):
    """2-UI eye density: returns (counts[height, width] uint32, metadata dict)."""
    counts, meta = _lightstone.eye_density(_f64(samples), int(sps), int(delay), float(center_phase), int(width), int(height), v_range)
    meta = json.loads(meta)
    return counts.reshape(height, width), meta


# ---------------------------------------------------------------------------------------------
# Component graph and custom components (docs/architecture.md, Sections 7.3–7.4)
# ---------------------------------------------------------------------------------------------

#: Port types of custom components.
PORT_TYPES = ("data", "electrical", "current", "voltage", "drive", "optical")


#: Component kinds that were renamed: old name → current name (same as the engine's).
RENAMED_KINDS = {"decision": "receiver_output", "fiber": "custom_fiber"}

#: Parameters of a multimode fiber preset (`mmf`) that are not its physics.
MMF_KEYS = ("type", "length", "attenuation_db_per_km", "extra_loss_db")


def fiber_presets() -> dict:
    """Multimode fiber presets: ``{"version": n, "types": {"om3": {...}, "om4": ..., "om5": ...}}``,
    each the full ``custom_fiber`` parameters of the preset (minimum spec, 100 m)."""
    return json.loads(_lightstone.fiber_presets_json())


def builtin_kinds() -> list[str]:
    """Built-in component kinds."""
    return list(_lightstone.builtin_kinds())


class Context:
    """Simulation context passed to custom components."""

    def __init__(self, d: dict):
        #: Node id.
        self.node: str = d["node"]
        #: Node parameters from the graph.
        self.params: dict = json.loads(d["params_json"]) or {}
        self.baud: float = d["baud"]
        self.sps: int = d["sps"]
        self.n_symbols: int = d["n_symbols"]
        #: Number of samples.
        self.n: int = d["n"]
        self.sample_rate: float = d["sample_rate"]
        #: Seed for random numbers (derived from the graph seed and the node id).
        self.seed: int = d["seed"]

    @property
    def time(self) -> np.ndarray:
        """Time axis [s]."""
        return np.arange(self.n) / self.sample_rate


class Waveform:
    """Explicit electrical waveform at a port of a nonlinear custom component.

    Inputs carry the noiseless and the noisy waveform, the analytic σ(t) and whether σ(t) is
    exact. Outputs only need ``noiseless`` and ``noisy``. Apply a memoryless nonlinearity to
    both with :meth:`map`.
    """

    def __init__(self, noiseless, noisy=None, sigma=None, quantity: str | None = None, gaussian_exact: bool = False):
        self.noiseless = np.asarray(noiseless, dtype=np.float64)
        self.noisy = self.noiseless if noisy is None else np.asarray(noisy, dtype=np.float64)
        self.sigma = sigma
        self.quantity = quantity
        self.gaussian_exact = gaussian_exact

    def map(self, f: Callable[[np.ndarray], np.ndarray]) -> "Waveform":
        """Apply ``f`` to the noiseless and the noisy waveform."""
        return Waveform(f(self.noiseless), f(self.noisy), quantity=self.quantity)


def _ports(ports) -> list[dict]:
    items = ports.items() if isinstance(ports, Mapping) else ports
    out = []
    for name, ty in items:
        if ty not in PORT_TYPES:
            raise ValueError(f"port '{name}': unknown type '{ty}' (one of {', '.join(PORT_TYPES)})")
        out.append({"name": str(name), "type": ty})
    return out


DOMAINS = ("time", "frequency")
TAP_SPACINGS = ("sample", "symbol")


class Component:
    """A custom component kind (docs/architecture.md, Section 7.4).

    ``linear`` and ``domain`` must always be given; together they say what ``function``
    describes:

    ============  ============  =============================================================
    ``linear``    ``domain``    ``function`` / alternative
    ============  ============  =============================================================
    ``True``      frequency     ``function(f, ctx)`` → H(f) at the frequencies ``f`` [Hz]
                                (FFT order, negative frequencies included; H(−f) = H(f)*).
    ``True``      time          ``function(t, ctx)`` → impulse response h(t) [1/s] at the
                                times ``t`` [s] of one period (negative times included);
                                ∫h dt is the DC gain. **Or** ``taps=[c0, c1, …]`` with
                                ``tap_spacing="sample"`` or ``"symbol"`` (dimensionless FIR,
                                tap i at delay i·spacing, Σc is the DC gain). ``taps`` may also
                                be a function ``taps(ctx)``.
    ``False``     time          ``function(inputs, ctx)`` → output waveforms (see below).
    ``False``     frequency     not possible: a nonlinear component has no H(f).
    ============  ============  =============================================================

    Linear components need one electrical or optical input and one output of the same kind.
    The engine turns h(t) or the taps into H(f) on the periodic grid (exact) and applies it,
    so signals stay lazy and the analytic noise σ(t) stays exact. Optical H(f) acts on optical
    power and is common to all mode bins.

    Nonlinear components get a dict of input port name → :class:`Waveform` (electrical) or
    symbol array (data) and return a dict of output port → :class:`Waveform` /
    ``(noiseless, noisy)`` (electrical) or symbol array (data). Noise that passes the block is
    no longer Gaussian, so SERs downstream are counted (Monte Carlo). Optical ports are not
    supported for nonlinear components yet.

    Custom components are unvalidated: they run your code as is.
    """

    def __init__(self, kind: str, *, inputs, outputs, linear: bool, domain: str, function: Callable | None = None, taps=None, tap_spacing: str | None = None, doc: str = ""):
        if not isinstance(linear, bool):
            raise TypeError(f"linear must be True or False, not {linear!r}")
        if domain not in DOMAINS:
            raise ValueError(f"domain must be 'time' or 'frequency', not {domain!r}")
        if not linear and domain == "frequency":
            raise ValueError("a nonlinear component has no transfer function H(f); use domain='time' (it works on explicit waveforms)")
        if taps is not None:
            if not (linear and domain == "time"):
                raise ValueError("taps are only possible for linear=True, domain='time'")
            if function is not None:
                raise ValueError("give either function or taps, not both")
            if tap_spacing not in TAP_SPACINGS:
                raise ValueError(f"taps need tap_spacing='sample' or 'symbol', not {tap_spacing!r}")
        else:
            if function is None:
                raise ValueError("give a function" + (" or taps" if linear and domain == "time" else ""))
            if tap_spacing is not None:
                raise ValueError("tap_spacing is only used with taps")
        self.kind = kind
        self.inputs = _ports(inputs)
        self.outputs = _ports(outputs)
        self.linear = linear
        self.domain = domain
        self.function = function
        self.taps = taps
        self.tap_spacing = tap_spacing
        self.doc = doc or ""

    def _spec_json(self) -> str:
        return json.dumps({"inputs": self.inputs, "outputs": self.outputs, "linear": self.linear, "domain": self.domain, "doc": self.doc})

    def __repr__(self) -> str:
        what = "taps" if self.taps is not None else "function"
        return f"Component({self.kind!r}, linear={self.linear}, domain={self.domain!r}, {what})"

    # Called from the Rust engine. Exceptions carry the Python traceback.
    def _transfer(self, ctx: dict, f: np.ndarray):
        try:
            h = np.asarray(self.function(f, Context(ctx)), dtype=np.complex128)
            h = np.broadcast_to(h, f.shape)
            return np.ascontiguousarray(h.real), np.ascontiguousarray(h.imag)
        except Exception:
            raise RuntimeError(traceback.format_exc()) from None

    def _impulse(self, ctx: dict, t: np.ndarray):
        try:
            c = Context(ctx)
            if self.taps is not None:
                taps = self.taps(c) if callable(self.taps) else self.taps
                taps = np.ascontiguousarray(taps, dtype=np.float64).ravel()
                return "taps", taps, self.tap_spacing
            h = np.asarray(self.function(t, c))
            if np.iscomplexobj(h):
                raise TypeError("an impulse response must be real")
            h = np.broadcast_to(h.astype(np.float64), t.shape)
            return "continuous", np.ascontiguousarray(h), None
        except Exception:
            raise RuntimeError(traceback.format_exc()) from None

    def _process(self, ctx: dict, inputs: list):
        try:
            args = {}
            for port, x in zip(self.inputs, inputs):
                if x[0] == "data":
                    args[port["name"]] = np.asarray(x[1])
                else:
                    q, y0, y, sg, exact = x
                    args[port["name"]] = Waveform(y0, y, sg, quantity=q, gaussian_exact=exact)
            res = self.function(args, Context(ctx))
            if not isinstance(res, Mapping):
                raise TypeError("process must return a dict of output port → value")
            out = []
            for port in self.outputs:
                name = port["name"]
                if name not in res:
                    raise KeyError(f"process returned no value for output '{name}'")
                v = res[name]
                if port["type"] == "data":
                    sym, levels = (v if isinstance(v, tuple) else (v, 4))
                    out.append(("data", np.ascontiguousarray(sym, dtype=np.uint8), int(levels)))
                else:
                    if isinstance(v, tuple):
                        v = Waveform(*v)
                    elif not isinstance(v, Waveform):
                        v = Waveform(v)
                    out.append(("electrical", np.ascontiguousarray(v.noiseless), np.ascontiguousarray(v.noisy)))
            return out
        except Exception:
            raise RuntimeError(traceback.format_exc()) from None


_REGISTRY: dict[str, Component] = {}


def register(comp: Component) -> Component:
    """Make a custom component kind available to every :class:`Graph` by its name."""
    if comp.kind in builtin_kinds():
        raise ValueError(f"'{comp.kind}' is a built-in component kind")
    _REGISTRY[comp.kind] = comp
    return comp


def component(kind: str, *, inputs, outputs, linear: bool, domain: str, doc: str | None = None):
    """Decorator that defines and registers a custom component. ``linear`` and ``domain``
    are required; see :class:`Component` for what the function receives and returns.

    Example (linear, frequency domain: H(f))::

        @lightstone.component("rc_filter", inputs={"in": "electrical"}, outputs={"out": "electrical"},
                              linear=True, domain="frequency")
        def rc(f, ctx):
            return 1 / (1 + 1j * f / ctx.params.get("fc", 20e9))

    Example (linear, time domain: impulse response h(t) [1/s], here a Gaussian pulse)::

        @lightstone.component("gauss_pulse", inputs={"in": "electrical"}, outputs={"out": "electrical"},
                              linear=True, domain="time")
        def gauss(t, ctx):
            s = ctx.params.get("sigma", 8e-12)
            return np.exp(-t**2 / (2 * s**2)) / (np.sqrt(2 * np.pi) * s)

    Example (nonlinear, time domain: waveforms)::

        @lightstone.component("limiter", inputs={"in": "voltage"}, outputs={"out": "voltage"},
                              linear=False, domain="time")
        def limiter(inputs, ctx):
            return {"out": inputs["in"].map(lambda v: np.clip(v, 0.3, 0.6))}

    FIR taps have no function; create them with ``Component(..., taps=…, tap_spacing=…)``.
    """

    def deco(fn):
        return register(Component(kind, inputs=inputs, outputs=outputs, linear=linear, domain=domain, function=fn, doc=doc or fn.__doc__ or ""))

    return deco


class GraphError(ValueError):
    """Graph validation or evaluation failed. ``issues``: [{node, wire, message}]."""

    def __init__(self, issues: list[dict]):
        self.issues = issues
        msgs = []
        for i in issues:
            where = i.get("node") or (f"wire {i['wire'] + 1}" if i.get("wire") is not None else None)
            msgs.append(f"{where}: {i['message']}" if where else i["message"])
        super().__init__("; ".join(msgs))


class ReceiverResult:
    """Materialized receiver waveforms from a ``receiver_output`` node."""

    def __init__(self, report: dict, symbols: np.ndarray, settings: dict):
        self.noiseless: np.ndarray = report["noiseless"]
        self.signal: np.ndarray = report["signal"]
        self.sigma: np.ndarray = report["sigma"]
        #: True if σ(t) describes the noise exactly (all blocks since the noise sources linear).
        self.gaussian_exact: bool = report["gaussian_exact"]
        self.symbols = symbols
        self.sps: int = settings["sps"]
        self.baud: float = settings["baud"]
        self.sample_rate = self.baud * self.sps

    @property
    def ser_method(self) -> str:
        """``semi_analytic`` while the noise is exactly Gaussian, else ``monte_carlo``."""
        return "semi_analytic" if self.gaussian_exact else "monte_carlo"

    @property
    def time(self) -> np.ndarray:
        return np.arange(self.signal.size) / self.sample_rate

    def analyze(self, **options) -> dict:
        """PAM-4 (or NRZ) metrics. After a nonlinear block, ``ser`` is the counted (Monte Carlo) SER;
        ``ser_resolution`` is the smallest nonzero SER that method can show (``None`` for the
        semi-analytic SER)."""
        n = len(self.symbols)
        if self.gaussian_exact:
            a = analyze(self.signal, self.symbols, self.sps, self.noiseless, self.sigma, **options)
        else:
            a = analyze(self.signal, self.symbols, self.sps, **options)
            a["ser"] = a["ser_counted"]
            # Gray-coded PAM-4: BER = SER/2; NRZ: BER = SER.
            a["ber"] = a["ser"] if len(a["level_mean"]) == 2 else a["ser"] / 2
        a["ser_method"] = self.ser_method
        # The semi-analytic SER has no pattern-length limit; counting cannot show SER < 1/N.
        a["ser_resolution"] = None if self.gaussian_exact else 1.0 / n
        return a

    def eye(self, width: int = 600, height: int = 400, analysis: dict | None = None, v_range=None):
        a = analysis or self.analyze()
        return eye_density(self.signal, self.sps, a["delay"], a["phase"], width, height, v_range)


def _ref(s: str, default_port: str) -> str:
    return s if "." in s else f"{s}.{default_port}"


class Graph:
    """Component graph (docs/architecture.md, Section 7). Nodes, parameters and wires are a JSON
    document; the engine caches node outputs, so a change recomputes only what is downstream.

    Example::

        g = lightstone.Graph(settings={"sps": 8})
        g.add("src", "prbs13q").add("drv", "driver").add("vcsel", "vcsel")
        g.add("con", "connector").add("fib", "mmf", type="om4", length=50.0)
        g.add("pin", "pin").add("tia", "tia").add("rxf", "bessel_thomson").add("rx", "receiver_output")
        g.chain("src", "drv", "vcsel", "con", "fib", "pin", "tia", "rxf", "rx")
        rx = g.run().receiver("rx")
        print(rx.analyze()["ser"])

    Custom kinds are looked up in the components passed here, then in the global registry
    (:func:`component`, :func:`register`).
    """

    def __init__(self, config: Mapping[str, Any] | None = None, *, settings: Mapping[str, Any] | None = None, components=()):
        cfg = copy.deepcopy(dict(config)) if config is not None else {}
        cfg.setdefault("settings", {})
        cfg.setdefault("nodes", [])
        cfg.setdefault("wires", [])
        cfg["settings"].update(settings or {})
        self._cfg = cfg
        self._components = {c.kind: c for c in components}
        self._registered: dict[str, Component] = {}
        self._g = _lightstone._Graph(json.dumps(cfg))
        # Fill in the defaults of all settings.
        self._cfg["settings"] = json.loads(self._g.graph_json())["settings"]

    # -- construction -------------------------------------------------------------------

    @classmethod
    def reference(cls, lane_config: Mapping[str, Any] | None = None, *, resolution: str = "mode", level: str = "power", components=()) -> "Graph":
        """Preset graph of a lane (default: the reference link). Node ids: ``src``, ``drv``,
        ``vcsel``, ``el0`` … (lane elements), ``pin``, ``tia``, ``rxf``, ``ffe`` (receiver
        equalizer; absent with ``{"rx": {"ffe_taps": 0}}``), ``rx``."""
        j = _lightstone.lane_graph_json(None if lane_config is None else json.dumps(lane_config), resolution, level)
        return cls(json.loads(j), components=components)

    @classmethod
    def from_json(cls, text: str, components=()) -> "Graph":
        return cls(json.loads(text), components=components)

    def to_json(self, indent: int | None = 2) -> str:
        return json.dumps(self._cfg, indent=indent)

    @property
    def config(self) -> dict:
        return copy.deepcopy(self._cfg)

    @property
    def settings(self) -> dict:
        return self._cfg["settings"]

    def _node(self, id: str) -> dict:
        for n in self._cfg["nodes"]:
            if n["id"] == id:
                return n
        raise KeyError(f"no node '{id}'")

    def nodes(self) -> list[str]:
        return [n["id"] for n in self._cfg["nodes"]]

    def use(self, *components: Component) -> "Graph":
        """Use these custom components in this graph (they take precedence over the global
        registry). Replacing a kind's component recomputes all nodes on the next run."""
        self._components.update({c.kind: c for c in components})
        return self

    def add(self, id: str, kind: str, params: Mapping[str, Any] | None = None, **kw) -> "Graph":
        """Add a node. Parameters as a dict and/or keywords."""
        if id in self.nodes():
            raise ValueError(f"node '{id}' exists already")
        p = {**(params or {}), **kw}
        self._cfg["nodes"].append({"id": id, "kind": kind, "params": p})
        return self

    def remove(self, id: str) -> "Graph":
        """Remove a node and all wires attached to it."""
        self._node(id)
        self._cfg["nodes"] = [n for n in self._cfg["nodes"] if n["id"] != id]
        self._cfg["wires"] = [w for w in self._cfg["wires"] if w["from"].split(".")[0] != id and w["to"].split(".")[0] != id]
        return self

    def params(self, id: str) -> dict:
        return copy.deepcopy(self._node(id).get("params") or {})

    def set(self, id: str, params: Mapping[str, Any] | None = None, **kw) -> "Graph":
        """Update (merge) node parameters.

        A multimode fiber preset (``mmf``) only has ``type``, ``length``, attenuation and extra
        loss. Setting any other fiber parameter (e.g. ``profile``) converts the node to a
        ``custom_fiber`` with the preset's values, and warns."""
        n = self._node(id)
        p = n.get("params") or {}
        new = {**(params or {}), **kw}
        if n["kind"] == "mmf" and any(k not in MMF_KEYS for k in new):
            t = p.get("type", "om4")
            base = dict(fiber_presets()["types"][t])
            base.update({k: v for k, v in p.items() if k != "type"})
            p = base
            n["kind"] = "custom_fiber"
            warnings.warn(f"node '{id}': converted the {t.upper()} preset to a custom fiber (physics changed: "
                          f"{', '.join(k for k in new if k not in MMF_KEYS)})", stacklevel=2)
        p.update(new)
        n["params"] = p
        return self

    def replace(self, id: str, kind: str, params: Mapping[str, Any] | None = None, **kw) -> "Graph":
        """Change a node's kind and parameters; wires stay."""
        n = self._node(id)
        n["kind"], n["params"] = kind, {**(params or {}), **kw}
        return self

    def wire(self, src: str, dst: str) -> "Graph":
        """Connect ``node.port`` → ``node.port`` (a bare node id means ``out`` / ``in``)."""
        self._cfg["wires"].append({"from": _ref(src, "out"), "to": _ref(dst, "in")})
        return self

    def unwire(self, dst: str) -> "Graph":
        """Remove the wire into input ``node.port``."""
        d = _ref(dst, "in")
        self._cfg["wires"] = [w for w in self._cfg["wires"] if w["to"] != d]
        return self

    def chain(self, *ids: str) -> "Graph":
        """Wire ``a.out → b.in`` for consecutive node ids."""
        for a, b in zip(ids, ids[1:]):
            self.wire(a, b)
        return self

    def insert(self, before: str, id: str, kind: str, params: Mapping[str, Any] | None = None, **kw) -> "Graph":
        """Insert a node with ports ``in``/``out`` into the wire feeding input ``before``."""
        d = _ref(before, "in")
        for w in self._cfg["wires"]:
            if w["to"] == d:
                self.add(id, kind, params, **kw)
                src, w["from"] = w["from"], f"{id}.out"
                self._cfg["wires"].append({"from": src, "to": f"{id}.in"})
                return self
        raise KeyError(f"no wire into '{d}'")

    # -- evaluation ---------------------------------------------------------------------

    def _sync(self) -> None:
        builtin = set(builtin_kinds())
        for n in self._cfg["nodes"]:
            # Renamed kinds (older saved graphs): use the current name.
            n["kind"] = RENAMED_KINDS.get(n["kind"], n["kind"])
            kind = n["kind"]
            if kind in builtin:
                continue
            comp = self._components.get(kind) or _REGISTRY.get(kind)
            if comp is None or self._registered.get(kind) is comp:
                continue  # unknown kinds are reported by the engine
            try:
                self._g.register(kind, comp._spec_json(), comp)
            except ValueError as e:
                raise GraphError([{"node": n["id"], "wire": None, "message": str(e)}]) from None
            self._registered[kind] = comp
        self._g.set_graph_json(json.dumps(self._cfg))

    def validate(self) -> list[dict]:
        """Structural problems: [{node, wire, message}] (empty if the graph can run)."""
        self._sync()
        return json.loads(self._g.validate_json())

    def ports(self) -> dict:
        """Ports of every node: {id: {kind, linear, inputs, outputs}}."""
        self._sync()
        return {p["id"]: p for p in json.loads(self._g.ports_json())}

    def run(self) -> "Graph":
        """Evaluate the graph (only nodes affected by changes are recomputed)."""
        self._sync()
        issues = self._g.run()
        if issues is not None:
            raise GraphError(json.loads(issues))
        return self

    @property
    def computed_last_run(self) -> int:
        """Number of nodes recomputed in the last run."""
        return self._g.computed_last_run()

    def warnings(self) -> list[tuple[str, str]]:
        """(node, message) from the last run."""
        return list(self._g.warnings())

    def report(self, node: str) -> dict:
        """Report of a node from the last run. ``type`` says what it is: ``transmitter``,
        ``fiber_span``, ``detection``, ``filter``, ``receiver``, ``power_meter``, or the
        result of an analysis node: ``pam4_analysis``, ``eye_diagram`` (``counts`` as a
        [height, width] array), ``tdecq``, ``ffe``."""
        r = self._g.report(node)
        if r is None:
            raise KeyError(f"node '{node}' has no report (not run yet, or no report)")
        if "json" in r:
            r = {"type": r["type"], **json.loads(r["json"])}
            if r["type"] == "eye_diagram":
                r["counts"] = np.asarray(r["counts"], dtype=np.uint32).reshape(r["height"], r["width"])
        return r

    def output(self, node: str, port: str = "out") -> dict:
        """Value of an output port: symbols (data), explicit waveforms (electrical) or
        average power (optical)."""
        r = self._g.output(node, port)
        if r is None:
            raise KeyError(f"no output '{node}.{port}' (not run yet?)")
        return r

    def power(self, node: str) -> float:
        """Average optical power [W] at a power meter."""
        return self.report(node)["average_power"]

    def receiver(self, node: str | None = None, symbols_from: str | None = None) -> ReceiverResult:
        """Receiver waveforms of a ``receiver_output`` node (default: the only one in the graph),
        with the transmitted symbols from ``symbols_from`` (``node`` or ``node.port``; default:
        the only data source)."""
        if node is None:
            outs = [n["id"] for n in self._cfg["nodes"] if RENAMED_KINDS.get(n["kind"], n["kind"]) == "receiver_output"]
            if len(outs) != 1:
                raise ValueError(f"cannot pick the receiver output automatically ({len(outs)} receiver_output nodes); pass node=")
            node = outs[0]
        rep = self.report(node)
        if rep.get("type") != "receiver":
            raise KeyError(f"node '{node}' is not a receiver_output node")
        if symbols_from is None:
            srcs = [
                f"{p['id']}.{o['name']}"
                for p in self.ports().values()
                if not p["inputs"]
                for o in p["outputs"]
                if o["type"] == "data"
            ]
            if len(srcs) != 1:
                raise ValueError(f"cannot pick the symbol source automatically ({len(srcs)} candidates); pass symbols_from=")
            symbols_from = srcs[0]
        n, _, p = _ref(symbols_from, "out").partition(".")
        sym = self.output(n, p)
        if sym["type"] != "data":
            raise ValueError(f"'{symbols_from}' is not a data output")
        return ReceiverResult(rep, sym["symbols"], self.settings)

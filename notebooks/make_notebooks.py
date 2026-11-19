"""Generate the Lightstone notebooks from plain Python cell lists (keeps notebooks diff-friendly).

Run:  python notebooks/make_notebooks.py && jupyter nbconvert --execute --inplace notebooks/*.ipynb
"""

from pathlib import Path

import nbformat as nbf

HERE = Path(__file__).parent


def nb(cells):
    n = nbf.v4.new_notebook()
    n.metadata["kernelspec"] = {"name": "python3", "display_name": "Python 3", "language": "python"}
    n.cells = [nbf.v4.new_markdown_cell(c[3:]) if c.startswith("md:") else nbf.v4.new_code_cell(c) for c in cells]
    return n


validation = [
    "md:# Lightstone validation report\n\nRe-runs the analytical validation checks (docs/validation.md, Section 1) from Python and plots them. "
    "The authoritative pass/fail gates are the Rust tests (`cargo test`) and `tests/python`.",
    "import math, sys\nimport numpy as np\nimport matplotlib.pyplot as plt\nfrom scipy.special import jv, kv, jn_zeros\nfrom scipy.optimize import brentq\nimport lightstone\nsys.path.insert(0, '../reference')\nimport lightstone_ref",
    "md:## V1 – step-index LP modes vs. Bessel eigenvalue equation (V = 4.5)",
    """lam, na, V = 850e-9, 0.1, 4.5
a = V * lam / (2 * math.pi * na)
res = lightstone.solve_modes({"core_radius": a, "na": na, "profile": {"kind": "step"}, "grid_points_per_radius": 800, "domain_factor": 8.0}, lam)
def b_exact(l, V):
    f = lambda u: u * jv(l - 1, u) / jv(l, u) + math.sqrt(V*V - u*u) * kv(l - 1, math.sqrt(V*V - u*u)) / kv(l, math.sqrt(V*V - u*u))
    z = [0.0] + list(jn_zeros(l, 10)); out = []
    for lo, hi in zip(z[:-1], z[1:]):
        if lo + 1e-9 < min(hi, V) - 1e-9:
            try: out.append(1 - (brentq(f, lo + 1e-9, min(hi, V) - 1e-9, xtol=1e-15) / V) ** 2)
            except ValueError: pass
    return out
n1, n2 = res["n_core"], res["n_clad"]
for m in res["modes"]:
    b = b_exact(m["l"], V)[m["m"] - 1]
    print(f"LP{m['l']}{m['m']}: Δn_eff = {abs(m['n_eff'] - math.sqrt(n2**2 + b*(n1**2 - n2**2))):.2e}")""",
    "md:## V2 – parabolic profile: mode-group spacing\n\nFor an infinite parabola, β² = k²n₁² − 2kn₁√(2Δ)·g/a.",
    """ms = lightstone.solve_modes({"profile": {"kind": "graded", "alpha": 2.0}, "grid_points_per_radius": 800})
k = 2 * math.pi / 850e-9; n1, n2 = ms["n_core"], ms["n_clad"]; d = (n1**2 - n2**2) / (2 * n1**2)
g = np.array([m["group"] for m in ms["modes"]]); ne = np.array([m["n_eff"] for m in ms["modes"]])
ex = np.sqrt(k*k*n1*n1 - 2*k*n1*math.sqrt(2*d)/25e-6*g) / k
fig, ax = plt.subplots(1, 2, figsize=(11, 3.5))
ax[0].plot(g, ne, "o", label="solver"); ax[0].plot(g, ex, "-", label="infinite parabola"); ax[0].set(xlabel="group g", ylabel="n_eff"); ax[0].legend()
ax[1].semilogy(g, np.abs(ne - ex), "o"); ax[1].axhline(1e-5, color="r", ls="--"); ax[1].set(xlabel="group g", ylabel="|Δn_eff|", title="deviation grows near cutoff (finite core)")
plt.tight_layout()""",
    "md:## V4 – coaxial Gaussian on LP01 of a parabolic fiber",
    """w1 = math.sqrt(2 * (25e-6)**2 / ms["v_number"])
w0s = np.linspace(2e-6, 12e-6, 11); num = []
for w0 in w0s:
    c = lightstone.couple({"profile": {"kind": "graded", "alpha": 2.0}, "grid_points_per_radius": 400}, {"beam_radius": w0, "vcsel_mode_groups": 1})
    num.append(c["power"][0])
ana = (2 * w0s * w1 / (w0s**2 + w1**2))**2
print("max relative error:", np.max(np.abs(np.array(num) - ana) / ana))
plt.plot(w0s * 1e6, ana, "-", label="analytic"); plt.plot(w0s * 1e6, num, "o", label="Lightstone"); plt.xlabel("w0 [µm]"); plt.ylabel("LP01 coupling"); plt.legend();""",
    "md:## V8 – OM4 model bandwidths and DMD\n\nRelative group delay per mode for the default OM4 preset (α = 2.0777, germania-doped core) and the OFL/EMB values.",
    """om4 = lightstone.solve_modes()
r = lightstone.simulate()
gd = np.array([m["group_delay"] for m in om4["modes"]]); gg = np.array([m["group"] for m in om4["modes"]])
plt.plot(gg, (gd - gd[0]) * 1e15, "o"); plt.xlabel("principal mode group"); plt.ylabel("relative delay [ps/km]")
print(f"OFL bandwidth: {om4['ofl_bandwidth_length']/1e9:.0f} MHz·km (OM4 min 3500)")
print(f"EMB (default launch): {r.metrics['emb_length']/1e9:.0f} MHz·km (OM4 min 4700)")""",
    "md:## V9/V10 – receiver noise calibration",
    """r0 = lightstone.simulate({"tx.p_avg_dbm": -40.0, "rx.rin_noise": False, "rx.shot_noise": False, "tx.pattern_periods": 8})
resid = r0.rx_signal - r0.rx_noiseless
print("thermal: measured/expected variance =", np.var(resid) / np.mean(r0.rx_sigma**2))
r1 = lightstone.simulate({"tx.pattern_periods": 8})
print("thermal+shot+RIN: measured/expected =", np.var(r1.rx_signal - r1.rx_noiseless) / np.mean(r1.rx_sigma**2))""",
    "md:## V15 – Rust vs NumPy reference",
    """cfg = lightstone.with_params(None, {"fiber.length": 120.0, "fiber.grid_points_per_radius": 300, "launch.offset": 4e-6, "rx.rin_noise": False, "rx.ffe_taps": 0})  # the reference has no FFE
rr, ref = lightstone.simulate(cfg), lightstone_ref.simulate_noiseless(cfg)
for k in ["tx_power", "rx_optical", "rx_noiseless", "reference_optical", "rx_sigma"]:
    a, b = getattr(rr, k), ref[k]
    print(f"{k:18s} max rel. diff {np.max(np.abs(a - b)) / np.max(np.abs(b)):.1e}")""",
]

sweeps = [
    "md:# Length and offset sweeps\n\nTDECQ, semi-analytic SER and channel bandwidth versus fiber length and connector offset for the default link.",
    "import numpy as np\nimport matplotlib.pyplot as plt\nimport lightstone",
    """sim = lightstone.Simulator()
L = np.arange(0, 251, 25.0); td, ser, bw = [], [], []
for l in L:
    r = sim.set(fiber__length=float(l)).run()
    t = r.tdecq()["tdecq_db"]; td.append(np.nan if t is None else t)
    ser.append(r.analyze()["ser"]); b = r.metrics["channel_bandwidth"]; bw.append(np.nan if b is None else b / 1e9)  # None: no roll-off (L = 0)
fig, ax = plt.subplots(1, 3, figsize=(14, 3.5))
ax[0].plot(L, td, "o-"); ax[0].axhline(4.5, color="r", ls="--", label="example TDECQ limit"); ax[0].set(xlabel="length [m]", ylabel="TDECQ [dB]"); ax[0].legend()
ax[1].semilogy(L, ser, "o-"); ax[1].set(xlabel="length [m]", ylabel="SER (5-tap FFE)")
ax[2].plot(L, bw, "o-"); ax[2].set(xlabel="length [m]", ylabel="channel BW [GHz]"); plt.tight_layout()""",
    "md:The 4.5 dB line is only a visual reference; check the applicable PMD clause for the actual limit.",
    """sim = lightstone.Simulator({"fiber.length": 100.0})
off = np.arange(0, 21, 2.0); td, emb = [], []
for o in off:
    r = sim.set(launch__offset=o * 1e-6).run()
    t = r.tdecq()["tdecq_db"]; td.append(np.nan if t is None else t); emb.append(r.metrics["emb_length"] / 1e9 if r.metrics["emb_length"] else np.nan)
fig, ax = plt.subplots(1, 2, figsize=(10, 3.5))
ax[0].plot(off, td, "o-"); ax[0].set(xlabel="offset [µm]", ylabel="TDECQ @100 m [dB]")
ax[1].plot(off, emb, "o-"); ax[1].set(xlabel="offset [µm]", ylabel="modal BW [MHz·km]"); plt.tight_layout()""",
    """r = lightstone.simulate({"fiber.length": 100.0})
a = r.analyze(); counts, meta = r.eye(analysis=a)
plt.figure(figsize=(7, 4)); plt.imshow(np.log1p(counts), aspect="auto", cmap="inferno", extent=[-1, 1, meta["v_min"]*1e3, meta["v_max"]*1e3])
plt.xlabel("time [UI]"); plt.ylabel("TIA output [mV]"); plt.title("Eye @ 100 m (after the 5-tap receiver FFE)");""",
]

custom_component = [
    "md:# Custom component: load it, wire it into the reference graph, manipulate the signal\n\n"
    "The example component `example_component` (file `examples/components/example_component.py`, model doc "
    "`docs/model/example-component.md`) is an electrical gain and delay block, $H(f) = g\\,e^{-j2\\pi f\\tau}$. "
    "With its defaults ($g = 1$, $\\tau = 0$) $H(f) = 1$. This notebook loads it from its file, inserts it into the "
    "reference graph between the receive filter `rxf` and the FFE, and changes the signal step by step.",
    """import importlib.util
import numpy as np
import matplotlib.pyplot as plt
import lightstone

# Load the component from its file. Importing the module only creates the Component object;
# it is handed to the graph below (components=[...] or g.use(...)).
spec = importlib.util.spec_from_file_location("example_component", "../examples/components/example_component.py")
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
mod.example_component""",
    "md:## 1. The reference graph as it is",
    """g = lightstone.Graph.reference()
print(g.nodes())
for w in g.config["wires"]:
    print(f"  {w['from']:8s} → {w['to']}")
g.run()
base = g.receiver()
a0 = base.analyze()
print(f"SER {a0['ser']:.2e}, eye heights {np.round(np.array(a0['eye_height']) * 1e3, 1)} mV")""",
    "md:## 2. Rewire: insert the example component between `rxf` and `ffe`\n\n"
    "`insert(\"ffe\", ...)` cuts the wire into `ffe.in` and puts the new node in it: `rxf → ex → ffe`. "
    "With $H(f) = 1$ the receiver output is bit-identical to the reference graph.",
    """g.use(mod.example_component).insert("ffe", "ex", "example_component")
for w in g.config["wires"]:
    if "ex" in (w["from"].split(".")[0], w["to"].split(".")[0]):
        print(f"  {w['from']:8s} → {w['to']}")
g.run()
r = g.receiver()
print("max |Δ signal| =", np.max(np.abs(r.signal - base.signal)), " max |Δ σ| =", np.max(np.abs(r.sigma - base.sigma)))
print("SER", r.analyze()["ser"], "(nodes computed:", g.computed_last_run, "— all, because a new kind was registered)")""",
    "md:## 3. Gain: $g = 0.5$\n\n"
    "The block is linear, so Lightstone scales the signal **and** every noise term by $g$. The eye halves, "
    "the SER does not change. Only `ex`, `ffe` and `rx` are recomputed.",
    """def snippet(g, node, n_sym=24):
    o = g.output(node)
    sps = g.settings["sps"]
    t = np.arange(n_sym * sps) / sps
    return t, o["noiseless"][: n_sym * sps], o["signal"][: n_sym * sps]

g.set("ex", gain=0.5).run()
a = g.receiver().analyze()
print(f"SER {a['ser']:.2e}, eye heights {np.round(np.array(a['eye_height']) * 1e3, 1)} mV, nodes computed: {g.computed_last_run}")
fig, ax = plt.subplots(figsize=(10, 3.2))
for node, lab, col in (("rxf", "input (rxf)", "C0"), ("ex", "output (ex), g = 0.5", "C3")):
    t, y0, y = snippet(g, node)
    ax.plot(t, y * 1e3, lw=0.6, alpha=0.5, color=col)
    ax.plot(t, y0 * 1e3, lw=1.5, color=col, label=lab)
ax.set(xlabel="time [UI]", ylabel="voltage [mV]", title="Example component: gain 0.5 (thin: one noisy realization)")
ax.legend(); plt.tight_layout()""",
    "md:## 4. Delay: $\\tau = 0.25$ UI\n\n"
    "A pure delay shifts the waveform. The analysis finds the new symbol alignment by itself, so the SER is the same; "
    "the alignment delay grows by 0.25 UI × 16 samples/UI = 4 samples.",
    """T = 1 / g.settings["baud"]
g.set("ex", gain=1.0, delay=0.25 * T).run()
a = g.receiver().analyze()
print(f"SER {a['ser']:.2e}; alignment delay {a0['delay']} → {a['delay']} samples")
fig, ax = plt.subplots(figsize=(10, 3.2))
for node, lab in (("rxf", "input (rxf)"), ("ex", "output (ex), τ = 0.25 UI")):
    t, y0, _ = snippet(g, node, 12)
    ax.plot(t, y0 * 1e3, lw=1.5, label=lab)
ax.set(xlabel="time [UI]", ylabel="voltage [mV]", title="Example component: delay"); ax.legend(); plt.tight_layout()""",
    "md:## 5. Change H(f): from gain block to a CTLE that replaces the FFE\n\n"
    "A new kind reuses the example's transfer function and multiplies it by one zero and one pole, "
    "$H(f) = g\\,e^{-j2\\pi f\\tau}\\,(1 + jf/f_z)/(1 + jf/f_p)$: a continuous-time linear equalizer (CTLE) with DC gain $g$ "
    "and high-frequency boost $f_p/f_z$. It goes into the reference link **without** its FFE (`rxf → ctle → rx`). "
    "Without any equalizer the 100 m eye is closed (SER ≈ 0.10).",
    """def ctle(f, ctx):
    fz, fp = ctx.params.get("fz", 12e9), ctx.params.get("fp", 40e9)
    return mod.transfer(f, ctx) * (1 + 1j * f / fz) / (1 + 1j * f / fp)

example_ctle = lightstone.Component("example_ctle", inputs={"in": "electrical"}, outputs={"out": "electrical"},
                                    linear=True, domain="frequency", function=ctle)

h = lightstone.Graph.reference({"rx": {"ffe_taps": 0}}, components=[example_ctle]).run()
eq0 = h.receiver(); s0 = eq0.analyze()
h.insert("rx", "ctle", "example_ctle", fz=12e9, fp=40e9).run()
eq1 = h.receiver(); s1 = eq1.analyze()
print(f"no equalizer: SER {s0['ser']:.2e};  CTLE (fz 12 GHz, fp 40 GHz): SER {s1['ser']:.2e}")

f = np.linspace(0, 60e9, 600)
class Ctx:  # minimal stand-in for the simulation context, to plot H(f)
    params = {"fz": 12e9, "fp": 40e9}
fig, ax = plt.subplots(1, 3, figsize=(14, 3.6))
ax[0].plot(f / 1e9, 20 * np.log10(np.abs(ctle(f, Ctx)))); ax[0].set(xlabel="f [GHz]", ylabel="|H| [dB]", title="CTLE")
for k, (rx, s, lab) in enumerate(((eq0, s0, "no equalizer"), (eq1, s1, "CTLE"))):
    counts, meta = rx.eye(300, 200, analysis=s)
    ax[k + 1].imshow(np.log1p(counts), aspect="auto", cmap="inferno",
                     extent=[-1, 1, meta["v_min"] * 1e3, meta["v_max"] * 1e3])
    ax[k + 1].set(xlabel="time [UI]", ylabel="voltage [mV]", title=f"{lab}: SER {s['ser']:.1e}")
plt.tight_layout()""",
    "md:The CTLE opens the eye (SER from about 0.10 to about 2.5 × 10⁻⁴); the adaptive 5-tap FFE of the reference link "
    "reaches 1.9 × 10⁻⁷. Because both blocks are linear, the noise stays Gaussian and exact, and the SER is still "
    "semi-analytic. In both eyes the sampling point is at 0 UI. The dark diamonds at ±0.5 UI of the unequalized eye "
    "are gaps between traces that ISI has split apart, not the three PAM-4 eyes; the analyzer's SER is lowest at 0 UI. "
    "Try other $f_z$, $f_p$ with `h.set(\"ctle\", fz=..., fp=...).run()`.",
]

signal_tutorial = [
    "md:# Signal tutorial: what flows through a Lightstone graph\n\n"
    "A hands-on walk along the reference link (PRBS13Q → driver → VCSEL → connector → 100 m OM4 → PIN → TIA → "
    "Bessel–Thomson → FFE → receiver output). At each wire we look at what the signal contains, and we recompute the "
    "conversions electrical → optical (VCSEL) and optical → electrical (PIN, TIA) by hand from what the engine reports.\n\n"
    "Background and the full definitions: `docs/tutorial-signals-and-math.md` and `docs/signal-spec.md`.",
    """import numpy as np
import matplotlib.pyplot as plt
import lightstone

g = lightstone.Graph.reference().run()
ports = g.ports()
for n in g.nodes():
    p = ports[n]
    ins = ", ".join(f"{i['name']}: {i['type']}" for i in p["inputs"]) or "—"
    outs = ", ".join(f"{o['name']}: {o['type']}" for o in p["outputs"]) or "—"
    print(f"{n:6s} {p['kind']:16s} {'linear' if p['linear'] else 'nonlinear/sink':15s} in [{ins}]  out [{outs}]")""",
    "md:## 1. Three signal types, one time grid\n\n"
    "Every port has a type: **data** (symbols), **electrical** (a waveform tagged as drive, current or voltage) or "
    "**optical**. Wires only connect matching types. All signals share one **periodic** time grid: "
    "N = N_sym · sps samples at f_s = baud · sps, an integer number of pattern periods. Because everything is periodic, "
    "every filter is an exact circular convolution, i.e. a multiplication by H(f) on the FFT grid.",
    """s = g.settings
n_sym = 8191 * s["pattern_periods"]
N = n_sym * s["sps"]
fs = s["baud"] * s["sps"]
print(f"baud {s['baud']/1e9:.3f} GBd, {s['sps']} samples/UI → fs = {fs/1e9:.0f} GS/s")
print(f"N = {n_sym} symbols × {s['sps']} = {N} samples, record {N/fs*1e9:.1f} ns")
print(f"FFT bin spacing Δf = fs/N = {fs/N/1e6:.2f} MHz (the PRBS line spacing), Nyquist {fs/2/1e9:.0f} GHz")
t = np.arange(N) / fs                                    # time axis [s]
f = np.fft.fftfreq(N, 1 / fs)                            # FFT-order frequencies [Hz]
sps = s["sps"]
ui = t * s["baud"]                                       # time in UI""",
    "md:## 2. Data: the symbol stream\n\n"
    "The pattern source outputs symbols only, no waveform and no noise. PRBS13Q has 8191 PAM-4 symbols, "
    "each level appearing almost equally often.",
    """src = g.output("src")
sym = src["symbols"]
print(src["type"], "levels:", src["levels"], "first symbols:", sym[:20])
print("counts per level:", np.bincount(sym))""",
    "md:## 3. Electrical: the drive\n\n"
    "The driver turns symbols into a dimensionless drive waveform (quantity `drive`): each symbol holds its level "
    "for one UI, band-limited to the grid (the ripple at the edges is the Gibbs effect of an ideal NRZ spectrum). "
    "An electrical output always carries three arrays: `noiseless`, `signal` (noiseless + one noise realization) and "
    "`sigma` (the exact RMS noise vs. time). The driver adds no noise.",
    """drv = g.output("drv")
print(drv["quantity"], {k: drv[k].shape for k in ("noiseless", "signal", "sigma")}, "max σ:", drv["sigma"].max())
k = slice(0, 24 * sps)
fig, ax = plt.subplots(figsize=(11, 3))
ax.plot(ui[k], drv["noiseless"][k], label="drive (noiseless)")
ax.step(np.arange(24) + 1, sym[:24] / 3, where="pre", color="k", lw=0.8, alpha=0.6, label="symbol / 3")
ax.set(xlabel="time [UI]", ylabel="drive", title="Driver output: one level per symbol"); ax.legend(); plt.tight_layout()""",
    "md:## 4. Electrical → optical: the VCSEL\n\n"
    "The VCSEL maps the drive to four optical power levels P₀…P₃ (set by average power and extinction ratio) and "
    "filters it with its modulation response (resonance f_r, damping ζ). The result is an **optical power** waveform "
    "in W. The VCSEL also defines the source's RIN (relative intensity noise), wavelength and spectral width. The port list above shows it as nonlinear: it clips the output power at 0 W (a laser cannot emit negative power), so the engine treats it as the point where the optical signal is created, not as a filter.",
    """tx = g.report("vcsel")
P = np.array(tx["levels"])
print("levels P0…P3 [mW]:", np.round(P * 1e3, 4))
print(f"average {P.mean()*1e3:.4f} mW (= {10*np.log10(P.mean()/1e-3):.2f} dBm), ER {10*np.log10(P[3]/P[0]):.2f} dB, "
      f"OMA_outer {(P[3]-P[0])*1e3:.4f} mW, VCSEL bandwidth {tx['vcsel_bandwidth']/1e9:.1f} GHz")
p_tx = tx["power"]
fig, ax = plt.subplots(figsize=(11, 3))
ax.plot(ui[k], p_tx[k] * 1e3, label="P_tx(t)")
for p in P: ax.axhline(p * 1e3, color="k", lw=0.5, ls=":")
ax.set(xlabel="time [UI]", ylabel="power [mW]", title="VCSEL output: optical power (dotted: the four levels)"); ax.legend(); plt.tight_layout()""",
    "md:## 5. The optical wire is lazy\n\n"
    "Asking the engine for the VCSEL's optical output gives only a summary: average power and the number of **source "
    "contributions** (one per laser feeding the wire). Internally an optical signal is *source + transfer function*: "
    "the transmitter spectrum plus, per **mode bin** (one fiber mode or mode group), a power weight w_k, a delay τ_k and "
    "a chromatic broadening σ_k. No per-mode waveform is ever stored while every block is linear.",
    """for n in ("vcsel", "el0", "el1"):
    o = g.output(n)
    print(f"{n:6s} {o['type']}: average {o['average_power']*1e3:.4f} mW, {o['contributions']} contribution(s)")
fib = g.report("el1")
print(f"fiber: {fib['modes']} LP modes guided, V = {fib['v_number']:.1f}, launched fraction guided {fib['guided_fraction']:.5f}, "
      f"EMB {fib['emb_length']/1e9:.0f} MHz·km")""",
    "md:### Inside the mode bins\n\n"
    "The connector computes how much of the VCSEL's light lands in each mode (overlap integrals); the fiber gives each "
    "mode its delay. `lightstone.simulate()` runs the same reference link and exposes the per-mode data: "
    "`launched_power` (w_k), `relative_delay` (s/m) and `chromatic_slope` (s/m², the delay change per wavelength and length).",
    """r = lightstone.simulate()
M = r.modes
grp = np.array([m["group"] for m in M]); w = np.array([m["launched_power"] for m in M])
L = 100.0
tau = np.array([m["relative_delay"] for m in M]) * L              # relative delay after 100 m [s]
sig = np.abs([m["chromatic_slope"] for m in M]) * L * 0.4e-9      # broadening σ_k for 0.4 nm RMS source [s]
groups = np.unique(grp)
fig, ax = plt.subplots(1, 2, figsize=(12, 3.4))
ax[0].bar(groups, [w[grp == q].sum() for q in groups]); ax[0].set(xlabel="mode group", ylabel="launched power fraction", title="Launch: power per mode group")
lit = w > 1e-5   # modes that received light (the highest groups get almost none)
ax[1].scatter(tau[lit] * 1e12, w[lit], s=12); ax[1].set_yscale("log")
ax[1].set(xlabel="relative delay after 100 m [ps]", ylabel="w_k", title=f"Mode bins with light ({lit.sum()} of {len(w)}): weight vs. delay")
plt.tight_layout()""",
    "md:Most of the light sits in the low mode groups, within about 30 ps of each other after 100 m; that spread is "
    "what limits the fiber's bandwidth. The single point near −400 ps is LP₁,₉ in group 18, the highest group, close to "
    "cutoff: its delay is far off, but it carries only 2 × 10⁻⁵ of the power.",
    "md:### The fiber as a filter, recomputed by hand\n\n"
    "At the photodiode the mode bins add up as power, so the fiber is one transfer function on power:\n\n"
    "$$H_\\text{fiber}(f) = T\\sum_k w_k\\,e^{-j2\\pi f\\tau_k}\\,e^{-2\\pi^2 f^2\\sigma_k^2}$$\n\n"
    "with T the attenuation. Applying it to the VCSEL waveform with one FFT must give the detected power "
    "that the PIN reports.",
    """T = 10 ** (-3.5 * L / 1e3 / 10)          # OM4 preset: 3.5 dB/km
H = np.zeros(N, complex)
for wk, tk, sk in zip(w, tau, sig):
    H += wk * np.exp(-2j * np.pi * f * tk) * np.exp(-2 * np.pi**2 * f**2 * sk**2)
H *= T
mean_delay = np.sum(w * tau) / np.sum(w)                 # the engine removes the common delay
H *= np.exp(2j * np.pi * f * mean_delay)
p_hand = np.fft.ifft(np.fft.fft(p_tx) * H).real
p_det = g.report("pin")["p_det"]
print("max |hand − engine| / max P =", np.max(np.abs(p_hand - p_det)) / p_det.max())
fp = np.linspace(0, 60e9, 6001)
Hp = sum(wk * np.exp(-2j*np.pi*fp*tk) * np.exp(-2*np.pi**2*fp**2*sk**2) for wk, tk, sk in zip(w, tau, sig)) / w.sum()
print(f"fiber −3 dB bandwidth: {fp[np.argmax(np.abs(Hp) < 0.5)]/1e9:.1f} GHz (engine: {r.metrics['channel_bandwidth']/1e9:.1f} GHz)")
fig, ax = plt.subplots(1, 2, figsize=(12, 3.4))
ax[0].plot(fp / 1e9, 20 * np.log10(np.abs(Hp))); ax[0].set(xlabel="f [GHz]", ylabel="|H_fiber| [dB]", ylim=(-15, 1), title="100 m OM4 (normalized)")
ax[1].plot(ui[k], p_tx[k] * T * 1e3, label="P_tx · T (no dispersion)"); ax[1].plot(ui[k], p_det[k] * 1e3, label="P_det (engine)")
ax[1].plot(ui[k], p_hand[k] * 1e3, "--", label="by hand"); ax[1].set(xlabel="time [UI]", ylabel="power [mW]"); ax[1].legend(fontsize=8)
plt.tight_layout()""",
    "md:The hand calculation reproduces the engine's detected power to about 10⁻¹¹: the optical wire really is just "
    "the source spectrum times this H(f).\n\n"
    "## 6. Optical → electrical: PIN and TIA\n\n"
    "The PIN photodiode turns detected optical power into a **current**, I = R · P_det (responsivity R in A/W). "
    "The TIA turns current into a **voltage**, V = Z · I (transimpedance Z in Ω). Both are linear, so the quantity "
    "tag changes (`current`, then `voltage`) but the signal stays an exact filter of the source.",
    """R = g.params("pin")["responsivity"]; Z = g.params("tia")["transimpedance"]
i_pin = g.output("pin"); v_tia = g.output("tia")
print(f"PIN: quantity {i_pin['quantity']}, R = {R} A/W, max |I − R·P_det| = {np.max(np.abs(i_pin['noiseless'] - R*p_det)):.1e} A")
print(f"TIA: quantity {v_tia['quantity']}, Z = {Z:.0f} Ω, max |V − Z·I| = {np.max(np.abs(v_tia['noiseless'] - Z*i_pin['noiseless'])):.1e} V")
print(f"mean photocurrent {i_pin['noiseless'].mean()*1e3:.3f} mA → mean TIA output {v_tia['noiseless'].mean()*1e3:.1f} mV")""",
    "md:## 7. Noise is carried, not just added\n\n"
    "Each electrical signal carries a list of noise terms (RIN from the VCSEL, shot noise from the PIN, thermal noise "
    "from the TIA), each a Gaussian source with its strength and the filter it has passed since. From that the engine "
    "computes the **exact** σ(t). Shot noise and RIN grow with the optical power, so σ(t) is larger on the upper "
    "levels; on this link RIN dominates. The `signal` array is one realization; its sample spread matches σ.",
    """v = g.output("rxf")
y0, y, sg = v["noiseless"], v["signal"], v["sigma"]
print(f"gaussian_exact = {v['gaussian_exact']};  RMS of (signal − noiseless) {np.std(y - y0)*1e3:.3f} mV, "
      f"RMS σ(t) {np.sqrt(np.mean(sg**2))*1e3:.3f} mV")
fig, ax = plt.subplots(figsize=(11, 3.4))
ax.plot(ui[k], y[k] * 1e3, lw=0.6, alpha=0.6, label="signal (one realization)")
ax.plot(ui[k], y0[k] * 1e3, label="noiseless")
ax.fill_between(ui[k], (y0 - 3*sg)[k] * 1e3, (y0 + 3*sg)[k] * 1e3, alpha=0.2, label="± 3 σ(t)")
ax.set(xlabel="time [UI]", ylabel="voltage [mV]", title="After the receive filter (rxf)"); ax.legend(fontsize=8); plt.tight_layout()""",
    "md:### Which noise is where\n\n"
    "Switching the sources off one at a time shows each contribution. The terms are independent Gaussians, so their "
    "variances add exactly: σ²_total = σ²_RIN + σ²_shot + σ²_thermal.",
    """def sigma_rxf(rin=True, shot=True, thermal=True):
    h = lightstone.Graph.reference()
    h.set("vcsel", rin_enabled=rin).set("pin", shot_noise=shot).set("tia", thermal_noise=thermal).run()
    return h.output("rxf")["sigma"]

parts = {"RIN": sigma_rxf(True, False, False), "shot": sigma_rxf(False, True, False), "thermal": sigma_rxf(False, False, True)}
total = sg
print("RMS σ [mV]:", {kk: round(float(np.sqrt(np.mean(x**2))) * 1e3, 3) for kk, x in parts.items()}, "total", round(float(np.sqrt(np.mean(total**2)))*1e3, 3))
print("max |σ²_total − Σσ²_i| / max σ²_total =", np.max(np.abs(total**2 - sum(x**2 for x in parts.values()))) / np.max(total**2))
fig, ax = plt.subplots(figsize=(11, 3))
for kk, x in parts.items(): ax.plot(ui[k], x[k] * 1e3, label=kk)
ax.plot(ui[k], total[k] * 1e3, "k", lw=1.2, label="total")
ax.set(xlabel="time [UI]", ylabel="σ(t) [mV]", title="Noise per source after the receive filter (thermal is flat; RIN and shot follow the power)"); ax.legend(fontsize=8); plt.tight_layout()""",
    "md:## 8. In the frequency domain\n\n"
    "The electrical spectrum analyzer shows the same two parts as spectra: the signal PSD (a PRBS line spectrum, "
    "averaged over the resolution bandwidth) and the noise PSD from the noise terms. The optical spectrum analyzer "
    "shows the source line at its wavelength and width.",
    """h = lightstone.Graph.reference()
h.add("esa", "electrical_spectrum_analyzer").wire("rxf", "esa")
h.add("osa", "optical_spectrum_analyzer").wire("el1", "osa").run()
esa, osa = h.report("esa"), h.report("osa")
print(f"ESA: signal RMS {esa['signal_rms']*1e3:.1f} mV, noise RMS {esa['noise_rms']*1e3:.2f} mV, SNR {esa['snr_db']:.1f} dB, RBW {esa['rbw']/1e6:.0f} MHz")
fig, ax = plt.subplots(1, 2, figsize=(12, 3.4))
fr = np.asarray(esa["frequency"])
ax[0].plot(fr / 1e9, esa["signal_db"], label="signal"); ax[0].plot(fr / 1e9, esa["noise_db"], label="noise")
ax[0].set(xlabel="f [GHz]", ylabel="PSD [dB V²/Hz]", title="Electrical spectrum at rxf"); ax[0].legend()
pk = max(osa["power_dbm"])
ax[1].plot(np.asarray(osa["wavelength"]) * 1e9, osa["power_dbm"])
ax[1].set(xlabel="λ [nm]", ylabel="power per RBW [dBm]", xlim=(847, 853), ylim=(pk - 40, pk + 3), title="Optical spectrum at the fiber output (0.4 nm RMS line)")
plt.tight_layout()""",
    "md:## 9. Linear vs. nonlinear\n\n"
    "Everything so far was linear, so the engine only multiplied transfer functions and kept σ(t) exact "
    "(`gaussian_exact = True`). A level-dependent block breaks that. Clipping the TIA output cuts the noise tails: the "
    "engine then carries the noise as an explicit realization, `gaussian_exact` becomes False and the SER is counted "
    "(Monte Carlo) instead of computed.",
    """c = lightstone.Graph.reference()
c.set("tia", clip=[0.30, 0.60]).run()
o = c.output("tia")
print("TIA with clipping: gaussian_exact =", o["gaussian_exact"])
a = c.receiver().analyze()
print(f"SER {a['ser']:.2e} via {a['ser_method']}, resolution {a['ser_resolution']:.1e}")
print("warnings:", c.warnings()[:2])

# TIA output with clipping: one noisy realization and the noiseless waveform (first 1000 samples).
lo, hi = 0.30, 0.60
n_show = 1000
t_ns = t[:n_show] * 1e9                                   # time of each sample [ns], t = n / fs
fig, ax = plt.subplots(figsize=(12, 3.6))
ax.plot(t_ns, o["signal"][:n_show] * 1e3, color="C0", lw=0.8, alpha=0.75, label="signal (noisy)")
ax.plot(t_ns, o["noiseless"][:n_show] * 1e3, color="C1", lw=1.8, label="noiseless")
for y_, lab in ((hi, "clip high"), (lo, "clip low")):
    ax.axhline(y_ * 1e3, color="C3", ls="--", lw=1)
    ax.text(t_ns[-1], y_ * 1e3, f" {lab} {y_*1e3:.0f} mV", color="C3", va="center", ha="left", fontsize=8)
ax.set(xlabel=f"time [ns]  (sample n / fs, fs = {fs/1e9:.0f} GS/s)", ylabel="TIA output [mV]", xlim=(t_ns[0], t_ns[-1]),
       title=f"TIA output with clipping, samples 0…{n_show - 1} (gaussian_exact = False)")
# Second x-axis on top: the same time in unit intervals (1 UI = 1/baud = {sps} samples).
top = ax.secondary_xaxis("top", functions=(lambda x: x * 1e-9 * s["baud"], lambda u: u / s["baud"] * 1e9))
top.set_xlabel(f"time [UI]  (1 UI = {1e12/s['baud']:.2f} ps = {sps} samples)")
ax.grid(alpha=0.25)
ax.legend(loc="upper left", fontsize=8, framealpha=0.9)
plt.tight_layout()""",
    "md:### Caching follows the signal flow\n\n"
    "Each node's output is cached. Changing a parameter recomputes only that node and everything downstream.",
    """g.set("el1", length=50.0).run(); print("fiber length changed:", g.computed_last_run, "of", len(g.nodes()), "nodes recomputed")
g.set("tia", transimpedance=800.0).run(); print("TIA gain changed:   ", g.computed_last_run, "of", len(g.nodes()), "nodes recomputed")""",
    "md:## Summary\n\n"
    "| Wire | Type / quantity | What it holds |\n|---|---|---|\n"
    "| src → drv | data | 8191 PAM-4 symbols, no noise |\n"
    "| drv → vcsel | electrical, drive | band-limited NRZ drive (noiseless, signal, σ) |\n"
    "| vcsel → fiber → pin | optical | lazy: source spectrum + per-mode-bin weight, delay, broadening; RIN as a noise source |\n"
    "| pin → tia | electrical, current | I = R·P_det, plus shot noise |\n"
    "| tia → rxf → ffe → rx | electrical, voltage | V = Z·I, plus thermal noise, each filter applied to signal and noise |\n\n"
    "Electrical → optical happens in the VCSEL (drive → power levels → modulation response); optical → electrical in the "
    "PIN (power → current) and the TIA (current → voltage).",
]

for name, cells in [("validation_report", validation), ("sweeps", sweeps), ("custom_component", custom_component), ("signal_tutorial", signal_tutorial)]:
    nbf.write(nb(cells), HERE / f"{name}.ipynb")
    print("wrote", name)

"""Generate the figures of docs/model/ (textbook style, SVG).

Schematics are drawn here, plots are computed with the simulator, so the figures stay in
sync with the code. Run from the repository root after building the Python package:

    .venv/bin/python scripts/make_figures.py            # writes docs/model/img/*.svg
    .venv/bin/python scripts/make_figures.py --png DIR  # also PNG previews into DIR

Style: STIX (Times-like) serif fonts, italic math, black line art on a white background
(readable on light and dark pages alike). Text is converted to paths, so the SVGs render the
same everywhere. Output is deterministic (fixed SVG ids, no timestamps).
"""

from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from matplotlib.patches import Circle, FancyArrowPatch, Polygon, Rectangle  # noqa: E402

import lightstone  # noqa: E402

OUT = Path(__file__).resolve().parents[1] / "docs" / "model" / "img"

plt.rcParams.update(
    {
        "font.family": "STIXGeneral",
        "mathtext.fontset": "stix",
        "font.size": 11,
        "axes.linewidth": 0.8,
        "axes.edgecolor": "black",
        "xtick.direction": "in",
        "ytick.direction": "in",
        "xtick.top": True,
        "ytick.right": True,
        "svg.fonttype": "path",
        "svg.hashsalt": "lightstone",
        "figure.facecolor": "white",
        "savefig.facecolor": "white",
    }
)

INK = "black"
ACCENT = "#8b1a1a"  # textbook dark red for highlighting


def save(fig, name: str, png_dir: Path | None, title: str, desc: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / f"{name}.svg"
    fig.savefig(path, bbox_inches="tight", pad_inches=0.08, metadata={"Date": None, "Creator": None})
    # Accessible name and description (screen readers; docs/architecture.md, 5.3).
    svg = path.read_text()
    i = svg.index(">", svg.index("<svg")) + 1
    esc = lambda t: t.replace("&", "&amp;").replace("<", "&lt;")  # noqa: E731
    svg = svg[:i] + f'<title id="{name}-title">{esc(title)}</title><desc>{esc(desc)}</desc>' + svg[i:]
    svg = svg.replace("<svg ", f'<svg role="img" aria-labelledby="{name}-title" ', 1)
    path.write_text(svg)
    if png_dir is not None:
        png_dir.mkdir(parents=True, exist_ok=True)
        fig.savefig(png_dir / f"{name}.png", dpi=150, bbox_inches="tight", pad_inches=0.08)
    plt.close(fig)
    print("wrote", OUT / f"{name}.svg")


def arrow(ax, p, q, **kw):
    kw.setdefault("color", INK)
    kw.setdefault("lw", 1.0)
    ax.add_patch(FancyArrowPatch(p, q, arrowstyle="-|>", mutation_scale=10, shrinkA=0, shrinkB=0, **kw))


def line(ax, xs, ys, **kw):
    kw.setdefault("color", INK)
    kw.setdefault("lw", 1.0)
    ax.plot(xs, ys, solid_capstyle="butt", **kw)


# ---------------------------------------------------------------------------------------------
# FFE: tapped delay line
# ---------------------------------------------------------------------------------------------


def ffe_delay_line(png_dir):
    """Schematic of a 5-tap T-spaced FFE with main cursor m = 2."""
    n, m = 5, 2
    xs = [1.0 + 2.2 * j for j in range(n)]  # tap pick-off positions
    y_top, y_gain, y_sum = 3.0, 1.75, 0.55
    fig, ax = plt.subplots(figsize=(7.4, 3.6))
    ax.set_xlim(-0.6, xs[-1] + 1.9)
    ax.set_ylim(-0.75, 4.25)
    ax.set_aspect("equal")
    ax.axis("off")

    # Input and delay chain.
    arrow(ax, (-0.5, y_top), (xs[0] - 0.02, y_top))
    ax.text(-0.5, y_top - 0.15, r"input $x$", ha="left", va="top")
    w, h = 0.75, 0.6
    for j in range(n - 1):
        cx = 0.5 * (xs[j] + xs[j + 1])
        ax.add_patch(Rectangle((cx - w / 2, y_top - h / 2), w, h, fill=False, ec=INK, lw=1.0))
        ax.text(cx, y_top, r"$T$", ha="center", va="center")
        arrow(ax, (xs[j], y_top), (cx - w / 2, y_top))
        line(ax, [cx + w / 2, xs[j + 1]], [y_top, y_top])

    labels = [r"$x(t+2T)$", r"$x(t+T)$", r"$x(t)$", r"$x(t-T)$", r"$x(t-2T)$"]
    for j, x in enumerate(xs):
        col = ACCENT if j == m else INK
        ax.add_patch(Circle((x, y_top), 0.055, color=INK, zorder=3))
        ax.text(x, y_top + 0.38, labels[j], ha="center", va="bottom", fontsize=9.5, color=col)
        # Pick-off down to the gain triangle (pointing down).
        tri_top, tri_bot, half = y_gain + 0.38, y_gain - 0.32, 0.3
        arrow(ax, (x, y_top), (x, tri_top))
        ax.add_patch(Polygon([(x - half, tri_top), (x + half, tri_top), (x, tri_bot)], closed=True, fill=False, ec=col, lw=1.6 if j == m else 1.0))
        ax.text(x + half + 0.08, y_gain + 0.12, rf"$c_{j}$", ha="left", va="center", color=col, fontsize=12)
        arrow(ax, (x, tri_bot), (x, y_sum + 0.3))

    # Summer.
    x0, x1 = xs[0] - 0.55, xs[-1] + 0.55
    ax.add_patch(Rectangle((x0, y_sum - 0.3), x1 - x0, 0.6, fill=False, ec=INK, lw=1.0))
    ax.text(0.5 * (x0 + x1), y_sum, r"$\Sigma$", ha="center", va="center", fontsize=14)
    arrow(ax, (x1, y_sum), (x1 + 1.05, y_sum))
    ax.text(x1 + 1.1, y_sum, r"$y(t)$", ha="left", va="center")


    # Pre- and post-cursor braces.
    def brace(xa, xb, text, col=INK):
        y = -0.12
        line(ax, [xa, xa, xb, xb], [y + 0.1, y, y, y + 0.1], color=col, lw=0.8)
        ax.text(0.5 * (xa + xb), y - 0.1, text, ha="center", va="top", fontsize=9.5, color=col)

    brace(xs[0] - 0.35, xs[m - 1] + 0.35, "pre-cursor taps")
    brace(xs[m] - 0.35, xs[m] + 0.35, "main cursor", ACCENT)
    brace(xs[m + 1] - 0.35, xs[-1] + 0.35, "post-cursor taps")

    ax.text(
        0.5 * (x0 + x1),
        -0.72,
        r"$y(t)=\sum_{j=0}^{L-1} c_j\, x\left(t-(j-m)\,T\right),\qquad \sum_j c_j = 1$",
        ha="center",
        va="top",
        fontsize=11,
    )
    save(
        fig,
        "ffe-delay-line",
        png_dir,
        "Tapped delay line of a 5-tap FFE",
        "The input passes four delays of one symbol period T. Each of the five pick-offs is "
        "weighted by a tap c0 to c4 and the products are summed into the output. Tap c2 is the "
        "main cursor; c0 and c1 are pre-cursor taps, c3 and c4 post-cursor taps.",
    )


# ---------------------------------------------------------------------------------------------
# FFE: eye before and after equalization (computed)
# ---------------------------------------------------------------------------------------------


def ffe_eyes(png_dir):
    """Reference link at 100 m: eye without and with a 5-tap adaptive FFE, and the taps."""
    g = lightstone.Graph.reference()
    # The reference graph already has the 5-tap receiver FFE (`ffe`, rx.ffe_taps = 5).
    g.run()
    lo = min(g.output("rxf")["signal"].min(), g.output("ffe")["signal"].min())
    hi = max(g.output("rxf")["signal"].max(), g.output("ffe")["signal"].max())
    pad = 0.05 * (hi - lo)
    vr = [lo - pad, hi + pad]
    for name, src in (("eye0", "rxf"), ("eye1", "ffe")):
        g.add(name, "eye_diagram", width=500, height=360, v_range=vr).wire(src, name).wire("src", f"{name}.ref")
        g.add(f"ana_{name}", "pam4_analyzer").wire(src, f"ana_{name}").wire("src", f"ana_{name}.ref")
    g.run()
    taps = g.report("ffe")["taps"]

    fig = plt.figure(figsize=(8.6, 3.0))
    gs = fig.add_gridspec(1, 3, width_ratios=[1, 1, 0.62], wspace=0.32)
    for k, (name, title) in enumerate((("eye0", "(a) without FFE"), ("eye1", "(b) with 5-tap FFE"))):
        ax = fig.add_subplot(gs[0, k])
        e, a = g.report(name), g.report(f"ana_{name}")
        img = np.sqrt(e["counts"] / max(e["max_count"], 1))  # persistence-like contrast
        ax.imshow(img, cmap="Greys", aspect="auto", origin="upper", extent=[-1, 1, e["v_min"] * 1e3, e["v_max"] * 1e3], interpolation="bilinear")
        ax.set_xlabel(r"time relative to sampling instant [UI]")
        if k == 0:
            ax.set_ylabel(r"receiver output [mV]")
        ax.set_title(title, fontsize=11)
        ax.set_xticks([-1, -0.5, 0, 0.5, 1])
        ser = a["ser"]
        mant, ex = f"{ser:.1e}".split("e")
        ax.text(0.03, 0.97, rf"SER $= {mant}\times 10^{{{int(ex)}}}$", transform=ax.transAxes, ha="left", va="top", fontsize=9.5, bbox=dict(fc="white", ec="none", pad=1.5))
        print(f"{name}: SER {ser:.3e}, EH {[round(x * 1e3, 1) for x in a['eye_height']]} mV")

    ax = fig.add_subplot(gs[0, 2])
    j = np.arange(len(taps)) - g.report("ffe")["main"]
    ax.axhline(0, color=INK, lw=0.6)
    ax.vlines(j, 0, taps, color=INK, lw=1.2)
    ax.plot(j, taps, "o", mfc="white", mec=INK, ms=5)
    ax.plot([0], [taps[2]], "o", mfc=ACCENT, mec=ACCENT, ms=5)
    ax.set_xticks(j)
    ax.set_xlabel(r"tap position $j-m$ [UI]")
    ax.set_ylabel(r"tap weight $c_j$")
    ax.set_title("(c) adapted taps", fontsize=11)
    ax.set_ylim(-0.4, 1.55)
    print("taps", [round(t, 3) for t in taps])
    save(
        fig,
        "ffe-eyes",
        png_dir,
        "Eye diagram at 100 m without and with a 5-tap FFE, and the adapted taps",
        f"(a) Without equalization the eye is closed (SER {g.report('ana_eye0')['ser']:.1e}). "
        f"(b) With the adaptive FFE four separated levels appear (SER {g.report('ana_eye1')['ser']:.1e}). "
        f"(c) Tap weights {', '.join(f'{t:.2f}' for t in taps)}; the main tap is the largest, "
        "the neighbors are negative.",
    )


# ---------------------------------------------------------------------------------------------
# Shared schematic helpers
# ---------------------------------------------------------------------------------------------


def schematic(w, h, figsize):
    fig, ax = plt.subplots(figsize=figsize)
    ax.set_xlim(0, w)
    ax.set_ylim(0, h)
    ax.set_aspect("equal")
    ax.axis("off")
    return fig, ax


def box(ax, x, y, w, h, text="", fs=11, col=INK, lw=1.0, dashed=False):
    """Rectangle with lower-left corner (x, y) and centered text."""
    ax.add_patch(Rectangle((x, y), w, h, fill=False, ec=col, lw=lw, ls="--" if dashed else "-"))
    if text:
        ax.text(x + w / 2, y + h / 2, text, ha="center", va="center", fontsize=fs, color=col)


def caption(ax, x, y, text, **kw):
    kw.setdefault("ha", "center")
    kw.setdefault("va", "top")
    kw.setdefault("fontsize", 9.5)
    ax.text(x, y, text, **kw)


def sci(x: float, digits: int = 1) -> str:
    """Scientific notation for mathtext, e.g. 3.3 \times 10^{-2}."""
    mant, ex = f"{x:.{digits}e}".split("e")
    return rf"{mant}\times 10^{{{int(ex)}}}"


def panel_title(ax, text):
    ax.set_title(text, fontsize=11)


# ---------------------------------------------------------------------------------------------
# VCSEL
# ---------------------------------------------------------------------------------------------


def vcsel_levels_response(png_dir):
    """(a) Transmitted power over 24 symbols with the four levels; (b) |H_V(f)|."""
    r = lightstone.simulate({"rx.rin_noise": False})
    cfg = lightstone.default_config()
    lv = np.asarray(r.tx_levels) * 1e3
    sps, n_show, start = r.sps, 24, 40
    t = np.arange(n_show * sps) / sps
    p = r.tx_power[start * sps : (start + n_show) * sps] * 1e3

    fig, (a, b) = plt.subplots(1, 2, figsize=(8.6, 3.0), gridspec_kw={"width_ratios": [1.35, 1], "wspace": 0.3})
    for k, v in enumerate(lv):
        a.axhline(v, color=INK, lw=0.6, ls=(0, (4, 3)))
        a.text(n_show + 0.3, v, rf"$P_{k}$", va="center", ha="left", fontsize=10)
    a.plot(t, p, color=INK, lw=1.1)
    a.annotate("", xy=(1.0, lv[3]), xytext=(1.0, lv[0]), arrowprops=dict(arrowstyle="<|-|>", color=ACCENT, lw=0.9, mutation_scale=8))
    a.text(1.4, 0.5 * (lv[0] + lv[3]), r"$\mathrm{OMA}_\mathrm{outer}$", color=ACCENT, va="center", fontsize=10)
    a.set_xlim(0, n_show)
    a.set_ylim(lv[0] - 0.2, lv[3] + 0.2)
    a.set_xlabel("time [UI]")
    a.set_ylabel("optical power [mW]")
    panel_title(a, "(a) PAM-4 power levels and VCSEL response")

    fr, z = cfg["vcsel"]["resonance_freq"], cfg["vcsel"]["damping"]
    f = np.linspace(0, 60e9, 1200)
    h = 1 / (1 - (f / fr) ** 2 + 2j * z * f / fr)
    b.plot(f / 1e9, 20 * np.log10(np.abs(h)), color=INK, lw=1.1)
    f3 = r.metrics["vcsel_bandwidth"]
    b.axhline(-3, color=INK, lw=0.6, ls=(0, (4, 3)))
    b.axvline(f3 / 1e9, color=ACCENT, lw=0.8)
    b.text(f3 / 1e9 + 1, -12, rf"$f_\mathrm{{3dB}} = {f3 / 1e9:.1f}$ GHz", color=ACCENT, fontsize=10)
    b.text(2, -2.4, r"$-3$ dB", fontsize=9)
    b.set_xlim(0, 60)
    b.set_ylim(-20, 3)
    b.set_xlabel(r"frequency $f$ [GHz]")
    b.set_ylabel(r"$|H_V(f)|$ [dB]")
    panel_title(b, rf"(b) $f_r = {fr / 1e9:.0f}$ GHz, $\zeta = {z}$")
    # Consistency: the plotted response crosses −3 dB where the simulator says.
    i = np.argmax(20 * np.log10(np.abs(h)) < -3)
    assert abs(f[i] - f3) < 0.1e9, (f[i], f3)
    save(
        fig,
        "vcsel-levels-response",
        png_dir,
        "VCSEL power levels and modulation response",
        f"(a) Transmitted optical power over 24 symbols with the four levels P0 to P3 "
        f"({lv[0]:.2f} to {lv[3]:.2f} mW); the transitions are rounded and overshoot slightly "
        f"because of the VCSEL bandwidth. (b) Magnitude of the second-order response, "
        f"-3 dB at {f3 / 1e9:.1f} GHz.",
    )


# ---------------------------------------------------------------------------------------------
# OM4 fiber
# ---------------------------------------------------------------------------------------------


def multimode_fiber_profile(png_dir):
    """(a) Graded index profile n(r); (b) ray picture of low- and high-order modes."""
    m = lightstone.solve_modes()
    cfg = lightstone.default_config()["fiber"]
    a_um, alpha = cfg["core_radius"] * 1e6, cfg["profile"]["alpha"]
    n1, n2 = m["n_core"], m["n_clad"]
    delta = (n1**2 - n2**2) / (2 * n1**2)

    fig = plt.figure(figsize=(8.6, 3.0))
    gs = fig.add_gridspec(1, 2, width_ratios=[1, 1.7], wspace=0.25)
    ax = fig.add_subplot(gs[0, 0])
    r = np.linspace(-40, 40, 801)
    n = np.where(np.abs(r) < a_um, n1 * np.sqrt(1 - 2 * delta * (np.abs(r) / a_um) ** alpha), n2)
    ax.plot(r, n, color=INK, lw=1.1)
    for x in (-a_um, a_um):
        ax.axvline(x, color=INK, lw=0.5, ls=(0, (4, 3)))
    ax.annotate("", xy=(a_um, n2 - 0.0016), xytext=(0, n2 - 0.0016), arrowprops=dict(arrowstyle="<|-|>", lw=0.8, mutation_scale=8))
    ax.text(a_um / 2, n2 - 0.0013, r"$a$", ha="center", va="bottom")
    ax.text(0, n1 + 0.0007, r"$n_1$", ha="center", va="bottom")
    ax.text(33, n2 + 0.0007, r"$n_2$", ha="center", va="bottom")
    ax.text(0, n2 + 0.0035, "core", ha="center", fontsize=9.5)
    ax.text(-33, n2 + 0.0007, "cladding", ha="center", va="bottom", fontsize=9.5)
    ax.set_xlim(-40, 40)
    ax.set_ylim(n2 - 0.003, n1 + 0.0032)
    ax.set_xlabel(r"radius $r$ [µm]")
    ax.set_ylabel(r"refractive index $n(r)$")
    panel_title(ax, rf"(a) index profile, $\alpha = {alpha}$")

    ax = fig.add_subplot(gs[0, 1])
    ax.set_xlim(0, 10)
    ax.set_ylim(-1.45, 1.45)
    ax.axis("off")
    for y in (-1, 1):
        line(ax, [0, 10], [y, y], lw=1.0)
    line(ax, [0, 10], [0, 0], lw=0.5, ls=(0, (6, 3, 1, 3)))
    for y0, y1 in ((1, 1.3), (-1.3, -1)):
        ax.add_patch(Rectangle((0, y0), 10, y1 - y0, color="#e6e6e6", lw=0))
    z = np.linspace(0, 10, 600)
    # Ray paths in a parabolic core are sinusoids with (nearly) the same period: the outer,
    # longer path runs through lower index and arrives almost at the same time.
    ax.plot(z, 0.28 * np.sin(2 * np.pi * z / 3.2), color=INK, lw=1.1, label="low-order mode")
    ax.plot(z, 0.86 * np.sin(2 * np.pi * z / 3.2 + 0.6), color=ACCENT, lw=1.1, label="high-order mode")
    ax.text(10.05, 1.12, "cladding", fontsize=9, va="center")
    ax.text(10.05, 0.0, "axis", fontsize=9, va="center")
    arrow(ax, (8.4, 1.15), (9.6, 1.15), lw=0.8)
    ax.text(8.25, 1.15, r"$z$", ha="right", va="center", fontsize=10)
    ax.legend(loc="lower center", bbox_to_anchor=(0.5, -0.12), ncol=2, frameon=False, fontsize=9.5)
    panel_title(ax, "(b) graded index: longer paths run through lower index")
    save(
        fig,
        "multimode-fiber-profile",
        png_dir,
        "Graded-index profile of the OM4 fiber and ray picture",
        f"(a) Refractive index versus radius: highest on the axis (n1 = {n1:.4f}), falling almost "
        f"parabolically to the cladding value (n2 = {n2:.4f}) at the core radius a = {a_um:.0f} um. "
        "(b) A low-order and a high-order mode drawn as rays: both follow sinusoidal paths of "
        "nearly the same period, so the longer outer path is compensated by the lower index.",
    )


def _fiber_channel(m, powers, length, sigma_lambda):
    """Relative delays [s], RMS widths [s], weights of all modes after `length` (eq:fiber-power-propagation)."""
    tau = np.array([md["group_delay"] for md in m["modes"]]) * length
    sig = np.abs([md["chromatic_slope"] for md in m["modes"]]) * length * sigma_lambda
    w = np.asarray(powers) / np.sum(powers)
    tau = tau - np.sum(w * tau)
    return tau, sig, w


def _h(f, tau, sig, w):
    return np.abs(np.exp(-2j * np.pi * np.outer(f, tau) - 2 * np.pi**2 * np.outer(f**2, sig**2)) @ w)


def multimode_fiber_delays(png_dir):
    """(a) Relative mode delays at 100 m with launched power; (b) |H(f)| at 50/100/150 m."""
    m = lightstone.solve_modes()
    c = lightstone.couple()
    sl = lightstone.default_config()["vcsel"]["spectral_width_rms"]
    groups = np.array([md["group"] for md in m["modes"]])
    fig, (a, b) = plt.subplots(1, 2, figsize=(8.6, 3.1), gridspec_kw={"wspace": 0.3})
    tau, sig, w = _fiber_channel(m, c["power"], 100.0, sl)
    a.axhline(0, color=INK, lw=0.5, ls=(0, (4, 3)))
    lim = 20.0
    d = tau * 1e12
    on = np.abs(d) <= lim
    a.scatter(groups[on], d[on], s=6 + 500 * w[on], facecolors="none", edgecolors=INK, lw=0.8)
    # Modes near cutoff (highest groups) have much larger delays but carry almost no power.
    # They are drawn off scale on the side where they lie (late for an over-compensated
    # profile, early for an under-compensated one).
    off_groups = sorted(set(groups[~on]))
    late = d[~on].mean() > 0
    yoff = lim + 3.0 if late else -lim - 3.0
    a.scatter(off_groups, [yoff] * len(off_groups), marker="^" if late else "v", s=18, color=ACCENT)
    extreme = d.max() if late else d.min()
    a.text(off_groups[0] - 0.4, yoff, f"{(~on).sum()} high-order modes off scale\n(up to {extreme:+.0f} ps, {100 * w[~on].sum():.2f} % of the power)", ha="right", va="center", fontsize=8.5, color=ACCENT)
    lo_on, hi_on = d[on].min(), d[on].max()
    a.set_ylim(*((lo_on - 3.0, yoff + 3.0) if late else (yoff - 3.0, hi_on + 3.0)))
    a.set_xlabel(r"principal mode group $g = 2m + l - 1$")
    a.set_ylabel(r"relative delay $\tau_m$ at 100 m [ps]")
    panel_title(a, "(a) differential mode delay")
    a.text(0.03, 0.97 if not late else 0.03, r"marker area $\propto$ launched power", transform=a.transAxes, ha="left", va="top" if not late else "bottom", fontsize=9, style="italic")

    f = np.linspace(0, 80e9, 801)
    styles = {50.0: (0, (5, 2)), 100.0: "-", 150.0: (0, (1.5, 1.5))}
    for L, ls in styles.items():
        tau, sig, w = _fiber_channel(m, c["power"], L, sl)
        h = _h(f, tau, sig, w)
        b.plot(f / 1e9, h, color=INK, lw=1.1, ls=ls, label=f"{L:.0f} m")
        f3 = f[np.argmax(h < 0.5)]
        if L == 100.0:
            sim = lightstone.simulate({"fiber.length": 100.0, "rx.rin_noise": False}).metrics["channel_bandwidth"]
            assert abs(f3 - sim) < 0.2e9, (f3, sim)  # same channel as the simulator
    b.axhline(0.5, color=ACCENT, lw=0.7)
    b.text(1, 0.47, r"optical $-3$ dB", color=ACCENT, ha="left", va="top", fontsize=9)
    b.set_xlim(0, 80)
    b.set_ylim(0, 1.05)
    b.set_xlabel(r"frequency $f$ [GHz]")
    b.set_ylabel(r"$|H(f)/H(0)|$")
    b.legend(frameon=False, fontsize=9.5, loc="lower left")
    panel_title(b, "(b) fiber transfer function (default launch)")
    save(
        fig,
        "multimode-fiber-delays",
        png_dir,
        "Mode delays and transfer function of the OM4 fiber",
        "(a) Relative group delay of all 90 LP modes after 100 m versus their principal mode "
        "group, marker area proportional to the launched power: the excited modes lie within "
        "a few picoseconds; a few near-cutoff modes with much larger delay carry almost no "
        "power and are drawn off scale. (b) Magnitude of the fiber transfer function on optical power "
        "for 50, 100 and 150 m; the optical -3 dB bandwidth falls with length.",
    )


# ---------------------------------------------------------------------------------------------
# Connector / launch
# ---------------------------------------------------------------------------------------------


def connector_launch_geometry(png_dir):
    """(a) End face with offset beam spot; (b) side view with tilt."""
    fig, (a, b) = plt.subplots(1, 2, figsize=(8.0, 3.2), gridspec_kw={"width_ratios": [1, 1.25], "wspace": 0.1})
    for ax in (a, b):
        ax.set_aspect("equal")
        ax.axis("off")
    a.set_xlim(-40, 40)
    a.set_ylim(-36, 38)
    a.add_patch(Circle((0, 0), 31, color="#e6e6e6", lw=0))
    a.add_patch(Circle((0, 0), 31, fill=False, ec=INK, lw=0.8))
    a.add_patch(Circle((0, 0), 25, color="white", lw=0))
    a.add_patch(Circle((0, 0), 25, fill=False, ec=INK, lw=1.0))
    dx, w = 9.0, 4.0
    for k, alpha in ((2.2, 0.15), (1.5, 0.3), (1.0, 0.55)):
        a.add_patch(Circle((dx, 0), w * k, color=ACCENT, alpha=alpha, lw=0))
    a.add_patch(Circle((dx, 0), w, fill=False, ec=ACCENT, lw=0.9, ls=(0, (3, 2))))
    line(a, [-28, 28], [0, 0], lw=0.4, ls=(0, (6, 3, 1, 3)))
    line(a, [0, 0], [-28, 28], lw=0.4, ls=(0, (6, 3, 1, 3)))
    a.annotate("", xy=(dx, -11), xytext=(0, -11), arrowprops=dict(arrowstyle="<|-|>", lw=0.8, mutation_scale=8))
    a.text(dx / 2, -12.5, r"$\Delta x$", ha="center", va="top")
    ang = np.deg2rad(110)
    line(a, [0, 25 * np.cos(ang)], [0, 25 * np.sin(ang)], lw=0.7)
    a.text(12.5 * np.cos(ang) - 2.2, 12.5 * np.sin(ang), r"$a$", fontsize=10, ha="right")
    a.text(dx + w + 0.8, 3.2, r"$w$", color=ACCENT, fontsize=10)
    a.text(0, 33, "cladding", ha="center", fontsize=9)
    a.text(-16, 9, "core", ha="center", fontsize=9)
    panel_title(a, "(a) end face: beam spot offset")

    b.set_xlim(-26, 40)
    b.set_ylim(-28, 30)
    b.add_patch(Rectangle((0, 25), 40, 6, color="#e6e6e6", lw=0))
    b.add_patch(Rectangle((0, -31), 40, 6, color="#e6e6e6", lw=0))
    line(b, [0, 40], [25, 25])
    line(b, [0, 40], [-25, -25])
    line(b, [0, 0], [-27, 27], lw=1.2)
    line(b, [-26, 40], [0, 0], lw=0.4, ls=(0, (6, 3, 1, 3)))
    th = np.deg2rad(14)
    y0 = 9.0  # hit point of the beam axis on the end face (offset)
    xl = -22.0
    for off in (-4.0, 0.0, 4.0):
        # Parallel rays rising toward the face at angle θ; the center ray hits (0, Δx).
        line(b, [xl, 0], [y0 + off + np.tan(th) * xl, y0 + off], color=ACCENT, lw=1.1 if off == 0 else 0.6)
    line(b, [xl, 0], [y0, y0], lw=0.5, ls=(0, (3, 2)))
    b.add_patch(matplotlib.patches.Arc((0, y0), 26, 26, theta1=180, theta2=194, lw=0.8))
    b.text(-14.5, y0 - 1.4, r"$\theta$", fontsize=10, ha="right", va="center")
    b.annotate("", xy=(2.5, y0), xytext=(2.5, 0), arrowprops=dict(arrowstyle="<|-|>", lw=0.8, mutation_scale=8))
    b.text(3.5, y0 / 2, r"$\Delta x$", ha="left", va="center")
    b.text(-24, y0 + 6.5, "VCSEL beam", color=ACCENT, fontsize=9.5)
    b.text(20, 14, "core", ha="center", fontsize=9)
    b.text(1.5, -22, "fiber end face", fontsize=9)
    panel_title(b, "(b) side view: offset and tilt")
    save(
        fig,
        "connector-launch-geometry",
        png_dir,
        "Launch geometry: beam spot offset and tilt",
        "(a) Fiber end face with cladding, core of radius a and the VCSEL beam spot of radius w, "
        "shifted from the core center by the offset delta x. (b) Side view: the beam hits the "
        "end face at offset delta x and tilted by the angle theta against the fiber axis.",
    )


def _group_powers(c, m):
    g = np.array([md["group"] for md in m["modes"]])
    gmax = g.max()
    p = np.array([c["power"][g == k].sum() for k in range(1, gmax + 1)])
    return np.arange(1, gmax + 1), p


def connector_launch_groups(png_dir):
    """Launched power per mode group for three offsets (computed)."""
    m = lightstone.solve_modes()
    fig, ax = plt.subplots(figsize=(6.4, 3.0))
    offs = (0.0, 5e-6, 10e-6)
    width = 0.27
    hatches = (None, "////", None)
    faces = ("white", "white", INK)
    emb = []
    for k, (off, hat, fc) in enumerate(zip(offs, hatches, faces)):
        c = lightstone.couple(launch={"offset": off})
        g, p = _group_powers(c, m)
        ax.bar(g + (k - 1) * width, p, width, facecolor=fc, edgecolor=INK, hatch=hat, lw=0.7, label=rf"$\Delta x = {off * 1e6:.0f}$ µm")
        emb.append(c["emb_length"])
    ax.set_xlabel(r"principal mode group $g$")
    ax.set_ylabel("launched power fraction")
    ax.set_xlim(0.4, 18.6)
    ax.set_xticks(range(1, 19))
    ax.legend(frameon=False, fontsize=9.5)
    ax.set_title("Launched power per mode group (VCSEL, 4 LG mode groups, $w = 4$ µm)", fontsize=11)
    save(
        fig,
        "connector-launch-groups",
        png_dir,
        "Launched power per mode group for three connector offsets",
        "Bar chart of the launched power fraction in each principal mode group for offsets of "
        "0, 5 and 10 um. A centered beam excites mainly the low groups; with growing offset the "
        f"power moves to higher groups (EMB {emb[0] / 1e9:.0f}, {emb[1] / 1e9:.0f} and "
        f"{emb[2] / 1e9:.0f} MHz km).",
    )


def attenuator_modes(png_dir):
    """Mode-independent loss: the modal power distribution keeps its shape."""
    m = lightstone.solve_modes()
    g, p = _group_powers(lightstone.couple(launch={"offset": 5e-6}), m)
    t = 10 ** (-3 / 10)
    fig, ax = plt.subplots(figsize=(6.0, 2.8))
    ax.bar(g - 0.19, p, 0.38, facecolor="white", edgecolor=INK, lw=0.7, label="before")
    ax.bar(g + 0.19, t * p, 0.38, facecolor=INK, edgecolor=INK, lw=0.7, label="after 3 dB")
    ax.set_xlabel(r"principal mode group $g$")
    ax.set_ylabel("power fraction")
    ax.set_xlim(0.4, 18.6)
    ax.set_xticks(range(1, 19))
    ax.legend(frameon=False, fontsize=9.5)
    ax.set_title(r"Attenuator: every mode bin scaled by $T = 10^{-\mathrm{loss}/10}$", fontsize=11)
    save(
        fig,
        "attenuator-modes",
        png_dir,
        "Effect of a 3 dB attenuator on the mode-group power distribution",
        "Bars of the power per mode group before and after a 3 dB attenuator (5 um offset "
        "launch): every group is halved, so the shape of the distribution and therefore the "
        "modal bandwidth are unchanged.",
    )


# ---------------------------------------------------------------------------------------------
# PIN / TIA
# ---------------------------------------------------------------------------------------------


def _noise_source(ax, x, y, label, col=INK):
    """Current-noise source symbol (circle with arrow) feeding a summing node above it."""
    ax.add_patch(Circle((x, y), 0.32, fill=False, ec=col, lw=1.0))
    arrow(ax, (x, y - 0.2), (x, y + 0.2), color=col, lw=0.9)
    ax.text(x, y - 0.45, label, ha="center", va="top", fontsize=10, color=col)


def pin_tia_chain(png_dir):
    fig, ax = schematic(14.2, 4.3, (8.8, 2.7))
    yc = 2.7
    # Optical input with RIN.
    arrow(ax, (0.1, yc), (1.5, yc), color=ACCENT, lw=1.2)
    ax.text(0.1, yc + 0.25, r"$P(t)$", va="bottom", color=ACCENT)
    ax.text(0.1, yc - 0.25, "incl. RIN", va="top", fontsize=9, color=ACCENT, style="italic")
    # Photodiode symbol.
    tri = Polygon([(1.6, yc + 0.45), (1.6, yc - 0.45), (2.35, yc)], closed=True, fill=False, ec=INK, lw=1.0)
    ax.add_patch(tri)
    line(ax, [2.35, 2.35], [yc - 0.45, yc + 0.45])
    ax.text(1.95, yc + 0.6, "PIN", ha="center", va="bottom", fontsize=9.5)
    ax.text(1.95, yc - 0.6, r"$I = R\,P$", ha="center", va="top", fontsize=10)
    line(ax, [2.35, 3.6], [yc, yc])
    # Summing nodes: shot and thermal noise.
    for x, lab in ((3.9, r"$2qI$"), (5.4, r"$i_n^2$")):
        ax.add_patch(Circle((x, yc), 0.28, fill=False, ec=INK, lw=1.0))
        ax.text(x, yc, "+", ha="center", va="center", fontsize=10)
        _noise_source(ax, x, yc - 1.35, lab)
        arrow(ax, (x, yc - 1.03), (x, yc - 0.28))
    line(ax, [4.18, 5.12], [yc, yc])
    ax.text(3.9, yc + 0.4, "shot", ha="center", va="bottom", fontsize=9)
    ax.text(5.4, yc + 0.4, "thermal", ha="center", va="bottom", fontsize=9)
    # TIA.
    arrow(ax, (5.68, yc), (6.6, yc))
    ax.add_patch(Polygon([(6.6, yc + 0.7), (6.6, yc - 0.7), (7.8, yc)], closed=True, fill=False, ec=INK, lw=1.0))
    ax.text(6.95, yc, r"$-$", ha="center", va="center", fontsize=10)
    line(ax, [6.3, 6.3, 6.85], [yc, yc + 1.15, yc + 1.15])
    box(ax, 6.85, yc + 0.95, 0.7, 0.4)
    ax.text(7.2, yc + 1.5, r"$Z$", ha="center", va="bottom")
    line(ax, [7.55, 8.1, 8.1], [yc + 1.15, yc + 1.15, yc])
    ax.text(7.2, yc - 0.85, "TIA", ha="center", va="top", fontsize=9.5)
    # Bessel-Thomson filter.
    arrow(ax, (7.8, yc), (8.9, yc))
    box(ax, 8.9, yc - 0.55, 2.2, 1.1, r"$H_\mathrm{BT}(f)$")
    ax.text(10.0, yc - 0.75, "4th-order Bessel–Thomson", ha="center", va="top", fontsize=9)
    arrow(ax, (11.1, yc), (12.6, yc))
    ax.text(12.7, yc, r"$u(t),\ \sigma(t)$", va="center")
    save(
        fig,
        "pin-tia-chain",
        png_dir,
        "Receiver chain with its noise sources",
        "Optical power P(t), which already carries the transmitter RIN, enters the PIN "
        "photodiode and becomes the current I = R P. Shot noise 2qI and the TIA's thermal "
        "noise in squared are added as current noise. The transimpedance amplifier with "
        "feedback Z converts the current to a voltage, and the Bessel-Thomson filter shapes "
        "signal and noise into the output u(t) with noise sigma(t).",
    )


def pin_tia_filter(png_dir):
    r = lightstone.simulate({"rx.rin_noise": False})
    baud = r.baud
    f3 = 0.5 * baud
    w3 = 2.1139
    f = np.linspace(0, 100e9, 2001)
    s = 1j * w3 * f / f3
    h = 105 / (s**4 + 10 * s**3 + 45 * s**2 + 105 * s + 105)
    bn = r.metrics["rx_noise_bandwidth"]
    fig, ax = plt.subplots(figsize=(6.0, 2.9))
    ax.add_patch(Rectangle((0, 0), bn / 1e9, 1, facecolor="#e6e6e6", edgecolor="none"))
    ax.plot(f / 1e9, np.abs(h) ** 2, color=INK, lw=1.1)
    ax.axvline(f3 / 1e9, color=ACCENT, lw=0.8)
    ax.text(f3 / 1e9 + 1.5, 0.75, rf"$f_\mathrm{{3dB}} = {f3 / 1e9:.1f}$ GHz", color=ACCENT, fontsize=10)
    ax.text(f3 / 1e9 - 1.2, 0.08, rf"$B_n = {bn / 1e9:.1f}$ GHz", ha="right", fontsize=10)
    ax.set_xlim(0, 100)
    ax.set_ylim(0, 1.05)
    ax.set_xlabel(r"frequency $f$ [GHz]")
    ax.set_ylabel(r"$|H_\mathrm{BT}(f)|^2$")
    ax.set_title(r"Receive filter and its noise bandwidth $B_n = \int_0^\infty |H_\mathrm{BT}|^2\,df$", fontsize=11)
    # The shaded rectangle has the same area as the curve.
    area = np.trapezoid(np.abs(h) ** 2, f)
    assert abs(area - bn) / bn < 0.01, (area, bn)
    save(
        fig,
        "pin-tia-filter",
        png_dir,
        "Bessel-Thomson receive filter and noise bandwidth",
        f"Squared magnitude of the 4th-order Bessel-Thomson filter with -3 dB at {f3 / 1e9:.1f} GHz. "
        f"A shaded rectangle of height 1 and width B_n = {bn / 1e9:.1f} GHz has the same area: "
        "white noise passes the filter as if it had this ideal bandwidth.",
    )


# ---------------------------------------------------------------------------------------------
# PAM-4 analysis and TDECQ
# ---------------------------------------------------------------------------------------------


def _eye_axes(ax, e, scale, unit):
    img = np.sqrt(e["counts"] / max(e["max_count"], 1))
    ax.imshow(img, cmap="Greys", aspect="auto", origin="upper", extent=[-1, 1, e["v_min"] * scale, e["v_max"] * scale], interpolation="bilinear")
    ax.set_xticks([-1, -0.5, 0, 0.5, 1])
    ax.set_xlabel("time relative to sampling instant [UI]")
    ax.set_ylabel(unit)


def pam4_eye_annotated(png_dir):
    """Eye at 50 m (after a 5-tap FFE, so the eye is clearly open) with thresholds, eye
    heights and widths."""
    g = lightstone.Graph.reference()
    g.set("el1", length=50.0)
    # The reference graph already has the 5-tap receiver FFE (`ffe`, rx.ffe_taps = 5).
    g.add("ana", "pam4_analyzer").wire("ffe", "ana").wire("src", "ana.ref")
    g.add("eye", "eye_diagram", width=500, height=380).wire("ffe", "eye").wire("src", "eye.ref")
    g.run()
    a, e = g.report("ana"), g.report("eye")
    fig, ax = plt.subplots(figsize=(6.4, 3.8))
    _eye_axes(ax, e, 1e3, "receiver output [mV]")
    th = np.array(a["thresholds"]) * 1e3
    mu = np.array(a["level_mean"]) * 1e3
    for k, v in enumerate(th):
        ax.axhline(v, color=ACCENT, lw=0.7, ls=(0, (5, 3)))
        ax.text(1.02, v, rf"$\theta_{k}$", color=ACCENT, va="center", transform=ax.get_yaxis_transform(), fontsize=10)
        eh = a["eye_height"][k] * 1e3
        ax.annotate("", xy=(0, v + eh / 2), xytext=(0, v - eh / 2), arrowprops=dict(arrowstyle="<|-|>", color=INK, lw=0.9, mutation_scale=7))
        ax.text(0.03, v + eh / 4, rf"EH$_{k}$", fontsize=9, va="center")
        ew = a["eye_width"][k]
        ax.annotate("", xy=(ew / 2, v), xytext=(-ew / 2, v), arrowprops=dict(arrowstyle="<|-|>", color=INK, lw=0.6, mutation_scale=6))
        ax.text(-ew / 2 - 0.02, v, rf"EW$_{k}$", fontsize=8.5, ha="right", va="center", bbox=dict(fc="white", ec="none", pad=0.8))
    for k, v in enumerate(mu):
        ax.text(-0.75, v, rf"$\mu_{k}$", va="center", ha="center", fontsize=10, bbox=dict(fc="white", ec="none", pad=1.0))
    ax.set_title(rf"PAM-4 eye at 50 m after a 5-tap FFE: SER $= {sci(a['ser'])}$", fontsize=11)
    save(
        fig,
        "pam4-eye-annotated",
        png_dir,
        "PAM-4 eye diagram with thresholds, eye heights and eye widths",
        "Eye diagram of the receiver output at 50 m after a 5-tap FFE, over two unit intervals around the "
        "sampling instant. Four levels mu0 to mu3, three dashed decision thresholds theta0 to "
        "theta2 halfway between them, vertical arrows for the eye heights and horizontal "
        f"arrows for the eye widths at each threshold. SER {a['ser']:.1e}.",
    )


def pam4_ser_tails(png_dir):
    """Two neighboring levels with Gaussian noise; the tails beyond the threshold are errors."""
    fig, ax = plt.subplots(figsize=(6.0, 2.8))
    x = np.linspace(-3.2, 3.2, 1200)
    mu0, mu1, s = -1.0, 1.0, 0.5
    pdf = lambda m: np.exp(-((x - m) ** 2) / (2 * s**2)) / (np.sqrt(2 * np.pi) * s)  # noqa: E731
    ax.plot(x, pdf(mu0), color=INK, lw=1.1)
    ax.plot(x, pdf(mu1), color=INK, lw=1.1)
    ax.fill_between(x, 0, pdf(mu0), where=x > 0, color=ACCENT, alpha=0.55, lw=0)
    ax.fill_between(x, 0, pdf(mu1), where=x < 0, color=ACCENT, alpha=0.25, lw=0)
    ax.axvline(0, color=ACCENT, lw=0.8, ls=(0, (5, 3)))
    ax.text(0.05, 0.86, r"$\theta_k$", color=ACCENT, fontsize=11)
    for m, lab in ((mu0, r"$\mu_k$"), (mu1, r"$\mu_{k+1}$")):
        line(ax, [m, m], [0, pdf(m).max()], lw=0.5, ls=(0, (2, 2)))
        ax.text(m, -0.06, lab, ha="center", va="top", fontsize=11)
    ax.annotate("", xy=(0, 0.6), xytext=(mu0, 0.6), arrowprops=dict(arrowstyle="<|-|>", lw=0.8, mutation_scale=8))
    ax.text(mu0 / 2, 0.63, r"$d/2$", ha="center", va="bottom", fontsize=10)
    ax.text(-3.1, 0.9, "shaded: error probability", color=ACCENT, fontsize=9.5, va="top")
    ax.text(-3.1, 0.8, r"$Q\!\left(\dfrac{\theta_k - \mu_k}{\sigma}\right)$", color=ACCENT, fontsize=11, va="top")
    ax.set_xlim(-3.2, 3.2)
    ax.set_ylim(0, 0.95)
    ax.set_xticks([])
    ax.set_yticks([])
    ax.spines[["top", "right"]].set_visible(False)
    ax.tick_params(top=False, right=False)
    ax.set_xlabel("sampled value", labelpad=18)
    ax.set_ylabel("probability density")
    ax.set_title("Symbol errors: Gaussian tails beyond the threshold", fontsize=11)
    save(
        fig,
        "pam4-ser-tails",
        png_dir,
        "Gaussian noise distributions of two neighboring PAM-4 levels",
        "Two Gaussian probability densities centered at the levels mu k and mu k+1, a dashed "
        "threshold theta k halfway between them (distance d/2 to each level), and the shaded "
        "tails beyond the threshold: the probability of a symbol error, Q((theta - mu)/sigma).",
    )


def tdecq_eye(png_dir):
    """Reference-equalized eye with the two TDECQ time slices and fixed thresholds."""
    r = lightstone.simulate()
    t = r.tdecq()
    sps = r.sps
    x = r.reference_optical
    y = sum(c * np.roll(x, (j - 2) * sps) for j, c in enumerate(t["taps"]))  # eq:ffe, main tap 2
    counts, meta = lightstone.eye_density(y, sps, t["delay"], t["phase"], 500, 380)
    e = {"counts": counts, "max_count": meta["max_count"], "v_min": meta["v_min"], "v_max": meta["v_max"]}
    fig, ax = plt.subplots(figsize=(6.4, 3.8))
    _eye_axes(ax, e, 1e3, "equalized optical power [mW]")
    for side in (-0.05, 0.05):
        ax.axvspan(side - 0.02, side + 0.02, color=ACCENT, alpha=0.22, lw=0)
    ax.text(0, 0.05, r"time slices at $\pm 0.05$ UI", transform=ax.get_xaxis_transform(), ha="center", va="bottom", fontsize=9, color=ACCENT, bbox=dict(fc="white", ec="none", pad=1.2))
    labels = [r"$P_\mathrm{ave} - \mathrm{OMA}/3$", r"$P_\mathrm{ave}$", r"$P_\mathrm{ave} + \mathrm{OMA}/3$"]
    for v, lab in zip(t["thresholds"], labels):
        ax.axhline(v * 1e3, color=ACCENT, lw=0.7, ls=(0, (5, 3)))
        ax.text(1.02, v * 1e3, lab, color=ACCENT, va="center", fontsize=9, transform=ax.get_yaxis_transform())
    ax.set_title(rf"After the 5-tap reference FFE (100 m): TDECQ $= {t['tdecq_db']:.2f}$ dB", fontsize=11)
    save(
        fig,
        "tdecq-eye",
        png_dir,
        "TDECQ measurement on the reference-equalized eye",
        f"Eye of the optical waveform after the reference receiver and the 5-tap reference "
        f"equalizer at 100 m. Two shaded time slices 0.04 UI wide at -0.05 and +0.05 UI around "
        f"the eye center, and three dashed fixed thresholds at P_ave and P_ave plus or minus "
        f"OMA/3. TDECQ {t['tdecq_db']:.2f} dB.",
    )


# ---------------------------------------------------------------------------------------------
# Splitter, combiner, probe, power meter, receiver output
# ---------------------------------------------------------------------------------------------


def _fiber(ax, xa, xb, y, col=ACCENT, lw=1.3):
    line(ax, [xa, xb], [y, y], color=col, lw=lw)


def splitter_combiner(png_dir):
    fig, ax = schematic(16.8, 4.6, (9.8, 2.8))
    # (a) Splitter.
    _fiber(ax, 0.2, 2.0, 2.4)
    ax.text(0.2, 2.65, r"$P_\mathrm{in}$", color=ACCENT)
    ax.add_patch(Polygon([(2.0, 1.7), (2.0, 3.1), (3.6, 3.5), (3.6, 1.3)], closed=True, fill=False, ec=INK, lw=1.0))
    ax.text(2.8, 2.4, "1×2", ha="center", va="center", fontsize=9.5)
    for y, lab in ((3.2, r"$r_1\,10^{-\alpha_\mathrm{ex}/10}\,P_\mathrm{in}$"), (1.6, r"$r_2\,10^{-\alpha_\mathrm{ex}/10}\,P_\mathrm{in}$")):
        _fiber(ax, 3.6, 5.0, y)
        arrow(ax, (4.6, y), (5.05, y), color=ACCENT, lw=1.0)
        ax.text(5.15, y, lab, va="center", fontsize=10)
    caption(ax, 2.8, 0.75, "(a) splitter: power divided by the ratios $r_i$", fontsize=10)
    # (b) Combiner.
    x0 = 11.2
    for y, lab in ((3.4, "VCSEL 1"), (1.4, "VCSEL 2")):
        box(ax, x0 - 1.8, y - 0.3, 1.4, 0.6, lab, fs=9)
        _fiber(ax, x0 - 0.4, x0 + 0.6, y)
    ax.add_patch(Polygon([(x0 + 0.6, 3.8), (x0 + 0.6, 1.0), (x0 + 2.1, 2.0), (x0 + 2.1, 2.8)], closed=True, fill=False, ec=INK, lw=1.0))
    _fiber(ax, x0 + 2.1, x0 + 3.3, 2.4)
    tri = Polygon([(x0 + 3.3, 2.85), (x0 + 3.3, 1.95), (x0 + 3.95, 2.4)], closed=True, fill=False, ec=INK, lw=1.0)
    ax.add_patch(tri)
    line(ax, [x0 + 3.95, x0 + 3.95], [1.95, 2.85])
    ax.text(x0 + 4.15, 2.4, r"$P_1 + P_2$", va="center", fontsize=10)
    caption(ax, x0 + 1.4, 0.75, "(b) combiner: sources add as powers", fontsize=10)
    save(
        fig,
        "splitter-combiner",
        png_dir,
        "Splitter and combiner",
        "(a) A 1x2 splitter divides the input power by the ratios r1 and r2, reduced by the "
        "excess loss. (b) A combiner joins the light of two VCSELs into one fiber; the sources "
        "are mutually incoherent, so the detector sees the sum of their powers.",
    )


def probe_power_meter(png_dir):
    fig, ax = schematic(13.0, 4.0, (8.0, 2.5))
    _fiber(ax, 0.3, 4.2, 2.8)
    ax.text(0.3, 3.05, r"$P_\mathrm{in}$", color=ACCENT)
    box(ax, 4.2, 2.3, 1.6, 1.0, "probe", fs=10)
    _fiber(ax, 5.8, 9.4, 2.8)
    arrow(ax, (9.0, 2.8), (9.5, 2.8), color=ACCENT, lw=1.0)
    ax.text(9.6, 2.8, r"out: $(1-r)\,10^{-\alpha_\mathrm{IL}/10}\,P_\mathrm{in}$", va="center", fontsize=10)
    line(ax, [5.0, 5.0], [2.3, 1.2], color=ACCENT, lw=1.3)
    _fiber(ax, 5.0, 6.6, 1.2)
    ax.text(5.15, 1.75, r"tap: $r\,P_\mathrm{in}$", fontsize=10, va="center")
    ax.add_patch(Circle((7.1, 1.2), 0.5, fill=False, ec=INK, lw=1.0))
    ax.text(7.1, 1.2, r"$\bar P$", ha="center", va="center", fontsize=10)
    ax.text(7.8, 1.2, "power meter", va="center", fontsize=9.5)
    caption(ax, 6.5, 0.45, r"ideal probe: $r = 0$, $\alpha_\mathrm{IL} = 0$, and the tap sees the full signal (an ideal observer)", fontsize=9.5, style="italic")
    save(
        fig,
        "probe-power-meter",
        png_dir,
        "Inline probe with tap and power meter",
        "Optical power passes an inline probe. The through output keeps (1 - r) of the power "
        "times the insertion loss; the tap branch carries r times the input power to a power "
        "meter, which reports the average power. An ideal probe removes nothing.",
    )


def receiver_output_nodes(png_dir):
    fig, ax = schematic(14.6, 5.4, (8.8, 3.2))
    yc = 3.4
    box(ax, 0.2, yc - 0.4, 1.6, 0.8, "src", fs=10)
    box(ax, 2.6, 4.45, 1.6, 0.8, "fiber", fs=10)
    box(ax, 2.6, yc - 0.4, 1.6, 0.8, "rxf", fs=10)
    ax.text(2.2, yc, "…", ha="center", va="center")
    ax.text(3.4, 5.4, "optical", ha="center", va="bottom", fontsize=8.5, color=ACCENT, style="italic")
    box(ax, 5.2, yc - 0.4, 1.6, 0.8, "ffe", fs=10, dashed=True)
    arrow(ax, (4.2, yc), (5.2, yc))
    nodes = [("receiver_output", 4.5), ("pam4_analyzer", 3.4), ("eye_diagram", 2.3)]
    for name, y in nodes:
        box(ax, 8.4, y - 0.38, 2.9, 0.76, name, fs=9.5)
        arrow(ax, (6.8, yc), (8.4, y))
    box(ax, 8.4, 0.62, 2.9, 0.76, "tdecq", fs=9.5)
    arrow(ax, (4.2, 4.85), (7.3, 4.85), color=ACCENT)
    line(ax, [7.3, 7.3], [4.85, 1.0], color=ACCENT, lw=1.0, ls=(0, (4, 2)))
    arrow(ax, (7.3, 1.0), (8.4, 1.0), color=ACCENT)
    # ref wires from src.
    line(ax, [1.0, 1.0, 12.1, 12.1], [yc - 0.4, 0.2, 0.2, 3.4], lw=0.7, ls=(0, (2, 2)))
    line(ax, [6.0, 6.0], [0.2, 1.6], lw=0.7, ls=(0, (2, 2)))
    arrow(ax, (6.0, 1.6), (6.0, yc - 0.4), lw=0.7)
    for y in (1.0, 2.3, 3.4):
        arrow(ax, (12.1, y), (11.3, y), lw=0.7)
    ax.text(12.25, 2.6, r"ref", fontsize=9, style="italic")
    ax.text(6.15, 1.9, r"ref", fontsize=9, style="italic")
    ax.text(5.98, yc + 0.55, "optional", ha="center", fontsize=8.5, style="italic")
    caption(ax, 7.3, 5.35, "", fontsize=1)
    ax.text(13.0, yc, "SER method:\nsemi-analytic while\nσ(t) is exact,\notherwise counted", fontsize=8.5, va="center", ha="left")
    save(
        fig,
        "receiver-output-nodes",
        png_dir,
        "End of a receiver graph with receiver output and analysis nodes",
        "The receive filter output, optionally after an FFE, feeds the receiver output, the "
        "PAM-4 analyzer and the eye diagram. The fiber output feeds the TDECQ node. The "
        "analysis nodes and the FFE also take the transmitted symbols from the pattern source "
        "as reference. The SER is semi-analytic while sigma(t) is exact and counted otherwise.",
    )

def driver_jitter(png_dir):
    """(a) Receiver waveform with RJ + DCD and the slope noise σ(t); (b) bathtub vs dual-Dirac."""
    from scipy.special import erfc

    baud, sps, rj, dcd = 10.3125e9, 64, 0.002, 0.08  # RJ, DCD in UI
    g = lightstone.Graph.reference(lane_config={"tx": {"sps": sps, "baud": baud, "levels": 2}, "rx": {"ffe_taps": 0}})
    g.set("vcsel", rin_enabled=False, vcsel={"resonance_freq": 50e9, "damping": 0.7})
    g.set("el1", length=0.0)
    g.set("pin", shot_noise=False)
    g.set("tia", thermal_noise=False)
    g.set("rxf", f3db_rel=4.0)
    g.set("drv", rj_rms=rj / baud, dcd=dcd / baud)
    g.run()
    rx = g.receiver("rx")
    y, sg, s = np.asarray(rx.noiseless), np.asarray(rx.sigma), np.asarray(rx.symbols)
    d = lightstone.analyze(y, s, sps, y, sg, levels=2)["delay"]
    n = len(s)
    base = d + np.arange(n) * sps
    mid = y[(base + sps // 2) % y.size]
    lo, hi = mid[s == 0].mean(), mid[s == 1].mean()
    th = 0.5 * (lo + hi)
    # Bathtub on a fine grid (linear interpolation of y0 and σ).
    idx = np.arange(y.size + 1)
    yw, sw = np.append(y, y[0]), np.append(sg, sg[0])
    ts = np.arange(-sps // 4 * 8, (sps + sps // 4) * 8) / 8
    ber = []
    for p in ts:
        x = (base + p) % y.size
        v, sv = np.interp(x, idx, yw), np.interp(x, idx, sw)
        z = np.where(s == 1, v - th, th - v) / np.maximum(sv, 1e-300)
        ber.append(np.mean(0.5 * erfc(np.minimum(z, 40) / np.sqrt(2))))
    t_ui = ts / sps
    # Dual-Dirac model, centred on the simulated eye.
    q = lambda x: 0.5 * erfc(x / np.sqrt(2))  # noqa: E731
    def dd(t):
        return 0.25 * (q((t + dcd / 2) / rj) + q((t - dcd / 2) / rj) + q((1 - dcd / 2 - t) / rj) + q((1 + dcd / 2 - t) / rj))
    lb = np.log10(np.maximum(ber, 1e-300))
    open_ = t_ui[lb < -12]
    shift = 0.5 * (open_.min() + open_.max()) - 0.5
    fig, (a, b) = plt.subplots(1, 2, figsize=(9.6, 3.3))
    # (a) 0 → 1 → 0 → 1 1 → 0: noiseless waveform and σ(t).
    k = next(i for i in range(8, n - 8) if list(s[i:i + 5]) == [0, 1, 0, 1, 1])
    seg = (d + (k - 1) * sps + np.arange(7 * sps)) % y.size
    tt = np.arange(7 * sps) / sps - 1
    a.plot(tt, (y[seg] - lo) / (hi - lo), color=INK, lw=1.0)
    a2 = a.twinx()
    a2.plot(tt, sg[seg] / (hi - lo) * 1e3, color=ACCENT, lw=1.0)
    a2.set_ylabel(r"$\sigma(t)$ / swing $\times 10^{3}$", color=ACCENT)
    a2.tick_params(axis="y", colors=ACCENT, right=True)
    a.tick_params(right=False)
    a.set_xlabel("time (UI)")
    a.set_ylabel("received signal (normalized)")
    a.set_xlim(tt[0], tt[-1])
    panel_title(a, "(a) slope noise at the edges")
    b.semilogy(t_ui - shift, ber, color=INK, lw=1.2, label="Lightstone, semi-analytic")
    tm = np.linspace(-0.25, 1.25, 1500)
    b.semilogy(tm, [dd(x) for x in tm], color=ACCENT, lw=1.0, ls=(0, (5, 3)), label="dual-Dirac model")
    b.set_ylim(1e-15, 1)
    b.set_xlim(-0.08, 0.12)
    b.set_xlabel("sampling instant (UI), left eye edge")
    b.set_ylabel("BER")
    b.legend(frameon=False, fontsize=9.5, loc="lower left")
    for x0 in (-dcd / 2, dcd / 2):
        b.axvline(x0, color=INK, lw=0.5, ls=(0, (2, 2)))
    b.text(0.0, 3e-15, r"$\pm\mathrm{DCD}/2$", ha="center", va="bottom", fontsize=9.5)
    panel_title(b, f"(b) bathtub: RJ {rj * 1e3:.0f} mUI rms, DCD {dcd:.2f} UI")
    fig.tight_layout()
    save(
        fig,
        "driver-jitter",
        png_dir,
        "Transmitter jitter: slope noise and bathtub curve",
        "Left: a received NRZ bit sequence of a fast 10.3125 GBd link with its noise standard "
        "deviation sigma(t), which peaks at the edges and is zero in between (random jitter as "
        "slope noise). Right: the BER versus sampling instant (bathtub) from the semi-analytic "
        "model with 2 mUI random jitter and 0.08 UI duty-cycle distortion, on top of the "
        "dual-Dirac model curve; the two agree down to 1e-15.",
    )


def wdm_demux(png_dir):
    """(a) Passbands of the SWDM4 demux ports; (b) OSA trace at demux port 1 (880 nm)."""
    chans = [850e-9, 880e-9, 910e-9, 940e-9]
    # (a) Transmission vs wavelength, measured with a power meter behind a bandpass filter
    # that has the demux's passband (same model, eq:wdm-passband).
    lam = np.linspace(830e-9, 960e-9, 131)
    tr = []
    for c in chans:
        g = lightstone.Graph(settings={"sps": 4})
        g.add("src", "prbs13q").add("drv", "driver").add("vcsel", "vcsel", rin_enabled=False)
        g.add("bp", "bandpass_filter", center=c, width=22e-9, order=3, insertion_loss_db=1.0, isolation_db=30.0)
        g.add("pm", "power_meter").chain("src", "drv", "vcsel", "bp", "pm")
        row = []
        for l in lam:
            g.set("vcsel", vcsel={"wavelength": float(l), "spectral_width_rms": 0.0}).run()
            row.append(10 * np.log10(g.power("pm") / 1e-3))
        tr.append(row)
    # (b) SWDM4: 4 VCSELs → mux → 100 m OM5 → demux; OSA at port 1.
    g = lightstone.Graph(settings={"sps": 4})
    for i, c in enumerate(chans):
        g.add(f"s{i}", "prbs13q", seed=[2731, 1, 77, 4001][i]).add(f"d{i}", "driver")
        g.add(f"v{i}", "vcsel", vcsel={"wavelength": c}).chain(f"s{i}", f"d{i}", f"v{i}")
    g.add("mux", "wdm_mux").add("con", "connector").add("fib", "mmf", type="om5", length=100.0)
    g.add("dmx", "wdm_demux").add("osa", "optical_spectrum_analyzer", rbw=0.1e-9)
    for i in range(4):
        g.wire(f"v{i}", f"mux.in{i}")
    g.chain("mux", "con", "fib", "dmx").wire("dmx.out1", "osa")
    g.run()
    o = g.report("osa")
    fig, (a, b) = plt.subplots(1, 2, figsize=(9.6, 3.2))
    for k, row in enumerate(tr):
        a.plot(lam * 1e9, row, color=ACCENT if k == 1 else INK, lw=1.2 if k == 1 else 0.8)
        a.text(chans[k] * 1e9, 0.6, f"out{k}", ha="center", fontsize=9)
    a.set(xlabel="wavelength (nm)", ylabel="transmission (dB)", xlim=(830, 960), ylim=(-36, 3))
    panel_title(a, "(a) demux passbands: 22 nm, IL 1 dB, isolation 30 dB")
    b.plot(np.array(o["wavelength"]) * 1e9, o["power_dbm"], color=INK, lw=0.9)
    b.set(xlabel="wavelength (nm)", ylabel="power per 0.1 nm RBW (dBm)", xlim=(830, 960), ylim=(-70, 0))
    for s in o["sources"]:
        b.annotate(f"{s['power_dbm']:.1f} dBm", (s["wavelength"] * 1e9, -9.5 if s["power_dbm"] > -20 else -39.5),
                   ha="center", fontsize=8.5, color=ACCENT if s["power_dbm"] > -20 else INK)
    b.text(958, -3, "labels: total power per channel", ha="right", va="top", fontsize=8.5)
    panel_title(b, "(b) OSA at out1 (880 nm) after 100 m OM5")
    fig.tight_layout()
    save(
        fig,
        "wdm-demux",
        png_dir,
        "SWDM4 demultiplexer passbands and the spectrum at one output",
        "Left: transmission versus wavelength of the four demux ports centred at 850, 880, 910 "
        "and 940 nm, each a flat-topped passband 22 nm wide with 1 dB insertion loss and a floor "
        "30 dB below the peak. Right: the optical spectrum analyzer at the 880 nm output of a "
        "four-channel link over 100 m of OM5: the 880 nm channel and, about 30 dB lower, the "
        "leakage of the three other channels (crosstalk).",
    )


FIGURES = {
    "wdm-demux": wdm_demux,
    "driver-jitter": driver_jitter,
    "ffe-delay-line": ffe_delay_line,
    "ffe-eyes": ffe_eyes,
    "vcsel-levels-response": vcsel_levels_response,
    "multimode-fiber-profile": multimode_fiber_profile,
    "multimode-fiber-delays": multimode_fiber_delays,
    "connector-launch-geometry": connector_launch_geometry,
    "connector-launch-groups": connector_launch_groups,
    "attenuator-modes": attenuator_modes,
    "pin-tia-chain": pin_tia_chain,
    "pin-tia-filter": pin_tia_filter,
    "pam4-eye-annotated": pam4_eye_annotated,
    "pam4-ser-tails": pam4_ser_tails,
    "tdecq-eye": tdecq_eye,
    "splitter-combiner": splitter_combiner,
    "probe-power-meter": probe_power_meter,
    "receiver-output-nodes": receiver_output_nodes,
}

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--png", type=Path, default=None, help="also write PNG previews into this directory")
    p.add_argument("--only", nargs="*", default=None, help="figure names to generate (default: all)")
    args = p.parse_args()
    for name, fn in FIGURES.items():
        if args.only is None or name in args.only:
            fn(args.png)

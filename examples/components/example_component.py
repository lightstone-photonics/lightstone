"""Example custom component for Lightstone: an electrical gain and delay block.

    H(f) = gain · exp(−j·2π·f·delay)

With the defaults (gain = 1, delay = 0 s) H(f) = 1: the block passes its input unchanged.
Model doc: docs/model/example-component.md (also explains how to write and load your own).

Load it into a graph:

    import importlib.util, lightstone
    spec = importlib.util.spec_from_file_location("example_component", "examples/components/example_component.py")
    mod = importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
    g = lightstone.Graph.reference(components=[mod.example_component])
    g.insert("ffe", "ex", "example_component", gain=0.5)   # between rxf and ffe
"""

import numpy as np

import lightstone


def transfer(f, ctx):
    """H(f) at the grid frequencies f [Hz] (FFT order, negative frequencies included)."""
    gain = float(ctx.params.get("gain", 1.0))  # dimensionless
    delay = float(ctx.params.get("delay", 0.0))  # s
    return gain * np.exp(-2j * np.pi * f * delay)


example_component = lightstone.Component(
    "example_component",
    inputs={"in": "electrical"},
    outputs={"out": "electrical"},
    linear=True,
    domain="frequency",
    function=transfer,
    doc="Electrical gain and delay, H(f) = gain·exp(−j2πf·delay); defaults give H(f) = 1. "
    "See docs/model/example-component.md.",
)

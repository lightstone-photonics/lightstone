"""Composable systems (docs/architecture.md, Section 6): R1 golden regression and Python API."""

import json
from pathlib import Path

import numpy as np
import pytest

import lightstone

GOLDEN = json.loads((Path(__file__).parent / "golden_reference_link.json").read_text())


@pytest.mark.parametrize("case", sorted(GOLDEN))
def test_r1_refactor_matches_golden(case):
    """R1: golden snapshots. The v0.11 cases pin the `fixed_delta` fiber model (and α = 1.969)
    bit for bit; the `_v012` cases pin the germania-doped default model."""
    g = GOLDEN[case]
    r = lightstone.simulate(g["params"])
    idx = g["idx"]
    for key in ("rx_noiseless", "rx_sigma", "rx_signal"):
        a = getattr(r, key)[idx]
        b = np.array(g[key])
        assert np.max(np.abs(a - b)) <= 1e-9 * np.max(np.abs(b)), key
    for k, v in g["metrics"].items():
        if v is None:
            assert r.metrics[k] is None, k
        else:
            assert r.metrics[k] == pytest.approx(v, rel=1e-9, abs=1e-30), k


def test_system_default_equals_link():
    a = lightstone.System().run()[0]
    b = lightstone.simulate()
    assert np.array_equal(a.rx_signal, b.rx_signal)


def test_multi_lane_and_elements():
    sys = lightstone.System([
        lightstone.lane([lightstone.connector(offset=3e-6), lightstone.fiber(60), lightstone.connector(loss_db=0.5), lightstone.fiber(40)], name="A"),
        lightstone.lane([lightstone.fiber(150), lightstone.attenuator(2.0)], name="B", seed=1),
        lightstone.lane([lightstone.attenuator(1.0)], name="B2B", seed=2),
    ])
    assert [i["severity"] for i in sys.validate()] == ["warning"]  # back-to-back lane
    a, b, c = sys.run()
    # Lane B: 2 dB attenuator + 150 m fiber loss ⇒ less power than lane A.
    assert b.metrics["rx_avg_power_dbm"] < a.metrics["rx_avg_power_dbm"]
    assert c.modes == []
    assert c.metrics["transmission"] == pytest.approx(10 ** -0.1)


def test_invalid_system_raises():
    sys = lightstone.System([lightstone.lane([lightstone.fiber(50), lightstone.connector(offset=2e-6), lightstone.fiber(50)])])
    issues = sys.validate()
    assert any(i["severity"] == "error" and i["element"] == 1 for i in issues)
    with pytest.raises(lightstone.SystemError):
        sys.run()

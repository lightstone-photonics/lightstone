"""Example graphs in examples/graphs/: they run, and the start-page example stays the reference
link (the engine's lane preset, as the browser builds it) plus its analysis nodes."""

import json
from pathlib import Path

import pytest

import lightstone

EXAMPLES = Path(__file__).resolve().parents[2] / "examples" / "graphs"


def load(name):
    return json.loads((EXAMPLES / name).read_text())


def test_start_page_example_is_reference_link_plus_analysis():
    """pam4-100m-om4.json = Graph.reference() at 8 samples/UI (the browser preset) + `ana` and
    `eye` after the FFE. Fails when a default of the reference link changes: re-export it then."""
    ex = load("pam4-100m-om4.json")
    ref = lightstone.Graph.reference(lane_config={"tx": {"sps": 8}}).config
    extra = {"ana", "eye"}
    assert ex["settings"] == ref["settings"]
    assert [n for n in ex["nodes"] if n["id"] not in extra] == ref["nodes"]
    wires = lambda g: sorted((w["from"], w["to"]) for w in g["wires"])  # noqa: E731
    assert [w for w in wires(ex) if w[1].split(".")[0] not in extra] == wires(ref)
    assert {n["id"]: n["kind"] for n in ex["nodes"] if n["id"] in extra} == {"ana": "pam4_analyzer", "eye": "eye_diagram"}
    assert sorted(w for w in wires(ex) if w[1].split(".")[0] in extra) == sorted(
        [("ffe.out", "ana.in"), ("src.out", "ana.ref"), ("ffe.out", "eye.in"), ("src.out", "eye.ref")])


@pytest.mark.parametrize("name", sorted(p.name for p in EXAMPLES.glob("*.json")))
def test_example_runs_and_channels_meet_kp4(name):
    g = lightstone.Graph.from_json((EXAMPLES / name).read_text()).run()
    kinds = {n["id"]: n["kind"] for n in g.config["nodes"]}
    analyzers = [i for i, k in kinds.items() if k == "pam4_analyzer"]
    assert analyzers
    for a in analyzers:
        assert g.report(a)["ber"] < 2.4e-4, (name, a)


def test_large_seeds_survive_javascript():
    """Seeds above 2^53 are written as strings (JavaScript numbers would round them), and a
    graph gives the same noise realization with the seed as a string or as a number."""
    cfg = lightstone.Graph.reference(lane_config={"tx": {"sps": 8}}).config
    drv = next(n for n in cfg["nodes"] if n["id"] == "drv")
    assert isinstance(drv["params"]["seed"], str) and int(drv["params"]["seed"]) > 2**53
    as_numbers = json.loads(json.dumps(cfg))
    for n in as_numbers["nodes"]:
        for k in ("seed", "noise_seed"):
            if isinstance(n["params"].get(k), str):
                n["params"][k] = int(n["params"][k])
    a = lightstone.Graph(cfg).run().receiver().signal
    b = lightstone.Graph(as_numbers).run().receiver().signal
    assert (a == b).all()
    rounded = json.loads(json.dumps(as_numbers))
    for n in rounded["nodes"]:
        for k in ("seed", "noise_seed"):
            if isinstance(n["params"].get(k), int) and n["params"][k] > 2**53:
                n["params"][k] = int(float(n["params"][k]))  # what JSON.parse did before
    c = lightstone.Graph(rounded).run().receiver().signal
    assert not (a == c).all()

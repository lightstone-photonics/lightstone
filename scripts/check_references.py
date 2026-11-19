"""Check that every URL in docs/references.yaml resolves (non-blocking CI job).

Uses only the standard library; parses the simple `url:` lines of the YAML file.
"""

import re
import sys
import urllib.request
from pathlib import Path

text = (Path(__file__).resolve().parents[1] / "docs" / "references.yaml").read_text()
urls = re.findall(r"^\s*url:\s*(https?://\S+)", text, flags=re.M)
bad = 0
for u in urls:
    req = urllib.request.Request(u, method="GET", headers={"User-Agent": "lightstone-link-check"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            code = r.status
    except Exception as e:  # noqa: BLE001 - report any failure
        code = getattr(e, "code", str(e))
    ok = isinstance(code, int) and code < 400
    bad += not ok
    print(f"{'OK ' if ok else 'BAD'} {code} {u}")
sys.exit(1 if bad else 0)

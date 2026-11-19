"""Extract the 850 nm sheets of the IEEE 802.3ae 10G Ethernet link model for validation V14a.

Source: "10GEPBud3_1_16a.xls" by D. Hanson, D. Cunningham, P. Dawe and D. Dolfi
(Agilent Technologies), the link model spreadsheet aligned to IEEE P802.3ae D3.2/3.3, at
https://www.ieee802.org/3/ae/public/adhoc/serial_pmd/documents/10GEPBud3_1_16a.xls
(introduction: https://grouper.ieee.org/groups/802/3/efm/public/sep01/dawe_1_0901.pdf).

The workbook stores computed values only (no formulas are read). This script copies the
inputs and the length-dependent columns of the five 850 nm serial sheets into
tests/python/data/ieee_10g_link_model.json; tests/python/ieee_link_model.py re-implements
the equations and tests/python/test_v14_ieee_link_model.py checks them against these values.

Usage (one-off, needs network and xlrd):
    pip install xlrd==2.0.1
    python scripts/extract_ieee_10g_link_model.py [path/to/10GEPBud3_1_16a.xls]
"""

import json
import sys
import urllib.request
from pathlib import Path

import xlrd

URL = "https://www.ieee802.org/3/ae/public/adhoc/serial_pmd/documents/10GEPBud3_1_16a.xls"
SHEETS = ["850S62_160", "850S62_200", "850S50_400", "850S50_500", "850S2000"]
OUT = Path(__file__).resolve().parents[1] / "tests" / "python" / "data" / "ieee_10g_link_model.json"

# Input cells (row, column), 0-based, as laid out in rev. 3.1.16a.
INPUTS = {
    "ts_20_80_ps": (1, 6),
    "ts_10_90_ps": (2, 6),
    "q": (2, 2),
    "baud_mbd": (3, 2),
    "wavelength_nm": (5, 2),
    "spectral_width_rms_nm": (6, 2),
    "dcd_ps": (7, 6),
    "c1_ns_mhz": (8, 11),
    "lambda0_nm": (6, 15),
    "s0_ps_nm2_km": (7, 15),
    "d1_ps_nm_km": (8, 15),
    "bwm_mhz_km": (11, 15),
    "rx_bw_mhz": (4, 19),
    "c_rx_ns_mhz": (5, 19),
    "t_rx_10_90_ps": (6, 19),
}
# Length-dependent columns (rows 16 … 37).
COLUMNS = {
    "length_km": 0,
    "d1l_ps_nm": 3,
    "bw_cd_mhz": 5,
    "eff_bwm_mhz": 6,
    "te_ps": 7,
    "tc_ps": 8,
    "p_isi_db": 9,
}


def main() -> None:
    if len(sys.argv) > 1:
        data = Path(sys.argv[1]).read_bytes()
    else:
        with urllib.request.urlopen(URL, timeout=60) as r:
            data = r.read()
    book = xlrd.open_workbook(file_contents=data)
    out = {
        "source": URL,
        "file": "10GEPBud3_1_16a.xls",
        "revision": book.sheet_by_name(SHEETS[-1]).cell_value(1, 20),
        "authors": "D. Hanson, D. Cunningham, P. Dawe, D. Dolfi (Agilent Technologies)",
        "note": "Values copied from the workbook's computed cells for validation V14a.",
        "sheets": {},
    }
    for name in SHEETS:
        s = book.sheet_by_name(name)
        inputs = {k: s.cell_value(r, c) for k, (r, c) in INPUTS.items()}
        rows = []
        for r in range(16, 38):
            if s.cell_value(r, 0) == "":
                break
            rows.append({k: s.cell_value(r, c) for k, c in COLUMNS.items()})
        out["sheets"][name] = {"inputs": inputs, "rows": rows}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(out, indent=1) + "\n")
    print(f"wrote {OUT} ({sum(len(v['rows']) for v in out['sheets'].values())} rows)")


if __name__ == "__main__":
    main()

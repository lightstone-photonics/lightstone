import sys
from pathlib import Path

# Make the reference implementation importable without installation.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "reference"))

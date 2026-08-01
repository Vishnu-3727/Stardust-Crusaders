"""Puts the compatibility shim on `sys.path` so the untouched upstream
`test_fuzzy.py` can `import fuzzy` and reach the Rust binary.

This file and `shim/fuzzy.py` are the only additions in this directory.
`test_fuzzy.py` is byte-identical to upstream — its SHA-256 is recorded in
`.port-mortem.toml` and checked by `make verify`.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "shim"))

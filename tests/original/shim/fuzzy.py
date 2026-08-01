"""Compatibility shim: satisfies `import fuzzy` for the ORIGINAL test suite.

This exists because of a collision between two hackathon rules:

  * Rule 02 wants the original test suite to run unmodified — and
    `test/test_fuzzy.py` is pytest doing `import fuzzy`.
  * Rule 05 forbids linking the source-language runtime — a Python -> Rust port
    "cannot link against the Python interpreter", which rules out PyO3.

So the port does not become a Python extension. It stays a plain binary, and
this module talks to it over a pipe. A subprocess boundary is not linking:
`target/release/fuzzy` contains no Python, no libpython, no CPython ABI, and
the Dockerfile's runtime stage has no interpreter in it at all. Nothing here
ships in the artifact — this file lives under `tests/`, and only pytest and the
benchmark harness ever import it.

What it does NOT do is change any behavior. Every value below comes back from
the Rust binary; the shim's whole job is to restore the *types* the Cython
extension returned, which is where the subtlety is:

  * `Soundex(n)(s)` returned `str`  (`c_string_type=unicode`)
  * `DMetaphone()(s)` returns `[bytes, bytes|None]` — upstream issue #13, a
    known wart: the metaphone codes come back as bytes while soundex comes back
    as str, because `cdef bytes o1` is declared but the module-level string
    type is unicode. The test asserts `== [b'MR', None]`, so the wart is load
    bearing.
  * Non-ASCII raises `UnicodeEncodeError` before any phonetics happen, because
    that is where the Cython-generated code raises it too (upstream issue #15).
"""

import os
import subprocess
import sys
from pathlib import Path

_REPO = Path(__file__).resolve().parents[3]

_CANDIDATES = [
    _REPO / "target" / "release" / "fuzzy.exe",
    _REPO / "target" / "release" / "fuzzy",
    _REPO / "target" / "debug" / "fuzzy.exe",
    _REPO / "target" / "debug" / "fuzzy",
]


def _binary():
    override = os.environ.get("FUZZY_BIN")
    if override:
        return Path(override)
    for path in _CANDIDATES:
        if path.exists():
            return path
    raise RuntimeError(
        "fuzzy binary not found; run `cargo build --release` or set FUZZY_BIN.\n"
        "looked in:\n  " + "\n  ".join(str(p) for p in _CANDIDATES)
    )


class _Backend:
    """One long-lived process, reused across calls."""

    _instance = None

    @classmethod
    def get(cls):
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance

    def __init__(self):
        self.proc = subprocess.Popen(
            [str(_binary())],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            bufsize=1,
            text=True,
            encoding="utf-8",
        )

    def call(self, algo, size, word):
        if "\n" in word or "\r" in word:
            raise ValueError("the fuzzy CLI line protocol cannot carry newlines")
        self.proc.stdin.write(f"{algo}\t{size}\t{word}\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("fuzzy backend exited unexpectedly")
        status, _, rest = line.rstrip("\n").partition("\t")
        if status != "OK":
            raise RuntimeError(f"fuzzy backend error: {rest}")
        return rest.split("\t")


def _as_ascii(s):
    """Reproduce the Cython layer's encode step, including where it raises."""
    if isinstance(s, bytes):
        return s.decode("ascii")
    s.encode("ascii")  # raises UnicodeEncodeError, exactly as upstream does
    return s


class Soundex:
    """`fuzzy.Soundex(size)` — returns `str`, zero-padded to `size`."""

    def __init__(self, size):
        self.size = size

    def __call__(self, s):
        (code,) = _Backend.get().call("SOUNDEX", self.size, _as_ascii(s))
        return code


class DMetaphone:
    """`fuzzy.DMetaphone(size=0)` — returns `[bytes|None, bytes|None]`."""

    def __init__(self, size=0):
        self.size = size

    def __call__(self, s):
        primary, secondary = _Backend.get().call("DMETAPHONE", self.size, _as_ascii(s))
        return [
            None if primary == "NULL" else primary.encode("ascii"),
            None if secondary == "NULL" else secondary.encode("ascii"),
        ]


def nysiis(s):
    """`fuzzy.nysiis(s)` — returns `str`. Accepts non-ASCII, as upstream does."""
    (code,) = _Backend.get().call("NYSIIS", 0, s)
    return code


__all__ = ["Soundex", "DMetaphone", "nysiis"]

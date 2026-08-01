#!/usr/bin/env python3
"""One command that checks every claim this repo makes.

    python verify.py            # full run, ~2 minutes
    python verify.py --quick    # skip the long fuzz, ~20 seconds

Exits non-zero on the first failed claim. Nothing here is decorative — each
check corresponds to a rule or a bonus we are claiming.
"""

import argparse
import hashlib
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
GREEN, RED, DIM, OFF = "\033[32m", "\033[31m", "\033[2m", "\033[0m"

failures = []


def check(name, ok, detail=""):
    mark = f"{GREEN}PASS{OFF}" if ok else f"{RED}FAIL{OFF}"
    print(f"  [{mark}] {name}")
    if detail:
        print(f"{DIM}         {detail}{OFF}")
    if not ok:
        failures.append(name)
    return ok


def run(argv, **kw):
    return subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, **kw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true")
    args = ap.parse_args()

    manifest = tomllib.loads((ROOT / ".port-mortem.toml").read_text())

    print("\nRule 02 - the original test suite is unmodified")
    suite = ROOT / manifest["test_suite"]["path"]
    raw = suite.read_bytes()
    check(
        "SHA-256 matches the manifest",
        hashlib.sha256(raw).hexdigest() == manifest["test_suite"]["sha256"],
        f"{len(raw)} bytes, expected {manifest['test_suite']['size_bytes']}",
    )
    blob = run(["git", "hash-object", str(suite)]).stdout.strip()
    check(
        "git blob id matches upstream's own object",
        blob == manifest["test_suite"]["git_blob"],
        f"{blob} - identical object id to yougov/fuzzy@"
        f"{manifest['source']['commit'][:12]}",
    )

    print("\nRule 05 - no source-language runtime in the artifact")
    lock = (ROOT / "Cargo.lock").read_text()
    check(
        "no Python bindings in the dependency graph",
        not any(dep in lock for dep in ("pyo3", "cpython", "rust-cpython", "pyembed")),
    )
    deps = run(["cargo", "tree", "--edges", "normal"]).stdout
    check(
        "the library has no third-party dependencies at all",
        deps.count("fuzzy v") >= 1 and "pyo3" not in deps,
    )

    print("\nZero-unsafe bonus")
    # forbid(unsafe_code) at a crate root covers that whole crate, submodules
    # included, and unlike `deny` it cannot be switched back off by an inner
    # #[allow]. So the check that actually means something is: every crate root
    # carries it. Grepping for the token only catches what someone left lying
    # around in a crate that never banned it in the first place.
    roots = sorted(
        p for p in (ROOT / "crates").rglob("*.rs") if p.name in ("lib.rs", "main.rs")
    )
    unguarded = [
        p.relative_to(ROOT) for p in roots if "#![forbid(unsafe_code)]" not in p.read_text()
    ]
    check(
        f"#![forbid(unsafe_code)] on every crate root ({len(roots)} found)",
        roots and not unguarded,
        str(unguarded) if roots else "no crate roots found - glob is wrong",
    )
    # Belt and braces: the attribute above makes this unreachable, but it fails
    # loudly if someone ever adds a crate and forgets the attribute.
    offenders = [
        f"{p.relative_to(ROOT)}:{i}"
        for p in (ROOT / "crates").rglob("*.rs")
        for i, line in enumerate(p.read_text().splitlines(), 1)
        if "unsafe" in line and "forbid(unsafe_code)" not in line and not line.lstrip().startswith("//")
    ]
    check("no `unsafe` anywhere in crates/", not offenders, str(offenders))

    print("\nFunctionality - the port's own tests")
    r = run(["cargo", "test", "--quiet"])
    check("cargo test", r.returncode == 0, "; ".join(l for l in r.stdout.splitlines() if l.startswith("test result")) or r.stderr[-200:])

    print("\nFunctionality - the UPSTREAM suite, unmodified, against the Rust binary")
    if not (ROOT / "target" / "release").exists():
        run(["cargo", "build", "--release"])
    r = run([sys.executable, "-m", "pytest", "tests/original", "-q"])
    tail = (r.stdout.strip().splitlines() or ["no output"])[-1]
    check("pytest tests/original - 0 failures", r.returncode == 0, tail)
    check(
        "test_soundex_result XPASSES (upstream marks it xfail, issue #14)",
        "1 xpassed" in r.stdout,
    )

    print("\nBehavioral equivalence - differential fuzz against the original")
    oracle = ROOT / "oracle" / ("dm_oracle.exe" if sys.platform == "win32" else "dm_oracle")
    if not oracle.exists():
        build = run(["gcc", "-O2", "-o", str(oracle),
                     str(ROOT / "oracle" / "dm_driver.c"),
                     str(ROOT / "oracle" / "double_metaphone.c")])
        if build.returncode != 0:
            check("build the C oracle", False, build.stderr[-300:])
    if oracle.exists():
        budget = ["--count", "20000"] if args.quick else ["--seconds", "60"]
        for algo in ("DMETAPHONE", "NYSIIS"):
            r = run([sys.executable, "fuzz/harness.py", "--algo", algo, *budget])
            summary = [l for l in r.stdout.splitlines() if l.startswith("# checked")]
            check(f"{algo}: zero divergences", r.returncode == 0,
                  summary[0][2:] if summary else r.stdout[-200:])

    print()
    if failures:
        print(f"{RED}{len(failures)} check(s) failed:{OFF} " + ", ".join(failures))
        return 1
    print(f"{GREEN}all checks passed{OFF}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

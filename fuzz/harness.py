#!/usr/bin/env python3
"""Differential fuzzer: the original C Double Metaphone vs the Rust port.

The reference is not a reimplementation or a pinned PyPI wheel — it is
`src/double_metaphone.c` from yougov/fuzzy, compiled byte-for-byte unmodified
and driven through `oracle/dm_driver.c`, which reproduces the `None` collapsing
that `fuzzy.pyx` does on top of it. Any disagreement is a real disagreement.

Corpus, in order of usefulness:
  1. every string literal in the C source, plus one- and two-letter mutations
     of each — this is what actually reaches the rare branches
  2. names and words with known phonetic edge cases
  3. uniform random ASCII, including bytes the algorithm never expects

Usage:
    python fuzz/harness.py --seconds 90
    python fuzz/harness.py --count 200000 --seed 7
"""

import argparse
import random
import re
import string
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORACLE_DIR = ROOT / "oracle"
PORT = ROOT / "target" / "release" / ("fuzzy.exe" if sys.platform == "win32" else "fuzzy")
C_SOURCE = ORACLE_DIR / "double_metaphone.c"

# The claimed surface is DMETAPHONE + NYSIIS. Soundex is excluded and the
# exclusion is not a convenience: the original's Soundex is undefined behavior
# (read-after-free, upstream #14/#17/#20), so it has no stable output to compare
# against. Fuzzing it would compare the port to whatever the allocator felt like
# that second.
#
# The two surfaces get different references because upstream implements them in
# different languages:
#   DMETAPHONE  -> src/double_metaphone.c, compiled unmodified
#   NYSIIS      -> src/fuzzy.pyx's Python-semantics routine, extracted verbatim
_EXE = ".exe" if sys.platform == "win32" else ""
ORACLES = {
    "DMETAPHONE": [str(ORACLE_DIR / f"dm_oracle{_EXE}")],
    "NYSIIS": [sys.executable, str(ORACLE_DIR / "nysiis_oracle.py")],
}

SEEDS = [
    # Upstream README and test suite.
    "fuzzy", "mayer", "Test", "FancyFree",
    # The branch names the C's own comments call out.
    "dumb", "thumb", "smith", "smyth", "schmidt", "schneider", "snider",
    "michael", "chemistry", "chorus", "chore", "architect", "arch",
    "orchestra", "orchid", "wachtler", "wechsler", "tichner", "mchugh",
    "czerny", "focaccia", "bellocchio", "bacchus", "accident", "accede",
    "succeed", "mcclellan", "caesar", "chianti", "edge", "edgar",
    "ghislane", "ghiradelli", "hugh", "bough", "broughton", "laugh",
    "mclaughlin", "cough", "gough", "rough", "tough", "cagney", "tagliaro",
    "danger", "ranger", "manger", "biaggi", "jose", "san jacinto",
    "yankelovich", "jankelowicz", "bajador", "cabrillo", "gallegos",
    "campbell", "raspberry", "rogier", "hochmeier", "island", "isle",
    "carlisle", "carlysle", "sugar", "resnais", "artois", "thomas", "thames",
    "van damme", "von neumann", "school", "schooner", "schermerhorn",
    "schenker", "wasserman", "vasserman", "uomo", "womo", "arnow", "arnoff",
    "filipowicz", "breaux", "zhao", "xavier", "knight", "wright", "pneumatic",
    "psychology", "gnome", "science", "scissors", "scene", "scythe", "cent",
    "city", "ciao", "cyst", "decentralization", "floyd", "macintosh",
    "mcintosh", "pfister", "jeroboam", "",
]

MUTATION_ALPHABET = string.ascii_uppercase + string.ascii_lowercase + " '-."


def literals_from_c(path):
    """Every string literal in the C source — the branch predicates themselves."""
    try:
        text = path.read_text(encoding="latin-1")
    except OSError:
        return []
    found = set(re.findall(r'"([^"\\\n]*)"', text))
    return [s for s in found if s and s.isprintable()]


def mutations(rng, word):
    """One edit away, plus the word glued to a random prefix/suffix."""
    if not word:
        return rng.choice(MUTATION_ALPHABET)
    op = rng.randrange(5)
    i = rng.randrange(len(word))
    c = rng.choice(MUTATION_ALPHABET)
    if op == 0:
        return word[:i] + c + word[i:]
    if op == 1:
        return word[:i] + word[i + 1:]
    if op == 2:
        return word[:i] + c + word[i + 1:]
    if op == 3:
        return c + word
    return word + c


def make_corpus(rng, seeds, n):
    """Yield n candidate inputs, weighted toward the interesting ones."""
    for _ in range(n):
        r = rng.random()
        if r < 0.45:
            yield mutations(rng, rng.choice(seeds))
        elif r < 0.65:
            yield rng.choice(seeds)
        elif r < 0.75:
            # Two seeds joined — exercises the "current == last" predicates.
            yield rng.choice(seeds) + rng.choice(" -'") + rng.choice(seeds)
        elif r < 0.95:
            length = rng.randrange(0, 14)
            yield "".join(rng.choice(MUTATION_ALPHABET) for _ in range(length))
        else:
            # Bytes the algorithm never expects: digits, punctuation, DEL.
            length = rng.randrange(0, 10)
            yield "".join(chr(rng.randrange(32, 127)) for _ in range(length))


class Proc:
    def __init__(self, argv, prefix="", encoding="ascii"):
        self.prefix = prefix
        self.p = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            bufsize=1,
            text=True,
            encoding=encoding,
            errors="strict",
        )

    def ask(self, words):
        # One request, one response, in lockstep. Writing the whole batch first
        # deadlocks: the child's replies fill its stdout pipe and it stops
        # draining ours. A reader thread would restore batching if this ever
        # becomes the bottleneck; at ~20k inputs/s it is not.
        out = []
        for w in words:
            self.p.stdin.write(self.prefix + w + "\n")
            self.p.stdin.flush()
            out.append(self.p.stdout.readline().rstrip("\n"))
        return out

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)


def normalise_port(line):
    """`OK<TAB>rest` -> `rest`, so the port lines up with the oracle's output."""
    if not line.startswith("OK\t"):
        return line
    return line[3:]


# Characters that uppercase into ASCII A-Z, which is the only way a non-ASCII
# input can reach NYSIIS's output at all. If Rust's `to_uppercase` and Python's
# `str.upper()` ever disagree, it is here.
UNICODE_PROBES = "ßﬁﬂŉǰﬅſıİµﬄàÉçÑ﻿·’İǰẞKＡⒶ"


def main():
    # Divergence reports carry the offending input verbatim; a console that
    # cannot encode it must not crash the run that found it.
    sys.stdout.reconfigure(encoding="utf-8", errors="backslashreplace")

    ap = argparse.ArgumentParser()
    ap.add_argument("--algo", choices=sorted(ORACLES), default="DMETAPHONE")
    ap.add_argument("--seconds", type=float, default=90.0)
    ap.add_argument("--count", type=int, default=None,
                    help="stop after this many inputs instead of after --seconds")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--batch", type=int, default=2000)
    ap.add_argument("--max-report", type=int, default=25)
    args = ap.parse_args()

    surface = args.algo
    oracle_argv = ORACLES[surface]
    for path in (Path(oracle_argv[-1]), PORT):
        if not path.exists():
            sys.exit(f"missing {path}\n"
                     "  oracle: gcc -O2 -o oracle/dm_oracle.exe oracle/dm_driver.c "
                     "oracle/double_metaphone.c\n"
                     "  port:   cargo build --release")

    # NYSIIS is the only entry point that accepts non-ASCII — it never crosses
    # into C, so there is no ASCII encode to raise first.
    unicode_ok = surface == "NYSIIS"
    encoding = "utf-8" if unicode_ok else "ascii"

    rng = random.Random(args.seed)
    seeds = SEEDS + literals_from_c(C_SOURCE)
    if unicode_ok:
        seeds = seeds + [c for c in UNICODE_PROBES] + [
            "stra" + "ß" + "e", "ﬁnger", "İstanbul", "ǰohn", "MAC" + "ß",
        ]

    oracle = Proc(oracle_argv, encoding=encoding)
    port = Proc([str(PORT)], prefix=f"{surface}\t0\t", encoding=encoding)

    checked = 0
    divergences = []
    started = time.time()
    print(f"# differential fuzz: {surface}")
    print(f"# oracle: {' '.join(oracle_argv)}")
    print(f"# port:   {PORT}")
    print(f"# seed={args.seed} batch={args.batch} "
          f"{'count=' + str(args.count) if args.count else 'seconds=' + str(args.seconds)}")
    print("# excluded from the claimed surface: SOUNDEX (upstream behavior is UB)")

    try:
        while True:
            if args.count is not None:
                if checked >= args.count:
                    break
                todo = min(args.batch, args.count - checked)
            else:
                if time.time() - started >= args.seconds:
                    break
                todo = args.batch

            # A newline cannot cross a line protocol, and the algorithm treats
            # it as a no-op character anyway; ASCII-only because the Cython
            # original raises on anything else before it reaches the C.
            words = [w.replace("\n", "").replace("\r", "").replace("\t", "")
                     for w in make_corpus(rng, seeds, todo)]

            got_oracle = oracle.ask(words)
            got_port = [normalise_port(x) for x in port.ask(words)]

            for w, a, b in zip(words, got_oracle, got_port):
                checked += 1
                if a != b:
                    divergences.append((w, a, b))

            if divergences and len(divergences) >= args.max_report:
                break
    finally:
        oracle.close()
        port.close()

    elapsed = time.time() - started
    print(f"# checked {checked} inputs in {elapsed:.1f}s "
          f"({checked / max(elapsed, 1e-9):.0f}/s)")

    if divergences:
        print(f"# DIVERGENCES: {len(divergences)}")
        for w, a, b in divergences[: args.max_report]:
            print(f"  input={w!r}\n    oracle={a!r}\n    port  ={b!r}")
        return 1

    print("# divergences: 0")
    return 0


if __name__ == "__main__":
    sys.exit(main())

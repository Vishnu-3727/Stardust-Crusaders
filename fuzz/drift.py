#!/usr/bin/env python3
"""How far has yougov/fuzzy drifted from Double Metaphone as published?

This is NOT a correctness check on the port — the port's job is to match
`fuzzy`, and `fuzz/harness.py` proves it does. This measures something else:
how much `fuzzy` disagrees with an independent implementation of the same
algorithm, and therefore how wrong a port written from the algorithm
*description* would be.

Reference: the `metaphone` package on PyPI (an independent Python
implementation of Philips' Double Metaphone). Install it to run this:

    pip install metaphone
    python fuzz/drift.py

Corpus: `bench/words-10k.txt`, the 10,000 most frequent English words
(first20hours/google-10000-english, USA list). Ordinary words, not a corpus
chosen to make a point.

The reference's codes are truncated to four characters before comparing, so
`fuzzy`'s documented 4-char cut (upstream issue #5) is not counted as drift.
"""

import subprocess
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ".exe" if sys.platform == "win32" else ""
ORACLE = ROOT / "oracle" / f"dm_oracle{EXE}"
WORDS = ROOT / "bench" / "words-10k.txt"

try:
    from metaphone import doublemetaphone as reference
except ImportError:
    sys.exit("needs the reference implementation: pip install metaphone")


def classify(word):
    """Which known upstream defect explains this word's divergence?"""
    w = word.lower()
    if any(c in w for c in ("ce", "ci", "cy")):
        return "soft C coded K (DECISIONS 04)"
    if "sc" in w and "sch" not in w:
        return "SC fallthrough (DECISIONS 05)"
    if w.endswith("ch"):
        return "word-final CH matches the pad (DECISIONS 20)"
    if "c" in w:
        return "C cursor over-advance eats the next letter (DECISIONS 04)"
    return "other"


def main():
    if not ORACLE.exists():
        sys.exit(f"missing {ORACLE} — see README")

    words = WORDS.read_text().split()
    got = subprocess.run([str(ORACLE)], input="\n".join(words).encode(),
                         stdout=subprocess.PIPE, check=True).stdout.decode().splitlines()

    diverged = []
    for word, line in zip(words, got):
        primary = line.split("\t")[0]
        primary = "" if primary == "NULL" else primary
        expected = reference(word)[0][:4]
        if primary != expected:
            diverged.append((word, primary, expected))

    print(f"corpus: {len(words)} most-frequent English words")
    print(f"fuzzy disagrees with the published algorithm on "
          f"{len(diverged)} of them ({100 * len(diverged) / len(words):.1f}%)\n")

    causes = Counter(classify(w) for w, _, _ in diverged)
    for cause, n in causes.most_common():
        print(f"  {n:5d}  {cause}")

    print("\nexamples:")
    for word, primary, expected in diverged[:15]:
        print(f"  {word:<14} fuzzy={primary:<6} published={expected}")

    print("\nThe port reproduces the fuzzy column, deliberately. A port written "
          "from\nthe algorithm description would produce the published column "
          "and be wrong\nabout the library it claims to have ported, on "
          f"{100 * len(diverged) / len(words):.0f}% of ordinary English.")


if __name__ == "__main__":
    main()

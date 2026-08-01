# Demo runsheet — 5 minutes

Screen recording only, no slides. Every claim on screen is a command someone
else can run. Terminal at ~110 columns, `cd fuzzy-rs` before recording.

Pre-record: `cargo build --release` and `python fuzz/harness.py --seconds 5`
once, so nothing compiles or cold-starts on camera.

---

## 0:00 — What was ported (25s)

```
$ cloc original/src
```

Say: yougov/fuzzy calls itself a Python phonetics library. It is 27 KB of 1999 C
plus a Cython wrapper. So Track H's "Python → Rust" is really C + Cython → Rust,
and no PyO3 — rule 05 forbids linking the source runtime.

## 0:25 — The upstream suite, unmodified (60s)

Lead with this. It is the strongest single frame in the video.

```
$ sha256sum tests/original/test_fuzzy.py
6dd19f9a38f848001d990ccb3745213a60efbb36a11293642f1b3bdbd5510ae5

$ pytest tests/original -v
2 passed, 1 xpassed, 2 xfailed
```

Say: same hash as kickoff, not one byte edited. It imports `fuzzy` and gets the
Rust binary through a shim. And one test **xpasses** — `test_soundex_result`,
which upstream marked xfail against its own issue #14. The port fixes it.

Be honest on camera about the two that stay xfail: `test_soundex_Test` expects
`Soundex(8)('Test') == 'T23'`, but a correct Soundex pads to `size`, so `'T2300000'`
is right and the *expectation* is wrong. Point at DECISIONS.md and move on.
Do not oversell this.

## 1:25 — Behavioral equivalence (60s)

```
$ python fuzz/harness.py --seconds 60
checked 558000 inputs in 60.1s (9281/s)
divergences: 0
```

Say: the other side of that pipe is `double_metaphone.c` compiled unmodified —
the real original, not a description of it. Cumulative to date: 1.35M inputs,
zero divergences. Show `fuzz/log.txt`.

## 2:25 — The headline: why bug-for-bug (75s)

```
$ python fuzz/drift.py
8.5% of the 10,000 most common English words disagree
```

Say: measured against the `metaphone` PyPI package, an independent
implementation of the published algorithm. On 8.5% of common English words
upstream's output is not Double Metaphone. Two brace bugs explain 96% of it.

Show the C, both sites:

- **soft C** (`double_metaphone.c` ~384) — an inner `if` got wrapped in braces,
  so the `else` rebinds to the outer `if` and fires for every C that is not
  `CC`. `cent` → `KNT`, should be `SNT`. The cursor over-advances too, so
  `click` → `KK` — the L is gone.
- **SC fallthrough** (~935) — a missing closing brace parks the `SC`+I/E/Y and
  default arms inside a branch whose both halves already `break`. Dead code.
  `science` → `SKNK`, should be `SNS`.

Then the point of the whole segment:

```
$ cargo test upstream_brace_bugs_are_preserved
test result: ok
```

Say: the port reproduces both **deliberately**, and has a test that fails if
anyone ever "fixes" them. A port that corrected these would break every index
built with the original. Behavioral equivalence means bug-for-bug.

## 3:40 — Zero unsafe, zero deps (35s)

```
$ grep -rn 'forbid(unsafe_code)' crates/
$ cargo tree
```

Say: the six-year issue cluster — #14, #17, #20, #4, #3, #6 — is one root cause,
`fuzzy.pyx:207` reading a `char*` into a freed temporary. Not fixed by
discipline here; there is no line in this repo where it could be written.

## 4:15 — One command proves all of it (30s)

```
$ python verify.py
all checks passed        # 11/11
```

Let it scroll. Unsafe audit, cargo test, the hash-checked upstream suite, both
fuzz campaigns.

## 4:45 — Close (15s)

Benchmarks honest in both directions: algorithm 1.68× faster in-process, CLI
protocol 32% slower than the C driver, peak RSS 4.00 vs 4.81 MB. Bugs filed
upstream, `upstream-issues/`. 22 entries in DECISIONS.md. Done.

---

## Do not

- Do not claim Soundex equivalence. Upstream Soundex is read-after-free UB —
  there is no stable output to be equivalent *to*. It is excluded from the
  fuzzed surface on purpose, and DECISIONS.md says why.
- Do not call the word-final `CH` → `K` behavior a third new bug. It is in
  Aubrey's original C unmodified (DECISIONS 20).
- Do not show `docker build` unless it has been run green beforehand.

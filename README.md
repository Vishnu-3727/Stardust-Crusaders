# fuzzy-rs

A Rust port of [`yougov/fuzzy`](https://github.com/yougov/fuzzy) — Soundex,
NYSIIS and Double Metaphone.

**Port Mortem 2026, Track H** (open pair): Python + Cython + C → Rust.
Source commit [`e15b195`](https://github.com/yougov/fuzzy/commit/e15b195467223a684a26fadb53997bf6f36be2c4), MIT.

```
$ pytest tests/original -v
2 passed, 1 xpassed, 2 xfailed          # upstream's own suite, byte-identical

$ python fuzz/harness.py --seconds 120
# checked 1172000 inputs in 120.2s
# divergences: 0                        # against the original C, compiled
```

---

## What this library is, and why it was worth porting

`fuzzy` maps words to sound-alike codes, so `Smith` and `Smyth` collide. It is
the plumbing under fuzzy name matching, deduplication and record linkage — 51
stars, on PyPI, last pushed July 2023, 15 open issues, no release since 2017.

The README calls it a Python library. It is not:

```
src/double_metaphone.c   26,965 B   Maurice Aubrey's 1999 C — the real engine
src/fuzzy.pyx             5,301 B   Cython: Soundex + NYSIIS + a C wrapper
test/test_fuzzy.py           738 B   the entire test suite (5 tests, 3 xfail)
```

So the pair is really **C + Cython → Rust**, and that is the whole argument for
it. Read the open issue list as a group rather than one at a time:

| Issue | Symptom |
|---|---|
| #14, #17, #20 | `Soundex(4)('FancyFree')` returns `''`, or garbage, or works |
| #4 | `deepcopy` of an object "breaks" after a soundex call |
| #3 | a module name disappears from the caller's namespace |
| #15 | `UnicodeEncodeError` on any non-ASCII input |
| #6 | intermittent, unreproducible wrong results |

Six issues, six reporters, six years — **one root cause**. `fuzzy.pyx` line 207:

```cython
cs = s          # char* into a temporary that is freed at end of statement
ls = strlen(cs) # ...and then read
```

A string library corrupting its caller's memory. That is not a bug you fix with
a patch and a test; it is a bug the language permitted. Rust does not permit it,
and not by discipline — `crates/fuzzy` is `#![forbid(unsafe_code)]` and has zero
dependencies, so there is no line in this repo where that class of defect could
be written.

The other memory bug — a guaranteed heap overrun when `size == 0` — is in
[`DECISIONS.md` 03](DECISIONS.md). There are two more, in the C, that nobody had
filed at all: see **New bugs found** below.

---

## The result

**`test_soundex_result` — a test upstream marks `xfail(reason="issue #14")` —
passes against this port, with the test file unmodified.**

```
tests/original/test_fuzzy.py::test_soundex_does_not_mutate_strings PASSED
tests/original/test_fuzzy.py::test_soundex_result                  XPASS
tests/original/test_fuzzy.py::test_soundex_Test                    XFAIL
tests/original/test_fuzzy.py::test_soundex_non_ascii               XFAIL
tests/original/test_fuzzy.py::test_DMetaphone                      PASSED
```

The two remaining xfails stay xfail and we are not claiming them: they assert
`Soundex(8)('Test') == 'T23'` and `Soundex(8)('Jéroboam') == 'J615'`, but
Soundex pads to `size`, so a *correct* implementation returns `'T2300000'`.
Those tests encode wrong expectations, not fixable behavior
([`DECISIONS.md` 02](DECISIONS.md)).

### The test file is provably untouched

Not "diffs clean" — the same git object as upstream's:

```
$ git hash-object tests/original/test_fuzzy.py
a5ff4adb996a60e637c13137488fa43892d0c2bc     # == yougov/fuzzy@e15b195:test/test_fuzzy.py
```

---

## Behavior is preserved, bugs included

This is a port, not a rewrite. Where upstream disagrees with the published
algorithms, **this port agrees with upstream**:

```rust
assert_eq!(dmetaphone("cent", 0)?.0.unwrap(),    "KNT");   // Philips: SNT
assert_eq!(dmetaphone("science", 0)?.0.unwrap(), "SKNK");  // Philips: SNS
assert_eq!(nysiis("FLOYD"),                      "FLYD");  // NYSIIS:  FLAD
```

Those are locked in by `upstream_brace_bugs_are_preserved` and
`nysiis_upstream_warts`, so a later "cleanup" fails the build. Anyone who ports
Double Metaphone from its published description instead of from *this repo's C*
disagrees with the library they claim to have ported — quietly, on ordinary
English words.

### New bugs found

Two defects in `src/double_metaphone.c`, neither among the 15 open issues, both
found by reading the C against Philips' algorithm and confirmed against the
compiled original:

1. **Soft C is coded `K`** — a stray pair of braces rebinds an `else` from the
   inner `if` to the outer one, so Pierce's rule fires for every `C` that is not
   `CC`. The `CI`/`CE`/`CY` branch is unreachable and the cursor over-advances,
   swallowing the next letter — and the over-advance hits hard C too, so
   `click` → `KK`, losing the `L`. `cent` → `KNT`, `city` → `KT`.
2. **`SC` not followed by `H` falls through entirely** — a missing closing brace
   puts the `SC`+I/E/Y and default `SK` arms inside a block whose every path
   already `break`s. Dead code. `science` → `SKNK`, `scissors` → `SKSR`.

Both reports are written up in [`upstream-issues/`](upstream-issues/) with
reproducers and fixes, every expected value checked against an independent
implementation. Deliberately **not** fixed in this port
([`DECISIONS.md` 04, 05](DECISIONS.md)).

### How much does this actually matter?

`python fuzz/drift.py` runs the 10,000 most frequent English words through the
original C and through an independent Double Metaphone, with the reference
truncated to four characters so upstream's documented cut is not counted:

```
fuzzy disagrees with the published algorithm on 851 of them (8.5%)

    454  soft C coded K
    362  C cursor over-advance eats the next letter
     16  word-final CH matches the padding
     19  other

  services   fuzzy=SRFK  published=SRFS
  click      fuzzy=KK    published=KLK
  city       fuzzy=KT    published=ST
  price      fuzzy=PRK   published=PRS
```

**One ordinary English word in twelve.** That is the number behind every
"keep the bug" decision here: a port written from the algorithm's description
would pass its own tests, look right in review, and disagree with the library
it claims to have ported on 8.5% of real input. The differential fuzzer is what
makes that failure mode impossible.

---

## Equivalence, measured

`fuzz/harness.py` runs the port against the original C — `double_metaphone.c`
compiled unmodified, SHA-256 verified — and against `nysiis()` extracted
verbatim from `fuzzy.pyx`.

| Surface | Reference | Inputs | Divergences |
|---|---|---|---|
| Double Metaphone | the original C, compiled | 1,172,000 | **0** |
| NYSIIS | the original `.pyx` routine | 358,000 | **0** |
| Soundex | *excluded — upstream is UB* | — | — |

Captured in [`fuzz/log.txt`](fuzz/log.txt) (seed 20260801). An earlier run at a
different seed added 1,354,000 more Double Metaphone inputs, also clean.

Soundex is excluded on purpose and the exclusion is stated everywhere it
matters: its upstream behavior is a read-after-free, so there is nothing stable
to compare against. A narrow claim that holds beats a broad one that does not
([`DECISIONS.md` 17](DECISIONS.md)).

The corpus is seeded with **every string literal in the C source** — the branch
predicates themselves — plus single-edit mutations of them. Random ASCII almost
never reaches `ORCHES`, `UCCEE` or `EWSKI`; mutations of those literals reach
them constantly.

---

## Build and run

```bash
cargo build --release
printf 'DMETAPHONE\t0\tmayer\n' | ./target/release/fuzzy
# OK	MR	NULL
```

Or with Docker — the runtime stage contains no Python, which is what makes the
rule-05 claim checkable rather than assertable:

```bash
docker build -t fuzzy-rs .
docker run --rm -i fuzzy-rs --help
```

### As a library

```rust
use fuzzy::{soundex, nysiis, dmetaphone};

soundex("fuzzy", 4)?;        // "F200"
nysiis("fuzzy");             // "FASY"
dmetaphone("fuzzy", 0)?;     // (Some("FS"), None)
```

Zero dependencies. `#![forbid(unsafe_code)]`.

### Verify every claim on this page

```bash
python verify.py             # ~2 min    (--quick for ~20s)
```

Checks the test-suite hash and git blob id, the absence of any Python linkage,
the absence of `unsafe`, `cargo test`, the upstream suite, and a fresh
differential fuzz of both surfaces. Exits non-zero if any claim is false.

---

## Layout

```
crates/fuzzy/            the library — #![forbid(unsafe_code)], no dependencies
  src/dmetaphone.rs      the 1,200-line C, transliterated branch for branch
  src/nysiis.rs          from fuzzy.pyx
  src/soundex.rs         the one deliberate divergence — see DECISIONS.md 02
crates/fuzzy-cli/        one binary, three consumers: shim, fuzzer, benchmarks
tests/original/          upstream's test_fuzzy.py, byte-identical
  shim/fuzzy.py            satisfies `import fuzzy` over a pipe, not a link
oracle/                  the original C + its driver; the differential reference
fuzz/harness.py          differential fuzzer
bench/                   methodology and results
DECISIONS.md             22 entries — every non-obvious call and its cost
```

## License

MIT, as upstream. `oracle/double_metaphone.c` and
`tests/original/test_fuzzy.py` are unmodified files from `yougov/fuzzy`,
included under the same license for reference and verification.

# Technical Report — porting `yougov/fuzzy` to Rust

**Project:** fuzzy-rs
**Event:** Port Mortem 2026, Track H (open pair) — Python + Cython + C → Rust
**Source:** [`yougov/fuzzy`](https://github.com/yougov/fuzzy) @ `e15b195`, MIT
**Window:** kickoff 2026-07-31 18:00 UTC → code freeze 2026-08-03 18:00 UTC

---

## The short version, in plain language

`fuzzy` is a small Python library that turns a word into a code representing how
it *sounds*, so that `Smith` and `Smyth` come out the same. Databases use this
to match names that are spelled differently. It has been on PyPI for years and
has 51 stars.

It also has 15 open bug reports, no release since 2017, and — as it turns out —
six of those bug reports are the same bug wearing six different hats. The
library corrupts the memory of whatever program calls it. Not occasionally: on
every call, by design, because of one line written in a language that allowed it.

We rewrote it in Rust, where that line cannot be written.

The hard part was **not** making it work. The hard part was making it work
*exactly as wrongly as the original does*. A port that quietly fixes bugs is
useless to anyone with an existing database — every name they indexed under the
old codes would stop matching. So we kept the bugs. All of them. On purpose,
with tests that fail the build if someone later "cleans them up."

Along the way we found two bugs in the original that nobody in six years had
reported, measured that they corrupt the output for **one English word in
twelve**, and wrote both up for upstream.

The result runs the original project's own test suite — not a copy, the same
file, hash-verified untouched — and passes a test the original marks as broken.

---

## Part 1 — The starting point

### What we were told we were porting

The upstream README describes a Python phonetics library. Track H is listed as
"Python → Rust." Both are, strictly, wrong.

```
src/double_metaphone.c   26,965 B   Maurice Aubrey's 1999 C — the real engine
src/fuzzy.pyx             5,301 B   Cython: Soundex + NYSIIS + a C wrapper
test/test_fuzzy.py           738 B   the entire test suite — 5 tests, 3 xfail
```

**In plain terms:** the "Python library" is 27 KB of 1999-era C with a thin
Python-ish jacket on it. Almost none of the behavior we had to reproduce lives
in Python at all.

**Technically:** `fuzzy.pyx` is Cython — a Python/C hybrid that compiles to a C
extension module. It implements Soundex and NYSIIS itself and delegates Double
Metaphone to `double_metaphone.c` through a C call. So the real port pair is
**C + Cython → Rust**, and roughly 80% of the behavioral surface is the C file.

### The state of the code we inherited

15 open issues. Read individually they look like a scattered mess. Read as a
group they collapse:

| Issue | Reported symptom |
|---|---|
| #14, #17, #20 | `Soundex(4)('FancyFree')` returns `''`, or garbage, or works fine |
| #4 | `deepcopy` of an unrelated object "breaks" after a soundex call |
| #3 | a module name vanishes from the caller's namespace |
| #15 | `UnicodeEncodeError` on any non-ASCII input |
| #6 | intermittent, unreproducible wrong results |

Six issues. Six reporters. Six years. **One line.** `fuzzy.pyx:207`:

```cython
cs = s          # char* borrowed from a temporary, freed at end of statement
ls = strlen(cs) # ...and then read
```

**In plain terms:** the code asks Python for the raw bytes of a string, Python
hands over a pointer to a scratch buffer, Python immediately throws the buffer
away, and the code then reads it anyway — and *writes* to it. Whatever memory
Python has since reused for something else gets scribbled on. That is why an
unrelated `deepcopy` breaks, and why the same input gives different answers on
different runs.

**Technically:** a use-after-free with a write, in a library that returns
attacker-influenced-length output into that freed region. `Soundex` also has a
second, independent defect — a guaranteed heap overrun when `size == 0`
([`DECISIONS.md` 03](DECISIONS.md)).

This is the entire rationale for the port. It is not a bug you patch. It is a
bug the language permitted, and the fix is a language that does not permit it.

### The two constraints that shaped everything

**Rule 05 — no source-language runtime.** A Python → Rust port may not link the
Python interpreter. That forbids PyO3, the obvious tool for the job.

**Rule 02 — the original test suite must run unmodified.** Its files were hashed
at kickoff.

These are in direct conflict. `test_fuzzy.py` is pytest doing `import fuzzy`
against a C extension module. A Rust crate cannot satisfy that import without
PyO3 — which rule 05 forbids. Resolving this was the first architectural
decision and the one everything else hangs off.

---

## Part 2 — How the project was built, phase by phase

### Phase 1 — Establish ground truth before writing any Rust

**In plain terms:** before you can prove your copy behaves like the original,
you need a working original to compare against. That turned out to be the
riskiest part of the whole project.

**Technically:** the plan flagged building the 2017 original as the critical
path — Cython plus a Python-2-era `setup.py`, and issues #18/#19/#21 are *all*
installation failures. The fallback was to difference against a third-party
implementation instead, which would have badly weakened the equivalence claim:
you would be proving your port matches *somebody else's* library, not this one.

We avoided both. The insight was that we never needed the Python package at all,
only the *behavior*:

- **Double Metaphone** — compile `double_metaphone.c` **unmodified** with MinGW
  gcc, plus a small `dm_driver.c` that reproduces exactly what `fuzzy.pyx` does
  to the C's output (collapsing an absent secondary code to `None`). This is not
  a model of the original engine. It **is** the original engine, byte for byte,
  SHA-256 verified.
- **NYSIIS** — `nysiis()` in `fuzzy.pyx` is pure Python that never crosses into
  C, so extracting it verbatim into `oracle/nysiis_oracle.py` loses no fidelity.
- **Soundex** — deliberately no oracle. See Phase 4.

**Result:** no Docker, no 2017 toolchain, no Cython build, and a *stronger*
reference than a successful `pip install` would have given us.

> One trap worth recording: the NYSIIS oracle must call
> `reconfigure(encoding='utf-8')` or Windows cp1252 silently mangles every
> non-Latin-1 input, and you spend an hour debugging "divergences" that are your
> own terminal.

### Phase 2 — Resolve the rule 05 / rule 02 conflict

**In plain terms:** the original's tests need to `import fuzzy` and get a
working library back. We are not allowed to give them a Rust library pretending
to be a Python module. So we gave them a tiny Python file that talks to the Rust
program through a pipe — like one program phoning another instead of merging
with it.

**Technically:** `tests/original/shim/fuzzy.py` (133 lines) exposes the exact
upstream API surface — `DMetaphone()`, `Soundex(n)`, `nysiis()` — and services
every call by writing a line to a long-lived `fuzzy` subprocess and reading a
line back. A subprocess boundary is a *process* boundary, not a link: the port
artifact contains zero Python and no interpreter is loaded into it.

The shim reproduces the original's return types exactly, including its warts —
`DMetaphone()` returns `[b'MR', None]` (bytes, per issue #13, a known ugliness
we preserve), `Soundex(n)` returns `str`.

That claim is made checkable rather than asserted, by a Dockerfile whose runtime
stage has no interpreter in it at all:

```
$ docker run --rm --entrypoint sh fuzzy-rs -c 'command -v python python3 || echo no python'
no python
$ docker run --rm --entrypoint sh fuzzy-rs -c 'ldd /usr/local/bin/fuzzy'
libgcc_s.so.1, libc.so.6        # the entire dependency set
```

**Validated against the organizers.** Their published Q&A confirms it directly:
*"If you keep the original tests exactly as they are and run them against your
build through a thin wrapper, that counts as the original test suite passing
unchanged. That is the best proof."* Asked specifically about Python → Rust,
they endorsed *"the original pytest files run without changes against your Rust
port"* — naming PyO3 as one example mechanism, not a requirement. Our subprocess
route satisfies the same criterion while staying clear of the rule 05 conflict
entirely, which makes it the safer architecture whichever way that rule is read.

### Phase 3 — Transliterate, don't reimplement

**In plain terms:** we did not read what Double Metaphone is *supposed* to do
and write that. We copied the original code's structure decision for decision,
including the parts that are wrong.

**Technically:** `crates/fuzzy/src/dmetaphone.rs` (884 lines) follows the C
branch for branch. Where the C has a `switch` arm, the Rust has the same arm in
the same order with the same predicates. Cursor arithmetic stays **signed**
([`DECISIONS.md` 16](DECISIONS.md)) because the C indexes with `int` and relies
on out-of-range reads returning the pad character; making it `usize` would have
been more idiomatic and would have changed behavior at the boundaries.

This is the discipline that separates a port from a rewrite, and it is what
Phase 5 later proved was necessary.

Deliberately preserved oddities include:

- The five-space input pad that makes end-of-word match `"ch "` phrase rules.
- Upstream's 4-character truncation of Double Metaphone output (issue #5).
- NYSIIS's `AY` mapping sitting in the wrong table (issue #8) and its `MAC`
  window quirk.
- `Ç` and `Ñ` handling, ported even though the encoding path means nothing can
  actually reach it ([`DECISIONS.md` 09](DECISIONS.md)).

### Phase 4 — The one place we refused to be bug-compatible

**In plain terms:** you cannot faithfully copy a bug that behaves differently
every time you run it.

**Technically:** upstream `Soundex` is undefined behavior. It returns `''` or
garbage nondeterministically (#14) and raises `UnicodeEncodeError` on non-ASCII
(#15). There is no stable output to be equivalent *to*. So:

- We implement Soundex **correctly** — standard algorithm, padded and truncated
  to `size` ([`DECISIONS.md` 02](DECISIONS.md)).
- We **exclude** Soundex from the claimed differential-fuzz surface, and state
  that exclusion in the README, `DECISIONS.md`, `fuzz/log.txt` and the demo
  script ([`DECISIONS.md` 17](DECISIONS.md)). A narrow claim that holds beats a
  broad one that does not.
- Non-ASCII still raises where upstream raises ([`DECISIONS.md` 12](DECISIONS.md)),
  because *that* part is deterministic and therefore portable.

The consequence is the project's single strongest artifact, and it is worth
being precise about it. `test_soundex_result` is marked
`xfail(reason="issue #14")` upstream. Against this port it **XPASSes**: the
original's own test, unmodified, for a bug the original gave up on, passes.

We are equally precise about what we did *not* fix. Two tests stay `xfail` and
we claim neither:

```python
test_soundex_Test       expects Soundex(8)('Test')     == 'T23'
test_soundex_non_ascii  expects Soundex(8)('Jéroboam') == 'J615'
```

Soundex pads to `size`, so a *correct* implementation returns `'T2300000'`.
Those tests encode wrong expectations, not fixable behavior. An earlier draft of
the plan predicted three XPASSes; one is the honest number, and the README, the
decision log and the demo script all say so in those words.

### Phase 5 — Prove equivalence instead of asserting it

**In plain terms:** we generated millions of random and near-miss words, fed
each one to both the original and the port, and compared the answers. Nothing
disagreed.

**Technically:** `fuzz/harness.py` drives both binaries over the same line
protocol and diffs the responses.

| Surface | Reference | Inputs | Divergences |
|---|---|---|---|
| Double Metaphone | the original C, compiled unmodified | 1,172,000 | **0** |
| NYSIIS | the original `.pyx` routine, extracted | 358,000 | **0** |
| Soundex | *excluded — upstream is UB* | — | — |

Cumulative across seeds and sessions: **2.5M+ inputs, zero divergences.**

The corpus design is the part that matters. Random ASCII essentially never
generates `ORCHES`, `UCCEE` or `EWSKI` — so a naive fuzzer never reaches the
branches that test for them, and reports a clean run that proves nothing. Ours
seeds the corpus with **every string literal in the C source** — the branch
predicates themselves — plus single-edit mutations of them. Mutations of the
literals hit those arms constantly.

> Harness trap worth recording: batching requests deadlocks. The child's replies
> fill its stdout pipe before it has drained our stdin. It runs in lockstep at
> 7–10k inputs/s instead, which is plenty.

### Phase 6 — Find out how much the drift actually costs

**In plain terms:** we already knew the original had bugs. What we did not know
was whether they mattered on real words. They do — badly.

**Technically:** `fuzz/drift.py` runs the 10,000 most frequent English words
through the original C and through the independent `metaphone` PyPI package,
truncating the reference to four characters so upstream's *documented* cut is
not counted as drift.

```
fuzzy disagrees with the published algorithm on 851 of 10,000 (8.5%)

    454  soft C coded K
    362  C cursor over-advance eats the next letter
     16  word-final CH matches the padding
     19  other

  services   fuzzy=SRFK  published=SRFS
  click      fuzzy=KK    published=KLK
  city       fuzzy=KT    published=ST
  price      fuzzy=PRK   published=PRS
```

**One ordinary English word in twelve.** This number retroactively justifies the
entire Phase 3 discipline: a port written from the algorithm's description would
pass its own tests, look correct in review, and silently disagree with the
library it claims to have ported on 8.5% of real input.

Two of the three drift causes turned out to be *new bugs*, and one turned out
not to be:

1. **Soft C is coded `K`** (`double_metaphone.c` ~line 384). yougov wrapped the
   inner `if` of the `CC` test in braces, so the `else` carrying Pierce's rule
   rebinds from the inner `if` to the outer one and fires for every `C` that is
   not `CC`. The `CK`/`CG`/`CQ`, `CI`/`CE`/`CY` and default arms became
   unreachable. The cursor also over-advances, swallowing the next letter — and
   that part hits hard C too, so `click` → `KK`, losing the `L` entirely.
2. **`SC` not followed by `H` falls through** (~line 935). The brace closing
   `if (GetAt(current+2) == 'H')` is missing, so the `SC`+I/E/Y and default `SK`
   arms sit inside a block in which both paths already `break`. Dead code.
   `science` → `SKNK`.
3. **Word-final `CH` codes `K`** (`such` → `SK`). We checked this one and did
   **not** file it: it is in Aubrey's 1999 C unmodified, so it is upstream
   behavior rather than a yougov regression. Documented in
   [`DECISIONS.md` 20](DECISIONS.md) instead.

Both filed bugs are written up in `upstream-issues/` with reproducers and fixes.
Every "expected" value in those reports was verified against the `metaphone`
package rather than asserted from the algorithm description — because the whole
project is a lesson in how far a description drifts from an implementation.

Both are preserved in the port, locked by
`upstream_brace_bugs_are_preserved`, so a future cleanup fails the build.

### Phase 7 — Make every claim executable

**In plain terms:** a judge should not have to trust the README. One command
should re-prove all of it, and fail loudly if anything is false.

**Technically:** `python verify.py` — 11 checks, non-zero exit if any claim on
the README is untrue:

```
Provenance         test-suite SHA-256 and git blob id vs upstream@e15b195
Rule 05            no PyO3/cpython in Cargo.lock; no third-party deps at all
Zero-unsafe        #![forbid(unsafe_code)] on every crate root; no unsafe token
Functionality      cargo test; pytest tests/original with 0 failures
                   test_soundex_result XPASSes
Equivalence        fresh differential fuzz of both surfaces, 0 divergences
```

The provenance check is stricter than a diff — it compares the **git blob id**,
so the file is provably the same object as upstream's, not merely similar:

```
$ git hash-object tests/original/test_fuzzy.py
a5ff4adb996a60e637c13137488fa43892d0c2bc   # == yougov/fuzzy@e15b195:test/test_fuzzy.py
```

`.gitattributes` is load-bearing here ([`DECISIONS.md` 19](DECISIONS.md)) — on
Windows, git's line-ending normalization would rewrite the file and break the
hash.

### Phase 8 — Close the gaps found in review

Two things were tightened after the main build, and both are worth recording
because they are the kind of gap that survives into a submission unnoticed.

**The Dockerfile had never been built.** It was the one artifact making a claim
nothing had checked. Now built green (Docker 29.6.2, 118 MB), with the no-Python
and `ldd` evidence above captured. Linux output matches Windows byte for byte,
brace bugs included — which also demonstrates the port is not relying on
platform quirks to reproduce them.

**The zero-unsafe guarantee only covered half the artifact.**
`#![forbid(unsafe_code)]` was on `crates/fuzzy` but not on `crates/fuzzy-cli` —
and the CLI is the binary a judge actually runs. Worse, `verify.py` graded it by
grepping for the token `"unsafe "`, which a crate that never banned unsafe
passes trivially. Both fixed: the attribute is now on **every crate root**, and
the check requires it there rather than grepping. `forbid` (not `deny`) matters
— it covers all submodules and cannot be switched back off by an inner
`#[allow]`.

The new check was negative-tested, because an unverified check is worse than no
check:

```
as-is        -> (2 roots, [])
attr removed -> (2 roots, ['crates/fuzzy-cli/src/main.rs'])   # caught
restored     -> (2 roots, [])
```

---

## Part 3 — Before and after, measured

### Safety

| | upstream | port |
|---|---|---|
| Use-after-free on every `Soundex` call | yes (`fuzzy.pyx:207`) | **structurally impossible** |
| Heap overrun at `size == 0` | yes | **structurally impossible** |
| `unsafe` blocks | n/a — the whole thing is C | **0** |
| Enforcement | code review | `#![forbid(unsafe_code)]`, every crate root |
| Third-party dependencies | Cython toolchain | **0** |
| Issues resolved by construction | — | **#3, #4, #6, #14, #17, #20** |

Six of the fifteen open issues are closed not by a patch but by the choice of
language. There is no line in this repository where that defect class could be
written, and `forbid` means that is compiler-enforced rather than a convention.

### Correctness

| | upstream | port |
|---|---|---|
| `test_soundex_result` | `xfail`, issue #14 | **XPASS** |
| Suite result | 2 passed, 3 xfailed | **2 passed, 1 xpassed, 2 xfailed, 0 failed** |
| Test file | — | byte-identical, blob id verified |
| Behavioral equivalence evidence | none | 2.5M+ fuzz inputs, 0 divergences |
| Known-bug documentation | 15 open issues | 22 decision entries + 2 new bugs filed |

### Performance

`python bench/run.py`, 14,590-word corpus, 3 iterations, methodology in
`bench/methodology.md`. Reported in both directions, including where we lose:

| | port | original C | |
|---|---|---|---|
| Algorithm, in-process | 2,041,787 w/s | 1,215,833 w/s | **1.68× faster** |
| Via the line protocol | 133,260 w/s | 196,706 w/s | **32% slower** |
| Startup, median | 20.15 ms | 19.55 ms | a wash |
| Peak RSS | 4.00 MB | 4.81 MB | **17% lower** |
| Binary size | 201,728 B | 153,988 B | 31% larger |

The honest reading: **the algorithm is faster, the transport is slower.** The
32% pipe deficit is the price of the subprocess boundary that keeps Python out
of the artifact — a deliberate trade, and one that vanishes entirely for anyone
consuming the crate as a library. The startup difference is dominated by Windows
process spawn and is noise, not a result. We report it anyway; a benchmark table
with no losses in it is a marketing document.

### Size

| | lines |
|---|---|
| Upstream: C + header + Cython | 1,494 |
| Port: Rust | **1,457** |

Near parity — 2.5% smaller — while absorbing the C *and* the Cython glue layer
*and* carrying `#![forbid(unsafe_code)]` with zero dependencies.

---

## Part 4 — What we deliberately did not do

Recorded because the omissions were choices, not oversights
([`DECISIONS.md` 22](DECISIONS.md)):

- **We did not fix the two new bugs in the port.** Fixing them would break
  behavioral equivalence on 8.5% of English words and invalidate every index
  built with the original. They are filed upstream instead, where fixing them is
  a versioning decision for the maintainers.
- **We did not claim Soundex equivalence.** No stable oracle exists.
- **We did not file word-final `CH` as a third bug.** It is in Aubrey's original,
  not a yougov regression.
- **We did not use PyO3**, even though the organizers named it as an acceptable
  mechanism, because rule 05 as written forbids linking the source runtime and
  the subprocess route satisfies the same criterion without the conflict.
- **We did not pad the port with abstractions** to reach a larger line count.
  The plan flagged the repo as small against the preferred size band; the
  response was to invest in evidence and documentation, not in scope.

---

## Part 5 — Honest limitations

- **Soundex has no differential coverage.** Justified, documented, still a gap
  in the fuzz surface: two of three algorithms are fuzzed, not three.
- **The benchmark is single-platform** (win32). The Docker build shows the port
  behaves identically on Linux, but the timings are not re-measured there.
- **The port's own Rust test suite is thin** — 6 `#[test]`s. The upstream suite
  and the fuzzer carry the correctness argument; a named edge-case suite
  (empty string, `size == 0`, the padding boundary, per-branch brace-bug
  coverage) is the clearest remaining improvement, and the organizers explicitly
  asked for a short note on edge cases covered for library ports.
- **`drift.py` depends on the `metaphone` PyPI package** as its independent
  reference. That is a third-party implementation and could itself be wrong;
  the specific expected values in the bug reports were spot-checked by hand
  against Philips' published rules for this reason.

---

## Appendix — commit history

```
7c845a1  Extend the zero-unsafe guarantee to the binary crate
2a89646  Add the 5-minute demo runsheet
4667e56  Verify the Dockerfile and record the rule-05 evidence
bd05370  Quantify upstream drift and draft the two bug reports
2e4771d  Add differential fuzzing, benchmarks, docs and one-command verification
f23f4ee  Port yougov/fuzzy to Rust: Soundex, NYSIIS, Double Metaphone
```

## Appendix — reproducing every claim in this report

```bash
python verify.py            # 11 checks, ~2 min, non-zero exit on any false claim
python fuzz/drift.py        # the 8.5% figure
python bench/run.py         # the performance table
docker build -t fuzzy-rs .  # the rule-05 evidence
```

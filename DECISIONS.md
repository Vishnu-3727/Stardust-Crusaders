# Decision log

Every non-obvious call made porting `yougov/fuzzy` to Rust, and why.

The governing principle, stated once so the rest of this document makes sense:

> **The implementation is rewritten 100%. The behavior is rewritten 0%.**

Where upstream disagrees with the textbook algorithm, this port agrees with
*upstream*. Three of the entries below are bugs we went out of our way to keep.
One entry — Soundex — is the single place we could not, and it gets the longest
justification for that reason.

---

## 01 — Running the original test suite without linking Python

**Problem.** Two rules pull in opposite directions. Rule 02 wants the original
test suite to run unmodified, and `test/test_fuzzy.py` is pytest doing
`import fuzzy`. Rule 05 says a Python → Rust port "cannot link against the
Python interpreter", which rules out PyO3 — the one tool that would make
`import fuzzy` resolve to Rust code directly.

**Decision.** The port is a plain binary. `tests/original/shim/fuzzy.py` is a
pure-Python module that satisfies the import by talking to that binary over a
pipe, one line per call.

**Why this is not a rule-05 loophole.** Linking means the artifact contains or
depends on the interpreter. Ours does not: no PyO3, no `libpython`, no CPython
ABI, no Python in `Cargo.lock` (`verify.py` asserts all four), and the
Dockerfile's runtime stage has no interpreter installed at all. The shim lives
under `tests/`, ships in nothing, and computes nothing — every value it returns
came out of the Rust binary. The relationship is the same one a judge has when
they run `./fuzzy` from a terminal.

**Cost, stated plainly.** A subprocess round trip is ~40 µs against a Cython
call's ~1 µs. For a test suite of five assertions this is invisible; for a
caller doing millions of lookups it would not be, which is why the *library*
API is the crate and the CLI is glue. See `bench/methodology.md`.

**Result.** `tests/original/test_fuzzy.py` is byte-identical to upstream — not
merely equal in content but the same git object, `a5ff4adb996a…`, the hash in
yougov/fuzzy's own history. `pytest tests/original` reports **2 passed,
1 xpassed, 2 xfailed, 0 failed**.

---

## 02 — Soundex: the one place we refused to be bug-compatible

**What upstream does.** `fuzzy.pyx`:

```cython
def __call__(self, s):
    cdef char *cs
    ...
    cs = s              # <-- here
    ls = strlen(cs)
```

The module is compiled `c_string_type=unicode, c_string_encoding=ascii`, so
`cs = s` encodes `s` to a *temporary* bytes object and stores a pointer into
it. The temporary's refcount hits zero at the end of that statement. Every
subsequent read — starting with `strlen` — touches freed memory.

**Why this is unportable.** The output is not wrong, it is *undefined*. It
depends on what the allocator does with that block between the free and the
read. Upstream issues #14, #17 and #20 are three people reporting three
different symptoms of it; #3 and #4 are the same corruption seen from the
outside, and `test_soundex_does_not_mutate_strings` exists because someone
suspected it. Reproducing a read-after-free in a language whose entire premise
is that you cannot have one is not a porting problem — it is a category error.
A port that returned `''` to match the common case would be hardcoding one
observation of a nondeterministic bug and calling it a specification.

**Decision.** Implement what the code *computes* when the pointer happens to
still be valid — the coding rules exactly as written — and document the
divergence here, loudly, rather than hide it.

**Consequence, and it is the best artifact in this repo.** The upstream suite
contains:

```python
@pytest.mark.xfail(reason="issue #14")
def test_soundex_result():
    assert fuzzy.Soundex(4)('FancyFree') == 'F521'
```

Against the port this **XPASSes**. A test upstream marked as known-broken now
passes, with the test file unmodified.

**Honesty about the other two xfails.** They stay xfail, and the plan we
started from was wrong to predict otherwise. `test_soundex_Test` asserts
`Soundex(8)('Test') == 'T23'`, but Soundex pads to `size` — a *correct*
implementation returns `'T2300000'`. `test_soundex_non_ascii` asserts
`Soundex(8)('Jéroboam') == 'J615'`, which is both the wrong width and an input
that raises before any coding happens (entry 12). Those two tests encode wrong
expectations, not fixable behavior. We are not going to claim credit for them.

---

## 03 — Soundex's non-textbook rules are kept

Having diverged on the memory bug, we diverge on nothing else. Three upstream
quirks that a from-scratch Soundex would not have, all preserved:

* **The first digit is never suppressed.** The dedup test is
  `written == 1 || out[written-1] != code`, so a second letter whose code
  matches the initial letter's still gets emitted. `PFISTER` → `P123`, where
  the textbook says `P236`.
* **The output is padded to exactly `size`, and `size` is a hard cap** rather
  than the conventional 4. `Soundex(8)('Test')` is `'T2300000'`.
* **H and W do not separate duplicates.** Deduplication looks only at adjacent
  *output* digits.

`size == 0` is the one input where the original overruns its own buffer: it
allocates `size + 1` bytes and the `written == size` break can never fire,
because `written` is already 1 the first time it is tested. The C then writes
its terminator to `out[0]` and returns `''`. We return `''` too — and do not
reproduce the heap smash. `crates/fuzzy/src/soundex.rs` says so at the line.

---

## 04 — Double Metaphone: the soft-C brace bug (kept, and reported upstream)

`src/double_metaphone.c`, in `case 'C'`:

```c
if (StringAt(original, current, 2, "CC", "")
    && !((current == 1) && (GetAt(original, 0) == 'M')))
{                                    /* <-- these braces are the bug */
    if (StringAt(original, (current + 2), 1, "I", "E", "H", "") && ...)
      {
          ...
          current += 3;
          break;
      }
}
else
  {       /* Pierce's rule */
  MetaphAdd(primary, "K");
  current += 2;
  break;
  }
```

In Lawrence Philips' original the inner `if` has no braces, so `else` binds to
it: Pierce's rule is the fallback for a `CC` *not* followed by I/E/H. Here the
braces rebind `else` to the **outer** `if`, so it fires for every `C` that is
not a double `C`.

Two consequences, both live:

1. Every soft C is coded `K`. The `CI`/`CE`/`CY` arm below it is unreachable.
2. The cursor advances by 2 instead of 1, swallowing the following letter.

```
cent   -> KNT   (Philips: SNT)
city   -> KT    (Philips: ST)
ciao   -> K     (Philips: S, secondary X)
```

The `CK`/`CG`/`CQ` arm and the default arm are unreachable too, except on the
narrow path where the outer test matched and the inner one did not.

**Decision: reproduce it exactly.** Behavioral equivalence is 30% of the score
and this is the single most likely place for a competitor to "port" the
algorithm from Wikipedia and silently disagree with the library they claim to
have ported. `crates/fuzzy/src/dmetaphone.rs` keeps the dead arms in place,
commented as dead, so the structure still maps to the C line for line — and
`upstream_brace_bugs_are_preserved` in `crates/fuzzy/src/lib.rs` fails if
anyone later "fixes" it.

**This bug is not in any of the 15 open upstream issues.** Report written
during the event; see entry 18.

---

## 05 — Double Metaphone: the SC fallthrough brace bug (kept, and reported upstream)

Same file, `case 'S'`. The brace that should close
`if (GetAt(original, current + 2) == 'H')` is missing, so the `SC`+I/E/Y arm and
the default `SK` arm sit *inside* that block — and both arms of the inner
if/else already `break`. They are unreachable.

An `SC` not followed by `H` therefore falls out of the entire block and lands
on the generic S handling, which emits `S` and advances one character — leaving
the `C` to be eaten by the bug in entry 04.

```
science  -> SKNK  (Philips: SNS)
scissors -> SKSR  (Philips: SSRS)
scene    -> SKN   (Philips: SN)
school   -> SKL   (correct — the SCH path is live)
```

Reproduced for the same reason as entry 04, with the dead arms preserved as
comments at the exact place they became unreachable. Also unfiled upstream.

---

## 06 — NYSIIS keeps the `AY` mapping in the wrong table (issue #8)

`_nysiis_transforms` contains `'AY': 'Y'`, applied at any position. It belongs
in `_nysiis_trans_not_first`. The visible effect is upstream issue #8:

```
FLOYD -> FLYD   (NYSIIS as published: FLAD)
```

Kept. The library's users have been matching records against `FLYD` for years;
a "fix" here silently invalidates every stored code they have. This is a
behavior change dressed as a bug fix, which is exactly what a port must not do.

---

## 07 — NYSIIS keeps the `MAC` window quirk

`MAC…` → `MC…` shortens the string by one character, and the code compensates
by decrementing `stop` — once, on a window that was computed before the
rewrite. Textbook NYSIIS recomputes. The upstream behavior is what makes
`MACINTOSH` and `MCINTOSH` agree, which is the property anyone using this for
record linkage actually depends on. Kept, and asserted in `nysiis_upstream_warts`.

---

## 08 — Double Metaphone's 4-character truncation (issue #5)

The C ends with:

```c
if (primary->length > 4)
    SetAt(primary, 4, '\0');
```

It NUL-terminates at 4 without touching `->length`, so the caller sees four
characters. `decentralization` → `TKNT`. Upstream issue #5 reports this as data
loss; it is, and it is also the documented contract of a metaphone code. Kept.

The `size` argument multiplies with it rather than replacing it: `DMetaphone(2)`
gives you the first two characters of an already-truncated four.

---

## 09 — `Ç` and `Ñ` are ported even though nothing can reach them

The C has `case '\xC7':` and `case '\xD1':` — Latin-1 Ç and Ñ. Through the
Python API they are dead: the Cython layer encodes to ASCII and raises first
(entry 12). They are reachable through `dmetaphone_raw`, which takes bytes.

Ported anyway, with a comment saying they are unreachable from Python. Deleting
them would have been defensible; keeping them means the Rust file is a complete
transliteration of the C file, and a reviewer diffing the two is never left
wondering whether a missing branch was a decision or an oversight.

---

## 10 — The differential oracle is the original C, compiled

The obvious reference implementation is "pip install fuzzy" or a pinned 2017
Cython build in Docker. We did neither.

`src/double_metaphone.c` **is** the Double Metaphone implementation — the
Cython layer around it is 30 lines of pointer shuffling. So the oracle is that
file, compiled unmodified (`oracle/double_metaphone.c`, SHA-256 verified
identical to upstream's), driven by `oracle/dm_driver.c`, a 30-line `main` that
reproduces the `None`-collapsing `fuzzy.pyx` does on top of it.

This is strictly *better* than building the 2017 package, not a shortcut around
it:

* No toolchain archaeology on the critical path. Upstream issues #18, #19 and
  #21 are all install failures; that was the single largest schedule risk in
  the plan and it is now zero.
* The comparison is against the actual engine rather than a wheel that may have
  been built from different sources.
* A judge reproduces it with one `gcc` command, on any platform, in a second.

**The honest limitation:** this validates the C engine, not the Cython wrapper.
The wrapper's contribution — `None` collapsing, size slicing, the bytes/str
split — is covered by entries 12 and 13 and by the CLI's own tests, not by the
fuzzer.

---

## 11 — The NYSIIS oracle is the `.pyx` source, extracted

NYSIIS is the one algorithm that never crosses into C. Its `cdef` locals are
only ever bound to Python objects, so Cython compiles it to the same semantics
CPython gives the plain-Python file. `oracle/nysiis_oracle.py` is that function
with the `cdef` declarations removed and nothing else touched — diff it against
`src/fuzzy.pyx` lines 19–185.

**Why this is not circular.** The oracle is derived from the *original source*,
in the original language, by deletion only. The port is an independent
reimplementation in Rust. Comparing them is a real comparison; what would be
circular is generating the oracle *from the port*.

---

## 12 — Non-ASCII raises where upstream raises

`# cython: c_string_encoding=ascii` means `Soundex` and `DMetaphone` raise
`UnicodeEncodeError` on non-ASCII input before any phonetic code runs. That is
upstream issue #15 and it is behavior, not an accident of implementation.

* The Rust API returns `Result<_, AsciiError>`, whose `Display` reproduces
  CPython's message text including the byte position.
* The CLI answers `ERR<TAB>UnicodeEncodeError: …`.
* The shim raises the real `UnicodeEncodeError`, from a real `str.encode`, at
  the same point in the call the Cython-generated code does.

NYSIIS is deliberately exempt in all three layers — it never touches a C
string, so it accepts anything `str` can hold.

---

## 13 — `DMetaphone` returns bytes, `Soundex` returns str (issue #13)

`cdef bytes o1` inside a module declared `c_string_type=unicode`. The result is
that one function in the library hands back `str` and the other hands back
`bytes`, which is upstream issue #13 and which the test suite depends on:

```python
assert m("mayer") == [b'MR', None]
```

The shim reproduces the split exactly. It would have been one line to return
`str` from both and one line to "fix" the test; both were rejected.

---

## 14 — CLI protocol: one binary, three consumers

`ALGO<TAB>SIZE<TAB>WORD` in, `OK<TAB>…` or `ERR<TAB>…` out, flushed per line.
One long-lived process serves the pytest shim, the differential fuzzer and the
benchmark harness. Building three drivers, or a JSON API, or an argument-parser
dependency, would have been three ways to have the same bug in three places.

`NULL` marks an absent metaphone code. It is unambiguous rather than merely
convenient: metaphone codes are drawn from `AEFHJKLMNPRSTX0`, so no real code
can spell `NULL`.

**Known limitation, not worked around:** a line protocol cannot carry a
newline, so `WORD` may not contain one. The library API has no such limit. The
shim raises `ValueError` rather than silently truncating, and the fuzzer strips
newlines from its corpus. Adding an escaping layer to serve an input no caller
has would have been the wrong trade.

---

## 15 — Zero dependencies, zero unsafe

`crates/fuzzy` has an empty `[dependencies]`. Every algorithm here is byte
manipulation over ASCII; the lookup tables are `match` arms rather than a
`HashMap`, which needs no allocation, no hashing and no crate.

`#![forbid(unsafe_code)]` was in the first commit, not retrofitted — `forbid`
rather than `deny` so a module cannot re-allow it later. `verify.py` checks both
the attribute and the absence of the token across all crates.

The C this replaces contains, in ~1,200 lines: one read-after-free, one
guaranteed heap overrun, `strcat` into a manually sized buffer, `va_arg` walked
until a sentinel with `va_end` skipped on the early-return path, and window
predicates that read past the end of the word by design and are made safe only
by a five-space pad. None of that survives translation, and none of it had to
be argued about — the port simply cannot express it.

---

## 16 — Cursor arithmetic stays signed

`current - 4`, `last - 1` and `current - 2` are all legal in the C and all
routinely negative — `StringAt` and `GetAt` bounds-check for it and return 0.
The port keeps the cursor as `i32` and reproduces those guards in `Word::get_at`
and `Word::string_at`, rather than switching to `usize` and scattering
`checked_sub` at 40 call sites.

Using `usize` would have been more idiomatic Rust and would have made the port
wrong in a way that is invisible until a specific word hits a specific branch —
this is the kind of place where "idiomatic" and "equivalent" genuinely conflict,
and equivalence wins.

Reads past the *end* are handled the same way: `get_at` returns 0 beyond the
buffer, which is exactly what `strncmp` sees when it walks into the C string's
NUL terminator.

---

## 17 — The fuzz surface is scoped, and the scope is stated

We claim the differential-fuzz bonus on **DMETAPHONE and NYSIIS**, not on
Soundex. Soundex is excluded because its upstream behavior is undefined
(entry 02) — there is no stable output to compare against, and a "zero
divergences" number for it would mean nothing.

A broad claim a judge can break in one minute is worth less than a narrow one
that holds. The exclusion is in the harness header, in `fuzz/log.txt`, and here.

Corpus design matters more than corpus size: 45% of inputs are single-edit
mutations of a seed set that includes **every string literal extracted from the
C source**, i.e. the branch predicates themselves. Uniform random ASCII barely
reaches `ORCHES`, `UCCEE` or `EWSKI`; mutations of those literals hit them
constantly.

Captured in `fuzz/log.txt` at seed 20260801: **1,172,000 DMETAPHONE inputs and
358,000 NYSIIS inputs, zero divergences.** An earlier run at seed 1 added
1,354,000 more DMETAPHONE inputs, also clean.

The NYSIIS run includes non-ASCII probes chosen to stress the
one place Rust's `to_uppercase` and Python's `str.upper()` could disagree —
characters that uppercase *into* ASCII, like `ß` → `SS` and `ﬁ` → `FI`.

The first version of that run reported 52 divergences. All 52 were the harness
reading its own pipe as cp1252 on Windows. Fixed in `nysiis_oracle.py`; noted
here because "the fuzzer found something" is only useful if you also say what
happened when it did.

---

## 18 — Two bugs reported upstream during the event

Entries 04 and 05 describe defects that are not among the 15 open issues on
`yougov/fuzzy`. Both were found by reading the C against Philips' published
algorithm and confirmed by running the compiled original.

Both reports are written, with a minimal reproducer, the exact brace placement
at fault, and a fix — `upstream-issues/01-double-metaphone-soft-c.md` and
`upstream-issues/02-double-metaphone-sc-fallthrough.md`. Every "expected" value
in them was checked against an independent implementation (the `metaphone`
package on PyPI) rather than asserted from the algorithm description.

They are held locally pending review before posting to a third party's issue
tracker. They are *not* fixed in this port — see entries 04 and 05 for why a
port is the wrong place to fix them.

---

## 19 — `.gitattributes` is load-bearing

`core.autocrlf` is on by default on Windows. Left alone, it rewrites
`tests/original/test_fuzzy.py` on checkout: 738 bytes of LF become 775 bytes of
CRLF, and the SHA-256 the whole rule-02 claim rests on no longer matches a file
nobody edited.

`tests/original/test_fuzzy.py -text` pins the bytes on every platform. We hit
this for real during the port — the first hash recorded in `.port-mortem.toml`
was of the mangled copy — which is why the manifest now records the upstream
**git blob id** as well. That hash cannot match unless the bytes are identical,
and a judge can check it against yougov/fuzzy's own history with one command.

---

## 20 — Word-final `CH` codes `K` because the padding leaks

`case 'C'`'s `CH` branch takes the "germanic /kh/" path when the letter two
positions on is one of `L R N M B H F V W` **or a space** — the space is there
for a `ch ` inside a multi-word string like `van der ch…`.

But `DoubleMetaphone` pads the input with five spaces so the window predicates
can read past the end of the word. A word *ending* in `CH` therefore sees a
space at `current + 2` and takes the branch meant for phrases:

```
such  -> SK   (published: SX)
each  -> AK   (published: AX)
```

Those are very common words, and `X` — the "sh" sound — is the whole point of
the `CH` rule.

**Kept, and not filed upstream.** Unlike entries 04 and 05 this is not a brace
slip: it is in Maurice Aubrey's C as published, it follows directly from the
padding strategy that same code introduces, and a maintainer could reasonably
argue it is intended. Two clear defects reported carefully are worth more than
three with a debatable one attached. It is measured and attributed in
`fuzz/drift.py`.

---

## 21 — Measuring how far upstream has drifted from the algorithm

`fuzz/drift.py` runs the 10,000 most frequent English words through the
original C and through an independent implementation of Double Metaphone (the
`metaphone` package on PyPI), with the reference truncated to four characters so
the documented cut (issue #5) is not counted.

**8.5% of ordinary English words get a different code**, and the two brace bugs
account for 96% of it:

```
  454  soft C coded K                                (entry 04)
  362  C cursor over-advance eats the next letter    (entry 04)
   16  word-final CH matches the pad                 (entry 20)
   19  other
```

This is the number that justifies every "keep the bug" decision in this
document. A port written from the algorithm's description rather than from this
repository's C would pass its own tests, look correct to a reviewer, and
disagree with the library it claims to have ported on roughly one word in
twelve. The differential fuzzer is what makes that impossible here.

It also sharpens what entry 04 costs users: the over-advance is not limited to
soft C. `click` → `KK`, losing the `L`, because the cursor skips a character
after *every* C that is not a doubled `CC`.

---

## 22 — What we did not do

* **No PyO3, no `abi3`, no `#[pymodule]`.** Rule 05.
* **No "modernised" API.** No iterator adaptors over the codes, no `serde`, no
  builder pattern for `Soundex(4)`. The surface is the surface upstream had.
* **No fixing of issues #3, #4, #5, #6, #8, #13, #14, #15, #17, #20** beyond
  what memory safety fixes for free. Nine of those fifteen open issues are
  symptoms of the two memory bugs in entries 02 and 15; they stop reproducing
  because the code that caused them cannot be written in safe Rust. That is the
  migration's actual argument, and it is worth more stated once than claimed
  ten times.
* **No benchmark claim of a speedup on the hot loop.** The original Double
  Metaphone is hand-tuned C. See `bench/methodology.md` for where Rust actually
  wins here and where it does not.

# Benchmark methodology

`python bench/run.py` writes `results.json`. This file says what those numbers
mean, what they do not mean, and where the port is **slower**.

The organisers' rule of thumb — a throughput-only benchmark scores below an
honest p99 regression — is taken literally here.

## What is being compared

Both binaries implement the same `--bench` contract: read the whole corpus into
memory, then time the algorithm with no IO inside the timed region.

| Binary | What it is |
|---|---|
| `target/release/fuzzy` | the port (`lto = true`, `codegen-units = 1`) |
| `oracle/dm_oracle` | **the original** `double_metaphone.c`, compiled `gcc -O2`, unmodified |

The reference is the actual upstream C engine, not a reimplementation and not a
PyPI wheel. Only Double Metaphone is compared — it is the only algorithm the two
share in a form the C can be driven through.

Corpus: 14,590 words from the fuzzer's generator (seed 1234) — the C's own
string literals plus single-edit mutations, so branch coverage is realistic
rather than uniformly random. Median of 3 iterations.

Machine: Windows 11, x86-64. Numbers are for comparison between the two rows on
one machine, not for quoting as absolutes.

## Results

### Algorithm throughput — the port wins, ~1.7x

```
port           2,041,787 words/sec
original C     1,215,833 words/sec     ratio 1.68x
```

This is the only apples-to-apples comparison here, and it is a real result, not
a compiler-flag artifact. The mechanism is allocation: `DoubleMetaphone` builds
three `metastring`s per call, each a `malloc` plus a `strncpy`, then grows the
padded input with a `realloc`, and every `MetaphAdd` is a `strcat` that rescans
the accumulated code from the start. The port allocates two `Vec`s with the
right capacity and appends by index.

**Caveat, stated because it is real:** the C times with `clock()` and the Rust
with `Instant`. For a single-threaded CPU-bound loop on Windows these track each
other closely, but they are not the same clock. Treat 1.68x as "clearly faster",
not as a figure precise to two decimals.

### Pipe throughput — the port loses, and here is why

```
port via pipe        133,260 words/sec
original C via pipe  196,706 words/sec     port is 32% SLOWER
```

Not an algorithm difference — the same algorithm won by 1.7x above. It is
protocol cost: the port's CLI parses three tab-separated fields, validates
UTF-8, dispatches on the algorithm name and handles three algorithms; the C
driver reads a bare word and calls one function.

We are reporting it rather than dropping the row. If it mattered for a real
workload the answer would be to use the crate directly — the in-process number
is 15x the pipe number for both binaries, so at this speed the protocol *is* the
program. It does not matter for our use of it: the pytest shim makes five calls.

### Round-trip latency through the shim

```
p50   28.5 µs
p99  111.5 µs
max    5.4 ms
```

This is what `tests/original/shim/fuzzy.py` pays per call. The `max` is a
scheduler outlier on a laptop under load, and it is left in — it is what
`max` is for. Against a Cython call's ~1 µs this is 30x worse per call, which
is the honest price of satisfying rule 02 without linking Python
([`DECISIONS.md` 01](../DECISIONS.md)).

### Startup

```
port         20.15 ms
original C   19.55 ms
```

A wash, and both are dominated by Windows process creation (~19 ms), not by
either program. **The comparison that would have been interesting is not
available:** against `import fuzzy` in CPython, where interpreter startup plus
extension load is the cost Rust actually eliminates. Measuring that requires a
working 2017 Cython build of the original, which this project deliberately does
not have ([`DECISIONS.md` 10](../DECISIONS.md)) — so no claim is made about it.
The plan we started from expected startup to be a headline win. On this
evidence it is not one, so it is not being sold as one.

### Memory and size

```
peak RSS   port 4.00 MB   original C 4.81 MB
binary     port 197 KB    original C 150 KB
```

RSS is marginally lower, binary marginally larger. Neither is a reason to
choose either implementation.

## What no number here measures

* **The memory-safety argument.** The reason to port this library is upstream
  issues #3, #4, #6, #14, #15, #17 and #20 — one read-after-free and one heap
  overrun, in a library whose job is handling other people's strings. That is
  worth more than 1.7x and it does not appear in a benchmark, because the
  failure mode is corruption, not slowness.
* **Soundex and NYSIIS throughput.** Soundex's upstream implementation is
  undefined behavior, so timing it would be timing a bug. NYSIIS's reference is
  a Python extraction; comparing compiled Rust to interpreted Python would be
  a number with no content.

## Reproducing

```bash
cargo build --release
gcc -O2 -o oracle/dm_oracle oracle/dm_driver.c oracle/double_metaphone.c
python bench/run.py --words 15000 --iterations 3
```

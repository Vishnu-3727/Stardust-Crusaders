#!/usr/bin/env python3
"""Benchmarks: the Rust port against the original C, and against the pipe.

Writes bench/results.json. Read bench/methodology.md before quoting any number
from it — in particular, the throughput comparison is not a claim that Rust
beats hand-tuned C at this workload. It does not, and saying so is the point.

    python bench/run.py [--iterations 5] [--words 20000]
"""

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "fuzz"))
import harness  # noqa: E402  — reuse the fuzzer's corpus generator

EXE = ".exe" if sys.platform == "win32" else ""
PORT = ROOT / "target" / "release" / f"fuzzy{EXE}"
ORACLE = ROOT / "oracle" / f"dm_oracle{EXE}"


def corpus(n, seed=1234):
    import random
    rng = random.Random(seed)
    seeds = harness.SEEDS + harness.literals_from_c(ROOT / "oracle" / "double_metaphone.c")
    words = []
    for w in harness.make_corpus(rng, seeds, n):
        w = w.replace("\n", "").replace("\r", "").replace("\t", "")
        if w:
            words.append(w)
    return words


def pipe_throughput(argv, words, prefix=""):
    """Whole-process wall time to answer every word over the line protocol."""
    payload = "".join(prefix + w + "\n" for w in words).encode("ascii")
    start = time.perf_counter()
    p = subprocess.run(argv, input=payload, stdout=subprocess.PIPE)
    elapsed = time.perf_counter() - start
    assert p.returncode == 0, p.returncode
    return len(words) / elapsed


def in_process_throughput(words, argv):
    """Algorithm cost alone — corpus loaded first, no IO in the timed region.

    Both binaries implement the same `--bench` contract, so these two numbers
    are the only apples-to-apples comparison in this file.
    """
    payload = "".join(w + "\n" for w in words).encode("ascii")
    p = subprocess.run(argv, input=payload, stdout=subprocess.PIPE, check=True)
    count, nanos, _sink = p.stdout.decode().split()
    return int(count) / (int(nanos) / 1e9)


def startup_latency(argv, probe, iterations):
    """Cold process start to first answer — spawn included."""
    samples = []
    for _ in range(iterations):
        start = time.perf_counter()
        subprocess.run(argv, input=probe.encode("ascii"), stdout=subprocess.PIPE, check=True)
        samples.append((time.perf_counter() - start) * 1000)
    return samples


def peak_rss_mb(argv, words, prefix=""):
    """Peak working set, via the OS. None if we cannot measure it here."""
    payload = "".join(prefix + w + "\n" for w in words).encode("ascii")
    if sys.platform == "win32":
        try:
            import ctypes
            from ctypes import wintypes
        except ImportError:
            return None
        p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL)
        p.stdin.write(payload)
        p.stdin.close()

        class COUNTERS(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD),
                        ("PageFaultCount", wintypes.DWORD),
                        ("PeakWorkingSetSize", ctypes.c_size_t),
                        ("WorkingSetSize", ctypes.c_size_t),
                        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                        ("PagefileUsage", ctypes.c_size_t),
                        ("PeakPagefileUsage", ctypes.c_size_t)]

        handle = ctypes.windll.kernel32.OpenProcess(0x1000 | 0x0400, False, p.pid)
        peak = 0
        while p.poll() is None:
            counters = COUNTERS()
            counters.cb = ctypes.sizeof(COUNTERS)
            if ctypes.windll.psapi.GetProcessMemoryInfo(
                    handle, ctypes.byref(counters), counters.cb):
                peak = max(peak, counters.PeakWorkingSetSize)
            time.sleep(0.002)
        p.wait()
        return round(peak / 1024 / 1024, 2) if peak else None

    import resource
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    subprocess.run(argv, input=payload, stdout=subprocess.DEVNULL, check=True)
    after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    scale = 1024 if sys.platform == "darwin" else 1
    return round(max(after - before, 0) * scale / 1024 / 1024, 2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--iterations", type=int, default=5)
    ap.add_argument("--words", type=int, default=20000)
    args = ap.parse_args()

    for path in (PORT, ORACLE):
        if not path.exists():
            sys.exit(f"missing {path} — see README build instructions")

    words = corpus(args.words)
    print(f"corpus: {len(words)} words\n")

    port_argv = [str(PORT)]
    port_prefix = "DMETAPHONE\t0\t"
    oracle_argv = [str(ORACLE)]

    results = {
        "corpus_words": len(words),
        "iterations": args.iterations,
        "platform": sys.platform,
        "note": "read bench/methodology.md before quoting these",
    }

    port_pipe = [pipe_throughput(port_argv, words, port_prefix) for _ in range(args.iterations)]
    orig_pipe = [pipe_throughput(oracle_argv, words) for _ in range(args.iterations)]
    port_lib = [in_process_throughput(words, [str(PORT), "--bench", "DMETAPHONE"])
                for _ in range(args.iterations)]
    orig_lib = [in_process_throughput(words, [str(ORACLE), "--bench"])
                for _ in range(args.iterations)]

    results["throughput_words_per_sec"] = {
        "port_in_process": round(statistics.median(port_lib)),
        "original_c_in_process": round(statistics.median(orig_lib)),
        "port_via_pipe": round(statistics.median(port_pipe)),
        "original_c_via_pipe": round(statistics.median(orig_pipe)),
    }
    results["throughput_words_per_sec"]["algorithm_ratio_port_over_c"] = round(
        statistics.median(port_lib) / statistics.median(orig_lib), 2)

    port_start = startup_latency(port_argv, port_prefix + "mayer\n", args.iterations)
    orig_start = startup_latency(oracle_argv, "mayer\n", args.iterations)
    results["startup_ms"] = {
        "port_median": round(statistics.median(port_start), 2),
        "original_c_median": round(statistics.median(orig_start), 2),
    }

    # p99 of the per-call round trip, which is what the pytest shim pays.
    backend = subprocess.Popen(port_argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               bufsize=1, text=True, encoding="ascii")
    latencies = []
    for w in words[:5000]:
        t0 = time.perf_counter()
        backend.stdin.write(port_prefix + w + "\n")
        backend.stdin.flush()
        backend.stdout.readline()
        latencies.append((time.perf_counter() - t0) * 1e6)
    backend.stdin.close()
    backend.wait()
    latencies.sort()
    results["round_trip_latency_us"] = {
        "p50": round(latencies[len(latencies) // 2], 1),
        "p99": round(latencies[int(len(latencies) * 0.99)], 1),
        "max": round(latencies[-1], 1),
    }

    results["peak_rss_mb"] = {
        "port": peak_rss_mb(port_argv, words, port_prefix),
        "original_c": peak_rss_mb(oracle_argv, words),
    }

    results["binary_bytes"] = {
        "port": PORT.stat().st_size,
        "original_c_oracle": ORACLE.stat().st_size,
    }

    out = ROOT / "bench" / "results.json"
    out.write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main()

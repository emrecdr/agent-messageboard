"""Measure per-invocation process startup, which is what actually costs this design.

The throughput benchmark (bench_queue.py) measures how fast the queue moves messages once a
process is already running. That is the wrong question for a CLI: agents shell out per
operation, so every message pays a fresh process. This measures that term.

The /bin/echo row is a stand-in for "a small native binary" and is deliberately optimistic --
a real binary linking rusqlite will be slower. It is kept as a floor, not as a substitute: the
`amb` rows below are the point of the script and `main` exits 1 if a binary exists and none ran.

**This paragraph used to read "Add the real binary once it exists", followed by a
`./target/release/amb` snippet.** M15 uncommented those rows and repaired the path; the
instruction to add them survived here, in the same file, still naming a path that does not exist
on a machine sharing one cargo target directory. Fixing one instance trains attention on the
thing fixed rather than on its siblings.
"""

import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _harness import amb_binary  # noqa: E402

RUNS = 50


def bench(cmd, runs=RUNS, env=None):
    """Median and p95 wall-clock milliseconds for launching `cmd`."""
    # One warm-up so we measure steady state rather than first-touch page faults. For `amb` it
    # also creates the scratch board, so the migration is not billed to the first timed run.
    subprocess.run(cmd, capture_output=True, env=env)
    times = []
    for _ in range(runs):
        t0 = time.perf_counter()
        subprocess.run(cmd, capture_output=True, env=env)
        times.append((time.perf_counter() - t0) * 1000)
    times.sort()
    return statistics.median(times), times[int(runs * 0.95)]


# **The `amb` rows are the point of this script and they were commented out.** The header said
# "Uncomment once built"; the binary was built, the comment was never revisited, and README.md
# went on publishing `amb --version` at 2.1 ms and `amb inbox` at 3.0 ms while naming this file as
# the harness. A reader following that pointer could not reproduce either row. An unverified
# measurement script is a false comment with a shebang.
#
# `amb inbox` gets its own scratch board through `AMB_DB`. Pointed at the real one it would touch
# the roster fifty times per run, which is a benchmark writing to the ledger it is meant to be
# measuring beside.
AMB = amb_binary()
SCRATCH = pathlib.Path(tempfile.mkdtemp(prefix="amb-bench-"))
ISOLATED = {
    **os.environ,
    "AMB_DB": str(SCRATCH / "bench.db"),
    "AMB_AGENT": "bench-startup",
    "AMB_PROJECT": "bench",
}

CANDIDATES = [
    ("python3 -c pass", [sys.executable, "-c", "pass"], None),
    ("python3 -c 'import sqlite3'", [sys.executable, "-c", "import sqlite3"], None),
    ("native binary (/bin/echo)", ["/bin/echo", "x"], None),
]
if AMB:
    CANDIDATES += [
        ("amb --version", [AMB, "--version"], ISOLATED),
        ("amb inbox", [AMB, "inbox"], ISOLATED),
    ]
else:
    print("! no `amb` binary found - build with `cargo build --release` for the rows that matter")


def main():
    print(f"Process startup, {RUNS} runs each - python {sys.version.split()[0]}")
    print("=" * 62)
    rows = []
    for label, cmd, env in CANDIDATES:
        if not shutil.which(cmd[0]) and not cmd[0].startswith(("/", ".")):
            print(f"  {label:<30} SKIPPED (not found)")
            continue
        p50, p95 = bench(cmd, env=env)
        rows.append((label, p50))
        print(f"  {label:<30} p50 {p50:6.2f} ms   p95 {p95:6.2f} ms")

    # **The script asserts its own coverage**, because the way this one rotted was not a crash.
    # The `amb` candidate sat commented out behind "Uncomment once built" while README.md published
    # two rows naming this file as the harness. Nothing failed; the script ran, printed three rows,
    # and exited 0. A silent gap in a measurement harness is the same defect class the project
    # catalogues everywhere else, so it is made loud here rather than left to a reader to notice.
    if AMB and not any(label.startswith("amb ") for label, _ in rows):
        print("\n! a binary was found at", AMB, "but no `amb` row ran - this harness is not")
        print("  measuring the thing README.md cites it for.")
        return 1

    if len(rows) >= 2:
        slowest = max(rows, key=lambda r: r[1])
        fastest = min(rows, key=lambda r: r[1])
        ratio = slowest[1] / fastest[1]
        print("=" * 62)
        print(f"  {slowest[0]} is {ratio:.1f}x the cost of {fastest[0]}")
        # 17 agents polling every 2s is ~8.5 invocations/second.
        rate = 8.5
        print(f"\n  At {rate} invocations/s (17 agents polling every 2s):")
        for label, p50 in rows:
            ms_per_s = p50 * rate
            print(f"    {label:<30} {ms_per_s:6.1f} ms/s  "
                  f"({ms_per_s / 10:.1f}% of one core) on startup alone")
    return 0


if __name__ == "__main__":
    sys.exit(main())

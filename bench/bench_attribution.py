"""Attribute the PreToolUse cost: is it the vault size, or the number of *matching* notes?

The bound on the fetch window changed nothing measurable, which falsifies "fetching 1000 rows is
the cost". Two vaults of the same size, differing only in how many notes concern the queried path,
separate the two explanations.
"""
import json, os, pathlib, shutil, statistics, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _harness import amb_binary, index_or_die, note, scratch_env  # noqa: E402

BIN = amb_binary()
ROOT = pathlib.Path("/tmp/amb-attr")
N = 50
MATCHED = "src/delivery.rs"


def build(name, total, matching):
    """A vault of `total` notes, `matching` of which concern MATCHED.

    `matching` is this experiment's **independent variable**, so `index_or_die` below asserts it
    landed. Without that the three vaults are indistinguishable and the table is one measurement
    printed three times — which is exactly what happened between D81 and 2026-08-29.
    """
    d = ROOT / name
    proj = d / "vault" / "projects" / "bench"
    proj.mkdir(parents=True)
    for i in range(total):
        path = MATCHED if i < matching else f"src/other/mod{i:05d}.rs"
        (proj / f"2026-08-{(i % 28) + 1:02d}-note-{i:05d}.md").write_text(
            note("bench", f"observation number {i}",
                 f"2026-08-{(i % 28) + 1:02d}T00:00:00Z", [path]))
    env = scratch_env(BIN, d)
    index_or_die(BIN, env, total, f"{name} ({total} notes, {matching} matching)")
    return env


def timed(env, stdin):
    out = []
    for _ in range(N):
        t = time.perf_counter()
        subprocess.run([BIN, "hook", "memory"], env=env, input=stdin.encode(),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        out.append((time.perf_counter() - t) * 1000)
    return statistics.median(out)


if ROOT.exists():
    shutil.rmtree(ROOT)
cases = {
    "1000 notes, 1000 match": build("all", 1000, 1000),
    "1000 notes,    8 match": build("few", 1000, 8),
    "   8 notes,    8 match": build("small", 8, 8),
}
hit = json.dumps({"hook_event_name": "PreToolUse", "tool_name": "Read",
                  "tool_input": {"file_path": os.getcwd() + "/src/delivery.rs"}})
miss = json.dumps({"hook_event_name": "PreToolUse", "tool_name": "Read",
                   "tool_input": {"file_path": os.getcwd() + "/src/nope.rs"}})

# **Coverage, not a value.** The experiment manipulates one variable — how many notes concern
# the queried path — so the guard asserts that the hit path actually reaches notes and the miss
# path actually reaches none. It says nothing about how long either took; `tools/bench.sh`
# explains why a harness check and a performance gate cannot be the same thing here.
def concerning(env, stdin):
    out = subprocess.run([BIN, "hook", "memory"], env=env, input=stdin,
                         capture_output=True, text=True)
    return len(out.stdout.strip())


gaps = []
for label, env in cases.items():
    if concerning(env, hit) == 0:
        gaps.append(f"{label}: the matching path injected nothing, so `hit` is not a hit")
    if concerning(env, miss) != 0:
        gaps.append(f"{label}: the non-matching path injected something, so `miss` is not a miss")
if gaps:
    print("\n! this run did not measure what MEASUREMENTS.md says it does:", file=sys.stderr)
    for g in gaps:
        print(f"    {g}", file=sys.stderr)
    sys.exit(1)

runs = [{k: (timed(e, hit), timed(e, miss)) for k, e in cases.items()} for _ in range(3)]
print(f"n={N}, 3 interleaved runs\n")
print(f"{'vault':26} {'PreToolUse hit':>18}  {'miss':>16}")
for k in cases:
    h = [r[k][0] for r in runs]
    m = [r[k][1] for r in runs]
    print(f"{k:26} {min(h):6.2f} - {max(h):5.2f} ms  {min(m):5.2f} - {max(m):5.2f} ms")

print("\n\u2713 every vault indexed in full; the hit path injects and the miss path does not")

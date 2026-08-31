"""Measure what the memory hook costs, per invocation, at realistic vault sizes.

The plan is explicit that this must be measured: `SessionStart` performs a *write* (the citation
ledger) and `PreToolUse` fires before every file tool call, which is the most frequent hook in the
system. An injecting feature whose cost is unmeasured is the thing MEMORY-DESIGN.md s13 forbids.

Interleaves scenarios within a run rather than running each to completion, so a machine that gets
busy halfway through skews every scenario equally instead of one of them. CLAUDE.md: two wrong
sub-claims in MEASUREMENTS.md came from single un-repeated runs.
"""
import json, os, pathlib, shutil, statistics, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _harness import amb_binary, index_or_die, note, scratch_env  # noqa: E402

BIN = amb_binary()
ROOT = pathlib.Path("/tmp/amb-bench")
N = int(sys.argv[1]) if len(sys.argv) > 1 else 50


def timed(env, args, stdin, n=N):
    """Per-invocation wall clock, in ms."""
    out = []
    for _ in range(n):
        t = time.perf_counter()
        subprocess.run([BIN] + args, env=env, input=stdin.encode(),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        out.append((time.perf_counter() - t) * 1000)
    return out


START = json.dumps({"hook_event_name": "SessionStart"})
STOP = json.dumps({"hook_event_name": "Stop"})


def pre(path):
    return json.dumps({"hook_event_name": "PreToolUse", "tool_name": "Read",
                       "tool_input": {"file_path": path}})


def run_once(envs):
    """One interleaved pass over every scenario."""
    r = {}
    off = dict(envs[0]); off.pop("AMB_VAULT")
    hit = pre(os.getcwd() + "/src/delivery.rs")
    miss = pre(os.getcwd() + "/src/nothing-here.rs")
    r["echo (native floor)"] = timed(envs[0], [], "", 0) or None
    for notes, env in zip((0, 8, 100, 1000), envs):
        r[f"SessionStart, {notes} notes"] = timed(env, ["hook", "memory"], START)
    r["SessionStart, memory off"] = timed(off, ["hook", "memory"], START)
    r["PreToolUse hit, 1000 notes"] = timed(envs[3], ["hook", "memory"], hit)
    r["PreToolUse miss, 1000 notes"] = timed(envs[3], ["hook", "memory"], miss)
    r["PreToolUse skipped tool"] = timed(
        envs[3], ["hook", "memory"],
        json.dumps({"hook_event_name": "PreToolUse", "tool_name": "TodoWrite",
                    "tool_input": {"file_path": "x"}}))
    r["delivery hook (Stop), reference"] = timed(envs[1], ["hook", "turn"], STOP)
    del r["echo (native floor)"]
    return r


def main():
    # Each vault size needs its own board, and they must survive the whole run.
    if ROOT.exists():
        shutil.rmtree(ROOT)
    envs = []
    for notes in (0, 8, 100, 1000):
        d = ROOT / f"v{notes}"
        proj = d / "vault" / "projects" / "bench"
        proj.mkdir(parents=True)
        for i in range(notes):
            (proj / f"2026-08-{(i % 28) + 1:02d}-note-{i:05d}.md").write_text(
                note("bench", f"observation number {i} about the delivery path",
                     f"2026-08-{(i % 28) + 1:02d}T00:00:00Z", ["src/delivery.rs"],
                     body="Some prose about what was learned."))
        env = scratch_env(BIN, d)
        # **The vault size IS the variable here**, so it is asserted rather than assumed. Between
        # D81 and 2026-08-29 every note was rejected and all four rows measured an empty vault.
        index_or_die(BIN, env, notes, f"the {notes}-note vault")
        envs.append(env)

    runs = [run_once(envs) for _ in range(3)]
    print(f"n={N} per scenario, 3 interleaved runs\n")
    print(f"{'scenario':34} {'p50 across runs':>22}   {'p95':>7}")
    for k in runs[0]:
        p50s = [statistics.median(r[k]) for r in runs]
        p95 = max(sorted(r[k])[int(len(r[k]) * 0.95)] for r in runs)
        print(f"{k:34} {min(p50s):7.2f} - {max(p50s):5.2f} ms   {p95:5.2f} ms")

    # Coverage, never a value. `index_or_die` has already proved each vault exists; what is left
    # to prove is that the *injection* this table is about actually happened at all. A hook that
    # silently stopped injecting would leave every row looking plausible and slightly faster.
    injected = subprocess.run([BIN, "hook", "memory"], env=envs[3], input=START,
                              capture_output=True, text=True).stdout
    if not injected.strip():
        print("\n! SessionStart injected nothing from a 1000-note vault — this harness is not",
              file=sys.stderr)
        print("  measuring what MEASUREMENTS.md M9 cites it for.", file=sys.stderr)
        return 1
    print(f"\n\u2713 every vault indexed in full; SessionStart injects "
          f"{len(injected)} characters from the 1000-note vault")
    return 0


sys.exit(main())

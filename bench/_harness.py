"""Shared plumbing for the measurement harnesses, and the reason it exists.

**Two of the four harnesses were void for weeks and printed healthy tables the whole time.**
`bench_memory.py` and `bench_attribution.py` each wrote synthetic vault notes with a
`project:` frontmatter key. D81 renamed that key to `scope:` and *deliberately removed the
fallback* — "the vault is regenerable, and a key that means two things in two files is the drift
this whole change removes." True of the real vault. Nobody regenerates a benchmark fixture.

So `parse_note` returned `None` for all 1000 notes, `memory index` reported `1000 scanned ·
0 indexed`, and both scripts measured an empty vault and exited 0. `bench_attribution.py`'s whole
experiment is "two vaults differing only in how many notes concern the queried path"; with nothing
indexed, its three rows were the same measurement printed three times.

Two lessons, and this module is both of them:

1. **The note format lived in two files, neither next to the parser.** One writer here means the
   next schema change breaks one place, and [`index_or_die`] makes it break *loudly*.
2. **A negative decision's blast radius includes every fixture that constructs the thing.** D81
   said the vault is regenerable and was right about the vault. It was not a claim about fixtures.
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys


def amb_binary():
    """The release binary, wherever this machine puts it.

    **Not `./target/release/amb`.** All Rust projects on this machine share one cargo target
    directory (`~/.cache/cargo-target`), so the in-tree path does not exist here — which is half
    of why `bench_startup.py`'s `amb` rows stayed commented out long after the binary was built
    (M15). The other two harnesses hardcoded the absolute cache path instead, which is the same
    bug with a literal that happens to be right on one machine.
    """
    for c in (
        pathlib.Path.home() / ".cache/cargo-target/release/amb",
        pathlib.Path("target/release/amb"),
    ):
        if c.is_file():
            return str(c)
    return shutil.which("amb")


def note(scope, title, created, files, kind="observation", body="prose"):
    """One vault note, in the frontmatter `memory::parse_note` actually accepts.

    **`scope:`, not `project:` (D81).** `parse_note` does `get("scope")?` with no fallback, so the
    old key does not degrade — it rejects the note entirely.

    `id:` is deliberately omitted: `parse_note` falls back to the filename slug for notes typed
    straight into Obsidian, and a fixture is exactly that case. Writing an id here would be a
    second place for the id format to drift.
    """
    lines = [
        "---",
        f'kind: "{kind}"',
        f'scope: "{scope}"',
        f'title: "{title}"',
        'status: "active"',
        f'created: "{created}"',
        "files:",
    ]
    lines += [f'  - "{f}"' for f in files]
    lines += ["---", "", body, ""]
    return "\n".join(lines)


def index_or_die(binary, env, expected, what):
    """Index the fixture vault and **fail loudly** unless every note landed.

    This is the guard the two broken harnesses did not have, and it is the reason the shared
    `note()` above is safe: if the frontmatter schema moves again, this exits 1 with the indexer's
    own diagnosis rather than letting a script measure an empty vault.

    It asserts **coverage, never a value** — that the fixture the experiment needs actually
    exists, not how fast anything was. `tools/bench.sh` explains why that distinction decides
    whether these scripts are honest.

    The indexer already prints exactly what is wrong (`frontmatter key 'project' is read by
    nothing`, 1000 times). Both callers passed `capture_output=True` and threw it away.
    """
    out = subprocess.run(
        [binary, "--json", "memory", "index"], env=env, capture_output=True, text=True
    )
    try:
        r = json.loads(out.stdout)
    except json.JSONDecodeError:
        print(f"\n! `memory index` produced no JSON for {what}:", file=sys.stderr)
        print(out.stdout[:400] or out.stderr[:400], file=sys.stderr)
        sys.exit(1)

    if r.get("indexed") != expected:
        print(
            f"\n! {what}: wrote {expected} note(s), indexed {r.get('indexed')} of "
            f"{r.get('scanned')} scanned — this harness is measuring a vault that is not there.",
            file=sys.stderr,
        )
        for u in r.get("unknown_keys", [])[:3]:
            print(f"    frontmatter key `{u['key']}` is read by nothing — {u['note']}",
                  file=sys.stderr)
        if r.get("unknown_keys"):
            print("    (the indexer says this on every note; see bench/_harness.py)",
                  file=sys.stderr)
        sys.exit(1)
    return r


def scratch_env(binary, root, project="bench"):
    """An environment pointed entirely at a throwaway board and vault.

    Never the real ones: a harness that writes to the board it measures beside is the defect
    `bench_startup.py`'s `ISOLATED` comment describes, and here it would also write into the
    ledger the injection window is being judged on.
    """
    env = dict(
        os.environ,
        AMB_DB=str(root / "board.db"),
        AMB_VAULT=str(root / "vault"),
        AMB_AGENT="bench",
        AMB_PROJECT=project,
    )
    env.pop("CLAUDE_CODE_SESSION_ID", None)
    return env

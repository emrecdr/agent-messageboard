#!/usr/bin/env python3
"""Find documentation that has drifted away from the code.

**This class bit seven times in one day**, which is what makes it a script rather than a note —
the same reasoning `find_unread_fields.py` records for its own defect:

  - `CHANGELOG.md` said "Nothing yet" under `[Unreleased]` with nine commits since the tag, one of
    them a security fix.
  - The README command reference omitted seven shipped commands, including the whole of
    `amb memory derive|candidates|promote|export|capture|expire`.
  - `"Nothing is ever written inside a repository"` (D11) was stated flatly in three files after
    `amb memory export` had been writing into repositories since D49.
  - `MEMORY-DESIGN.md` and `DESIGN.md` both opened with a status banner describing an earlier
    build.
  - Two proposal documents existed on disk and appeared in no index.
  - Test counts and the D-range went stale in five places across three files.

**Documentation drift is this project's failure shape wearing a different coat.** A reference that
omits half a surface reads as complete; a negative decision stated too strongly gets defended by a
reader after the code stopped honouring it. Neither produces an error, and no compiler sees either.

Only mechanical facts are checked — the ones with a single source of truth in the repository.
Whether prose is *true* is not checkable here and never will be; that still wants a person. This
is a screen for the half that does not.

    python3 tools/check_docs.py
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
README = (ROOT / "README.md").read_text(encoding="utf-8")
CLAUDE = (ROOT / "CLAUDE.md").read_text(encoding="utf-8")
CHANGELOG = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")


def every_doc_is_indexed():
    """A document nobody links is a document nobody reads.

    **Anchored to an actual markdown link, not to a mention.** The first version of this check
    asked whether the filename appeared anywhere in the README, and passed because a *sentence*
    elsewhere happened to name the file. A guard satisfied by prose it was not written to inspect
    is D51's finding: correct by coincidence, and silent when it stops being.
    """
    missing = [
        p.name
        for p in sorted((ROOT / "docs").glob("*.md"))
        if f"](docs/{p.name})" not in README
    ]
    return [f"docs/{n} exists but is in no README index" for n in missing]


def every_command_is_documented():
    """The README calls itself a command reference, so an omission reads as 'does not exist'."""
    def subcommands(*args):
        out = subprocess.run(
            [str(BIN), *args, "--help"], capture_output=True, text=True
        ).stdout
        body = out.partition("Commands:")[2].partition("Options:")[0]
        return [
            m.group(1)
            for line in body.splitlines()
            if (m := re.match(r"\s{2}([a-z][a-z-]*)\s", line))
            and m.group(1) not in ("help",)
        ]

    # Scoped to the reference section itself. Searching the whole README passed for `snapshot`
    # because a paragraph three sections away described it — the same coincidence as above.
    reference = README.partition("## Command reference")[2].partition("\n## ")[0]
    rows = [line for line in reference.splitlines() if line.startswith("| `amb ")]
    table = "\n".join(rows)

    problems = []
    for c in subcommands():
        if c == "memory":
            continue
        if f"`amb {c}" not in table:
            problems.append(f"`amb {c}` ships but is absent from the README command reference")
    for c in subcommands("memory"):
        if f"amb memory {c}" not in table:
            problems.append(f"`amb memory {c}` ships but is absent from the README command reference")
    return problems


def counts_are_current():
    """Test counts and the decision range, both quoted in prose and both easy to forget."""
    problems = []
    highest = max(int(m) for m in re.findall(r"^## D(\d+) ", (ROOT / "docs" / "DECISIONS.md").read_text(encoding="utf-8"), re.M))
    for name, text in (("README.md", README), ("CLAUDE.md", CLAUDE)):
        for quoted in set(re.findall(r"D1[–-]D(\d+)", text)):
            if int(quoted) != highest:
                problems.append(f"{name} says D1–D{quoted}; DECISIONS.md reaches D{highest}")

    out = subprocess.run(
        ["cargo", "test", "--no-fail-fast"], capture_output=True, text=True, cwd=ROOT
    ).stdout
    actual = sum(int(m) for m in re.findall(r"^test result: ok\. (\d+) passed", out, re.M))
    if actual:
        for name, text in (("README.md", README), ("CLAUDE.md", CLAUDE)):
            for quoted in set(re.findall(r"(\d+) tests", text)):
                if int(quoted) != actual:
                    problems.append(f"{name} says {quoted} tests; the suite runs {actual}")
    return problems


def records_are_uniquely_numbered():
    """Two records with the same number, which nothing else here could see.

    **Found by making the mistake.** Two sessions committing within minutes both appended `M31`:
    one had read the range before the other's commit landed, then reused the number it remembered
    rather than re-reading. `counts_are_current` validates that README and CLAUDE.md quote the
    right *highest* D, which is why a duplicate is invisible to it — a duplicate does not move the
    maximum. Every citation still resolved, and the gate stayed green.

    That is this project's own catalogued failure applied to a number: a constant validated early
    and reused after the ground moved. The concurrency is not incidental — several sessions work
    these repos at once by design, so "read the range, then append" is a read-modify-write with
    the same lost-update shape D99 records for `settings.json`.

    Duplicates only. A *gap* is not checked, because a withdrawn record is a legitimate reason for
    one and a rule with a legitimate exception is one people switch off; a duplicate never is.
    """
    problems = []
    for name, prefix in (("DECISIONS.md", "D"), ("MEASUREMENTS.md", "M")):
        text = (ROOT / "docs" / name).read_text(encoding="utf-8")
        seen = {}
        for m in re.finditer(rf"^## {prefix}(\d+)[^\n]*", text, re.M):
            seen.setdefault(m.group(1), []).append(m.group(0).strip())
        for n, headings in sorted(seen.items(), key=lambda kv: int(kv[0])):
            if len(headings) > 1:
                problems.append(
                    f"{name} has {len(headings)} records numbered {prefix}{n}:"
                    + "".join(f"\n      {h}" for h in headings)
                )
    return problems


def unreleased_is_honest():
    """`[Unreleased]` saying nothing, while commits exist since the tag, is the drift itself."""
    tag = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0"], capture_output=True, text=True, cwd=ROOT
    ).stdout.strip()
    if not tag:
        return []
    n = subprocess.run(
        ["git", "rev-list", "--count", f"{tag}..HEAD"], capture_output=True, text=True, cwd=ROOT
    ).stdout.strip()
    section = CHANGELOG.partition("## [Unreleased]")[2].partition("\n## ")[0]
    if n.isdigit() and int(n) > 0 and "Nothing yet" in section:
        return [f"CHANGELOG [Unreleased] says 'Nothing yet' with {n} commit(s) since {tag}"]
    return []


BIN = ROOT / "target" / "debug" / "amb"
if not BIN.exists():
    import os
    BIN = pathlib.Path(os.path.expanduser("~/.cache/cargo-target/debug/amb"))


def every_bench_script_is_named(problems=None):
    """A measurement harness nobody references is one nobody runs.

    Cheap and mechanical, like the rest of this file — it cannot tell whether a harness still
    measures the right thing. That is `tools/bench.sh`'s job, and it is separate because it costs
    ~17s and this runs before every commit.

    What this *can* catch is the half that made `bench_startup.py` rot survive: it was cited by
    README.md and MEASUREMENTS.md as the harness behind published numbers, so a reader had every
    reason to trust it, while its `amb` rows sat commented out. The citation is the promise. A
    script with no citation makes none and should be deleted; a cited one has to keep it.
    """
    out = []
    docs = "\n".join(
        p.read_text() for p in [ROOT / "README.md", *sorted((ROOT / "docs").glob("*.md"))]
    )
    # `tools/` was outside this check until 2026-08-31, and the asymmetry is what let a new script
    # land uncited — the exact condition the docstring above calls a script that "makes no promise".
    # It was found by adding `tools/eyeball.sh` and noticing nothing objected. Every pre-existing
    # `tools/` script was already cited, so widening the glob cost nothing and closed the hole.
    tracked = set(
        subprocess.run(
            ["git", "ls-files", "bench", "tools"], capture_output=True, text=True, cwd=ROOT
        ).stdout.split()
    )
    for directory, pattern in (("bench", "*.py"), ("tools", "*")):
        for script in sorted((ROOT / directory).glob(pattern)):
            # Untracked means work in progress, not an uncited script. Without this the check
            # fires on a *peer session's* half-finished file and blocks a commit that has nothing
            # to do with it — observed the hour this widening was written.
            if f"{directory}/{script.name}" not in tracked:
                continue
            # `_`-prefixed files are shared modules, not harnesses — Python's own convention for
            # "not part of the public surface". The rule below is about a *harness* nobody runs; a
            # module with no callers is `find_unread_fields.py`'s question, not this one.
            if script.name.startswith("_") or not script.is_file():
                continue
            if f"{directory}/{script.name}" not in docs:
                out.append(
                    f"{directory}/{script.name} is referenced by no doc"
                    " — either cite it or delete it"
                )
    return out


def main():
    if not BIN.exists():
        print(f"no debug binary at {BIN} — run `cargo build` first")
        return 2
    problems = (
        every_doc_is_indexed()
        + every_command_is_documented()
        + counts_are_current()
        + records_are_uniquely_numbered()
        + unreleased_is_honest()
        + every_bench_script_is_named()
    )
    if problems:
        print(f"{len(problems)} documentation drift(s):")
        for p in problems:
            print(f"  {p}")
        print("\nMechanical only. Whether the prose is true still wants a person.")
        return 1
    print("docs are consistent with the code on every mechanical check.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

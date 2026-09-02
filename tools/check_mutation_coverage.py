#!/usr/bin/env python3
"""Whether "every module has been mutation-tested" is TRUE, derived rather than remembered.

**This exists because the claim was made falsely twice in one day, by two sessions.** M55's
title, its CHANGELOG entry and a board broadcast all said the crate-wide inventory closed; it
was three files short. The correction that caught it was itself short by one, because it
spot-checked the two files it suspected instead of doing the set-difference. Both parties
enumerated from *the record of rounds they remembered running* — and a completeness claim
derived from memory is not a completeness claim (D39/D45: the fix for a recurring error is a
script, not a note).

**Its blind spot, named because it bit within a day** (M62): this answers *has this module ever
had a round*, never *was it mutated in the form it is in now*. `vendors.rs` tripled in size after
its clean pass and kept reading as covered; re-running found three survivors, all in the new
code. A set-difference over files cannot see time. Left as a stated limit rather than machinery
because the honest fix is to re-run a module you have just rewritten, and a script that guessed
at staleness from dates would be a new instrument to keep true.

**It polices the claim, not the work.** Mutation is deliberately not a commit gate — it is slow,
and `tools/mutants.sh`'s header says why. So an uncovered module is never a failure here; it is
printed and left. What fails is a document asserting closure while the set-difference disagrees.
That is D95's rule applied to a completeness claim: a stated condition nothing can evaluate is
worse than no condition.

Covered set is parsed from `docs/MEASUREMENTS.md`, the only place a round is recorded.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Files that generate no mutants at all, so "never mutated" is not a gap.
# Verify any entry with:  cargo mutants --list --file <path>   -> must print nothing.
EXEMPT = {
    "src/lib.rs": "re-exports only; cargo mutants --list finds 0 mutants",
    "src/memory.rs": "the facade re-exporting src/memory/*; 0 mutants",
}

# Documents that describe the CURRENT state, and may therefore not overstate it.
# docs/MEASUREMENTS.md is excluded on purpose: it is an append-only log where a title records
# what was claimed at the time and the correction sits beneath it — M55 is the standing example.
CURRENT_STATE_DOCS = ["README.md", "CLAUDE.md", "CHANGELOG.md"]

CLOSURE_CLAIM = re.compile(
    r"(crate-wide[^.\n]{0,60}(inventory|mutation)[^.\n]{0,60}clos"
    r"|(inventory|ledger)[^.\n]{0,30}(is |now )?clos(es|ed)"
    r"|every module[^.\n]{0,80}(mutat|inventory))",
    re.I,
)
# A sentence that DENIES closure is the honest form and must not trip the check.
NEGATED = re.compile(r"(not clos|does not|is short|two files|three files|no longer|fails to)", re.I)


def _normalise(path: str, src: set[str]) -> str | None:
    """`memory/note.rs` and `delivery.rs` are how the score tables name modules."""
    for candidate in (path, f"src/{path}"):
        if candidate in src:
            return candidate
    return None


def covered_from_measurements(src: set[str]) -> set[str]:
    """Every src file a recorded round actually ran.

    **Three forms, because the record has three and reading only one is what produced this
    script.** The first version parsed invocations alone, reported `memory/note.rs` as never
    mutated, and that claim went out on the board before M27's score table refuted it — the
    same enumerate-from-memory error the docstring above describes, committed by the tool
    written to prevent it. A scored table row is evidence of a run; a prose *mention* is not,
    which is why form three requires the numeric columns.
    """
    text = (ROOT / "docs" / "MEASUREMENTS.md").read_text()
    covered = set()
    # 1. `tools/mutants.sh src/a.rs src/b/c.rs`
    for line in re.findall(r"tools/mutants\.sh[^\n`]*", text):
        covered.update(re.findall(r"src/[\w/]+\.rs", line))
    # 2. `--file src/x.rs`
    covered.update(re.findall(r"--file\s+(src/[\w/]+\.rs)", text))
    for line in text.splitlines():
        # 3. A scored table row: | `memory/note.rs` | 354 | 24 | 23 | 1 | 96% |
        if line.startswith("|") and re.search(r"\|\s*\d+\s*\|", line):
            for raw in re.findall(r"`([\w/]+\.rs)`", line):
                if (hit := _normalise(raw, src)):
                    covered.add(hit)
    # 4. A `**Modules:**` block — the explicit form, for a round whose invocation named no
    #    paths. It runs to the next blank line: the first version read only the opening line
    #    and reported five of one round's eight modules as never mutated, which is this
    #    script's own recurring bug in miniature — a derivation that stops early.
    for block in re.findall(r"^\*\*Modules:\*\*(.+?)\n\n", text, re.S | re.M):
        for raw in re.findall(r"`([\w/]+\.rs)`", block):
            if (hit := _normalise(raw, src)):
                covered.add(hit)
    return covered


def main() -> int:
    src = {
        str(p.relative_to(ROOT))
        for p in (ROOT / "src").rglob("*.rs")
    }
    covered = covered_from_measurements(src) & src
    exempt = set(EXEMPT) & src
    uncovered = sorted(src - covered - exempt)

    stale = sorted(set(EXEMPT) - src)
    problems = []

    print(f"{len(covered)} of {len(src)} module(s) have a recorded mutation round; "
          f"{len(exempt)} exempt.")
    if uncovered:
        print("\nnever mutation-tested (not a failure — mutation is not a gate):")
        for f in uncovered:
            print(f"  {f}")
    else:
        print("every module with mutants has a recorded round — the inventory IS closed.")
    if exempt:
        print("\nexempt, with the reason each was excused:")
        for f in sorted(exempt):
            print(f"  {f} — {EXEMPT[f]}")

    for f in stale:
        problems.append(f"exemption names {f}, which no longer exists — delete it")

    # The enforcement: a current-state document may not assert what the set-difference denies.
    if uncovered:
        for doc in CURRENT_STATE_DOCS:
            path = ROOT / doc
            if not path.exists():
                continue
            for n, line in enumerate(path.read_text().splitlines(), 1):
                if CLOSURE_CLAIM.search(line) and not NEGATED.search(line):
                    problems.append(
                        f"{doc}:{n} claims the inventory is closed while "
                        f"{len(uncovered)} module(s) have never been mutated:\n"
                        f"      {line.strip()[:110]}"
                    )

    if problems:
        print(f"\n{len(problems)} problem(s):")
        for p in problems:
            print(f"  {p}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

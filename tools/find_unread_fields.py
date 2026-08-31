#!/usr/bin/env python3
"""Find public struct fields that nothing ever reads.

**This pattern has bitten this codebase three times**, which is what makes it worth a script:

  - `messages.attempts` and `messages.failed_at` had no writer at all (fixed by D23).
  - `IndexStats::skipped` recorded "this directory was too large to auto-index" and no code path
    consulted it, so a 501-note vault reported itself as empty (D45).
  - claude-mem's `relevance_count`, the incumbent's own version, is zero across all 80,264 rows —
    which is the evidence D39 rests on.

`rustc`'s `dead_code` lint cannot catch it: these are `pub` fields on a library crate, so they are
reachable by definition. A field that is written, never read, and describes something true is
indistinguishable from a working feature until someone reads the code — which is this project's
documented worst failure shape, a silence rather than an error.

The counting is deliberately crude (substring matches across `src/` and `tests/`), so it
over-counts rather than under-counts: it is a *screen*, not a proof. A zero is worth
investigating; a low number is worth eyeballing. Run it after adding a struct.

    python3 tools/find_unread_fields.py

**`rglob`, not `glob`, and it printed the file count only after that mattered.** This walked
`src/*.rs` flatly until D81. The day D80 turned `src/memory.rs` into `src/memory/`, the audit
stopped seeing the module it was *written for* — D45 is a field in `src/memory/index.rs` — and
went from 161 fields to 55 while still printing "every one is read somewhere". A tool reporting
success over a third of the ground it used to cover is the same silence it exists to find, and the
only thing that noticed was the number moving. So the count of *files* is printed beside the count
of fields: a future narrowing has to say so.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORPUS = "\n".join(
    p.read_text(encoding="utf-8")
    for d in ("src", "tests")
    for p in (ROOT / d).rglob("*.rs")
)
# Everything under tests/, plus each src file's own `mod tests`, so "called only by its own tests"
# is answerable.
TESTS_ONLY = "\n".join(p.read_text(encoding="utf-8") for p in (ROOT / "tests").rglob("*.rs"))
for _p in (ROOT / "src").rglob("*.rs"):
    _t = _p.read_text(encoding="utf-8")
    _i = _t.find("#[cfg(test)]")
    if _i != -1:
        TESTS_ONLY += "\n" + _t[_i:]

# Production only: every src file up to its own `mod tests`. Needed to tell a function that is
# *referenced* without being *called* — passed by reference — from one nothing mentions at all.
PROD = "\n".join(
    _t[: _t.find("#[cfg(test)]")] if "#[cfg(test)]" in _t else _t
    for _t in (p.read_text(encoding="utf-8") for p in (ROOT / "src").rglob("*.rs"))
)

STRUCT = re.compile(r"pub struct (\w+)\s*\{(.*?)\n\}", re.S)
FIELD = re.compile(r"^\s*pub (\w+):", re.M)
FUNC = re.compile(r"^pub fn (\w+)", re.M)


def main() -> int:
    rows, unread = [], []
    scanned = 0
    for path in sorted((ROOT / "src").rglob("*.rs")):
        scanned += 1
        text = path.read_text(encoding="utf-8")
        for struct, body in STRUCT.findall(text):
            for field in FIELD.findall(body):
                reads = len(re.findall(r"\." + field + r"\b", CORPUS))
                rows.append((path.name, struct, field, reads))
                if reads == 0:
                    unread.append((path.name, struct, field))

    # **Functions too, because the field check missed a whole class of the same defect** — but
    # advisory, because this heuristic cannot see a function passed by reference rather than
    # called. It found four in one session that had shipped with no caller but their own tests. Four
    # public functions shipped in one session with no caller but their own tests —
    # `parse_transcript`, `render_facts`, `candidates_concerning` and `across_repos`. A field that
    # nothing reads and a function that nothing calls are the same silence, and `dead_code` sees
    # neither on a `pub` item in a library crate.
    dead_fns = []
    for path in sorted((ROOT / "src").rglob("*.rs")):
        for fn in FUNC.findall(path.read_text(encoding="utf-8")):
            # A call outside its own definition and its own tests.
            #
            # **Count the definitions rather than assuming there is one, and this was a real
            # off-by-one.** The old code subtracted a hardcoded 1 for "the definition itself",
            # which assumes the definition matches the *call* pattern. A generic signature does
            # not: `pub fn nearest<'a>(` has `<'a>` between the name and the paren, so
            # `\bnearest\s*\(` never matched it, and the subtraction removed a real production
            # call instead. `messages::nearest` has exactly one caller and was reported as having
            # none — a false positive on the audit's own arithmetic, in the tool whose whole job
            # is to be believed about a zero.
            calls = len(re.findall(r"\b" + fn + r"\s*\(", CORPUS))
            in_tests = len(re.findall(r"\b" + fn + r"\s*\(", TESTS_ONLY))
            # **Definitions are counted the same way calls are — outside tests.** Counting all of
            # them over-subtracts the moment a test fixture shares a name with a production
            # function: `memory::receipt` has one of each, so its fixture's definition was removed
            # from the production side while its calls were already on the test side, and a
            # function with a caller was reported as having none. Symmetry is the fix, and the
            # asymmetry is what made the first version of this line wrong twice in a row.
            defs = len(re.findall(r"\bfn\s+" + fn + r"\s*\(", CORPUS)) - len(
                re.findall(r"\bfn\s+" + fn + r"\s*\(", TESTS_ONLY)
            )
            if calls - in_tests - defs <= 0:
                # Bare mentions in production that are *not* calls: `.is_some_and(f)`, `map(f)`,
                # a function pointer in a table. One of these means "referenced, not called",
                # which is the entire content of the advisory below — so the tool says it instead
                # of leaving the reader to work it out for the same function every time.
                refs = len(re.findall(r"\b" + fn + r"\b(?!\s*\()", PROD)) - len(
                    re.findall(r"\bfn\s+" + fn + r"\b(?!\s*\()", PROD)
                )
                dead_fns.append((path.name, fn, calls - in_tests - defs, refs))

    width = max(len(r[1]) for r in rows) if rows else 10
    for name, struct, field, reads in rows:
        mark = "  <-- NEVER READ" if reads == 0 else ""
        print(f"{name:16} {struct:{width}} {field:22} reads={reads}{mark}")

    print()
    if dead_fns:
        print(f"{len(dead_fns)} public fn(s) called only by their own tests:")
        for name, fn, production, refs in dead_fns:
            why = (
                f" — but referenced {refs}x without parentheses, so it is passed by reference"
                if refs > 0
                else "  <-- nothing in production mentions it at all"
            )
            print(f"  {name} :: {fn}()  ({production} production call(s)){why}")
        print("  ADVISORY — this check over-reports and does not fail the run. A function passed")
        print("  by reference (`.is_some_and(f)`) has no parentheses and looks uncalled. Read each")
        print("  one; the question is whether it has a *production* caller, not whether it has any.")
        print()

    if unread:
        print(f"{len(unread)} field(s) set but never consulted:")
        for name, struct, field in unread:
            print(f"  {name} :: {struct}.{field}")
        print("\nEach is either a defect (D45) or wants a comment saying why it is carried.")
        return 1
    print(f"{len(rows)} public fields across {scanned} file(s) checked; every one is read somewhere.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

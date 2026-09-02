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


# Things a reader needs in order to *interpret* a result, which are not themselves drift.
# `main` prints these whatever the exit code. There is exactly one so far and it earned the
# channel: see the end of `counts_are_current`.
ADVISORIES = []


def counts_are_current():
    """Test counts and the decision range, both quoted in prose and both easy to forget."""
    problems = []
    highest = max(int(m) for m in re.findall(r"^## D(\d+) ", (ROOT / "docs" / "DECISIONS.md").read_text(encoding="utf-8"), re.M))
    for name, text in (("README.md", README), ("CLAUDE.md", CLAUDE)):
        for quoted in set(re.findall(r"D1[–-]D(\d+)", text)):
            if int(quoted) != highest:
                problems.append(f"{name} says D1–D{quoted}; DECISIONS.md reaches D{highest}")

    # **Tracked test targets only, because a bare `cargo test` counts a peer's scratch file.**
    # Other sessions work this tree concurrently; an untracked `tests/props_probe.rs` appeared
    # mid-commit and moved the count by one, so the number the docs are required to quote came
    # partly from a file that is not in the repository and would rot the moment its author
    # renamed it. `every_bench_script_is_named`, twenty lines below, was already repaired for
    # exactly this and the sibling was left with the hole — the third instance in this file of
    # the rule that fixing one case trains attention on the case rather than on its siblings.
    #
    # **A pathspec `*` matches across `/`, which this got wrong on the first attempt.**
    # `git ls-files "tests/*.rs"` also returns `tests/common/mod.rs`, so `--test mod` went on the
    # command line, cargo exited 101 with an empty stdout, and the count came back 0 — see below
    # for why that was silent. Only files directly under `tests/` are integration targets.
    tracked = sorted(
        pathlib.Path(f).stem
        for f in subprocess.run(
            ["git", "ls-files", "tests/*.rs"], capture_output=True, text=True, cwd=ROOT
        ).stdout.split()
        if f.count("/") == 1
    )
    if not tracked:
        # An inability to answer, not a clean result — `checks_can_still_fail`'s rule, applied to
        # the input rather than to the output.
        return problems + ["git lists no tracked integration tests — the count examined nothing"]
    targets = [a for stem in tracked for a in ("--test", stem)]
    out = subprocess.run(
        ["cargo", "test", "--no-fail-fast", "--lib", *targets],
        capture_output=True, text=True, cwd=ROOT,
    ).stdout
    actual = sum(int(m) for m in re.findall(r"^test result: ok\. (\d+) passed", out, re.M))
    # **`if actual:` was here and it made the bug above invisible** (M39). A cargo invocation that
    # never ran produces no `test result:` lines, so `actual` is 0, so the comparison was skipped
    # and the check printed success — the same sentence as `unreleased_is_honest`'s
    # `if not tag: return []`, in the function directly above the one repaired for it. Zero tests
    # is not a repository state worth tolerating; it is this check being unable to answer.
    if not actual:
        return problems + [
            "`cargo test` reported no passing tests — the count could not be taken, which is not"
            " the same as the docs being right"
        ]
    # **That number is the WORKING TREE's, and the commit's may be different** (M60). The count
    # above already ignores *untracked* files, which was the first version of this problem; the
    # second version is tracked files with uncommitted edits, and on this machine those are
    # routinely another session's. Twice now a count taken here described a tree nobody was about
    # to commit: once it would have published a README claiming seven tests that were not in the
    # commit, and CI — which only ever sees committed code — would have gone red exactly as it did
    # on 83f75b1.
    #
    # **It says so rather than blocking, and that is a decision.** Blocking would fire on this
    # project's own documented practice — stage selectively, because peers edit this tree
    # concurrently — and would have refused every commit made on 2026-09-02 while two sessions
    # worked. The failure it would prevent costs one red CI run and one follow-up commit; the
    # block would cost a bypass on every concurrent commit, and a gate routinely bypassed is worse
    # than one that is occasionally wrong. **CI is the authority on the committed count**; this is
    # a fast approximation, and the one thing it must not do is present itself as more.
    unstaged = [
        f
        for f in subprocess.run(
            ["git", "diff", "--name-only"], capture_output=True, text=True, cwd=ROOT
        ).stdout.split()
        if f.endswith(".rs")
    ]

    # The suite is deliberately platform-asymmetric — the statfs magic table compiles only on
    # Linux (M46: "CI's Linux leg is the assertor"), so one count stopped being checkable the
    # day that landed: whichever leg the docs quoted, the other platform's run disagreed, and
    # this check went red on CI while green at the gate. The docs therefore quote both legs as
    # `N tests (M on Linux)`, and this check verifies the leg it is actually standing on —
    # bare `N tests` quotes (no annotation) are still held to the local count, so a stray
    # unannotated number elsewhere stays guarded.
    on_linux = sys.platform.startswith("linux")
    for name, text in (("README.md", README), ("CLAUDE.md", CLAUDE)):
        for mac_n, linux_n in set(re.findall(r"(\d+) tests(?: \((\d+) on Linux\))?", text)):
            expected = int(linux_n) if (on_linux and linux_n) else int(mac_n)
            if expected != actual:
                leg = "Linux" if on_linux else "this platform"
                because = (
                    f" — and {len(unstaged)} tracked .rs file(s) have unstaged edits, so this is"
                    f" not the count CI will take: {', '.join(unstaged)}"
                    if unstaged
                    else ""
                )
                problems.append(
                    f"{name} quotes {expected} tests for {leg}; the suite here runs"
                    f" {actual}{because}"
                )
    if unstaged:
        ADVISORIES.append(
            f"the test count was taken over a working tree with unstaged edits to"
            f" {len(unstaged)} tracked .rs file(s), so it is not necessarily the count a clean"
            f" checkout runs: {', '.join(unstaged)}."
            " CI is the authority on the committed count."
        )
    return problems


def examples_show_the_current_schema():
    """README transcripts quoting `schema N` must show the schema this source builds.

    The install example sat at `schema 12` while the binary shipped 13 — a stale example of the
    versioning feature itself, invisible to every other check here because none of them read
    embedded output examples (U9). One deliberate exemption: the doctor transcript's
    "which reports" line *is* a stale binary, demonstrating staleness — the point of that
    example is the mismatch, so the line that says `which reports` keeps its old number.
    """
    problems = []
    m = re.search(r"^pub const SCHEMA_VERSION: i64 = (\d+);", (ROOT / "src" / "db.rs").read_text(encoding="utf-8"), re.M)
    if not m:
        return ["src/db.rs no longer declares SCHEMA_VERSION where this check reads it"]
    current = int(m.group(1))
    for lineno, line in enumerate(README.splitlines(), 1):
        if "which reports" in line:
            continue
        for quoted in re.findall(r"schema (\d+)", line):
            if int(quoted) != current:
                problems.append(
                    f"README.md:{lineno} shows `schema {quoted}`; the binary builds schema {current}"
                )
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
    """`[Unreleased]` saying nothing, while commits exist, is the drift itself.

    **This check switched itself off on 2026-08-31 and the gate stayed green.** It used to
    `git describe --tags` and `return []` when there was no tag. The history was then reset at the
    user's direction and re-initialised, which destroyed the `v0.1.0` tag — so the early return
    became unconditional and one of the six checks here passed without evaluating anything. Nothing
    said so, because a check that reports nothing and a check that finds nothing print identically
    (D88's shape, in the gate itself, disabled by an operation that had nothing to do with it).

    The repair is to stop depending on the tag rather than to recreate it. "Commits exist since the
    last release" and "commits exist at all" are the same question whenever no release has happened,
    and the second one cannot be switched off by a `git init`. A tag, when there is one, still
    narrows the count and the message.
    """
    tag = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0"], capture_output=True, text=True, cwd=ROOT
    ).stdout.strip()
    rev = f"{tag}..HEAD" if tag else "HEAD"
    n = subprocess.run(
        ["git", "rev-list", "--count", rev], capture_output=True, text=True, cwd=ROOT
    ).stdout.strip()
    section = CHANGELOG.partition("## [Unreleased]")[2].partition("\n## ")[0]
    # An *empty* section makes exactly the claim the placeholder makes, and the literal-only test
    # could not see it — the same shape as the rest of M36, one level in: the check existed, ran,
    # and could not fail on half the cases it is for.
    #
    # **Matched as the section's whole content, not as a substring, and that distinction is not
    # theoretical.** `"Nothing yet" in section` fired on the changelog entry *describing this very
    # fix*, because that entry quotes the sentinel in its prose — a check made unfailable in one
    # direction and unpassable in the other by the same commit. A sentinel searched for anywhere
    # cannot survive being written about, and a `CHANGELOG` is exactly where it gets written about.
    body = "\n".join(
        line
        for line in section.splitlines()
        if line.strip() and not line.lstrip().startswith("###")
    ).strip()
    placeholder = re.fullmatch(r"[-*]?\s*Nothing yet\.?", body, re.I) is not None
    silent = placeholder or not body
    if n.isdigit() and int(n) > 0 and silent:
        since = f"since {tag}" if tag else "in a history with no tag"
        how = "says 'Nothing yet'" if placeholder else "is empty"
        return [f"CHANGELOG [Unreleased] {how} with {n} commit(s) {since}"]
    return []


def the_gate_and_ci_run_the_same_checks():
    """Whatever `tools/verify.sh` runs, `.github/workflows/ci.yml` runs too.

    **D70 states this rule and says outright that a sentence is the only thing enforcing it.** The
    divergence it was written after is the proof it needs more: `check_secret_literals.py` was added
    to the gate and not to the workflow, so CI would have passed a commit the gate rejects — and the
    two disagreeing about what "verified" means is the one thing the workflow exists to prevent.

    Cheap and one-directional on purpose. A step in CI that is not in the gate is fine and expected:
    the matrix builds on Linux, which the local gate cannot do at all. A step in the gate that is
    missing from CI is the drift.
    """
    verify = (ROOT / "tools" / "verify.sh").read_text(encoding="utf-8")
    workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    problems = []
    for step in re.findall(r'^\s*run "([^"]+)"', verify, re.M):
        # The gate's label is a substring of CI's shell line, which is why this is `in` rather
        # than equality: `run "cargo test"` against `- run: cargo test`.
        #
        # It briefly took `step.split()[-1]` for `tools/` labels, which was a no-op for all six
        # current labels and wrong for any that grow a flag — `tools/foo.py --x` would have
        # matched on `--x`. Removed rather than repaired: the whole label is the thing that must
        # appear in CI.
        if step not in workflow:
            problems.append(
                f"tools/verify.sh runs '{step}' and .github/workflows/ci.yml does not"
                " — CI would pass a commit the gate rejects (D70)"
            )
    return problems


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
    # An empty index is not "no scripts are tracked" — it is an inability to answer, and skipping
    # every script on the strength of it is the same silent pass this file was just repaired for
    # (M35). `checks_can_still_fail` reports the cause; this returns rather than pretending.
    if not tracked:
        return ["git lists no tracked files under bench/ or tools/ — this check examined nothing"]
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


OPEN_QUESTIONS = (ROOT / "docs" / "OPEN-QUESTIONS.md").read_text(encoding="utf-8")

# A claim that questions have been retired. Deliberately tight: `OPEN-QUESTIONS.md` also contains
# prose *about* the convention ("none should be settled by whoever notices it first", "a settled
# question is deleted rather than annotated") and long blockquotes narrowing a question that is
# still open. None of those is a retirement, and a looser pattern reads all of them as one.
#
# **The slack sits between the auxiliary and `settled`, and that placement was found the hard
# way.** The first version required the two words adjacent and promptly refused to recognise the
# paragraph written to fix the drift it had just reported — which said "have *also* been settled".
# So the fragile part was never the verb form; it was assuming nobody puts a word in the middle.
# Enumerating more verb phrases would not have helped, and each one added is a branch no real
# paragraph exercises.
#
# Failing to recognise a retirement is the harmless direction — its questions go unaccounted and
# rule 4 reports them — but a rule that rejects ordinary English is one people switch off, which
# `records_are_uniquely_numbered` says of its own legitimate exception. Verified against the real
# file: exactly the three retirement paragraphs match and nothing else in it does.
RETIREMENT = re.compile(r"\bQ\d[^.\n]{0,100}?\b(?:was|were|has|have)\b[^.\n]{0,25}?\bsettled\b")


def _question_numbers(text):
    """Question numbers a span names, with `Q1-Q6` expanded to the six it stands for.

    The range form is not decoration. `**Q1-Q6 and Q9 were settled**` is how this file retires
    seven questions in one sentence, so a parser reading only the two endpoints accounts for
    three of them and reports four perfectly well-documented questions as lost.
    """
    found = {int(n) for n in re.findall(r"\bQ(\d+)\b", text)}
    for lo, hi in re.findall(r"\bQ(\d+)\s*[\u2013\u2014-]\s*Q?(\d+)\b", text):
        found.update(range(int(lo), int(hi) + 1))
    return found


def retired_questions_name_their_decision():
    """A question deleted from `OPEN-QUESTIONS.md` leaves its answer behind, or it is lost.

    That file's convention is to **delete** a settled question rather than annotate it, and until
    2026-08-31 the net under that convention was `git log`. The history was reset to publish the
    repository, so the net is gone — the prose of Q1-Q7, Q9 and Q12 survives only in an archive
    outside this repo. The file says so itself and draws the right conclusion: from now on a
    deleted question must leave its answer in `DECISIONS.md` rather than trusting `git log`. What
    it could not do is enforce it, so the convention lived in one sentence that nothing read.

    **It was already broken when that sentence was written, and the sentence is what hid it.** The
    reset note promises each deleted question "names the decision it became, immediately below".
    Q12 appears in its own list and is named nowhere below it. Q13 was settled into D98 the same
    day and does not appear in the file at all. Both answers exist and are correct — D85 and D98 —
    but neither was reachable from the register that promised them, and the register read as
    complete because everything it *did* say was true.

    Four rules, and the fourth is the one that fires:

      1. a retirement paragraph names at least one decision,
      2. every decision it names exists as a `## Dn` heading,
      3. nothing is called settled while it still carries an open `## Qn` section, and
      4. **every question number the docs cite is either open or retired by name.**

    Rules 1-3 inspect what is written, so they can only catch a claim somebody made. Rule 4 is the
    absence check this project keeps having to learn it needs: take the union of every `Qn` any
    document mentions, subtract the open sections and the retired ones, and report the remainder.
    Presence checks cannot see a paragraph that was deleted; arithmetic over the numbers can.

    **The residual hole is at the top of the range**, and naming it is cheaper than pretending it
    is closed. A question created and deleted while no other document ever cited it leaves nothing
    for rule 4 to subtract, and only the archive can see that. What rule 4 does cover is the case
    that has actually happened twice here: the answer was written down properly and only the
    pointer from the register was lost.

    A retirement paragraph this fails to *recognise* fails loudly rather than quietly — its
    questions go unaccounted and rule 4 reports them. For a pattern match, that is the safe
    direction to err in.
    """
    decisions = (ROOT / "docs" / "DECISIONS.md").read_text(encoding="utf-8")
    recorded = {int(n) for n in re.findall(r"^## D(\d+) ", decisions, re.M)}
    still_open = {int(n) for n in re.findall(r"^## Q(\d+) ", OPEN_QUESTIONS, re.M)}

    problems, retired = [], set()
    for para in re.split(r"\n\s*\n", OPEN_QUESTIONS):
        if not RETIREMENT.search(para):
            continue
        # **Scoped to the match, not to the paragraph, and the difference is not pedantic.** The
        # regex spans from the first `Qn` to `settled`, so its match *is* the subject phrase:
        # "Q1-Q6 and Q9 were settled" yields exactly those seven. Reading the whole paragraph
        # instead swept up every cross-reference in it — the paragraph retiring Q8 closes by
        # pointing at Q14, which is open, and rule 3 duly reported Q14 as settled-and-still-open.
        # Found by this check on its own author's prose, an hour after it was written. `finditer`
        # rather than `search` so a paragraph retiring two questions in two sentences counts both.
        claimed = set().union(*(_question_numbers(m.group(0)) for m in RETIREMENT.finditer(para)))
        # Decisions stay paragraph-scoped, because that is where they are written: the subject
        # phrase names questions and the sentence after it names the decisions they became.
        named = {int(n) for n in re.findall(r"\bD(\d+)\b", para)}
        retired |= claimed
        label = ", ".join(f"Q{n}" for n in sorted(claimed))
        if not named:
            problems.append(
                f"OPEN-QUESTIONS.md retires {label} and names no decision — the answer is"
                " recoverable only from the archive outside this repository"
            )
        for d in sorted(named - recorded):
            problems.append(
                f"OPEN-QUESTIONS.md sends {label} to D{d}, which is no heading in DECISIONS.md"
            )
        for q in sorted(claimed & still_open):
            problems.append(
                f"OPEN-QUESTIONS.md calls Q{q} settled and still carries its open `## Q{q}` section"
            )

    # Rule 4. Every doc, because the citation that proves a question existed is as likely to be in
    # MEASUREMENTS.md as in DECISIONS.md — Q13's only surviving pointers were one of each.
    cited = set()
    for doc in [ROOT / "README.md", ROOT / "CLAUDE.md", *sorted((ROOT / "docs").glob("*.md"))]:
        cited |= _question_numbers(doc.read_text(encoding="utf-8"))
    for q in sorted(cited - still_open - retired):
        problems.append(
            f"Q{q} is cited in the docs, is not open in OPEN-QUESTIONS.md, and no retirement"
            " paragraph there says which decision it became"
        )
    return problems


def checks_can_still_fail():
    """Every check above needs something to examine, and an empty input is not a clean bill of health.

    **The generalisation of M35, and the industry has a name for both halves.** The failure is the
    one Vitest guards with `passWithNoAssertions: false` — its canonical case is seven integration
    tests passing while the dev server was never running, because an early return meant no assertion
    was ever reached. `unreleased_is_honest` was `if not tag: return []`, which is the same sentence
    in Python. The remedy is the other half: MongoDB's *canary test*, which tests the testbed rather
    than the software, so that a broken harness is distinguishable from a healthy subject.

    This is that canary. It does not check the repository; it checks that the checks above are still
    looking at something. Each entry names a population that must be non-empty for some check above
    to be *able* to fail, and none of them can be non-empty by accident.

    The concrete window this closes is not hypothetical. On 2026-08-31 `.git` was deleted and
    re-initialised; between `git init` and the first `git add` the index was empty, and in that
    window every check keyed on `git ls-files` — including `check_secret_literals.py` — would have
    reported success having read no files at all. On the one operation whose entire purpose was
    getting past secret scanning.
    """
    problems = []
    tracked = subprocess.run(
        ["git", "ls-files"], capture_output=True, text=True, cwd=ROOT
    ).stdout.split()
    if not tracked:
        problems.append(
            "git lists no tracked files — every check keyed on the index examined nothing,"
            " including tools/check_secret_literals.py"
        )
    populations = {
        "docs/*.md": list((ROOT / "docs").glob("*.md")),
        "decisions in DECISIONS.md": re.findall(
            r"^## D\d+ ", (ROOT / "docs" / "DECISIONS.md").read_text(encoding="utf-8"), re.M
        ),
        "measurements in MEASUREMENTS.md": re.findall(
            r"^## M\d+ ", (ROOT / "docs" / "MEASUREMENTS.md").read_text(encoding="utf-8"), re.M
        ),
        # Rules 1-3 of `retired_questions_name_their_decision` iterate over these and are dead
        # without one. Rule 4 still fires on an empty file — loudly, on every question ever
        # deleted — so the check does not go fully vacuous, but three quarters of it would.
        "retirement claims in OPEN-QUESTIONS.md": RETIREMENT.findall(OPEN_QUESTIONS),
        "steps in tools/verify.sh": re.findall(
            r'^\s*run "', (ROOT / "tools" / "verify.sh").read_text(encoding="utf-8"), re.M
        ),
    }
    for name, found in populations.items():
        if not found:
            problems.append(f"{name} is empty — the check that reads it cannot fail")
    return problems


def main():
    if not BIN.exists():
        print(f"no debug binary at {BIN} — run `cargo build` first")
        return 2
    problems = (
        every_doc_is_indexed()
        + every_command_is_documented()
        + counts_are_current()
        + examples_show_the_current_schema()
        + records_are_uniquely_numbered()
        + unreleased_is_honest()
        + the_gate_and_ci_run_the_same_checks()
        + retired_questions_name_their_decision()
        + checks_can_still_fail()
        + every_bench_script_is_named()
    )
    # Before the verdict, not after: an advisory that qualifies a result has to be read with it.
    for a in ADVISORIES:
        print(f"  ~ {a}")
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

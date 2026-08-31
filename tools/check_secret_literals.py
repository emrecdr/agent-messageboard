#!/usr/bin/env python3
"""Refuse a credential-shaped literal in tracked source.

**This exists because of a push, not a leak.** GitHub push protection blocked the first push of
this repository on five commits, flagging a Slack token and a Stripe key in `src/memory.rs`. Every
one was a *test fixture* for `redact.rs` — the module whose whole job is catching credential
shapes — and none was a real secret: the AWS one is `AKIAIOSFODNN7EXAMPLE`, Amazon's own published
placeholder, and another literally spells the alphabet.

That is the permanent condition, not an accident: **you cannot test a secret-redactor without
strings shaped like secrets.** So the fixtures are built with `concat!`, which rejoins them at
compile time while leaving no contiguous match in the file. The value under test is byte-identical;
the scanner sees nothing.

Two ways that decays, and both are silent:

  1. Somebody adds a fixture as a plain literal. The next push is blocked, by which point the
     commit is already written and the fix is a history question rather than an edit.
  2. Somebody "tidies" a `concat!` back into one string, because it looks pointlessly split. This
     project's recorded failure mode exactly — a negative decision leaves no trace in the code and
     gets helpfully fixed later.

Exit 1 on any match, listing file, line and the matched prefix. The body is never printed: this
tool reports that something is credential-shaped, and printing it would put the thing it objects
to into a terminal, a CI log and a scrollback.
"""

import re
import subprocess
import sys

# The prefixes `memory::redact::SECRET_PREFIXES` knows, which is also roughly what the scanners
# look for. Kept here rather than parsed out of the Rust so this tool still runs if that file is
# the one being broken.
PREFIXES = [
    "xoxb-", "xoxp-", "xoxa-", "xoxs-", "xapp-",
    "sk_live_", "sk_test_", "pk_live_", "rk_live_",
    "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_",
    "glpat-", "glrt-", "AKIA", "ASIA", "AIza", "ya29.",
    "hf_", "npm_", "dop_v1_", "sq0csp-", "shpat_",
    "sk-ant-", "sk-proj-", "pypi-", "SG.",
]
# A prefix immediately followed by enough body to look like a credential. Six is below every real
# token length and above the point where a prefix appears in ordinary prose.
PATTERN = re.compile(
    "(" + "|".join(re.escape(p) for p in PREFIXES) + r")[A-Za-z0-9_.\-]{6,}"
)

# Where fixtures legitimately live. Everything is checked; this only shapes the advice.
FIXTURE_FILES = ("redact.rs", "memory_e2e.rs", "messages.rs")


def tracked_text_files():
    out = subprocess.run(
        ["git", "ls-files", "-z", "--", "src", "tests", "tools", "bench", "docs", "*.md"],
        capture_output=True, text=True, check=True,
    )
    return [f for f in out.stdout.split("\0") if f]


def main() -> int:
    # **Reading nothing is not the same as finding nothing** (M35). `check=True` above catches git
    # *failing*; it does not catch git succeeding with an empty index, which is exactly what sits
    # between `git init` and the first `git add`. This repository passed through that state on
    # 2026-08-31, during a history reset whose entire purpose was getting past secret scanning —
    # and in that window this check would have printed "no credential-shaped literal in tracked
    # source" having opened no file at all.
    paths = tracked_text_files()
    if not paths:
        print(
            "git lists no tracked files, so this check read nothing. That is an inability to"
            " answer, not a clean result.",
            file=sys.stderr,
        )
        return 1

    findings = []
    for path in paths:
        if path == "tools/check_secret_literals.py":
            continue  # the prefix list above is not a credential
        try:
            with open(path, encoding="utf-8") as fh:
                lines = fh.readlines()
        except (OSError, UnicodeDecodeError):
            continue
        for n, line in enumerate(lines, 1):
            m = PATTERN.search(line)
            if m:
                findings.append((path, n, m.group(1)))

    if not findings:
        print("no credential-shaped literal in tracked source.")
        return 0

    print(f"{len(findings)} credential-shaped literal(s) in tracked source:")
    for path, n, prefix in findings:
        print(f"  {path}:{n} — begins {prefix!r}")
    print()
    print("If this is a test fixture, split it so no contiguous match exists in the file:")
    print('    concat!("ghp_", "16CharsOfSecretHere")')
    print("The value is rejoined at compile time, so the test is unchanged.")
    print()
    print("If it is a real credential, it is already in your working tree — rotate it first.")
    return 1


if __name__ == "__main__":
    sys.exit(main())

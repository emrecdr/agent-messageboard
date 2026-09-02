#!/usr/bin/env bash
# Every quality claim this project makes, in one command.
#
# **The problem this solves is not that the checks are missing. It is that nothing runs them.**
# `cargo test`, `cargo clippy`, `cargo fmt --check`, `tools/check_docs.py` and
# `tools/find_unread_fields.py` all exist and all pass. Whether they were run before a commit
# depended entirely on whoever was committing remembering to — in a repository several concurrent
# agents write to. A guarantee enforced by memory is not enforced.
#
# **Why this is the gate, now that CI also runs.** Until 2026-08-31 this repository had no remote,
# so `.github/workflows/ci.yml` could not execute and shipping it as the fix would have been a guard
# that cannot fire — the defect class D45, D51 and D58 record. The repository was then published and
# that workflow ran, green on Linux and macOS.
#
# It changes nothing about which check is primary. **CI fires after a commit is pushed; this fires
# before one is written**, and only the second stops a bad commit from existing. The workflow
# duplicates these checks so the two cannot disagree about what "verified" means — they had already
# diverged once, over `check_secret_literals.py` — so anything added here belongs there too (D70).
#
# Cost: **the script measures it and prints it** — the last line of a run is `gate: Ns`. Two
# states differ by roughly 3x. With nothing changed since the last run the gate is mostly the two
# audit scripts, because clippy and the suite are cache hits; after touching a source file it is
# dominated by clippy and the suite. Dated figures live in D70 and M28, where they are records of
# when they were taken rather than claims about now.
#
# **A cost claim in a comment is a constant that rots, and this one rotted twice** — 6.5s, then
# 16.9s, then neither. The second was worse than stale: `16.9s warm` named a cache state spanning
# both cases above, so it was reproducible in neither and misread in both directions — against the
# cheap state it reads as padded, against the expensive one as a shipped regression (M28).
#
# `tools/mutants.sh` carried the same defect in the same session — a hardcoded line range that
# silently truncated its own usage text — and was fixed the same way: **stop asserting a
# measurement, derive it.** This comment previously replaced one rotting literal with two, which is
# the same mistake at a higher resolution.
#
# A first run after any change to the clippy flags or the dependency graph rebuilds everything —
# 172s observed on this machine under load; after `cargo clean`, expect longer still.
#
# **It said 6.5s until 2026-08-29 and that had quietly become false.** The figure was honest when
# written; the suite has since grown from roughly 250 tests to 376 and two audit scripts were
# added. Nothing failed, which is why it drifted — a cost claim is prose, and prose rots. Quoted
# from three runs rather than one because an earlier draft of this comment said 13.5s, measured
# with a different flag set before `--all-features -D warnings` was added, which is the shape
# MEASUREMENTS.md M5 and M7 already record.
#
# Escape hatch: `AMB_VERIFY_SKIP=1 git commit …`. Deliberately present and deliberately loud —
# a gate with no override gets disabled wholesale the first time it is inconvenient, and then
# nothing runs again.

set -euo pipefail

cd "$(dirname "$0")/.."

# Measured, not asserted: see this script's header.
SECONDS=0

# Each check names itself before running, so a failure is attributable without reading the output
# above it. `cargo test` is run before the Python audits because `check_docs.py` needs the debug
# binary and refuses with exit 2 without one.
step() { printf '\n\033[1m→ %s\033[0m\n' "$1"; }

# Every Rust project on this machine shares one target directory (`~/.cache/cargo-target`), and
# several agent sessions work these repositories at once. A concurrent `cargo` run produces
# failures in code you did not touch — observed while writing this script: `identity_e2e`'s
# worktree test failed with `.git/index: index file open failed: Not a directory`, then passed
# 5/5 in isolation seconds later, while a peer was committing twice.
#
# **Said before the checks rather than after them.** A spurious failure that arrives with no
# explanation is indistinguishable from a real one, and the first thing it costs is trust in the
# gate — which is how a gate stops being run. CLAUDE.md already tells a human to "check for
# another build before debugging"; this does the checking.
if pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1; then
  printf '\033[33m! another cargo/rustc is running — this machine shares one target directory,\n'
  printf '  so a failure below may belong to that build rather than to your change.\n'
  printf '  Re-run before believing it.\033[0m\n'
fi

failed=()
run() {
  local name="$1"; shift
  step "$name"
  if "$@"; then
    return 0
  fi
  # Collected rather than fatal, so one run reports every problem instead of only the first.
  # A commit blocked twice for two reasons it could have shown at once trains people to use
  # the escape hatch.
  failed+=("$name")
  return 0
}

# `--locked` on everything that resolves dependencies. The 2026-08-20 crates.io incident —
# a compromised maintainer account shipping build-time code execution through a typosquatted
# proc-macro, exposure window ~90 minutes — is defended at this project's scale by exactly two
# things: a committed lockfile that only moves when someone moves it, and the advisory check CI
# runs. `--locked` is what makes the first one enforced rather than customary: a resolve that
# wants to differ from Cargo.lock fails loudly instead of updating it as a side effect.
run "cargo fmt --check"          cargo fmt --check
run "cargo clippy --locked --all-targets" cargo clippy --locked --all-targets --all-features -- -D warnings
run "cargo test --locked"        cargo test --locked --quiet
run "tools/check_docs.py"        python3 tools/check_docs.py
run "tools/find_unread_fields.py" python3 tools/find_unread_fields.py
run "tools/check_mutation_coverage.py" python3 tools/check_mutation_coverage.py
run "tools/check_secret_literals.py" python3 tools/check_secret_literals.py

if [ ${#failed[@]} -ne 0 ]; then
  printf '\n\033[31m✗ %d check(s) failed:\033[0m\n' "${#failed[@]}"
  printf '    %s\n' "${failed[@]}"
  printf '\nTo commit anyway: AMB_VERIFY_SKIP=1 git commit …\n'
  printf 'gate: %ss\n' "$SECONDS"
  exit 1
fi

printf '\n\033[32m✓ all checks passed\033[0m  gate: %ss\n' "$SECONDS"

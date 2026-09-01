#!/usr/bin/env bash
# **Mutation testing, which this project treats as the only evidence a guard is real.** Two things
# it is important to know before reading a result, both stated first because both have already
# produced a wrong answer here:
#
#   1. **A result produced while anything else was building is void, not weak.** `cargo mutants`
#      copies the source tree but NOT the cargo config, so it inherits this machine's shared
#      `target-dir` and compiles every mutant in under this package's own name. On 2026-08-29
#      `verify.sh` ran mid-pass and tested a *mutant*; afterwards `cargo test` reported 225 lib
#      tests against a source holding 231, six of them silently absent from a stale binary.
#      `cargo clean -p amb` freed 17.3 GiB — that volume under one package name IS the collision.
#      The re-run proved discarding was right: `Message::scope -> ""` read as missed when polluted
#      and caught when clean. This script forces a private target directory; do not remove it, and
#      do not run anything else while it works (M17).
#
#   2. **`--diff` mode cannot see a change to a test.** cargo-mutants matches the diff against the
#      code under test only, so a commit that *deletes a test* generates no mutants and passes
#      green. That is precisely the change mutation testing exists to catch, which makes diff mode
#      useful for feedback and disqualifying as a gate. It is offered here and deliberately not
#      wired into `verify.sh` or `.githooks/pre-commit`. A mechanism that cannot reach the case it
#      exists for is this project's most repeated defect (D58, D91).
#
# It is slow enough not to be a commit gate — the same reason `tools/bench.sh` is not one — and
# **cargo-mutants reports its own cost on the last line** (`N mutants tested in Xm`), so read that
# rather than a rate quoted here. This comment said "roughly 25 s per mutant" until 2026-08-31,
# when `status.rs` measured 94 mutants in 5m, about 3 s each: off by 8x, a third rotted constant in
# this file's own header (M28). Cost per mutant depends on how fast the suite fails under it, which
# is a property of the mutant, not of the tool.
#
# **Three traps that make a run report the wrong thing, each hit on 2026-08-31:**
#
#   - **A MISSED row in `#[cfg]`'d-out code means "not compiled here", not "untested".** Mutating
#     a Linux-only function on macOS builds fine — the mutated code is simply absent from the
#     binary — and every test passes, so the row prints MISSED and no test on this machine can
#     ever redden it. db.rs reported 16 such rows in one run (M46). Read the platform gate before
#     prosecuting a survivor, and assert foreign-platform code in tests cfg'd to where it
#     compiles: CI's other leg is the assertor.
#
#   - **Do not pipe this script anywhere.** `tools/mutants.sh … | tail` reports *tail's* exit
#     status, and a baseline failure then prints `exit 0` beside a run that tested **nothing**.
#     cargo-mutants exits 2 when mutants survive and 101-ish when the baseline breaks; both are
#     information, and a pipe throws them away. Redirect with `>` if you need the output in a file.
#
#   - **The private target directory must not live under `$TMPDIR`, and the reason is an mtime
#     from 2006.** It used to, and macOS's age-based cleaner ate `libsqlite3-sys`'s generated
#     `bindgen.rs` — not merely between runs but *mid-run*: the bundled build script writes that
#     file with its packaged 2006 timestamp, so it is perpetually eligible for eviction the
#     moment it lands. Two consecutive baselines failed on the same missing file with the
#     directory freshly deleted in between (2026-09-01), which is what proved the cleaner was
#     concurrent rather than nightly. The directory now lives under `~/.cache`, where no cleaner
#     runs, and the script refuses a `$TMPDIR`-resolved target outright below — a note saying
#     "do not move it back" is the D39/D45 failure, so the check is mechanical.
#
# Usage:
#   tools/mutants.sh src/claims.rs [src/other.rs ...]   one or more modules, exhaustively
#   tools/mutants.sh --diff [<git-diff-args>]           only lines changed (see caveat 2)
#
# Read the score against this project, not against an industry figure. A mutation score's
# denominator is "viable mutants this tool's operators happened to generate", which is not
# comparable across tools, languages or codebases — quoting 83% against a published 80% would be
# question 1 of the ratio rule with extra steps. What the number is good for is *this module,
# before and after*: `messages.rs` went 60/72 to 72/72, and the 12 were five real silences.

set -uo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo-mutants >/dev/null 2>&1; then
  echo "cargo-mutants is not installed:  cargo install cargo-mutants --locked" >&2
  exit 127
fi

# Private, so a mutant can never land in the shared target directory. Kept between runs: a cold
# build costs ~90s and the incremental rebuild per mutant is most of what makes this affordable.
export CARGO_TARGET_DIR="${AMB_MUTANTS_TARGET:-${XDG_CACHE_HOME:-$HOME/.cache}/amb-mutants-target}"

# Refuse, not warn: a $TMPDIR-resolved target dir fails as a missing generated file mid-run,
# which reads as a broken test suite, never as this line (see the 2006-mtime trap in the header).
case "$CARGO_TARGET_DIR" in
  "${TMPDIR:-/tmp}"*)
    echo "refusing: CARGO_TARGET_DIR ($CARGO_TARGET_DIR) resolves under \$TMPDIR, where macOS's" >&2
    echo "age-based cleaner eats libsqlite3-sys's 2006-mtime bindgen.rs mid-run (header, trap 2)." >&2
    echo "Point AMB_MUTANTS_TARGET somewhere durable, e.g. ~/.cache/amb-mutants-target" >&2
    exit 78
    ;;
esac

# `--copy-vcs true` or `build.rs` cannot fingerprint the repository and the BASELINE fails before
# a single mutant runs — the first symptom anyone hits, and it looks like a broken test suite.
# `--jobs 1` because the private target directory above is one directory: parallel jobs would
# serialise on its lock and reintroduce exactly the contention this script exists to prevent.
#
# **The timeout is relative, and a fixed one broke this.** It was `--timeout 180`, which cargo-
# mutants applies to the *baseline* as well as to each mutant. The suite runs in ~3s normally and
# ~145s here — every e2e test spawns the binary, and a spawn in the sandbox costs orders more than
# in the shared target directory — so the margin was 81% consumed before anyone added a test.
# Adding one that spawns twelve processes crossed it, and the run reported `TIMEOUT Unmutated
# baseline` and tested **nothing**. Loud, but only to a reader; a fixed ceiling under a growing
# suite fails on whichever commit happens to cross it. `--timeout-multiplier` scales with the
# measured baseline, which is the property a constant cannot have, and the baseline itself is then
# measured rather than raced.
#
# **The `~145s` above was not reproduced, and it is the third rotted constant in this header**
# (M39). On 2026-08-31 `src/hooks.rs` ran with the baseline at `7s build + 5s test`, and the
# eighteen logged test phases spanned 5-11s. Those eighteen are all *survivors*, which is the
# strong form of the measurement: a caught mutant fails fast, a survivor runs the suite to the
# end, so 11s is an upper bound on a full run in the sandbox rather than a sample of quick ones.
#
# **What that changes and what it does not.** The relative timeout stays — but not for the reason
# the paragraph above gives, because the margin under a fixed 180s ceiling was never 81% consumed.
# It stays for M27's reason, which is independent and still measured: the ceiling is set once from
# a baseline taken at minute zero, so a machine quiet then and busy at minute twenty times out
# mutants for reasons that have nothing to do with them. A constant cannot track that. M28 already
# found two rotted numbers in this header, and the pattern is the part worth carrying — a comment
# explaining a mechanism rots fastest in the figures it uses to justify itself, because nothing
# reads them.
#
# **It is measured once, at the start, and that is the residual hole (M27).** If the machine is
# quiet at minute zero and busy at minute twenty, the ceiling is stale and mutants time out for
# reasons that have nothing to do with them. Measured 2026-08-30, same module and same commit:
# a quiet baseline printed `Auto-set test timeout to 120s` — the `--minimum-test-timeout` floor —
# and one mutant reported TIMEOUT; a loaded baseline (104s build + 248s test, 8.5x slower) set the
# ceiling at 746s and none did. Re-running that mutant by hand, the whole suite passed in 20s: it
# had been a live survivor all along.
#
# So **a TIMEOUT row is an unanswered question, not a caught mutant.** Resolve each one by hand
# before reading a score — filed as "probably caught" it removes a real survivor from the count,
# and always in the flattering direction.
COMMON=(--copy-vcs true --jobs 1 --timeout-multiplier 3 --minimum-test-timeout 120)

if [ "${1:-}" = "--diff" ]; then
  shift
  DIFF="$(mktemp)"; trap 'rm -f "$DIFF"' EXIT
  git diff "${@:-HEAD}" > "$DIFF"
  if [ ! -s "$DIFF" ]; then
    echo "no diff against ${*:-HEAD} — nothing to mutate" >&2
    exit 0
  fi
  printf '\033[33m! diff mode is blind to changes in test code — see this script'\''s header\033[0m\n' >&2
  exec cargo mutants "${COMMON[@]}" --in-diff "$DIFF"
fi

if [ $# -eq 0 ]; then
  # Derived, not a line range: a hardcoded '2,32p' silently truncated this header mid-sentence
  # the moment it grew (M28). Prints the leading comment block and stops at the first line
  # that is not one, so it cannot fall out of step with the text it exists to show.
  awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$0"
  exit 64
fi

FILES=()
for f in "$@"; do
  [ -f "$f" ] || { echo "no such file: $f" >&2; exit 66; }
  FILES+=(--file "$f")
done

echo "target dir: $CARGO_TARGET_DIR"
echo "run nothing else until this finishes."
exec cargo mutants "${COMMON[@]}" "${FILES[@]}"

#!/usr/bin/env bash
# **This verifies that each harness still executes and still measures what a document says it
# measures. It asserts nothing about the numbers.**
#
# Stated first because the distinction decides whether the script is honest. A harness check and a
# performance gate look identical from the outside and cannot be the same thing here: this machine
# runs many live sessions, and the project's own rule is to repeat a measurement before quoting it
# rather than to pin a threshold. A script that failed on a value would be asserting a comparison
# across conditions the rule forbids — a false comment on the gate itself.
#
# Every measurement harness this project publishes numbers from, in one command.
#
# **Separate from `tools/verify.sh` on purpose, and the reason is a number.** These four take
# roughly 17s together — `bench_queue.py` alone is 11.5s because it spawns 17 concurrent writers
# — against a gate that is meant to run before every commit. A pre-commit hook that triples in
# cost gets disabled, and then nothing runs at all, which is the failure `verify.sh`'s own header
# describes.
#
# **What "wired in" means here is not "run on every commit".** It is that the scripts have one
# entry point with an exit code, that each one fails loudly when it stops measuring what a
# document says it measures, and that `check_docs.py` refuses to let one become orphaned.
#
# **That sentence was false for half of them when it was written, which is the joke at this
# script's expense** (M18). `bench_memory.py` and `bench_attribution.py` had no guard of any
# kind — and both had been measuring an *empty vault* since D81 renamed the `scope:` frontmatter
# key, because each wrote synthetic notes with the old `project:` key from its own private copy of
# the format. `memory index` reported `1000 scanned · 0 indexed` and said why, on every note; both
# scripts passed `capture_output=True` and discarded it. A script written to enforce "an
# unverified measurement script is a false comment with a shebang" was itself one.
#
# All four now guard. The note format lives once, in `bench/_harness.py`, next to the
# `index_or_die` that makes a schema change loud instead of silent.
#
# The defect that prompted this: `bench_startup.py` had its `amb` rows commented out behind
# "Uncomment once built" — for as long as the binary had existed — while README.md published
# `amb --version` at 2.1 ms and `amb inbox` at 3.0 ms and named this file as the harness. Nothing
# failed. The script ran, printed three rows and exited 0. **An unverified measurement script is a
# false comment with a shebang.**
#
# These print numbers; they do not assert them. A benchmark that fails on a slow machine is a
# gate nobody trusts, and this project's rule is to repeat a measurement before quoting it rather
# than to pin it. What is asserted is *coverage*: that each harness measured the thing it claims.

set -uo pipefail

cd "$(dirname "$0")/.."

step() { printf '\n\033[1m→ %s\033[0m\n' "$1"; }

if [ ! -x "$HOME/.cache/cargo-target/release/amb" ] && [ ! -x target/release/amb ]; then
  printf '\033[33m! no release binary — run `cargo build --release` first, or the rows that\n'
  printf '  matter will be skipped.\033[0m\n'
fi

failed=()
for b in bench/bench_startup.py bench/bench_memory.py bench/bench_attribution.py bench/bench_queue.py; do
  step "$b"
  # Collected rather than fatal, for `verify.sh`'s reason: one run should report every problem.
  python3 "$b" "$@" || failed+=("$b")
done

if [ ${#failed[@]} -ne 0 ]; then
  printf '\n\033[31m✗ %d harness(es) failed:\033[0m\n' "${#failed[@]}"
  printf '    %s\n' "${failed[@]}"
  exit 1
fi

printf '\n\033[32m✓ every harness ran and measured what it claims to\033[0m\n'

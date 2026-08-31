#!/usr/bin/env bash
# Build `amb` and update **every** copy of it, because the copies are what actually run.
#
# **The failure this closes has recurred five times, and detection was already shipped.** The hooks
# in `~/.claude/settings.json` invoke an absolute path — `~/.local/bin/amb` here — while
# `cargo install --path .` writes `~/.cargo/bin/amb`, which is also what `PATH` resolves first.
# After a schema change that produces the worst possible split: **every manual `amb` command works
# perfectly while every hook on the machine fails silently**, which is exactly why it goes
# unnoticed. D73 made `amb doctor` compare the fingerprints so the condition is *visible*; it was
# visible again within minutes of the next commit. Detecting a failure that recurs on every commit
# is not the same as closing it.
#
# So the copy stops being a thing to remember. Use this instead of `cargo install`.
#
# Hook paths are read out of the settings file rather than hardcoded, so a machine that installed
# the hooks somewhere else is still covered — and so this script cannot quietly go stale in the
# same way the binary did.

set -euo pipefail

cd "$(dirname "$0")/.."

printf '\n\033[1m→ cargo build --release\033[0m\n'
cargo build --release

# The shared target directory (`~/.cache/cargo-target`) is this machine's; the in-tree path is the
# default. Try both rather than assuming either.
BIN=""
for c in "$HOME/.cache/cargo-target/release/amb" "target/release/amb"; do
  [ -x "$c" ] && BIN="$c" && break
done
[ -n "$BIN" ] || { printf '\033[31m✗ no release binary was produced\033[0m\n'; exit 1; }
printf '  built %s\n' "$("$BIN" --version)"

# Every destination: the PATH copy, plus every distinct path an installed hook actually invokes.
DESTS=("$HOME/.cargo/bin/amb" "$HOME/.local/bin/amb")
SETTINGS="$HOME/.claude/settings.json"
if [ -f "$SETTINGS" ]; then
  while IFS= read -r p; do
    [ -n "$p" ] && DESTS+=("$p")
  done < <(python3 - "$SETTINGS" <<'PY'
import json, re, sys
try:
    s = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(0)
seen = set()
def walk(node):
    if isinstance(node, dict):
        cmd = node.get("command")
        if isinstance(cmd, str):
            # The hook command is `<path> hook <mode>`; take the executable only.
            exe = cmd.split()[0].strip('"')
            if exe.endswith("amb") and exe.startswith("/") and exe not in seen:
                seen.add(exe); print(exe)
        for v in node.values():
            walk(v)
    elif isinstance(node, list):
        for v in node:
            walk(v)
walk(s)
PY
  )
fi

printf '\n\033[1m→ updating every copy\033[0m\n'
DONE=()
for d in "${DESTS[@]}"; do
  case " ${DONE[*]:-} " in *" $d "*) continue ;; esac
  DONE+=("$d")
  mkdir -p "$(dirname "$d")"
  cp "$BIN" "$d"
  printf '  %s\n' "$d"
done

printf '\n\033[1m→ amb doctor\033[0m\n'
"$BIN" doctor

#!/usr/bin/env bash
# **This prints what a person actually gets, against a copy of the real board. It asserts almost
# nothing, it is not a gate, and its whole value is that someone reads the output.**
#
# Stated first because the distinction decides what the script is, the same way `bench.sh` opens by
# saying it checks harnesses rather than values. A gate answers *did anything change*; this answers
# *what does a session actually see right now*, and those need different tools.
#
# **Why it exists: the third source of truth had no script** (M29). Tests and mutation both work on
# code against fixtures. Neither can see a defect in the *composition* of correct parts — M29 was a
# banner announcing schema 9 to a board running schema 12, with every test passing, mutation at
# 88/91 and `doctor` green, because D23, D24 and D33 were each doing exactly what they were built
# to do. M24 was a rendered line with a run of spaces that every `contains` assertion straddled.
# Both were found by a person reading real output, and that was the only way either could be found.
#
# This is the industry's "smoke test against the deployed system" rather than its "integration test
# against a fixture", and the distinction is the reason a fixture cannot substitute: a fixture is
# built to match the code, so drift between accumulated state and current code is exactly what it
# can never contain. The real board has four days of other sessions' messages, a vault of 46 notes
# and a schema that has moved four times. That accumulation IS the input under observation.
#
# **The standard caution about running against production data is answered by copying, not by
# ignoring it.** Every command below runs against a throwaway copy, so nothing here can mark mail
# read, record an injection into `note_events`, or move a counter the D87 measurement window is
# computed over. The copy is made with `sqlite3 .backup` rather than `cp`, because the board is in
# **WAL mode** and its `-wal` sidecar holds committed transactions the main file does not — `cp
# board.db` alone silently produces a board missing its most recent writes, which would make this
# script quietly show a stale picture while claiming to show the live one.
#
# **The two assertions are about this script's own safety claim, and they are here because a claim
# with nothing able to check it is the shape D95 names.** "Read-only" is easy to write and easy to
# break: one command run without `AMB_DB` pointed at the copy would write to the real board and
# nothing would say so. So the real board's digest and the vault's listing are taken before and
# after, and a mismatch is the one thing that makes this script exit non-zero.
#
# **One cross-artefact check, printed as a warning and never as a failure.** Schema numbers appearing
# in rendered text are compared against the schema the board is actually at. That single comparison
# is M29's defect, and no unit test can make it because no unit holds both numbers. It warns rather
# than fails because a *message body* legitimately carries a stale number — it is a record of what
# someone said at the time, and correcting it is not this script's business.
#
# Usage:
#   tools/eyeball.sh              every surface, against a copy of the real board
#   AMB_BIN=./target/debug/amb tools/eyeball.sh    eyeball a build you have not installed yet
#
# By default it runs **the binary the installed hooks invoke**, read out of `~/.claude/settings.json`
# rather than assumed, because a stale copy of that binary is a five-times-recurring condition here
# (D73, D94) and is one of the things worth seeing. Run `./tools/install.sh` first if you want this
# to reflect your working tree.
#
# What you see is what *this* session sees: identity comes from `CLAUDE_CODE_SESSION_ID`, so the
# inbox and the delivery banner are yours and another agent's would differ.

set -uo pipefail
cd "$(dirname "$0")/.."

bold() { printf '\033[1m%s\033[0m\n' "$1"; }
warn() { printf '\033[33m! %s\033[0m\n' "$1"; }
rule() { printf '\033[2m%s\033[0m\n' "────────────────────────────────────────────────────────────"; }

REAL_DB="${AMB_DB:-$HOME/.agent-messageboard/board.db}"
VAULT="${AMB_VAULT:-}"

if [ ! -f "$REAL_DB" ]; then
  echo "no board at $REAL_DB — nothing to eyeball" >&2
  exit 0
fi

# The binary the hooks actually invoke, not the one PATH happens to resolve (D73).
if [ -n "${AMB_BIN:-}" ]; then
  BIN="$AMB_BIN"
else
  BIN="$(python3 - <<'PY'
import json, os, pathlib
p = pathlib.Path(os.path.expanduser("~/.claude/settings.json"))
seen = []
try:
    d = json.loads(p.read_text())
except Exception:
    d = {}
for entries in d.get("hooks", {}).values():
    for e in entries:
        for h in e.get("hooks", []):
            c = h.get("command", "").split()
            if c and c[0].endswith("/amb"):
                seen.append(c[0])
print(seen[0] if seen else "")
PY
)"
  [ -n "$BIN" ] || BIN="$(command -v amb || true)"
fi
if [ ! -x "$BIN" ]; then
  echo "no amb binary found — set AMB_BIN, or run ./tools/install.sh" >&2
  exit 69
fi

command -v sqlite3 >/dev/null 2>&1 || { echo "sqlite3 is required to copy a WAL board safely" >&2; exit 69; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
COPY="$WORK/board.db"

# `.backup` and not `cp`: the board is in WAL mode, see this script's header.
sqlite3 "$REAL_DB" ".backup '$COPY'" || { echo "could not copy the board" >&2; exit 69; }

# The safety claim, measured before and after rather than asserted.
#
# **Logical row counts, not a byte digest, and the first version of this check got that wrong.**
# `board.db`'s sha256 changed twice in three seconds with no `amb` command running at all
# (measured 2026-08-31), because another session merely *reading* a WAL database updates `-shm`
# and can trigger a checkpoint that rewrites the main file. The digest version printed "THE REAL
# BOARD CHANGED" on a run that had not written a single row. A digest of a live WAL database is
# not a modification signal.
snap_board() { sqlite3 -readonly "$1" \
  "SELECT (SELECT count(*) FROM messages)||' msg / '||(SELECT count(*) FROM reads)||' read / '||(SELECT count(*) FROM claims)||' claim / '||(SELECT count(*) FROM note_events)||' inject'" 2>/dev/null; }
snap_vault() { [ -n "$VAULT" ] && [ -d "$VAULT" ] && (cd "$VAULT" && find . -type f -name '*.md' | sort | tr '\n' ' ' | shasum -a 256 | cut -d' ' -f1); }

# Baselined from the COPY, not from the real board: the copy IS the board as of copy time, so a
# concurrent write landing between the two reads cannot show up as a difference this script made.
board_before="$(snap_board "$COPY")"
vault_before="$(snap_vault)"

export AMB_DB="$COPY"

run() { "$BIN" "$@" 2>&1; }
hook() { printf '%s' "$2" | AMB_DB="$COPY" "$BIN" hook "$1" 2>&1; }

INJECTED="$WORK/injected.txt"; : > "$INJECTED"
# Sections are tagged so the cross-check below can tell who *authored* a number. Without that it
# cannot separate "amb announced a stale schema" from "amb correctly showed you a two-day-old
# message in which someone else said one", and those have opposite verdicts.
show() {                       # show <title> <text>
  rule; bold "$1"
  if [ -z "$2" ]; then printf '\033[2m(nothing)\033[0m\n'; else printf '%s\n' "$2"; fi
  printf '\036%s\036\n%s\n' "$1" "$2" >> "$INJECTED"
}

bold "eyeballing the live board"
printf '  binary %s\n  board  %s\n  copy   %s\n  vault  %s\n' \
  "$BIN" "$REAL_DB" "$COPY" "${VAULT:-unset — memory is off (D35)}"
printf '  %s\n' "$(run --version)"

show "amb doctor" "$(run doctor)"

# The two hooks the installed settings actually invoke, driven with the payloads Claude Code sends.
show "SessionStart · delivery  (amb hook monitor)" \
     "$(hook monitor '{"hook_event_name":"SessionStart"}')"
show "SessionStart · memory  (amb hook memory)" \
     "$(hook memory '{"hook_event_name":"SessionStart"}')"
show "Stop · delivery  (amb hook monitor)" \
     "$(hook monitor '{"hook_event_name":"Stop"}')"
show "PreToolUse · memory  (amb hook memory)" \
     "$(hook memory '{"hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"src/messages.rs"}}')"

show "amb inbox   (the command the banner tells every agent to run first — D90)" "$(run inbox)"
show "amb claims" "$(run claims)"
[ -n "$VAULT" ] && show "amb memory status   (the receipt D59 is read off)" "$(run memory status)"

rule
bold "cross-artefact check"

board_schema="$(sqlite3 "$COPY" 'PRAGMA user_version' 2>/dev/null)"
printf '  board schema: %s\n' "${board_schema:-unknown}"
python3 - "$INJECTED" "${board_schema:-0}" <<'XCHK'
import re, sys, pathlib

text = pathlib.Path(sys.argv[1]).read_text()
board = sys.argv[2]

# Split on the record separator `show` writes, so every number keeps the surface it came from.
parts = text.split("\x1e")
sections = [(parts[k], parts[k + 1]) for k in range(1, len(parts) - 1, 2)]

def author(title):
    """Who wrote the words this number sits in — which is what decides the verdict."""
    t = title.lower()
    if "inbox" in t:
        return "inbox"          # deliberate: D96 gives the horizon to delivery, never to `inbox`
    if "delivery" in t:
        return "injected"       # M29's defect exactly: stale sender text reaching a session
    return "amb"                # doctor, memory status, claims — amb's own voice

found = {}
for title, body in sections:
    for m in re.finditer(r"\bschema (\d+)", body):
        found.setdefault((author(title), m.group(1)), set()).add(title.strip())

if not found:
    print("  no rendered surface names a schema number")
else:
    for (who, n), titles in sorted(found.items()):
        where = "; ".join(sorted(titles))
        if n == board:
            print(f"  ok    schema {n} agrees with the board  ({where})")
        elif who == "amb":
            print(f"\033[33m  !!    amb's OWN voice announces schema {n} to a board at {board} — M29's defect  ({where})\033[0m")
        elif who == "injected":
            print(f"\033[33m  !     a message announcing schema {n} is being INJECTED into sessions; board is at {board}  ({where})\033[0m")
            print("        This is the M29 condition D96's horizon exists to retire.")
        else:
            print(f"  note  schema {n} appears in `amb inbox`; board is at {board}  ({where})")
            print("        Not a defect: a message records what someone said at the time, and D96")
            print("        gives the horizon to the delivery path only. Read it, do not fix it.")
XCHK

rule
bold "did this script touch anything"
rc=0

# The structural guarantee, which is the one that actually holds: AMB_DB is exported once, above,
# so every child process inherits the copy. Nothing can reach the real board without an explicit
# inline override, and this line is what would catch someone adding one.
printf '  AMB_DB   -> %s\n' "$AMB_DB"
if [ "$AMB_DB" != "$COPY" ]; then warn "AMB_DB is not the copy — everything above ran against something else"; rc=1; fi

# The positive check. If the copy is untouched, the hooks did nothing and the output above is not
# the picture it claims to be — an empty result that looks identical to a quiet board (D89).
board_copy_after="$(snap_board "$COPY")"
if [ "$board_copy_after" != "$board_before" ]; then
  printf '  copy      %s  (the writes landed here)\n' "$board_copy_after"
else
  warn "the copy is unchanged — no hook wrote anything, so the run may not have exercised delivery"
fi

# Informational, and deliberately NOT a failure. See the note on WAL above: this board has other
# sessions on it by design, so a difference here is not attributable to this script.
board_after="$(snap_board "$REAL_DB")"
if [ "$board_after" = "$board_before" ]; then
  printf '  board     %s  (unchanged)\n' "$board_after"
else
  printf '  board     %s  ->  %s\n' "$board_before" "$board_after"
  printf '            \033[2mA concurrent session can account for this; the board is shared by design.\n'
  printf '            Attribution is what this cannot do, so it does not fail the run.\033[0m\n'
fi

vault_after="$(snap_vault)"
if [ -n "$vault_before" ]; then
  if [ "$vault_before" = "$vault_after" ]; then printf '  vault     unchanged\n'
  else printf '  vault     note count changed — a concurrent session writes notes too\n'; fi
fi
exit $rc

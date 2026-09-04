# Design

**Scope: the bus and claims.** Messages, broadcast addressing, the roster, and advisory file
claims — sized to the taxonomy in `DECISIONS.md` D2 as it stood, three kinds of traffic plus the
roster D12 requires to make broadcast and liveness meaningful. All of it is built.

> **This document does not cover the memory layer**, and its schema below is the bus half only.
> A fourth kind of traffic — the session observation — arrived with D34, and D49 then revised D2
> and D16 rather than leaving them as written. `MEMORY-DESIGN.md` owns that design and
> `DECISIONS.md` D34–D61 own its arguments; repeating either here would be the second copy this
> project spends a rule avoiding. The five tables it adds (`notes`, `note_paths`, `note_events`,
> `claim_notices`, `memory_counters`) arrive as migrations, not as edits to `schema.sql`.

---

## Storage

One SQLite file, **outside every repo** (D15). It holds ephemeral cross-project state and must
never be committed: a lease in git history is worse than worthless, because it reads as current.

`~/.agent-messageboard/board.db`, with a sibling `README.md` there saying what it is. On open,
refuse a path that resolves inside a sync root (iCloud `Mobile Documents`, Dropbox, Google Drive,
OneDrive) or a network mount — SQLite's locking primitives are not reliably honoured there.
The sync roots are matched by name as a fast path; the network case is decided by asking the
kernel what filesystem the path is on (D72).

**`amb` never writes inside a repository on its own initiative** (D11). No `.msgboard/`, no
rendered inbox, no drop directory, and `amb snapshot` refuses a path inside one. `amb memory
export` writes into a repo only because a person ran it (D49) — the rule is about initiative, not
about bytes.

### Pragmas, on every open

```sql
PRAGMA busy_timeout = 30000;      -- FIRST: writers queue rather than fail (D30)
PRAGMA journal_mode = WAL;        -- readers never block the writer
PRAGMA synchronous  = NORMAL;     -- durable enough; fsync per commit is not needed here
PRAGMA foreign_keys = ON;
```

**The order is load-bearing, and the timeout is not sufficient on its own.** Switching journal
mode needs a brief *exclusive* lock, and SQLite will not invoke the busy handler for it — so the
mode is **read before it is written** (once any process has converted the file, every later open
skips the write) and the conversion carries a bounded retry. Without all three, 10 of 12
concurrent first-opens failed (`MEASUREMENTS.md` M8).

Schema migrations run under `BEGIN IMMEDIATE` with `user_version` re-read inside the lock, for the
same reason: an upgrade reaches every live session at once.

Wrap each send in `BEGIN IMMEDIATE` so a writer takes the lock up front rather than discovering
the conflict at commit time. On this configuration, 17 concurrent OS processes sent 1,700 messages
with **zero `SQLITE_BUSY` and zero lost**, p50 0.09 ms and p99 ~35 ms (`MEASUREMENTS.md` M1, as
corrected by M16). Cite those, not M1's `msg/s`: that figure is a send-then-read *loop* rate and
halved when the harness was given the shipped inbox query, which is M16's finding.

---

## Schema

```sql
CREATE TABLE agents (
  id           TEXT PRIMARY KEY,   -- CLAUDE_CODE_SESSION_ID; the routing key (D12)
  name         TEXT NOT NULL,      -- display name; mutable, never routed on
  project      TEXT NOT NULL,
  cwd          TEXT,               -- the *repository* root (D20); claim paths are relative to it
  pid          INTEGER,            -- the *session's* pid, from CLAUDE_CODE_MESSAGING_SOCKET (D21);
                                   -- NULL when unknowable, and liveness falls back to last_seen.
                                   -- Only ever > 0: kill(0,·) and kill(-1,·) mean something else
  first_seen   REAL NOT NULL,
  last_seen    REAL NOT NULL       -- refreshed by every command; no heartbeat obligation
);
CREATE INDEX ix_agents_project ON agents(project, last_seen);

CREATE TABLE messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ext_id     TEXT UNIQUE,           -- caller-supplied stable ID; makes resend idempotent (D6)
  ts         REAL    NOT NULL,
  from_agent TEXT    NOT NULL,
  from_proj  TEXT    NOT NULL,
  to_agent   TEXT,                  -- NULL = not addressed to one agent
  to_proj    TEXT,                  -- NULL = every project (NULL/NULL is the global `@@`)
  kind       TEXT    NOT NULL,      -- a lowercase tag: note | question | proposal | ... (D107)
  subject    TEXT    NOT NULL,
  body       TEXT    NOT NULL,
  thread_id  TEXT
  -- `attempts` and `failed_at` used to live here and were dropped in schema version 2. A message
  -- is offered *per recipient*, so counting per message would silence a broadcast for everyone
  -- because one agent ignored it (D23). The counter is in `reads`.
);
CREATE INDEX ix_inbox ON messages(to_proj, to_agent, id);

CREATE TABLE reads (
  msg_id       INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  agent        TEXT    NOT NULL,
  delivered_at REAL,                -- we injected it; we can observe this
  read_at      REAL,                -- only `amb read` sets this; declared, never inferred
  attempts     INTEGER NOT NULL DEFAULT 0,  -- offers to *this* agent; past MAX_OFFERS the hook
                                            -- stops injecting, `amb inbox` still shows it (D23)
  PRIMARY KEY (msg_id, agent)
);

CREATE TABLE claims (
  path       TEXT NOT NULL,         -- a path prefix: "lib/src/" claims everything beneath
  agent      TEXT NOT NULL,
  project    TEXT NOT NULL,
  intent     TEXT,                  -- why, in a few words, so a peer can judge whether to wait
  source     TEXT NOT NULL,         -- 'declared' (amb claim) | 'observed' (PostToolUse hook)
  taken_at   REAL NOT NULL,
  expires_at REAL NOT NULL,         -- wall-clock, load-bearing (D13)
  PRIMARY KEY (path, agent)
);
CREATE INDEX ix_claims_live ON claims(project, expires_at);
```

**`PRIMARY KEY (path, agent)` is D5 written in DDL.** Two agents can both hold the same path; the
table is physically incapable of expressing exclusivity. It also makes renewal free — re-claiming
is `ON CONFLICT (path, agent) DO UPDATE SET expires_at = …` (D13).

### Why broadcast is a nullable column

`to_agent IS NULL` means *"everyone in `to_proj`"*. Because read state lives in its own table
rather than as a flag on the message, **one broadcast is consumed independently by each
recipient** without the sender needing to know who they are — which is what makes it work when
agents come and go mid-conversation.

The payoff is that direct, broadcast and cross-project addressing become one query rather than
three code paths:

```sql
SELECT m.* FROM messages m
WHERE m.to_proj = :me_proj
  AND (m.to_agent = :me OR m.to_agent IS NULL)
  AND m.failed_at IS NULL
  AND NOT EXISTS (SELECT 1 FROM reads r
                  WHERE r.msg_id = m.id AND r.agent = :me AND r.read_at IS NOT NULL)
ORDER BY m.id;
```

---

## Addressing

Matches the `repo#ID` convention the sibling repos already adopted for cross-repo citations, so
there is one addressing idea to learn rather than two.

| Form | Meaning |
|---|---|
| `alice` | agent `alice` in my project |
| `alice@nestwatch` | agent `alice` in project `nestwatch` |
| `@nestwatch` | broadcast to every agent in `nestwatch` |
| `@` | broadcast to every agent in my project |
| `@@` | broadcast to every agent in **every** registered project (D17) |

A name, a short ref (`c0a251`) or a full session id all resolve. Resolution happens **before**
anything is written, and an unknown name is an error rather than an undeliverable row (D18).

**A name is resolved, never routed on.** Routing uses the session UUID (D12); `alice` and the
short ref `[c0a251]` both resolve to it. This mirrors what `ListAgents` already displays.

**`@project` addresses a place, not a process** — which is the one thing no competitor does. A
message to `@nestwatch` is waiting for whoever works there, whenever they arrive.

**Bounded since D96: "whenever they arrive" means within 24 hours, for a broadcast, and only on the
delivery path.** The row is never deleted and `amb inbox` still returns it; what expires is
automatic injection. Direct mail does not expire at all. The bound exists because the per-recipient
back-off and the per-injection render cap do not bound their product, so the number of hook
injections grew with the backlog (M29).

---

## Identity and registration

> **Vendor-parameterised since D111 (2026-09-02).** The variable named below is Claude Code's;
> identity now reads whichever session id the host CLI exported, from `Vendor::session_env`.
> Gemini CLI sets `GEMINI_SESSION_ID`. Everything else in this section is unchanged — a session
> is still whoever exported an id, and the roster row is still auto-created.

Identity is `CLAUDE_CODE_SESSION_ID`, read from the environment on every invocation. **Verified:**
it is inherited by subshells and fresh `exec`s and equals the session's own transcript filename.

- `amb register --name alice` records the row explicitly. An explicit name that is already taken
  is an **error** (D18) — it must reach the agent that can still choose another.
- **Any other command auto-creates it** if missing, naming the agent from project plus short ref.
  Forgetting to register is not a failure mode: if that generated name is taken, the ref **widens**
  (6 → 8 → 12 → the whole id) rather than failing, so the cost is a less readable name and never
  a locked-out session (D32).
- `last_seen` is refreshed by every command. No heartbeat, no timer, no client obligation.

---

## Delivery

Hooks trigger the read; the agent never has to remember (D9). Installed once per machine with
`amb install --global`, removed with `amb uninstall`. Never silent — it edits
`~/.claude/settings.json`.

| Mode | Hook | Delivers |
|---|---|---|
| `session` | `SessionStart` | unread at session start |
| `turn` | `Stop` | new mail at each turn boundary |
| `monitor` | `SessionStart` → `amb watch` under the agent's Monitor tool | blocking read, seconds |
| *(all but `session`)* | `PostToolUse` | **mid-turn, per tool call** (D25) |
| *(all but `session`)* | `SessionEnd` | lapses the departing session's claims (D109) |

`turn` and `monitor` also install `PostToolUse` and `SessionEnd` hooks — the second lapses the
departing session's live claims instead of running out their TTL, best-effort, with the TTL kept
as the crash backstop (D109). `PostToolUse` does two jobs: it records the exact
file of every `Edit`/`Write`, which is what makes claims **observed** rather than declared (D14),
and — since its `additionalContext` *is* injected into the model's context, verified 2026-08-27 —
it delivers mid-turn (D25). Both halves are restricted to new facts so the hook does not repeat
itself after every tool call: mail that has never been offered, and a conflict only on an edit
that took a claim rather than renewing one.
| `off` | — | nothing; `amb inbox` only |

**`Stop`, not `UserPromptSubmit`** — the latter blocks the user's turn on a 30 s timeout, so a
hung `amb` would hang the human.

**Hooks must always exit 0** and carry a short timeout. Mail delivery must never break a session.

**`amb watch` blocks in-process.** SQLite has no cross-process change notification, so the block
is an internal poll loop — one process startup amortised across the whole wait, rather than one
per poll. That is what makes it cheap, and why it is not the notification subsystem D7 rejected.

### Read state is declared, not inferred

`delivered_at` is set when we inject a message — we can observe that. `read_at` is set **only** by
`amb read`. `reads.attempts` increments per injection *to that agent*; past `MAX_OFFERS` the hook
stops injecting it — but `amb inbox` still shows it, because backing off is not deletion and a log
you cannot re-read is not a log (D23).

---

## CLI surface

**`--json` on every command**, so an agent parses structured output rather than scraping text.

```
amb send    <to> --subject S (--body B | --body-file F|-) [--kind K] [--thread T] [--id EXT]
amb inbox   [--unread] [--json]
amb read    <msg-id>... | --all          # the only thing that marks a message read
amb reply   <msg-id> --body B
amb watch   [--timeout S] [--poll MS]    # blocking read, for monitor mode
amb agents  [--project P] [--live]       # the roster
amb register [--name N]                  # optional; every command auto-registers
amb claim   <path> [--intent "..."] [--ttl 4h]   # never blocks; reports conflicts
amb release <path>                       # only ever your own claim
amb claims  [--project P] [--live] [--raw]       # lapsed rows shown unless --live
amb install [--mode session|turn|monitor] [--memory] [--dry-run]   # the flag set is the
amb uninstall [--dry-run]                                           # complete desired state
amb doctor                               # what is silently wrong; always exits 0, read --json worst
amb snapshot <path> [--all]              # markdown snapshot; refuses paths inside a repo (D11)
amb hook    <mode>                       # internal; invoked by the installed hooks
```

Project is the **git working-tree root's** directory name, found by walking up for a `.git` that
may be a file as well as a directory (D20); `AMB_PROJECT` overrides. On failure under `--json`,
the error is emitted as JSON on stdout as well as prose on stderr.

### Expiry is a read-time filter, not a job

Never write a reaper process. `claims --live` filters `WHERE expires_at > now`, so an expired
claim disappears without anything having to run. Deleting rows can be a periodic `amb vacuum` or
simply never — at this volume the table stays trivially small.

---

## What deliberately is not here

- **No decisions, findings or notes on the bus** (D2). ~~Those live in the repos they govern.~~
  **Revised by D49**: the vault is authoritative and decisions live there, published into a repo
  one-way and only when a person asks. Nothing here changed — a decision still never travels as a
  message.
- **No `propose`/`promote` and no per-repo inbox file** (D16). ~~As written.~~ **Revised by D49**:
  promotion exists, behind a human gate, one candidate at a time, and never writes without
  `--yes`. The findings-inbox is still refused.
- **No enforcement of claims** (D5). Conflicts are reported; nothing blocks.
- **No outbox** (D10). `amb send` is the only write path.
- **No files written inside repositories** (D11).
- **No server, daemon or socket** (D3). One binary, one file.
- **No notification subsystem** (D7). Hooks are the harness's existing extension point, and what
  they run is a poll.

---

## Testing notes

Carried from the sibling repos' hard-won conventions — these are the traps that actually bit
there:

- **A test that iterates its own hardcoded list is probably tautological.** If a fixture mirrors
  a list that also exists in the code, ask what fails when the two drift.
- **After adding a guard, delete it and watch the test go red.** A test that passed on its first
  run has proven nothing yet.
- **Concurrency claims need concurrent processes, not threads.** The whole point of this design is
  N unrelated OS processes; a threaded test would exercise a case that does not occur.
- **Re-run `bench/bench_startup.py` once the real binary exists** and add it to the candidate
  list. The ~1.7 ms floor is `/bin/echo` standing in for "a small native binary" and is
  deliberately optimistic — a real binary linking `rusqlite` will be slower. Do not quote it as if
  it were measured for this binary.

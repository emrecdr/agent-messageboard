-- Schema for the agent messageboard. See docs/DESIGN.md.
--
-- **This is migration 0 -> 1, not the current schema.** It stopped running on every open when
-- versioning arrived; it now runs exactly once, on a board that does not exist yet, and the
-- ladder in src/db.rs MIGRATIONS carries it forward from there. Read that array to know what a
-- live board actually looks like -- two columns below are dropped by 1 -> 2 milliseconds after
-- this file creates them.
--
-- Every statement is still IF NOT EXISTS, which is what lets one baseline serve both a fresh
-- file and a pre-versioning board (src/db.rs).
--
-- Frozen by construction: editing a statement here silently re-points every board that already
-- ran it. A schema change goes in a new MIGRATIONS entry, never in this file.

CREATE TABLE IF NOT EXISTS agents (
  id           TEXT PRIMARY KEY,   -- CLAUDE_CODE_SESSION_ID; the routing key (D12)
  name         TEXT NOT NULL,      -- display name; resolved to an id, never routed on
  project      TEXT NOT NULL,
  -- Holds the *repository root*, not a working directory, since D20 -- it is what claim paths
  -- are relative to. The column name predates that and is kept only because renaming it would
  -- cost a migration for no behavioural gain; identity::Identity calls the field `root`.
  --
  -- It means "the root of the last session that registered under this project name", NOT "the
  -- project's root", and the two diverge. It can legitimately hold a non-repository: a session
  -- started outside a repo falls back to the directory basename for `project`, so this board
  -- carries a project named `T` rooted at a temp directory. Harmless while every project resolves
  -- to one distinct value -- which is exactly the condition D57 detects the failure of -- and
  -- load-bearing the moment anything resolves a *foreign* project's files through it. That was
  -- proposed once, for validating another project's declared note paths, and declined in D68
  -- partly on this.
  cwd          TEXT,
  pid          INTEGER,
  first_seen   REAL NOT NULL,
  last_seen    REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_agents_project ON agents(project, last_seen);
-- A name must resolve to exactly one agent or direct addressing is ambiguous. Enforced here so
-- the clash surfaces once, at registration, to the agent that can still rename itself.
CREATE UNIQUE INDEX IF NOT EXISTS ux_agents_name ON agents(project, name);

-- Addressing is a 2x2 over two nullable columns, which is why four modes are one query:
--
--   to_agent   to_proj    meaning
--   ---------  ---------  -----------------------------------------------
--   <uuid>     (info)     one agent, in any project
--   NULL       'nest'     everyone in that project
--   NULL       NULL       everyone, everywhere  (`@@`)
--
-- `to_agent` holds a resolved agent id, never a display name. Storing a name here is the bug
-- that made direct messages silently undeliverable before v0.1.
CREATE TABLE IF NOT EXISTS messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ext_id     TEXT UNIQUE,
  ts         REAL    NOT NULL,
  from_agent TEXT    NOT NULL,
  from_proj  TEXT    NOT NULL,
  to_agent   TEXT,                 -- NULL = not addressed to one agent
  to_proj    TEXT,                 -- NULL = every project
  kind       TEXT    NOT NULL,
  subject    TEXT    NOT NULL,
  body       TEXT    NOT NULL,
  thread_id  TEXT,
  -- DROPPED BY MIGRATION 1 -> 2. Do not write against these; they exist on no live board.
  -- Offer counting moved to reads.attempts, because delivery is per recipient and this counted
  -- per message: one broadcast advanced it once per reader, so the dead-letter threshold
  -- silenced it for everyone (D23). Kept here only because a baseline may not be rewritten.
  attempts   INTEGER NOT NULL DEFAULT 0,
  failed_at  REAL
);
-- These two serve a plan that is SELECTIVITY-DEPENDENT, and it is worth saying so here because
-- the obvious reading of an EXPLAIN is that they are dead.
--
-- An audit on 2026-09-05 reported `messages::select` as a permanent full scan with both indexes
-- unused by every production query, and recommended dropping them. That finding was WITHDRAWN
-- before anything was changed, and this comment is what was learned instead:
--
--   * On the live board (425 rows, 11 distinct recipients) one agent matches 109 of 425 rows —
--     25.6% — and SQLite chooses SCAN. At that selectivity a scan really is cheaper than two
--     index probes plus random row fetches, so the plan is correct rather than defective.
--   * On a synthetic board with the same 73/21/5 direct/project/global mix but 40 agents, the
--     match falls to ~9% and the same query plans as MULTI-INDEX OR using ix_inbox_agent. Size
--     is not the variable; how many agents share the board is.
--   * Forcing either plan at 60k rows lands within a few ms of the other, so no configuration was
--     found in which the choice measurably matters.
--
-- The practical consequences: do NOT add an `INDEXED BY` hint, which would pin the wrong plan for
-- half the boards that exist; do NOT assert a plan for this query the way `claims::list_sql` does,
-- because that query has one correct plan and this one has two; and do NOT conclude from a single
-- EXPLAIN that these are dead weight — the first measurement said exactly that and was wrong.
CREATE INDEX IF NOT EXISTS ix_inbox_proj  ON messages(to_proj, id);
CREATE INDEX IF NOT EXISTS ix_inbox_agent ON messages(to_agent, id);

CREATE TABLE IF NOT EXISTS reads (
  msg_id       INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  agent        TEXT    NOT NULL,
  delivered_at REAL,
  read_at      REAL,
  PRIMARY KEY (msg_id, agent)
);

-- PRIMARY KEY (path, agent), not (path): two agents can both hold a path. That is D5 in DDL.
CREATE TABLE IF NOT EXISTS claims (
  path       TEXT NOT NULL,
  agent      TEXT NOT NULL,
  project    TEXT NOT NULL,
  intent     TEXT,
  source     TEXT NOT NULL DEFAULT 'declared',
  taken_at   REAL NOT NULL,
  expires_at REAL NOT NULL,
  PRIMARY KEY (path, agent)
);
CREATE INDEX IF NOT EXISTS ix_claims_live ON claims(project, expires_at);

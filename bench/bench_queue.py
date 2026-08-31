"""Measure whether SQLite can carry an agent message bus at this project's real scale.

Two questions the report needs answered with numbers rather than citations:

  1. At 17 concurrent writers -- the number of Claude sessions actually live on this
     machine right now -- does the single-writer lock become the bottleneck people warn
     about, or is the warning about a workload six orders of magnitude busier?
  2. What does the NAIVE configuration do? "SQLite is fine" and "SQLite is fine if you
     set two pragmas" are different claims, and the difference is where people get hurt.

Runs three scenarios against the same schema. Reports p50/p99 latency and, crucially,
the count of SQLITE_BUSY failures -- the error that turns into a lost message.

**The schema below is src/schema.sql as the migration ladder leaves it, not an approximation
of it, and that is a repair.** It used to declare `to_proj TEXT NOT NULL` with a single
`ix_inbox(to_proj, to_agent, id)`, which makes the global broadcast (`to_agent IS NULL AND
to_proj IS NULL`) unrepresentable -- one of the four addressing modes D17 calls this design's
central claim could not be inserted at all, so it was never in anything measured here. The
reader was `SELECT m.id ... LIMIT 50` against a shipped query that joins `agents`, selects every
column including the body, and has no LIMIT.

**Which makes the headline msg/s a loop figure, not a write-capacity figure.** It is
sends / wall-clock over a send-then-read loop, so it moves with whatever the reader costs. The
numbers that answer the motivating question -- does the single writer lock become the bottleneck
-- are the send-latency percentiles and the SQLITE_BUSY count, and those are measured on the
write path alone. Cite those; treat throughput as the shape of the loop.
"""

import multiprocessing as mp
import os
import sqlite3
import statistics
import sys
import time

SCHEMA = """
-- src/schema.sql after the migration ladder: `agents` (the inbox query LEFT JOINs it), the
-- 2x2 over two NULLABLE columns, per-agent read tracking, advisory claims.
CREATE TABLE IF NOT EXISTS agents (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  project    TEXT NOT NULL,
  cwd        TEXT,
  pid        INTEGER,
  first_seen REAL NOT NULL,
  last_seen  REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_agents_project ON agents(project, last_seen);
CREATE UNIQUE INDEX IF NOT EXISTS ux_agents_name ON agents(project, name);

CREATE TABLE IF NOT EXISTS messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ext_id     TEXT UNIQUE,        -- NULL on the default send path; UNIQUE treats NULLs as distinct
  ts         REAL NOT NULL,
  from_agent TEXT NOT NULL,
  from_proj  TEXT NOT NULL,
  to_agent   TEXT,               -- NULL = not addressed to one agent
  to_proj    TEXT,               -- NULL = every project (`@@`)
  kind       TEXT NOT NULL,
  subject    TEXT NOT NULL,
  body       TEXT NOT NULL,
  thread_id  TEXT
);
CREATE INDEX IF NOT EXISTS ix_inbox_proj  ON messages(to_proj, id);
CREATE INDEX IF NOT EXISTS ix_inbox_agent ON messages(to_agent, id);

CREATE TABLE IF NOT EXISTS reads (
  msg_id       INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  agent        TEXT NOT NULL,
  delivered_at REAL,
  read_at      REAL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (msg_id, agent)
);

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
"""

BODY = "A realistic message body. " * 12  # ~300 bytes, typical of a real agent note


def connect(path, tuned):
    con = sqlite3.connect(path, timeout=30 if tuned else 5, isolation_level=None)
    # On BOTH configurations, so scenario [2] isolates exactly one variable -- WAL versus the
    # rollback journal -- as MEASUREMENTS.md M1 says it does. The shipped board sets this on
    # every open; setting it only under `tuned` would quietly add a second difference.
    con.execute("PRAGMA foreign_keys=ON")
    if tuned:
        con.execute("PRAGMA journal_mode=WAL")
        con.execute("PRAGMA busy_timeout=30000")
        con.execute("PRAGMA synchronous=NORMAL")
    return con


# `messages::select` verbatim on the read path that matters -- unread, no back-off, which is
# what `amb inbox` runs. The LEFT JOIN, the every-column projection and the absent LIMIT are all
# load-bearing: the previous `SELECT m.id ... LIMIT 50` made the reader a constant while the real
# one grows with the unread set, and the throughput figure is sends over the whole loop.
INBOX_SQL = """
SELECT m.id, m.ts, m.from_agent, a.name, m.to_agent, m.to_proj, m.kind, m.subject,
       m.body, m.thread_id
FROM messages m
LEFT JOIN agents a ON a.id = m.from_agent
WHERE m.from_agent <> ?
  AND (m.to_agent = ? OR (m.to_agent IS NULL AND (m.to_proj IS NULL OR m.to_proj = ?)))
  AND NOT EXISTS (SELECT 1 FROM reads r
                  WHERE r.msg_id = m.id AND r.agent = ? AND r.read_at IS NOT NULL)
ORDER BY m.id
"""


def addressed(i, agent_id):
    """(to_agent, to_proj) for message `i` -- all three send modes the 2x2 allows.

    Every 20th is global (`@@`), every other 5th is a project broadcast, the rest are direct.
    The global mode could not be expressed at all before the schema was repaired.
    """
    if i % 20 == 19:
        return None, None                                  # @@  -- everyone, everywhere
    if i % 5 == 4:
        return None, "nestwatch"                           # @   -- everyone here
    return f"agent-{(agent_id + 1) % 17:02d}", "nestwatch"  # direct


def agent(path, tuned, agent_id, n_msgs, think_s, out_q):
    """One agent: send a message, then read its inbox, n_msgs times."""
    con = connect(path, tuned)
    send_lat, busy, other_err, rows_read = [], 0, 0, 0
    me = f"agent-{agent_id:02d}"
    for i in range(n_msgs):
        if think_s:
            time.sleep(think_s)
        to_agent, to_proj = addressed(i, agent_id)
        t0 = time.perf_counter()
        try:
            con.execute("BEGIN IMMEDIATE")
            con.execute(
                "INSERT INTO messages(ts,from_agent,from_proj,to_agent,to_proj,kind,"
                "subject,body,thread_id) VALUES(?,?,?,?,?,?,?,?,?)",
                (time.time(), me, "nestwatch", to_agent, to_proj, "note",
                 f"msg {i} from {me}", BODY, f"t{agent_id}-{i}"),
            )
            con.execute("COMMIT")
            send_lat.append((time.perf_counter() - t0) * 1000)
        except sqlite3.OperationalError as e:
            try:
                con.execute("ROLLBACK")
            except sqlite3.Error:
                pass
            if "locked" in str(e) or "busy" in str(e):
                busy += 1
            else:
                other_err += 1
        try:
            rows_read += len(con.execute(INBOX_SQL, (me, me, "nestwatch", me)).fetchall())
        except sqlite3.OperationalError:
            other_err += 1
    out_q.put((send_lat, busy, other_err, rows_read))


def scenario(label, tuned, n_agents, n_msgs, think_s, note):
    path = f"/tmp/bench_q_{os.getpid()}_{abs(hash(label))}.db"
    for suffix in ("", "-wal", "-shm"):
        try:
            os.unlink(path + suffix)
        except OSError:
            pass
    con = connect(path, tuned)
    con.executescript(SCHEMA)
    # The roster the inbox query LEFT JOINs. Absent, the join still succeeds and every sender
    # name comes back NULL -- a read that costs less than the real one and looks identical.
    con.executemany(
        "INSERT INTO agents(id,name,project,first_seen,last_seen) VALUES(?,?,?,?,?)",
        [(f"agent-{i:02d}", f"agent-{i:02d}", "nestwatch", time.time(), time.time())
         for i in range(n_agents)],
    )
    con.close()

    q = mp.Queue()
    procs = [
        mp.Process(target=agent, args=(path, tuned, i, n_msgs, think_s, q))
        for i in range(n_agents)
    ]
    t0 = time.perf_counter()
    for p in procs:
        p.start()
    results = [q.get() for _ in procs]
    for p in procs:
        p.join()
    wall = time.perf_counter() - t0

    lat = [x for r in results for x in r[0]]
    busy = sum(r[1] for r in results)
    errs = sum(r[2] for r in results)
    rows_read = sum(r[3] for r in results)
    sent = len(lat)
    attempted = n_agents * n_msgs

    # What the workload actually contained, asked of the database rather than of `addressed`.
    con = connect(path, tuned)
    modes = {
        "direct": con.execute(
            "SELECT count(*) FROM messages WHERE to_agent IS NOT NULL").fetchone()[0],
        "project": con.execute(
            "SELECT count(*) FROM messages WHERE to_agent IS NULL AND to_proj IS NOT NULL"
        ).fetchone()[0],
        "global": con.execute(
            "SELECT count(*) FROM messages WHERE to_agent IS NULL AND to_proj IS NULL"
        ).fetchone()[0],
    }
    con.close()

    print(f"\n{label}")
    print(f"  {note}")
    print(f"  {n_agents} agents x {n_msgs} messages = {attempted} attempted")
    print(f"  delivered ...... {sent}/{attempted}"
          f"   LOST: {attempted - sent}")
    print(f"  SQLITE_BUSY .... {busy}        other errors: {errs}")
    if lat:
        lat.sort()
        p50 = statistics.median(lat)
        p99 = lat[min(len(lat) - 1, int(len(lat) * 0.99))]
        print(f"  send latency ... p50 {p50:6.2f} ms   p99 {p99:7.2f} ms   max {lat[-1]:7.2f} ms")
    print(f"  addressing ..... direct {modes['direct']}   @project {modes['project']}   "
          f"@@global {modes['global']}")
    print(f"  inbox rows ..... {rows_read:,} returned across {sent} reads")
    print(f"  wall clock ..... {wall:.2f} s   ({sent / wall:,.0f} msg/s loop throughput)")
    print("                   ^ sends over the whole send-then-read loop, NOT write capacity;"
          "\n                     the write-path answer is the send latency and busy count above.")

    size = os.path.getsize(path) if os.path.exists(path) else 0
    print(f"  db size ........ {size / 1024:.0f} KB")
    for suffix in ("", "-wal", "-shm"):
        try:
            os.unlink(path + suffix)
        except OSError:
            pass
    return {"sent": sent, "attempted": attempted, "busy": busy,
            "modes": modes, "rows_read": rows_read}


if __name__ == "__main__":
    mp.set_start_method("spawn")
    print("=" * 72)
    print("SQLite as an agent message bus - measured on this machine")
    print(f"python {sys.version.split()[0]}  sqlite {sqlite3.sqlite_version}  "
          f"cpus {os.cpu_count()}")
    print("=" * 72)

    # 1. Saturation: what is the ceiling with the pragmas set?
    tuned = scenario("[1] SATURATION, tuned (WAL + busy_timeout)", True, 17, 100, 0,
                     "Every agent writing flat out. Finds the ceiling, not the real workload.")

    # 2. The same saturation WITHOUT the pragmas - the naive config.
    scenario("[2] SATURATION, naive (rollback journal, 5s timeout)", False, 17, 100, 0,
             "Same load, default settings. This is what 'just use SQLite' gets you.")

    # 3. Realistic: an agent sends a message every ~2 seconds.
    scenario("[3] REALISTIC, tuned (a message every 2s per agent)", True, 17, 5, 2.0,
             "17 live sessions at a human-plausible rate. This is the actual workload.")

    # ── Coverage, not a threshold ────────────────────────────────────────────────────────────
    #
    # This asserts what was *measured*, never what it measured at, for the reason stated at the
    # top of tools/bench.sh. The failure it exists to catch already happened once: with
    # `to_proj TEXT NOT NULL` the global broadcast was unrepresentable, so `@@global` was 0 on
    # every run this document ever quoted -- and nothing said so, because the script printed
    # three healthy rows and exited 0.
    gaps = []
    for mode, n in tuned["modes"].items():
        if n == 0:
            gaps.append(f"no {mode} messages were sent -- that addressing mode went unmeasured")
    if tuned["rows_read"] == 0:
        gaps.append("the inbox query returned nothing -- the read half of the loop is untested")
    if gaps:
        print("\n\033[31m! this run did not measure what MEASUREMENTS.md M1 says it does:\033[0m")
        for g in gaps:
            print(f"    {g}")
        sys.exit(1)
    print("\n\033[32m\u2713 all four addressing modes exercised; the inbox query returned "
          f"{tuned['rows_read']:,} rows\033[0m")

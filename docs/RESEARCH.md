# Research

Prior art and patterns surveyed 2026-08-27, before committing to a design. Recorded so the "why
not just use X" questions have answers on file.

---

## R1 · MCP Agent Mail

<https://github.com/dicklesworthstone/mcp_agent_mail>

Purpose-built for this exact problem: many coding agents (Claude Code, Codex, Gemini CLI) on one
codebase, needing to know who is editing what.

**What it has.** Registered agent identities with generated names; threaded inboxes with a
canonical message copy plus sender-outbox and recipient-inbox copies; **advisory file
reservations** with TTL, exclusive-versus-shared semantics and conflict reporting; full-text
search. Storage is dual — SQLite (with FTS5) for live state and queries, Git for a
human-auditable archive of messages, profiles and reservation artifacts. Exposed as an HTTP
FastMCP server on port 8765, so Claude Code calls MCP tools directly with no per-agent setup.

**Costs.** A server process to run and supervise. Python 3.14+ managed by `uv`. The installer
pulls in extras beyond the ask. Reservations are advisory and conflicts do not block the grant.

**Sharpest limitation for us:** **no documented renewal mechanism** for reservations — they
expire on TTL. A session working longer than its lease silently loses its claim while still
editing. Given the sessions that motivated this project had run for hours, that matters, and it
is the concrete example behind `DECISIONS.md` D5's renewal corollary.

**Cross-repo support** exists in two shapes: both repos under one `project_key` with distinct
path patterns, or separate `project_key`s with a `request_contact`/`respond_contact` handshake.

---

## R2 · AMQ — agent-message-queue

<https://github.com/avivsinai/agent-message-queue>

A single binary, no server, no listeners.

**What it has.** **Maildir delivery** — write to `tmp/`, fsync, atomically link into `new/`,
reader moves to `cur/` — so a message is never partially written even if the process dies
mid-write, and a same-name collision preserves both copies rather than overwriting. Messages are
**JSON frontmatter plus a Markdown body**, deliberately so you can `cat`, `grep` and version
them. Priorities, message kinds, threading, delivery receipts, and a dead-letter queue.

**Addressing already covers all three of our cases:** `--to codex` within a project,
`--to codex@infra-lib:collab` across projects, `--to codex --session feature-b` across sessions.
Cross-project peers are configured explicitly:

```json
{ "root": ".agent-mail", "project": "app",
  "peers": { "infra-lib": "/Users/me/src/infra-lib/.agent-mail" } }
```

**Costs.** A CLI rather than an MCP server, so agents shell out. **No file-reservation
primitive** — the claims row is not covered.

**Worth stealing regardless of what we build:** the write-then-atomically-move discipline, and
the decision to make messages human-inspectable with ordinary Unix tools.

---

## R3 · Git worktrees

The standard advice for parallel agents, and genuinely good at what it does: filesystem
isolation, so two agents cannot overwrite each other's bytes.

**Why it is not sufficient.** The consistent finding across practitioner write-ups is that
worktrees **isolate without coordinating**. Agents on different features still collide on shared
files — route definitions, config, barrel exports, type definitions, schemas — and the conflict
simply moves from the working tree to the merge. It solves overwrites, not awareness. Awareness
is what this project buys.

---

## R4 · Protocol landscape

For context on whether a standard should be adopted rather than a local design.

**MCP** addresses an agent's interaction with its environment (tools and data). **A2A** addresses
communication *between* agents — Agent Cards for capability discovery, a client-server model for
task delegation. A2A reached v1.0 in April 2026, was donated by Google to the Linux Foundation,
and both protocols now sit under the **Agentic AI Foundation** (launched December 2025; OpenAI,
Anthropic, Google, Microsoft, AWS, Block).

**Relevance here: low, for now.** A2A is built for autonomous agents across organisational and
network boundaries, with authentication and capability negotiation this case does not have — every
participant is the same user's session on one machine. Worth revisiting only if agents ever need
to coordinate across machines or trust domains.

---

## R5 · Messaging patterns worth copying

- **Competing consumers** — many consumers on one queue, each message delivered to one of them.
  Relevant if a "any agent can pick this up" work-queue is ever wanted; not needed for addressed
  messages.
- **At-least-once plus idempotent consumers** — the mature choice. Do not chase exactly-once at
  the broker; make a duplicate harmless instead. Drives the `ext_id` column in `DESIGN.md`.
- **Dead-letter channel** — a message that fails processing repeatedly will block a queue if
  nothing moves it aside. Both R1 and R2 ship one; it is the piece most often skipped when
  building and the most painful to retrofit. Drives `attempts` / `failed_at`.

---

## R6 · Leases, and why we stop short of safety

**Lease pattern:** acquire, work, renew while alive, expire if not renewed. Best practice is to
renew on a short interval against a longer lease so one or two missed renewals are survivable,
and to compute intervals from **monotonic** time rather than wall-clock.

**The safety gap.** Kleppmann's 2016 analysis of Redlock established that lease timeouts alone
are unsafe: a holder that pauses — GC, suspend, a closed laptop lid — can wake after its lease
expired and still write. The fix is a **fencing token**: a monotonically increasing number issued
with the lease, included in every write, and **checked by the storage layer**, which rejects
stale ones. Chubby got this right with Paxos-backed sequencers.

**Why we deliberately stop short.** Fencing requires the protected resource to validate tokens.
Ours is a git working tree, which cannot. And the failure consequence is a merge conflict a human
resolves, not silent data loss. So awareness is the achievable and sufficient goal — see
`DECISIONS.md` D5.

---

## R7 · SQLite and DuckDB concurrency

**SQLite.** WAL mode does not give concurrent writes — writes are still serialised, exactly one
transaction holding the write lock. What WAL changes is that **writes no longer block reads and
reads no longer block writes**. A second writer gets `SQLITE_BUSY`; the standard fix is a
generous `busy_timeout`, since a write transaction takes milliseconds and writers simply take
turns. Published benchmarks show throughput degrading as writers pile up — "more than halved at
16 writers" — which is why this was measured here rather than trusted (`MEASUREMENTS.md` M1: the
warning describes a workload ~1,000× busier than ours).

**DuckDB.** Locks the database file for writes and permits **one writer *process*** — many writer
threads, but only within that single process. Multi-process writing requires either **Quack**,
its remote client-server protocol (beta at v1.5.2, maturity targeted for v2.0 in autumn 2026), or
**DuckLake** coordinating through a central PostgreSQL catalog. Both reintroduce a server. It is
also a columnar OLAP engine, and this is a tiny-row OLTP workload. Rejected in `DECISIONS.md` D4.

---

## Sources

- <https://github.com/dicklesworthstone/mcp_agent_mail> — MCP Agent Mail
- <https://github.com/avivsinai/agent-message-queue> — AMQ
- <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — fencing tokens
- <https://duckdb.org/docs/current/connect/concurrency> — DuckDB concurrency model
- <https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors/> — SQLite `database is locked`
- <https://blog.skypilot.co/abusing-sqlite-to-handle-concurrency/> — SQLite under concurrency in practice
- <https://learn.microsoft.com/en-us/azure/architecture/patterns/competing-consumers> — competing consumers
- <https://www.enterpriseintegrationpatterns.com/patterns/messaging/DeadLetterChannel.html> — dead-letter channel
- <https://www.augmentcode.com/guides/git-worktrees-parallel-ai-agent-execution> — worktrees for parallel agents
- <https://www.baeldung.com/linux/ipc-performance-comparison> — local IPC performance comparison

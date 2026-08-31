# Brief

What was asked for, in the order it was asked, and what it settled into. Recorded because the
requirements moved several times and the *reasons* they moved are the useful part.

Captured 2026-08-27 from the originating session.

---

## Origin — the incident that started it

Worth reading first, because it scopes the project honestly.

Three Claude sessions were working concurrently: two in `nestwatch` (a Rust project) and one in
`nestwatch-mobile` (a Flutter client). The mobile session was asked by its user to record a
finding about nestwatch, and wrote it into **nestwatch's** `docs/OPEN-FINDINGS.md` as entry
`O72`.

Both nestwatch sessions found the file dirty. Both independently declined to commit it, on the
same reasoning: *you cannot vouch for prose you did not write, and committing it puts another
session's argument under your name in `git blame`.* The entry was correct and useful. It sat
uncommitted anyway.

**The diagnosis matters more than the incident.** Message delivery was never the problem — the
sessions talked to each other throughout using the existing `SendMessage`, corrected each other,
and caught two real errors in the process. What failed is that **a proposal had no legal state
in a register**. `OPEN-FINDINGS.md`'s own convention is that an entry stands until it is fixed,
which leaves an unvalidated foreign claim nowhere to sit.

**Consequence for this project:** a message bus would not have prevented this. The fix is a
second file per repo (`docs/FINDINGS-INBOX.md`) holding proposals until a local session
validates and promotes them. That work is independent of everything here and should ship first.
Do not let this project's scope absorb it.

---

## Goals

In the order they arrived.

### G1 · Cross-project communication

Two repositories that reference each other need a channel. `nestwatch` and `nestwatch-mobile`
had already started citing each other's findings in prose, which rots — the mobile session
independently built `repo#ID` addressing and a `tool/check_findings.sh` that resolves references
in both directions to fix that.

### G2 · Intra-project multi-agent coordination

> *"we can also have multiple agent instances running within the same project, they also need to
> communicate with each other"*

Several sessions in one repo need to know who is doing what. Today this is `SendMessage` plus
`ListAgents`, which works while both sessions are alive and records nothing.

### G3 · Broadcast and direct addressing

> *"an agent can broadcast to all agents working in a project or can send message to a specific
> agent working in another project"*

Three addressing cases: one-to-one within a project, one-to-many within a project, and
one-to-one across projects.

### G4 · Notes and decisions

> *"we can also use this to keep notes, global architectural decisions, or project wise
> decisions"*

**Partially declined, with reasoning** — see `DECISIONS.md` D2. Decisions belong in the repo they
govern, not in the queue. The bus announces that a decision was recorded; it does not hold it.

### G5 · Validation before promotion

> *"only local LLM is allowed to write to /OPEN-FINDINGS.md after validating issue task locally"*

A foreign session may propose. Only a session working in that repo may promote a proposal into
the canonical register, and only after validating it against that tree. This is a repo-file
convention, not a bus feature, but the bus must not make it easy to bypass.

### G6 · Lightweight, then performant

The requirement moved, and both readings are now satisfied by the same answer:

> *"I want this as lightweight as possible. maybe we can use python & sqlite"*
> *"or maybe messages can be kept in memory, total decision depends on research"*
> *"yes I meant a high performant message queue for agents"*
> *"we can even use rust in order to be performant"*

Resolved by measurement rather than preference. In-memory turned out to be the *heavier* option
for this process topology, and "performant" turned out to bind on process startup rather than
throughput — which is what makes Rust the right call. See `MEASUREMENTS.md`.

---

## Non-goals

Each of these was considered and deliberately excluded. Reasons in `DECISIONS.md`.

- **Holding architectural decisions or findings.** They go in the repo they govern (D2).
- **Enforcing file locks.** Claims are advisory; fencing tokens are not worth it here (D5).
- **Exactly-once delivery.** At-least-once with idempotent handling instead (D6).
- **A push/notification subsystem.** Polling is affordable at this rate and message volume (D7).
- **A daemon or server process.** The whole point of the storage choice is not needing one (D3).

---

## Constraints discovered while researching

Each was checked on the target machine on 2026-08-27, not assumed.

- **Sessions are unrelated OS processes** with no common parent, which rules out
  `multiprocessing.Queue` and every other parent-hands-down-a-handle mechanism.
- **No `/dev/shm` on Darwin.** The usual "in memory but still a file" trick is unavailable.
- **17 concurrent sessions were live** during the research, so concurrency figures are real
  rather than hypothetical.
- **The repos are asymmetric:** `nestwatch` has a GitHub remote, `nestwatch-mobile` has none. So
  nothing PR-shaped can be the coordination mechanism.
- **Rust 1.96.0 toolchain is present and pinned** (nestwatch's `rust-toolchain.toml`), so Rust
  adds no new toolchain.

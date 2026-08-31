# Handover & kickoff

> **Status: historical. This handover was executed — the project is built.**
>
> The prompt below was written when no code existed. It calls this a "designed-but-unbuilt
> project" and points at "D1-D8"; both were true on 2026-08-27 and neither is now. The system
> ships with 100 passing tests, and `DECISIONS.md` runs to D19. Kept as the record of what was
> asked for and on what terms — **not** as a starting point for new work. For that, read
> `CLAUDE.md`, then `docs/DECISIONS.md`.

Paste the block below into a fresh Claude Code session started in
`/Users/emrec/Projects/playground/agent-messageboard`.

Everything above the rule is context for you; everything below it is the prompt.

---

You are picking up **agent-messageboard**, a designed-but-unbuilt project. A previous session did
the research, ran the benchmarks and recorded the decisions. Your job is to build it — not to
redesign it.

## Read first, in this order

1. `README.md` — one-paragraph orientation
2. `docs/DECISIONS.md` — **D1–D8. These are settled. Do not re-litigate them.** Each records what
   was rejected and why, specifically so that argument doesn't get had again
3. `docs/DESIGN.md` — the schema, addressing model and CLI surface you are implementing
4. `docs/MEASUREMENTS.md` — the numbers the decisions rest on
5. `docs/OPEN-QUESTIONS.md` — **Q1–Q6 are genuinely undecided.** Two of them block code; see below

`docs/BRIEF.md` and `docs/RESEARCH.md` are reference — read them when a "why not X?" occurs to
you, because X has probably already been answered there.

## What this is

A message bus for concurrent Claude Code sessions on one machine: direct messages, project-wide
broadcasts, and advisory file claims, working across more than one repository. A **Rust CLI over
SQLite** — one static binary, one database file, **no daemon**.

The performance argument is not throughput. SQLite already sustains ~1,000× the real message rate.
It is **process startup**, paid on every invocation because agents shell out per operation, where
a native binary is ~12× cheaper than Python. That is why this is Rust.

## Settle these two before writing code

Both change what you build. Ask the user; do not pick silently.

- **Q6 — does this project own the findings-inbox convention, or just announce it?** Owning it
  means an `amb propose` / `amb promote` pair and makes the protocol enforceable. Not owning it
  keeps this to three tables. This is the bigger of the two.
- **Q4 — lease TTL and renewal.** Sessions on this machine have run for *hours*, which breaks a
  naive TTL. Three options are laid out in `OPEN-QUESTIONS.md`; none is obviously right.

Q1 (stable agent identity) will bite during implementation but can be deferred behind a UUID.

## Build order

1. **Cargo scaffold** — its own project, not a nestwatch crate. Pin a toolchain in
   `rust-toolchain.toml`. Dependencies: `rusqlite` (bundled feature), `clap`, `serde_json`.
2. **Schema + open path** — the three tables from `DESIGN.md`, with the four pragmas applied on
   every open and `BEGIN IMMEDIATE` around each send.
3. **`send` and `inbox` first.** Broadcast falls out of `to_agent IS NULL` plus the separate
   `reads` table — get that pair right and direct, broadcast and cross-project addressing are one
   query rather than three code paths.
4. **`read`, `reply`.** Then stop and use it.
5. **Claims last** (`claim`, `release`, `claims`) — and only after the trial below.

Every command takes `--json`, so an agent parses structured output instead of scraping text.

## Verification standards — these are not optional here

This codebase's sibling projects have been repeatedly bitten by green gates hiding real defects.

- **Confirm by running, not by reasoning.** After adding a guard, delete it and watch the test go
  red. A test that passed on its first run has proven nothing yet.
- **Repeat any measurement before quoting it.** The research pass produced *two* wrong sub-claims
  sitting inside correct conclusions — a single noisy run showed `import sqlite3` as free when it
  costs ~2.5 ms. A wrong number attached to a right answer has no tell.
- **Concurrency tests need concurrent *processes*, not threads.** The entire design premise is N
  unrelated OS processes. A threaded test exercises a case that does not occur.
- **A test that iterates its own hardcoded list is probably tautological.** If a fixture mirrors a
  list that also exists in the code, ask what fails when the two drift.
- **Re-run `bench/bench_startup.py` once the binary exists** and add it to the candidate list.
  The current ~1.5 ms is `/bin/echo` standing in for "a small native binary" and is deliberately
  optimistic — a real binary linking `rusqlite` will be slower. Do not quote 1.5 ms as if it were
  measured for this binary.

## Environment gotchas on this machine

- **All Rust projects share one cargo target directory** (`~/.cache/cargo-target`, set in
  `~/.cargo/config.toml`). Two consequences: `cargo clean` is global and will nuke other projects'
  builds, and a *second concurrent cargo run* can produce phantom compile errors in crates you
  never touched. If you get an inexplicable error, check whether another session is building
  before you debug it.
- **`rustup` has no global default.** `rustc --version` fails outside a directory with a
  `rust-toolchain.toml`. That is not a missing install — nestwatch pins 1.96.0 and builds fine.
  Give this project its own toolchain file.
- **Other Claude sessions work these repos concurrently.** Use `ListAgents` and `SendMessage` to
  claim scope before editing anything outside this folder. A peer's "go ahead" is *not* the user's
  authorisation.
- **Git writes need asking.** Read-only git is fine any time. `commit`, `add`, `branch`, `push`
  and friends only when the user asks in that turn. This folder is **not yet a git repo** — do not
  `git init` unprompted.
- **No `Co-Authored-By` trailer** in commit messages.

## Scope discipline

Two things are explicitly *not* this project, and both will be tempting:

- **Holding decisions, ADRs or findings.** They live in the repo they govern. The bus may announce
  that one was recorded; it never stores it. (D2 — this is what keeps the project a weekend
  rather than a documentation system.)
- **Enforcing claims.** They are advisory by decision, not by omission. Do not add fencing tokens;
  `DECISIONS.md` D5 explains why they cannot work against a git working tree.

## One piece of honest context

The incident that motivated this project would **not** have been prevented by a message bus.
Delivery was never the problem — `SendMessage` worked throughout. What failed was that a
*proposal* had no legal state in a *register*. That fix is a markdown file in each repo and is
independent of everything you are about to build. Keep the bus scoped to what a bus is for.

Start by reading the docs, then tell me your answers to Q6 and Q4 before you write any code.

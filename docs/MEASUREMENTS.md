# Measurements

> **Commit SHAs quoted below predate 2026-08-31 and no longer resolve.** The repository's
> history was reset to publish it, so hashes such as `21f5e3f` or `c4ffb01` are labels for a
> commit that existed when the record was written, not addresses you can look up. They are kept
> rather than stripped because the sentence around each one is still true and the hash is still
> the identifier the author used. The pre-reset history is archived outside the repository.

Every number the decisions rest on, with the harness to reproduce it. **All figures measured on
2026-08-27** on the target machine — python 3.14.7, sqlite 3.53.4, Darwin arm64, 12 cores — not
cited from benchmarks run elsewhere.

Two separate questions, and the second one is the one that mattered.

---

## M1 · Does SQLite's single writer become the bottleneck?

> **Corrected by M16 (2026-08-29). Read that before quoting anything below.** The harness built a
> schema that could not express one of the four addressing modes, and read the inbox with a query
> materially cheaper than the shipped one. **The answer to the question stands; the throughput
> number does not.** Tuned saturation reproduces at **4,114 msg/s**, not 8,304, and that figure is
> a send-then-read *loop* rate rather than write capacity. The numbers that actually answer this
> section's question — send latency and the `SQLITE_BUSY` count — are unaffected in kind and still
> report zero busy and zero lost.

**Motivation.** The literature warns that SQLite serialises writes and that throughput collapses
as writers pile up — one widely-cited benchmark has it *"more than halved at 16 writers."* There
were **17 live Claude sessions** on this machine during the research, so that warning landed
squarely on the design and was worth testing rather than trusting.

**Method.** `bench/bench_queue.py`. Builds the real proposed schema (messages with direct and
broadcast addressing, per-agent read tracking, claims), then runs 17 concurrent OS processes,
each sending messages and polling its inbox. Records send latency, `SQLITE_BUSY` count, and
messages lost.

### Results

| Scenario | Delivered | Busy | p50 | p99 | Max | Throughput |
|---|---|---|---|---|---|---|
| Saturation, tuned (WAL + `busy_timeout`) | 1700/1700 | 0 | 0.02 ms | 9.59 ms | 96 ms | **8,304/s** |
| Saturation, untuned (rollback journal) | 1700/1700 | 0 | 0.25 ms | 41.5 ms | 879 ms | 1,682/s |
| Realistic (1 msg / 2 s / agent) | 85/85 | 0 | 0.11 ms | 10.2 ms | 10 ms | 8/s |

### Reading

**The margin is large, and M16 shrinks it without touching the verdict.** The real workload is
~8 msg/s. The tuned loop sustained 8,304 msg/s as first measured and 4,114 msg/s once the harness
was repaired — a margin of roughly five hundredfold rather than a thousand. Nothing was lost and
nothing hit a busy error in any configuration, before or after. The contention warnings are
accurate, and they describe a workload hundreds of times busier than seventeen agents exchanging
notes.

**Being precise about the untuned row.** It is *not* "no timeout versus timeout." Python's
`sqlite3` sets a 5-second busy timeout by default, so even the untuned run had one — which is why
it also lost nothing. What that row isolates is **WAL versus the rollback journal**, and the cost
is latency, not correctness: worst-case send went from 96 ms to 879 ms. Set the pragmas for
responsiveness; delivery does not depend on them at this scale.

### Reproduce

```bash
python3 bench/bench_queue.py
```

---

## M2 · Where "performant" actually binds

**Motivation.** M1 answered the wrong question, and noticing that is the most useful result in
this document. It measured how fast the queue moves messages *once a process is running*. But
agents invoke a CLI — **a fresh process per operation** — so the real per-message cost is startup
plus queue work, and startup is the term nobody measures.

**Method.** `bench/bench_startup.py`. 50 invocations per binary, median and p95 of wall-clock
`subprocess.run` time.

### Results

Five independent runs, because the first one produced a claim that did not survive repetition
(see *Correction* below).

| Per invocation | p50 range across 5 runs | Median of medians |
|---|---|---|
| Python `-c pass` | 16.34 – 19.91 ms | ~17.4 ms |
| Python `-c "import sqlite3"` | 18.70 – 20.17 ms | ~19.7 ms |
| Native binary (`/bin/echo`) | 1.31 – 1.74 ms | ~1.54 ms |
| The SQLite work itself (from M1) | — | 0.11 ms |

### Reading

**A native binary is ~12× cheaper per invocation** (~1.5 ms against ~19.7 ms). That ratio, paid
on every single call, is the entire performance case for Rust.

**The queue work is ~0.6% of a Python invocation.** Optimising the queue optimises the wrong
half — the process that runs it costs 180× what it does.

### Correction — a claim that did not survive repetition

The first run showed `pass` at 19.91 ms and `import sqlite3` at 19.74 ms, and I wrote that
**"importing `sqlite3` is free — the cost is the interpreter existing at all."** That was wrong,
and it was wrong in the way single measurements usually are: `pass` had come in anomalously slow
that run (19.91 ms against a 16.3–18.0 ms range everywhere else), which made the import look
free by comparison.

Repeated four more times, `import sqlite3` costs **+1.3 to +3.8 ms**, median ~+2.5 ms. So the
breakdown is roughly 17 ms of interpreter plus 2.5 ms of importing the driver.

The conclusion is unchanged — native still wins by an order of magnitude — but the ratio is
**~12×, not the 15× first reported**, and "the import is free" should not be repeated. Recorded
here rather than quietly edited, because the failure mode is the interesting part: a single run
of a noisy measurement produced a confident, specific, wrong sub-claim inside a correct
conclusion, which is the hardest kind to notice.

**At real concurrency** — 17 agents polling every two seconds is ~8.5 invocations/s:

| | CPU spent on process startup alone |
|---|---|
| Python CLI | **~166 ms/s** ≈ 17% of one core |
| Native CLI | **~13 ms/s** ≈ 1.3% of one core |

That gap, paid ~18 ms at a time on every invocation, is the entire performance case for Rust. It
is sufficient on its own.

### Reproduce

```bash
python3 bench/bench_startup.py
```

---

## M3 · Platform facts

Checked rather than assumed, because two design options depended on them.

| Fact | Method | Result |
|---|---|---|
| `/dev/shm` on Darwin | `ls -d /dev/shm` | **absent** — no RAM-disk shortcut for "in memory" |
| Rust toolchain | `rustup toolchain list` | 1.96.0 active, pinned by nestwatch's `rust-toolchain.toml` |
| Live sessions during research | `ls /tmp/cc-socks/ \| wc -l` | 17 |
| Message size on disk | M1, 1700 rows | ~450 bytes/msg (768 KB total) |

**On the Rust toolchain:** a bare `rustc --version` run from a scratch directory fails with
*"rustup could not choose a version of rustc"* — that is rustup having no global default, not a
missing install. Inside a project with a `rust-toolchain.toml` it resolves correctly. Worth
knowing so it is not misread as a blocker; this project will want its own toolchain file.

---

## What is still unmeasured

Stated so nobody reads the above as more complete than it is.

- **Rust CLI startup specifically.** The 1.31 ms figure is `/bin/echo`, a stand-in for "a small
  native binary." A real Rust binary linking `rusqlite` will be somewhat slower — plausibly 2–4 ms,
  still an order of magnitude under Python, but **this should be measured once the binary exists**
  rather than assumed.
- **Contention with a real mix of readers and writers over hours**, rather than a burst. The
  realistic scenario ran 10 seconds.
- **Behaviour when the database file lives on a network or synced volume** (iCloud, Dropbox).
  SQLite is known to be hazardous there; if the coordination file could ever land in a synced
  directory, that needs testing or an explicit guard.

---

## M4 · Re-measured and newly measured, 2026-08-27 (implementation session)

Taken during the design-validation pass. M2's harness re-run independently, plus the numbers the
delivery decisions (D9–D11) rest on.

### M2 reproduced

Three runs of 50 invocations each, same method as M2.

| Per invocation | p50 across 3 runs | Agrees with M2? |
|---|---|---|
| Native floor (`/bin/echo`) | 1.66 – 1.79 ms | yes (M2: 1.31 – 1.74 ms) |
| `python3 -c pass` | 18.04 – 19.24 ms | yes (M2: 16.34 – 19.91 ms) |
| **`node -e ''`** | **28.00 – 28.56 ms** | **new** |

**Why node matters.** Every hook currently configured on this machine is node-based. They run on
every session start and, for `UserPromptSubmit`, every turn — and are not noticed. That sets the
budget for adding `amb` to the hook chain: **the tolerated cost is already ~28 ms.**

**Still not measured, and must not be quoted:** a Rust binary linking `rusqlite`. The floor is
~1.7 ms; the real figure is unknown until the binary exists. `bench/bench_startup.py` gets it
added as a candidate then, per the standing note below.

### Platform facts checked, not assumed

| Fact | Method | Result |
|---|---|---|
| Stable session identity | `env` in fresh `exec` + subshell; transcript filename | `CLAUDE_CODE_SESSION_ID` is a UUID, inherited, and equals `~/.claude/projects/<slug>/<uuid>.jsonl` |
| Hook context injection | read devt's `session-start.sh`; matched its output against this session's own context | **`SessionStart` injects** via `hookSpecificOutput.additionalContext` |
| Platform messaging socket | `ls` on `$CLAUDE_CODE_MESSAGING_SOCKET` | `/tmp/cc-socks/<pid>.sock`, mode `srw-------` — **see the correction below** |
| Live sessions | `ls /tmp/cc-socks \| wc -l`, cross-checked against `ps` | 18, all mapping to live `claude` processes |
| In-repo dir and `git status` | throwaway repo, rule applied and withheld | **without** an ignore rule: `?? .msgboard/`. **With** one line in `core.excludesfile`: absent from `status`, `check-ignore` and `git add -A` |
| `$HOME` volume | `df`, iCloud/Dropbox probes | local APFS; not inside any sync root |
| Toolchain | `rustup default`, `rustc --version` | no global default — fails outside a `rust-toolchain.toml` directory, exactly as documented. Not a missing install |
| Shared cargo target | `~/.cargo/config.toml` | `target-dir = /Users/emrec/.cache/cargo-target` — confirmed global |

### Correction to the socket row, 2026-08-27 (hardening pass)

That row concluded the socket was *"not usable by an outside tool"* because the auth token is only
in the target session's own environment. **Re-probed:** the socket is restricted to the operating
system *user*, and on macOS and Linux the auth line is optional — Claude Code accepts a connection
with or without it. An ordinary process running as the same user connected to **18 of 18** peer
sockets (connect-then-close, nothing sent, no session disturbed).

Two things follow. The socket's file name is a real liveness oracle for a session, which is what
D21 now uses — 19 sockets mapped to 19 live `claude` processes with 0 stale. And pushing a message
into a peer's inbox socket is available, unlike what the original row said, which makes it a
genuine option for sub-second delivery rather than a closed door. It is **not** built: it writes
into another session's context and wants its own decision first.

### A correction recorded, since the failure mode is the interesting part

Mid-pass, a documentation summary asserted that `SessionStart` hook output is **not** injected
into model context. That contradicted a direct observation, and the observation won: reading the
devt plugin's own `session-start.sh` showed it emitting `hookSpecificOutput.additionalContext`,
and its exact output string was present in the reading session's context.

**The lesson is the same one M2 already taught, in the other direction.** There, a single noisy
run produced a confident wrong sub-claim. Here, a plausible secondary source nearly overturned a
correct first-hand observation. Both are cases of a specific, confident claim sitting inside
otherwise sound material — and in both, the fix was to go and look again rather than to reason
harder.

---

## M5 · The real binary, measured at last

**Measured 2026-08-27.** This is the number `KICKOFF.md` asked for and that M2 deliberately
refused to guess at.

**A trap worth recording, because it nearly produced a false number.** The first release build
had `rusqlite` as a dependency but `main.rs` never referenced it, so the linker dropped it
entirely — `nm` found **zero** `sqlite3_` symbols. Measuring startup at that point would have
reproduced exactly the error M2 warns about: an optimistic stand-in quoted as the real thing.
The binary measured below contains **307** `sqlite3_` symbols and does real work.

**Method.** 50 invocations of `amb inbox --json` against a database holding one message, so the
open path, the four pragmas, schema initialisation, auto-registration, the inbox query and JSON
rendering all run. Three independent runs.

| Per invocation | p50 across 3 runs | p95 |
|---|---|---|
| Native floor (`/bin/echo`) | 1.36 – 1.69 ms | ~2 ms |
| **`amb inbox --json` — the real hot path** | **3.00 – 3.42 ms** | ~3.6 ms |
| `python3 -c pass` — an *empty* interpreter | 16.95 – 17.18 ms | ~18 ms |

### Reading

**~3.1 ms, doing real work, against ~17 ms for Python doing nothing at all.** The prediction in
"What is still unmeasured" — *"plausibly 2–4 ms"* — was right, and the ratio is now ~5.5× against
an empty interpreter rather than ~12× against a stand-in. That is a *more honest* comparison and
still decisive: the Python figure is a floor that no real implementation could reach, while the
Rust figure is an actual working command.

**Against the hook budget.** The node-based hooks already running on this machine cost ~28 ms
(M4) and are not noticed. `amb` in the hook chain costs ~3 ms — roughly 11% of what is already
tolerated there.

**Where the ~1.4 ms above the native floor goes:** opening the file, four pragmas, and four
`CREATE TABLE IF NOT EXISTS` plus three indexes on every single open. The schema check is the
obvious candidate if this ever needs to be cheaper — a `PRAGMA user_version` comparison would
skip it — but at 3 ms against a 28 ms tolerated budget there is nothing to fix yet. Recorded so
the option is known rather than rediscovered.

### Reproduce

```bash
AMB_DB=/tmp/bench.db AMB_AGENT=bench AMB_PROJECT=benchproj cargo run --release -- inbox --json
```

---

## M6 · What the delivery hook costs

**Measured 2026-08-27.** The hook installed by `amb install --global` runs on every session start
and every turn boundary, in **every** Claude Code session on the machine — including sessions in
projects that have never used the board. So its cost in the *uninvolved* case matters more than
its cost in the involved one.

**Method.** 50 invocations of `amb hook turn` with a `Stop` payload on stdin, three runs.

| Scenario | p50 across 3 runs | p95 |
|---|---|---|
| **No board at all** — a session that never uses `amb` | **2.20 – 2.21 ms** | ~2.7 ms |
| Board present, inbox empty | 3.15 – 3.31 ms | ~3.8 ms |
| Board present, one message waiting | 3.18 – 3.32 ms | ~3.7 ms |

### Reading

**The no-board case is the one to watch, and it is the cheapest.** It short-circuits before
opening or creating anything: `db_path()`, one `exists()`, exit. A user who never touches the
board pays ~2.2 ms per turn and gets no database created in their home directory. That property
is enforced by `with_no_board_the_hook_says_nothing_and_creates_nothing`, which was mutation-
tested: deleting the fast path turns it red.

**Delivering mail is not measurably dearer than finding none** (3.18 vs 3.15 ms). The cost is
process start plus opening the database, not the query — consistent with M1, where the queue work
itself was 0.11 ms.

**Against the budget from M4:** the node hooks already installed on this machine cost ~28 ms and
are not noticed. This adds ~3 ms, about 11% of what is already tolerated.

### The `PostToolUse` hook, which fires most often of the three

`turn` and `monitor` also observe edits, so this hook runs after **every** `Edit` and `Write` —
far more often than session start or turn boundaries. Same method, three runs.

| Scenario | p50 across 3 runs | p95 |
|---|---|---|
| After an `Edit` — records a claim | 3.04 – 3.23 ms | ~3.9 ms |
| After a `Read` — claims nothing | 2.86 – 3.17 ms | ~3.6 ms |

**Recording a claim is not measurably dearer than declining to** (3.1 vs 3.0 ms, within run-to-run
noise). As in M1 and M6 above, the cost is process start and opening the database; the SQLite
work is lost in the noise. There is no case for batching or deferring the write.

### Reproduce

```bash
echo '{"hook_event_name":"Stop"}' | AMB_DB=/tmp/board.db AMB_AGENT=x AMB_PROJECT=p amb hook turn
```

---

## M7 · The hardening pass, measured — and a correction to a claim made in the same pass

**Measured 2026-08-27**, after the D20–D28 changes. Two numbers were wanted: whether skipping
schema re-assertion (D-numbered as part of the migration work) collected the saving M5 identified,
and whether the new per-invocation work — the `.git` walk of D20, the permission stats of the
board-hardening change — cost anything.

### The correction, first, because the failure mode is the interesting part

The first measurement said the hot path had regressed **from p50 3.00–3.42 ms to 3.74–3.92 ms,
about 15%**. The native floor was re-measured immediately after at 1.57–1.63 ms — inside M5's
recorded 1.36–1.69 ms — which appeared to rule the machine out and make the regression real.

It was not real. Isolating each change made things *slower*, not faster, which is the signature of
noise rather than cause:

| Variant | p50 |
|---|---|
| current | 3.69 ms |
| without the permission stats | 3.78 ms |
| without the `.git` walk | 3.89 ms |
| with the old always-run schema | 3.89 ms |
| current, repeated | 3.67 ms |

**This is M2's lesson replaying in the same document.** There, a single noisy run produced a
confident, specific, wrong sub-claim inside a correct conclusion. Here, an *unpaired* comparison —
new binary now, old binary quoted from a document written earlier — produced one. The floor check
did not catch it because `/bin/echo` is a tiny static binary and `amb` is a 3.2 MB one linking
SQLite; they do not drift together.

**The rule this adds to the standing note:** compare binaries **interleaved, in the same minute**,
not sequentially against a recorded figure.

### The measurement that stands

Both binaries built from source, run alternately, 60 invocations of `amb inbox --json` each, three
rounds. The pre-change binary was built from `HEAD` via `git archive` into a scratch directory, so
no working tree was disturbed; each binary had its own board, so the older one never met a
`user_version` it would refuse.

| Round | HEAD (before) | this branch |
|---|---|---|
| 1 | 3.09 ms | 3.29 ms |
| 2 | 3.20 ms | 3.18 ms |
| 3 | 3.24 ms | 3.03 ms |

**Indistinguishable.** Three changes to the open path — a `.git` walk up from the working
directory, four permission `stat`s, and skipping four `CREATE TABLE IF NOT EXISTS` plus three
indexes — net out to nothing the harness can see.

**So M5's deferred optimisation was not collected, and must not be claimed.** Migrations were
worth doing because F8's silent-divergence problem is real (D22's index fix is inert on an
existing board without them), not because they made anything faster. The startup budget is
unchanged: ~3.1 ms against the ~28 ms of node hooks already tolerated on this machine (M4).

### What the delivery change costs in context, which is the resource that actually binds

Not milliseconds — tokens. Sixty unread messages, one agent, consecutive `Stop` hooks:

| | before | after |
|---|---|---|
| turn 1 | 20,779 chars (~5,200 tokens) | 3,548 chars |
| turn 2 | 20,779 chars | 3,548 chars |
| turn 3 | 20,779 chars | 3,548 chars |
| turn 12 | 20,779 chars | **0 — backed off (D23)** |
| `amb inbox` after all that | 60 messages | 60 messages |

The last row is the point: the injection stops, the log does not.

### Reproduce

```bash
git archive HEAD | tar -x -C /tmp/amb-head && (cd /tmp/amb-head && cargo build --release)
# then alternate the two binaries, 60 runs each, three rounds — never one after the other
```

---

## M8 · What happens when every session opens the board at once

**Measured 2026-08-27**, during the validation pass over the D20–D29 work. It is the measurement
that pass most needed and did not have: every earlier concurrency number here assumes the board
*already exists* and is *already in WAL*.

**Why that assumption hid two failures.** `tests/concurrency.rs` registers each participant
serially before racing anything, and M1's harness built its schema up front. So both measured
concurrent **use** and neither measured concurrent **arrival** — which is exactly how a machine
with nineteen live sessions meets a new board, or a new schema version rolling out to every
session's next hook.

**Method.** N unrelated processes, spawned before any is waited on, each running `amb register`
against the same path. Failures counted from a non-zero exit.

### Concurrent first open of a brand-new board

| | 12 racers |
|---|---|
| Before | **10 failed** — `database error while setting journal_mode`, exit 69 |
| `busy_timeout` moved ahead of the pragma | **2 failed** |
| Plus read-before-write and a bounded retry | **0 failed**, five rounds running |

The middle row is the interesting one, and it is why the first fix was not the whole fix:
**`busy_timeout` does not cover a journal-mode switch.** That needs a brief exclusive lock, and
SQLite declines to invoke the busy handler for it rather than deadlock against a connection
already holding a shared lock — it returns `SQLITE_BUSY` immediately, timeout or no. Halving the
failures looked like success; repeating the run showed it was not.

### Concurrent schema upgrade, from a board stamped one version back

| | 10 racers |
|---|---|
| Deferred transaction, version read outside the lock | **8 failed** |
| `BEGIN IMMEDIATE`, version re-read inside it | **0 failed** |

All ten also completed the work they came to do, and the board finished at the current version.

### Cost

None on the hot path. `amb inbox --json`, interleaved against a `HEAD` build, 60 invocations each,
three rounds: **HEAD 2.89–3.18 ms, this branch 3.02–3.20 ms.** The retry loop is only entered by
the one open that genuinely races another for a new file, and the read-before-write means every
later open finds WAL already engaged and skips the write.

### The regression guard is probabilistic, and that is recorded rather than hidden

`concurrent_conversions_to_wal_all_succeed` reproduces the premise, but with the fix removed it
goes red in only about **three runs out of six**. Three things were tried and measured:

| Approach | Detection with the bug present |
|---|---|
| Race an absent file, 12 racers | 3 / 5 |
| Race an absent file, 24 racers | 2 / 6 |
| Race an absent file, 40 racers | 0 / 6 |
| Seed the board not-yet-WAL, 12 racers | 3 / 6 |

**More racers detect it less**, which is the counter-intuitive part: spawning them serialises
their arrival, so the first process converts the file before the rest turn up. An in-process
version using a thread to hold a read lock was written and then **deleted** — it never reproduced
the contention at all, so it passed without exercising the retry. That is worse than a weak guard,
because it looks like a strong one.

The guard is kept at half-detection because that is real detection across repeated runs, and
because the numbers above are the actual evidence. Making it airtight needs a start gate the child
processes block on so they arrive together; that is not built.

### Reproduce

```bash
for i in $(seq 1 12); do AMB_DB=/tmp/race.db amb register >/tmp/o.$i 2>&1 & done; wait
grep -c 'amb:' /tmp/o.*      # expect none
```

---

## M9 · What the memory layer costs, and one expected win that did not materialise

> **The harness behind every number here was void from 2026-08-28 until 2026-08-29 (M18).** D81
> renamed the `project:` frontmatter key to `scope:` hours after these were taken; both harnesses
> wrote the old key from their own copy of the format, so `parse_note` rejected every synthetic
> note and they measured an *empty vault* while printing full tables and exiting 0.
>
> Repaired and re-run on 2026-08-29: **every timing row below reproduces within noise, and the
> three `SessionStart` rows of the token table do not** — they are ~377 characters higher, a
> constant, from guidance text added to the primer on 2026-08-28 (D60's containment framing, the
> `--same-as` block, the accurate-zero line). Both tables are corrected in place with the delta
> shown. The conclusion each supports is unchanged; one of the numbers under it is not.

**Measured 2026-08-28**, release build, three interleaved runs of 50 invocations each. Interleaved
rather than run-to-completion so a machine that gets busy halfway through skews every scenario
equally — the failure mode M4 and M7 both record.

`AMB_MEMORY` adds two hook entries of its own (D41), so these are costs *beside* mail delivery
rather than added to it. The reference row is the existing delivery hook measured in the same
pass, on the same machine, in the same minute.

| Scenario | p50 across 3 runs | p95 |
|---|---|---|
| **`SessionStart`, memory off** (`AMB_VAULT` unset) | **2.02 – 2.27 ms** | ~2.9 ms |
| `SessionStart`, empty vault | 2.79 – 2.98 ms | ~3.5 ms |
| `SessionStart`, 8 notes | 2.90 – 3.18 ms | ~3.6 ms |
| `SessionStart`, 100 notes | 4.01 – 4.23 ms | ~4.6 ms |
| `SessionStart`, 1000 notes | 3.68 – 4.00 ms | ~5.3 ms |
| `PreToolUse`, skipped tool (`TodoWrite`) | 2.74 – 2.85 ms | ~3.3 ms |
| `PreToolUse`, no match | 3.30 – 3.60 ms | ~4.4 ms |
| `PreToolUse`, match | 3.43 – 5.02 ms | ~5.5 ms |
| *delivery hook (`Stop`) — reference* | *2.90 – 3.26 ms* | *~3.8 ms* |

### Reading

**Memory costs about what mail costs.** 2.9–4.2 ms against the delivery hook's 2.9–3.3 ms, on a
budget M4 measured as ~28 ms already tolerated for node-based hooks on this machine. Switched off,
it is 2.0–2.3 ms — the price of starting a process and finding no `AMB_VAULT`, which is what every
session on the machine pays for having the entry installed.

**1000 notes is cheaper than 100, and that is the index bound working.** `AUTO_INDEX_LIMIT` is
500: above it `SessionStart` declines to re-scan the project directory and does one `read_dir`
instead of 1000 `stat`s. The 100-note row is paying for 100 mtime comparisons; the 1000-note row
is not paying at all. The bound is visible in the measurement, which is the only reason to trust
it exists.

### The expected win that did not materialise

`concerning()` had no `LIMIT`: it fetched **every** matching note — each with a `group_concat`
subquery for its paths — to display eight. On the hook that fires before every file tool call that
looked like the obvious cost, and windowing the fetch to 64 rows looked like the obvious fix.

**Measured before and after, it changed nothing.** 4.64 – 4.94 ms before, 4.54 – 4.76 ms after:
inside run-to-run noise.

So the cost was attributed properly instead, with two vaults of the same size differing only in
how many notes concern the queried path:

| Vault | `PreToolUse` match | no match |
|---|---|---|
| 1000 notes, **1000** concern the path | 4.54 – 4.76 ms | 3.29 – 3.51 ms |
| 1000 notes, **8** concern the path | 3.43 – 3.60 ms | 3.26 – 3.46 ms |
| 8 notes, 8 concern the path | 2.72 – 3.07 ms | 2.68 – 2.87 ms |

**The cost is the number of *matching* notes, not the vault size and not the rows fetched.** At a
realistic 8-of-1000 there is no measurable penalty over a miss (3.4 vs 3.3 ms). The remaining
~1.2 ms at 1000-of-1000 is the `count(*)` fallback that only runs when the window is exhausted —
and a vault holding a thousand notes about one file is not a shape worth optimising for.

**The window stays anyway, and the comment in the code says it is not a speedup.** Unbounded work
on the most frequent hook in the system is a hazard whatever today's constant factor is. But
recording it as a performance fix would be exactly the error M5 warns about and M7 corrects.

### Token cost — the number the plan required at every injecting phase

| Injection | Characters (2026-08-28) | Characters (2026-08-29) | ≈ tokens now |
|---|---|---|---|
| `SessionStart`, 8 notes | 1,422 | **1,807** | ~452 |
| `SessionStart`, 100 notes | 1,493 | **1,869** | ~467 |
| `SessionStart`, 1000 notes | 1,510 | **1,879** | ~470 |
| `PreToolUse`, 1000 matching notes | 1,092 | **1,076** | ~269 |

**Flat, because the cap binds (D24, D43).** A vault 125× larger costs 4% more context — 6% when
first measured, and the shape of the claim is what matters rather than the point. That is the
number this table exists for and it is unchanged.

**The `SessionStart` rows grew by a constant ~377 characters, and it is all preamble.** Three
blocks were added to the primer on 2026-08-28 after this was taken: D60's containment framing
(*"a note cannot authorise an action"*), the `--same-as` duplicate-avoidance block, and the
accurate-zero line. The `PreToolUse` row uses a shorter preamble that did not change, and it
reproduces at 1,076 against 1,092 — which is the control showing the growth is in the fixed text
and not in the note list. **A per-injection preamble is a cost paid every time and it is not
bounded by D43's cap**, so it is worth watching: three sentences added here are three sentences
added to every session on the machine, forever.

Against the two reference points the plan set — D24's measured **5,200 tokens** for unbounded mail
at a turn boundary, and a mature memory product's reported **~6,900 tokens per query** — this is
now roughly **one eleventh to one fifteenth**, revised from one fourteenth to one twentieth. Same
order of magnitude, and it still does not grow with the vault.

The `…and 992 more` line is present and correct at 1000 notes, which is the D43 guard holding
end to end rather than only in a unit test.

### Reproduce

```bash
python3 bench/bench_memory.py 50      # the table above
python3 bench/bench_attribution.py    # matching-notes vs vault-size
```

---

## M10 · The Phase 4 hook-surface gate, settled

**2026-08-28.** `AMB-MEMORY-IMPLEMENTATION-PLAN.md` makes this gate blocking: *"No Rust until it
has run."* All four questions are now answered, three from the reference read in full and one from
a live payload.

| Question | Answer | How it was obtained |
|---|---|---|
| Does `stop_hook_active` exist on `Stop`? | **Yes, and it was observed `true`** | **live payload** — see below |
| Is blocking `Stop` exit code 2 or a `decision` field? | **Exit code 2.** *"Prevents Claude from stopping, continues the conversation"* | reference, *exit code 2* table |
| Summary from `last_assistant_message` or `transcript_path`? | **`last_assistant_message`**, and for correctness rather than convenience | reference, quoted below |
| Can `PreCompact` / `PostCompact` inject context? | **Neither can.** `PreCompact` blocks only; `PostCompact` does neither | reference, *Decision control* + *exit code 2* tables |

### `stop_hook_active` — undocumented, but real

**Not in the reference, on two full readings.** It was found instead in a `Stop` payload delivered
to a hook that was running in this session, which reported it as **`stop_hook_active: true`**.

**The circumstance is the confirmation.** That hook is one that blocks and continues the
conversation, so at the moment it read the payload the session was in exactly the re-entry state
the field names — a `Stop` that is happening *because* a `Stop` hook already fired. A field that is
true precisely when re-entry is in progress is the re-entry guard `MEMORY-DESIGN.md` §9.2 requires,
and §11's infinite-loop risk moves from **unmitigated** to **mitigated by a named mechanism**.

**Provenance, marked because it matters:** this is a *report from a hook that received the payload*,
not a byte I captured myself. It is empirical rather than documentary, and it is consistent with
the semantics and the situation — but a first-hand capture would be stronger, and the way to get
one is a `Stop` hook installed in a **new** session (see below).

### `last_assistant_message` beats the transcript for a correctness reason

> *"The transcript file is written asynchronously and may lag the in-memory conversation, so it may
> not yet include the current turn's most recent messages when a hook fires."*

So 4b's split is not tidiness. The **summary** must come from `last_assistant_message` on `Stop` and
`SubagentStop`; only the **facts** may be parsed from `transcript_path`, where lag costs
completeness rather than accuracy.

### A negative result about the probe itself, recorded because it cost two turns

A `Stop` hook was installed in `.claude/settings.local.json` — a documented settings location, at
precedence level 3 — and **it never fired**, across two turns with the file in place. The
documentation is explicit that this should work:

> *"Claude Code watches your settings files and reloads them when they change, so it applies most
> edits to the running session without a restart, including edits to `permissions`, `hooks`… The
> reload covers user, project, local, and managed settings."*

The script was verified runnable by driving it directly. **Unresolved**: either the reload does not
cover a hook entry that is newly *added* to project-local settings, as opposed to one that is
edited, or something else prevented it. **The practical consequence for anyone probing hooks here:
add the entry, then start a new session** — do not conclude anything from a hook that was installed
mid-session and stayed silent.

**And the first null result nearly became a finding.** One silent turn was almost recorded as "hook
config is not reloaded mid-session", which the reference flatly contradicts. What made the silence
legible as *inconclusive* rather than as evidence was that the probe wrote a proof-of-execution
file, so "ran, field absent" and "never ran" were distinguishable. A probe that can only produce
one kind of silence cannot tell you which one you got.

---

## M11 · The concurrency defect in vault writes, and the cost of fixing it

**Measured 2026-08-28**, release build, 24 concurrent OS processes deriving one candidate, five
rounds before and after. Recorded as D55.

| | round 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| before the lock | 22/24 | **7/24** | 23/24 | 23/24 | 22/24 |
| after | 24/24 | 24/24 | 24/24 | 24/24 | 24/24 |

**The first probe — 8 processes, one round — lost nothing**, and would have been recorded as
"concurrent derivation is safe". The window is a file read followed by a file write; not hitting it
is the default outcome. This is the third time `CLAUDE.md`'s repeat-before-quoting rule has changed
a conclusion in this file.

### What the fix costs

Nothing measurable on the read path, which is the one that matters. Re-running M9's harness against
the current binary:

| Scenario | M9 (before Phases 2–4) | now |
|---|---|---|
| `SessionStart`, 8 notes | 2.90 – 3.18 ms | 3.06 – 3.92 ms |
| `PreToolUse`, no match | 3.30 – 3.60 ms | 3.54 – 3.68 ms |
| memory off | 2.02 – 2.27 ms | 2.25 – 2.42 ms |
| *delivery hook, reference* | *2.90 – 3.26 ms* | *3.24 – 3.27 ms* |

Everything moved up by roughly the same 0.2–0.4 ms **including the untouched delivery reference**,
which is the signature of machine load rather than of the change. **Injection takes no lock at
all**: twelve concurrent `SessionStart` hooks complete in 33 ms in total, so they are not queueing
behind writers.

### Reproduce

```bash
cargo test --test concurrency concurrent_derivations_do_not_lose_strikes
python3 bench/bench_memory.py 40
```

---

## M12 · What the `dirty` marker costs the inner loop

**Measured 2026-08-28** against the current binary, on the question D56 left as a code comment:
`build.rs` declares `src` as a rerun-if-changed input so the ` dirty` marker reflects the working
tree, and cargo attaches build-script output to the *whole package* — so touching one file in
`src/` re-runs the script and rebuilds the library and all eight integration binaries with it.

### Harness

```bash
# Variant B is the shipped build.rs with `PathBuf::from("src"),` removed from watch_list().
for r in 1 2 3; do
  cp build.rs.shipped build.rs; touch src/main.rs; time cargo test --no-run   # A
  cp build.rs.nosrc   build.rs; touch src/main.rs; cargo test --no-run        # settle
                                touch src/main.rs; time cargo test --no-run   # B
done
```

Interleaved within each round rather than run as two blocks, per M7 — the two variants see the
same machine load.

| | round 1 | 2 | 3 |
|---|---|---|---|
| **A** · as shipped, `src` watched | 3.31 s | 3.08 s | 3.30 s |
| **B** · `src` unwatched | 0.73 s | 0.62 s | 0.65 s |

**About 5x, or +2.6 s, on a `src`-only edit followed by `cargo test`** — which is the inner loop
`CLAUDE.md` names as the command to run.

### Why it is paid rather than avoided

**An earlier version of this claim, written in `build.rs` as a code comment, was wrong twice.** It
quoted "~3.7 s against ~1.4 s" from a single unrepeated run, and it excused the cost with "`cargo
test` and `cargo clippy --all-targets` rebuild the library regardless." Measured, they do not:
variant B completes `cargo test --no-run` in 0.62–0.73 s after a `main.rs`-only edit, and the
library is not rebuilt. The sentence retired a cost that is real.

Two cheaper options were considered and rejected:

- **Drop `src` from the watch list.** The marker then under-reports — a binary built from
  uncommitted source reports itself clean, which is the single thing it exists to catch. Wrong in
  the expensive direction.
- **Emit the `src` watch only when `PROFILE=release`.** Attractive, and rejected on a fact about
  this tool: `amb install` records `current_exe()`, so a debug binary *can* be the installed one.
  The marker would then lie in exactly the builds a developer installs.

The residual gap stands as `build.rs` documents it: a doc- or test-only edit leaves the tree dirty
without re-running the script, so such a build can still report itself clean. That is the cheap
direction, and source is watched.

---

## M13 · What actually grows in the board, and it is not what was predicted

**Measured 2026-08-28**, twice with identical results, via `dbstat` on the live board
(`~/.agent-messageboard/board.db`, 266,240 bytes total).

```sql
SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC;
```

| Table | Rows | Bytes | Bytes per row |
|---|---|---|---|
| **`messages`** | 19 | **61,440** | **3,233** |
| `note_events` | 57 | 16,384 | 287 |
| `notes` | 25 | 16,384 | 655 |
| `claims` | 79 | 16,384 | 207 |
| `reads` | 24 | 4,096 | 170 |

**The prediction was that `note_events` would be the table that moves first**, because it grows per
injection per session. On the evidence it is `messages`, at 3.7× the size on a third of the rows:
a message stores its **body inline**, and agents write long ones — several broadcasts in the session
that produced this measurement are multi-kilobyte.

Two different growth axes, and only one of them is visible in a row count. `note_events` grows in
**rows** at a rate the design controls (`MAX_INJECTED` is 8). `messages` grows in **bytes** at a
rate nothing controls, because it is however much an agent decided to say.

Neither is close to mattering — D83 sets the trigger at 50 MB or a slow `amb inbox`. Recorded
because the wrong table was named in a document first, and a retention design built on that
sentence would have pruned the ledger and kept the prose.

---

## M14 · What searching the file instead of the index costs

**2026-08-29.** D88 moved `recall`'s text match off `body_excerpt` — a 240-character slice of a
note's first paragraph — and onto the note file. The question that decides whether that is
affordable is the *worst* case, which is a query matching nothing, because it reads every
candidate.

Release build, isolated scratch board and vault, three runs of twenty invocations each. **The
machine was running eighteen live Claude Code sessions**, so these are a loaded ceiling rather than
a best case.

| Corpus | Path | run 1 | run 2 | run 3 |
|---|---|---:|---:|---:|
| 603 notes · 2.4 MB | `recall <hit deep in a body>` | 4.40 | 4.36 | 4.69 |
| 603 notes · 2.4 MB | `recall <matches nothing>` — reads every note | 11.66 | 11.23 | 12.46 |
| 603 notes · 2.4 MB | `hook memory` · SessionStart | 4.95 | 5.09 | 5.06 |
| 603 notes · 2.4 MB | `hook memory` · PreToolUse | 4.87 | 4.73 | 5.07 |

Before the change a miss cost about 4.2 ms at the same corpus. **The worst case roughly tripled,
to about twelve milliseconds.** Two things make that the right trade rather than a regression to
apologise for:

- **`recall` is not a hook.** Nothing on the delivery or injection path searches, and the two hook
  rows above are unchanged from the pre-D88 measurements at the same corpus. The per-turn tax D24
  bounds is untouched.
- **The alternative was a wrong answer.** The 4.2 ms miss included every note whose match sat past
  the first paragraph, which was the defect.

An early hit is unchanged at 4.4 ms, because the walk stops at the caller's limit.

### What was checked and is not quoted as ours

`grep -rl` over the same vault measured 4.7–6.2 ms at 603 notes and 7.1–10.8 ms at 6,000 notes /
23 MB. That is a different program doing a different job — no frontmatter split, no ordering, no
index filter — and it is recorded only as the reason a file scan was believed affordable before it
was built. **It is not a claim about `amb`'s implementation**, which allocates a lowercased copy of
each body and is measurably slower. The number to quote for `amb` is the table above.

### The escalation, and its trigger

A contentless FTS5 table (`content=''`) stores an index and returns `NULL` for every column, so it
satisfies D34 without storing note content. It is the answer if this worst case stops being
acceptable — a much larger vault, or a `searches` ledger (D89) showing recall being run often
enough that twelve milliseconds is a real cost. Neither is true today, and building it now would be
the ceremony D45 and D51 warn about.

---

## M15 · The harness behind M2 was broken, and the numbers it published were right anyway

**2026-08-29.** `bench/bench_startup.py` is cited by M2 above and by `README.md` as the harness
behind the startup figures. **It could not produce two of the four rows those documents publish.**

Three faults, none of which made anything fail:

- The `amb` candidate was commented out, behind a header reading `# Uncomment once built:`. The
  binary had existed for days.
- It pointed at `./target/release/amb`. Every Rust project on this machine shares one target
  directory (`~/.cache/cargo-target`), so that path does not exist here — uncommenting it would
  have skipped the row silently, because the loop skips a candidate it cannot find.
- There was no `amb inbox` candidate at all, though README publishes one.

The script ran, printed three rows, and exited 0. A reader following the citation to reproduce
`amb --version` at 2.1 ms would have got a table without that row.

### The published figures were honest

This is the part worth recording, because history will otherwise leave it ambiguous. Repaired and
run twice on 2026-08-29, release binary `4dbe9de` at schema 12, on a machine running eighteen live
Claude Code sessions:

| Per invocation | published (2026-08-27) | reproduced (2026-08-29, two runs) |
|---|---|---:|
| `python3 -c pass` | 15.9 / 15.6 ms | 16.00 / 16.20 ms |
| `/bin/echo` | 1.4 / 1.4 ms | 1.54 / 1.31 ms |
| **`amb --version`** | **2.1 / 2.1 ms** | **2.15 / 2.40 ms** |
| **`amb inbox`** | **3.0 / 2.9 ms** | **3.14 / 3.26 ms** |

Every row reproduces within noise, slightly high in the direction the load and the extra migration
step predict. **The measurements were sound; only the artefact asserting the method was not.**

### Why this is recorded rather than quietly fixed

Deleting the harness was a live option and would have been the wrong one. The instinct that a
script nobody verifies is a false comment with a shebang is right — but the correction available
here was repair, not deletion, and the evidence is that the citation was the *only* record that
anyone had reproduced these figures. Delete it and a later reader has a table with no method and no
way to tell whether it was ever trustworthy.

That inverts the usual direction of this project's false-comment rule. `sync_dir`'s comment (D67)
and `recall`'s (D88) each made *wrong* behaviour look considered. This one made *correct* work look
suspect. An artefact asserting a method is a claim in its own right and can fail either way.

### What now runs it

`tools/bench.sh`, deliberately **not** in `tools/verify.sh`: the four harnesses cost roughly 17s
together (`bench_queue.py` alone is 11.5s, spawning 17 concurrent writers), against a gate meant to
run before every commit. Its first line states that it verifies execution and coverage and asserts
nothing about values, because a harness check and a performance gate look identical from outside
and this machine cannot honestly run the second.

`bench_startup.py` now **fails loudly** when a binary exists and no `amb` row ran — the rot was
silent, so the guard is not. `check_docs.py` gained a check that every script in `bench/` is cited
by a document: the citation is the promise, and an uncited harness should be deleted rather than
kept.

---

## M16 · M1's harness could not express `@@`, and its headline number is a loop rate

**2026-08-29.** `bench/bench_queue.py` is cited by M1, by `DESIGN.md` and by `DECISIONS.md` D4 as
the harness behind **8,304 msg/s**. M1's Method says it *"builds the real proposed schema"*.
**It did not, in three ways, and one of them removed an addressing mode from everything ever
measured here.**

- `messages.to_proj` was declared `TEXT NOT NULL`. The global broadcast is `to_agent IS NULL AND
  to_proj IS NULL` — so **`@@` was not merely unexercised, it was unrepresentable**; an insert
  raises `IntegrityError`. D17 calls the 2×2 over two nullable columns this design's central
  claim, and the benchmark was run against a table where one cell of it cannot exist.
- The index was a single `ix_inbox(to_proj, to_agent, id)`. The shipped board has two,
  `ix_inbox_proj(to_proj, id)` and `ix_inbox_agent(to_agent, id)` — a different write cost and a
  different plan.
- The reader was `SELECT m.id … LIMIT 50`. `messages::select` joins `agents`, projects every
  column including the ~300-byte body, and **has no LIMIT**. There was no `agents` table at all,
  so even had the join been written it would have returned NULL for every sender name.

The script ran, printed three healthy rows, and exited 0 — the M15 shape exactly, one day later
and in a document with more authority.

### What reproduces, and what moves

Old harness re-run three times before touching it, then the repaired harness three times, on the
same machine (python 3.14.7, sqlite 3.53.4, Darwin arm64, 12 cores, ~18 live sessions).
Saturation, tuned, 17 processes × 100 messages:

| | published 2026-08-27 | old harness, 3 runs | **repaired, 3 runs** |
|---|---|---|---|
| loop throughput | 8,304/s | 8,152 / 8,489 / 9,741 | **3,948 / 4,114 / 4,694** |
| send p50 | 0.02 ms | 0.02 / 0.02 / 0.02 | 0.09 / 0.09 / 0.05 |
| send p99 | 9.59 ms | 9.84 / 3.95 / 4.94 | 37.2 / 43.2 / 25.1 |
| send max | 96 ms | 94.8 / 100.2 / 94.2 | 255 / 222 / 211 |
| `SQLITE_BUSY` | 0 | 0 / 0 / 0 | **0 / 0 / 0** |
| lost | 0 | 0 / 0 / 0 | **0 / 0 / 0** |

Untuned saturation: 1,682/s published, 1,699 / 1,861 / 1,886 old, **1,335 / 1,425 / 1,376**
repaired. Realistic: 8 msg/s throughout, unchanged.

**The original number was reproducible.** 8,304 sits inside the old harness's own spread. It was
not fabricated and it was not stale — it measured a different thing than the sentence around it
claims.

### The finding is what kind of number it is

Throughput here is `sends / wall-clock` over a **send-then-read loop**. Give the loop the shipped
reader and it halves, because the reader now returns ~336,000 rows across 1,700 reads instead of
being capped at 50 ids. **So `msg/s` was never a write-capacity figure**, and both citation sites
used it as one — `DESIGN.md` under the `BEGIN IMMEDIATE` paragraph, D4 as the reason DuckDB buys
no performance back.

The numbers that answer *"does the single writer become the bottleneck"* are the send-latency
percentiles and the busy count, which are measured on the write path alone. Those still say no:
zero busy, zero lost, p50 well under a millisecond at seventeen concurrent writers. Latency rose —
p99 9.6 ms → ~35 ms — because the readers now hold real read transactions, which is the honest
cost and still three orders of magnitude clear of a workload that sends eight messages a second.

**This is question 1 of the ratio rule wearing different clothes.** Not a ratio, but the same
defect: *what is one unit of this number?* One unit of `8,304/s` is "one send, plus one capped
50-row id scan". One unit of the claim it was cited for is "one send". Those are not the same
sentence, and nothing in the code doing the dividing could show it — the division is correct. It
was visible only in the harness's DDL, which is exactly where CLAUDE.md says to go looking.

### The guard

`bench_queue.py` now exits 1 when the saturation run contains zero messages in any addressing
mode, or when the inbox query returns no rows. **It asserts coverage, never a value**, for
`tools/bench.sh`'s stated reason. Verified by mutation: delete the `@@` branch from `addressed`
and the script fails with *"no global messages were sent — that addressing mode went unmeasured"*;
restore it and it passes. On the pre-repair harness this guard could not even have run — the
insert would have raised.

`PRAGMA foreign_keys=ON` now runs on **both** connections rather than only the tuned one, so
scenario [2] still isolates exactly one variable, as M1 says it does.

### Reproduce

```bash
python3 bench/bench_queue.py     # or tools/bench.sh for every harness
```

---

## M17 · Mutation-testing `messages.rs`, the module holding the central design claim

**2026-08-29.** CLAUDE.md makes mutation testing a standing convention — *"after adding a guard,
delete it and watch the test go red"* — and D51 records that the *survival* of a mutant was the
finding, not its death. **The module that had never been put through it is the one holding
`select()`**, the single query D17 calls this design's central claim. Seven `#[test]` in 880 lines,
against `hooks.rs`'s thirty in 1,157; its coverage was almost entirely indirect, through
process-level suites that drive the binary.

**Method.** `cargo-mutants 27.1.0`, `--file src/messages.rs --copy-vcs true --jobs 1`, with
`CARGO_TARGET_DIR` pointed at a private directory and nothing else building. All three flags are
load-bearing and the reason is in the next section.

### The first run was void, and the way that was caught is the useful part

`cargo mutants` copies the source tree but **not** the cargo config, so it inherits this machine's
shared `target-dir` and compiles every mutant into it under this package's own name. Two failures
followed, in opposite directions:

- `./tools/verify.sh` ran while the mutation run was in progress and three `messages.rs` tests
  failed with `attempt to subtract with overflow`. **The gate had tested a mutant.**
- Afterwards `cargo test` reported **225 lib tests where the source held 231**. The stale binary
  was reused and six newly-written tests were simply absent — a green run that proved nothing and
  said so in no way at all. `cargo clean -p amb` fixed it and freed **17.3 GiB**; that volume under
  one package name *is* the collision, measured.

The results were discarded rather than reasoned about, and the re-run proved that was right:
`replace Message::scope -> &'static str with ""` was reported **missed** by the polluted run and
**caught** by the clean one. `each_scope_is_labelled_distinctly` had been pinning all three scope
labels the whole time.

### Results

| | mutants |
|---|---:|
| generated | 80 |
| unviable (do not compile) | 8 |
| **viable** | **72** |
| caught | 60 |
| **missed** | **12** |

**60 of 72, or 83%.** The twelve fell into five clusters, and no cluster was a near miss — each
was a rule with no test touching it at all.

| cluster | mutants | what it deletes |
|---|---:|---|
| `Message::is_broadcast` / `is_global` | 5 | which of the four addressing modes `--json` reports |
| `watch` | 3 | the blocking-read lane the `SessionStart` banner tells every agent to use |
| `nearest`'s tie guard | 2 | D26: a clear winner among close candidates is still suggested |
| `undelivered` | 1 | D25: mid-turn delivery |
| `distance`'s first column | 1 | the edit metric `nearest` thresholds on |

### Every one is a silence

*"This project's failures are silences, not errors"* is the first line of its own conventions, and
the survivors are a list of them. `undelivered` returning `Ok(vec![])` deletes the `PostToolUse`
lane: mail still arrives, at the next `Stop`, so the symptom is **later, not lost**. All three
`watch` mutants collapse to the same thing — returns nothing, immediately, forever — on the command
the banner names for immediate delivery. Both `nearest` mutants mean "never suggest when two names
are close", which reads as the conservative behaviour the function documents rather than as a
defect.

### Two of the five clusters were decorative assertions, and one had a false comment on it

This is the part that generalises. `a_tie_produces_no_suggestion_at_all` contained:

```rust
// A clear winner among several candidates is still suggested.
assert_eq!(nearest("api-v1x", &["api-v1", "totally-elsewhere"]), Some("api-v1"));
```

`totally-elsewhere` is outside the budget, so the filter drops it and `scored` reaches the
**one-candidate** arm. The `best < runner_up` guard is never evaluated by this assertion, which is
why replacing it with `false` survived, and why flipping `<` to `>` did too. The comment states the
rule correctly and the code beneath it tests a different one — D88's shape, in a *test* rather than
in production code, where it is harder to see because a passing assertion looks like evidence.

`Message::is_broadcast` and `is_global` are the same defect without the comment: read only by
`to_json`, and no test anywhere asserted those keys.

### After

Four tests added and one assertion repaired: the machine surface across all four addressing modes,
the mid-turn ration together with its explicit-read exception, the edit metric as a table including
the column the mutant lived in, `watch` on both the mail-waiting and the deadline path — and
`a_tie_produces_no_suggestion_at_all`'s second candidate changed to one inside the budget, so the
guard is actually evaluated. `messages.rs` goes from 7 `#[test]` to 11.

Re-run clean, same flags, same machine: **72 of 72 viable mutants caught, 0 missed.**

### Reproduce

```bash
tools/mutants.sh src/messages.rs
```

The script exists so the three flags above are not a thing to remember: it forces a private
`CARGO_TARGET_DIR`, passes `--copy-vcs true` so `build.rs` can fingerprint the repository, and
pins `--jobs 1`. Nothing else may build while it runs — a mutation result produced alongside
another `cargo` is **void rather than weak evidence**.

It also offers `--diff` for changed lines only, and says in its own header why that mode is
**not** a gate: cargo-mutants matches the diff against the code under test and not the test code,
so a commit that deletes a test generates no mutants and passes green. A mechanism blind to the
one change it exists to catch is D58's shape, so it is offered for feedback and wired into
nothing.

---

## M18 · Both memory harnesses measured an empty vault for a day, and printed full tables

**2026-08-29.** M15 repaired `bench_startup.py`; M16 repaired `bench_queue.py`. This is the other
two, and it is the worst of the three because the failure had a *published diagnosis* that the
harness threw away.

### What happened

`bench_memory.py` and `bench_attribution.py` each built a synthetic vault by writing markdown with
this frontmatter, from its own private copy of the format:

```
---
project: "bench"
title: "observation number 0"
...
```

**D81 renamed that key to `scope:` and deliberately removed the fallback** — *"the vault is
regenerable, and a key that means two things in two files is the drift this whole change
removes."* `parse_note` does `get("scope")?`, so the old key does not degrade: it rejects the note
entirely. `c4ffb01`, 2026-08-28 — the same day M9's numbers were taken, hours after.

From that commit until 2026-08-29 both scripts indexed **zero** notes, measured an empty vault,
printed complete tables and exited 0. `bench_attribution.py`'s entire experiment is *"two vaults
of the same size differing only in how many notes concern the queried path"*; with nothing indexed
its three rows were one measurement printed three times:

| Vault | hit, while void | miss, while void |
|---|---|---|
| 1000 notes, 1000 concern the path | 2.87 – 3.01 ms | 2.87 – 3.04 ms |
| 1000 notes, 8 concern the path | 2.75 – 2.95 ms | 2.62 – 3.04 ms |
| 8 notes, 8 concern the path | 2.79 – 2.96 ms | 2.74 – 2.99 ms |

The conclusion that table exists to support — *the cost is the number of matching notes* — is
unsupportable by it. Every row is the same number.

### The published figures were honest, for the third time

Repaired and re-run on 2026-08-29, same machine:

| | published | reproduced |
|---|---|---|
| `PreToolUse` match, 1000-of-1000 | 4.54 – 4.76 ms | **4.60 – 4.93 ms** |
| `PreToolUse` no match, 1000-of-1000 | 3.29 – 3.51 ms | **3.42 – 3.66 ms** |
| `PreToolUse` match, 8-of-1000 | 3.43 – 3.60 ms | **3.46 – 3.64 ms** |
| `SessionStart`, memory off | 2.02 – 2.27 ms | **2.13 – 2.27 ms** |
| `SessionStart`, 100 notes | 4.01 – 4.23 ms | **4.09 – 4.25 ms** |
| `SessionStart`, 1000 notes | 3.68 – 4.00 ms | **3.86 – 4.15 ms** |
| delivery hook (`Stop`), reference | 2.90 – 3.26 ms | **2.90 – 3.31 ms** |

Every row within noise, slightly high in the direction load predicts. The 1.2 ms gap at
1000-of-1000 is back, and so is the 100-cheaper-than-1000 inversion that `AUTO_INDEX_LIMIT`
predicts — **neither of which an empty vault can produce**, which is independent evidence that the
harness was working when M9 was measured and broke afterwards.

### The diagnosis was available and discarded

This is the part that generalises. `memory index` does not fail silently. It printed, on **every
one of the thousand notes**:

```
1000 scanned · 0 indexed · 0 unchanged · 0 pruned
  ? projects/bench/2026-08-01-note-00000.md — frontmatter key `project` is read by nothing
```

Both harnesses called it as `subprocess.run(..., capture_output=True)` and never looked. The
system knew. `amb memory status --json` on that vault reports `"on_disk": 1000, "indexed": 0,
"drifted": true` — three fields that each say it outright.

**A silence is not always the absence of a signal. Sometimes it is a caller discarding one.** The
project's catalogue is full of mechanisms that failed to *produce* a diagnosis; this is the first
where the diagnosis existed, was correct, was emitted a thousand times, and was thrown away by the
line that asked for it.

### And the cause was a negative decision's blast radius

D81 said the vault is regenerable. That is **true of the vault** and was not a claim about
fixtures — nobody regenerates a benchmark fixture. The note format lived in two files, neither
next to the parser, so one decision voided two harnesses and there was nothing to notice.

Both are now built through `bench/_harness.py`: one `note()` writer, and an `index_or_die()` that
exits 1 with the indexer's own diagnosis unless every note lands. Verified by mutation — change
`scope:` back to `project:` and both scripts exit 1 naming the key; restore it and both exit 0.

`bench_attribution.py` additionally asserts its **independent variable**: that the hit path
injects and the miss path does not. Asserting the variable the experiment manipulates is stronger
than asserting the vault exists, and it is still coverage rather than a value.

### The repaired instrument immediately found something

Worth recording because it is the argument for repairing rather than deleting, stated as an
outcome rather than a principle. The first honest run after the fix showed M9's timing rows
reproducing and its **token** rows not: the three `SessionStart` figures are ~377 characters
higher than published, a constant, while `PreToolUse` reproduces at 1,076 against 1,092.

The cause is three blocks added to the primer on 2026-08-28 — D60's containment framing, the
`--same-as` block, the accurate-zero line — none of which is wrong, and all of which are paid on
**every injection in every session on the machine**. D43's cap bounds the note list; nothing
bounds the preamble. That is a live cost this project had no way to see for a day, and the
harness that could see it was the one printing tables about an empty vault.

An instrument that has been void for a day is not merely untrusted — it is a day of findings
nobody got.

### One more in the same file

`bench_startup.py`'s module docstring still read *"Add the real binary once it exists:"* followed
by a `./target/release/amb` snippet — the instruction M15 had already acted on, naming the path
M15 had already established does not exist here. Fixing one instance trains attention on the thing
fixed rather than on its siblings.

### Reproduce

```bash
tools/bench.sh          # all four; each now fails loudly rather than measuring nothing
```

---

## M19 · The same instrument on three more modules: 140 of 184

**2026-08-29.** M17 mutation-tested `messages.rs` because it holds `select()`. This is
`claims.rs`, `doctor.rs` and `identity.rs`, chosen by a cheap prior: an inventory of every
`to_json` in the library, separating keys whose value is a **computed** expression from keys that
are a plain field read. A field copy needs no assertion; a computed one is a decision, and it is
also the only kind `cargo-mutants` can mutate. 21 computed keys were asserted nowhere, and they
clustered in those three files.

The prior held. `Claim::remaining` — the `expires_in_secs` key — and `Report::worst`,
`AgentRow::appears_alive` and `AgentRow::ref` were all among the survivors.

**Method.** `tools/mutants.sh src/claims.rs src/doctor.rs src/identity.rs`, which forces a private
`CARGO_TARGET_DIR` for M17's reason.

### Results

| | mutants |
|---|---:|
| generated | 204 |
| unviable | 19 |
| **viable** | **184** |
| caught | 140 |
| **missed** | **44** |

**76%**, against `messages.rs`'s 83% before its fixes. Do not read either against a published
industry figure: a mutation score's denominator is *"viable mutants this tool's operators happened
to generate"*, which is not comparable across tools or codebases. It is question 1 of the ratio
rule. What the number is for is this module, before and after.

### What the 44 were

| cluster | mutants | what it deletes |
|---|---:|---|
| `claims::my_conflicts` | 4 | every overlap reported, once, across holders |
| `claims::is_live` + `remaining` | 6 | a lease's deadline and its countdown |
| `claims::take` | 1 | `at + ttl` → `at * ttl`: **no claim ever lapses** |
| `claims::summarise` | 1 | declared claims are shown as written, never grouped |
| `doctor::schema_check` | 5 | the check for the condition that has recurred five times |
| `doctor::freshness_check` | 5 | three boundaries, none asserted on either side |
| `doctor::Health` + `Report::to_json` | 5 | the machine surface a script reads for health |
| `identity::session_pid` | 6 | the pid rule D93's addressing half rests on — 3 in the rule, 3 in the shell |
| `identity::is_unique_violation` | 4 | only a constraint violation is a name clash |
| `identity::is_alive`, `to_json`, `list` | 3 | liveness, and the roster surface |
| remainder | 4 | `resolve`'s blank-project guard, `collisions`, one arithmetic |

`claims::take` is the one to read twice. `at * ttl` against a unix timestamp puts expiry roughly
three thousand years out, so **every claim becomes permanent** — and claims are advisory (D5), so
nothing fails. The board simply stops forgetting.

`identity::session_pid` returning `Some(0)` or `Some(-1)` unconditionally both survived. `kill(0,
sig)` addresses the caller's whole process group and `kill(-1, sig)` addresses every process the
caller may signal, so either reports **every peer permanently alive** — the liveness oracle D21
was written to remove, reintroduced by a constant.

### The finding is a fourth variant of the decorative assertion

`a_board_newer_than_the_binary_is_bad_and_older_is_routine` already existed, and asserted
`schema_check` across all four of its cases:

```rust
assert_eq!(schema_check(Some(9), 8).health, Health::Bad);
assert_eq!(schema_check(Some(7), 8).health, Health::Ok);
assert_eq!(schema_check(Some(8), 8).health, Health::Ok);
assert_eq!(schema_check(None,    8).health, Health::Ok);
```

Every input is right. The fixture *does* reach the branch. **The assertion reads a field the
branch does not change** — three of the four arms report `Ok` and differ only in their `detail`,
so all five mutants on the `v < binary` guard survived a test named for that guard.

Set beside the other three, the family is now:

| | what fails |
|---|---|
| D51 | nothing runs the mutated code |
| D90 | the assertion is against a different caller than the one that matters |
| M17 (`nearest`) | the fixture never reaches the branch — a filter drops it first |
| **M19 (`schema_check`)** | **the fixture reaches the branch; the assertion reads a field it does not change** |

Only the last two are catchable by mutation testing, and only by running it — reading the test
proves nothing in either case, because in both the test looks exactly right.

### After: 140 → 181 → 186 of 193, and two corrections to this section

**Every number in this heading was run.** Four more mutants have been closed since the last of
them, each verified individually against the exact mutant that named it — but no total is quoted
for that state, because none has been measured, and this section has already been wrong twice for
quoting one that had not.

First re-run, on the committed tree: **181 of 193 viable caught, 12 missed — 94%**, from 140 of
184.

**The first version of this paragraph said four survivors remained. It was twelve, and the
mistake is worth keeping.** It came from writing the count from the *fixes intended* rather than
from the re-run, which is the same move as quoting a benchmark without running it — the error
this document exists to catch, made inside the document. Measured, the twelve were:

| still missed | why |
|---|---|
| `claims::covers` `>` → `>=` | **equivalent mutant** — see below |
| `claims::my_conflicts` (1 of 4) | my fixture killed three; the fourth needs two holders of *one path* |
| `identity::session_pid` ×3 | the extraction guarded the rule, not the shell around it |
| `identity::list` | intended and not written |
| `identity::is_unique_violation` ×3 | the match guards at `register` and `reclaim` |
| `identity::resolve` | the blank-`AMB_PROJECT` guard |
| `identity` `15.0 * 60.0` → `+` | the constant's *value* is pinned nowhere |
| `identity::collisions` | its e2e fixture has one project, so grouping cannot be seen |

Two of those are this document's own subject arriving again. `my_conflicts` was a fixture that
could not reach the fourth branch — three conflicts over two holders kills three of the four
dedup mutants and no more; the fourth compares *agents* on a matching path, so it needs two
holders of the same path, which advisory claims make ordinary (`PRIMARY KEY (path, agent)`, D5).
And `session_pid`: extracting `pid_from_socket` made the **rule** testable and left the **shell**
exactly as unguarded as before, because every wrong answer there degrades to `last_seen` recency —
and in a test everything is recent, so everybody reads alive. Only a *dead* pid discriminates.

Five more closed, each verified against the mutant that named it: the `my_conflicts` fixture
widened, `list` given its test, and one e2e test that registers a session under a pid nothing is
running and asserts it reads as gone. That last one kills all three `session_pid` survivors at
once and is the only place proving `AMB_SESSION_PID` reaches `session_pid` at all — the shared
harness strips it. Measured: **186 of 193, 7 missed.**

**That paragraph originally predicted 188, and the prediction was wrong by simple arithmetic** —
twelve missed less five closed is seven, so 186. Two unmeasured numbers in one section, from the
same reflex, and the second was worse than the first because it came *after* correcting it. The
number now in the table was run.

Three more closed after that, each hand-verified the same way:

- **`resolve`'s blank-`AMB_PROJECT` guard.** An exported but empty variable becomes the project
  *name*, addressing a place no other session can type. Silent, and it would look like broadcasts
  simply not arriving.
- **`collisions`' grouping guard.** Rows arrive `ORDER BY project, cwd`; with the guard always
  true, four roots across two projects report as one project with four roots. The e2e fixture
  covering this function uses a single project name and cannot see the difference — the same
  fixture-shaped blindness this section is about, in the test that already existed for it.
- **`ASSUMED_ALIVE_FOR_SECS`.** `15.0 * 60.0` → `15.0 + 60.0` is a 75-second liveness window
  rather than fifteen minutes, and it survived because *every* assertion around it was written in
  terms of the constant itself. The fix is one absolute number: a session that spoke five minutes
  ago is still alive. The constant's docstring calls the window "generous"; that assertion is what
  makes the word mean something.

A fourth after those, and it is the one whose *comment* was the specification. `reclaim` carries:

> *"If the fallback is itself taken this returns a unique violation, the reclamation does not
> happen, and the caller reports `NameTaken` as before. Failing closed is right: the point is to
> free a name, not to start a cascade of renames."*

A described scenario, a stated outcome, and no test. With the guard forced to `false` the clash
stops being an ordinary outcome and becomes a raw SQLite error — so an agent asking for a taken
name gets an internal failure instead of D18's clash, on the one path built to fail closed. It is
now a test: a dead holder of `shared`, somebody already sitting on the name it would be renamed
to, and a newcomer asking for `shared`.

**The `true` direction of the same guard provably survives it, and that is worth stating rather
than hiding.** In this fixture the error *is* a unique violation, so widening the guard changes
nothing; killing it needs a SQLite error that is not a constraint violation, arriving at exactly
that statement. Verified both ways — `false` reddens, `true` stays green.

### What is deliberately left

**`claims::covers`'s `>` → `>=` is an equivalent mutant.** `child.len() > parent.len() && …
child.as_bytes().get(parent.len()) == Some(&b'/')` — the trailing clause already excludes
equality, so both spellings behave identically. CLAUDE.md says to check whether a mutation was
mistargeted before concluding a test is weak; this is that, and a test for it would prove nothing.

**Two `is_unique_violation` match guards remain**, one in `reclaim` and one in `register`, both in
the `true` direction. Each needs a SQLite error that is *not* a constraint violation to arrive at
one exact statement — a board that fails mid-transaction for an unrelated reason. The predicate
itself is guarded, and so is the `false` direction of `reclaim`'s. Named here rather than left as
a gap somebody rediscovers.

### `claims.rs` had no connection fixture at all

Every database path in it was exercised only through `tests/claims_e2e.rs`, at process level —
the same indirect coverage M17 found in `messages.rs`, and the reason `take`'s expiry arithmetic
and the whole of `my_conflicts` were unguarded. It has one now.

### Reproduce

```bash
tools/mutants.sh src/claims.rs src/doctor.rs src/identity.rs
```


---

## M20 · `tests/concurrency.rs` did not have M16's defect. It had the other one

**2026-08-29.** The question asked was whether the concurrency suite repeats `bench_queue.py`'s
failure — a fixture whose constraints quietly exclude a case the product exists to provide. **It
does not, and asking produced a larger finding anyway.**

### The answer to the question as asked: no

M16's defect was *unrepresentability*. `bench_queue.py` declared `to_proj TEXT NOT NULL` and built
its own schema, so the global-broadcast cell could not exist and no fixture could have reached it.
`tests/concurrency.rs` does not build a schema — it spawns the shipped binary, which runs the
shipped DDL. All four of D17's cells are representable here, and one of them, `@@`, is asserted
end to end in `tests/delivery.rs::a_global_broadcast_reaches_every_project` against three projects.

**The distinction decides the repair.** A schema that excludes a case is fixed in the schema; a
fixture that skips one is fixed in the test file. They are indistinguishable from the test report,
which is why the question was worth asking rather than answering from memory.

### The fixture did skip one, and the probe is one character

`concurrent_broadcasts_reach_every_recipient_exactly_once` registers three readers, all in `nest`,
and sends `@`. Under that fixture `@` and `@@` are the same message. **Changing its address to
`@@` leaves it green** — run, not reasoned:

```
test concurrent_broadcasts_reach_every_recipient_exactly_once ... ok
```

So the test's name says *broadcasts* and its fixture cannot tell the two broadcast modes apart.
That is M17's `nearest` shape — a fixture that never reaches the branch the name implies — and it
is now stated in the test's own doc comment rather than left for the next reader to rediscover.

### The larger finding: the rule was guarded at one layer only

Delete project scoping from the central predicate and run everything:

```sql
-- src/messages.rs, messages::select
OR (m.to_agent IS NULL AND (m.to_proj IS NULL OR m.to_proj = ?1))
--                                                ^ replaced with a tautology
```

| what asserts it | layer | caught the deletion |
|---|---|---|
| `messages::tests::one_predicate_covers_every_addressing_mode…` | unit, in-process | **yes** |
| `tests/delivery.rs::a_project_broadcast_stays_inside_its_project` | library, own `Connection` | **yes** |
| every other suite — 137 tests across 7 files | **the shipped binary** | **no** |

**137 tests drive the real binary and not one of them reddened.** `@project` addressing a *place*
is D17's central claim and the property CLAUDE.md says no competitor has; through the executable
that ships, it was unasserted. Nothing was broken — the two guards that exist are correct and the
behaviour was right — but the coverage was one `main.rs` wiring mistake away from silent, which is
this project's whole failure mode.

**This is D90, one layer up.** D90's rule is that a guard written against one caller is not a
guard on the rule, and its arithmetic is *grep the field, count the renderers, count the
assertions*. Here the "callers" are not sibling functions but **layers** — pure predicate,
library, binary — and the count comes out 2 against 3. Counting renderers would not have found
it; counting layers does.

### After

`a_global_broadcast_crosses_projects_under_contention_and_a_project_one_does_not` puts one reader
in each of three projects and races twelve concurrent senders — four `@@` from `nest`, five `@`
from `nest`, three `@` from `mobile`. Expected counts are 9, 7 and 4, deliberately unequal so no
single wrong answer satisfies two assertions. Verified by breaking it both ways: collapsing `@@`
to `@` gives the `mobile` reader 3 against 7, and the predicate deletion above now reddens it.

410 tests, from 409.

**A note on `check_docs.py`, which fired falsely on the sentence recording this.** Its pattern is
any `N tests` in README or CLAUDE, and the new prose quotes 137 of them. The fix was to reword the
prose, not to narrow the pattern: every alternative anchor — `all N tests`, proximity to
`cargo test` — silently stops checking a number written any other way, which converts a false
positive into a decorative pass. This file already records two checks that were decorative
(M15's lineage) and none that were noisy. **A strict checker's false positive is a prose problem.**

### Reproduce

```bash
cargo test --test concurrency a_global_broadcast_crosses
```

---

## M21 · The rest of `run_memory`, and sixteen mutations run by hand

**2026-08-29.** D92 moved `memory status`'s 190 lines of rendering out of `src/main.rs`, the one
file with no tests. This is the same move applied to the nine arms that were left, and the reason
to record it as a measurement rather than a refactor is the second half: **every extracted rule was
verified by deleting it.**

`src/main.rs` 1,773 → 1,631 lines. Lib tests 250 → 270. Suite 410 → 430.

### What was extracted, and where it went

| Arm | Function | Module |
|---|---|---|
| `observe` | `render_written` | `write.rs` |
| `derive` | `render_derived` | `promote.rs` |
| `candidates` | `render_candidates` | `promote.rs` |
| `recall` | `render_recall` | `query.rs` |
| `history` | `render_history` | `index.rs` |
| `index` | `render_index` | `index.rs` |
| `export --check` | `render_export_check` | `export.rs` |
| `window` (report) | `render_window_report` | `events.rs` |
| `window` (change) | `render_window_change` | `events.rs` |

**`write.rs` and `export.rs` had no test module at all.** `write.rs` holds `observe` — the only
path in the project that authors a file into the vault. Both have one now.

**Four arms were deliberately left.** `expire`, `capture`, `export`'s write path and `promote`'s
confirmation branches have no conditional in their output: they format and print. Extracting them
would add indirection and guard nothing, and a render function with no decision in it is a place
for a future decision to hide.

### The mutations

Each rule deleted or inverted in the source, `cargo test --lib` run, the source restored.

| Mutation | Caught by |
|---|---|
| redaction line always printed | `a_redaction_is_announced_when_it_happened_and_never_otherwise` |
| `observe`: counting rule inverted | `observe_states_the_counting_rule_the_same_way_derive_does` |
| near offers suppressed | `a_near_match_is_offered_after_the_note_and_names_the_flag_that_links_it` |
| **near offers hoisted above the note** | *(the same test's ordering assertion)* |
| supersession line dropped | `a_supersession_is_named_and_says_what_it_costs_the_old_note` |
| `derive`: counting rule inverted | `a_derivation_that_did_not_count_says_so_and_names_the_reason` |
| offer appears one derivation late | `the_offer_appears_at_the_threshold_and_not_one_below_it` |
| empty candidate list prints nothing | `no_candidates_is_said_rather_than_printed_as_nothing` |
| candidate status suffix dropped | `a_retired_candidate_carries_its_status_and_an_active_one_carries_none` |
| empty recall prints nothing | `a_search_that_matched_nothing_says_so_rather_than_printing_nothing` |
| recall status suffix dropped | `recall_shows_a_retired_note_and_marks_it_as_retired` |
| paths line always printed | `the_paths_line_appears_only_when_the_note_declares_paths` |
| stands-alone sentence dropped | `a_note_with_no_lineage_says_so_instead_of_printing_nothing` |
| unreadable line always printed | `an_unreadable_note_is_reported_and_a_clean_pass_stays_quiet` |
| drift condition re-derived differently | both `export.rs` tests |
| `AlreadyOpen` reads like `Opened` | `opening_a_window_that_is_already_open_refuses_in_words_that_cannot_be_misread` |

**16 of 16.** No survivors, and the run is cheap enough to repeat: it is fifteen textual
substitutions and a sixteenth that moves a block.

### Two of these are not formatting, and that is the point

**The near-match ordering.** `render_written` emits the near-candidate offer *after* the note's own
lines, and the test asserts the byte offsets rather than the presence. Hoisting the block is the
only mutation here that a presence assertion misses, and it is the one that matters: a candidate
shown *before* the note is written is context handed to a writer still deciding, which is exactly
what `INJECTABLE` exists to prevent (D51, D86). Shown afterwards it is a linking affordance offered
to someone who has already thought. Same bytes, opposite rule.

**`export --check`'s two consumers.** The human text and `Error::ExportStale`'s exit code must
never disagree about whether anything drifted, so both read `ExportStatus::drifted()`. The
mutation replaced the render's condition with `st.current.is_empty()` — a re-derivation that is
right on most inputs and wrong when files are missing with nothing stale. `a_missing_export_is_
drift_even_when_nothing_is_stale` exists for that one input.

### The rule this run applies

**A guard that stays green when you delete it is not protecting anything.** Twenty tests passed on
their first run, which per CLAUDE.md proves nothing. The sixteen reds are the evidence; the twenty
greens were the hypothesis.

---

## M22 · The macOS-only guard could be deleted and nothing would go red

**2026-08-29.** Thirteen platform-specific sites live in `src/db.rs` and four in `src/identity.rs`,
all only ever executed on macOS here. The question asked of them was the one the `session_pid`
finding suggests: **not whether the extracted decision is right, but what the shell does when the
syscall fails, and whether any fixture reaches that path.**

`identity.rs` answers well. `is_alive` degrades to recency on any pid `kill` would misread, and
four tests cover it including an absolute sharp-edge assertion (M19).

`db.rs` did not answer at all.

### Three mutations, run against all 430 tests

| Mutation | What it means | Result |
|---|---|---|
| `volume_of` returns `None` always | the remote-volume guard is disabled outright | **survived** |
| `is_remote_volume(&fstype, None)` | macOS's `MNT_LOCAL` authority is dropped; a ten-name list decides | **survived** |
| `volume_of` returns `("nfs", Some(false))` always | every volume reads as remote | 190 reds |

**The third is not a guard, it is a canary.** Everything reddened because no board could open
anywhere, and every test in the project needs one. Nothing asserted the rule; a great many things
happened to need the rule not to fire.

**The second is the one that matters**, because it is a plausible edit rather than a contrived one.
`MNT_LOCAL` is clear for every remote filesystem whatever it is called, which is precisely why the
flag beats the name — and `NETWORK_FSTYPES` is deliberately short (D28), so `webdav` is not on it
and never will be. Dropping `mnt_local` silently reverts the two-authority design to the guess it
was built to replace, and 430 tests had nothing to say about it.

### Why the existing split did not catch it

`is_remote_volume` was extracted with a docstring saying, correctly, that *"the syscall half cannot
be tested — this project cannot mount NFS in a unit test"*. It is thoroughly tested. **The gap was
that "the decision" had been drawn too small:** the marker check, the precedence between the two
refusals, and whether the kernel's answer is consulted at all all stayed on the shell side of the
line, reachable only through a real mount.

This is the same shape as D90 and as M20 — a guard that is real, and a rule that is larger than the
guard. The check that finds it is not reading the code, which looks right, but **mutating the part
that was left in the shell and seeing whether anything notices.**

### After

`db::location_verdict(as_written, resolved, volume)` takes both answers as data and returns the
same `Result`. `guard_location` resolves the path, calls `volume_of`, and delegates. Five tests:

- the kernel's verdict outranks the name list **in both directions** — `webdav` with `MNT_LOCAL`
  clear is refused, `smbfs` with it set is allowed
- no answer permits the board, from both silences: no ancestor exists, and the kernel failed
- a synced marker is reported in preference to the filesystem under it
- a marker found only after resolving still refuses, and the error quotes what the user typed
- `statfs` on a nonexistent path is `None` and on `/` is `Some` — the shell's own failure mode,
  gated to the platforms that have a syscall to fail

Mutations A and B now redden. So does a third, reversing the marker precedence.

435 tests, from 430.

**What is still not covered, and needs the remote.** No test here executes the Linux `volume_of` or
`fstype_name`, and none executes the `not(macos|linux)` stub. Those need CI on another platform;
the decision they feed is now fully covered, and the shells are four lines each.

## M23 · Mutation-testing the two modules that produce the window's numbers

**2026-08-29.** `tools/mutants.sh src/memory/inject.rs src/memory/events.rs`, in that order of
importance: these two produce the numbers D59 will retire or keep the injection layer on, and a
survivor here is a bug in the instrument while the instrument is being read.

**110 mutants in ~4h: 85 caught, 15 missed, 9 unviable, 1 timeout.** The ratio is a byproduct and
not comparable with anything — its denominator is "viable mutants this tool's operators happened
to generate". **The artifact is the sentence per survivor below.**

### The sixteen, each with why it lived

| Mutant | Why it survived |
|---|---|
| `events.rs:147:31` `&&`→`\|\|` | `lane_caveat`'s two fixtures set both lanes zero, or both non-zero. Only the diagonal corners. |
| `events.rs:147:26` `==`→`!=` (first) | Same fixture gap; the mixed corner was never built. |
| `events.rs:147:53` `==`→`!=` (second) | **Equivalent.** See below. |
| `events.rs:257:9` → `String::new()` | `crossed_note` had no assertion at all. D91 moved the cross-repo verdict onto the event and the move was never pinned. |
| `events.rs:257:9` → `"xyzzy"` | Same. |
| `events.rs:342:38` `+`→`-` | `verdict`'s sample floor sums both lanes; the shared fixture pins `injected_file` to 0, so `+` and `-` agree. |
| `events.rs:369:24` `+`→`-` | `ratio`'s *numerator* sums both lanes. Denominator mutants died; the numerator's did not, for the same zeroed-lane reason. |
| `events.rs:376:9` → `1.0` | `session_ratio` was never pinned to a value, only to being computed. |
| `events.rs:386:9` → `0.0` | Same for `file_ratio`. |
| `events.rs:584:29` `>`→`>=` | `by_force`'s filter drops forces with no events; the only assertion was `contains("rule")`. |
| `events.rs:584:42` `>`→`==` | Same filter, second clause. |
| `events.rs:584:42` `>`→`>=` | Same. |
| `events.rs:584:42` `>`→`<` | **Timed out, so unmeasured.** See below. |
| `inject.rs:85:13` delete `Ok(Scope::Global)` | A global note falls to `Foreign`: ranks last instead of third and is captioned "other project, advisory". `Nearness` had no *direct* assertion; it was exercised only through renderers, and only on the Local/Foreign axis. |
| `inject.rs:167:12` delete `!` | Nothing asserted a note renders the paths it concerns. |
| `inject.rs:188:15` `>`→`>=` | Always true on a `usize`: every injection would end "…and 0 more". The cap's admission was asserted when it fired and never when it should not. |

### One dominant cause, and it is the fixture

Four of the sixteen are the same defect: `receipt()` in the test module zeroes `injected_file` and
`cited_after_file`. Zero is the additive identity, so `x + <file field>` is indistinguishable from
`x - <file field>` in every fixture the module had; `+`→`*` dies in the same expressions because
`x * 0` is not `x`, which is why those mutants were caught and these were not.

**The zeroed lane is `PreToolUse`** — the lane D42 had to correct for being left out of a
denominator, and the one D74's caveat exists to stop being misread. The fixture reproduced, as a
convenient default, the omission the design has twice had to fix in production.

### The equivalent mutant, and why "equivalent" is itself a claim

`injected == 0 && injected_file != 0` never changes the answer, but **only relative to an
invariant**: `injected` is `count(*)` and `recency_sessions` is `count(DISTINCT session)` over the
same rows under the same predicate, so one is zero exactly when the other is, and the mutant always
falls through to a second guard that returns `None` anyway. Killing it needs a receipt with
`injected == 0` and `recency_sessions > 0`, which no query can return — and contriving one would
make the number better and the suite worse.

That invariant lives in two SQL strings and nowhere else. Change either `WHERE` and the word
"equivalent" becomes false with nothing going red. **A `missed.txt` entry is a claim about the
code, and this project pins its claims**, so `a_lane_with_no_injections_has_no_sessions_either`
asserts the zero-agreement in both directions. The same reading shows the guard's first clause is
*redundant* for every reachable receipt — which is why all three of its mutants lived — while two
of the three still change behaviour on reachable input and are killed.

### The timeout was a result, not noise

`584:42 → <` ran 579s and hit the ceiling, so it was never measured. Read as an unpaid debt rather
than as noise, it found a second gap: `cited < 0` is always false on a `usize`, so the mutant
reduces to `injected > 0`, and the first fixture written for that filter — one force with
injections — would not have killed it. The `|| cited > 0` clause **is** reachable: the `cited`
query filters on `force` while its `EXISTS` does not, so a note injected as `advice` and cited
after its force became `rule` counts `rule` as 0/1. The fixture now stages exactly that, and one
row pair kills all four mutants on the line.

**A test that kills three of four mutants on one line looks finished.**

### Two rules for the catalogue

1. **A positive assertion cannot guard a filter whose job is an omission.** `by_force` and
   `render_hidden` are the same defect in one run, in two modules — `contains("rule")` and "the cap
   said how many it hid" are both true whichever way the filter goes. Every exclusion needs an
   assertion of **absence**, and "assert the positive explicitly" is the advice that stops you
   writing one.
2. **Containment belongs to the field, not to the caller.** Recorded in `CLAUDE.md` against D90;
   the fix is deferred because it changes what a note renders and the measurement window is open.

### Conditions, stated because they were not clean

The machine was not quiet: load average 12 over fifteen minutes, from other sessions' running
binaries. There was **no target-directory collision** — mutants had their private directory and the
competing processes were other packages — so this is not M17 repeating. It is CPU contention, which
stretches wall time and manufactures timeouts. `CAUGHT` and `MISSED` verdicts survive that;
`TIMEOUT` does not, and that one mutant was replayed by hand.

### After

**Fifteen of the sixteen replayed by hand against the tests written for them, each confirmed red
before the test was kept. The sixteenth survives, as predicted.** 449 tests, from 435.

**What this did not cover.** `promote.rs` is still unmutated — the pipeline whose only pressure has
ever been mutation, and the next module to take.

## M24 · The measurement window had been open ten hours and collected nothing

**2026-08-29.** `amb memory status` printed `too early — needs 30 more session(s)`. The question
was whether that is a slow accumulation or a stalled one, and the two print identically.

Measured from three sources, none of which writes to the board: a copy of `board.db`, Claude
Code's own transcript files, and the `note_events` ledger.

### Sessions per day, and why the obvious source is the wrong one

| Source | Aug 27 | Aug 28 | Aug 29 |
|---|---|---|---|
| Board registrations (`agents.first_seen`) | 9 | 8 | 1 |
| Transcripts **created** | 3 | 0 | 0 |
| Sessions producing an injection | 0 | 3 | 1 |

**The roster reads like activity and is not.** `agents.first_seen` records when a session first
reached `amb`, which for a *resumed* session is the day it resumed. Transcript birth times settle
it: **no new session has started on this machine in two days, while sixteen were active today.**
Sessions here are resumed, not started.

**Three sessions have ever produced a note event**, one of them `probe-drop` — the hand-run probe
D87 already discounts. The entire receipt rests on two real sessions.

### Why the window collected zero, which is structural rather than slow

```sql
PRIMARY KEY (session, kind, scope, slug, event)
```

A session injected before the window opened **writes no row when it is re-injected**, so it can
never enter that window. The session writing this measured itself: 26 rows, last at 00:28, window
opened at 11:59, re-injected since — its banner was in context throughout — and it recorded
nothing.

**This is D77's finding arriving on a third question.** The key answers *did this happen*.
`CLAUDE.md` already records that it mis-answers *how often*. The window asks *did this happen
inside these dates*, and the key mis-answers that one **permanently rather than inaccurately**.
Three questions, one table, one key silently choosing which is askable.

The precedent for the fix already exists and was built for exactly this reason: `searches` is
`INTEGER PRIMARY KEY`, deduplicating nothing, because a windowed question needs a table that can
count repeats. **The answer is a different record, not a different divisor.**

### The consequence, stated rather than implied

D59 needs 30 sessions and 50 injections. Arrivals run at roughly one a day and were zero today.
**The withdrawal condition cannot fire under this machine's usage pattern, so the injection layer
currently has no live withdrawal condition — only one that looks live.** That is a fourth state
alongside working, not working, and not running, and `Verdict` could not express it.

### What shipped, and what deliberately did not

`Receipt::arrival_note` prints above the verdict: at zero, that the floor is *unreachable rather
than unreached*; below it, the arrival count against the floor. Silent over all time and once the
floor is met. Asserted at both layers (D90), and each of its four guards deleted and watched go
red.

**The floor was not lowered.** Fitting a threshold to data that will not reach it is what D87 made
`--open` non-idempotent to make expensive. **The denominator was not switched to injections
either** — injections inherit the same key, so that relocates the problem rather than escaping it.

## M25 · The promotion pipeline was tested for what it prints, not for what it does

> **Re-run 2026-08-31 against the tests `b75d150` added: 47 mutants, 40 caught, 7 unviable,
> and 0 missed.** 24 of 40 viable becomes **40 of 40**. Same mutant count and same unviable count,
> so the comparison is like-for-like rather than two different populations — which is the only form
> in which a mutation score means anything (`tools/mutants.sh` says so in its own last paragraph).
> **All sixteen survivors were real and all sixteen are closed.**
>
> Conditions recorded with the result, because M27's residual hole is that the timeout ceiling is
> measured once at the start: baseline 5 s build + 11 s test, `Auto-set test timeout to 120s` — the
> `--minimum-test-timeout` floor, which is what a *quiet* baseline produces — and **no TIMEOUT
> rows**, so there are no unanswered questions folded into the count. Load average 6.65 at the
> start and 7.76 at the end, with zero other `cargo` processes for the whole run.
>
> This was the confirming pass the original run never got. It is worth noting that it was
> outstanding for a day while a plan described the whole item as not yet started: the run had
> happened, the fixes had landed, and the *evidence that the fixes worked* was the part missing.

**2026-08-30.** `tools/mutants.sh src/memory/promote.rs`, the third module of the hardening item
and the one whose pipeline **has never run once** — mutation is the only pressure it has ever been
under, and the survivor rate says so.

**47 mutants in ~5 minutes: 24 caught, 16 missed, 7 unviable, 0 timeout.** 40% of viable mutants
survived, against `events.rs`'s 15% (M23).

### The survivors are not scattered. Fourteen of sixteen sit in two functions

| Function | Survivors | What it does |
|---|---|---|
| `expire_candidates` | 9 | retires a candidate nobody rediscovered within the TTL |
| `ready_candidates` | 4 | the same TTL rule, deciding what gets offered |
| `candidates_concerning` | 1 | dedups candidates matching an edited path |
| `stem_of` | 2 | filename → slug, the identity a note is parsed under |

And the fifteen tests that existed cover `render_derived`, `render_candidates`, `render_offer` and
`destination` — **every one of them pure.** The module was tested for what it *prints* and never
for what it *does*. `expire_candidates` could `return Ok(0)` unconditionally and nothing reddened,
because nothing had ever called it.

**This is M20's arithmetic on a module rather than on a rule.** Count the layers a module has and
count the layers asserted: the pure core had thirteen tests and the shell that reads the vault and
writes it back had none. The layer to suspect first is the outermost, because the inner one is
cheaper to test and is therefore the one that exists.

### The catalogued omission rule predicted this before the run finished

Two of the first survivors were filters whose job is to *not* emit: the dedup in
`candidates_concerning`, and the TTL skip in `ready_candidates`. That rule entered `CLAUDE.md`
yesterday off `by_force` and `render_hidden` (M23), and it is now predicting survivors in a module
it was not derived from — which is the difference between a rule and a description of two bugs.

### One number the fixture had to separate

`CANDIDATE_TTL_DAYS * 86_400.0` mutated to `+` is a threshold of about **one day** rather than
thirty. A fixture whose "live" candidate was an hour old would satisfy the mutant as happily as the
real code. The live candidate is **ten days** old, which sits between the two thresholds and is the
only reason that mutant dies.

### And the fix reproduced a catalogued defect on the first attempt

The boundary test was named `a_candidate_exactly_at_its_ttl_is_alive_in_both_halves_of_the_rule`,
carried one derivation, and called only `expire_candidates`. It passed, and `>` → `>=` in
`ready_candidates` **survived it** — that function's SQL filters `derived_count >= threshold()`, so
a one-derivation candidate never reaches the comparison the test claimed to be about. A filter
upstream of the thing under test, exactly where M17 says to look. Three derivations and both calls,
and it kills.

**Caught only because every survivor was replayed by hand.** The suite was green either way.

### After

**Sixteen of sixteen replayed and confirmed red. No equivalents.** 457 tests, from 452.

### On the harness

Two things this run measured that the last one could not:

- **The private target directory can go stale.** The baseline failed in 6s with
  `couldn't read .../libsqlite3-sys-*/out/bindgen.rs`. The directory is kept between runs on
  purpose — a cold build is ~90s and the incremental rebuild is most of what makes mutation
  affordable — and that is the cost. It failed loudly (`cargo build failed in an unmutated tree, so
  no mutants were tested`) rather than reporting a number, which is the behaviour that matters.
  Clearing it (1.8 GB) fixed it.
- **A quiet machine is worth far more than expected.** Baseline under load 12: 20s build + **192s**
  test. Under load 4.8: 8s build + **5s** test. A 38× difference in test time, so the previous run
  spent nearly all its wall time on contention. It also means `--timeout-multiplier 3` computes a
  15s ceiling here and `--minimum-test-timeout 120` is what actually holds the floor — both halves
  of the M23 fix are load-bearing, which was not the expectation.

## M26 · Validating M25 found D87's defect committed by the session that had just read D87

**2026-08-30.** A deep validation pass over the four commits of 2026-08-29/30. Three results, one
of them a defect introduced by this work.

### The independent re-run: 16 of 16, verified by the tool rather than by hand

M25's survivors were replayed with a hand-written script that **transcribed each mutation from
`missed.txt`**. A mis-transcription would have "verified" a mutation that never ran, so the pass
was repeated with `cargo mutants` generating them itself:

```
47 mutants tested in 3m: 40 caught, 7 unviable
```

Caught 24 → 40, missed 16 → **0**. The hand replay was faithful. **The check is worth keeping as a
habit**: a replay script is a claim about what survived, and it is written by the same reader who
might have misread the list.

### The defect: `arrival_note` reached a person and never a machine

D95 added `Receipt::arrival_note` above the verdict, and put it on the human surface **only**.
`amb memory status --json` emitted:

```json
{ "verdict": "too_early", "receipt": { "sessions": 0, "lane_caveat": null } }
```

A machine reads *not enough evidence yet*. Nothing in the document says the window cannot fill —
which is the entire content of D95, absent from the surface most likely to be parsed rather than
read.

**This is D87's defect exactly, on the other half of the same command, committed by the session
that had just read D87.** D87 exists because `--json` returned before the `counting over …` line
was built, and its correction is quoted in the field it added: *"Both surfaces answer the same
question or neither does."* That sentence sits two lines above where the omission was made.

### And the neighbour it should have been paired with was already unguarded

`lane_caveat` **was** on both surfaces — and nothing asserted the JSON half. Deleting
`"lane_caveat": self.lane_caveat()` from `Receipt::to_json` reddened **nothing in 457 tests**.
D74's caveat, the one that stops two incomparable ratios being read as a comparison, could vanish
from the machine surface silently.

So the count was: one rule, two renderers, and on the machine side **zero** assertions — D90's
arithmetic, on a field added specifically to prevent a misreading.

### The fix is structural, not two more lines

`Receipt::to_json` now **takes the window**, so a surface cannot emit one caveat and omit the other
by construction, and `every_caveat_reaches_the_human_surface_and_the_machine_one` enumerates both
caveats against both surfaces — the `delivery::UNTRUSTED` pattern, which this project already built
for this shape. Asserted against the JSON *value* rather than a substring of the serialised
document, so an escaping change cannot quietly turn it into a test of nothing. Five deletions
replayed, five red.

The residual hole is named in the test, as `UNTRUSTED`'s is: a third caveat added without being
listed there stays silent.

### The whole-shape scan came back clean

M24's wrapped-literal defect was searched for across all of `src/`: three hits, all false positives
(deliberate indentation after `\n`, and one fixture that tests whitespace trimming). The instance
that shipped was the only one.

### What is still unmutated, ranked

| Module | prod LOC | tests | why it matters |
|---|---|---|---|
| `memory/status.rs` | 811 | 20 | renders the receipt D59 retires the layer on; the largest never-mutated module |
| `memory/redact.rs` | 289 | 13 | **a security surface** (D46) — a redaction that silently stops working writes secrets into the vault |
| `memory/note.rs` | 354 | 6 | the frontmatter round-trip, where the writer/reader asymmetry behind the deferred `quoted()` fix lives |

`redact.rs` is the one to take first despite being smallest: it is the only one of the three whose
failure mode is a leak rather than a wrong number, and this project's failures are silences.

458 tests, from 457.

## M27 · Four modules, and the recurring defect is a guard over a derived count

**2026-08-30.** The three modules M26 ranked as unmutated, run back to back on a quiet machine
through `tools/mutants.sh`. M26 predicted `redact.rs` should go first *"despite being smallest: it
is the only one of the three whose failure mode is a leak rather than a wrong number."* That
prediction was correct — two of its survivors leak.

**The first reading of this was wrong, and `delivery.rs` is what refuted it.** Three modules
suggested the score tracked *what the module produces* — parser, transformer, renderer — with the
renderer worst. So `delivery.rs` was run next, as the finding's own prediction: a renderer that had
never been mutated, holding `render_all`, the banner every session on this machine reads.

| Module | prod LOC | viable | caught | missed | score |
|---|---|---|---|---|---|
| `memory/note.rs` | 354 | 24 | 23 | 1 | 96% |
| `delivery.rs` | 414 | 34 | 30 | **4** | 88% |
| `memory/redact.rs` | 289 | 86 | 68 | 18 | 79% |
| `memory/status.rs` | 811 | 92 | 52 | 40 | 57% |

**A renderer scoring 88% is a counterexample to the claim, and the corrected finding is better than
the one it replaces.** What separates `delivery.rs` from `status.rs` is not what they produce. It
is that `delivery.rs` already had two empty-case tests — `nothing_to_say_renders_nothing` and
`no_mail_and_no_conflict_still_renders_nothing` — and `status.rs` had none.

So the rule is **not** "renderers are unguardable". It is:

1. **The recurring defect is a guard over a count, and it now has three instances in three
   renderers.** `n > 0` has a relaxation, `n >= 0`, that a presence-only suite cannot see —
   `status.rs` (ten of them), `delivery.rs`'s `hidden > 0`, and `inject.rs`'s `render_hidden`,
   fixed in M23 and its sibling here left standing.
2. **A boolean guard does not have that relaxation.** `!xs.is_empty()` can only be inverted, which
   changes the answer in *both* directions at once, so any presence test kills it. The spelling of
   the guard decides how much test effort it needs — which is most of why `delivery.rs`, whose
   guards are `is_empty()` calls, scores as it does.
3. **An empty fixture catches only the guards an empty input reaches.** This is the part the
   three-module reading would have got wrong in practice, and `hidden > 0` is the proof:
   `delivery.rs` has two empty-case tests and neither touches it, because `hidden` is
   `ordered.len() - shown` and is only interesting when mail is *present and under the cap*.
   **A guard over a derived count needs a fixture populated in everything except the quantity it
   guards** — the middle state, neither empty nor triggering.

### `redact.rs`: the useful question is not which survived but which *leak*

Eighteen survivors, and counting them says nothing. D46 states the filter is *"deliberately biased
toward over-redacting"*, so a mutant that redacts more is moving in the direction the module was
designed to fail in. Split by direction:

| Direction | Survivors | Verdict |
|---|---|---|
| **Leaks a credential** | 4 | two defect sites, both tested and killed |
| **Silently under-reports a removal** | 1 | tested and killed |
| Over-redacts | 9 | the designed-safe direction; not chased |
| Near-equivalent (a trim feeding `contains`) | 4 | not chased |

**The two leaks, both in `redact_token` and both invisible to every existing fixture.**

- `core`'s trim predicate (`240:77`, `240:89`). It exists so `SECRET_PREFIXES` — D46's named
  shapes, the *primary* signal — matches a token that arrived wrapped in quotes, brackets or a
  trailing comma. Stop trimming quotes and `core.starts_with("sk-")` fails on **every** prefix in
  the list. All thirteen existing tests feed the *bare* token, which is the shape a secret has in a
  code sample and not the shape it has in the paste that produced the note.
- The length floor (`241:19`, two mutants). `core.len() < 8` runs *before* the prefix check, so it
  gates a rule with no length premise of its own. Both mutants drop an exactly-eight-character
  credential and return `None`.

**The under-report is the more interesting one, because its harm is in another file.**
`strip_pem`'s `*removed += 1` mutated to `*=` still removes the private key block — and leaves the
counter at zero. Follow the value: `Redacted.removed` → `Written.redacted` → `write.rs`'s
`if w.redacted > 0`, which suppresses the line **"N value(s) redacted before writing"** entirely.
So a note whose only secret was a private key reports that nothing was redacted. `write.rs`'s own
comment forbids exactly this: *"a redaction the author cannot see is one they cannot correct, and
they are still in the session that wrote it."*

**Read inside `redact.rs`, `*removed += 1` is bookkeeping.** Its contract — that it is the trigger
for a safety announcement — is established one module away, and neither module's tests could see
across the seam. This is the inverse of the recurring defect `find_unread_fields.py` was built for:
that tool finds a field with *no* reader (D23, D39, D45), and this is a field whose **reader is
what makes it load-bearing**. A field's importance is set by its reader, not by its writer, and
auditing the writer alone clears it.

**The thirteen remaining survivors are deliberately not chased, and that is a claim, not a
shrug.** Killing them means asserting *"this specific benign string is NOT redacted"*, which
encodes a false-negative-friendly rule into the suite of a security surface. Two tests already do
that job against real inputs — `ordinary_prose_and_identifiers_survive` and
`the_vocabulary_this_project_actually_uses_is_left_alone`, every string in them taken from this
repository's own documents. And their survival **is** the check: those two tests are in the suite
each of these mutants passed, so it is already established that no string this project actually
writes distinguishes them. `is_high_entropy`'s `||` → `&&`, for instance, lets a short opaque
token reach the entropy rule — and every vocabulary string then fails a *different* later clause,
the uuid and the note slug for want of an uppercase letter, the crate version and the paths on the
character set. Near-equivalent relative to this project's corpus, which is the only denominator
available.

### `status.rs`: thirty-seven of forty survivors sit on a print-guard

Not on the arithmetic — on the `if` that decides whether a line is **rendered at all**. Ten are
literally `x > 0` relaxed to `x >= 0`; the rest are the other operators in the same conditions
(`||` -> `&&`, `==` -> `!=`, and the two sums in the stopping rule). The remaining three are the
per-force ratio, an unasserted rendered *value*. Nothing was red, because the only render fixture
(`filled()`) has numbers in it and every assertion is a `contains`.

| Source line | Survivors | The guard |
|---|---|---|
| 310 | 9 | `injected + injected_file > 0 && cited + cited_after_file == 0` — the stopping rule |
| 173 | 7 | `injected > 0 \|\| injected_file > 0` — whether the lane split prints |
| 283 | 7 | `export_checks > 0 \|\| export_failures > 0` — phase 3 |
| 249, 255, 267, 295 | 3 each | phase 2's block, its suppression line, D49's reflex warning, unprompted |
| 204, 207 | 3 | the per-force ratio itself — a printed number nothing asserted |
| 142, 777 | 1 each | D62's loss line; the truncation remainder |

This is the omission rule from M23 — *a positive assertion cannot guard a filter whose job is an
omission* — but M23 found it in **one** filter and generalised cautiously. Here it is the dominant
defect class of an entire module, and the module's mutation score is very nearly a direct measure
of how many print-guards are unasserted.

**Severity is set by what the page is for.** This one is read by a person deciding whether to
withdraw a feature (D59). The mutants print, on a board where none of it is true:

- `! 0 note(s) on disk will not parse … so that content is gone` — D62's loss line, on a healthy
  vault
- `phase 2: 0 candidate(s), 0 at the threshold …` — a phase that has never run
- `as advice : 5/57 · 285.00` — the per-force ratio under `/` → `*`, an unasserted rendered value
- `! nothing has ever been declined — if approval has become reflex, D49 says withdraw the phase`
  — after a single approval, under `&&` → `||`
- `nothing injected has ever been cited … this feature has been answered and should be switched
  off` — on a board where nothing was ever injected at all

None of these is cosmetic. Each is a **wrong input to the decision the instrument exists to
inform**, and indistinguishable from real signal to the reader it is for.

**And this is a third way for an instrument to fail.** The catalogue so far asks whether the
*number* can answer the question put to it — whether its denominator matches, what it records on
the unhappy path, what can move it at all. M27 adds that a correct number is still **delivered on a
page**, and the page has its own failure mode. `status.rs`'s arithmetic was fine throughout; every
one of the forty survivors is in the rendering.

Six tests cover all forty, because the survivors are structural rather than scattered: an
empty-board sweep asserting *absence*, its presence-side twin, and four truth tables over the
conditions with more than one operator in them.

### `note.rs`: one survivor, and it needs the vault's premise to be reachable

`scan_frontmatter`'s `key.is_empty() || line.starts_with(char::is_whitespace)` narrowed to `&&`.
An indented `path:` under `files:` is then lifted to a top-level key, and `unknown_keys` reports it
as a key no reader consults — on a note whose every real key is known. The function's own docstring
says a warning that lies is worse than no warning, and names this as the reason there is exactly
one scanner.

`amb`'s writer never emits an indented `key: value` — list items go out as `  - x` and hit the
branch above — and **all 36 notes in the real vault were checked and are clean**. So the mutant is
unreachable through `render`. It is reachable through the thing `unknown_keys` exists for: a vault
that is *"hand-editable markdown that Obsidian and a human both write into"*.

### The needle that was too short

`an_empty_board_prints_no_line_that_implies_something_happened` failed on its first run against
correct code: the needle `"never shown"` for the conditional line
`N cite(s) of notes this session was never shown` also matches the **unconditional**
`unprompted (never shown, used anyway): 0`, which is printed at zero on purpose (D47). M24's rule
from the other side — there, `contains` could not see *between* two needles; here a needle short
enough to match two lines cannot assert either. Caught by the test itself, on a fixture built to
assert absence.

### `delivery.rs`: four survivors, and only one of them is a rendering defect

Run because the three-module reading predicted it would score badly. It did not, and its survivors
are more varied than any other module's — which is itself part of the argument against that reading.

- **`366:20`, the `write_snapshot` match guard. A D11 bypass, and the most serious defect of the
  session.** `path.parent()` on a bare filename is `Some("")`, not `None`; the guard
  `!p.as_os_str().is_empty()` sends that case to `Path::new(".")` so it resolves against the
  working directory. Every snapshot test passes an **absolute** path built from `b.cwd`, so nothing
  reached the branch — M17's shape, guarding a negative decision.

  **And the first test written for it was itself unable to see the defect**, which is the part
  worth keeping. With the repository *at* the working directory, `""` and `"."` agree: `repo_root`
  probes `dir.join(".git")`, and for an empty `dir` that is the relative path `.git`, resolved
  against the same cwd. The replay passed against a mutant known to be live. They diverge exactly
  one directory down — `canonicalize(".")` succeeds and the walk climbs to the repository root,
  while `canonicalize("")` fails, leaving `""`, whose parent is `None`, so the walk stops before it
  starts. From a subdirectory, which is the ordinary way to run the command, the mutant writes a
  snapshot inside a repository. The test now asserts both shapes and dies on the second.

  **The lesson is not "check the branch is reached" — that was done.** The fixture reached the
  branch and still could not separate the two values it distinguishes. The check is one step
  further in: *does the fixture make the guarded expression's two outcomes differ?* A guard can be
  executed and still be inert.

- **`205:19`, `hidden > 0` -> `>= 0`.** `…and 0 more — run `amb inbox` to see them all.` printed
  under a complete list, on the `SessionStart` banner. **The sibling of the `render_hidden` mutant
  M23 fixed in `inject.rs`** — same rule, same spelling, two modules, one fixed.

- **`170:12` and `156:12`, the two `if !out.is_empty()` separator guards**, three lines apart.
  Inverting either leaves the primer running into the conflict header on one line, or opens the
  banner on a blank line. `mail_and_conflicts_are_both_reported_together` asserts both blocks are
  present and cannot see the space between them — M24, on the same file that produced M24. **The
  first fix here covered one of the two and left the other**, so the replacement test enumerates
  every join and adds two whole-shape assertions — no leading newline, no triple newline — for the
  joins it does not name.

### A timeout is not a caught mutant, and on a shared machine it is not even about the mutant

The first run reported `3 missed, 30 caught, 1 unviable, 1 timeout`. The timeout was `156:12` — and
re-running that mutant by hand on a quiet machine, the **full** suite passes in 20s against a 120s
floor. It was never slow; the machine was loaded, and the same load inflated two neighbouring
mutants to 99s and 66s.

**A timeout row is an unanswered question that reads like a resolved one.** Filed as "probably
caught" it removes a live survivor from the count — here it would have hidden one of the two
separator defects and reported the module with three known problems instead of four. M25 measured
load moving the baseline 38×; this is the same contention arriving as a *wrong verdict* rather than
as a slow run. Resolve every timeout individually before reading a score.

### The double-registration defect, reproduced from the catalogue while adding one of these tests

Inserting the D11 test above `fn every_line_of_a_snapshot_body_is_quoted` placed it between that
function's `#[test]` and the function itself. The new test then carried two attributes and the
existing one none: `cargo test --test cli_e2e snapshot` printed five results with one name
**twice** and `every_line_of_a_snapshot_body_is_quoted` — a containment test — **absent**. Caught
by the arithmetic CLAUDE.md already prescribes.

**What matters is that it needed no new tool.** Clippy's `duplicated_attributes` fires on it, and
`tools/verify.sh` runs clippy with `-D warnings`, so the gate fails on this condition — verified by
introducing a duplicate deliberately and watching clippy report it. A `tools/check_test_names.py`
was drafted and **discarded on that evidence**. The reason it bit at all is that a *filtered*
`cargo test` was run instead of the gate.

### Two invariants replacing two enumerations

Both new enumerations named a residual hole, in the `delivery::UNTRUSTED` style. Both holes turned
out to be closeable, because each artefact has a marker to key on:

- `changing_the_text_always_costs_a_count` — the rule is not *"these four paths increment"* but
  **"changing the text costs a count"**, checkable over any input without knowing which path ran.
  Its one counterexample is asserted rather than described: `password=[redacted]` re-redacts to an
  identical string and still counts one, because `[redacted]` is ten characters and not all digits.
  Redaction is text-idempotent and not count-idempotent.
- The alarm-marker check in `an_empty_board_prints_no_line_that_implies_something_happened` — every
  warning `render_status` prints is prefixed `  ! `, there are eight, and on a healthy empty board
  none is true. One assertion covers all of them **including `failures > 0`, which the needle list
  never named**; that guard's own absence assertion turned out to live end-to-end, in
  `status_reports_whether_the_hook_is_actually_capturing`, at the outer layer M20 says to suspect.

Current practice agrees on the shape: property-based guidance is to assert *invariants or
symmetries* rather than reimplement the function, and to prefer structural invariants over exact
matching for rendered output. These are invariants over a fixed corpus rather than generated-input
property tests — `proptest` was considered and not adopted: D46's filter is named shapes, so random
strings are overwhelmingly benign, and a new dev-dependency during an open measurement window buys
little.

**On the score itself**, the industry convention of "above 80% is strong" is deliberately not
adopted. `tools/mutants.sh`'s header already gives the reason, and it is question 1 of the ratio
rule: the denominator is *viable mutants this tool's operators happened to generate*, not comparable
across tools, languages or codebases. These numbers are read against each other and against the same
modules before the change, and against nothing else.

### After

**`status.rs` and `note.rs` re-run in full: 121 mutants, 114 caught, 5 unviable, 2 missed** — from
75 caught and 41 missed. `redact.rs`'s five targeted mutants were replayed by hand instead, each
confirmed red.

**Both remaining survivors were defects in the new tests, and the tool found them, not the
reading.**

- **`255:25`, `p.suppressed > 0` -> `>= 0`.** The absence assertion for the suppression line lived
  in the empty-board sweep — where `p.candidates` is 0, so the *enclosing* guard returns first and
  the nested condition is **never evaluated**. The assertion passed and guarded nothing, which is
  M17's shape reproduced inside a test written to catch omissions. Under the mutant a board with
  candidates and no suppression prints `0 candidate(s) held back by a decline`: D64's
  tombstone-ROI number reporting a cost that was never paid.

  **The generalisation is that an absence assertion carries a hidden premise.** Asserting a line
  is missing proves nothing unless the block containing it *rendered*. So a nested guard needs its
  enclosing block asserted **present** in the same test — which is what
  `the_suppression_line_needs_a_suppression_and_not_merely_a_candidate` now does on its first line,
  and why the vacuous row is kept in the sweep with a comment saying so rather than deleted.

- **`283:49`, `p.export_failures > 0` -> `< 0`. Equivalent, and left alive on purpose.**
  `COUNTER_EXPORT_STALE` has exactly one bump site, inside the `if st.drifted()` that immediately
  follows the unconditional `COUNTER_EXPORT_CHECK` bump, so `failures > 0` with `checks == 0`
  cannot occur and the right operand of that `||` is unreachable. Fixturing it would pin a state
  the database cannot produce — the same thing declined for the zero-injection `by_force` row
  above.

  Instead the **premise** is asserted, as `a_lane_with_no_injections_has_no_sessions_either` does
  for its own equivalence: `export_publishes_a_decision_into_the_repo_it_governs_and_detects_drift`
  now reads the counters back and requires `checks >= failures`. Verified by breaking it — a second
  `COUNTER_EXPORT_STALE` bump outside the drift branch reddens it with `checks 3, failures 5`.
  Without that, adding such a bump would make the word "equivalent" false with nothing going red.

**Fifteen tests, and not one line of production code changed.** All four modules computed
correctly throughout; what was unguarded was what they put on the page — and, in `write_snapshot`,
which of two paths a guard chose. 473 tests, from 458 — 474 after a later cleanup pass lifted the
lane-split rule out of the test it was buried in, which is where it should have been written.

Every survivor across the four modules was replayed by hand and confirmed red, and `status.rs`,
`note.rs` and `delivery.rs` were then re-run under the tool: **121 mutants at 114/116 viable**, and
**`delivery.rs` at 34/34**. The one remaining live mutant anywhere is `status.rs:283:49`, kept
deliberately as an equivalent with its premise asserted.

Both new invariants were checked the way this project requires — by breaking what they guard, not
by watching them pass. Zeroing `strip_pem`'s counter reddens the redaction property on its own, and
adding an *unguarded* `  ! ` line to `render_status` reddens the alarm property while the needle
list beside it stays green, which is the whole point of having it.

### Re-measured after a cleanup pass, which is the only thing that could have proved it safe

**2026-08-31.** A `/simplify` review of this session's own diff produced six fixes to the new
tests — `empty()` reduced to `Receipt::default()`, five `..empty().receipt` likewise, `primer_end`
derived from `PRIMER`, an unused process spawn dropped, and the lane-split rule lifted out of the
test it had been buried inside. Every one is semantics-preserving *by construction*, and that
argument is worth exactly nothing here: a green suite cannot see a mutant a test has stopped
reaching, which is the whole premise of this document.

So `status.rs` was re-run: **94 mutants, 91 caught, 2 unviable, 1 missed — 91/92 viable**, the
predicted result. The single survivor is `283:49`, the same equivalent, unmoved (test-module edits
do not shift production line numbers, so a prediction that it would move was itself wrong).

**And the survivor confirms the premise assertion is load-bearing.** Column 49 is the `>` in
`p.export_failures > 0`; `export_failures` is a `usize`, so `< 0` is always false and the guard
collapses to `p.export_checks > 0`. That is equivalent **only if a failure cannot happen without a
check** — which is precisely the `checks >= failures` invariant now asserted end-to-end in
`memory_e2e.rs`. Change either `WHERE` behind those counters and the word "equivalent" here becomes
false, and that test is the thing that would go red.

### Two arithmetic errors in this document, both caught by re-deriving from the data

Written from memory and wrong: *thirty-one of forty* survivors characterised as one edit (the true
split is 37 on a print-guard, of which 10 are that edit), and a redaction direction table reading
3/1/12/2 against the actual 4/1/9/4. Neither changed a conclusion, and both were found by counting
`missed.txt` programmatically rather than by re-reading the prose.

**The project's rule was being applied to the measurements and not to the summaries of them.**
"Repeat any measurement before quoting it" reads as a rule about instruments; a table in a document
is a measurement too, and the cheapest guard is to derive it from the file rather than to
transcribe it.

### The finding applied as a sweep, and it closes

`n > 0` guards that decide whether something is *printed* are enumerable — there are eleven across
the whole of `src/`, and every one is now asserted in both directions:

| Where | Guard | Guarded by |
|---|---|---|
| `status.rs` ×5 | `unreadable`, `candidates`, `suppressed`, `unprompted`, `rest` | this session |
| `status.rs` | `failures` | `status_reports_whether_the_hook_is_actually_capturing`, e2e |
| `delivery.rs` | `hidden` | this session |
| `memory/inject.rs` | `hidden` | M23 |
| `memory/index.rs` | `stats.unreadable` | already correct — `an_unreadable_note_is_reported_and_a_clean_pass_stays_quiet` asserts both |
| `memory/write.rs` | `w.redacted` | already correct — `a_redaction_is_announced_when_it_happened_and_never_otherwise` |

A twelfth match, `events.rs`'s `if self.unprompted > 0`, is not in this class: it selects a
`Verdict` rather than a line, so any test asserting the returned value kills its mutants.

**Two of the eleven were already right, and both say so in a comment** — `index.rs`'s names the
rule outright: *"it is printed only when non-zero … so the absence of the line is itself a claim,
and both directions are asserted."* The pattern was known here before it was named; what was
missing was the sweep.

### The next target this measurement names

Not another renderer. The corrected finding points at **guards over derived counts**, wherever they
are, and at the modules still unmutated rather than at a category. `delivery.rs` was this
measurement's own prediction and it came back at 88% with its worst defect (`write_snapshot`) not a
rendering defect at all — so the useful next question is which modules have never been under this
pressure, not which ones print.

Mutated to date: `messages.rs`, `claims.rs`, `memory/inject.rs`, `memory/promote.rs`,
`memory/events.rs`, `memory/redact.rs`, `memory/status.rs`, `memory/note.rs`, `delivery.rs`.
Everything else has not been, and `src/hooks.rs` is the one that edits
`~/.claude/settings.json` for every project on the machine.

> **`hooks.rs` was done on 2026-08-31 and this paragraph picked the right module** — M39.
> 83.5%, and one of its survivors was a `pub fn` with no caller anywhere. `doctor.rs`
> followed the same day at 93.0% (M42), then `identity.rs` at 97.7% (M43). `delivery.rs`
> and both of those refute the renderer hypothesis, and M43 refutes the replacement
> hypothesis M42 offered for it.


## M28 · Two artefacts described themselves with a constant, and both constants had rotted

**2026-08-31.** A documentation-currency pass, asking the narrow question of whether fifteen new
tests had moved any number the docs quote. One had, and not in the direction expected — and the
same pass found a second artefact broken by the same session, in the same way.

Both are a **constant standing in for a property of something that changes**: a cost figure standing
in for a cache state, and a line range standing in for the length of a comment block. Neither fails
loudly. `verify.sh` prints a number; `mutants.sh` exits 0.

`tools/verify.sh`, `README.md` and D70 all published **16.9 s warm**, itself a re-measurement from
2026-08-29 that had corrected an earlier `6.5s`. Measured again today, on a quiet machine with no
concurrent `cargo`:

| State | Runs | Cost |
|---|---|---|
| Nothing changed since the last run | 9.60 / 9.83 / 9.93 / 12.11 | **~10 s** |
| After `touch src/lib.rs` | 31.07 / 29.12 | **~30 s** |
| *Published* | — | *16.9 s* |

**The published figure sits between two states that differ by 3×, and matches neither.** Both are
"warm" in the ordinary sense — the target directory is populated and nothing was cleaned.

### Which state the old number measured, settled from its own next clause

The header did not merely say `16.9s warm`; it said *"dominated by clippy and the suite"*. That
clause is what disambiguates it, because in the no-op state it is false. Per-step, no-op:

| Step | Cost | Share |
|---|---|---|
| `cargo fmt --check` | 0.16 s | 2% |
| `cargo clippy --all-targets -- -D warnings` | **0.13 s** | 1% |
| `cargo test --quiet` | 2.51 s | 26% |
| `python3 tools/check_docs.py` | 2.56 s | 26% |
| `python3 tools/find_unread_fields.py` | 4.35 s | 45% |

Clippy is a **0.13 s cache hit** and the two audit scripts are 71% of the run, so "dominated by
clippy and the suite" cannot describe this state. It describes the other one. The comparable value
is therefore ~30 s, the growth is the suite going 376 → 473 tests, and **nothing about the old
measurement was wrong when taken** — only the word standing in for its method.

### Why a cache-state claim fails in both directions

This is the third instance of the catalogue entry that an artefact asserting a *method* is a claim
in its own right, and the second where the failure is not the optimistic one. Re-measure the cheap
state and 16.9 s reads as inflated — a number someone padded. Re-measure the expensive state and it
reads as a 78% regression someone shipped. Both readings are wrong, and a reader has no way to pick
between them, because the claim never named the state.

So a cost claim has to name its cache state the way a ratio has to name its denominator, and for
the same reason: `9.6 s` and `31.1 s` are both honest measurements of `tools/verify.sh` on the same
commit, on the same machine, minutes apart.

### The second constant, and this session wrote it

`tools/mutants.sh` prints its own usage by extracting a fixed line range from itself:

```sh
sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'
```

Twelve lines were added to that header earlier in the same session — the note recording why a
TIMEOUT row is an unanswered question rather than a caught mutant. The header then ended at line
**34**, and the usage text began ending mid-sentence:

```
comparable across tools, languages or codebases — quoting 83% against a published 80% would be
```

The reader loses the clause that says what the score *is* good for, which is the whole point of the
paragraph. Nothing failed: `mutants.sh` with no arguments still exits 64, still prints thirty-one
lines, and still looks like a usage message. **This is M24's defect in a second artefact** — a
rendered thing truncated where no assertion looks — reached this time not by a wrapped literal but
by a hardcoded offset that the text it indexes is free to outgrow.

The fix is to stop asserting the length: print the leading comment block and stop at the first line
that is not one.

```sh
awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$0"
```

Verified the way this project requires — by changing what it depends on rather than by watching it
pass. A sentinel line inserted into the header renders, and the final sentence stays complete;
under the old range the sentinel would have pushed one more line off the end. The other three
scripts in `tools/` were checked for the same shape and none has it.

### An observation the breakdown surfaces, not acted on

`check_docs.py` runs `cargo test --no-fail-fast` internally to check the quoted test count, so
**the gate runs the suite twice** — 2.51 s of the no-op cost is a second full run. It is not a
defect: the count check needs a real run, and reusing the first would couple the two steps. It is
recorded because a future reader optimising the gate will find the duplicate and should know it was
seen and left alone deliberately, which is cheaper than rediscovering the coupling.

### A third instance, in the header fixed for the second

`tools/mutants.sh` said *"roughly 25 s per mutant"*. Measured the same day: 94 mutants in 5m, about
**3 s each** — off by 8x, in the same header whose line-range constant had just been replaced by a
derivation. Two rotted constants in one file, found hours apart, the second only because a run was
finally timed against the claim.

The fix is the derivation again, and it needed no new machinery: cargo-mutants already prints
`N mutants tested in Xm` on its last line, so the header now points at that instead of quoting a
rate. Per-mutant cost depends on how fast the suite fails under a given mutant — a property of the
mutant, not of the tool — so the rate was never a stable quantity to assert in the first place.

### What the two have in common, which is the reusable part

A cost claim and a line range look like different kinds of thing, and both are the same mistake: a
**literal that encodes a measurement of something still moving**. `16.9` encoded a cache state,
`32` encoded a comment block's length. Each was correct when written, neither had any way to notice
it had stopped being correct, and neither broke anything when it rotted — one printed a stale
number and the other printed a truncated paragraph.

The distinction that first suggested itself was that only one of them was *fixable* by removing the
constant: `mutants.sh` can derive its range from the file, whereas a gate's cost "cannot be derived
— it has to be measured", so naming the method was the only repair available.

**That was wrong, and wrong in the direction this document exists to catch.** A cleanup review of
the same session's diff observed that the repair chosen here replaced one rotting literal with
*two* — 29–31 s and ~10 s — in the same commit that taught `mutants.sh` to stop hardcoding; and
that a gate's cost is not un-derivable at all. The script is already running when the question is
asked. `tools/verify.sh` now ends every run with `gate: Ns`, on the failing path as well as the
passing one, and its header describes the two states qualitatively without transcribing either.
D70 and this entry keep the dated figures, where they are records rather than claims about now.

So the rule survives only in its strong form: **where a constant encodes something that moves,
derive it.** What made the exception persuasive is worth naming, because it will be persuasive
again — measuring a thing and reporting a thing feel like different activities, so "this one has to
be measured" sounds like a reason it cannot be automated, when it is a reason it can. *The
exception is what a constant sounds like from the inside.* Try making the artefact answer for
itself before concluding that it cannot.

---

## M29 · The delivery back-off rotates the inbox rather than draining it

**2026-08-31.** Observed on the live board during a read-only review, then measured. Not
theorised: it was found by reading the banner the reviewing session was actually being handed.

The board held **15 unread messages, all broadcasts, aged 2–3 days, all superseded** — two
announcing schema 9 and schema 10 against a board at schema 12, three announcing D-number ranges
against a record at D95.

One `Stop` hook against the real board:

```console
$ echo '{"hook_event_name":"Stop"}' | amb hook turn
injected characters: 2834      (~708 tokens)
```

**Then the banner dropped from 15 unread to 5, and the five were exactly the overflow set.** Two
explanations — somebody acknowledged ten, or the back-off retired ten — and they are distinguished
by one command, because `inbox` never hides whatever the offer count:

```console
$ amb inbox --json          -> 15  [1,3,10,11,12,13,14,16,17,18,19,20,22,23,24]
$ amb inbox --unread --json -> 15  [1,3,10,11,12,13,14,16,17,18,19,20,22,23,24]
```

**Nothing was acknowledged.** The first ten crossed `MAX_OFFERS` and left `deliverable()`. Per D33
an offer is recorded against *what was shown*, so the five sitting in the `…and 5 more` overflow had
accumulated **zero** attempts, and were beginning their own ten-offer cycle.

### The cost model, which is not the one the constants suggest

`MAX_OFFERS` bounds offers per message. `MAX_RENDERED` bounds messages per offer. **Neither bounds
the product**, because the cohorts run in sequence:

```
injections  ≈  MAX_OFFERS × ceil(backlog / MAX_RENDERED)  =  10 × ceil(N/10)  ≈  N
```

Hook injections scale **linearly with the backlog**. Fifteen messages produce about twenty; sixty
produce about sixty.

### And it lands on D24's own worked example

`MAX_RENDERED`'s comment records the measurement that motivated the cap: *"sixty unread messages
measured at 20,779 characters — roughly 5,200 tokens — injected at every turn boundary,
identically, because nothing drains an unacknowledged inbox."*

| | injections | chars each | aggregate |
|---|---:|---:|---:|
| uncapped | 10 | 20,779 | ~208,000 |
| capped, today | 60 | ~3,500 | ~210,000 |

**The aggregate is unchanged. D24 redistributed the cost rather than reducing it.**

This is not a refutation of D24 and must not be read as one. Peak matters independently of total:
5,200 tokens arriving in one context is a different failure from 700 arriving sixty times, and the
cap fixed the one that can wreck a small window. But the clause in its own comment — *"nothing
drains an unacknowledged inbox"* — was a diagnosis of the situation, not a description of what the
cap changed, and it stayed true. D96 is the clause that addresses it.

### How it was found, which is the reusable part

By reading the injected banner in a live session and noticing it announced schema 9 to a board
running schema 12. Every test passed; the last mutation run was 88/91; `doctor` was green; D23, D24
and D33 were each doing exactly what they were built to do. **The defect is in the composition of
three correct decisions, and there is no unit at which it is visible.**

`CLAUDE.md` already names running the binary against the real board as a third source of truth
alongside tests and mutation. This is the second recorded instance after M24 — and it is the one of
the three that still has no script.

---

## M30 · The argument for redacting message bodies was strong, and one run refuted it

**2026-08-31.** Q13 asked whether `send` should redact as `observe` does, filed the case for and
against at full strength, and named the experiment that would settle it. This is that experiment.
It closed the question as **no** (D98), and found two defects nobody was looking for.

### Method, and why it is not a reimplementation

The real `amb::memory::redact` did the redacting, called from a throwaway crate outside the working
tree with a path dependency on the library — a peer session was committing to this repo at the
time, so nothing transient was left in it. Rust redacted; Python diffed the output against the
original and produced the context windows. **No rule was restated anywhere**, so the probe cannot
drift from the filter it measures.

The board was read through `file:…?mode=ro`. Nothing was written to it.

Two numbers were recorded per message rather than one: the `removed` counter *and* whether the text
actually changed. They are never derived from each other, because `removed` is what an author is
shown and the delta is what happened to their text — M27's finding is that this field's meaning
lives in its reader. **They never disagreed across 53 bodies**, which is a small positive result for
`Redacted.removed`.

### Result

| | |
|---|---|
| corpus | **53 bodies, 98.3 KB**, 4 senders, 2 projects, 5 days |
| real secrets | **0** |
| body removals | **1** |
| subject removals | **0** |
| counter/delta disagreements | **0** |
| long tokens (≥40 chars) in corpus | 21, of which 1 removed |

Zero is the honest reading of the numerator, not a rounding: no PEM block, no `<private>`, no
credential prefix, no credential assignment. Every `password` hit is prose about password managers
and the only `sk-` is `task-1`.

### The one removal, which is the finding

An agent's scratchpad prefix, in a message telling a peer where their recovered work was saved —
the whole payload of its sentence, replaced by `[redacted]`. **The same path appears three times in
that body and the two longer forms survive:**

```
kept:    …/scratchpad/peer-WINDOWS-TESTING-uncommitted.md
kept:    …/scratchpad/peer-latency-edit.patch
REMOVED: …/scratchpad/
```

`is_high_entropy` returns early on `.`, so the discriminator between kept and destroyed is a **file
extension** — which has no relationship to secrecy. The shorter, less revealing form is the one that
dies.

Replacement is whole-token, so adjacent markup goes too. Confirmed by adding the call temporarily
and reading the failure:

```
right: "the command was `deploy --token [redacted] reproduced verbatim"
```

The closing backtick and the comma are gone.

**And that filter runs on the vault today** (D37). The live vault is clean — 46 notes, **zero**
redaction markers, so it has never fired on a real note either — but the false positive is
reachable there now. `is_high_entropy`'s docstring asserted such a path *"is lowercase and fails
the mixed-case test anyway"*; a macOS scratchpad path carries capitals from `-Users-…-Projects-…`
and digits from a session UUID. Corrected, and pinned by
`a_deep_path_is_redacted_which_is_a_known_false_positive` so it cannot rot back.

### A constant rotted inside one day

Q13 sized its own corpus at **"~26 KB"**. It was **98.3 KB** when the experiment ran, the same day
the question was filed. Nothing was wrong when written; the board grew. M28 recorded two artefacts
describing themselves with a stale constant and concluded that a cost claim should be measured
rather than transcribed — **this is the same shape in a question rather than a script**, and the
practical rule is narrower than it looks: a figure quoted to justify that an experiment is *cheap*
does not need to be right, and had it been quoted as a result it would have been wrong.

### The new catalogue entry: a doc comment attached to the wrong item

Q13's prescribed remedy was *"add a sentence to `send`'s docstring"*. **`send` had no docstring.**

```rust
/// Send a message. Returns its id.
/// … BEGIN IMMEDIATE … 17 concurrent processes … zero SQLITE_BUSY (M1, corrected by M16).
/// The longest body `send` accepts, in characters.     <- no blank line; same /// block
…
pub const MAX_BODY: usize = 100_000;

pub fn send(…)                                          <- undocumented
```

`MAX_BODY` was inserted between the doc block and the function it described, and with no blank line
the two comments merged. So `MAX_BODY` documented itself as *"Send a message. Returns its id."*, the
concurrency evidence for `BEGIN IMMEDIATE` was filed under a size limit, and the function had
nothing.

**This is not the catalogued false-comment defect and it is not stale prose.** Every sentence is
true and every sentence is in the file; only the *binding* is wrong. It therefore survives every
check this project has: `check_docs.py` screens documents against the code, not doc comments
against their items; rustdoc renders it without complaint; no lint fires, because the item is
documented — just not the one the author meant. Reading the source top to bottom does not reveal it
either, since the text sits in the right *place* and only the intervening `pub const` moves it to
the wrong *item*.

**The check is mechanical and worth running once: a `pub fn` with no doc comment, directly below a
`pub const` or `static` whose doc block opens with a verb phrase.** More generally — when adding an
item between a doc comment and what it documents, the blank line is load-bearing.

It was found only because a task required editing that exact docstring. Which is the same lesson as
M24 and M29 arriving from a third direction: **the defects this project keeps finding are ones no
instrument was pointed at, surfaced by doing something specific with the artefact.**

### Reproducing it

The probe is not in the tree — it is fifteen lines over `amb::memory::redact`, and the corpus is
whatever the board holds now, so a stored copy would be the stale thing rather than the
reproduction. Read message bodies out of `~/.agent-messageboard/board.db` read-only, call `redact`
on each through the library, and diff. **The result is dated and the population is small** — 53
messages from 4 agents. It is evidence, not proof. Re-run it rather than re-arguing D98.

---

## M31 · The settings read-modify-write loses updates, and a lock fixes only half of it

**2026-08-31.** A review asserted that `amb install`'s unguarded read-modify-write on
`~/.claude/settings.json` could lose a concurrent writer's change. Asserted, not measured. This is
the measurement, and it corrected the claim twice.

### The harness had to be rebuilt before it measured anything

The first two attempts reported **0 losses in 275 trials** and were worthless. The competing writer
was `python3 writer.py`, and interpreter startup is ~25 ms — so the writer always acted long after
`amb` (≈4 ms) had finished. **No race was ever created.** The rebuilt harness pre-warms the writer
in a thread of the driver process, so its read-modify-write costs microseconds, and sweeps the
delay in 0.1 ms steps across 0–9 ms.

M15 and M16 record harnesses that published wrong numbers. This is the same failure caught before
publication rather than after, and only because a negative result was disbelieved.

### The defect, measured

540 trials per configuration, 6 at each of 90 delays, quiet machine, debug binary.

| | third party's key lost | amb's own hooks lost | corrupt |
|---|---:|---:|---:|
| **before** | 38 | 8 | **0** |

**Both directions lose**, which the review had not predicted: `amb` destroys a foreign setting, and
a foreign writer destroys `amb`'s hooks — the second being a silent stop to mail delivery. The race
window is 2.7–4.0 ms, matching the measured duration of `amb install`.

**Zero corrupt files, in every configuration measured.** The temp-file-plus-`rename` was already
correct and is unchanged. The defect is the *cycle*, not the write.

### A lock fixes amb against amb, and nothing else

| after | third party's key lost | amb's hooks lost |
|---|---:|---:|
| advisory lock only, **cooperating** writer | **0** | **0** |
| advisory lock only, **uncooperative** writer | 38 | 4 |

Advisory locks require cooperation, and **Claude Code will never take amb's lock** — it writes this
file itself, `/config` storing `crossSessionInbound` into user settings. A lock alone would have
shipped as a fix for the threat that was actually documented while not addressing it.

### Compare-and-swap covers the uncooperative writer, and the ordering matters

Re-read the bytes immediately before the `rename`; a mismatch restarts the cycle.

| after | key lost | hooks lost | total |
|---|---:|---:|---:|
| lock + CAS, backup copied **inside** the window | 12 | 8 | 20 |
| lock + CAS, backup copied **before** the check | **5** | **5** | **10** |

**The backup copy was sitting between the check and the `rename`**, putting a whole file copy
inside the window the check exists to close. Moving it above halved the residual. Nothing about the
logic changed — only the order — and no test could have found it.

### What remains, stated rather than rounded away

- **amb against amb: 0 of 540.** Solved.
- **amb against an uncooperative writer: 10 of 540**, and the two halves are different problems.
  Five are amb's residual gap between its final check and its `rename` — one syscall, irreducible
  without an atomic compare-and-rename the platform does not portably offer. **Five are the other
  program's own non-atomic cycle**, which no change here can fix: it reads before amb's rename and
  writes after.
- 46 → 10 overall, of which 5 are not amb's to close.

Do not read the headline as "fixed". It is **0.9% residual against a hostile interleaving swept at
0.1 ms resolution**, against 7% before, and the remaining half of that is somebody else's race.

---

## M32 · The third source of truth got its script, and its first run found three defects in itself

**2026-08-31.** *Filed as M31 and renumbered within the hour: a peer session committed its own
M31 between the moment this session read the range and the moment it appended. That is the defect
`records_are_uniquely_numbered` now catches, and it is the same lost-update shape D99 records for
`settings.json`, one level up.*

`tools/eyeball.sh` runs the real binary against a **copy** of the real board and
prints what a session actually sees: `doctor`, both installed hooks under the payloads Claude Code
sends them, `inbox`, `claims`, the receipt. M29 closed by observing that running the binary against
the real board is a third source of truth alongside tests and mutation, that M24 and M29 were both
found that way, and that it was the only one of the three with no script. This is that script.

**Why a fixture cannot substitute, which is the whole argument.** Current practice separates a
*smoke test against the deployed system* from an *integration test against a fixture*, and the
separation is exactly the one this project needs: a fixture is built to match the code, so drift
between accumulated state and current code is the one thing it can never contain. M29 was a banner
announcing schema 9 to a board at schema 12 with every test green, mutation at 88/91 and `doctor`
green. There is no fixture that contains that defect, because writing the fixture means choosing
both numbers.

**The standard caution about running against production data is answered by copying.** Everything
runs against a throwaway copy, so nothing can mark mail read or record an injection into
`note_events` — the table D87's window is computed over. The copy is made with `sqlite3 .backup`
and not `cp`, because the board is in WAL mode and its `-wal` sidecar holds committed transactions
the main file does not.

### Three defects, all of them in the instrument, all found by running it

**1 · A byte digest of a live WAL database is not a modification signal.** The script's central
claim is that it touches nothing, and the first version checked that by comparing `sha256` of
`board.db` before and after. It printed **THE REAL BOARD CHANGED** on a run that had not written a
row. Measured directly:

```console
$ a=$(shasum -a 256 board.db); sleep 3; b=$(shasum -a 256 board.db)   # no amb command at all
   digest stable over 3s: NO
$ SELECT count(*) FROM messages WHERE ts > now-300     ->  0
$ SELECT count(*) FROM note_events WHERE ts > now-300  ->  0
```

Zero rows written in five minutes, and the file's bytes changed anyway: another session merely
*reading* a WAL database updates `-shm` and can trigger a checkpoint that rewrites the main file.
**A read mutates the file.** Replaced with logical row counts, which show the claim rather than
assert it — the copy gained one injection row and the board gained none:

```
copy   61 msg / 87 read / 193 claim / 82 inject  (the writes landed here)
board  61 msg / 87 read / 193 claim / 81 inject  (unchanged)
```

And the honest residual is stated in the script: on a board several sessions share by design, a
difference is **not attributable** to this script, so it is printed and never failed on. The
structural guarantee is the one that holds — `AMB_DB` is exported once, so every child inherits the
copy — and that is what the script checks and exits non-zero on.

**2 · A cross-artefact check without attribution cannot state its own finding.** The check compares
schema numbers in rendered text against the board's, which is M29's defect and something no unit
test can hold because no unit holds both numbers. The first version warned on any mismatch and
immediately fired on `schema 4` — from the **subject line of a two-day-old message** shown by
`amb inbox`. That is not a defect: D96 gives the horizon to the delivery path and deliberately not
to `inbox`, and a message is a record of what someone said at the time.

So the same string carries opposite verdicts depending on **who wrote it**, and a check that cannot
tell amb's own voice from amb correctly quoting someone else's is a warning that gets ignored inside
a week. Sections are now tagged by authoring surface: amb's own voice is a loud failure, a message
*being injected* is M29's live condition, and `inbox` is a note that says read it and do not fix it.

The first attempt at attribution was also wrong, and the way it was wrong is worth keeping: it
classified by the `> ` quote marker, on the reasoning that `quoted()` contains sender text. The
number was in a **subject**, and `amb inbox` renders subjects without that marker.

**3 · The doc gate checked `bench/*.py` and not `tools/*.sh`.** `check_docs.py` already argued that
"a script with no citation makes no promise and should be deleted" — and enforced it in one
directory. Found by adding `tools/eyeball.sh` and noticing that nothing objected. Widened, and on
its first run it named a **second** uncited script that a peer session had just added. It now skips
untracked files, because an untracked file is work in progress rather than an uncited script, and
without that the gate fires on another session's half-finished work.

### What the three have in common

Every one is the instrument failing rather than the code, which is this project's established
pattern — but note the sharper version: **the script found them by being run, in its first minutes,
and no test of it would have.** A test of the digest check would have used a quiet fixture board and
passed. The WAL behaviour only appears when other processes are on the file, which is the condition
the product exists for and the condition a fixture removes.

That is the same argument as the script's own reason for existing, arriving one level up, and it is
the reason to keep the thing un-asserted and read by a person.

---

## M33 · Eighteen shape assertions, and the defect they were written for reddened none of them

**2026-08-31.** The proposal was mechanical and sounded finished: *"every renderer that produces a
user-visible line gets one shape assertion — no double spaces, no leading whitespace, no trailing
whitespace. One shared helper asserted against each renderer's output closes the whole class rather
than the instance."* Two of its three clauses turned out to be wrong, and the way they were wrong
is the entry.

### The universal rule is false, and real output says so

M24's rule is *"a rendered line has no double space"*, confirmed red against the string that
shipped. Applied globally it fails immediately. Measured over **274 lines** of real `amb` output
captured by `tools/eyeball.sh`:

| invariant | violations | verdict |
|---|---|---|
| tab character | 0 | assertable |
| blank line made of spaces | 0 | assertable |
| trailing whitespace | **59** | a real defect, fixed below |
| interior run of 2+ spaces | **50** | **legitimate** — every one an aligned column |

The 50 are `board  /Users/…` beside `copy   /var/…` — deliberate alignment, in this project's own
tools. A rule with a legitimate exception on a fifth of its input is one people switch off, so
"no double space" stays a **per-renderer** assertion where the output is prose (`events.rs` keeps
it) and is deliberately absent from the shared helper, which says so in its docstring.

The 59 were one defect: `quoted_block` prefixed every line with `"> "`, so a blank line inside a
message body rendered as `"> "` — trailing whitespace on every blank line of every quoted body, on
`amb inbox` and the delivery banner alike. The containment is the prefix; the space was decoration.

### The finding: the helper was the cheap half, and it proved nothing

`assert_rendered_shape` was added and wired into **eighteen** renderers through their existing test
bindings. All 490 tests passed. Then the `quoted_block` defect was put back to check the guards
bit:

```
FAILING: NONE — nothing catches it            (490 passed, defect present)
```

**Not one of the eighteen.** No fixture anywhere in the suite had a message body containing a blank
line, so `quoted_block`'s empty-line branch was never reached by any of them. This is M17's shape —
a fixture that never reaches the guarded branch — arriving *inside* the guards written to close
M24's, in the same hour, and it passed green the way M17's did.

The fix is the **fixture**, not the assertion: one `\n\n` added to the body in
`every_renderer_of_a_sender_written_field_contains_it`, which already enumerated all three
`quoted_block` callers in the `UNTRUSTED` idiom. With it, reintroducing the defect reddens two
tests including the enumeration.

So the proposal's third clause — *"one shared helper closes the whole class rather than the
instance"* — is false in the direction that matters. A helper closes a class only where fixtures
reach it. **The helper is minutes of work and the fixtures are the work**, and the ratio between
those two is exactly what makes this failure easy to ship.

### And the automated pass missed the renderer that matters most

The eighteen were found by scanning tests for `let x = render_*(…)`. `render_inbox` is called in
tests without being bound to a variable, so it was skipped — and `render_inbox` is what
`amb inbox` prints, the command the `SessionStart` banner tells every agent to run first, the same
surface D90 found unguarded for the same structural reason. **A mechanical sweep inherits the shape
of what it scans for**, and the outermost surface is the one least likely to look like the pattern.
It is covered now through the enumeration rather than through a binding.

---

## M34 · Every threshold a decision names, and the one that was worse than absent

**2026-08-31.** D95 established that a decision naming a numeric threshold needs something able to
say whether that threshold is *reachable*, because a dead condition is trusted while an absent one
is questioned. This is the audit of the rest of them. It was expected to produce a list and it
produced one hit, one surprise, and a defect in the tool built to close them.

### The list

| Decision | Threshold | Could anything report reachability? |
|---|---|---|
| D59 | 30 sessions / 50 injections / cited ratio below 0.10 | **Yes** — `verdict: too early — needs 27 more session(s) and 32 more injection(s)`. D95's own fix, working |
| D13 | a claim expires at 4 h | **Yes** — `amb claims --all` shows lapsed rows and how long ago |
| D49 | a candidate expires after 30 days without a derivation | **Yes** — `amb memory candidates` reports how close each is |
| D96 | a broadcast leaves the delivery path at 24 h | **Partly** — the expiry works; nothing counts how many are past it |
| **D83** | **prune when the board passes 50 MB** | **No.** `doctor` printed the board's *path* and never its size |
| **D83** | **prune when `amb inbox` passes the 5 s hook budget** | **Worse than no** — see below |

One genuine hit out of six, which is what the audit predicted. The surprise is that D83's two halves
failed in *different* ways, and the second is the one worth the entry.

### The size half was absent. The latency half was answered by the wrong input

`bench/bench_startup.py` has timed `amb inbox` all along — it is where README's published 3.0 ms
comes from. But it points `AMB_DB` at an **empty scratch board**, deliberately and correctly, so
that a benchmark never touches the real one. The consequence is that the number it produces is
**structurally incapable of crossing a threshold that is about the real board growing**. It will
read ~3 ms at 50 MB and at 5 GB.

That is D95's shape with an extra step. An absent instrument makes the next reader go and look; an
instrument that reports a healthy number against input the condition cannot reach makes them
trust. It is also question 1 of the ratio rule — one unit of what was measured is "an inbox render
over zero messages", and the claim standing beside it is about a board with sixty-eight.

Both halves are readable now. `doctor` gained a `size` row; the latency moved to
`tools/eyeball.sh`, which is the one place in the tree where `amb inbox` runs over real accumulated
content with no side effects:

```
ok    size            0.5 MB of the 50 MB at which D83 builds pruning
  amb inbox   5 ms of the 5000 ms hook budget (D83), over 68 messages
```

**The footprint is three files.** `page_count * page_size` gives the main database exactly, but in
WAL mode the `-wal` sidecar holds committed transactions the main file does not yet contain, so
summing one file understates a busy board. Disk footprint is what "the board passes 50 MB" means to
a person, so `-wal` and `-shm` are added.

### And the tool built to close the gap had the same class of defect inside it

`eyeball.sh` snapshots the board before and after to show that it wrote nothing. Reading those
snapshots with `sqlite3 -readonly` **fails on a `.backup` copy**:

```console
$ sqlite3 -readonly copy.db "SELECT count(*) FROM messages"
Error: in prepare, unable to open database file (14)
$ sqlite3           copy.db "SELECT count(*) FROM messages"
68
```

The copy is WAL-mode with no `-shm`, and a read-only connection cannot create one. On the *live*
board the same command succeeds — but only while some other session happens to hold that shared
memory open, so it is non-deterministic on a machine whose whole premise is concurrent sessions.

**The failure mode is the part to keep.** `sqlite3` returned an empty string, and the check was
`[ "$board_after" = "$board_before" ]` — so **two failed reads compared equal and rendered
`unchanged` on a board that had changed.** A comparison of two failures is indistinguishable from
a match, and it fails in the flattering direction every time: the tool reports that it touched
nothing precisely when it has lost the ability to tell. That is D88's shape — an instrument that
only writes on the happy path reports a broken mechanism as an idle one — arriving in a comparison
rather than in a ledger.

Both sides are now counted through a `.backup` copy opened normally, and an empty snapshot is
reported as *"could not count the board"* rather than as agreement. **When a check compares two
reads, ask what it prints when both reads fail.**

---

## M35 · A gate check switched itself off, and the thing that switched it off never touched it

**2026-08-31.** The git history was reset at the user's direction and re-initialised, so the first
push to GitHub would not be blocked by push protection on 89 of 95 historical commits. That
operation destroyed the `v0.1.0` tag. It also, invisibly, disabled one of the six checks in
`tools/check_docs.py`:

```python
tag = subprocess.run(["git", "describe", "--tags", "--abbrev=0"], …).stdout.strip()
if not tag:
    return []                      # <- unconditional, from the moment the tag stopped existing
```

`unreleased_is_honest` is the check that catches `CHANGELOG` claiming *"Nothing yet"* while commits
exist. `CLAUDE.md` records that **all six were verified by breaking them**, and that was true when
written. Afterwards five were live and one returned an empty list every time, and the gate printed
`✓ all checks passed` exactly as before.

### Why this is not just another dead condition

D95 named the shape — a stated condition that cannot fire is worse than none, because a reader sees
a standard and assumes something is watching. M34 found the same shape in D83's threshold. Both of
those were **dead at birth**: nothing had ever been able to evaluate them.

This one **worked, was verified, and was then killed from outside.** The operation that killed it
was a `git init` — it did not touch `check_docs.py`, `CHANGELOG.md`, or anything the check reads
except a tag that was incidental to the check's purpose. There is no diff to review, no commit that
introduced it, and no test that could fail, because the check's own contract is "return the problems
you found" and it found none.

**So the reusable question is about dependencies rather than about conditions:** *what repository
state does this check need in order to be able to fail, and what routine operation destroys it?* A
check that reads a tag, a remote, a branch name, an untracked file or a directory is disabled by any
operation that removes one — and it reports that state identically to a clean run. This is D88's
shape ("a ledger that only writes on success reports a broken mechanism as an idle one") arriving
through an external dependency rather than through an unhappy path.

**The repair is to remove the dependency, not to restore the tag.** "Commits exist since the last
release" and "commits exist at all" are the same question while no release has happened, and the
second cannot be destroyed by a `git init`. A tag, when there is one, still narrows the count and
the message. Confirmed by breaking:

```
CHANGELOG [Unreleased] says 'Nothing yet' with 3 commit(s) in a history with no tag
```

### And a rule D70 said was enforced by one sentence

The same push exposed a real divergence: `check_secret_literals.py` had been added to
`tools/verify.sh` and not to `.github/workflows/ci.yml`, so **CI would have passed a commit the gate
rejects** — the one thing a duplicated workflow exists to prevent. D70 records the fix and then says
outright that *"this sentence is the only thing enforcing that"*.

`the_gate_and_ci_run_the_same_checks` now enforces it, and it is deliberately one-directional: a
step in CI that is not in the gate is expected, because the matrix builds on Linux and the local
gate cannot. A step in the gate missing from CI is the drift. Verified by removing one:

```
tools/verify.sh runs 'tools/check_secret_literals.py' and .github/workflows/ci.yml does not
```

Two rejections were amended the same day for the same reason — D70's `pre-push` alternative and the
release-automation rejection both argued from *"there is no remote"*. Both conclusions survive on
other grounds and both notes say so, because **a rejection defended by a fact that stopped being
true is how a settled question gets reopened on a technicality.**

---

## M36 · What a check needs in order to be *able* to fail, and two that could not

**2026-08-31.** M35 ended on a question rather than a finding: *what repository state does a check
need in order to be able to fail, and what routine operation destroys it?* This is the sweep. It
found two live instances, and **one of them was created by the fix for the other's sibling, three
hours earlier, in the same file.**

### The dependency map

| Check | State it needs to be *able* to fail | Destroyed by |
|---|---|---|
| `every_doc_is_indexed` | `docs/*.md` non-empty | deleting docs |
| `every_command_is_documented` | a built binary | nothing quietly — `main` exits 2 and says so |
| `counts_are_current` | suite output, `## D` headings | a suite that does not run |
| `records_are_uniquely_numbered` | `## D` / `## M` headings | an empty or renamed record file |
| `unreleased_is_honest` | ~~a git tag~~ commit count | **was** `git init` — repaired in M35 |
| `the_gate_and_ci_run_the_same_checks` | `run "…"` lines in `verify.sh` | rewriting the gate's step syntax |
| `every_bench_script_is_named` | a **non-empty git index** | `git init`, before the first `git add` |
| `check_secret_literals.py` | a **non-empty git index** | the same |

### The one written while fixing its sibling

`every_bench_script_is_named` was widened to `tools/` earlier the same day and given `git ls-files`
so it would skip untracked work in progress. An empty index makes `tracked` an empty set; every
script is then "untracked"; every script is skipped; the check returns `[]`. **That is the identical
shape as `unreleased_is_honest`'s `if not tag: return []` — written into the same file, hours after
repairing that one.**

`CLAUDE.md` already records the pattern: *"fixing one instance trains attention on the thing fixed
rather than on its siblings"*, with D86, D88 and D90 each a second instance of a defect repaired in
the same file the same day. This is a fourth, and the sibling was not merely nearby — **the repair
produced it.**

### The one that mattered, and the window was not hypothetical

`check_secret_literals.py` calls `git ls-files … check=True`. `check=True` catches git *failing*. It
does not catch git *succeeding with an empty index*, which is exactly the state between `git init`
and the first `git add`.

This repository was in that state on 2026-08-31, during a history reset **whose entire purpose was
getting past GitHub's secret scanning**. In that window the check would have printed

```
no credential-shaped literal in tracked source.
```

having opened no file at all, and exited 0. Confirmed in a fresh repository: before the change it
printed exactly that and exited 0; after it, `git lists no tracked files, so this check read
nothing. That is an inability to answer, not a clean result.` and exit 1.

### The remedy is two halves, and both are standard practice

The failure has a name outside this project. It is what Vitest's `passWithNoAssertions: false`
exists for, and the canonical case is **seven integration tests passing while the dev server was
never running**, because an early return meant no assertion was reached — *tests passing is not
tests verifying*. The published guidance is to make an unmet prerequisite an explicit skip or
failure rather than a silent return. That is the first half, applied to both instances above.

The second half is MongoDB's **canary test**: one that tests the testbed rather than the software
under test. `checks_can_still_fail` is that, and it deliberately checks nothing about the
repository — it checks that the checks are still looking at something. Each entry names a population
that must be non-empty for some check above to be able to fail, and none can be non-empty by
accident. Against a fresh empty repository it reports all four; against this one, nothing.

**A check with no input reports success, and the vacuous result is byte-identical to the healthy
one.** That is the whole class, and it is why no amount of reading the output finds it.

### And the first verification of the last fix proved nothing

`unreleased_is_honest` looked only for the literal *"Nothing yet"*, so an **empty** `[Unreleased]`
— which makes exactly the same claim — passed. Fixed, then verified by emptying the real
`CHANGELOG.md` and re-running the gate. It came back **green**.

Not because the rule was wrong. `HEAD` was the tagged commit at that moment, so `v0.2.0..HEAD` was
zero commits and `int(n) > 0` returned *before* the new clause was ever evaluated. **M17's shape — a
fixture that never reaches the guarded branch — inside the verification of a fix for this very
class, in the same hour it was written down.**

Re-verified against a scratch repository with one commit after a tag, as a truth table:

| `[Unreleased]` | verdict |
|---|---|
| empty | `is empty with 1 commit(s) since v0.1.0` |
| `Nothing yet.` | `says 'Nothing yet' with 1 commit(s) since v0.1.0` |
| real content | passes |

The third row is what proves the other two: it fails if the enclosing `int(n) > 0` ever stops being
reached, which is the premise the first attempt silently lacked. **An absence-only break test has an
unproven premise, and "I emptied it and the gate stayed green" is indistinguishable from "the fix
does not work" until something shows the branch was reached at all.**

---

## M37 · The promotion ledger redacted in silence, and the first fix guarded the wrong half

**2026-08-31.** `derive` called `redact(...).text` at three sites and discarded `.removed` at every
one. So a credential in a derivation was stripped correctly and **the author was never told** — in
the one flow D49 designed entirely around a human seeing what they are approving.

M27 already named this exact shape from the other side: *"`Redacted.removed` reads, inside
`redact.rs`, as bookkeeping. Its actual contract lives one module away"* — `write.rs` prints
`"N value(s) redacted before writing"` under `if w.redacted > 0`, and its comment gives the reason:
**a redaction the author cannot see is one they cannot correct.** `observe` honoured that.
`derive` did not, and nothing connected the two.

The count is computed for what is **written** rather than for what was examined. An existing
candidate keeps its stored title and body, so on that path the only new text is the derivation line;
counting the whole note there would report removals that never reached the file, and a number that
overstates is as untrustworthy as one that understates. The notice is word-for-word `write.rs`'s,
because two spellings of one guarantee are two guarantees to keep in step.

`redact(note)` was also being computed twice, one line apart, with both results discarded.

### The first fix was verified, and the verification was worthless

Two tests were written against it: a truth table over `render_derived` with rows at `0`, `1` and
`7`, and a cross-renderer check that the notice matches `observe`'s to the character. Both passed.
Relaxing the render guard `d.redacted > 0` to `>= 0` reddened the truth table, exactly as intended.

Then the *other* mutation — forcing the computed count back to `0`, which **is the original
defect** — was applied:

```
FAILING: NONE — the guard is fake
```

Nothing in the entire suite. Both tests set `Derived.redacted` **on a fixture**, so they guard the
renderer and never the computation. The path from `redact()` into the struct had no test at all, and
the two tests written specifically to close this defect could not see it being reintroduced.

**This is the sharpest form of the reader/writer split M27 records.** M27's advice is *"when you
check a counter, go and read what reads it"* — and following that advice produced two tests of the
reader and none of the writer. The advice is right and incomplete: guarding where the meaning is
does not guard where the value comes from, and a fixture-set field silently severs the two.

The fix is an end-to-end test through the real binary — a credential in `--note`, asserting both
that the vault file does not contain it *and* that the output says a value was removed. M20's
arithmetic picks that layer: count the layers a rule passes through, count the ones that assert it,
and suspect the outermost, because it is the expensive one to write and therefore the one that does
not exist. With it, discarding the count reddens.

The fixture is built with `concat!`, because `tools/check_secret_literals.py` refuses a contiguous
credential shape in tracked source and **testing a redactor means writing one** — the permanent
condition that tool's own header records.

---

## M38 · The convention that a retired question names its decision, and the sentence asserting it was already false

**2026-08-31.** `docs/OPEN-QUESTIONS.md` retires a settled question by **deleting** it, and until
the history was reset to publish the repository, `git log` was the net under that. The reset
destroyed it: the prose of Q1–Q7, Q9 and Q12 now exists only in an archive outside the repo. The
file records this honestly and draws the right conclusion — a question deleted from here must
leave its answer in `DECISIONS.md`. What it could not do is enforce it, so the convention lived in
one sentence that nothing read.

The check proposed for this was *"every `Qn was settled` line names at least one existing
D-number"*. **That check passes on the file as it stood.** Both retirement paragraphs name
decisions, and every decision they name exists. It would have shipped green.

### What was actually wrong, and why a presence check cannot see it

The reset note lists Q12 among the deleted and promises that *"each of those questions names the
decision it became, immediately below"*. **Q12 is named nowhere below it.** Q13 was settled into
D98 the same day and does not appear in the file at all — not in the note's list, not in a
retirement paragraph, nowhere.

Both answers exist and both are correct: D85 dropped `notes.content_hash` and its heading says
outright that it closes Q12; D98 closed Q13 on data. Nothing was lost. What went missing was the
**pointer from the register that promises them**, and it went missing inside the paragraph doing
the promising. Everything that paragraph *does* say is true, which is exactly why it reads as
complete — the same shape as a false comment being worse than an absent one, applied to a
convention rather than to a mechanism.

A convention whose entire content is an absence cannot be checked by reading what is present. So
the check is arithmetic over the numbers rather than a search for text:

| | |
|---|---|
| cited anywhere in the docs | Q1–Q13 |
| open `## Qn` sections | Q8, Q10, Q11 |
| named by a retirement paragraph | Q1–Q7, Q9 |
| **unaccounted** | **Q12, Q13** |

Four rules ship. Three of them — names a decision, the decision exists, nothing is settled while
still open — inspect claims somebody made, and all three were green on the broken file. The fourth
takes the union of every `Qn` any document cites, subtracts the open sections and the retired
ones, and reports the remainder. It is the only one that fired, and it fired before anything was
fixed.

**Every doc, not only `DECISIONS.md`.** The citation proving a question existed is as likely to be
in `MEASUREMENTS.md`; Q13's only two surviving pointers were one of each.

### The residual hole is at the top of the range, and it is named rather than closed

A question created and deleted while no other document ever cited it leaves nothing for rule 4 to
subtract. Only the archive can see that. What rule 4 covers is the case that has now happened
twice here — the answer written down properly, and only the pointer lost.

### The pattern was brittle in a place the verb form is not

The first version required `(?:was|were) settled` with the two words adjacent, and **it refused to
recognise the paragraph written to fix the drift it had just reported.** That paragraph reads
"Q12 and Q13 have *also* been settled". The token that broke the match was an adverb, not a verb
form — so enumerating more verb phrases would not have helped, and each one added is a branch no
real paragraph exercises. The slack now sits between the auxiliary and `settled`, where writers
put adverbs. Verified against the real file: exactly the three retirement paragraphs match, and
nothing else in it does — not the convention prose ("none should be settled by whoever notices it
first", "a settled question is deleted rather than annotated"), and not Q10's blockquotes.

Failing to recognise a retirement is the harmless direction, because its questions then go
unaccounted and rule 4 reports them. But a rule that rejects ordinary English is one people switch
off, which `records_are_uniquely_numbered` already says of its own legitimate exception.

### Verified by breaking each rule

All four reddened against the real file, each perturbation applied and restored inside one process
with the file's digest compared before and after. Rule 1 by deleting the D-number from Q7's
paragraph; rule 2 by pointing it at D999; rule 3 by adding Q8, which still has an open section;
rule 4 by deleting the paragraph that records Q12 and Q13. Removing every retirement paragraph
fires the `checks_can_still_fail` canary *and* rule 4 on all ten retired questions at once —
three quarters of the check goes vacuous with no population, and the quarter that does not is the
quarter that matters.

### The check found a defect in its own author's prose, an hour after it was written

The paragraph retiring Q8 closes by pointing at Q14, which is open and stays open. Extraction was
paragraph-scoped, so every `Qn` in the paragraph counted as retired *by* it, and rule 3 duly
reported Q14 as settled-and-still-open. **A cross-reference is not a claim.**

This is M17's shape arriving inside a check written days after M17 was recorded — the input reaches
a branch the author did not intend, and everything about the code is right except what it was
pointed at. It is worth noting which direction it failed in: a false *positive*, loud and on the
first real use, rather than a question silently going unaccounted.

Scoped now to the regex's own match span, which runs from the first `Qn` to `settled` and is
therefore the subject phrase by construction: `Q1–Q6 and Q9 were settled` yields exactly those
seven. Decisions stay paragraph-scoped, because that is where they are written — the subject phrase
names the questions and the sentence after it names what they became. `finditer` rather than
`search`, so a paragraph retiring two questions in two sentences counts both.

**The false positive is now a control row in the break-test**, and that is the part worth keeping.
Rule 3 must fire when a still-open question sits in the subject phrase *and* must not fire when one
is merely cited in the same paragraph. Four perturbations proving the rules can fail would have
passed identically before and after the fix; only the row asserting the rule stays quiet
distinguishes them. An absence needs its own row, which M23 and M27 both record from the other
side.

---

## M39 · The module that edits every project's settings had never been mutated, and one survivor was dead code

**2026-08-31.** `tools/mutants.sh src/hooks.rs` — the module M27's own roll-call named as the next
target, and the last major one never put under this pressure. It edits `~/.claude/settings.json`,
which configures Claude Code for every project on the machine, and `CLAUDE.md` says outright that
corrupting it breaks the user's whole tool rather than just this one.

**114 mutants in 8m: 86 caught, 17 missed, 11 unviable, 0 timeout — 83.5% of viable.**

Conditions, recorded because M17 and M27 each record a run invalidated by them: private
`CARGO_TARGET_DIR`, `--jobs 1`, load 6.00 at the start and 11.37 at the end, and the same five peer
processes present before and after with no `cargo` build appearing in the window. **Zero TIMEOUT
rows**, so nothing here is an unanswered question in M27's sense.

### The seventeen were four findings

| Function | Survivors | The finding |
|---|---|---|
| `our_hook_exes` | 6 | the detector for this project's most-recurring failure, with no test |
| `read_settings`, `read_raw`, `apply` | 8 | the whole read path, with no test |
| `quote_exe` | 1 | one rule, two instances, guarded at one |
| `Mode::parse` | 1 | the CLI's own contract check, with no test |
| `write_settings` | 1 | **dead code** |

### `write_settings` was dead, and the tool that exists to find that was explaining it away

`find_unread_fields.py` had printed the same three names on every run for days, under its own
inline advisory: *a function passed by reference has no parentheses and looks uncalled.* That
explanation is **true for two of the three and false for the third**. `plan_uninstall` has eleven
uses including `main.rs` passing it by reference; `command_is_ours` has fourteen including
`is_ours`'s `.is_some_and(command_is_ours)`. `write_settings` had **one definition, zero calls and
zero references** — its only two mentions were rustdoc links inside doc comments.

Mutation is what separated them: replacing the whole body with `Ok(())` survived the entire suite.
Neither signal is sufficient alone — an untested *live* function survives mutation too, and a
zero call count was already being explained away — but together they are conclusive.

D84 recorded this precise shape once before, when the same advisory printed three names for days
and one was a duplicate implementation. **It recurred with a different name**, which is the
argument for reading the advisory rather than scrolling past it, and the argument for not trusting
a tool's own explanation of its false positives to be exhaustive.

Deleted rather than tested, following D23, D39 and D45. It was superseded by D99's `apply` +
`write_if_unchanged` cycle and, left in place, it is a public function that writes settings
*without* the lost-update check — a convenience whose next caller silently reintroduces the defect
D99 exists to prevent. Its two doc references were repaired in the same change, because a comment
naming a deleted function is the false-comment failure this project has shipped four times.

### `our_hook_exes`: the reader was tested and the producer was not

Six survivors in one small function, including replacing its whole body with `vec![]`. It is the
only thing that can see the stale-binary condition D94 records as having recurred five times:
`command_is_ours` matches the file *name*, so a hook invoking last month's build is still "ours"
and `HookState` still reports `Installed`. Returning the path is what lets `doctor` compare
fingerprints — and `doctor`'s own tests build their fixtures directly, so the producer between
them ran under no assertion at all. That is M37's shape exactly, one module further out.

### `quote_exe`: the fixture reached the branch and the assertion looked elsewhere

`an_install_path_containing_an_apostrophe_is_still_recognised` installs `/Users/o'brien/bin/amb` —
an apostrophe with no space, which is precisely the isolating input — and then asserts that
`command_is_ours` accepts it and `plan_uninstall` removes it. **Both pass on an unquoted command
line**, because `unquote` is a no-op there and `file_name` is still `amb`. Its sibling asserts
`cmd.starts_with('\'')`, but only for a path containing a space.

One rule, two instances, guarded at one of them: D90's shape. It is *not* M17 — the fixture does
reach the branch — and it is not D51 — the mutation does redden something, just not this rule. The
test is thorough, carries a long docstring about O'Brien being a real surname, and defends against
a reviewer's stated worry rather than against the function's own contract. What it costs is an
unbalanced quote in `~/.claude/settings.json`: the shell cannot parse the hook command, so it never
runs, and a hook that fails is a hook that says nothing.

### `Mode::parse`: the variants are asserted 38 times and the parser never

`--mode` is a `String`, so `Mode::parse` *is* the contract check behind `amb install --mode <x>`,
and `src/main.rs` is its only caller. Deleting the `"session"` arm reddened nothing. M20's
arithmetic picks this out: count the layers a rule passes through, count the layers that assert it,
and suspect the outermost, because it is the one no cheap unit test happens to cover. Closed with a
round trip over every variant rather than three literals, so a fourth mode cannot arrive with a
parse arm spelled differently from its `as_str`.

### Every new test was verified by applying the mutation it was written for

Nine representative mutants, each applied to the real file, the targeted test run, and the file
restored with its digest compared. All nine reddened. The `!is_ours` one needed a longer anchor:
the same three lines appear in `duplicate_hooks`, so the first attempt reported the anchor twice
and was skipped rather than silently mutating the wrong function.

---

## M40 · A check I had just repaired answered "clean" having run nothing, and the repair is what did it

**2026-08-31.** Five new tests moved the suite from 505 to 510, and `counts_are_current` reported
**511**. The extra one came from `tests/props_probe.rs`, an untracked file a peer session had
created minutes earlier: `cargo test` compiles every `tests/*.rs`, tracked or not, so the number
the documentation is *required to quote* came partly from a file that is not in the repository and
would have rotted the moment its author renamed it.

`every_bench_script_is_named`, twenty lines below in the same file, had already been repaired for
exactly this, and its comment says so: *"Untracked means work in progress, not an uncited script.
Without this the check fires on a peer session's half-finished file and blocks a commit that has
nothing to do with it."* The sibling was left with the hole — the third instance in this file of
the rule that fixing one case trains attention on the case rather than on its siblings.

### The repair reproduced the defect it was citing

Scoping the count to `git ls-files "tests/*.rs"` looked right and was not: **a git pathspec `*`
matches across `/`**, so `tests/common/mod.rs` came back as well, `--test mod` went onto the
command line, and cargo exited 101 with an empty stdout. `actual` was therefore 0 — and the next
line was

```python
if actual:
```

so the comparison was skipped and the check printed *"docs are consistent with the code on every
mechanical check."*

**That was read as success.** It was caught one step later and only because this project's habit is
to perturb a new guard and watch it redden: setting `CLAUDE.md` to a wrong count produced nothing.
Without that step, the commit would have shipped a permanently unfailable check — in an edit whose
own comment cites M35 for that precise defect.

### Which of the two bugs matters

A wrong pathspec is an ordinary mistake and would have been caught by anyone eventually. `if
actual:` is not: it is `if not tag: return []`, the sentence M35 and M36 exist about, sitting in
the function immediately above the one repaired for it — and it silently converts **every** future
mistake of the first kind into a passing run. The pathspec was fixed. The guard was replaced with a
reported inability to answer, which is the form the rest of this file already uses, so a cargo
invocation that never ran is now a finding rather than a clean bill of health.

Verified in both directions afterwards, which is what the first attempt skipped: a wrong count
reddens, a correct one passes, and the peer's untracked file moves neither.

---

## M41 · The tool that exists to find dead code put the dead function in its reassuring arm

**2026-08-31.** M39 found `hooks::write_settings` dead by mutation. `find_unread_fields.py` exists
to find exactly that and had been printing the name every run for days — under the wrong heading.

Its advisory has two arms. One says *nothing in production mentions it at all*, which is a finding.
The other says *referenced N times without parentheses, so it is passed by reference*, which is the
tool explaining its own false positive. `write_settings` printed under the second, with `N = 2`.

**The two references were rustdoc links.** ``[`write_settings`]`` is a bare mention of a name with
no parentheses, which is indistinguishable — to a regex over raw source — from
`.is_some_and(command_is_ours)` or `apply(&path, dry_run, plan_uninstall)`. The reference count was
taken over `PROD`, which is every `src/*.rs` file up to its `mod tests`, comments included.

So the tool was not silent and was not wrong about the call count. It was **wrong about the arm**,
and in the reassuring direction. D84 records that this advisory gets scrolled past; the reason it
gets scrolled past is that two of its three lines were permanent noise and the third was
misfiled with them.

### The fix, and the probe that proves it

References are now counted over a comment-stripped view. The stripper is crude in the direction
that fails loudly: it over-removes, so the error it can make is a false *"nothing mentions it"* —
checkable in seconds — rather than a false reassurance.

Verified by reconstructing the condition rather than by reasoning about it. A `pub fn
amb_probe_dead` was added with two rustdoc self-links and no callers:

| | where the probe lands |
|---|---|
| with the fix | **NOTHING IN PRODUCTION MENTIONS AT ALL** |
| references counted over raw source, as before | *passed by reference — 2 bare mentions* |

The classification flips on exactly the change under test, and `write_settings`'s history is that
second row.

### The two arms are now printed separately

They mean opposite things and shared a paragraph, which is what a reader skims as one block. The
finding arm names D23, D39, D45 and M39 and says what to do next — confirm with mutation, since a
dead body replaced by a constant survives and that is the second half of the proof. Neither arm
fails the run: a `pub fn` reached only from an integration test is legitimate, and blocking a
peer's commit on a screen whose own header says it over-reports would trade a silence for a
worse one.

**Two signals were needed and neither was sufficient.** A zero call count was already being
explained away; a surviving mutation is equally consistent with a live but untested function. Only
together do they decide it, which is the argument for running both rather than trusting whichever
one is cheaper.

---

## M42 · The highest score in the project, and its four survivors were one line that two tests each half-covered

**2026-08-31.** `tools/mutants.sh src/doctor.rs`, following M39 — `doctor` is the *reader* that
`our_hook_exes` feeds, and M39 found that producer untested.

**68 mutants in 7m: 53 caught, 4 missed, 11 unviable, 0 timeout — 93.0% of viable**, the highest
recorded in this project (`delivery.rs` 88%, `hooks.rs` 83.5%, `status.rs` 56%).

Conditions: private `CARGO_TARGET_DIR`, `--jobs 1`, load 4.29 at the start and 8.59 at the end,
which is the run's own; no `cargo` build and no commit appeared in the window, and the tree was
clean throughout. Zero TIMEOUT rows. A peer was asked over the board to hold builds for ten
minutes, which is the coordination `amb` exists for and the first time this project has used it
to protect a measurement.

### All four survivors were one line, and the arithmetic beside it was caught

```rust
let mb    = bytes as f64 / (1024.0 * 1024.0);              // 4 survivors
let limit = db::PRUNE_AT_BYTES as f64 / (1024.0 * 1024.0); // caught
```

Identical expressions, one line apart. `limit` is computed from a constant and
`the_size_row_names_the_threshold_as_well_as_the_size` asserts `"50 MB"` appears, so every mutation
of it reddens. Nothing asserted the other one at a value where being wrong shows.

### Two tests covered half of this each, and the halves did not overlap

Neither test is wrong, and neither name oversells what it does:

| Test | Inputs | Asserts |
|---|---|---|
| `..._names_the_threshold_as_well_as_the_size` | `size_check(0)` | `.detail` contains `"0.0 MB"` |
| `..._fires_at_the_threshold_and_not_before` | `at - 1`, `at`, `at * 2` | `.health` only |

The first *does* inspect the rendered size — at the one input where every mutation agrees.
**Zero is the fixed point of all four**: `0/x`, `0*x` and `0%x` all render `0.0`. The second uses
inputs that discriminate perfectly and never looks at the number.

This is M17's fixture problem arriving through a **pair** of tests rather than one. M17's form is a
single test whose comment names a branch its fixture cannot reach; here the input that reaches the
interesting case and the assertion that would have seen it were in different functions, each
complete on its own terms. Neither would be found by re-reading either test, which is why mutation
is the only thing that could see it.

**The generalisable part is that the fixture decided it, not the code.** Same expression, same
file, one line apart, one guarded and one not — and what separated them was which operand a
fixture happened to make interesting. When a value is rendered from a computation, ask what the
test would print if the computation were wrong, and pick an input where that answer differs.

What it cost: `amb doctor` reporting `1536.0 MB`, `3145728.0 MB`, `3298534883328.0 MB` or
`0.0 MB` for a 3 MB board — a wrong number on the page while the verdict stayed correct. That is
M27's third instrument-failure mode with the halves swapped: there the arithmetic was right and the
rendering wrong, here the rendering is right and the arithmetic wrong, and both reach the reader
identically.

Closed with a truth table over the *rendered* size at three inputs, all four mutations confirmed
red. The zero row is kept with a comment saying it proves nothing about the arithmetic, because
deleting it would lose the empty-board case and leaving it unlabelled is how it came to stand in
for coverage it never had.

### The renderer hypothesis is refuted a second time

M27 read `status.rs` at 56% and asked whether renderers are inherently harder to guard.
`delivery.rs` came back at 88% and refuted it once. `doctor.rs` is the most verdict-rendering
module in the project — every line is a health judgement someone reads to decide whether to
reinstall — and it is the highest-scoring. **What predicts a low score is not what a module
produces but whether it has ever been under this pressure**, which is the question M27 said to ask
and this is now the second module to answer it the same way.

> **The second half of that sentence is too strong, and `identity.rs` refuted it the same
> afternoon** (M43). It had never been mutated either and came back at 97.7%. "Never mutated"
> predicts *unknown*, not *low* — which is a weaker claim and a better argument for running it
> everywhere rather than triaging by guesswork.

---

## M43 · Both survivors were one rule at the two call sites a test's own docstring names

**2026-08-31.** `tools/mutants.sh src/identity.rs` — liveness and session identity, feeding every
other surface, and never previously mutated.

**92 mutants in 13m: 85 caught, 2 missed, 5 unviable, 0 timeout — 97.7% of viable**, the highest
recorded here. Conditions: private `CARGO_TARGET_DIR`, `--jobs 1`, load 4.07 at the start and 7.95
at the end, no `cargo` build and no commit in the window, tree clean. A peer held builds again on
request.

### The two survivors

```
src/identity.rs:254  replace match guard is_unique_violation(&e) with true in reclaim
src/identity.rs:287  replace match guard is_unique_violation(&e) with true in register
```

One rule at both of its call sites, and **the test guarding that rule names those exact call sites
in its own docstring**:

> *"Found by mutation: `is_unique_violation` could return `true` for every error. **Its two call
> sites use it as a match guard** to decide whether to retry under a different name, so treating an
> unrelated failure — a missing table, a locked board — as a name clash would rename an agent in
> response to something that has nothing to do with its name."*

That test builds a synthetic table and asserts the predicate directly. It is correct, its comment
is correct, and it describes the consequence precisely. It does not touch either call site.

**So the sequence is worth stating plainly**: mutation found the predicate; the fix guarded the
predicate; the docstring named the call sites and the exact failure; and the call sites stayed
unguarded until mutation was pointed one layer further out. This is M20's arithmetic — count the
layers a rule passes through, count the layers that assert it — arriving on a rule whose inner
layer had already been fixed *by this same technique*. **A comment naming a call site is not a test
of that call site, however precisely it describes the failure.**

What it costs: a board that cannot be written reports `NameTaken`. The agent is told to pick a
different name for a condition no name can fix, which is this project's signature shape — not an
error, a plausible wrong answer.

### The predicate is broader than its name, and that shaped the test

`is_unique_violation` matches **any** `ErrorCode::ConstraintViolation`, not specifically a unique
one. Correct by the current schema, checked rather than assumed: `agents` carries a primary
key, four `NOT NULL` columns and one unique index. The primary key is absorbed by
`ON CONFLICT(id) DO UPDATE`; the `NOT NULL`s are unreachable because `register` constructs every
value it writes and `name` goes in through `COALESCE(?7, agents.name)`, which cannot yield NULL.
That leaves `ux_agents_name` as the only constraint able to fire — so the guard is right today.
The name overstated what the match does and is since renamed `is_constraint_violation`, with
this schema argument as its docstring; the record keeps the name it had when the finding was
made.

The name still asserts more than the code checks, and what would break it is a new column fed from
an `Option`, or a `CHECK`. Neither exists; both are cheap to add without noticing this.

It is also why the test induces failure with a trigger whose body names a missing table rather than
with `RAISE(ABORT)`: `RAISE` *is* a constraint violation, so it would have satisfied the mutated
guard and the real one alike. A missing table is `SQLITE_ERROR`, which is the distinction the
predicate actually draws.

### Verified by applying both mutations

`reclaim` reddens returning `None` — the swallowed error, read by `register` as "the name stays
taken". `register` reddens returning `NameTaken` where an `Error::Sqlite` belongs. The first
attempt used a wrong indentation for one anchor and reported *anchor appears 0x* rather than
silently mutating nothing, which is the reason to count anchors instead of trusting a replace.

### Three modules, three scores, and the tidy explanation did not survive the afternoon

`hooks.rs` 83.5%, `doctor.rs` 93.0%, `identity.rs` 97.7% — all three previously unmutated. M42
proposed that having never been mutated is what predicts a low score. `identity.rs` refutes it.
The honest version is that **nothing available so far predicts the score**, which is a weaker claim
and a better argument for running it on every module than any triage heuristic would be.

## M44 · The morning-after simulation, and what doctor actually prints

**2026-08-31.** Q14 filed the distribution question around one hazard: `brew upgrade` is D94's
stale-hook condition with a worse trigger, and its own text ends on "whatever ships has to answer
what `amb doctor` reports the morning after an unattended upgrade". The text then *asserted* the
answer — "D73 built `doctor`'s fingerprint comparison for exactly this question, so the detector
already exists" — which is D95's shape: a claim about an instrument, with the instrument never run
on the condition the claim is about. This run converts the assertion into a measurement.

### Method

Everything sandboxed, nothing touched: a copy of the real board (`sqlite3 .backup`, the WAL rule
M32 records), a fake `$HOME` whose `.claude/settings.json` is the real one with every amb hook
command rewritten to a binary at another fingerprint, and the real installed binary running
`doctor` against it. The second fingerprint cost nothing — the mutation baseline for the db.rs run
in flight was built at `a57c610` while the installed binary is at `4529eeb`. The stale side is
played by the *newer* commit; the comparison is symmetric, and what is under observation is the
sentence, not the direction. The donor binary is only ever invoked with `--version`.

### The first take was wrong, and the way it was wrong is documented behavior

The donor was first copied to `stale-amb`, and `doctor` reported **"no amb hooks are installed"**
— ten rows of a healthy-looking machine with two warns, on a settings file with six amb hooks in
it. `command_is_ours` matches the executable's *name*, exactly as D73 records. A rename is outside
the brew scenario (every packager in Q14's survey installs the binary under its own name), but the
take is kept here because it is the demonstration: a hook whose binary is renamed does not go
stale, it goes *invisible*, and the ten remaining rows read as a clean bill.

### Result, verbatim

```
BAD   binary          the PostToolUse hook runs …/staledir/amb
         which reports  0.2.0 (a57c610 2026-08-31 dirty, schema 12, sqlite 3.53.2)
         but this build is  0.2.0 (4529eeb 2026-08-31, schema 12, sqlite 3.53.2)
         Manual commands work and every hook is stale. Copy it: cp "$(command -v amb)" …/staledir/amb
```

Every other row `ok`. The condition is named in one sentence, both fingerprints are shown, and the
remedy is the literal command. Q14's main objection — an unattended upgrade breaking every hook
silently — is detected, named and remediable the morning after, **provided someone runs `doctor`**.

### What the run exposed that nobody was looking for

The process exited **0** with a `BAD` row on screen. That is D73's decision — a diagnosis is not
itself a failure, and `--json` carries the verdict in `worst` — but `doctor.rs` said otherwise in
three doc comments: "what the exit code and the summary line are built on", "`worst` is the exit
code", "it drives the exit code". None was true, and there is no summary line in the text output at
all. The comments described a design D73 explicitly rejected — the fifth instance of the false-
comment class (`sync_dir`, `recall`, the bench harness, D95), and on the exact field an unattended
check would be built on: a reader trusting them would write `amb doctor || alert` and alert never.
All three now state the true contract: `worst` is the verdict `--json` carries, the exit code is
always 0, and automation reads the field, not `$?`.

## M45 · The reached-assertion audit: two holes in seventeen constants, and neither was a threshold

**2026-08-31.** The audit item said: a test that can silently stop exercising its subject needs to
assert that it reached it — find the fixtures that could drift out of range. The sweep: every
numeric gate constant in `src/` (seventeen of them — caps, budgets, TTLs, thresholds), classified
by whether any test references it **by name** (self-adjusting) or only by literal; then the known
upstream-filter sites read by hand, because M17's class is filters, not constants.

### The refinement the sweep forced

A fixture literal-coupled to a threshold usually fails **loud** on drift: it lands on the wrong
side of the comparison and an equality assertion breaks. `MAX_CONFLICT_NOTICES` has no test naming
it, and is fine — `the_same_conflict_is_announced_three_times_across_every_path_and_then_stops`
counts to three and reddens the moment the constant moves. The silent class is narrower and worse:

- **an assertion of absence behind a gate**, where the early return produces the same absence the
  test expects, so gate drift changes the test's subject without changing its verdict; and
- **a writer no test reaches**, where the readers are all asserted on hand-built values.

### The two holes

**`sync_dir`'s decline branch had no test caller at all.** Production passes
`Some(AUTO_INDEX_LIMIT)`; `reindex` passes `None`; no test anywhere passed `Some`. The rule "above
the limit, decline and say so" passes through three layers — `sync_dir` writes `skipped`,
`index_is_behind` derives from it (tested, on a hand-built `IndexStats`, per D78), the banner
renders the answer — and the untested layer was the writer, exactly M20's arithmetic and exactly
D45's defect site: delete the branch and every test stays green while a 501-note vault reports
itself empty again. It also returns *before* the prune, and only that return stops a declined pass
from pruning the whole index against a scan that never happened — an omission nothing asserted.
Now `a_vault_past_the_limit_declines_loudly_and_prunes_nothing`: over-limit declines and reports
its size, at-limit indexes (the row that reddens `>` → `>=`), and a declined pass leaves the index
intact.

**The git-sha test could go vacuous on gate drift.** `a_git_sha_is_not_mistaken_for_a_secret`
asserts an absence — `removed == 0` — and `is_high_entropy`'s length gate returns exactly that
absence early. The fixture is 40 characters and the gate is `< 40`: raise it and the test silently
stops testing the tri-class rule its comment names, while the mixed-case positive control (44
chars) stays green. The closure is a control **at the same forty bytes**: one case flip crosses
the tri-class bar, so `redact(flipped).removed == 1` proves the fixture sits one bit from the
boundary — any drifted gate reddens the flip row rather than widening the absence row.

### Cleared, and why

`nearest`'s tie arm (repaired in M17 with an in-budget second candidate), promote's TTL fixtures
(M25/M26's confirming pass), `tests/properties.rs` (floors asserted, an order of magnitude under
observed rates), `VERDICT_MIN_INJECTED` (the tree's one existing `const { assert! }` coupling),
and the twelve remaining constants either name-coupled in tests or literal-coupled in the loud
direction. The wiring `main.rs` performs — passing `AUTO_INDEX_LIMIT` as the argument — remains
asserted by nothing but D70's thin-binary rule, noted here rather than closed: the e2e cost is a
501-file fixture, and the parameterised test above covers every decision the wiring delivers.

## M46 · Mutation-testing `db.rs`: the failure half of WAL had never run, and four timeouts held three survivors and an equivalent

**2026-08-31.** `tools/mutants.sh src/db.rs` — the module owning the schema, migrations, the
location guard and WAL engagement, never mutated before, and the target named to the peer session
on the board before the run started. **85 mutants in 43m: 42 caught, 29 missed, 10 unviable, 4
timeouts.**

**Validity, disclosed rather than assumed.** The peer held cargo as asked. Mid-run, this session's
own `tools/check_docs.py` invoked `cargo test` — forgotten, in the shared target directory, so no
corruption, but real load — and the two adjacent rows ran 53s and 91s against a ~30s norm. Ambient
load ran 9–10 for stretches (`mediaanalysisd`, a peer session, another project's test server). The
private target directory kept every verdict *mechanically* sound; what load can still do is
manufacture TIMEOUT at the floor, and it did.

### Every TIMEOUT was resolved by hand, and none was a catch

Re-run from a clean worktree at HEAD — not the working tree, which by then carried new tests that
would have caught one mutant for the wrong reason and misattributed the answer — on a quieter
machine: **10 mutants, 5 caught, 5 missed, 0 timeouts.**

- `check_not_newer` `>` → `>=` is **equivalent by upstream guard**: both call sites sit two lines
  below `if found == SCHEMA_VERSION { return Ok(()) }`, so the one value the operators disagree on
  cannot reach the comparison. The genuine refusal direction — a board newer than the binary — was
  already pinned by `tests/hook_safety.rs`. No test can or should kill this mutant; it is recorded
  here so nobody chases it again.
- All three `tighten` TIMEOUTs were **live survivors** — `mutants.sh`'s header warns that a
  TIMEOUT filed as "probably caught" removes a real survivor from the count, always in the
  flattering direction, and here it happened three times in one run.

### The 29 missed decompose into three different facts

**16 are mutations of code this host never compiled.** The Linux `volume_of`, Linux `fstype_name`
and the no-platform fallback are `#[cfg]`'d out on macOS: the mutated function is absent from the
binary, every test passes, and the row prints MISSED — indistinguishable from "untested" and
meaning "not present". No test on this machine can ever redden one. Now the third trap in
`mutants.sh`'s header. The Linux arm gets `#[cfg(target_os = "linux")]` tests — the magic table
and a `statfs("/")` row — so CI's Linux leg is the assertor; the fallback arm compiles on **no**
CI platform, and its six rows are the standing price of having a fallback at all.

**2 sat where no fixture can go.** `&` → `|` and `&` → `^` on the macOS `MNT_LOCAL` bit both read
every remote volume as local — and no test can mount a network share, so inline they were
unkillable. Extracted pure as `statfs_is_local`, where the flag word is synthetic: a remote
mount's word is busy, not zero, and the truth table separates all three operators.

**The rest are the failure half of `engage_wal` and `tighten`, and none of it had ever run under a
test.** Ten mutants in the retry loop: the guard verifying SQLite's answer forced `true` — D30's
"checked rather than assumed" check dead, any journal mode waved through silently — plus every
deadline comparison, because those arms only execute when a conversion attempt fails and no test
had ever made one fail. The deterministic refusal was sitting in the standard library: an
in-memory database always answers `journal_mode = WAL` with `memory` (probed with `sqlite3`
before writing the test; `query_only = ON` was probed too and does *not* block the conversion).
The new test asserts the error carries the real mode and arrives **no sooner than the full
budget** — the elapsed floor is what kills the fast-fail mutants, and time only inflates, so the
bound cannot flake. The deadline comparison now exists once, in `budget_spent`, with both sides
one millisecond apart. And `tighten`'s gate — there so a mode the user chose *tighter* than ours
is left alone — could be mutated four ways into widening `0o400` to `0o600`; a truth-table test
pins both directions, with the loose row proving the gate was consulted (M27's premise rule).

**Named residue, deliberately not chased:** the `Err`-arm guards need a race lost mid-conversion
(no fixture errors on cue); the answer-check forced `false` is self-healing (the next loop
iteration's read returns the same verdict, one sleep later); `backoff * 2` degraded to `/` is a
busy-spin with identical outcomes. Each is named in the test's own comment, per
`delivery::UNTRUSTED`'s convention that a residual hole is listed where the guard lives.

### Red-checked, then confirmed by a second pass

All seven new guards were verified by applying the mutation and watching the named test fail:
`statfs` `&`→`|`, both `tighten` masks, the dead answer-check, `budget_spent` flipped, the
`sync_dir` decline deleted (M45's test), and the entropy gate drifted one character (M45's flip
row). Every file reverted byte-identical.

### The confirming pass was void, and the confirmation was made by hand instead

The re-pass over the survivor regions ran 58 minutes and reported **18 timeouts in 49 mutants** —
including on mutants the original run had *caught* — and while it ran, the peer session was
building in parallel and landing its audit-round-two edits (schema 12 → 13) into the shared
working tree. Both facts void it under this file's own rule, and the line numbers in its output
prove the copy had already absorbed a half-landed refactor. Third polluted instrument in one day,
on a machine two sessions share: a long-running fleet pass is the wrong tool on a busy box,
because every spike prints TIMEOUT at the floor and every TIMEOUT demands a hand re-run anyway.

So the confirmation is the hand re-run, per M21's precedent, which load cannot touch — a filtered
test either fails or passes, and no timeout is consulted. **All twelve killable previously-missed
mutants were applied individually and every one reddened its named test**: both `statfs` masks,
all four `tighten` masks, the dead answer-check at both guards, the deadline sign, the deadline
comparison, the decline deletion and the drifted entropy gate — files reverted byte-identical
after each. With the four named residues and the upstream-guard equivalence, that is 16 of 16
real survivors accounted for: 12 killed with each kill observed, 4 accepted with the reason
written where the guard lives, 1 equivalent recorded so nobody chases it again.

## M47 · The diff pass over four fresh commits: the survivors were the newest function, and the cleaner eats mid-run

**2026-09-01.** `tools/mutants.sh --diff a57c610` — every line the last four commits changed: the
peer session's audit round two (never under mutation) and this session's db.rs closures (machine
corroboration for M46's hand-confirmed kills). Coordinated on the board; the peer held cargo.

### Two dead instruments before one live one, and the second death was the proof

The first attempt failed its baseline on a missing `bindgen.rs`; the header's trap said delete
the target directory and accept a cold build. The second attempt failed on the **same missing
file with the directory freshly deleted in between** — which is the observation that mattered,
because it proves macOS's TMPDIR cleaner is concurrent, not nightly: `libsqlite3-sys` writes its
generated bindings with their **packaged 2006 mtime**, so the file is eligible for age-based
eviction the moment it lands, and the cleaner takes it between the build script and the compile.
The remedy in the header was treatment for the symptom. The fix is the mechanism: the private
target directory now lives under `~/.cache`, where no cleaner runs, and the trap's text records
why rather than prescribing the delete-and-retry that just failed twice.

### The run, once it could run

**73 mutants in 9m: 54 caught, 6 missed, 11 unviable, 2 timeouts** — quiet machine, 7s baseline,
17× timeout headroom.

- **Both timeouts are the designed detection working.** `budget_spent → false` makes
  `engage_wal`'s loop never give up; the refusal test hangs and the harness timeout is the alarm.
  M46's test comment predicted exactly this before any run confirmed it.
- **Two missed are the named Err-arm residue** — reachable only by losing a real race — accepted
  in M46 and unchanged here. A prediction that holds across a second instrument is the cheapest
  confirmation there is.
- **Four missed were all in `quick_check`**, the youngest function in the diff: doctor's new
  integrity check could be replaced with always-healthy, always-corrupt, or a flipped comparison,
  and nothing reddened — the row on the page D15's "delete the board" advice hangs from, rendered
  from nothing. Killed with a two-verdict test; the corrupt fixture is one overwritten page,
  probed with the sqlite3 CLI first (`quick_check` answers corruption with a finding row, not an
  error). Both mutant classes re-applied by hand and seen red.
- **Every M46 kill was re-made by the machine**: the diff included this session's db.rs changes,
  and statfs, tighten and the engage_wal cluster all sit in the 54 caught — the hand-confirmed
  evidence now has an independent machine pass agreeing with it.

## M48 · query.rs exhaustively: three missed, and two of them are halves of one rule

**2026-09-01.** `tools/mutants.sh src/memory/query.rs` — the retrieval module, exhaustively:
`recall`'s search, the path lookup, id resolution, and the D45/D88 history. Coordinated on the
board (#121/#122); the peer held cargo, quiet machine, 10s baseline, **zero timeouts** — the
first pass this session with nothing to resolve by hand.

**48 mutants in 5m: 40 caught, 3 missed, 5 unviable.** The three missed:

- **`PATH_LOOKUP_WINDOW`'s `* 8` became `+ 8`** and nothing noticed — the window was a size no
  fixture had ever filled, so the bound on the hottest hook in the system was unobserved in both
  directions.
- **`concerning`'s `exhausted: == → !=`** ran the count(*) fallback on every ordinary vault,
  silently switching `total` to the coarse predicate. Observable only when coarse and filtered
  disagree, and no fixture held the docstring's own `src/auth`-vs-`src/authz.rs` shape.
- **`resolve`'s `1 =>` arm deleted** sent every *unique* bare slug to the ambiguity error. The
  zero and many arms were guarded; the arm every ordinary `--cites <slug>` traverses was the one
  nothing reached — the commonest call shape was the untested one, D88's pattern of a mechanism
  failing exactly where its traffic is.

One fixture kills the first two, because they are halves of the same windowing rule needing the
same middle state (M27's doctrine from the other side): more matching notes than any additive
mis-spelling of the window, fewer than the multiplicative truth, plus one prefix-not-a-segment
note that makes exact and coarse counts disagree. The resolve arm gets the presence row its match
was missing. All three re-applied by hand after writing the guards and seen red; reverts
byte-identical.

## M49 · The `searches` ledger, measured into D83's growth picture

**2026-09-02.** The read-only audit flagged `searches` (migration 12) as the one table that
post-dates D83's measurement — a row per `amb memory recall`, no dedup, never windowed out —
so "what does a year-old board look like" had an unmeasured term. Measured against a copy of
the live board (never the live file), with the `sqlite3` CLI:

- **`searches`: 1 row.** Forty payload bytes. Written 2026-08-31, lane `text`, one hit.
- The same board holds 116 `messages`, 116 `note_events`, and totals 786 KB.

So the flagged table is the **slowest-growing ledger on the board by two orders of magnitude**,
and its growth is bounded by how often `recall` is actually run — which the receipt already
counts. There is no pruning question here at any plausible horizon; D83's 50 MB trigger remains
the number to watch, and `messages` bodies remain what would trip it.

**The row count carries a second reading, and it is Q10's.** This ledger writes misses as well
as hits (`hits` is a column — D89 built it so a broken search and an unasked one stop printing
the same zero), so near-emptiness here means *recall is barely run*, not that failures go
unrecorded. That is also why the FTS5 upgrade stays held: the instrument that would justify it
is live and quiet, which is the cheap answer working as designed.

## M50 · write.rs: the whole missed set was one function, and its docstring named the stakes

**2026-09-02.** `tools/mutants.sh src/memory/write.rs` — the vault writer: `observe`, the atomic
rename, supersession, and the collision loop. Peer held cargo (#132); quiet machine, 17s
baseline, zero timeouts.

**23 mutants in 3m: 13 caught, 8 missed, 2 unviable — and all eight missed sit in `free_slug`.**
The function's own docstring calls silently overwriting a note "the one thing this design
promises never to do", and the promise had no witness: no test in the suite had ever written two
same-day same-title notes, so the bare first stem, the `-2` collision suffix, and the 200-probe
cap were all unobserved. Every operator in the loop could flip without a test noticing.

One sequential fixture kills all eight, driving the real function against a real directory
through every branch: first note unsuffixed, collision takes `-2` and never the first note's
path, and past 200 collisions the probe stops and knowingly reuses `-201` — the bounded-work
trade asserted as a trade. Seven re-applied by hand and seen red; `+= → *=` pins `n` at 1
forever and is observable only under collision — as a hang, which the harness timeout reports.
That is the designed detection (M46's `budget_spent` shape), recorded in the test's comment so
a future TIMEOUT row on this line reads as a kill rather than an open question.

**Verified in a worktree, which is new and worth keeping.** The shared tree carried a peer's
mid-edit hooks.rs and did not compile, so the guard and all eight hand-mutants ran in a
`git worktree` at HEAD plus this one file, reusing the warm private target dir. A live writer's
files were never touched — the stash-around offer was declined on principle. The M-number
collision (their in-flight M49) was caught by reading their diff before writing this entry,
which is what the #65 collision from history says to do.

## M51 · capture.rs: the worst score of the session, and every miss in the layer the tests injected around

**2026-09-02.** `tools/mutants.sh src/memory/capture.rs` — D108's day-old rewrite plus the
transcript parser and the phase receipts. Peer held cargo; quiet machine, zero timeouts.

**First run: 85 mutants in 13m — 55 caught, 23 missed, 7 unviable.** The worst viable score of
any module this session (55/78), and the misses are one story told three times:

- **Twenty sat in the marker shell** — `note_failure` could return 0, 1 or −1, its `+` could be
  `−` or `*`, `note_success` could do nothing, `session_key` could return `None`, `Some("")` or
  `Some("xyzzy")`, every charset comparison could flip, and the staleness window's `30 * 86_400`
  could become a day or zero. All green. D108 shipped `worst_recent_marker` path-injected *for
  testability* and tested it well — and every function above the injection seam was naked. The
  seam is where the tests stopped, which is M50's free_slug finding one module over: the pure
  half guarded, the I/O half trusted.
- **`parse_transcript`'s `status == "error"` arm was reached by no fixture** — every failure in
  the fixture came in through `is_error`, so the comparison could flip and the suite stayed
  green. M17's shape, fourth sighting.
- **`decline_rate`'s two mutants are M27's count-guard class on the number D49's withdrawal is
  read off**: `> 0` relaxing to `>= 0` turns "nothing offered" into `0/0`, and `/` becoming `*`
  turns one decline in two offers into 2.00.

The guards: `sanitise_key`, `bump_marker` and `clear_marker` extracted as injected cores with
row-by-row unit tables (the same move D108 made for the reader, finished for the writers); a
transcript fixture carrying both `status` rows; a two-row decline-rate table; and one e2e that
drives the real binary through the whole D108 story — a two-day-old corpse marker counts and a
sanitised dotted session name lands beside the board, three failures count consecutively, the
warning rides a *healthy* session's SessionStart, that session's success clears nothing of the
broken one's, and the broken session's own healed run clears only its own. Five representative
mutants hand-applied and seen red, reverts byte-identical; then the whole module re-run by
machine.

**Re-run after the guards: 91 mutants in 17m — 85 caught, 6 unviable, zero missed, zero
timeouts.** 55 of 78 viable becomes 85 of 85; the guard tests themselves added six mutants of
new surface and every one of those died too.

## M52 · events.rs: the instrument held, and the one survivor was equivalent until pinned

**2026-09-02.** `tools/mutants.sh src/memory/events.rs` — the instrument module: the receipt,
the searches ledger, the verdict, the window. 1,426 lines, the biggest target of the session,
built up by hand through D89–D95's rounds. Held for a foreign nextest build to drain before
launching; quiet machine, 34s baseline.

**92 mutants in 26m: 89 caught, 2 unviable, 1 missed — the best viable score of the session
(89/90), and the machine's corroboration that the D89–D95 truth-table discipline works.** The
23 tests with both-directions rows, absence rows and arrival notes killed everything with one
exception:

- **`lane_caveat`'s all-zero gate could flip its second `==` and nothing could ever notice**,
  because the distinguishing state — session counts with zero injections — cannot arise from
  the production query. `a_lane_with_no_injections_has_no_sessions_either` is that invariant,
  asserted; so the mutant was equivalent everywhere the invariant holds, and the empty-receipt
  test passed through the *other* `None` arm (M27's unproven-premise shape, in the module that
  taught it). Pinned with a deliberately inconsistent receipt whose comment spells out the
  vacancy: the gate is the first decision, so an invariant-violating receipt fails safe to
  silence. Applied by hand and seen red; revert byte-identical.

A survivor that is *equivalent under a data invariant* is a new row in the catalogue: neither a
weak test nor a mistargeted mutation, but a guard whose only observable input is a state the
system cannot produce. The choice is delete the guard or pin it with an impossible fixture that
owns its impossibility — deleting fail-safe defense on an instrument was the wrong half.

## M53 · redact.rs: thirteen survivors on the security module, and one of my kills was wrong the first time

**2026-09-02.** `tools/mutants.sh src/memory/redact.rs` — D46's named shapes and the counter
whose silence M27 named. First run under 8× ambient load (a Flutter dev loop and macOS media
indexing, no cargo): baseline 39s + 329s, ceiling auto-set to 988s, **zero timeouts** — the
relative-timeout design absorbing exactly the condition that voided a fixed ceiling in its
header's history. 74 minutes instead of ~15, all of it honest.

**87 mutants: 73 caught, 1 unviable, 13 missed.** Nine were one boundary told three ways — the
line where "credential" and "measurement" separate:

- **The wrapping counted toward the length that convicts.** Both value-trims (armed path and
  inline) could stop stripping quotes and commas, one mutant per character, so a seven-character
  value read as nine and was redacted. No fixture had ever put a *short* value in quotes after a
  sensitive key.
- **`substantial` could convict by length alone** (`-> true`, `&& -> ||`): an all-digit value —
  the measurement the rule exists to keep — after a sensitive key had no fixture either.
- **The entropy length floor had no boundary row**, and my first kill for it was wrong: a dotted
  long token is rejected by the charset gate under both codes, so the "dot exemption" row saw
  nothing. The hand-applied mutant staying green is what caught it — M17's fixture-never-reaches
  lesson, this time in a guard being written, found only because every kill here is re-applied
  before being believed. The real killer is a 39-character opaque run: under the flip the floor
  collapses and it gets entropy-checked.

**Four are equivalent, and the honesty check ran both ways.** The `!= -> ==` flips in the two
key-cleaners change only which edge characters are trimmed before a `contains()` against
`SENSITIVE_KEYS` — and end-trimming cannot destroy an internal substring match in either
direction, dashed keywords included. One was hand-applied against the *full* suite and survived,
confirming the analysis rather than assuming it. Named residue, not fake guards: a fixture that
kills them does not exist.

Every row asserts both halves of M27's seam — text unchanged *and* `removed == 0` — because on
this module a wrong count is not bookkeeping, it is a redaction the author was never told about.

**The confirming re-run was interrupted and the round does not lean on it.** Launched at load
2.6, killed mid-build when the machine climbed back past 11 — an interrupted run is void as a
score (M17's rule, applied to our own instrument). The evidence of record is stronger per
mutant than a batch score: the complete first run, each of the nine killable survivors
re-applied by hand and seen red against its own guard, and one claimed-equivalent re-applied
against the *entire* lib suite and seen survive. The partial re-run corroborates where it got:
its one logged MISSED before the interrupt is one of the four predicted-equivalent trim flips.

## M54 · export.rs: one survivor, and it was the exporter's own receipt

**2026-09-02.** `tools/mutants.sh src/memory/export.rs` — the D11-sanctioned repository write
and the `--check` hash comparison. Two runs, because the first died at ENOSPC seventeen mutants
in when the machine's disk hit zero (a machine-wide event, not ours — the shared-cache clean
another session ran had already been refilled by an unidentified consumer): an interrupted run
is void, so it was redone from a cold cache once space was freed. The 16/16 the void run
managed agree with the clean run, which is corroboration and not evidence.

**Clean run: 29 mutants in 2m — 25 caught, 3 unviable, 1 missed, zero timeouts.** The renderer
and the drift check held (the D90-family containment tests and the drift truth tables did their
work). The survivor is the now-familiar seam, third sighting: **`written += 1` in
`write_export` could become `*=` — zero forever — because no test had ever asserted the
returned count.** Files written while the person is told none were, on the one path that
authors into a repository: a wrong "0 exported" sends someone re-running a command that already
worked. Guarded with a two-file fixture asserting the count *and* one body on disk; the mutant
re-applied and seen red, revert byte-identical.

The pattern now has three instances this week — `Redacted.removed` (M27), the capture marker
(M51), and this — and one shape: **a counter whose writer works and whose only reader is a
human report.** The find-unread-fields tool cannot see it (the field IS read — by the print),
and only a count assertion at the caller's distance catches it.

## M55 · The tail in one pass: eight modules, 360 mutants, and the crate-wide inventory closes

> **Correction, same day, by another session.** This pass covered every module the *record*
> named, which is not every module in the crate. `cargo mutants --list` finds **79 mutants in
> `src/memory/index.rs` and 61 in `src/main.rs`**, and neither file has ever appeared in a round —
> this run's own `mutants.out/mutants.json` lists eight files and neither is among them.
> `src/lib.rs` and the `memory.rs` facade are genuinely exempt: both generate zero mutants, being
> re-exports. So the title's claim, and `CHANGELOG.md`'s, were two files short — one of them the
> binary carrying D9's exit-0 guarantee, which is behaviourally guarded (sixteen tests in
> `tests/hook_safety.rs` assert `code == 0`) but has never been systematically mutated. The gap is
> recorded rather than quietly closed, because **a completeness claim is the one kind that stops
> the next person looking.**

**Modules:** `src/memory/id.rs`, `src/memory/text.rs`, `src/memory/topics.rs`,
`src/memory/config.rs`, `src/address.rs`, `src/duration.rs`, `src/version.rs`, `src/error.rs` —
spelled out because the prose below named them with brace expansion, which no parser can read
and which is why `tools/check_mutation_coverage.py` reported all eight as never mutated.

**2026-09-02.** `tools/mutants.sh` over every module never exhaustively mutated —
memory/{id,text,topics,config}, address, duration, version, error — 1,912 lines in one
invocation: one cold build, one baseline, deliberately, because the machine hit ENOSPC three
times today and disk churn is the scarce resource. **360 mutants in 66m: 305 caught, 10
unviable, 45 missed, zero timeouts.** topics.rs and version.rs came back clean.

The 45 clustered into stories, each now a truth table or a pinned vector:

- **The calendar had no round-trip.** Eight arithmetic mutants in Hinnant's civil-date
  conversions survived because nothing drove the pair as an identity; a sweep across two
  million days plus the epoch anchor kills any single flip.
- **`content_hash` could degrade from XOR to OR and export staleness would read stale as
  current.** Pinned to FNV-1a's published vectors — an analytic kill no fixture family matches.
- **Every render boundary was off-by-one-able**: `age` and `humanise` at 60s/60m/24h/365d and
  90s/90m/48h, the slug cap at exactly 48, `parse_ts` at exactly ten bytes. Exact-boundary rows
  in both directions, M27's discipline.
- **The id grammar's guards** (`parse_id`'s three-part exclusions, `split_id`'s emptiness) and
  **the topic charset chain** each get row-per-operator tables.
- **Two more env shells got the M51 seam extraction**: `threshold` and `skip_tools` were
  untestable behind `std::env::var`, and their parse/refusal/default rows now run injected.
- **`Error::causes` was printed and never read** — the "caused by:" lines the binary shows on
  every failure could be fabricated or empty; one wrapped-source row reads them.

**One mutant is equivalent, and one equivalence claim of mine was wrong — in the good
direction.** `age`'s negative-delta guard is unreachable in effect (every negative delta also
renders "just now" through the `mins < 1` arm; verified by hand against the full suite) and is
kept as a fail-safe first decision with the reasoning in the test. The `safe_component` flip
isolating `'-'` — a character whose replacement is itself — looked equivalent by the same style
of reasoning, and hand-applying it reddened the suite: the conjunction also un-whitelists `'_'`,
which mangles visibly. The lesson is the session's oldest one pointed at its own analysis:
reasoning says "plausible", only the applied mutant says "confirmed".

Fourteen representative kills hand-applied and seen red, one equivalence hand-confirmed, every
revert byte-identical. No batch machine re-run — three ENOSPC events today make per-mutant hand
verification both the stronger standard and the only responsible one.

## M56 · The two files the inventory missed, and a counter seam's fourth sighting

**2026-09-02.** `tools/mutants.sh src/memory/index.rs src/main.rs` — the two modules M55's
"crate-wide inventory closes" did not cover, run the same day the claim was corrected. Load had
fallen from 45 to 7.1 and three foreign `xtask` builds had finished; `cargo` was claimed on the
board first and no other run overlapped. **140 mutants in 18m: 91 caught, 34 missed, 15 unviable,
zero timeouts.**

**Disclosed, because the header forbids it and I did it anyway:** the run was invoked through a
pipe (`| tail`), which is trap 2 in `mutants.sh`'s own header — the pipe reports *tail's* status,
so a baseline failure would have printed exit 0 beside a run that tested nothing. It did not
happen here (the baseline demonstrably passed and 140 mutants were tested), and every count in
this entry is read from `mutants.out/`, never from the exit status. The trap is real and the
mitigation was luck, not design.

### The dominant cluster is the shape catalogued four hours earlier

**Fifteen of the 34 are `+=` on `IndexStats` counters** — `scanned`, `indexed`, `unchanged`,
`unreadable`, `pruned`, in both `sync_dir` and `reindex`'s aggregation. Every one could become
`*=` and stay zero forever, because nothing asserted the numbers themselves. `amb memory index`
would print `0 scanned · 0 indexed` over a vault it had just walked in full, and the `--json`
lane — a declared stable contract — would tell a script the same.

That is **the fourth sighting** of the seam M27 named and this session catalogued at three
(`Redacted.removed`, `capture.rs`'s marker, `export.rs`'s `written`): *a counter whose writer
works and whose only reader is a human report*. The catalogue entry was written into `CLAUDE.md`
at 15:15 and this run found the next instance at 16:00, which is the strongest available evidence
that the shape is a shape. It is also the same struct D45 was written about — there the *reader*
was missing so a 501-note vault reported itself empty; here the reader exists and the
*assertion* was missing. Same field, opposite half, four months apart.

### Two more, neither a counter

- **`excerpt_of` could return `None`, `""` or `"xyzzy"` and nothing went red.** D88 records that
  `recall` matches `body_excerpt` — so this is not a display convenience, it is the corpus search
  actually runs against, and emptying it deletes most of what memory can find while every note
  stays present and every existing test passes. Its 240-character cap survived `==`, `<` and
  `>=` as well: fixtures at exactly 240 and 241 are the only pair that separates the four.
- **`render_history`'s `&&` could be `||`**, which makes a note *with* lineage print
  `stands alone — it replaced nothing, and nothing replaced it`. That sentence exists because
  this project's failures are silences (U5); the mutation turns the cure into the disease.

### What is guarded, what is named, and what is left

**Eighteen killed, each mutant hand-applied and seen red, every revert byte-identical.** Six
tests: the index receipt (counts *and* the rendered line), the three unreadable paths, the
scope-correction rule, `excerpt_of` with its exact cap, and a truth table for "stands alone".

**Two are unreachable on this host and it is not a `cfg` gate.** `stats.unreadable += 1` in the
`file_stem` arm needs a filename that is not UTF-8, and **APFS refuses to create one** —
`EILSEQ`, verified by trying. The code compiles here; the *input* is what macOS forbids. That is
a third category beside "real survivor" and "not compiled here", and `cfg_phantoms.py` correctly
calls it real, because it classifies by `cfg` and there is no `cfg`. The test is written and
gated to `target_os = "linux"`, so CI's other leg is the assertor — which is why the suite is now
589 on macOS and 591 on Linux.

**Fourteen are left standing and named rather than quietly dropped:** the cycle-break `==` in
`history` (two) and `validate_links` (one), `sync_dir`'s `CANDIDATE` match guard and its
`Scope::Project` arm (two), and nine in `main.rs` — three `delete !` in `run`, three on
`report_plan`'s `> 0` (M27's guard-over-a-count, again), two in `hook_deliver` and one `+` in
`run_memory`. `main.rs` is the binary carrying D9's exit-0 guarantee; it is behaviourally guarded
by sixteen `code == 0` assertions in `tests/hook_safety.rs` and has still never had its
*decisions* mutated, which is the gap this run measured rather than closed.

## M57 · The completeness claim made derivable, after being made falsely three times

**Modules:** `src/memory/note.rs` — re-run, 27 mutants, 24 caught, 3 unviable, **0 missed**
(M27 measured 24 viable with 1 missed on 2026-08-30; that survivor is gone).

**2026-09-02.** Not a hardening round. `tools/check_mutation_coverage.py` exists because
*"every module has been mutation-tested"* was asserted three times in one day and was wrong all
three, by two sessions and one tool:

1. **M55 claimed the crate-wide inventory closed.** It was three files short —
   `memory/index.rs`, `main.rs`, `memory/note.rs` — because it enumerated from the rounds its
   author remembered running rather than from the filesystem. The claim reached CHANGELOG, a
   commit message and the board before anyone checked it.
2. **The correction that caught it was itself short by one.** It spot-checked the two files it
   suspected, found them genuinely uncovered, and did not run the set-difference; `note.rs`
   sat unmentioned in both the wrong claim and its correction. (M56 then closed the two, which
   is when the inventory *actually* closed.)
3. **The checker written to end this made the same error on its first run.** Its parser read
   one of the record's three formats — the `tools/mutants.sh <paths>` invocation — and M27
   records its four modules in a **score table** instead. So it reported `note.rs` as never
   mutated, and that output went out on the board before being checked against the document
   it had just parsed. A second bug in the same function stopped a `**Modules:**` block at its
   first line and reported five of one round's eight modules as uncovered.

**The shape, which is the reusable part.** Every instance is a *completeness* claim — "every",
"all", "closed" — and completeness is the one kind of claim that cannot be verified by looking
at what you have; it can only be verified by set-differencing against what exists. Each of the
three parties looked at a list of things that *had* happened and concluded nothing was missing.
The record's three formats made that easy, but the format was the occasion, not the cause.

**What the script does, and deliberately does not do.** It derives the covered set from all
three forms plus an explicit `**Modules:**` block, subtracts two zero-mutant exemptions that
carry both a reason and the command that verifies them, and prints the difference. **An
uncovered module is never a failure** — mutation is not a commit gate and `mutants.sh`'s header
says why. What fails is a current-state document asserting closure while the set-difference
disagrees; `docs/MEASUREMENTS.md` is excluded from that policing because it is an append-only
log where M55's own title stands with its correction beneath it. Negation-aware, so an honest
*"the inventory does not close yet"* passes.

Proven by its truth table rather than by its first green run: uncovered-and-silent passes,
uncovered-plus-claim fails naming file and line, negated-claim passes, and the truthful state
passes. The middle row is the presence row — without it the other three prove only that a
script that always exits 0 exits 0.

## M58 · The nine survivors in the binary, and the extraction that made three of them testable

**2026-09-02.** M56 left fourteen mutants standing and named them; nine were in `src/main.rs`,
the file D9's exit-0 guarantee lives in. No new mutation run was needed — M56's report already
names each one — so this is the guarding pass, every mutant applied by hand against the fixture
written for it and reverted byte-identical.

**All nine are dead.** The suite goes 589 → 594 on macOS, 591 → 596 on Linux.

### Three of them could not be tested where they lived, which is D78's rule firing again

`report_plan`'s retry line is guarded by `done.retries > 0`, and **all three relaxations
survived** — `>= 0` announces contention on every quiet install, `== 0` and `< 0` silence a real
one. The line's own comment says why that matters: staying silent "would make a contended
settings file indistinguishable from a quiet one."

It could not be asserted from a test, and not because nobody had tried. A retry happens only when
another process writes `~/.claude/settings.json` between this one's read and its write — a race
a test cannot stage. The function was in `main.rs` for D78's exact reason, too: it needed `Cli`,
and the binary is where `Cli` already was.

So the human half moved to `hooks::render_applied`, pure, taking `(&Applied, &Path, bool, &str)`
and returning a `String`; the JSON lane stayed in the binary, where the stability contract over
it is asserted by driving the binary. The truth table over `retries` then kills all three, and
two lines that had never been asserted at all — the unlocked-write warning and the no-op —
came with it. **`main.rs` is 34 lines shorter and the rule it broke is kept again.**

### The other six were reachable and simply unasserted

- **`!taken.conflicts.is_empty()`** guards the sentence telling an agent what a conflict *means*.
  Claims are advisory (D5), so that line is the entire remedy the tool offers — and dropping the
  `!` prints it on every uncontended claim, where there is nobody to message, while silencing it
  on the one claim that has a holder to warn about.
- **`!all` twice, in `snapshot`.** One `!` chooses what `messages::inbox` collects, the other
  what the document calls itself (`Unread` / `All mail`). Either could be inverted alone, and
  both are M28's shape: a file headed `Unread` listing acknowledged mail, or one headed
  `All mail` missing everything read. The fixture has to acknowledge a message, or the two
  scopes are identical and the test passes under both mutants (M17).
- **`is_start && mode == "monitor"`**, two mutants. The `amb watch` hint is advice to run a
  blocking command under a Monitor tool; `||` appends it to every `Stop` as well, `!=` sends it
  to exactly the installs that cannot use it. Three rows separate them, and the positive row
  proves the line is reachable rather than absent for some other reason.
- **`st.stale.len() + st.missing.len()`** is **the counter seam's fifth sighting**, on the
  `--check` lane of the same command whose `written` was the fourth (M54). As `*` it prints
  `0 exported decision(s) ... disagree with the vault` beside exit 65, because in ordinary drift
  exactly one of the two is zero. The existing test asserted the exit code and never the
  sentence — which is the seam stated precisely: the number reaching the person was the part
  nothing checked.

### What this leaves

M56's other five are in `memory/index.rs`: the cycle-break `==` in `history` (two) and
`validate_links` (one), and `sync_dir`'s `CANDIDATE` match guard with its `Scope::Project` arm.
`main.rs` is clear. The crate's inventory of *rounds* was closed by M57's derived check; this
closes the binary's inventory of *survivors*, which is a different question and the one that
decides whether a round produced anything.

## M59 · The last five, and two fixtures that reached the wrong arm before they reached the right one

**2026-09-02.** M56's remaining survivors, all in `src/memory/index.rs`. No new mutation run —
M56 names each — so this is the guarding pass, every mutant applied by hand and reverted
byte-identical. **All five dead.** 594 → 597 tests on macOS, 596 → 599 on Linux. With M58's nine,
**M56's thirty-four are now eighteen guarded at the time, plus these fourteen, with two named
unreachable on this host** — the round is closed.

### The two cycle breaks, and why the test already sitting on them could not see them

`history` walks a lineage in both directions and breaks on
`descendants.iter().any(|s| s.id == step.id)`. Flipped to `!=`, `any` is satisfied the moment the
list holds anything *different* from the new step — so the walk stops after one hop on every
ordinary chain, and a four-note lineage reports one ancestor instead of three.

`a_supersession_cycle_terminates_the_walk_without_flooding_it` runs directly over this code and
cannot distinguish it. Its fixture is a two-note cycle: the honest walk collects two, the mutant
collects one, and both of its assertions — that `nest/b` is present, and that the length is at
most two — hold either way. **M17's shape, in a test written specifically about these guards.**
The fixture had to become a *chain* rather than a cycle, because the defect is that a chain is
truncated, not that a cycle runs away.

### The scope match, where the fixture reached the wrong arm twice

Two rules sit on one `match`, and both survived.

`_ if kind == CANDIDATE => String::new()` is D50/D81 — a candidate carries the empty scope,
because SQLite permits NULLs in a composite primary key and does not compare them equal, so the
absence has to be `''`. Replaced with `false`, a candidate is filed under a project scope, where
nothing that looks for it will look.

`Ok(Scope::Project(p)) => safe_component(&p)` is containment, and deleting it still *compiles* —
the next arm hands back the project name unsanitised. The rule was asserted at a different layer:
`a_hostile_scope_name_stays_inside_the_vault_whatever_the_kind` tests `vault_dir`, and `sync_dir`
is what actually writes the row (D90's shape — one rule, two layers, one assertion).

**Getting a fixture onto that arm took two attempts, and the first failure is the more useful
one.** `../../../etc` contains a `/`, so `parse_scope` refuses it and the `Err` arm sanitises:
the mutant changes nothing for that string and a test using only it passes. The one string that
reaches `Ok(Scope::Project(p))` while still being a traversal component is **`..`** — `parse_scope`
refuses only `/`, `@` and `#`, so `..` is a perfectly ordinary project id, and `safe_component`
turns it into `unknown` because it trims dots. Both are now in the fixture with a comment naming
which arm each takes.

And a third correction before that: the note's frontmatter has to declare a *different* scope than
the directory, or the "disk outranks status" correction never fires and the row keeps the
frontmatter's spelling under both versions. **Three fixtures in one test, each a case of the
input never reaching the branch** — which is what makes hand-applying every mutant the standard
rather than a formality. Each was found by the mutant surviving, not by reading.

## M60 · The seam audit: three decisions that no test could reach, and one of them was a kill switch

**2026-09-02.** The audit item outstanding since the round-at-569 report, done as reading rather
than as a mutation run. **The question is not "is this function tested" but "can a test reach the
decision at all"** — and the mechanical form is: grep `std::env::var` across the library, and for
each one ask whether the value it turns into a decision is reachable without setting process
environment, which a parallel test runner makes unsafe.

Six shells hold a real decision. **Three passed and three did not.**

### The one that matters: D49's kill switch accepted three spellings and one was tested

`AMB_MEMORY_PROMOTION` disables the promotion pipeline. The README's environment table publishes
**`0`, `off` and `false`**; `the_promotion_pipeline_has_a_kill_switch` drives the binary with
`off`. Replacing the match with `Ok("off")` alone — deleting the other two published spellings —
**left the entire suite green.**

So a person who read the documentation, wrote `AMB_MEMORY_PROMOTION=0`, and expected promotion to
stop would have got promotion. On the mechanism **D49 names as the response to approval degrading
into a rubber stamp** — the thing you reach for precisely when the pipeline is misbehaving.

This is D58's shape arriving from a new direction. D58 is about a mechanism that cannot reach the
party positioned to use it; here the mechanism is documented, reachable, and *two thirds inert*,
and the documentation is what makes the gap dangerous rather than harmless.

### The other two

- **`broadcast_horizon` had one caller and no test of any kind.** Its docstring argues a real
  decision — an unparseable value falls back rather than failing, because D9 puts delivering mail
  above honouring a typo in an environment variable — and D96 sets the default. Nothing asserted
  either. `unwrap_or(BROADCAST_HORIZON)` relaxed to a zero duration puts the cutoff at *now*, so
  **no broadcast is ever delivered**: a silence on the delivery path, which is this project's
  signature failure, in the one number that decides how far back a broadcast reaches.
- **`vault_path` holds D35 in its first two lines and neither was asserted.** "`AMB_VAULT` has no
  default. Unset means memory is off" — and a variable set to *nothing* has to mean the same,
  because `PathBuf::from("")` is the working directory. Delete the emptiness check and memory
  switches **on**, pointed at whatever repository the session is sitting in, which is a D11
  question as much as a D35 one.

### What passed, and why that is the useful half

`threshold` and `skip_tools` already carry their seams — `threshold_from` and `parse_skip_list`,
both extracted in M55 for exactly this reason. `session_pid` has `pid_from_socket` beneath it and
an `AMB_SESSION_PID` override that `identity_e2e.rs` drives. `db_path` is exercised by every e2e
test in the suite.

**The three that failed and the three that passed differ by one thing: whether somebody had
already pulled the decision out of the shell.** Not by importance, not by age, not by how much
the code is used — the kill switch is named in a decision record and the horizon is on the
delivery path. So the audit's rule generalises past `env`: **a decision that can only be reached
by arranging the process's own environment will not be tested, and the fix is extraction rather
than a cleverer test.** Every guard added here is a truth table over an injected argument, and
each was confirmed by mutating the decision and watching it redden.

## M61 · vendors.rs: the descriptor module, clean on its first pass

**Modules:** `src/vendors.rs`

**2026-09-02.** D111's new module, run the moment `tools/check_mutation_coverage.py` named it —
the checker written this morning doing the job it was written for, on code committed an hour
earlier. **Clean: every viable mutant caught, zero missed, zero timeouts.**

The reason is worth recording because it is an argument for the architecture rather than for the
tests: a descriptor module is mostly `const` data, and data has no surviving mutants when
something asserts the values. What logic it has — detection precedence and the blank-id filter —
was written with the environment injected (M51's rule) *before* a mutation pass could find it
untestable, which is the first time on this project that lesson was applied ahead of the finding
rather than after it.

## M62 · vendors.rs again, because "has a recorded round" is not "was mutated in its current form"

**Modules:** `src/vendors.rs`

**2026-09-02.** M61 gave this module a clean pass at **12 mutants**. It then grew the manifest
loader, the parser and its refusals, detection, and `tool_matcher` — **39 mutants**, more than
triple — and `tools/check_mutation_coverage.py` went on printing *the inventory IS closed*,
because it answers "has this module ever had a round" and cannot answer "was it mutated in the
form it is in now". That is the blind spot of a per-file completeness claim, and it is the same
shape M57 was written about one level down: a set-difference over *files* cannot see time.

**39 mutants: 36 caught, 3 missed** — and all three were in code written hours earlier, which is
the argument for re-running rather than trusting the closed inventory.

- **Two empty-string gates.** `!v.is_empty()` in the manifest parser could become `true`, and
  nothing reddened: every fixture *removed* a key and none set it to `""`, which take different
  branches (M17's shape, again, in a table written to prevent it). Not cosmetic —
  `"config_dir": ""` makes `home.join("")` the config directory, so `amb install` would write to
  `$HOME/settings.json`, a file that belongs to something else or to nothing.
- **`problems()` could return an empty list forever**, and doctor would report a healthy vendor
  set while silently ignoring every refused manifest. The doctor test asserted `vendors_check`
  against hand-built `Problem` values, so it proved the *renderer* and never the path from a bad
  file on disk to a line on a screen — D90's shape, inside the one feature whose entire purpose
  is that a skipped file is not a silence. Closed by an e2e through the real binary, asserting
  both halves: the refusal is named *and* the good manifest beside it still loads.

**The fixture for that e2e was itself wrong on the first run**, and the failure is worth keeping:
a manifest carrying only an `id` is refused for its missing `session_env` and never reaches the
`config_dir` check the test was about. The parser refuses on the first thing missing, so a
fixture has to clear the earlier gates to exercise a later one — M17's rule, broken minutes after
re-reading it in the paragraph above.

## M63 · five doc comments describing the wrong function, and two detectors each blind to what the other saw

**Modules:** `src/claims.rs`, `src/doctor.rs`, `src/hooks.rs`, `src/identity.rs`, `src/main.rs`

**2026-09-02.** A cleanup pass over the vendor arc found `summarise_by_project` carrying, as its
rustdoc, the three paragraphs that describe `summarise` — and `summarise`, the function that
actually does the grouping, carrying none. The mechanism is mechanical and dull: a new item was
inserted *between* an existing `///` block and the function it documented, so the block attached
itself to the newcomer. Nothing fails. `cargo doc` renders happily. The result is the failure mode
this project already has four entries about — **a false comment, which is worse than an absent
one** — sitting on public API, where the reader most likely to be misled is the one who came to
the file for the first time.

**Five instances, not one.** Two were introduced by the vendor arc and three predate it:

| item that lost its doc | item that inherited it | file |
|---|---|---|
| `claims::summarise` | `claims::summarise_by_project` | `src/claims.rs` |
| `claims::take` | `claims::end_session` | `src/claims.rs` |
| `doctor::size_check` | `doctor::vendors_check` | `src/doctor.rs` |
| `identity::register` | `identity::MAX_NAME` | `src/identity.rs` |
| `hooks::settings_path` | `hooks::settings_sources` | `src/hooks.rs` |

Two of them read as outright contradictions rather than as mere misplacement: `end_session`'s
rustdoc opened *"Take or renew a claim. Never blocks, never fails on conflict."* — the summary of
the function that **writes** claims, on the one that **lapses** them — and `MAX_NAME`, a `usize`,
opened *"The roster upsert, reporting anything it displaced."*

**The reusable part is that neither detector could have found all five.** Two were written and
both were run:

- **By history** — for each function, was it documented at some earlier commit and bare now?
  Precision 4/4, and it found `take`, `register`, `summarise` and `size_check`. It cannot see
  `settings_path`, whose loss predates the range, and as a *gate* check it has nowhere to stand:
  the pre-commit hook examines a working tree, not a range.
- **By text** — inside one `///` block's first paragraph, a line ending a sentence followed
  directly by a line opening a new capitalised one. It found `take` and `settings_path` — including
  the one history missed — at **2 real out of 11 candidates**, because a genuine two-sentence
  summary looks identical. Too noisy for the gate.

The union is five; each alone is four or two. This is `CLAUDE.md`'s "count the layers" arithmetic
arriving in a new place: two instruments with different blind spots, and the completeness claim
belonging to neither.

**What was rejected, and why it is a rejection rather than an omission.** `#![warn(missing_docs)]`
catches this class permanently and at compile time — the robbed item always ends up bare. It was
measured rather than assumed: **242 warnings**, of which **180 are struct fields and 29 are enum
variants**, leaving 33 items of the kind at issue. Turning it on means either documenting 180
self-describing fields or shipping an `#[allow]` that switches the lint off exactly where it would
have worked. Neither is a cleanup pass's call to make quietly, so the lint is recorded here as a
decision someone should take deliberately, with the backlog counted, instead of being bolted on.
The five instances are fixed; the mechanism is not yet guarded, and this paragraph is the only
thing saying so.

## M64 · the claims surface was dead on Gemini, and the constant guarding it was the one D111 called an optimisation

**Modules:** `src/claims.rs`, `src/vendors.rs`, `src/main.rs`, `src/doctor.rs`

**2026-09-02.** D111 moved the memory lane's tool matcher onto the descriptor because
`Read|Edit|Write|NotebookEdit` is Claude's vocabulary and Gemini's is `read_file`, `write_file`,
`replace`. `Vendor::tool_matcher`'s own doc records the reasoning and ends: *"the matcher is an
optimisation rather than the guard."* **The guard was `claims::EDITING_TOOLS`, it was still
Claude's four names, and it was never moved.**

It is also the *only* filter on that path. `Mode::events` installs the tool-completed hook with no
matcher at all, so nothing upstream narrows it: every `AfterTool` payload reached `edited_path`,
carried `write_file`, missed the list and returned `None`. **A Gemini session recorded zero claims,
ever, and said nothing** — one of `amb`'s three surfaces inert on a vendor the README advertises,
in a shape indistinguishable from a project where nobody edits anything. Measured against the
installed `@google/gemini-cli` 0.55.1 package rather than its docs: `write_file` 89 occurrences,
`read_file` 96, `read_many_files` 50, and `"Write"`, `"MultiEdit"`, `"NotebookEdit"` **zero each**.

`Vendor::edit_tools` now carries it, required in a manifest rather than defaulted — a descriptor
that cannot say which tools write files describes a vendor whose claims surface is dead, and
defaulting to Claude's names is the precise mistake being fixed. The guard is a three-row truth
table: Claude's `Edit` claims, Gemini's `write_file` claims, and **Claude's `Edit` fired at a
Gemini session claims nothing** — the third row is what fails if someone "fixes" this by unioning
every vendor's vocabulary. Against the code as it shipped, the guard reports the defect exactly
backwards, which is the clearest statement of it available: `["src/claude.rs",
"src/not-gemini-vocabulary.rs"]` where `["src/claude.rs", "src/gemini.rs"]` was expected.

**What generalises is where the fix stopped.** D111's own generalisation sentence reads *"read the
other vendor's own words for everything you are about to **write into its file**"*, and every
finding still open after it is on the **read** side — what `amb` reads out of a vendor's runtime:
tool names in a payload, event names in a payload, environment variables. The seam was drawn
around writes because writes were what the install command did, and the sentence that recorded the
lesson inherited that scope. A generalisation stated in terms of the operation you happened to be
performing will not cover the operation you were not.

**Two more instruments, from the same audit, both left standing with reasons:**
`memory::events::lane_caveat` explains the receipt as *"`PreToolUse` fires only on a Read/Edit/Write
tool call"* — the one sentence that stops D74's two lane ratios being read as a comparison, in a
vocabulary a Gemini reader will dismiss. `hooks::event_name` degrades a malformed payload to the
literal `"SessionStart"`, documented as *"every consumer treats that as the ordinary banner case"*;
consumers now compare against `vendor.events.session_start`, so that contract is true only because
both shipped vendors happen to spell it the same, and false for any manifest that does not. Both
are real; both change rendered output or a public signature, and neither is a cleanup pass's call.

## M65 · a field with no reader at all, cleared by the gate check written to catch exactly that

**Modules:** `src/vendors.rs`, `src/doctor.rs`, `tools/find_unread_fields.py`

**2026-09-02.** `Vendor::label` was added by D111 with the docstring *"The product's own name, for
a line a person reads."* Nothing read it. Every `.label` in production was
`hooks::label_of` — a free function — or `Nearness::label()` in `memory/inject.rs`; all four real
mentions were inside `mod tests`. This is D23, D39 and D45's defect, and the module's own header
argues against it in advance: *"a speculative field is a field nothing reads"*, citing that
`find_unread_fields.py` is in the gate.

**The script reported `Vendor label reads=7` and passed.** It counts a field by *name* across the
corpus, so two unrelated `label`s in other modules cleared it. Its docstring states its error
direction — *"it over-removes rather than under-removes, which for a reference count means a false
'nothing mentions it' — loud and checkable — rather than a false reassurance"* — and that is true
of its comment stripping and **false of its name matching**, which fails in the flattering
direction and silently. An instrument whose stated failure direction is safe in one subsystem and
unsafe in another is worse than one with no stated direction, for the same reason a false comment
beats an absent one: the sentence is what stops you checking.

`label` now has its reader — `doctor`'s vendors check names the vendors rather than counting them,
which is what the field was for. The script's name-collision blind spot is **recorded and not
fixed**: making it type-aware is a rewrite of a deliberate heuristic, and the honest interim is
that a field whose every mention is in `mod tests` currently passes. That is the next thing to
build here, and the count in its summary line is what should stop reading as a guarantee.

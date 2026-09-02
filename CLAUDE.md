# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`amb` — a message bus for concurrent Claude Code sessions on one machine: direct messages,
project-wide broadcasts, and advisory file claims, across more than one repository. A Rust CLI
over SQLite. **One static binary, one database file, no daemon.**

It is installed machine-wide: `amb install` writes hooks into `~/.claude/settings.json`, and from
then on every session on the machine registers itself, receives mail directly in its context, and
records claims on files it edits. Agents never poll.

## Commands

`rustup` has **no global default on this machine**, so `cargo` only resolves inside a directory
containing `rust-toolchain.toml`. A bare `rustc --version` failing elsewhere is not a broken
install.

```bash
cargo build                      # debug
cargo build --release            # bundled SQLite compiles; ~15s cold
cargo test                       # all 604 tests (606 on Linux)
cargo clippy --all-targets       # lint policy lives in Cargo.toml, not a CI flag
cargo fmt                        # run before finishing; the gate runs `cargo fmt --check`
./tools/verify.sh                # every gate check in one command, ~30s after a change (D70)
./tools/install.sh               # build AND update every copy the hooks invoke (D94)
./tools/bench.sh                 # every measurement harness; ~17s, deliberately not in the gate
./tools/mutants.sh src/claims.rs # mutation-test one module — run nothing else meanwhile (M17)
./tools/eyeball.sh               # what a session actually sees, against a COPY of the real board
python3 tools/cfg_phantoms.py    # mutants.sh runs this itself; separates "not compiled here" from "untested"
python3 tools/check_secret_literals.py   # in the gate; see its header for why fixtures use concat!
```

**The last three are separate from the gate on purpose and each says why in its own first line.**
`bench.sh` verifies that a harness still measures what a document says it measures and asserts
nothing about values; `mutants.sh` forces a private `CARGO_TARGET_DIR`, because a mutation result
produced while anything else was building is **void, not weak** — that has happened, and the
polluted run reported a caught mutant as missed (M17); `eyeball.sh` runs the real binary against a
copy of the real board and prints what a person gets, because **tests and mutation both work on
code against fixtures and neither can see a defect in the composition of correct parts** — M24 and
M29 were both found that way and nothing else could have found either (M32).

**A MISSED row can mean "not compiled on this host", and that is now classified rather than
remembered.** `cargo mutants` does not evaluate `#[cfg]` and says so in its own Limitations
chapter, so mutating a `#[cfg(target_os = "linux")]` function on macOS prints MISSED for code the
binary never contained — 16 of `db.rs`'s 29 missed rows in one run (M46). `mutants.sh` now ends by
calling `tools/cfg_phantoms.py`, which splits the two against the host's real flags and **refuses
rather than guesses** on a predicate it cannot model. Do not "fix" this with the documented
`#[cfg_attr(not(target_os = "linux"), mutants::skip)]` annotation: cargo-mutants does not evaluate
the `cfg_attr` condition either, so that skips the mutant on **every** platform including the one
where the code is live — the guard would remove exactly the coverage it appears to protect.

**Turn the gate on once per clone:** `git config core.hooksPath .githooks`. It runs before every
commit, collects every failure rather than stopping at the first, and `AMB_VERIFY_SKIP=1` bypasses
one commit loudly. `.github/workflows/ci.yml` **ran for the first time on 2026-08-31**, when the
repository was published — Linux and macOS, both green. It is a second net, not the gate: CI fires
after a push, the hook fires before a commit exists. Anything added to one belongs in the other,
and they had already diverged once (D70).

```bash
amb doctor                       # stale hook binary, schema drift, whether each lane is firing
```

Running a single test:

```bash
cargo test --lib a_partial_segment_is_not_a_prefix   # one unit test by name
cargo test --lib claims                              # one module's unit tests
cargo test --test delivery                           # one integration suite
cargo test --test claims_e2e two_agents_can_hold     # one test in one suite
```

Driving the binary manually — `AMB_DB`, `AMB_AGENT` and `AMB_PROJECT` override the real board and
the session's identity, which is how every test isolates itself. (The full env surface — including
`AMB_BROADCAST_HORIZON`, which tunes D96's horizon — is tabled in the README; this file names only
what tests need.)

```bash
AMB_DB=/tmp/t.db AMB_AGENT=alice AMB_PROJECT=nest cargo run -- send @ --subject s --body b
echo '{"hook_event_name":"Stop"}' | AMB_DB=/tmp/t.db AMB_AGENT=alice cargo run -- hook turn
AMB_HOOK_DEBUG=1 ...             # hooks swallow all errors; this prints them to stderr
AMB_VAULT=/tmp/v cargo run -- memory observe --title t --learned l   # memory is off without it
```

## `docs/DECISIONS.md` is the specification

D1–D110 are **settled**, and each records *what was rejected and why*. Read it before proposing a
design change — the argument has probably already been had. `docs/OPEN-QUESTIONS.md` holds what is
genuinely undecided; when one is settled, delete it there and record it as a new decision.

**Several decisions are negative, and negative decisions leave no trace in the code.** They read
as omissions and get "helpfully" fixed. These are deliberate:

- **No decisions, findings or ADRs in the bus** (D2). Those live in the repos they govern.
- **Claims are advisory. No fencing tokens, nothing blocks** (D5). `claims` has
  `PRIMARY KEY (path, agent)`, not `PRIMARY KEY (path)` — exclusivity is not representable, on
  purpose.
- **No outbox** (D10). `amb send` is the only write path; the outbox pattern needs a relay daemon.
- **`amb` never writes inside a repository *on its own initiative*** (D11). No `.msgboard/`, no
  rendered inbox, and `amb snapshot` refuses such a path outright. `amb memory export` is the one
  thing that writes into a repo, and only because a person ran it (D49). Read D11 as a rule about
  initiative, not about bytes — the flat version of this sentence was wrong once export shipped.
- **No findings-inbox** (D16). `amb memory promote` exists, **revised by D49** — behind a human
  gate, one candidate per offer, derivations shown, and it never writes without `--yes`.

## Architecture

### Thin binary over a library

`src/main.rs` parses arguments, calls the library, and maps an `Error` to a `sysexits`-style exit
code. **No logic should live there** — that is what lets tests exercise real code paths instead of
shelling out.

**The rule was broken and is now kept, which is worth knowing because it broke quietly** (D78).
`memory_for_session` held D45's declined-rebuild guard, `observe_edit` held D19's renew-suppression
rule, and the `tool_name`/`file_path` extraction was written out three times — all on the hook
path, all untested, in a file with no tests at all. Nothing failed; the functions were correct.
They are now `memory::index_is_behind`, `claims::conflicts_to_report`, `memory::failure_note` and
`hooks::tool_and_file`, each with a test that reddens when the rule is deleted.

**How it broke is the reusable part.** Nobody moved logic into `main.rs`. Each function arrived
there because it needed a `serde_json::Value` from a hook payload and the binary was where the
payload already was. The pull is toward whichever file already holds the argument, and it is
strongest exactly where the shell meets a schema someone else owns. Typed errors (`thiserror`) inside the library; the binary is the only place that
prints.

Exit codes are a contract a hook reads without parsing stderr: `64` usage, `65` no such
agent/message/claim/note, `69` board unavailable (locked or corrupt), `70` internal bug, `73`
file or directory could not be created, `78` misconfigured. `amb doctor` alone always exits 0.

**That contract only became true on 2026-08-31, and how it was false is worth keeping** (D97).
It covered the errors the *library* raises; `Cli::parse` terminated the process before `run` was
called, with clap's default of `2` — so the **commonest** usage errors, every mistyped flag and
missing option, exited outside the documented set while the rarer typed ones exited 64. `main` now
uses `try_parse` and maps the result itself.

**And `2` is not an arbitrary number to the process that invokes us.** Claude Code's hook runner
reads exit 2 as *blocking*: on `Stop` it prevents the session from stopping. `hook_main`'s exit-0
guarantee is written downstream of parsing, so a hook entry carrying an argument this build cannot
parse never reached it — the one thing D9 forbids, reachable through the stale-entry condition D69
and D94 record five times. A malformed hook invocation now exits 0 and says nothing, like every
other hook failure. **If you add an argument anywhere, that path is a hook-safety change, not a CLI
change.**

### Four addressing modes are one query

The central design claim. `messages` has **two nullable columns**, and the 2×2 over them is the
whole addressing model:

| `to_agent` | `to_proj` | meaning |
|---|---|---|
| agent id | *informational* | direct, to any project |
| `NULL` | `"nest"` | everyone in that project |
| `NULL` | `NULL` | everyone, everywhere (`@@`) |

`messages::inbox` is a single `SELECT` covering all four. If you find yourself adding a branch for
an addressing mode, the schema is being fought rather than used.

`to_agent` holds a **resolved agent id, never a display name** (D18). Resolution happens in
`resolve_recipient` before anything is written; an unknown name is an error, not a stored row.

### It is a log, not a queue

Read state lives in the `reads` table, not as a flag on the message. One broadcast row is consumed
independently by each recipient — so an agent that registers *after* a broadcast still receives it.
That is what makes `@project` address a **place** rather than a set of connected processes, and it
is the property no competitor has. A refactor toward "proper" queue semantics would silently
destroy it; `a_project_broadcast_reaches_an_agent_that_registered_afterwards` guards it.

### Delivery is hooks, not polling

`amb install` writes `SessionStart`, `Stop`, `PostToolUse` and `SessionEnd` hooks (the last
lapses the departing session's claims, D109). `Stop` rather than
`UserPromptSubmit` because the latter blocks the user's turn on a 30 s timeout.

Two hard requirements on `hook_main` in `src/main.rs`:

1. **It always exits 0**, whatever happens — a corrupt board, no identity, hostile stdin. Mail
   delivery must never break a session.
2. **It does nothing when no board exists.** The hook runs in every session on the machine,
   including ones that never use `amb`; it must not create a database for them.

Both are mutation-tested — delete either and a test in `tests/hook_safety.rs` goes red.

### Functional core, imperative shell

The decisions are pure and exhaustively tested without a filesystem; only the shell performs I/O.
`address::parse`, `duration::parse`, `claims::overlaps`, `claims::edited_path`,
`delivery::render_all`, and `hooks::plan_install`/`plan_uninstall` are all pure. The last two
matter most: they edit `~/.claude/settings.json`, which configures Claude Code for every project
on the machine — corrupting it breaks the user's whole tool, not just this one.

### Memory is a fifth surface, and it is off by default

`src/memory/` is a vault of markdown notes plus a **derived** index (D34–D43, Phase 1 of
`docs/AMB-MEMORY-IMPLEMENTATION-PLAN.md`). It was one 5,883-line file until D80; `src/memory.rs`
is now a facade that re-exports fourteen modules and carries the table saying which holds what, so
start there. Callers are unchanged — everything is still `memory::observe`.

Six things about it are load-bearing and read as omissions otherwise:

- **`AMB_VAULT` has no default. Unset means memory is off** (D35). Do not add one — a default
  vault path creates a directory the user never asked for and starts filling it.

- **A `capture` is never injected, and `INJECTABLE` is what enforces that** (D86). Failure notes
  from `PostToolUseFailure` are machine-written scrollback — title `"Bash failed"`, body raw tool
  output. They were 38.5% of the vault and **six of the eight notes in one real injection**, and
  they cannot be cited, so they only ever inflated the denominator of the ratio D59 retires the
  layer on. They stay indexed and searchable; they are excluded by *kind*, through the same
  constant that excludes candidates, because D51's finding was that a guard named in a comment
  while an unrelated filter did the work is correct by accident. `KINDS` partitions against
  `INJECTABLE`/`NON_INJECTABLE` and against `SEARCHABLE`, both asserted — a new kind cannot be
  added without deciding both questions.

- **`amb memory status` counts over the open measurement window, not over all time** (D87). The
  window is a row in `measurement_window`, written by `amb memory window --open`. Before it, D59's
  condition and D79's start date were prose nothing could evaluate: the only control was
  `--days N`, counted back from *now*, so a fixed instant was unsayable and the default counted
  everything — including `probe-drop`, a hand-run session that wrote 8 injections, could never
  cite, and was 14% of the denominator. **Do not make `--open` idempotent.** A window that resets
  by re-running the command is one that can be retried until it reads well; `--reopen` is the
  deliberate spelling.
- **The index stores no note content** (D34). `rm board.db` must lose zero notes, and
  `deleting_the_board_loses_zero_notes` is the guard. Adding a `body` column would be faster and
  would make D15's "the board is disposable" false.
- **Memory registers its *own* hook entry** (D41), never an extra event on the delivery command.
  Hook timeouts are per entry; merging them would put unmeasured work inside D9's guarantee.
- **`SessionStart` and `PreToolUse` injections are counted separately** (D42) — but the receipt
  divides by **both**. They are split to compare *retrieval modes*: recency against path
  anchoring, which is `MEMORY-DESIGN.md` §6's open question and the design's weakest-evidenced
  claim. An earlier version excluded `PreToolUse` from the denominator on the belief that its
  delivery was undocumented; that came from a truncated read of the hooks reference and is
  corrected in D42.
- **Redaction is named shapes, not entropy** (D46). Entropy was measured over this project's own
  vocabulary and does not separate: a crate version scores 4.06 bits/char, four real secrets score
  below it. Do not "improve" the filter with a global entropy threshold.

**Phases 2 and 3 are built (D49), and D2 and D16 are revised rather than ignored.** Read D49
before touching promotion or export — it records what changed, what did not, and the condition
under which the phase is withdrawn:

- **Candidates are never injected** (`INJECTABLE`), and D51 records why that constant, not a
  convenient `WHERE` clause, has to be the thing enforcing it.
- **Promotion never writes without `--yes`.** The threshold produces an offer. One candidate per
  offer, derivations shown rather than counted — batching approval is D16's defect with extra
  steps.
- **Export is one-way and user-invoked.** `amb` still never authors into a repository on its own
  initiative (D11), and `--check` compares content hashes, never a timestamp.
- **It was built before the receipt existed**, at the user's explicit direction and against the
  plan's own ordering. D49 says so in its second paragraph so nobody mistakes the order for an
  accident.

### Identity is free

`CLAUDE_CODE_SESSION_ID` is present in the environment of every command a session shells out to and
equals that session's transcript filename. Registration is optional: every command auto-creates the
roster row, so forgetting `amb register` yields a less readable name, not a failure.

## Conventions that have actually bitten here

- **After adding a guard, delete it and watch the test go red.** A test that passed on its first
  run has proven nothing. When a mutation survives, check whether the *mutation* was mistargeted
  before concluding the test is weak — that has happened.
- **Concurrency tests need concurrent processes, not threads.** The premise is N unrelated OS
  processes with no common parent; a threaded test shares a `Connection` and exercises a case that
  does not occur. See `tests/concurrency.rs`.
- **Repeat any measurement before quoting it.** `docs/MEASUREMENTS.md` records two wrong sub-claims
  that came from single noisy runs. Do not quote a startup number that was not measured for the
  current binary.
- **A ratio is a verdict only if its numerator and denominator describe the same opportunity.**
  This is not another entry in the catalogue of silences — that catalogue is about mechanisms
  failing to *reach* someone, and this mechanism reached fine. It compared two things against
  denominators that were never comparable. `amb memory status` printed `by path 0/8 · 0.00` beside
  `by recency 4/29 · 0.14`, which reads as path anchoring losing badly. `PreToolUse` fires on four
  tool names; `SessionStart` fires always. A session that reads its files through `Bash` — an
  ordinary way to work — raises one denominator and contributes nothing to the other, so the two
  lanes had 3 sessions of exposure against 1. **D59 withdraws the injection layer on exactly that
  kind of flat number**, and nearly did (D74).

  The checkable form is **two** questions, and the first one alone is not enough:

  1. **What is one unit of the denominator, on each side?** If the two sentences are not the same
     sentence, the ratios are two measurements and not a comparison — publish the exposure
     alongside them or do not publish the ratio. This is what catches D74.
  2. **Does the denominator rise every time the cost is paid?** A denominator that counts
     *distinct things offered* rather than *times the offer was made* understates the cost while
     the numerator is unaffected, so the ratio improves for free. This is the same failure from
     the opposite side — one lane paying twice and counting once, rather than two lanes counted
     against incomparable units — and question 1 does not catch it, because the unit is identical
     in both payments.

  Question 2 has a concrete instance here. `note_events` is `PRIMARY KEY (session, kind, scope,
  slug, event)`, so a note injected twice into one session records **one** row. That is right for
  the question the table was built for — *was this note put in front of this session* — and wrong
  for *what did injection cost*, and the primary key silently picks the first without the second
  ever being asked. Duplicate memory-hook entries in two settings files would have made every
  injection cost twice and count once, invisibly and in the flattering direction (D77).

  **Where to look, which is the part that generalises past this instance.** The answer to question
  2 is almost never in the code doing the dividing — that code is a division and looks correct.
  It is in the **primary key** of the table the denominator came from, or in whatever else decides
  what counts as the same row. So: when you divide by a count, go and read the DDL of the table it
  was counted from, and ask what that key *deduplicates*. Idempotency there is usually right, which
  is exactly why the failure is invisible — nothing is broken, and the number is only wrong for the
  question you have started asking of it. A ledger built to answer *did this happen* will answer
  *how often did this happen* with a straight face.

  The next instrument this project builds will face both questions, and the receipt is the one
  place where being wrong retires a feature rather than merely misinforming a reader.

  **It faced them, and needed a third.** D89 and D91 are the next instrument, and question 2
  caught the cheap design — a sentinel row in `note_events` would have recorded five searches in
  one session as one row, because of that table's primary key, exactly as the paragraph above
  predicts. But two more failures were sitting underneath, and neither question sees them:

  - **A number that nothing writes when the mechanism fails.** `unprompted: 0` counts citations of
    notes a session was never shown — reachable only by searching — and *no search was ever
    recorded*. So "nobody wanted a note" and "somebody asked and the search missed" printed the
    same zero, and D88 proves the second was happening. **Ask what the instrument records on the
    unhappy path.** A ledger that only writes on success reports a broken mechanism as an idle one.
  - **A number attached to the wrong event.** `status` printed the cross-repo counter as a verdict
    on cross-repo memory. Only `recall --file --across-repos` bumps it — a flag in no README, no
    primer and no banner — while `across_repos` merely re-sorts `concerning`, so plain `--file`
    was returning foreign notes and moving nothing (D91). **Ask what can move this number, and
    whether anyone can reach it.** A verdict computed from a flag nobody was told about is a zero
    by construction.

  So the checkable form is now four questions, and the two new ones are about the *writer* rather
  than the arithmetic: what is recorded when it fails, and what can move it at all.

  **And two catalogued failures can compose into a third neither predicts alone.** D91 is exactly
  that. The cross-repo counter watched `--across-repos`, a flag in no README, no primer and no
  banner — **D58's shape**, a mechanism that cannot reach the party positioned to use it. It was
  simultaneously **question 1 of the ratio rule**: one unit of that denominator was "an invocation
  of a flag nobody was told about" while the claim printed beside it was "cross-repo memory is dead
  weight". Neither entry alone flags it. D58 asks whether a mechanism can be reached and would have
  passed the *capability*, which fires fine through `recall --file`. Question 1 asks whether the
  units match and would have passed a reading of the *code*, which counts flag invocations
  consistently. The defect only appears where they meet: an unreachable mechanism used as the
  denominator of a claim about a reachable one.

  Read the catalogue as composable rather than as a checklist. And note that this was the **third**
  time this mistake was made on this one question — `OPEN-QUESTIONS.md` Q10 records the other two —
  which is the strongest available evidence that a question you have already been wrong about twice
  deserves the instrument checked before the number is read.

- **A guard that stays green when you delete it is not protecting anything.** D51: adding
  `CANDIDATE` to `INJECTABLE` — the constant whose whole purpose is that candidates are never
  shown — broke no test, because the exclusion was actually being done by an unrelated project
  filter. Correct by accident. Mutation testing was the only thing that could see it, and the
  *survival* was the finding.

- **A guard written against a caller is not a guard on the rule, and it looks identical.** This is
  D51's sibling and mutation testing does *not* catch it: every assertion passes, the mutation you
  try reddens, and the hole is in a call site you never thought to name. `quoted()` contains
  sender-written text and its docstring names the attack precisely.
  `a_newline_in_a_field_cannot_forge_ambs_own_voice` asserted it — against `render_all`. There were
  three renderers of `sender`/`subject`/`body`; `snapshot` happened to be right and `amb inbox`
  printed all three verbatim, on the command the `SessionStart` banner tells every agent to run
  first (D90). Nothing was red for as long as the command existed.

  **The check is arithmetic, not judgement: grep the field, count the renderers, count the
  assertions.** If those two numbers differ the rule is unguarded somewhere, whatever the suite
  says. And when you fix one instance, grep for the *literal* — not for the function you just
  changed. Each of D86, D88 and D90 was a second instance of a defect fixed in the same file hours
  earlier, because fixing one trains attention on the thing fixed rather than on its siblings.

  **The callers can be layers, and then counting renderers finds nothing** (M20). Project scoping
  — `@project` addressing a *place*, D17's central claim — was asserted twice: once as a pure
  predicate and once against the library. Delete it and **every test that drives the shipped
  binary stays green** — 137 of them, across all seven suites that spawn it. There is no second renderer to count here;
  there is one rule guarded at two of its three layers, and the missing one is the executable
  users actually run. So the arithmetic generalises to *count the layers the rule passes through,
  count the layers that assert it* — and the layer to suspect first is the outermost, because a
  library test is cheaper to write and therefore usually the one that exists.

  **Containment belongs to the field, not to the caller, or it regrows with every new renderer**
  (M23). D90's fix produced `delivery::UNTRUSTED`: one constant, and one test that enumerates
  every renderer of a sender-written field rather than asserting against one of them. Its own
  comment names the residual hole — a renderer added without being listed there stays silent. The
  note side has the same rule and none of that machinery. `quoted()` guards `n.title` at
  `inject.rs` and **six other renderers printed it raw** by the time audit round two counted —
  the count had grown from four while this paragraph stood still, which is this file's own
  doc-rot warning firing on itself. One of them is `amb memory recall`, the command the memory
  banner tells every agent to run, exactly as the delivery banner named `amb inbox` in D90. Two
  instances, one shape, and the second was not found by looking where the first was fixed.
  Round two guarded the two outside the injection ledger — `render_offer` (the approval gate)
  and `render_export` (a heading in a checked-in file) — so **four remain, waiting on the
  window**: `recall`, `candidates`, `observe`'s near-lines and `history`.

  **What hides it is an asymmetry between the writer and the reader, so auditing the writer alone
  clears the code.** `yaml_scalar` is `serde_json::Value::String(s).to_string()`, so a newline in a
  title is written as an escaped `\n` and the file stays one physical line — inspect that and
  there is no problem. `yaml_read` then JSON-*decodes* on the way back, correctly and by design, so
  `Note.title` in memory carries a real newline and the grammar becomes the renderers' to keep.
  **Deferred deliberately rather than missed**: the fix changes what a note renders, which the open
  measurement window forbids.

- **An assertion whose fixture never reaches the guarded branch, and mutation testing *does* catch
  this one.** Third variant, and the one that hides best, because everything about it is right
  except the input. `nearest`'s tie rule (D26) is `[(best, _), (runner_up, _), ..] if best <
  runner_up`, and `a_tie_produces_no_suggestion_at_all` carried, under the comment *"a clear winner
  among several candidates is still suggested"*:

  ```rust
  assert_eq!(nearest("api-v1x", &["api-v1", "totally-elsewhere"]), Some("api-v1"));
  ```

  `totally-elsewhere` is outside the edit budget, so the filter drops it and the match reaches the
  **one-candidate** arm. The guard is never evaluated. Replacing it with `false` survived mutation;
  so did flipping `<` to `>`. Both mean *"never suggest when two names are close"*, which is the
  rule that arm exists for — and it read as the conservative behaviour the function documents (M17).

  Note what this is not. It is not D51 (the mutation reddened nothing because nothing ran it) and
  it is not D90 (the assertion is against the right function). The comment states the rule
  correctly and the code beneath it exercises a different one — D88's shape, moved from production
  into a test, where it is *harder* to see, because a passing assertion looks like evidence in a
  way a comment does not. **When a test's comment names a branch, check that its fixture reaches
  that branch.** A filter or a guard upstream of the thing under test is where to look.

- **A substring assertion cannot see anything between the substrings, and this one is not a
  variant of the three above** (M24). Every case catalogued so far is a test looking at the *wrong*
  thing: a fixture that never reaches the branch, a rule carried by a test's name, a positive
  assertion guarding an omission. This one looked at exactly the right thing and still could not
  see a hole in the middle of it. A wrapped string literal kept its indentation and rendered
  `"before it          opened cannot enter one"`; every `contains` passed, because each needle sat
  on one side of the damage. **`contains` describes points, and the defect was in the space between
  them.**

  So a rendered artefact needs at least one **whole-shape** assertion and not only presence checks
  — something constraining the region the needles skip. Here that is "a rendered line has no double
  space", confirmed red against the exact string that shipped. The shape matters more than the
  specific rule: any assertion over the *whole* output would have caught it, and no number of
  additional `contains` would.

  **And note how it was found: by running the binary against the real board.** No unit test
  performs that check, and mutation testing cannot — a mutant of correct code is still rendered
  into a buffer nobody reads. That is a **third source of truth alongside tests and mutation**, and
  it is the only one that sees what a person actually gets. Use all three; this session needed
  each of them to find something the other two could not.

- **A credential-shaped test fixture blocks the push, and it is a permanent condition rather than
  an accident.** `python3 tools/check_secret_literals.py`, in the gate.

  GitHub push protection rejected this repository's first push on five commits, flagging a Slack
  token and a Stripe key in `src/memory.rs`. **Every one was a `redact.rs` test fixture and none
  was real** — the AWS one is Amazon's own published `AKIA…EXAMPLE` placeholder, and another spells
  the alphabet. The condition does not go away, because *a module that catches credential shapes
  has to be tested with credential shapes*.

  So the fixtures are built with `concat!`: no contiguous match exists in the file, the compiler
  rejoins it, and the asserted value is byte-identical. **They look pointlessly split, which is the
  danger** — rejoining one is a tidy-up that passes every test and breaks the next push, and this
  file's own catalogue is full of negative decisions that left no trace and got helpfully fixed.
  The checker fails the gate on any rejoin and was verified by rejoining one. It tripped on the
  comment that documents it, which is a fair demonstration.

  **This paragraph used to say the originals were still in history, and that rewriting had been
  rejected because `DECISIONS.md`, `MEASUREMENTS.md`, `CHANGELOG.md` and the vault cite SHAs
  constantly. The rewrite happened anyway.** On 2026-08-31 the history was reset to publish the
  repository — `check_secret_literals.py`'s own docstring names getting past secret scanning as
  the reset's purpose — and the predicted cost was paid in full: every pre-publish SHA is now a
  dangling label, which is why `DECISIONS.md`, `OPEN-QUESTIONS.md` and the README's conventions
  each open by saying so. What is true now is simpler. **No commit in the published history holds
  a contiguous credential shape**, checked with the gate's own pattern over every commit on every
  ref, so there is no allowlist to click through and `concat!` is the only thing standing between
  this repository and a blocked push.

  **How the false sentence survived is the reusable part.** The three files that were given a
  banner are ones a person opens on purpose. This one is loaded into every session automatically,
  so nobody had to open it and nobody did — and a rejection recorded as settled reads as current
  for as long as it stands. **When an event overturns a decision, the file likeliest to keep the
  old version is the one that is always already open.**

- **Documentation drifts from the code silently, and there is a script for the mechanical half.**
  `python3 tools/check_docs.py`. It checks what has one source of truth: every doc in `docs/` is
  linked from the README index, every shipped subcommand appears in the README's command-reference
  table, the quoted test count matches `cargo test`, the quoted `D1–Dn` range matches
  `DECISIONS.md`, and `[Unreleased]` does not claim "Nothing yet" while commits exist since the
  tag. **All six were verified by breaking them** — and two of the first four were decorative,
  passing because a sentence elsewhere in the README happened to name the file or the command.
  Anchoring them to the actual link and the actual table is what made them real.

  What it cannot check is whether prose is *true*, and that is where the worse failures live: a
  negative decision stated too strongly gets defended by a reader long after the code stopped
  honouring it. `"Nothing is ever written inside a repository"` survived in three files after
  `amb memory export` began doing exactly that.

- **A false comment about a mechanism is worse than an absent one, and this project has shipped
  one.** `sync_dir` said `mtime` was the cheap gate and `content_hash` "the decision". The second
  half was never true — the skip is decided on `mtime` alone and the function returns before
  `content_hash` is read. A migration was then written trusting that sentence (clear the hash to
  force a re-derive), and it could not take effect: fourteen notes sat with empty hashes and no
  `note_links` until D67's repair. An absent comment makes you check; a false one makes you trust.
  **When a field stops being consulted, its comment is part of the change** — nothing fails when
  prose rots, which is exactly why it rots.

  **The second instance was worse, because the comment was not stale — it was answering a
  different question.** `recall`'s docstring said "`LIKE`, not FTS5, and that is a scope decision
  rather than an oversight", which reads as a considered trade between lexical and semantic search.
  The actual behaviour was that it matched `body_excerpt` — the first paragraph of a note,
  truncated to 240 characters — so most of every note was never searched at all (D88). Two readers
  had passed over it, one of them while adding `capture` to `SEARCHABLE` on the belief that this
  made captures searchable. **A comment that argues for the design you have is the hardest kind to
  disbelieve; check it against the code it sits on, not against whether it sounds right.**

  **The third instance cuts the other way, and that is the part neither of the first two would
  have predicted.** `bench/bench_startup.py` had its `amb` rows commented out behind
  "Uncomment once built" — for as long as the binary had existed — and pointed at
  `./target/release/amb`, which does not exist on a machine sharing one target directory. Meanwhile
  README.md published `amb --version` at 2.1 ms and `amb inbox` at 3.0 ms and named that file as
  the harness. Nothing failed: the script ran, printed three rows, exited 0.

  The published numbers turned out to be **honest and independently reproducible** — 2.15 / 2.40
  and 3.14 / 3.26 once the harness was repaired. So the first two instances are cases where a
  false artefact made *wrong* work look sound; this is one where a broken artefact made *correct*
  work look suspect. The rule is not "prose drifts optimistic". It is that an artefact asserting a
  method is a claim in its own right, and it can fail in either direction. Delete-versus-repair was
  a real fork here and deleting would have been wrong — the citation was the only evidence anyone
  had reproduced those figures at all.

  **The fourth instance is not in code or in a script. It is in `DECISIONS.md`** (D95, M24). D59
  states a withdrawal condition — 30 sessions, 50 injections — and on this machine it **cannot
  fire**: `note_events` is keyed so a resumed session writes no injection row, and no new session
  had started in two days. The decision was not wrong when written and no line of it had rotted.
  It had simply stopped being able to happen, and `Verdict::TooEarly` printed identically either
  way. **A stated condition that cannot fire is worse than no condition**, for exactly the reason a
  false comment is worse than an absent one: the next reader sees a standard and assumes something
  is watching. An absent condition makes you ask; a dead one makes you trust. So the rule extends
  past prose and past artefacts to **decision records themselves** — when a decision names a
  threshold, something has to be able to say whether it is reachable, or the record is a comment
  with a number in it.

- **A green test result can prove less than it appears, and arithmetic is what catches it.** A new
  test read `2 passed; 158 filtered out` on a name filter that matches one test, in a suite of 159
  — it had been inserted between an existing `#[test]` and its function and so carried two
  attributes, registering twice. Nothing was red. Same family as D51's surviving mutation: the
  *shape of the number* was the finding, not the pass/fail.

- **A field that nothing reads is this codebase's recurring defect — check for it.**
  `python3 tools/find_unread_fields.py`. **Read its advisory, do not scroll past it** (D84): it had
  printed the same three names on every run for days, and one of them was a duplicate
  implementation whose only callers were the two tests pinning the delivery back-off — so that
  rule was being asserted against code nothing runs. The other two were a false positive the tool
  now explains inline, and a bug in the tool's own arithmetic. `rustc`'s `dead_code` lint cannot catch it, because these
  are `pub` fields on a library crate and therefore reachable by definition. It has happened three
  times: `messages.attempts` and `messages.failed_at` had no writer at all (D23),
  `IndexStats::skipped` had no reader so a 501-note vault reported itself empty (D45), and the
  incumbent's own `relevance_count` is zero across 80,264 rows (D39). **Writing D39 did not prevent
  D45**, three days later, which is why this is a script rather than a note.

  **The inverse is the one that tool cannot see: a field whose *reader* is what makes it
  load-bearing** (M27). `Redacted.removed` reads, inside `redact.rs`, as bookkeeping — a count of
  what was replaced. Its actual contract lives one module away: `write.rs` prints `"N value(s)
  redacted before writing"` under `if w.redacted > 0`, and its comment says a redaction the author
  cannot see is one they cannot correct. So mutating `*removed += 1` to `*=` in `strip_pem` does
  not produce a wrong count — it produces a **silent redaction**, the private key stripped and the
  note reporting that nothing was. Neither module's tests could see across the seam, and auditing
  the writer alone clears the code. **A field's importance is set by its reader, so when you check
  a counter, go and read what reads it.** The guard belongs where the meaning is, which is usually
  not where the increment is.

  **Three instances in one week make it a shape rather than an incident, and the shape has a
  name: a counter whose writer works and whose only reader is a human report.** `Redacted.removed`
  (M27), `capture.rs`'s failure marker (M51), and `export.rs`'s `written` (M54) — where
  `written += 1` could become `*=`, zero forever, so `amb memory export` tells a person nothing
  was written while every file lands, on the one path that authors into a repository (D11). The
  mechanical property is what makes it recur: **`find_unread_fields.py` structurally cannot see
  these**, because the field *is* read — by the print. Nothing is dead and nothing is unwired, so
  the only thing that catches it is an assertion at the caller's distance that the count itself is
  right. When a number reaches a person, test the number and not just the sentence around it.

- **This project's failures are silences, not errors.** Three real bugs here were a message
  accepted and never delivered, a `strip_prefix` returning `None` on macOS so no edit was ever
  claimed, and an empty `additionalContext` where text belonged. None crashed. Assert the positive
  explicitly.

  **And its inverse, which the sentence above reads as covering while doing the opposite** (M23).
  A positive assertion cannot guard a filter whose job is an *omission*. `by_force` drops any force
  with no events, so the per-force split does not pad itself with a 0/0 row for every force the
  board never used; the only assertion on it anywhere was `forces.contains(&"rule")`, which is true
  whether or not the filter works, and relaxing `injected > 0` to `>=` survived the whole suite.
  This generalises past the instance: every filter, every exclusion, every "we deliberately do not
  show X" needs an assertion of **absence**, and "assert the positive explicitly" is the advice
  that stops you writing one.

  **And the shape it takes most often is a guard over a count** (M27). `status.rs` scored 52/92,
  and **thirty-seven of its forty survivors sit on the `if` that decides whether a line is rendered
  at all** — ten of them the literal edit `x > 0` -> `x >= 0`, the rest the other operators in the
  same conditions; only three are in the arithmetic. That relaxation is invisible to a
  presence-only suite, and it has now appeared in three renderers: `status.rs`, `inject.rs`'s
  `render_hidden` (fixed in M23), and `delivery.rs`'s `hidden > 0` — the sibling left standing when
  the first was fixed. **A boolean guard has no such relaxation**: `!xs.is_empty()` can only be
  inverted, which changes the answer in both directions at once, so any presence test kills it. How
  a guard is spelled decides how much test effort it needs.

  **An empty fixture is the first test to write, and it does not catch the count guards.** This is
  the part that looks solved and is not. `delivery.rs` already had two empty-case tests and neither
  reaches `hidden > 0`, because `hidden` is `ordered.len() - shown` — only interesting when mail is
  *present and under the cap*. **A guard over a derived count needs a fixture populated in
  everything except the quantity it guards**: the middle state, neither empty nor triggering.

  **It is also a third way for an instrument to fail, and the questions above do not cover it.**
  Those all interrogate the *number* — does its denominator match, what does it record on the
  unhappy path, what can move it at all. `status.rs`'s arithmetic was correct throughout; all forty
  survivors were in the rendering. A correct number is still **delivered on a page**, and the page
  has its own failure mode: under these mutants the command D59's withdrawal is read off prints
  `! 0 note(s) … that content is gone` on a healthy vault, and `should be switched off` on a board
  where nothing was ever injected. Not cosmetic — a wrong input to the decision the instrument
  exists to inform, and indistinguishable from signal to the person it is for.

  **Do not generalise this to "renderers are unguardable" — that reading was tried and refuted.**
  Three modules suggested the score tracked what a module *produces*, so `delivery.rs` was run as
  the prediction's own test: a renderer, never mutated, holding the banner every session reads. It
  came back at 88%, and its worst survivor was not a rendering defect at all but a **D11 bypass**
  in `write_snapshot` — `path.parent()` on a bare filename is `Some("")`, and the guard sending
  that to `"."` had no test because every fixture passed an absolute path.

  **And an assertion of absence carries a hidden premise, which is how the rule above bites the
  person applying it** (M27). Six tests were written against that finding; two mutants survived
  them, and one was this. `p.suppressed > 0` -> `>= 0` lived through an assertion that its line
  was **absent from an empty board** — because on an empty board the *enclosing* `p.candidates >
  0` returns first, so the nested condition is never evaluated and the assertion proves only that
  the outer block was missing. It passed, and guarded nothing.

  This is M17's shape — a filter upstream of the thing under test — arriving inside a test written
  specifically to catch omissions, which is worth knowing because the two rules read as though the
  second protects you from the first. **Asserting a line is missing proves nothing unless the
  block containing it rendered.**

  The check is mechanical, and it is about the *shape of the test* rather than the depth of the
  code: **an absence-only assertion has an unproven premise; a truth table containing at least one
  presence row proves its own.** Five of the six tests written against this finding are truth
  tables and were fine for exactly that reason — their `expected == true` row fails if the
  enclosing block stops rendering. The sixth was a list of needles that are all absences, and that
  is the one that had the hole. So when a test only ever asserts that things are *not* there, add
  the row that proves the renderer got that far. Keep the vacuous needle with a comment saying why
  rather than deleting it; the trap is easier to fall into than to remember.

  **And a needle list closes only the holes someone thought of, where a property closes all of
  them.** Every warning `render_status` prints carries the same `  ! ` prefix, so *"a healthy
  empty board raises no alarm"* is one assertion covering all eight — including `failures > 0`,
  which the needle list never named. Prefer the invariant over the enumeration wherever the
  artefact has a marker to key on; enumerate only when it does not, and then name the hole as
  `delivery::UNTRUSTED` does.

## Environment

- **`cargo install` does not update the binary the hooks run, and that is why the stale-binary
  failure keeps recurring.** The hooks in `~/.claude/settings.json` invoke
  `/Users/emrec/.local/bin/amb`; `cargo install --path .` writes `~/.cargo/bin/amb`, which is also
  what `PATH` resolves first. So after a schema change **manual `amb` commands work perfectly while
  every hook on the machine fails silently** — which is exactly why it goes unnoticed. Observed a
  fourth time on 2026-08-28, with `~/.local/bin/amb` at schema 4 against a board at 5. Fix it with
  `./tools/install.sh` (or by hand: `rm` the stale copy first, then `cp` — an in-place `cp` onto
  a cached signature leaves macOS SIGKILLing the copy), then check `amb --version` reports the
  same commit from both paths. The fingerprint (D56) exists to make that comparison possible at all — before it,
  both printed `amb 0.1.0`.

  **`./tools/install.sh` is the fix, and `amb doctor` is only the detector** (D94). Detection
  shipped first and the condition recurred within minutes of the next commit — a failure that
  recurs on every commit is not closed by being visible. The script builds, copies to `PATH` *and*
  to every path an installed hook actually invokes (read out of `settings.json`, not hardcoded),
  then runs `doctor`. **Use it instead of `cargo install`.**

  **`amb doctor` performs the comparison, so the condition is one command rather than a thing to
  remember** (D73). It runs each installed hook's binary with `--version` and compares the
  fingerprint, reporting `BAD` with the exact `cp` to run. It found a stale hook binary on its
  first execution. Nothing else covers this: D69's `HookState` is built on `command_is_ours`, which
  matches the executable's *name* and never its path, so a hook pointing at last month's `amb` is
  still "ours" and still `Installed`.

- **All Rust projects on this machine share one cargo target directory**
  (`~/.cache/cargo-target`). `cargo clean` is global, and a second concurrent `cargo` run can
  produce phantom errors in crates you never touched — check for another build before debugging.

  **`cargo mutants` inherits that setting, and the failure is worse than a phantom error.** It
  copies the source tree but not the cargo config, so every mutant it compiles lands in the shared
  target directory under this package's own name. Observed 2026-08-29: `verify.sh` ran while a
  mutation run was in progress and three `messages.rs` tests failed with `attempt to subtract with
  overflow` — the gate had tested a *mutant*. It corrupts in the other direction too, and that one
  is silent: afterwards `cargo test` reported 225 lib tests where the source had 231, because the
  stale binary was reused and the six new tests were simply absent. **A green run proved nothing
  and said so in no way at all.** `cargo clean -p amb` fixed it and removed 17.3 GiB — the volume
  under one package name is the collision, measured.

  So: run it as `CARGO_TARGET_DIR=<somewhere private> cargo mutants … --jobs 1`, run nothing else
  meanwhile, and treat any mutation result produced while another `cargo` was running as void
  rather than as evidence. It also needs `--copy-vcs true`, or `build.rs` cannot fingerprint the
  repository and the baseline fails before a single mutant is tested.
- **Git history starts at the initial commit** on `main`. Commits follow `.devt/rules/git-workflow.md`
  (conventional `type(scope): subject`), with the exception below.
- Other Claude sessions work these repos concurrently. A peer's "go ahead" is not the user's
  authorisation.
- No `Co-Authored-By` trailer in commit messages.

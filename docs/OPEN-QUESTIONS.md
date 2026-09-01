# Open questions

Genuinely undecided. Each one changes what gets built, so none should be settled by whoever
notices it first without saying so.

**Convention:** when a question is settled, **delete it from here** and record the answer in
`DECISIONS.md`. A stale question spends a reader's attention.

> **This convention leaned on `git log`, and on 2026-08-31 that support was removed.** The
> repository's history was reset to publish it, so the commits that held the deleted text of
> Q1–Q7, Q9 and Q12 no longer exist here. **Nothing was lost** — each of those questions names
> the decision it became, immediately below, and the decisions are the authoritative record.
> The pre-reset history is archived outside the repository. What changed is that the *prose* of
> a retired question is now recoverable only from that archive, so a question deleted from here
> from now on should leave its answer in `DECISIONS.md` rather than trusting `git log` to keep it.

**Q1–Q6 and Q9 were settled on 2026-08-27** and have been deleted per that convention. They are
now `DECISIONS.md` D12 (identity), D14 (claim etiquette), D9 (discoverability), D13 (lease TTL),
D15 (database location), D16 (findings inbox) and D19 (observed-claim granularity).

**Q7 was settled on 2026-08-27**, empirically and exactly as it asked to be: a probe hook proved
`PostToolUse` output *is* injected into the model's context. It is now D25, and mid-turn delivery
ships. The `monitor` mode it asked about is therefore a nicety rather than load-bearing.

**Q12 and Q13 have also been settled and deleted, and until 2026-08-31 neither was recorded here.**
They are now `DECISIONS.md` D85 (`notes.content_hash` is dropped, and the measurement Q12 asked for
could not have answered it) and D98 (a message body is stored exactly as written, closing Q13 on
data).

> **The reset note above lists Q12 among the deleted and names no decision for it, and Q13 is not
> in that list at all.** Both answers were written down properly and both are correct; what went
> missing was the pointer from the register that promises them — in the same paragraph that
> promises them. Everything that note *does* say is true, which is exactly why it read as complete.
> `tools/check_docs.py` now does the arithmetic instead of trusting the sentence, and found both on
> its first run (M38).

**Q8 was settled on 2026-08-31** and deleted per that convention — on a check, not on the cost
argument it framed itself as. It is now D101: the one cross-vendor mechanism that would have made
breadth cheap cannot push into a running session, and the request to make it was closed
`NOT_PLANNED` twice. What Q8 called "the choice" was already foreclosed. The distribution half of
its market argument is split off as Q14 rather than settled with it.

---

## Q10 · Does cross-project memory do anything?

**Narrowed to this, 2026-08-28.** Everything Q10 used to ask has been answered and moved to
`DECISIONS.md`, per this file's own rule that a settled question is deleted rather than annotated:

- *Does memory belong in `amb` or a second binary?* — **D48.** Architectural, needed no receipt.
- *Should Phases 2 and 3 be built?* — **D49**, which revises D2 and D16 explicitly.
- *What retires the injection layer if it does not earn its keep?* — **D59**, decided in advance:
  below a 0.10 cited ratio over 30 sessions and 50 injections, with nothing ever reached for
  unprompted, it is withdrawn rather than extended. `amb memory status` computes it.

What is left is one question that no amount of design can answer, and it is the gate on everything
in `AMB-MEMORY-ARCHITECTURAL-DIRECTION.md`.

**The question.** Is a note derived in one project ever wanted in another?

**Why it blocks the whole architectural direction.** Every axis proposed there — topic scope, force
levels, the promotion router — is a *precision* mechanism, and precision has no value at this
corpus size. Scope narrows what gets injected, but nothing is being over-injected. Force ranks
under budget pressure, and there is no budget pressure. The topic rung needs three projects sharing
a topic. Building any of it now ships ceremony that cannot be evaluated, which is the failure this
project has already caught twice (D45, D51).

**Why it cannot be answered today, precisely.** The memory layer has only ever run in **one
repository**. `~/.claude/settings.json` installs the delivery hooks machine-wide (`amb hook
monitor`), but the memory hook and `AMB_VAULT` live in this repository's `.claude/settings.local.json`
and nowhere else. So `cross_repo_queries` reading 0 is not evidence that cross-repo memory is
unwanted — **there has never been a second repo to query.** A zero from a mechanism that could not
have fired is the shape D58 names, and reading it as a verdict would be the mistake.

**The experiment.** Enable memory in exactly **one** more repository — two arms, not nine, so the
cost is bounded and the variable is isolated — by adding the same two things this repository has:
the `amb hook memory` entries on `SessionStart` and `PreToolUse`, and `AMB_VAULT` pointing at the
same vault. Then work normally for a few weeks.

**What settles it, read straight off `amb memory status`:**

| Reading | Means |
|---|---|
| `cross_repo_queries` still 0 | The differentiator is dead weight. Say so and retire it. |
| Nothing reaches the derivation threshold with two projects to derive from | Promotion has no fuel, and D49's phase goes with it. |
| Foreign notes injected but never cited | Cross-project memory is a tax. Scope would be narrowing something nobody wants. |
| Foreign notes cited | The direction has its premise, and the axes become worth arguing. |

**A negative result is the valuable one.** If cross-project memory is never reached for, that
retires most of the architectural document — a large amount of unbuilt work correctly avoided,
which is the cheapest outcome available and the reason for running the arm before building the
axes.

**Do not start the second arm during D59's measurement window** (D77). The two experiments share
one instrument and adding a repository doubles the injection rate mid-window, so the second arm
would make the first uninterpretable. There is also a way to start it *by accident*: setting
`AMB_VAULT` anywhere broader than one repository starts the second arm silently. **When the arm is
wanted it gets its own date and its own decision** — not as a side effect of tidying an
environment file.

> **Correction, 2026-08-28: the second arm has been running since 11:52 today, and this section
> asserted it could not be.** The paragraph above used to claim `AMB_VAULT` "survives only in this
> repository's `.claude/settings.local.json`, which is the sole reason the machine-wide memory
> hooks no-op in the other eight projects". It does not:
>
> ```
> $ cat ~/Projects/greenfield-api/.claude/settings.local.json
> { "env": { "AMB_VAULT": "/Users/emrec/vault" } }
> ```
>
> Four `greenfield-api` notes are in the vault and in the index, from two sessions, the earliest at
> `2026-08-28T11:52:28Z`. One is a substantive observation with attached files; three are
> `PostToolUseFailure` captures.
>
> **This is the catalogue's shape with the sign flipped.** The usual failure here is a mechanism
> that cannot fire being read as a mechanism that fired and found nothing. This is a mechanism that
> *was* firing being written up as one that could not — and the sentence doing the asserting was
> the load-bearing one, because it is what made `cross_repo_queries = 0` uninterpretable rather
> than negative. The check was one `cat` away and was never run, in a question whose entire
> difficulty is that it cannot be answered without a second repository.
>
> **What it changes.** Capture is running in two projects; cross-project *retrieval* is not, because
> the injection query filters on scope — `scope IN (project, #topics…, '@@')` since D81 — so a
> foreign observation is reachable only through `recall --across-repos` and `concerning`. That is the right shape for what
> Q10 asks and it means the arm is live for the table above. It also means D81's scope axis landed
> against two real projects rather than one hypothetical one — and D82's topic rung stays dormant,
> because those two are Rust and Python and share nothing.
>
> The arm still has no date and no decision, because nobody chose it. Whether to keep it, and
> whether two arms are enough to read the table, is now a live question rather than a blocked one.

> **Second correction, 2026-08-29: three of the arm's four notes were machine-written scrollback,
> and the table above cannot be read against them.** `greenfield-api` held 4 notes; 3 were
> `PostToolUseFailure` captures titled `"Bash failed"`, with raw tool output for a body.
>
> That invalidates one row specifically. **"Foreign notes injected but never cited → cross-project
> memory is a tax"** requires that a foreign note *could* have been cited. A capture cannot be —
> there is nothing in it to cite — so a zero against this corpus measures the capture filter and
> not the differentiator. Reading it as a verdict would be D58's shape again, and this file has
> already recorded that mistake once with the sign flipped.
>
> **D86 changes what accumulates from here**, since captures are their own kind and are no longer
> injected. It does not retroactively make this arm interpretable: what it has collected so far is
> one substantive foreign observation. **The arm needs re-running against a corpus of curated
> notes before any row of the table above is read** — and, per D87, not while D59's window is open,
> for the reason the paragraph above this one already gives.

> **Third correction, 2026-08-29: the row this table is read off was measuring the wrong thing,
> and this is the third time that has been true of this question.** `amb memory status` printed
> `cross-repo query run 0 time(s) — if that holds, the differentiator is dead weight`. The counter
> behind it is bumped from one place, the `--across-repos` branch of `recall` — a flag that appears
> in `DECISIONS.md` and in this file and **in no README, no primer and no banner**, so no agent and
> no reader has ever been told it exists. And `across_repos` calls `concerning` and only re-sorts
> it, so plain `recall --file` was already returning foreign notes without touching the counter.
>
> Demonstrated: a `--file` lookup from one project returned another project's note in the same
> second `status` reported the differentiator as dead weight.
>
> **The first two corrections here were about the arm; this one is about the instrument**, and it
> is the same shape as both — a zero from a mechanism that could not fire, read as a finding.
> D91 moves the count onto the event: a search that returns a note scoped outside the caller's
> project is a cross-repo hit, on every lane, windowed like the rest of the receipt.
>
> **What it does not change.** The table above still needs a corpus of curated notes and still
> should not be re-run while D59's window is open. What it changes is that the row
> *"foreign notes injected but never cited"* can now be distinguished from *"no foreign note was
> ever returned"* — which, with D88 and D89, is the third distinction this question needed and did
> not have.

> **The precision axes wait, decided 2026-08-29 by the user who had asked for them.** Work on the
> scope axis, the topic rung and the promotion router was requested and then withdrawn, in the
> requester's own words: *"That is the receipt discipline I had defended for many turns and then
> abandoned under a stated goal of closing gaps as fast as possible."*
>
> The argument for waiting is this file's own: the corpus is 27 notes, nothing is being
> over-injected, and until D88 the search that would evaluate any of those axes could not see past
> a note's first paragraph. Building precision now ships ceremony that cannot be evaluated — D45
> and D51's failure, and this question already carries three recorded instances of reading a number
> produced by an instrument that could not answer it.
>
> **What the layer needs next is sessions, not commits.** The window was reopened on 2026-08-29
> onto corrected instruments (D88, D89, D91), and nothing further should be built on the injection
> path until the receipt says something. That includes every axis in
> `AMB-MEMORY-ARCHITECTURAL-DIRECTION.md`.

---

## Q11 · Does `amb` grow a cross-machine transport, and is it the SSH hub?

**Deferred 2026-08-27, not rejected.** Raised by the user: *"I want to be able to use our
messageboard for my other agents within same network."* Scoped in the same conversation to **their
own machines only** — one trust domain, two or three of them, macOS or Windows. That scoping
matters: it is what keeps this a transport question instead of a multi-user product with an
authentication model.

**This contests a clause of D27**, which disposed of the idea in one line — *"a cross-machine
relay is better served by `hcom`'s MQTT or by Remote Control"*. Settling Q11 therefore amends D27
rather than merely adding to it. D27 was right about **live** traffic and never weighed the
durable case, which is the half `amb` exists for.

### What was checked, so it is not checked twice

- **The naive answer is impossible, not merely unwise.** `amb` runs WAL (`engage_wal`,
  `src/db.rs`), and WAL keeps its wal-index in shared memory. SQLite: *"All processes using a
  database must be on the same host computer; WAL does not work over a network filesystem …
  processes on separate host machines obviously cannot share memory with each other."* A board on
  an SMB/NFS share is off the table by construction. <https://sqlite.org/wal.html>
- **SQLite documents the alternative, and it is the hub.** *"Host an SQLite database in WAL mode,
  but do all reads and writes from processes on the same machine that stores the database file."*
  The engine must be local to the file; the invoker may be remote. `ssh hub amb …` is literally
  that sentence. <https://sqlite.org/useovernet.html>
- **The platform does not cover a LAN.** Native cross-session messaging reaches another of your
  machines *"through Anthropic servers, arriving over that machine's Remote Control connection"* —
  a cloud round trip, both sessions live, one named recipient at a time, and absent on Bedrock,
  AWS, GCP and Foundry. <https://code.claude.com/docs/en/cross-session-messaging>
- **And has not committed to covering it.** `anthropics/claude-code#28300`, *"Multi-agent
  collaboration across machines (Agent-to-Agent protocol)"*, opened 2026-02-24 — no assignee, no
  milestone, no official response. D27's stated trip-wire has not fired.
- **The field's answer costs a broker.** `hcom` does cross-device over an MQTT relay
  (`hcom relay new` / `hcom relay connect`), end-to-end encrypted, self-hostable. D27's citation
  was accurate — and it means every competitor charges a resident process, which is the thing D3
  can still beat.
- **Replication is healthy but lands wrong.** `cr-sqlite` is real and active (3.8k★, backed by
  Turso/Fly/ElectricSQL); Litestream, LiteFS, rqlite and dqlite all need a daemon. For `amb`
  specifically, multi-master forces `messages.id AUTOINCREMENT` → ULID, a migration through the
  single query D17 calls the project's central claim. High cost against a problem one hub does
  not have.

### The shape recommended, if it is built

One environment variable, `AMB_HUB`. Unset, behaviour is today's, byte for byte. Set to
`user@host`, anything that would touch the database runs `ssh $AMB_HUB amb …` instead. Local keeps
identity, git-root detection, path relativisation and rendering; the hub keeps the database and
nothing else. One board means no merge and no conflict resolution — D6, D17, D22 and D23 hold
verbatim.

Three things it costs, none of them free:

- **`agents.host`, migration 2→3, additive.** Without it liveness lies across machines: `pid` is a
  bare integer, so pid 1234 from a laptop is checked against pid 1234 *on the hub* — a live agent
  reported dead, or a dead one reported live. This project's signature failure mode is a silence,
  and that is one.
- **A Windows port, bounded but real.** `HOME` is read in three places (`db.rs`, `identity.rs`,
  `hooks.rs`) and Windows sets `USERPROFILE`; liveness is `libc::kill(pid, 0)`. The hub design
  dodges most of it — client machines open no database, so no SQLite, WAL or file permissions —
  leaving env vars, git root and `ssh`. Replication would need the whole stack correct instead.
- **A third hook invariant.** Today: always exit 0, and do nothing when no board exists. It gains:
  and do nothing when the hub is unreachable — `BatchMode=yes`, `ConnectTimeout` short, fail open.
  A sleeping hub otherwise taxes every turn in every repo on the machine. Mutation-test it with
  the other two.

Testable without a network: put a fake `ssh` on `PATH` that execs locally, and the transport
becomes a seam like any other.

### What is not measured

`amb inbox` costs **3.30 / 3.12 / 3.01 ms** per invocation (release binary, three runs of fifty,
scratch board, 2026-08-27). The SSH leg is **unmeasured** — `sshd` is not running on this machine,
so the multiplexing figures in the literature were read and deliberately not quoted as ours.
Measure it against a real hub before any latency claim enters `MEASUREMENTS.md`.

### What would change the answer

If the platform persists messages for absent sessions *and* grows a broadcast address, this
question closes with D27 and the messaging half of the project with it. If the machines turn out
to be laptops that sleep with no always-on host among them, the hub degrades into a merge problem
and the replication argument deserves re-opening on its merits rather than being inherited from
here.

---

## Q14 · How does anyone who is not us install `amb`?

**Raised 2026-08-31, split off from Q8 rather than settled with it.** They are different questions
and pairing them hides the cheaper one: vendor breadth costs a delivery contract per vendor and buys
vendors on which D9's guarantee is weaker (D101), while distribution costs a release pipeline and
touches no invariant.

**The gap, measured against the field the same day.** `hcom` installs with
`brew install aannoo/hcom/hcom`, and also offers `uv tool install hcom`, a shell installer and a
PowerShell one. `amb` installs by cloning the repository and running `cargo install --path .` — or,
since that does not update the binary the hooks actually invoke, `./tools/install.sh`. The
repository was published on 2026-08-31 and CI ran for the first time the same day. Nothing has ever
been released from it.

**What the field does.** `dist` — formerly `cargo-dist`, 2.1k★, used by Zed, rustfmt and starship —
generates the GitHub Actions workflow: macOS x86_64 and aarch64, Linux glibc and musl, Windows, then
a Release with signed artifacts, a Homebrew tap and a shell installer. `cargo-binstall` consumes
those releases. crates.io is a separate axis and orthogonal to all of it.

**Two things here are not generic packaging questions, and they are why this is filed rather than
just done.**

- **`publish = false` is a decision, not a default** (D56). The version covers four contract
  surfaces and the Rust API is deliberately not one of them, because nothing links against `amb`.
  Publishing does not overturn that reasoning, but it makes the manifest's claim visible to
  strangers, so D56 is what to read before the flag is flipped.
- **A package manager upgrading the binary is the stale-hook hazard with a new cause.** The
  condition D94 records has recurred five times: `~/.claude/settings.json` invokes a binary at a
  fixed path and `cargo install` writes a different one, so manual commands work perfectly while
  every hook on the machine fails silently. `brew upgrade` has the same shape and a worse trigger,
  because it fires without anyone thinking about `amb` at all. Whatever ships has to answer what
  `amb doctor` reports the morning after an unattended upgrade — and D73 built `doctor`'s
  fingerprint comparison for exactly this question, so the detector already exists.

  **Simulated and answered, 2026-08-31** (M44): under a sandboxed `$HOME` whose hooks invoke a
  binary at another fingerprint, `doctor` prints `BAD`, names the hook, shows both fingerprints,
  states the condition in one sentence — *"Manual commands work and every hook is stale"* — and
  gives the literal `cp` that fixes it. The main objection to distributing is therefore detected,
  named and remediable the morning after. Two caveats are the residue: detection requires a person
  to *run* `doctor` (it exits 0 by D73, so anything unattended must read `--json`'s `worst`, not
  `$?`), and the comparison keys on the executable being *named* `amb` — true for Homebrew and
  every packager in Q14's survey, but a rename makes the hooks invisible rather than stale.

**Not urgent and deliberately undecided.** Nobody has tried to install it. This is filed because
publication made the gap real and because the answer is cheap, not because there is evidence of
demand — and Q10's lesson is that shipping a mechanism before anything can evaluate it is how this
project wastes work.

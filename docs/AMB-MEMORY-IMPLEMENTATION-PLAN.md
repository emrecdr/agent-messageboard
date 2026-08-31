# `amb` memory — implementation plan

> **Status: all four phases are built. D34–D52.** Phase 4a — the blocking `Stop` self-compression
> — is the one part deliberately refused, and D52 records why and what would change it.
>
> **The ordering this document is built around was overridden, and that is worth seeing rather
> than tidying away.** The plan puts the cheap receipt before the expensive argument: Phase 2 was
> gated on a non-zero citation ratio *and* on Q10 being settled, in that order. The user directed
> that Phases 2 and 3 be built anyway, which is theirs to direct — **D49 is that decision**, and it
> revises D2 and D16 rather than quietly contradicting them. The receipt read `7 injected · 0
> cited` at the time.
>
> The struck-through preconditions below are kept deliberately. A plan that reads as though it was
> followed, when it was overridden, teaches the next reader the wrong thing about how this was
> built.
>
> `DECISIONS.md` is the specification; this file is not. **Every phase's arguments now carry
> D-numbers and live there** — Phase 1 as D34–D48, Phases 2 and 3 as D49–D51, Phase 4 as D52, and
> the four later audits as D53–D55. The plan's closing section asked that the scope expansion
> "arrive by decision, not by accumulation"; it did. Read the phases below as the record of an
> argument settled in `DECISIONS.md`, not as the place it is settled.
>
> **Q10 governed Phases 2 and 3. It never governed Phase 1.** Phase 2 proposes a promotion
> pipeline against D16, and Phase 3 a central home for decisions against D2; ~~both stay unbuilt
> until `OPEN-QUESTIONS.md` Q10 is settled, and D2 and D16 stand as written until it is.~~
> **Struck by D49**, which took the decision Q10 was holding rather than waiting for it, and
> revises D2 and D16 explicitly. D48 had already settled the other half — whether memory belongs
> in the binary — without needing the receipt at all.
> **Phase 1 stores only session observations** — a traffic type D2's taxonomy has no row for and
> D11 forbids putting in a repo — so it contradicts nothing settled and can proceed on its own.
> That split is deliberate: it puts the receipt that decides the whole idea *before* the argument
> that is expensive to have. See **What Q10 actually governs**, below.

Step-by-step plan for the central vault, decision promotion, project export, and the claude-mem
replacement. Each phase ships alone and produces the receipt that justifies the next one.

**Companion: `MEMORY-DESIGN.md`.** It holds the design detail this plan cites — the D2/D16 tension
stated rather than resolved, the storage model and schema, retrieval and trust, the export gate,
and the `Stop`-hook mechanism behind Phase 4. Two earlier drafts (a per-repo index design and a
separate claude-mem replacement note) were consolidated into it; the vault moved from per-repo to
central in the process, so any surviving reference to a per-repo vault is stale.

---

## What Q10 actually governs

> **Historical, kept because the reasoning is still the reasoning.** Q10 as described below no
> longer exists: D48 settled the architectural half, D49 took the decision the rest was holding,
> and D59 supplied the withdrawal condition it wanted. `OPEN-QUESTIONS.md` Q10 has been narrowed to
> the one thing still genuinely open — *is a note derived in one project ever wanted in another?* —
> which nothing below answers, because the memory layer has only ever run in one repository.

Stated before the phases, because getting it wrong gates the cheap experiment behind the expensive
argument.

D2 settles where **decisions** live. D16 settles whether a **promotion** command may exist. Neither
says anything about the third kind of traffic this plan is mostly about:

| Traffic | Addressed? | Lifecycle | Home | Settled by |
|---|---|---|---|---|
| Direct message / broadcast | yes | consumed once | bus | D17 |
| Claim | implicitly | expires | bus | D5, D13 |
| **Session observation** | **no** | **decays** | **vault** | **nothing — this is the gap** |
| Candidate | no | expires unless promoted | vault | **D16 territory** |
| Decision | no | never consumed | vault, exported to repo | **D2 territory** |

A session observation is unaddressed like a decision but decays like a claim, and **D11 forbids
writing it inside a repository**. There is no settled decision it can violate, because there is no
settled decision about it. The vault is the only place it can live.

So:

- **Phase 1 is outside Q10.** Observations only. No candidate, no promotion, no decision, no
  export. Nothing in it needs D2 or D16 amended, reinterpreted or reconciled.
- **Phases 2 and 3 are Q10.** The moment a candidate is promoted to a decision, D16 is engaged;
  the moment a decision's authoritative home is the vault, D2 is engaged.

**Why the ordering is the point.** Phase 1 carries the receipt that decides everything else —
*does injected memory change what a session does?* If the answer is no, Q10 never needs settling,
because Phases 2 and 3 are never built. Settling Q10 first spends the expensive argument before
knowing whether the feature is worth anything.

### D27 moved the ground under this question — in both directions

**D27 redefined what `amb` is**, after the platform shipped native session-to-session messaging:
not "a message bus for concurrent sessions" but **"a durable, place-addressed board with advisory
file claims"**. Q10 is now being argued against that narrower identity, and the argument cuts both
ways. Whoever settles it should engage both halves rather than the convenient one.

**For the vault.** D27's surviving claim is precisely *durability* and *place-addressing* — reach
a recipient who is not running, address a repository rather than a session, leave a claim visible
to a session that starts tomorrow. A vault of session observations addressed to a project is that
same shape with a different lifecycle, and Phase 4c's cross-repo query (*"who touched `src/auth`
recently, in any repo on this machine?"*) is place-addressing in its purest form. On this reading
memory is not a fourth concern; it is the third one generalised.

**Against the vault.** D27's own words are that the claim *"narrows, and gets more defensible"*.
A memory layer widens it again, immediately, and `D·four-concerns-one-binary` now has to argue
against a definition that was deliberately tightened rather than against a vague one. "One
install, one store, one cross-repo axis" is a weaker answer to a project that just earned its
focus than it was to one still looking for it.

**And the existential note D27 leaves is directly relevant.** It records what would end the
messaging half: *"if the platform ever persists messages for absent sessions and grows a broadcast
address, the messaging half of this project is finished and only claims remain."* Q10 therefore
also asks whether the durable vault is what `amb` becomes when that happens, or a second thing to
maintain while the first is obsoleted. Neither answer is obvious; the plan's position is only that
Phase 1's receipt is evidence for whichever one is argued.

---

## Patterns borrowed from devt (proven in the field, not invented here)

devt's recent arc solved problems this plan would otherwise hit blind. Five worth taking:

**Agreement over presence.** Checking that a thing *exists* is not checking that it *agrees*.
devt's `assert-correction-applied` makes partial application arithmetic — `missing[]`, `extra[]` —
because "a completion claim without the id ledger is indistinguishable from a stale state." Every
verification in this plan follows that shape.

**Promotion is an arithmetic identity.** devt's severity promotions carry
`{id, from, to, reason}` and the verdict compares against ledger-adjusted totals. Promotion is
never a judgement call downstream; it is arithmetic against a recorded ledger. Phase 2 uses this
directly.

**The id echo.** A consumer claiming to have used something must echo the id it used. This solves
the counting problem this plan otherwise has (see Phase 2).

**Disk outranks status.** A safety gate that passes beside five on-disk reviews is not a safety
gate. Where a status field and the filesystem disagree, the filesystem wins.

**Empty is not broken.** devt reserves the bare `{}` for "genuinely unavailable" and requires a
one-line checkable claim for "nothing matched", because every agent otherwise burns a
is-this-broken check. Injection in Phase 1 does the same.

Also taken: kill-switch config per feature, honest limits stated in code, and scope the injection
to the consumer (devt's lane-scoped signal exists because five lanes wrote near-identical
explain-aways of the same two global docs).

---

## What claude-mem's live corpus shows (measured, not read about)

This plan calls itself a claude-mem replacement, so the incumbent was inspected directly rather
than from its README. **Measured 2026-08-27** against the install on this machine —
`~/.claude-mem`, plugin version 13.16.1 — schema, aggregate queries and shipped source. No note
contents were read.

**Scope of the claim, stated first.** The plugin is currently **disabled**
(`"claude-mem@thedotmack": false`, no hooks registered), and capture stops on 2026-08-07 for that
reason. Nothing below infers an outage from that gap.

### The relevance counter that was never incremented

`observations` carries `relevance_count INTEGER DEFAULT 0`, added in its schema version 26.
Someone had exactly the instinct behind this plan's citation ledger.

```
sqlite> SELECT count(DISTINCT relevance_count), max(relevance_count), count(*) FROM observations;
1|0|80264
```

**Every one of 80,264 rows is zero.** In the shipped source, `relevance_count` occurs only in the
`ALTER TABLE … ADD COLUMN` that creates it and in the `PRAGMA table_info` check that guards it —
never in a `SELECT`, never in an `UPDATE`.

**This is the receipt for `note_events`, and also the warning about how it dies.** A relevance
signal implemented as a *column on the note* requires the read path to remember to bump it, and
the read path never did. Implemented as a *table the read path writes*, recording an `injected`
row for everything it showed, it cannot silently become decorative — because writing it is how
injection happens, not an extra step after it. Phase 1's design is already the second shape; this
is why it must stay that shape.

### What unbounded capture costs

| Measure | Value |
|---|---|
| `claude-mem.db` | **570 MB** |
| `chroma/` (vector store) | **3.0 GB** |
| observations | **80,264** across 21 projects, 407 sessions |
| per session | **197 observations**, 25 summaries |
| observation prose | **130 MB** |
| largest single project | 25,064 observations, 37 MB |
| 50 observations injected (title + subtitle) | 10,603 chars ≈ **2,650 tokens** |
| the same 50 with `facts` + `concepts` | 92,247 chars ≈ **23,000 tokens** |

Two things follow. Their **capped** injection at ~2,650 tokens is *half* of what D24 measured for
amb's uncapped mail flood — their cap discipline is good and worth copying. But the full
structured record would be **4.4x amb's worst case**, which is what an uncapped memory injection
looks like. And the vector store is **five times the size of the database it indexes**, which is
the cost this plan avoids by staying lexical (`MEMORY-DESIGN.md` §10).

### Dedup that never fired, and work that was never reaped

- **80,264 rows, 80,264 distinct `content_hash`.** The unique index is
  `(memory_session_id, content_hash)` — scoped to a session, so a repeat across sessions is not a
  duplicate. Cross-session dedup has therefore never once fired. Any dedup this plan builds must
  be keyed across sessions or it is decoration; that is the same shape as the counter above.
- **85 `pending_messages`, all still `pending`, dated 2026-04-30 to 2026-05-12.** **43 of 407
  sessions still `active`, from the same fortnight.** The system then ran three more months and
  added 80,000 observations without ever reaping or retrying them. A queue with no dead-letter and
  no reaper does not surface its own backlog — which is exactly what D6 and D23 exist to prevent
  on the bus side, and what any memory work queue here would need from day one.

### The free-text column they abandoned

`observations` has both a structured shape (`title`, `subtitle`, `facts`, `narrative`,
`concepts`, `files_read`, `files_modified`) and a free-text `text` column. Fill rates across all
80,264 rows:

```
title 100%   facts 100%   concepts 100%   files_modified 100%   narrative 98.3%   text 0%
```

**The free-text column is empty in every single row.** This plan's `observe --summary "…"` is the
design they started with and stopped using. Phase 1 adopts the structured shape instead.

---

## Phase 0 · Finish claims — shipped

Not part of memory. Listed because it was the prior commitment, and starting a second subsystem
before the first lands leaves both half-built.

`claim` / `release` / `claims` and the D14 auto-claim hook all ship, covered by
`tests/claims_e2e.rs`. Q9 — the granularity question this phase was waiting on — was settled as
**D19**: an observed claim stores the exact file and aggregates only at display time. **The
remaining instruction still stands: read the observation signal before Phase 4 reuses it.**

---

## Phase 1 · Vault, observe, inject — **SHIPPED 2026-08-27**

Recorded as **D34–D43**. `src/memory.rs`, migration 2 → 3, `amb memory`, `amb hook memory`,
`amb install --memory`; 221 tests, of which 52 are new. The rest of this section is the plan as
written, kept because it is the argument; **What actually shipped**, below, records where the
build diverged from it and why.

The minimal claude-mem replacement. Five pieces, a few hundred lines, no new architecture.

> **The index tables arrive as a migration, not as new DDL in `schema.sql`.** The board now
> carries a real migration ladder keyed on `PRAGMA user_version` (`SCHEMA_VERSION = 2` at time of
> writing), applied in one transaction so a board is never half-migrated. The three memory tables
> are step **2 → 3**, and they fit the ladder's stated precondition — *"amb's migrations are
> additive by construction"* — because adding them drops nothing. Two consequences: `foreign_keys`
> is now `ON`, so `note_paths`' foreign key is **enforced rather than decorative** (which is why
> it had to agree with `notes`' key, corrected in `MEMORY-DESIGN.md` §5); and a board written by a
> memory-aware binary is opened by memory-unaware ones during rollout, so the step must be additive
> in fact and not only in intent.
>
> **The vault is not `amb`'s directory, and D31 governs what that means.** D31 was written after
> permission hardening narrowed a pre-existing `~/scratch` from `0755` to `0700` because a board
> file happened to land in it. Its rule — *narrow the directory only when this open created it* —
> applies with more force here: the vault is a directory the user chose and points Obsidian at,
> and may well be a git repo or a sync root. `amb` creates files inside it and **never touches its
> mode**. The same reasoning covers the vault's own `.git`, if it has one.
>
> **This phase needs a configuration mechanism that does not exist.** `amb` has **no config
> file** — three environment variables (`AMB_DB`, `AMB_AGENT`, `AMB_PROJECT`) and nothing else.
> Every phase below assumes one anyway: the vault path, the injection cap, the promotion
> threshold, and a kill-switch per layer. Introducing config is its own decision with its own
> questions — where does it live, does it sync, does it collide with D15's refusal to open on a
> synced volume, and what happens when it is malformed in a hook that must never fail. Decide it
> before Phase 1, not during. The cheapest answer that stays consistent with the existing design
> is `AMB_VAULT` plus defaults in code, deferring a real config file until something needs a value
> that cannot be an environment variable.

**Nothing in this phase touches D2 or D16.** Observations only: no candidate file is written, no
promotion offered, no decision authored, no repo exported to. It can be built and run while Q10
stays open, and it produces the evidence Q10 should be argued on.

**The vault is plain markdown in a configurable directory**, defaulting somewhere a human would
point Obsidian at — not inside `~/.agent-messageboard/`. It is yours, not `amb`'s data directory.

```
~/vault/
├── projects/<name>/2026-08-27-flaky-fixture-race.md   # one file per observation
├── candidates/                        # Phase 2
├── decisions/                         # Phase 3
├── topics/<topic>/                    # topic-scoped decisions      (D82)
└── global/                            # decisions that apply everywhere (D81;
                                       #   was `patterns/` when this was written)
```

**One file per observation, not one append-only file per project.** The earlier draft had
`projects/<name>/observations.md` appended to forever. That cannot work, for three reasons that
only appear once the index is written down:

- Every project's file has the stem `observations`, so with `PRIMARY KEY (kind, slug)` they all
  collide on a single row. (`MEMORY-DESIGN.md` §5 is corrected to `(kind, scope, slug)` — `project` until D81
  regardless — `note_paths` was already keyed per-project while referencing a table that was not.)
- An individual observation has no identity, so "inject the last N", FTS5 over observations, and
  the id echo below each have nothing to name.
- Rebuilding a disposable index from an append-only blob needs a stable in-file anchor. Separate
  files are that anchor, for free.

Obsidian prefers many small notes anyway, and `rm board.db` still loses nothing.

Files are truth; `board.db` holds only a derived index. The test, unchanged from
`MEMORY-DESIGN.md`: **`rm board.db` must lose zero notes.** Vault may live in git or iCloud —
plain text syncs fine. The index must not (D15 already refuses synced paths), so it rebuilds
locally per machine.

**Ships:**
1. `amb memory observe` — writes one dated note under the project and indexes it. **Structured,
   not free text**, following the shape claude-mem's corpus validates: a one-line `--title`, the
   `--files` it concerns, and a short `--learned`. `--cites <id>…` records that this observation
   was prompted by something injected; `--supersedes <id>` retires an earlier one.

   The frontmatter is the structure and the body is prose beneath it, so the file stays
   Obsidian-readable while the index has fields to anchor on. Free text was the first design and
   is the one claude-mem abandoned — 0% fill on its `text` column against 100% on `title`,
   `facts` and `files_modified`.
2. `SessionStart` hook — injects observations concerning the caller's project **under D24's
   three rules** (below), and **every injected note is rendered with its id**.
3. **`PreToolUse` with `matcher: "Read"`** — inject what is known about a file *immediately before
   the agent opens it*. This is the best idea in claude-mem's hook layout and it costs nothing
   extra here: the query is the same path-anchored lookup `SessionStart` already does, narrowed to
   one path. It is also the strictest possible form of "scope the injection to the consumer" — at
   `SessionStart` the relevant file is a guess, and at `PreToolUse(Read)` it is stated. Subject to
   the same cap and the same ledger row, so a file-context injection that is never cited is
   measured like any other.
4. Redaction on the write path — `<private>` exclusion plus a secret-shaped-string filter.
   Non-deferrable: everything captured eventually reaches a model.
5. **A write-path skip list.** claude-mem ships `CLAUDE_MEM_SKIP_TOOLS` — `TodoWrite`, `Skill`,
   `AskUserQuestion` and similar — and **197 observations per session** is what the noisy tools
   cost without one. Nothing worth remembering happens in a `TodoWrite`.
6. Empty-vs-broken discipline: no match injects *"no prior observations for this project"*, not
   an empty block; only a genuinely unreadable vault injects the unavailable marker.
7. **The citation ledger.** Every injection records the ids it showed; every `observe --cites`
   records the ids it used. Two counters, one table, no judgement — and, per the corpus evidence
   above, a *table written by the read path* rather than a *column something must remember to
   bump*.

**Injection reuses D24 rather than reinventing it.** D24 is not merely decided — it is
**implemented in `delivery::render_all` and covered by `a_flood_of_mail_is_capped_and_says_so`**,
so this is a working implementation to copy rather than a principle to re-derive. Same problem,
different payload:

> **Measured 2026-08-27:** sixty unread rendered **20,779 characters — roughly 5,200 tokens —
> injected at every turn boundary, byte-identical each time**, because nothing drained an
> unacknowledged inbox.

Its three rules were three separate defects, and all three apply verbatim to memory:

1. **Cap the count, spelled out.** D24 found the per-message body preview "bounded the wrong axis"
   — one line each was already right; the *count* was what grew. Memory will fail the same way.
2. **Admit what was hidden.** "A reader who cannot tell *ten messages* from *ten of sixty* is
   being misled by the cap rather than helped by it." An injection that silently truncates the
   vault is worse than one that injects nothing.
3. **Order by scope before recency**, so a stale cross-project note cannot push out the local one
   that concerns the file being opened.

**External corroboration for the size of the problem:** a mature memory product reports **~6,900
tokens per query** for injection at steady state — the same order as amb's own measured 5,200 for
unbounded mail. Injection is a permanent tax and both numbers say so.

**D23 is the retirement precedent.** It counts offers per recipient and backs off after ten:
*"past it the message is not being missed, it is being declined."* That is a shipped, measured
decay rule, and it is the shape the "do observations expire?" question should take — a note that
has been injected N times without ever being cited is being declined, not missed.

**The id echo belongs here, not in Phase 2.** The earlier draft introduced it in Phase 2 to
distinguish derivation from citation. It is cheaper and more valuable one phase earlier, for two
reasons:

- **It makes Phase 1's receipt arithmetic instead of self-report.** This plan borrows devt's rule
  that *"a completion claim without the id ledger is indistinguishable from a stale state"* — then
  asks the decisive question of this whole document as *did anything injected change what you
  did?*, answered by an agent about itself. That is exactly the shape the borrowed rule forbids.
- **It is the only thing that makes Phase 2's counting rule true.** See Phase 2.

Rendering an id costs a few bytes on an injection that is already being rendered.

**Two failure modes the field treats as standard, and this plan had no story for.** Both are
Phase 1 constraints rather than later features, because both are cheaper to design in than to
retrofit once a vault has content:

- **Staleness is the most-cited failure of memory systems** — the canonical example is a fact that
  is "accurate until they change jobs, at which point it becomes confidently wrong". The plan
  previously deferred expiry to an open question. It cannot be deferred *silently*: at minimum
  every injected note renders its age, so a reader can discount it without the system having to
  decide. D23's back-off then supplies the retirement rule once the ledger has data.
- **Contradiction has no mechanism at all.** The vault will hold "we use X" and, later, "we moved
  off X". `notes.status` has a `superseded` value and **nothing in this plan ever writes it**.
  Today both notes are injected and the model picks — which is the worst of the three options.
  Phase 1's floor: `observe --supersedes <id>` marks the older note, superseded notes are never
  injected, and the file records what replaced it. Detecting contradiction automatically is out of
  scope; *representing* it is not optional.

**Deliberately not yet:** the Stop hook. Call `observe` by hand (or let the agent call it when it
judges the moment worth recording) for one to two weeks.

**Receipt for Phase 1 → next — arithmetic, not impression:**

```
cited_ids / injected_ids     over two weeks of real sessions
```

If the ratio is **zero**, stop. The whole memory idea is answered cheaply, no automation is built,
and Q10 never needs settling. If it is non-zero, the ids say *which* notes earned their place —
which is also the first real input to the injection cap and to the token-cost measurement.

A ratio near zero with a large denominator is a distinct and useful failure: injection is working
and is *noise*. Fix retrieval before building anything else.

### What actually shipped, and where it diverged

Every one of the seven ships landed. Four things came out differently, and each is recorded as a
decision rather than left as an undocumented difference between this file and the code.

| Planned | Shipped | Why |
|---|---|---|
| `AMB_VAULT` "plus defaults in code" | `AMB_VAULT` and **no default path** (D35) | Every other value keeps a default. A vault path cannot have one: a wrong default creates a directory nobody asked for and starts filling it. Unset is the kill switch. |
| Empty-is-not-broken everywhere | `SessionStart` always speaks; `PreToolUse` is silent on no match | devt's rule answers a consumer that *asked*. `PreToolUse` fires dozens of times a turn and was not asked — "nothing known about `src/foo.rs`", forty times, is the noise the cap exists to prevent. |
| One citation ledger | **Two counters** — `injected` and `injected_file` (D42) | `PreToolUse` `additionalContext` is undocumented and therefore unverified. Counting it in the denominator would let a discarded injection drive the ratio to zero, and the stopping rule below retires the feature at zero. |
| FTS5 as a supplement | **The index narrows and the file decides** (D88); no FTS5 yet | §6's receipt for full-text is borrowed and covers decisions only. Building the answer before the measurement is what this plan exists to avoid. This row read "`LIKE` over titles and excerpts" until 2026-08-29, and that was the defect rather than the design: `body_excerpt` is the first paragraph capped at 240 characters, so the search reached almost none of each note. A **contentless** FTS5 table (`content=''`) would satisfy D34 and is the escalation — but the trigger is the `searches` ledger (D89) saying lexical recall is what is missing, and until that ledger existed it could say nothing. |

**Five things the build found that the plan did not have:**

- **D43 — the hidden count cannot be derived by the renderer.** `SessionStart` selects with
  `LIMIT 8`, so `notes.len() - shown` is zero and the injection silently truncates the vault. This
  is **D33's seam in a new place**, and the renderer's own unit test could not see it; an
  end-to-end test caught it.
- **The installed binary is stranded by a schema bump.** Hooks invoke a *copy* of `amb` named in
  `settings.json`. Migration 1 → 2 dropped `messages.failed_at`, and the copy at
  `~/.local/bin/amb` predated it — so every hook on this machine was failing with
  `no such column: m.failed_at` and **mail delivery was silently dead machine-wide**. Found while
  starting this phase, fixed by reinstalling. It is why migration 2 → 3 is additive in fact and
  not only in intent.
- **A `.claude/settings.json` on this machine already carries a foreign `PreToolUse` matcher**
  (`matcher: "Bash"`, belonging to another tool). Covered by a test now.
- **D47 — the primer was arguing for its own receipt.** It told the reader that "this echo is the
  only measure of whether any of this earns its context, so please do it", inside the context whose
  output is that measure. Caught by validating the design against the sycophancy literature rather
  than by testing the code, and it is the only defect found here that no test could have caught,
  because the code was doing exactly what it was written to do.
- **D45 — ship #6's taxonomy was one row short.** *Empty* and *broken* do not cover *an index this
  hook is deliberately not maintaining*: above the 500-note bound the rebuild declines, and a vault
  of 501 notes rendered as "no prior observations". Found by auditing this document against the
  code after every one of the seven ships had been confirmed present — which is the argument
  against trusting a checklist.

**Measured, as this section required — `MEASUREMENTS.md` M9.** Injection is **~355 tokens** at
eight notes and **~377** at a thousand: flat, because the cap binds. Against the two reference
points this plan set — D24's 5,200 for unbounded mail and a vendor's reported ~6,900 per query —
that is roughly one fourteenth to one twentieth, and it does not grow. The hook costs 2.9–4.2 ms
against the delivery hook's 2.9–3.3 ms, and 2.0–2.3 ms when switched off.

M9 also records an expected optimisation that **measured as no change at all**, and says so rather
than quietly keeping the code and the claim.

**Deliberately still not done:** the FTS5 index, the `Stop` hook, and any expiry rule. All three
wait on the receipt.

**Reading the receipt:**

```
amb memory status --days 14
```

Zero cited against a non-zero `injected` after two weeks of real sessions **is the answer**, and
the answer is to switch it off — `amb install` without `--memory`. `amb memory status` prints that
instruction itself, so the stopping rule is visible where the number is read rather than only in
this document.

---

## Phase 2 · Candidates and promotion — **BUILT (D49–D51)**

> ~~**Q10 territory, blocked until Q10 settles.** Do not start it before Phase 1's receipt is
> non-zero *and* Q10 is settled — in that order, because the receipt is the best evidence Q10 will
> ever get.~~
>
> **Overridden by direction, 2026-08-28.** Built with the receipt at `0` cited. The precondition is
> struck through rather than deleted so the sequence break stays legible; D49 records the decision
> and the condition under which the phase is withdrawn.

The three-strikes model. This is the phase where devt's patterns earn their place, and the phase
D16 was entitled to veto.

**Two tiers, and only one of them governs.**

| Tier | File | Injected? | Why |
|---|---|---|---|
| Candidate | `candidates/<slug>.md` | **No** | Cannot be circular if never shown |
| Decision | `decisions/`, `topics/` or `global/` | Yes | Earned trust through promotion |

**The counting rule — the crux, and the earlier draft got it wrong.** A candidate promotes on
**independent derivations**, not on citations. The draft's justification was:

> *"Since candidates are never injected, every derivation is independent by construction."*

**That is false, and it is the load-bearing sentence of the phase.** Candidates are indeed never
injected — but *observations* are (Phase 1) and *decisions and patterns* are (Phase 3). An agent
that reads an injected observation about auth lock ordering and then proposes a candidate about
auth lock ordering has produced a citation wearing a derivation's clothes. The guard covered one
of three injected kinds, and left the highest-volume kind unguarded. "By construction" is the
phrase that stops a reader checking, which is why it earned its own correction here.

**The rule that actually holds** needs the Phase 1 citation ledger, and is a property of the
session rather than of the candidate:

> A derivation is independent when **nothing injected into that session** — observation, decision
> or decision at any scope — concerned the paths the new note concerns. Anything else is a
> citation. (Said as "or pattern" before D81 made a pattern a decision at `@@`.)

This is checkable because Phase 1 already records what each session was shown. Without that
ledger the rule is unenforceable, which is the second reason the id echo moves to Phase 1.

Two consequences worth stating:

- **Independence gets rarer as the vault fills.** That is correct behaviour, not a defect — a
  system that has already told you something should not take credit for you repeating it.
- **The count measures rediscovery, not truth.** Three independent derivations mean a thing keeps
  being *noticed*, not that it is *right*. This plan's own open questions concede that "frequency
  favours trivia". The arithmetic is honest about what it checks; what it checks is a proxy for
  importance. The user's approval, below, is what supplies the missing judgement — which is
  precisely why that approval must not degrade into a rubber stamp.

**The strongest remaining attack on this phase is sycophancy, and the counting rule does not
touch it.** 2026 has a named benchmark for it — *MemSyco-Bench* — covering two behaviours that
land directly here:

- **Agents write memories that flatter or agree with the user** rather than record what happened.
- **Self-confirming loops form when an agent both writes and reads its own memory** without
  external validation.

The counting rule defends against *citation contaminating derivation*: the same note being fed
back and counted twice. It has no defence against **one bias sampled three times**. Three
independent derivations produced by the same model, agreeable in the same direction, satisfy every
mechanical test in this phase and are not independent in any sense that matters. Cross-project
spread does not help — the model is the same in all three projects.

**Checked against current practice on 2026-08-28, and the field does have answers — all of which
collide with D3.** The plan previously recorded this as an unsolved limit. It is not unsolved; it
is solved expensively:

| Published mitigation | Cost here |
|---|---|
| **Write-ahead validation** — a separate, smaller model judges each proposed memory write before it commits | a second model on the write path |
| **Distill-Verify** — a third-party agent analyses the trajectory; unanimously approved experience goes to shared memory, partially approved to private | multi-agent verification per candidate |
| **Human-in-the-loop before writing to shared memory** | what Phase 2 already proposes |
| **Adversarial probing** — periodically challenge stored beliefs with counterexamples | a second model, periodically |

So the honest position is sharper than "no defence exists": **every external validator the field
has found is a second model in the loop**, and D3 rejected exactly that class of dependency — a
process to supervise, or a second API key. Phase 2 therefore costs more than the plan admitted, and
the choice is to pay it or to drop the phase. The three-strikes count alone is not a defence and
the human gate is the field's own weakest listed option, not a clever one.

Two consequences, and neither is a mitigation so much as an honest limit:

- **The human approval gate is load-bearing for a second reason.** It is not only what reconciles
  D16; it is the only external validator in the loop. Everything above about keeping approval
  expensive applies twice over.
- **A promoted decision that nobody ever cites is the observable symptom.** The Phase 1 ledger
  already measures this. If promotions accumulate and citations do not follow, the pipeline is
  manufacturing agreeable trivia and should be switched off — which is what the kill-switch is
  for.

*Reference read at abstract level only; cited as a named risk, not as evidence for a number.*

**Derivation evidence lives in the candidate's frontmatter**, not only in the index, or
`rm board.db` destroys the promotion evidence:

```yaml
derived_count: 3
derived_in: [nestwatch, amb, devt]     # projects, not sessions
derivations:
  - {ts: …, project: nestwatch, session: …, note: "…"}
```

A human reading the file sees *why* it is being promoted. Rewriting a small file per increment
costs nothing at this volume.

**Promotion is arithmetic, not vibes.** At the threshold (config, default 3) the promotion offer
states the ledger: three derivations, these projects, these dates. The user approves. Approval
writes `promoted_from`, the derivation ledger carries over, and the candidate file is archived,
never deleted.

**The human approval is what reconciles D16, so it has to stay expensive.** D16's objection is to
"a mechanism that looks authoritative while checking the part that matters not at all". Phase 2
survives that objection on one ground only: the arithmetic is advisory, and a person supplies the
judgement the count cannot. Every design choice that makes approval cheaper attacks the
reconciliation:

- **One candidate per offer.** A batch of six with a single confirmation is a rubber stamp, and a
  rubber stamp is D16's defect with extra steps. The earlier draft's "offer batched at session
  end" is withdrawn for this reason; batching the *timing* is fine, batching the *approval* is not.
- **The offer shows the derivations, not just the count.** Three dates and three one-line notes,
  so approving requires reading something.
- **Declining is recorded and cheap.** A declined candidate is not re-offered until it derives
  again. Without this, approval becomes the path of least resistance.

If approval measurably degrades to reflex — and the ledger can show that, since decline rate is
observable — Phase 2's D16 argument has failed and the phase should be withdrawn, not patched.

**The destination is decided by the ledger, not by the user's mood** — and this is `amb`'s unique
capability, since no per-repo tool can see it:

- derivations in **one** project → `decisions/<project>/` (a project decision)
- derivations in **two or more** projects → `topics/` if they share one, else `global/` (D82;
  this said `patterns/` when the router had two rungs)

**Dedup is an affordance, not an algorithm.** Deciding that a new note "is the same as" an
existing candidate was listed as the hardest unsolved piece here, with normalized-title-plus-paths
and accepted misses as the fallback. It does not need solving. Retrieval is already path-anchored,
so at `observe` time the candidates concerning those paths are a free query — show them, with
their ids, and let the agent echo one:

```
amb memory observe --title "lock ordering" --files src/auth/lock.rs --same-as cand-auth-lock-ordering
```

That replaces fuzzy matching with a checkable record, produces exactly the derivation evidence the
ledger needs, and fails safe: a missed link creates a duplicate candidate, which is visible and
mergeable, rather than a wrong merge, which is neither. **A near-match shown at observe time is
also an injection** — so it is recorded in the citation ledger, and a candidate derived after
seeing one is a citation, not a derivation, by the rule above.

**Also ships:** direct promotion (skip candidacy when something is obviously important on first
sight — frequency favours trivia, so judgement needs an override) and candidate expiry (30 days
without re-derivation; unpromoted is not permanent).

**Kill-switch:** `memory.promotion_enabled: false`.

---

## Phase 3 · Decisions and project export — **BUILT (D49)**

> ~~**Q10 territory.**~~ D2 is revised rather than overridden: its first ground — a decision has no
> recipient — stands, and nothing here puts a decision in the bus. Its second is answered by the
> vault being a git repository, leaving `amb` an index and an injector.

**Vault is truth; repo copies are generated publications.** One direction only.

**D2's second reason is answered better than the earlier draft admitted.** D2 gives two grounds.
The first — *a decision has no recipient* — this design agrees with completely and never contests:
decisions do not travel through `messages`. The second is scope, and ends *"git already is that
system"*. The draft simply conceded it (*"the scope expansion D2's second reason warned about"*).

The concession is unnecessary, because **the vault is itself a git repository of markdown**. Under
that reading `amb` never becomes a documentation system: search, versioning, review and
durability-measured-in-years all stay git's, exactly as D2 requires, and Obsidian supplies the
reading surface. What `amb` adds is an **index and an injector** over a documentation system that
already exists — which is a weekend, and is the thing git does not do.

That is the argument Q10 should be decided on. It is stronger than "yes, this is scope creep, but
worth it", and it is falsifiable: if building this requires `amb` to grow search, revision history
or a viewer, D2's second reason has won and the design was wrong.

`amb memory export <project>` writes `docs/decisions/` in that repo with a generated-by header,
committed so anyone who clones the code finds them — satisfying D2's "put it in the repo it
governs" without `amb` ever authoring a decision in place (D11 intact, since export is an explicit
user command targeting a path the user names).

**`amb memory export --check` is not optional.** It exits non-zero when a repo's exported copy is
stale against the vault. Without it, record-centrally-forget-to-export produces exactly the
synchronization decay the ADR literature names, and D2's model had no such failure state. Wire it
into CI or a pre-commit hook: drift becomes a detected failure, not a silent one.

Two things from devt's arc apply here directly: **no export ships a placeholder** — hard-fail on
any unsubstituted token in a rendered file — and **disk outranks status**: `--check` compares file
content hashes, never an `exported_at` field that can lie.

**Open before building:** whether export is per-decision opt-in (`scope: project` frontmatter) or
dumps everything tagged with that project. I lean opt-in — a personal principle derived partly in
your employer's repo should not auto-publish there.

---

## Phase 4 · Automation and cross-repo — **capture half SHIPPED, 4a declined (D52)**

Built at the user's direction, before the receipt this phase was gated on. **4b's deterministic
facts, `PostToolUseFailure` capture, 4c's cross-repo surface and the fail-loud counter ship; 4a's
blocking `Stop` self-compression is refused** — its own gate says to test the non-blocking
alternative first, and blocking a turn puts unmeasured work inside D9. D52 records what would
change that.

### Before any of Phase 4 is written: verify the hook surface

**Re-checked against the primary source on 2026-08-28, and the result changes this phase's
design.** The earlier pass read the reference in summary and produced three findings; one of them
was wrong, and the error had already hardened into a decision elsewhere (D42, amended). So this
pass quoted the *Decision control* and *exit code 2* tables directly, and marks how each line was
obtained.

| Claim this phase rested on | Status |
|---|---|
| `stop_hook_active` is an input field on `Stop` | **Settled — it exists.** Undocumented on two full readings, then observed as `stop_hook_active: true` in a live `Stop` payload (`MEASUREMENTS.md` M10) |
| Blocking `Stop` is `{"decision":"block"}` | **Wrong — it is exit code 2.** *"`Stop` · Yes · Prevents Claude from stopping, continues the conversation"* |
| Summaries need `transcript_path` | **Wrong, and for a better reason than first recorded** — see below |
| `PreCompact` can inject context | **False.** It can *block* (exit 2 blocks compaction) and supports no `additionalContext` |
| `PostCompact` gives a re-injection point | **False.** *"`PostCompact` · No · Shows stderr to user only"* — it can neither block nor inject |
| Hook timeouts are per entry | **Confirmed.** Default 600 s per `command` entry; `UserPromptSubmit` lowers it to 30, which is why D9 chose `Stop` over it |

**4a's most attractive idea does not work as designed, and this is what the gate is for.** The plan
proposed `PreCompact` to trigger a self-summary and `PostCompact` to re-inject it — *"the session
that just lost its context is the session most in need of what was learned in it"*. Neither event
can inject anything.

**The idea survives on a different event.** `SessionStart` accepts a matcher, and one of its values
is **`compact`** — *"when a session resumes after compaction"*. That is precisely the re-injection
point `PostCompact` was wanted for, and it sits on an event whose `additionalContext` this project
has **verified first-hand twice**: `delivery::envelope` records the original probe, and the memory
hook shipped in Phase 1 does it every session. So 4a becomes: trigger on `PreCompact` by *blocking*
if a summary is wanted, and re-inject on `SessionStart` with `matcher: "compact"`.

> **A caution about how this was checked, because it nearly went wrong again.** Two fetches of the
> same page returned *contradictory* answers on whether `SessionStart` supports `additionalContext`
> — one quoted the Decision control table listing it, the other inferred "likely not" from the
> *blocking* table, which is a different question. The tiebreaker was not a third reading; it was
> that `amb` already does it and it works. **Where a document and an observation disagree, the
> observation wins** — and where two readings of one document disagree, neither is evidence.

**`last_assistant_message` is not merely more convenient than the transcript — the transcript can
be wrong.** The reference states the file *"is written asynchronously and may lag the in-memory
conversation, so it may not yet include the current turn's most recent messages when a hook
fires"*. 4b's split therefore stands on a correctness argument rather than a tidiness one: the
summary must come from `last_assistant_message` (on `Stop` and `SubagentStop`), and only the
*facts* may be parsed out of `transcript_path`, where lag costs completeness rather than accuracy.

**And the re-injection point turns out to be already wired.** Phase 1 installs `SessionStart`
with no matcher, so it fires on every source — verified 2026-08-28 by driving the hook with each
of `startup`, `resume`, `clear`, `compact` and `fork`, plus an unknown value: all five inject, and
the unknown one exits 0. **So the hook Phase 4 wanted for post-compaction re-injection is running
today.** What is missing is not the trigger but the *content*: it re-injects the project's recent
notes, where 4a would re-inject what *this session* had learned before its context went away.

Deliberately not refined now. Making the injection aware of `source` is a two-line change and it is
Phase 4 work — building it before the receipt exists is the thing this plan's ordering is arranged
to prevent.

**The gate is closed — `MEMORY-DESIGN.md` §9.2 can now name its mechanism.** `stop_hook_active`
exists despite being absent from the reference, and it was seen **`true`** in a payload delivered
to a hook that blocks and continues the conversation — that is, at the exact moment the session was
in the re-entry state the field describes. The circumstance is the confirmation: a flag that is true
precisely when a `Stop` is happening *because* a `Stop` hook fired is the guard §9.2 requires, and
§11's infinite-loop risk moves from unmitigated to mitigated.

**Provenance is marked rather than smoothed over.** That is a report from a hook that received the
payload, not a byte captured first-hand. It is empirical rather than documentary and it is
consistent with both the semantics and the situation, but a direct capture is stronger — and M10
records how to get one, because the obvious way failed: **a `Stop` hook added to
`.claude/settings.local.json` mid-session never fired across two turns**, though the reference says
hooks reload without a restart. Install the entry, then start a new session.

**So Phase 4 is unblocked on its own terms — and still gated on Phase 1's receipt**, which is the
other precondition and reads `0/0`. None of this required Rust, which was the rule.

### And it must not endanger D9

The `Stop` hook that would block for self-compression is a hook that also delivers mail — and as
of **D25**, so is `PostToolUse`. D9's hard requirement is that mail delivery never breaks a
session, and it is currently mutation-tested. Phase 4 would put unmeasured, LLM-adjacent work —
reading a vault that may sit on a synced volume, parsing a transcript, deliberately blocking a
turn — inside that guarantee. A hang in memory takes delivery with it, and the failure looks like
an empty inbox, which is this project's documented worst failure shape.

**The isolation is structural, not a discipline.** Hook timeouts are **per entry** (platform
default 600 s; `amb` self-imposes 5 s), so memory registers its **own hook entry** rather than
extending the command that delivers mail. A memory entry that hangs then burns its own budget and
nothing else. The hook-safety tests are extended to cover a memory layer that **hangs, panics or
returns garbage**, not merely one that is absent.

**Fail soft — but count, and eventually say so.** D9's silence is right for delivery, where the
worst case is a message arriving a turn late. It is wrong as an unlimited policy for a capture
layer, where the worst case is months of believing something is recording when it is not.
claude-mem ships `CLAUDE_MEM_HOOK_FAIL_LOUD_THRESHOLD: 3` for exactly this, and its own corpus
shows why the threshold matters: **85 queue items and 43 sessions sat in a non-terminal state from
2026-04-30, and the system ran three more months and added 80,000 observations without ever
surfacing them.** So: swallow every error, keep a consecutive-failure count, and after N say one
line — never by failing the hook, and never on the delivery path. For the same reason,
`amb memory` gains a status surface that answers *is this actually capturing?* without reading a
log.

**And note which architecture produced that backlog.** claude-mem compresses through a background
worker on a local port; its own logs record the worker restarting and losing SDK context, after
which its parser rejected the output it was handed. D3 rejected broker daemons on the argument
that they are *"a process to start, supervise, restart after crashes and reboots… a single point
of failure a file does not have"*. The incumbent is a working demonstration of that cost, and the
`PreCompact` design below exists to avoid needing the daemon at all.

**4a · Self-compression, triggered by `PreCompact` and re-injected at `SessionStart(compact)`.**
The mechanism is unchanged from `MEMORY-DESIGN.md` §9.1 — ask the session to summarize itself,
since it already holds the context; no worker, no second API key, D3 intact. **What changes is
when it fires — and the gate above changed it a second time**, because neither compaction event
can inject and only `PreCompact` can block.

Earlier drafts triggered on a turn-count threshold. A threshold is a *proxy* for "enough has
happened to be worth recording". The hook surface has the event itself:

| Event | Fires | Matcher | Blocks | Injects |
|---|---|---|---|---|
| `PreCompact` | **before context is compacted** | `manual` \| `auto` | yes | **no** |
| `PostCompact` | after compaction | `manual` \| `auto` | no | **no** |
| **`SessionStart`** | **after the session resumes** | includes **`compact`** | no | **yes** |

**Compaction is the moment session context is about to be destroyed** — which is exactly when a
summary is worth most and cheapest to produce, because the material is still in context and the
harness has already decided it is going away. A turn threshold guesses at that moment; `PreCompact`
*is* it, and it costs nothing on sessions that never compact. **`SessionStart` with `matcher: "compact"`** then gives
the re-injection point — the session that just lost its context is the one most in need of what was
learned in it — on the one event whose injection this project has verified first-hand. `PostCompact`
was the obvious candidate and cannot do it.

`SessionEnd` is the other candidate and is the wrong one for heavy work: it shares a **1.5-second
budget** across all hooks (raisable to 60 s only by explicit per-hook timeout), and a session that
ends by `logout` or `clear` may not be around to finish. Use it, if at all, for a cheap flush.

Whichever event is used, the guard against a hook that blocks forever must be verified to exist
first — see the table above. And test the non-blocking `--append-system-prompt` alternative
before the blocking one; if it produces comparable summaries it costs less and cannot hang a turn.

**4b · Deterministic session facts — and the summary is not one of them.** Split the two sources,
because the reference is explicit that they are not interchangeable:

- **The summary** comes from `last_assistant_message`, provided directly on `Stop`. The docs say
  hooks needing the final assistant text *"should use `last_assistant_message` … instead of
  reading the transcript"*, and it is a field read rather than a file parse.
- **The facts** — files touched, commands, exit codes, failures — come from `transcript_path`.
  Zero LLM, Rust's home ground, and the only source that has them.

The transcript is an internal format with no compatibility promise, so parse defensively and fail
soft; that risk now falls on the *facts* only, and no longer on the summary. Facts share the D14
observation signal: a claim is that observation with an expiry, a session fact is the same one
without.

**Worth noting for later, not for now:** `PostToolUseFailure` exists as its own event. Failures
are disproportionately what is worth remembering, and capturing them does not require any of 4a.

**4c · Cross-repo queries.** *"Who touched `src/auth` recently, in any repo on this machine?"* The
one capability no per-repo tool has. Foreign results stay labelled and advisory, per
`MEMORY-DESIGN.md` §Trust — cross-repo is the shared-root case by default, and devt shipped
attribution, found it insufficient, then bypassed its own tier three releases later. Advisory by
construction, not by a config flag a later feature can forget to read.

**Then, and only then:** uninstall claude-mem. Run both in parallel for a few weeks first and
compare what each surfaces at session start. That comparison is the receipt.

---

## What each phase must measure

| Phase | Receipt | Shape |
|---|---|---|
| 1 | `cited_ids / injected_ids` over two weeks. Zero — stop, and the rest is unbuilt | **arithmetic** |
| 2 | Do candidates reach 3 *independent* derivations at all? And what is the decline rate on offers — a decline rate near zero means approval has become reflex, and the D16 argument has failed | **arithmetic** |
| 3 | Does `--check` ever fire? If never, either export is unused or hygiene is perfect — find out which | counter |
| 4 (gate) | **Mostly settled 2026-08-28 from the reference** — exit-2 blocking, `last_assistant_message`, and the compaction events' inability to inject. One question left: does a `Stop` re-entry guard exist at all? **Probe with a shell hook before any Rust** | **empirical, blocking** |
| 4a | Are self-written summaries usable? Prototype in **shell** before any Rust | judgement |
| 4b | Is the cross-repo query ever run? If not, the differentiator is dead weight | counter |

**Token cost is measured at every phase that injects, against two reference points:** D24's
measured 5,200 tokens per turn boundary for unbounded mail on this machine, and ~6,900 tokens per
query reported by a mature memory product at steady state. Both say the same thing — injection is
a permanent tax, and an uncapped one is the largest single cost this design can incur.

Injection that is never cited is a permanent tax, and the citation ledger is what makes "never
cited" a number rather than an impression.

---

## Decisions this plan needs recorded

Named, not numbered. **Numbers are allocated at acceptance**, in this repo's convention — this file
carries no D-number and must not reserve one, since `DECISIONS.md` is being extended concurrently
and a reserved number here would collide.

Grouped by whether Q10 blocks them, which is the whole point of the split above.

**Buildable now — no Q10 dependency:**

- **D·vault-is-truth** — the vault is authoritative, the index is derived, and `rm board.db` loses
  nothing. The test that keeps the design out of D2's rejected shape.
- **D·observations-are-a-third-traffic-type** — unaddressed like a decision, decaying like a claim,
  and forbidden inside a repo by D11. This is the decision that makes Phase 1 legal under the
  existing spec rather than an exception to it.
- **D·injection-is-ledgered** — every injected note carries an id; a use-claim without an echoed id
  is not a use. Makes Phase 1's receipt arithmetic, and is the precondition for Phase 2's counting
  rule being enforceable at all.
- **D·memory-never-degrades-delivery** — D9 extends to the memory layer: memory registers its own
  hook entry so its timeout is its own, it fails soft and silent, and hook-safety tests cover a
  memory layer that hangs or panics, not only one that is absent.
- **D·injection-obeys-D24** — the memory injection is capped by count, states how many notes it
  hid, and orders by scope before recency. Not a new rule: D24 measured this failure on mail
  (5,200 tokens per turn boundary, byte-identical) and memory is the same defect with a different
  payload.
- **D·supersession-is-represented** — `observe --supersedes <id>` marks the older note, superseded
  notes are never injected, and the file records what replaced it. Automatic contradiction
  *detection* is out of scope; contradiction being *unrepresentable* is not acceptable, because
  the fallback is injecting both and letting the model choose.
- **D·memory-config-is-environment-first** — `amb` has no config file and this plan should not be
  what introduces one by accident. `AMB_VAULT` plus defaults in code until some value genuinely
  cannot be an environment variable; a real config file arrives by decision, with its own answers
  for sync, malformation, and D15's synced-volume guard.

**Blocked on Q10:**

- **D·candidates-are-never-injected** — the anti-circularity rule and the poisoning defence.
  Necessary but, as Phase 2 now records, **not sufficient**: independence is a property of what the
  session was shown, not of the candidate's kind.
- **D·promotion-is-arithmetic-under-human-approval** — a derivation ledger decides *what is
  offered*; a person decides *what is promoted*; project spread decides *where it lands*. The human
  gate is the load-bearing half, because it is what distinguishes this from D16's rejected
  `promote`.
- **D·export-is-one-way-with-a-staleness-gate** — vault → repo, never back, and `--check` compares
  content hashes. Without the gate this design is worse than D2's and should be rejected.
- **D·four-concerns-one-binary** — bus, claims, vault, session memory. Phase 3 argues this is
  *not* the scope expansion D2 warned about, because the vault is a git repo and `amb` supplies
  only index and injection. **D27 raised the bar this has to clear**: `amb` is now defined as a
  durable, place-addressed board with advisory claims, a claim D27 says "narrows, and gets more
  defensible". Anything recorded here must argue against that definition rather than against the
  looser one it replaced — and should say whether the vault *generalises* the surviving claim or
  merely sits beside it. Record whichever way it lands, including a rejection.

---

## Open questions

**Does memory belong in `amb` at all?** The alternative is a second binary sharing the vault
directory but not the database. Cheaper to reason about, one more thing to install.

One argument against the split that was not previously stated: **a second binary would have to
write `~/.claude/settings.json` too**, since memory needs `SessionStart` to inject and `Stop` to
capture. That is the file `CLAUDE.md` singles out as the one whose corruption "breaks the user's
whole tool, not just this one", and two installers contending over it is a worse failure than one
binary with a kill-switch per layer. It also duplicates identity resolution, project resolution and
the sync-root guard. Not decisive, but it belongs in the Q10 argument.

**Threshold value.** 3 is a guess. Config it and revisit once you can see what actually
accumulates.

**Do observations expire?** *Checked 2026-08-28: current practice treats expiry as a **required**
mitigation rather than a refinement* — "retire unvalidated reflections after a set period" and
"decay confidence over time without confirming evidence" appear as standard answers to the
staleness failure. This plan defers it, and that deferral is now against the grain.

It is deferred anyway, and the reasoning is worth stating rather than assuming: **the exposure is
already bounded by three shipped things.** The cap shows eight notes; ordering is recency-first, so
an old note falls out of the injection as the vault fills; and D38 renders every note's age, so a
reader can discount one without the system deciding. What is missing is *automatic* retirement, and
picking a number for it today would be a guess where D23 supplies a shape that needs no guessing —
a note injected N times and never cited is being declined, not missed. Decide when the ledger has
data.

Claims expire, decisions do not. Session facts are closer to claims —
a fact about a file from four months ago is mostly noise, and staleness is the most-cited failure
mode of memory systems generally. Phase 1 now requires every injected note to render its age, so
the question is no longer whether staleness is *visible*; it is what retires a note automatically.
Two inputs, neither of which needs guessing: **D23's shipped back-off** (offered ten times without
being taken means it is being declined, not missed) and **the citation ledger** (if nothing older
than N days is ever cited, N is the expiry). Decide it when the ledger has data, not before.

**Is lexical-only recall enough for observations?** Inherited from `MEMORY-DESIGN.md` §6 and
unchanged: path anchoring is known to work for decisions on borrowed evidence, and is expected to
miss on observations, where the real query is semantic ("the session where we fought the flaky
fixture"). This is the weakest-evidenced part of the whole design and Phase 1 is where it gets
tested, since the citation ledger records what was *shown* — and a low cite rate with good coverage
is a retrieval problem, not a memory problem.

~~**Dedup mechanism for derivations.**~~ **Answered in Phase 2:** dedup becomes an affordance —
show the candidates already concerning these paths at `observe` time and let the caller echo an id
— rather than an algorithm that has to guess. A miss produces a visible duplicate rather than a
silent wrong merge.

---

## Where the outside claims come from

This repo's convention is that a claim is marked with how it was obtained. Everything above that
did not come from this codebase came from here, checked **2026-08-27**:

- **claude-mem's live corpus** — the numbers in *What claude-mem's live corpus shows* were
  measured on this machine on **2026-08-27** against `~/.claude-mem/claude-mem.db` and plugin
  version 13.16.1: aggregate SQL, `du`, and `grep` over the shipped source. Schema and counts
  only; no note contents were read. **Every figure was re-run and reproduced before being written
  here.** One earlier reading was retracted in that pass — a gap in capture after 2026-08-07 was
  first taken for a silent outage and is simply the plugin being disabled
  (`"claude-mem@thedotmack": false`, no hooks registered). It is a single install belonging to one
  user, so treat it as a worked example rather than a population.
- **Hook surface** — event list, `Stop` inputs, exit-2 blocking, `last_assistant_message`,
  `PreCompact` / `PostCompact` / `SessionStart` matchers / `SessionEnd` semantics, and per-entry
  timeout defaults: <https://code.claude.com/docs/en/hooks>. Re-read in full on **2026-08-28**,
  quoting the *Decision control* and *exit code 2* tables rather than a summary of them.
  **Re-check at build time** — the surface is ~31 events and moves.

  **Two warnings from doing this twice.** The first pass produced a finding that was simply wrong
  and had already become a decision (D42): a fetch returned the section truncated, and "not
  mentioned" was read as "not supported". And on the second pass two fetches of the *same page*
  contradicted each other about `SessionStart`. Neither reading settled it; a first-hand
  observation did. **Quote the table, and where a document disagrees with something you can run,
  run it.**
- **~6,900 tokens per query for steady-state memory injection**, and staleness as the most-cited
  failure mode: <https://mem0.ai/blog/state-of-ai-agent-memory-2026>. A vendor's own report —
  treat the number as an order of magnitude, not a measurement of this system.
- **Memory sycophancy and self-confirming write/read loops** — *MemSyco-Bench*,
  <https://arxiv.org/pdf/2607.01071>; *Escaping the Self-Confirmation Trap: an
  Execute-Distill-Verify paradigm*, <https://arxiv.org/pdf/2606.24428>; and a survey of mechanisms
  and evaluation, <https://arxiv.org/html/2603.07670v1>. **Read at abstract and summary level
  only**; cited as named risks and named mitigations, never as evidence for a number.

  **One of these changed shipped code rather than a document.** The finding that prompting an agent
  about its memory use *"does not make it reassess memory but instead reinforces memory-shaped
  answers"* applies directly to Phase 1's own primer, which asked for a citation while telling the
  reader the feature's survival depended on it. That is D47, and the receipt was biased by the
  sentence requesting it until it was fixed.
- **"Injected memory is assumed to be honoured, and that assumption is proving unreliable"** — the
  shared premise across current memory systems, and the reason Phase 1's citation ledger measures
  something the field does not yet measure for itself. Survey context:
  <https://www.graphlit.com/blog/survey-of-ai-agent-memory-frameworks>.

**Everything measured on this machine** — D24's 5,200 tokens, the startup figures corrected in
`MEMORY-DESIGN.md` §9.1 — is recorded in `DECISIONS.md` and `MEASUREMENTS.md` and should be
re-measured rather than re-quoted.

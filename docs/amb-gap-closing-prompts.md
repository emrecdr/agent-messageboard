# Prompts to close the remaining devt-memory gaps

> **Status: historical, and wrong in three places.** Kept because it is where D79–D81 came from,
> annotated rather than corrected in place so the corrections are visible as corrections.
>
> **1. Prompt A's premise expired while it was being read.** It says the window "opens on the next
> fresh session in this repository, possibly the one that reads this". It was that session:
> `SessionStart` fires on **compaction**, and the window opened at **2026-08-28 21:43:06** on a
> `/compact`. The recommendation survives and gets cheaper — see **D79**.
>
> **2. Prompt C's axis cannot be called `scope`.** `Note.scope` already exists and means the export
> opt-out (`scope: private`). `AMB-MEMORY-ARCHITECTURAL-DIRECTION.md` §0 found this first and named
> the axis **`address`**, which is also the better name, since `src/address.rs` is the vocabulary
> being inherited. That document also names a fork this sheet misses: `Address` has no topic form
> and no room for one.
>
> **3. Prompt D's "global config" would introduce a config file**, which `src/memory.rs`'s own
> module header refuses by name. Topic definitions ship built in; membership is detected, which
> B3 of the companion plan already says is the same data.
>
> Smaller: `memory.rs` is **5,883** lines, not 4,200. The vault is **21** notes, not 19. The board
> is **236 KB**, not 216. The `0.09 / 0.99` mutation ratio in the hardening section **appears in no
> measurement or document** and should not be quoted; the claim under it holds on its own — four
> unit tests in `messages.rs` against twenty-nine in `hooks.rs`.
>
> Everything else in this sheet validated exactly.

Four prompts, paste in order, each after the previous settles. Prompt A first and on its own,
because it decides whether the rest can happen at all.

At the end: what stays closed, and why the list is shorter than it looks.

---

# PROMPT A — the window versus development, decide before building

**You cannot develop the injection path and measure it at the same time, and right now we are
trying to do both.**

D59's window opens at the first post-install `SessionStart` event, which means it opens on the next
fresh session in this repository, possibly the one that reads this. Everything queued behind it is
substantial injection-path work: a scope axis, topic matching, a changed promotion router. If the
window opens first, the fortnight measures a configuration that changes underneath it, and per D77's
protocol most of that work invalidates the measurement anyway.

Three options, and I want your read rather than my assumption:

1. **Develop now, restart the window deliberately when the work settles.** The measurement is worth
   more when it measures something stable. Cost: the fortnight starts later, and D77's start bound
   moves.
2. **Freeze injection-path work for two weeks and measure first.** Cost: development stops on the
   layer's most important remaining gap, and the thing being measured is a layer we already believe
   is incomplete.
3. **Something the code suggests that I have not seen.**

**My recommendation is 1**, for one reason: a fortnight measuring a layer without topic scope tells
us how a layer without topic scope performs, which is not a question anyone is asking. Measuring
should follow the shape settling, not precede it.

**If 1, then say so explicitly in D77** rather than letting the bound drift. The new start is the
first post-install event *after the development lands*, and the reason is recorded rather than
inferred.

Nothing else in this batch should start until this is settled.

---

# PROMPT B — split memory.rs, alone

**Precondition: the peer has answered or is provably gone. If neither, stop and say so.**

This moves ahead of the scope work rather than after it, for a reason that is not aesthetic: the
scope refactor touches this file heavily, and doing a wide semantic change inside a 4,200-line file
is how the test-module drift in the vault's own note happens again. Split first, refactor into the
seams.

**The seams the doc comments already draw:** redaction, frontmatter parsing, the index, injection
rendering, the citation ledger, the promotion pipeline, export.

**Mechanical and behaviour-preserving.** Same technique you used for B2: capture a golden injection
through the real binary first, prove byte-identical output after with age normalised, and land it
alone in its own commit.

**Cite the vault's own test-module-drift note as the justification.** A refactor with a real
recorded failure behind it is rare, and that note is the receipt.

---

# PROMPT C — separate kind from scope

**The largest remaining structural gap, and the no-legacy rule just made it cheap.**

Today `kind` does two jobs: it names the semantic type and it encodes the scope. `pattern` means
global, `decision` means project-scoped, and the type is implied. That survives two scopes and
breaks on three, which is why topic scope has nowhere to land.

**What changes.** `kind` becomes semantic only: observation, candidate, decision. `scope` becomes
its own axis: `@@` global, `@project`, `#topic`. A pattern is simply a decision at global scope.

**Why now rather than earlier.** I deferred this because it touches D50's id scheme and D51's
`INJECTABLE` guard, which made it a migration. **There is no backward compatibility to preserve, no
users, and no legacy to carry.** The vault is 19 notes and regenerable from markdown. Rewrite the
shape cleanly and leave nothing behind. If existing note files need rewriting, rewrite them.

**Extend D50's and D51's tests to the new axis before changing anything.** Both were written to
catch exactly this class of mistake, and they are the safety net that makes this safe rather than
bold.

**Reuse the addressing grammar rather than inventing one.** `address.rs` says its four forms exist
so there is one addressing idea to learn rather than two. Memory should inherit that vocabulary, not
parallel it. Someone who understands `@@` already understands a global note.

**Record scope on the injection and citation events in the same commit.** Without it, "do
topic-scoped notes get cited more than global ones" is unanswerable, and the axis cannot be
evaluated later. This is the D74 lesson applied before the fact rather than after.

---

# PROMPT D — topics, and the promotion router

Only after C lands and is stable.

**Topic definitions are machine-wide; membership is per-project.** A topic is a named set of path
globs in global config, because Python is Python everywhere. Which topics apply to a project is a
per-project statement.

**Detection reuses the definitions, so there is no second table.** The globs that define a topic are
the same data that detects it: does this repository contain files matching them. `Cargo.toml` gives
rust, `pyproject.toml` gives python.

**Topic injection rides the existing path lane.** No new retrieval mechanism. `PreToolUse` already
matches paths, and the receipt already separates the lanes, so topic recall is measured for free by
machinery that exists. **Note the D74 caveat applies here too:** the path lane only fires on four
tool names, so a topic's exposure is not comparable to session-start exposure.

**State the limit in code.** Topics that are not path-shaped, such as security or api-design, cannot
be auto-detected. They are reachable by explicit recall or live at project or global scope. Write
the limit down rather than covering it with a heuristic.

**The promotion router gains a middle rung**, which is what C's refactor actually buys:

```
derived in 1 project                  -> @project
derived in 3 projects sharing a topic -> #topic
derived in 3 projects sharing nothing -> @@
```

Today the router is binary and over-generalises: rediscovering something in three Rust repositories
is evidence for a Rust principle, not a universal one. This makes the ledger's arithmetic more
honest, which is what D49 rests on.

**Gate: the router's middle rung cannot fire with one project.** Build it, test it with fixtures,
and expect it to be dormant until a second arm exists. Say so rather than treating dormancy as a
defect.

---

# What stays closed, and why

So the remaining gap list is honest rather than aspirational.

**Cannot close without becoming a different product.** `enforce:` compilation into blocking findings,
and block-mode edit denial. Both reverse D52, and D52 was a decision rather than an omission. If
that decision is ever revisited, it should be revisited deliberately with its own argument, not
arrived at by feature accumulation.

**Declined on cost.** AST symbol anchoring, and everything that depends on it (stale-symbol
detection, blast radius, god-node de-ranking). That is tree-sitter across N languages inside a
project whose pitch is one static binary. The honest reason is cost, not absence of value.

**Declined on evidence.** A confidence taxonomy was inert inside devt itself. Path-resolution
validation was answered by one command rather than an instrument.

**Delegated.** Discovery and curation belong to claude-mem.

**Gated on the pipeline running at all.** Keyword-based decline suppression, the REJ tombstone
equivalent. Zero candidates have ever been derived, so there is nothing to suppress. The trigger is
the first candidate a person judges too close to one already declined.

**After the window.** The `TeammateIdle` hook from A1. It is a genuinely new opening and agent teams
are live on this machine, but installing a hook changes the configuration the fortnight measures.

---

# Two hardening items, whenever there is room

**Mutation-test `messages.rs`.** Ratio 0.09 against `hooks.rs` at 0.99, it holds `select()` which
D17 calls the project's central design claim, and its coverage is entirely indirect through
process-level suites. If one module gets mutation-tested, that is the one.

**Retention.** No prune, vacuum or TTL anywhere. Not a problem at 216 KB and not worth building for
now. Worth recording that `note_events` grows per-injection-per-session, so it is the table that
moves first once the window opens and a second arm exists.

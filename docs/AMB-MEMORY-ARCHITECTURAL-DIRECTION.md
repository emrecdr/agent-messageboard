# `amb` memory — architectural direction

Where the memory layer is going, and why. Four architectural moves, the invariants they must not
disturb, and how the layer participates in a session.

> **Status, 2026-08-28: three of the four moves are built.** `DECISIONS.md` is the specification;
> this file is the argument behind it. Companion to `AMB-MEMORY-IMPLEMENTATION-PLAN.md` (phases 1
> to 4, built) and `MEMORY-DESIGN.md` (storage, retrieval, trust).
>
> | Move | Built as |
> |---|---|
> | §2 · memory inherits the addressing model | **D81** |
> | §3 · declared identity and a registry | **not built.** `AMB_PROJECT` covers the declaration; the registry needs its own argument |
> | §4 · `kind` splits into orthogonal axes | **D81** |
> | §5 · force, a second gradient | **D64** |
>
> **§0's first correction was itself corrected, which is the interesting part.** It renamed the
> axis from `scope` to `address` because `Note.scope` was occupied by the export opt-out. D81
> renamed *that* field to `visibility` — which is what it always meant — and took the name `scope`
> back. §0's premise was removed rather than worked around, and its **second** argument survives in
> a stronger form than it asked for: `Scope` does not merely borrow `address.rs`'s vocabulary, it
> lives in that module and shares its parser.
>
> **§0's third correction shipped as written.** `Address` gained no `Topic` variant; instead
> `address::parse` refuses `#topic` **by name**, so the vocabulary is shared and the transport is
> refused, with the error saying which half was reached for.
>
> §4's "the tests grow to cover the new axis before anything moves" was followed literally: D50's
> id round-trip and D51's `INJECTABLE` guard were extended first.
>
> **Validated against the code and against current practice, 2026-08-28.** The argument survives;
> three specific things in it did not, and are corrected in place rather than left for the
> implementer to trip over. See **§0**.

---

## 0 · What was checked, and what changed

Everything below was read against `src/memory.rs`, `src/address.rs` and `src/identity.rs`, and
against the 2026 literature on agent memory. Recorded here so the next reader knows which claims
rest on verification and which are still argument.

**Confirmed by inspection.**

- The identity defect is real. `identity::resolve` falls back to `root.file_name()` — the
  basename — so two repositories named `api` do share a namespace, and a rename does orphan notes.
- The conflation §4 describes is real, and already has *two shadow implementations*:
  `order_and_cap` and `render_lines` each derive an address rank from `kind` + `project` in a local
  closure. The concept is not missing; it is computed twice and stored nowhere.
- §5's mechanism exists. `Receipt::unprompted` counts cites for notes a session was never shown,
  documented as "counted apart so it cannot inflate a ratio it is not evidence for".

**Confirmed by research.** Multi-scope memory — tagging each write with identity scopes composed
at retrieval — is the dominant 2026 pattern, so §2 is aligned rather than inventive. And §5's
anti-echo argument is not merely prudent: the literature names *self-reinforcing error* and
*manufactured corroboration* as the failure, and **corroboration-gating** as the defence. "A
candidate is never injected" is that defence, arrived at independently.

**Three corrections.**

1. **The axis is called `address`, not `scope`.** `Note.scope` already exists and means something
   else entirely — `private` keeps a decision out of `export`. Naming the new axis `scope` would
   land the refactor on an occupied field. `address` is also the better name on this document's
   own terms, since `src/address.rs` is exactly the vocabulary §2 argues for inheriting.
2. **There are four axes, not three.** Publication visibility — the existing `scope` — is already
   independent of address, force and lifecycle. This document's own thesis, that a field answering
   two questions cannot answer a third, applies to its axis count.
3. **§2's inheritance has a fork it does not name.** `Address` is `Agent` / `Broadcast` /
   `Everyone`. There is no topic form and no room for one, so either the shared type grows a
   variant the bus cannot route, or memory forks its own — and the fork contradicts the "one
   addressing idea to learn" argument that motivates the move. **Resolution, consistent with §6's
   "the addressing vocabulary is shared; the transport is not":** extend `Address` with `Topic` and
   have `messages::resolve_recipient` reject it with a typed error. Shared vocabulary, refused
   transport, enforced by the type system rather than by convention.

**One thing this document does not know about its own Phase A.** `AMB_PROJECT` already overrides
the derived basename, and `.claude/settings.json` takes an `env` block and is committed in this
repository by explicit `.gitignore` policy. So *declared, committed, rename-proof* identity is
available today with no new code, and it has been applied. What that does **not** give is
uniqueness enforcement or the registry — which is the half that actually needs the argument in §3,
and it now needs it on its own merits rather than carried by the cheap half.

**Caveat, stated rather than assumed:** `env` is documented to apply to "every session and its
subprocesses", but it reaches a session's shell only from session start, and most `env` values
apply only after the folder is trusted. It could not be verified live in the session that applied
it. Verify in a fresh session with `amb memory status --json`, or simply `echo $AMB_PROJECT`.

---

## 1 · What is actually changing

The memory layer today knows two things about a note: **what stage of life it is in**
(observation, candidate, promoted) and, implicitly, **whether it is about one project or about
everything**. That second fact is not a field. It is smuggled inside the note's kind: a `pattern`
*is* a global decision, a `decision` *is* a project one.

Three additions are wanted: notes that apply to a **topic** rather than a project or the whole
world, notes that differ in **how binding** they are, and project identity that does not depend on
what a directory happens to be called.

None of those fit the current model, and the reason they do not is the same in each case: the model
has one field doing several jobs, and a field that answers two questions cannot answer a third.
So the direction is not "add three features". It is **separate the axes that are currently
conflated, and let each of the three additions land on the axis it belongs to.**

---

## 2 · Move one: memory inherits the bus's addressing model

**What changes.** A note gains an explicit **address**, drawn from the same vocabulary the message
bus already uses: everywhere, a named project, or (new) a named topic. *Called `address` and not
`scope` for the reason in §0: `Note.scope` is taken, and means export visibility.*

**Why.** `amb` already has an addressing system, and its own design note says the forms exist "so
there is one addressing idea to learn rather than two". Inventing a second, differently-shaped
addressing model for memory would violate that on its own terms. Someone who understands that a
broadcast reaches a place rather than a process already understands what a project-scoped note is;
someone who understands "everyone in every project" already understands a global one.

The deeper reason is that **scoping a note and addressing a message are the same problem**. Both
ask: who should see this, and where does it apply. The bus answers it for things that are consumed
once; memory answers it for things that are never consumed. The lifetime differs; the addressing
question does not.

**What this makes possible that is not possible today.** A decision about a language, a framework
or a practice currently has nowhere to live. It is not about one project, and calling it global
over-claims: a Python convention is not a universal one. The topic scope is the missing address,
and once notes carry an address, retrieval becomes a matter of *matching* rather than *guessing*.

**The honest limit.** A project's identity is knowable from where you are standing. A topic's is
not. Topics that correspond to files (a language, a test suite) can be recognised; topics that are
conceptual (security, performance, api design) cannot be recognised automatically at all. That
limit is real, it will not be closed by a cleverer heuristic, and the architecture should state it
rather than hide it behind one.

---

## 3 · Move two: identity becomes declared, and a registry appears

**What changes.** A project says what it is called, in a file it commits, instead of `amb` guessing
from the directory name.

**Why.** Identity that is inferred is identity that can change without anyone intending it.
Renaming a directory is a filesystem operation with no semantic content, but under derived identity
it severs a repository from everything remembered about it. Two repositories that happen to share a
basename share a memory namespace with no warning. A clone into a different folder name becomes,
silently, a different project. **A thing that matters this much should be stated rather than
guessed.**

**The architectural consequence is larger than the file.** Declared identity creates the
possibility of two projects claiming the same name, and conflict needs an arbiter. That arbiter
cannot live in either repository, because the declarations come from repositories you did not
author and may not be able to change. So `amb` gains something it does not have today: **a registry
of the projects this machine knows about**, which is the first machine-global claim `amb` makes
about the world beyond its own store.

This follows the pattern the bus already set. When the question is "who orders these unrelated
processes", the answer has consistently been "the one thing they all hold", which is the board.
Uniqueness is that question again.

**The design constraint that falls out of it.** Because the enforcing authority is not the
declaring authority, conflict must be resolvable *locally* and must never require editing someone
else's repository. And it must never be resolved silently, because a quietly invented name is
exactly the kind of unreported divergence this project treats as its worst failure shape.

**A threat model changes here, and it is worth naming.** A project name used to be a directory
basename: filesystem-constrained, chosen by you. It becomes an arbitrary string in a file that
arrives with a repository you cloned. The guard that keeps names inside the vault already exists,
but it was written against much friendlier input, and that shift should be recorded rather than
assumed.

---

## 4 · Move three: the kind field splits into orthogonal axes

**What changes.** What a note *is* (semantic type), *where it applies* (**address**), and *how far
it has got* (lifecycle) become independent facts rather than one overloaded one. With publication
visibility — the field currently called `scope` — that is **four** axes, not three (§0).

**Why.** The current arrangement worked because there were exactly two scopes. `pattern` meant
global, `decision` meant project-scoped, and the type was implied. Add a third scope and the
encoding has nowhere to put it. This is the ordinary failure of a conflated field: it survives
until the third value, then forces a choice between an ugly special case and a refactor.

**Why not the cheaper alternative.** Topics could be bolted on as a filter, leaving notes still
either global or project-scoped and merely tagged. That is less work and it is wrong in a way that
matters later: a Python principle would be stored as a *global* note that happens to mention Python,
which over-claims its reach, and it leaves the promotion machinery with no topic destination to
promote *into*. The cheap version preserves the conflation while adding a fourth thing beside it.

**What separating the axes buys immediately.** Promotion currently routes on a binary: derived in
one project means project scope, derived in more means global. That over-generalises, because
rediscovering something in three Python repositories is evidence for a Python principle, not a
universal one. With scope as a real axis, the evidence can select the address: a shared topic among
the deriving projects routes to that topic, and only genuinely unrelated projects route to global.
**The arithmetic gets more honest, and that arithmetic is what the promotion argument rests on.**

**The risk is concentrated and known.** This touches the two mechanisms most recently hardened: the
identity scheme and the guard that keeps candidates out of injection. That is a reason for care, not
avoidance, because both already carry tests written to catch exactly this class of mistake. The
sequencing rule is simply that those tests grow to cover the new axis before anything moves.

---

## 5 · Move four: a second gradient, earned differently

**What changes.** A note gains a force: advice, decision, or rule. This is separate from lifecycle.
A new note can be a rule; an old, well-established one can remain advice.

**Why it is a separate axis.** Lifecycle answers "has this earned its place". Force answers "how
much weight should it carry". Those come apart constantly. Something rediscovered three times may
still be a suggestion; something recognised once may be non-negotiable. Folding force into lifecycle
would mean the only way to make a note binding is to make it old, which is not how conventions
work.

**The reasoning that matters most in this whole document.** Force upgrades cannot be earned the way
existence is earned, and the difference is structural rather than a matter of tuning.

Existence promotion is safe by construction. Candidates are never shown, so when someone arrives at
the same conclusion again, they arrived independently. The count measures genuine rediscovery.

A note being considered for upgrade is already promoted, and therefore already being injected. Every
subsequent "we applied it again" happens *after* the system put it in front of someone. Counting
those would measure the system's own echo: a note shown often would become binding *because* it was
shown often. That is compliance, not correctness.

Stated plainly: **repeated citation is evidence of relevance, which justifies prominence. It is not
evidence of correctness, which is what bindingness requires.** The two need different evidence, and
the layer already distinguishes them, because citations of notes that were never shown are already
counted apart precisely so they cannot inflate a ratio they are not evidence for.

**Force must have consequences or it is decoration.** This project has twice shipped a field that
recorded something true which nothing consulted, and both times the fix was to delete the field
rather than build a consumer. So each level ships with its effect or does not ship: what gets
rendered first when the budget is tight, what is expected to be cited, what is eligible to be
published into a repository.

**And force stops short of coercion, deliberately.** A rule is *expected* and a miss is *reported*.
It is never *denied*. That line was drawn when the blocking mechanism was refused, and it holds for
a reason beyond caution: the moment `amb` starts blocking, it becomes a governance tool competing
with a governance tool that already exists and is far better at it. Staying advisory is what keeps
the two composable instead of overlapping.

---

## 6 · What must not move

The additions above are shaped by the constraints they are not permitted to break. Listing them is
part of the architecture, not a footnote.

- **The vault is truth; the index is disposable.** Deleting the database loses nothing. Every
  addition, including the new registry, has to respect the direction of that dependency.
- **`amb` never authors into a repository on its own initiative.** A per-project file is written
  only on explicit command, exactly as an export is. The notification suggests; the user acts.
- **Candidates are never injected.** This is what makes rediscovery evidence rather than echo, and
  everything in the promotion argument rests on it.
- **Decisions never travel as messages.** The addressing vocabulary is shared; the transport is
  not.
- **No daemon, no model on the write path.** Detection, matching and routing stay deterministic.
- **A person approves every promotion**, one at a time, with declining cheaper than assenting.
- **Advisory, never coercive.**

---

## 7 · How the layer participates

Unchanged in shape, extended in precision.

**It reads at two moments.** At session start it offers what is known about where you are. Before an
edit it offers what is known about the file you are touching. Scope makes both narrower and more
accurate: today the first is "this project plus everything global", and it becomes "this project,
its topics, and what is genuinely universal".

**It writes at three.** When a session records what it learned. When something is noticed again and
the ledger increments. When a person approves a promotion.

**It refuses three things.** It does not block work. It does not write into repositories unasked.
It does not decide, on its own, that something has become binding.

**The one thing that must be built alongside the fields rather than after them** is the recording of
scope and force on the events that inject and cite notes. Every claim this direction makes is a
ratio sliced by one of those two, and neither can be reconstructed later. A layer that adds axes
without instrumenting them has added ceremony it cannot evaluate.

---

## 8 · What this is worth, and how we would know

Each move has a way of being wrong that should be visible rather than argued about later.

- **Scope** is worth it if topic-scoped notes are cited at a better rate than global ones. If they
  are not, the topic axis is ceremony and should be withdrawn rather than tuned.
- **Identity** is worth it if the file is accepted when offered, and if a collision is ever actually
  detected. If nobody accepts it, membership needs a different source.
- **Force** is worth it if rules are cited more than advice. If not, the levels are cosmetic.
- **The upgrade path** is worth it if any note ever reaches the bar on unprompted use. If rules
  accumulate while unprompted citations do not follow, the ladder is manufacturing bindingness and
  the mechanism is withdrawn, not adjusted.

That last one extends an existing commitment: the promotion pipeline already has a stated condition
under which it is abandoned rather than repaired, and the force ladder inherits it.

---

## 9 · Sequence, and why in that order

**Identity first**, because it is a correctness fix that stands alone. It closes a real defect
today, and it supplies the membership declaration the topic work depends on.

**Then the axis separation**, because every later field lands on it. Doing it after force would mean
migrating more.

**Then force**, which is additive and independently useful, and can be pulled forward if the
refactor stalls.

**Then the routing changes** that need both, since the topic destination requires scope and the
upgrade path requires force.

---

## 10 · Open architectural questions

**~~Can a note hold more than one address?~~** **Answered from practice, 2026-08-28.** Multi-scope
memory — each write tagged with *one or more* scopes, composed at retrieval — is the standard
shape, so matching any of several addresses is both the obvious answer and the industry's. What
remains genuinely open is narrower: whether *this* vault, at its size, ever holds a note that needs
two. That is a question for the ledger, not for the architecture.

**Which topics in real use are not file-shaped?** This decides how much the detection limit in §2
actually costs. It is worth collecting from actual work before building around an assumption.

**Is topic membership a property of the repository or of the person?** It is proposed as a committed
file, which makes it the repository's. A personal view of which topics matter may not be shared by
everyone who clones it.

**Does the registry ever need to be authoritative beyond one machine?** Today it is deliberately
local. Nothing in this direction requires otherwise, but it is the boundary that would be tested
first if the vault were ever shared between people rather than machines.

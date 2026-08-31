# `amb` memory — scope, force and project identity

Implementation plan for three additions: a declared project identity with enforced uniqueness,
a topic axis alongside global and project scope, and force levels for notes.

> **Status, 2026-08-28: Phases B, C and D are built. Phase A is not, and is now unlikely to be.**
> `DECISIONS.md` is the specification; this file is not.
>
> | Phase | Shipped as | Differed how |
> |---|---|---|
> | **A** · declared identity + registry | **not built** | Its cheap half — a declared id — was already available through `AMB_PROJECT` in `.claude/settings.json` and has been applied. What is left is uniqueness enforcement and the registry, which need their own argument now that the easy half no longer carries them. **A1's `.amb` file is refused outright** — see below. |
> | **B** · scope as an axis | **D81** | Named `scope` as planned, but for a reason this plan did not have: the export opt-out that held the name became `visibility`. `Scope` lives in `address.rs`, not in `memory`. |
> | **C** · force levels | **D64** | As planned, ahead of B. |
> | **D** · promotion router | **D82** | Three rungs as specified. D2's force-upgrade ladder is **not** built. |
>
> **Two things this plan specified were rejected on their merits, and the reasons are worth reading
> before anyone re-proposes them:**
>
> 1. **A1's `.amb` TOML file.** `src/memory.rs` refuses a configuration file by name, and nothing
>    in this repository parses TOML — so it costs a dependency in a project whose pitch is one
>    static binary. **B3's own sentence removes the need for it:** detection *is* the definition,
>    so topic membership is derivable and the declaration was only ever an override. Topics ship
>    built in (D82).
> 2. **B3's path globs for detection.** Detection runs on `PreToolUse`, under D9's budget. Root
>    marker files are a dozen `stat` calls; `**/*.rs` across a large repository is not, and it
>    answers a worse question anyway — one vendored `.rs` file does not make a Rust project.
>
> **The one fork this plan called out was decided as it recommended:** `kind` and scope became
> orthogonal axes rather than topics being bolted on as a filter.

---

## What this adds

Three axes, none derivable from the others:

| Axis | Values | Answers |
|---|---|---|
| **Scope** | `@@` global · `@project` · `#topic` | where does it apply |
| **Force** | advice · decision · rule | how binding is it |
| **Lifecycle** | observation → candidate → promoted | has it earned its place |

Two of the three scopes already exist, hidden inside `kind`: `pattern` is a decision at global
scope, `decision/<project>` is one at project scope. The topic scope is genuinely new, and so is
force.

---

## Phase A · Declared project identity

**The reason is correctness, not topics.** D20 derives a project id from the repository directory's
basename. Two repos named `api` on one machine share a vault namespace and collide silently;
renaming a directory orphans every note filed under the old name; a clone into a different
directory name is a different project. A declared id fixes all three.

### A1 · The file

`.amb` at the repository root, TOML, minimal. This is amb's **first configuration file** (today
everything is env vars: `AMB_VAULT`, `AMB_MEMORY_THRESHOLD`, `AMB_MEMORY_PROMOTION`), which is a
surface addition worth its own decision rather than arriving as a detail of this feature.

```toml
id = "nestwatch"
topics = ["rust", "cli"]
```

**D11 carve-out, stated deliberately.** amb never creates this file on its own initiative. Only
`amb project init` writes it, on explicit invocation, exactly as `amb memory export` does. Record
the carve-out in the decision so it is not a second unexamined exception.

**`init` defaults `id` to the current basename**, so generating the file moves nothing. Changing
the id afterwards orphans notes, so that path warns loudly or refuses without an explicit
migration flag.

**The file stays optional.** Absent: id equals basename (today's behaviour), topics equal whatever
detection found. A tool whose value is that it just works must not acquire a setup step.

### A2 · Uniqueness enforcement

**The declaring authority is not the enforcing authority.** The id is committed and travels with
the repo; uniqueness must hold on *your* machine across repos you did not author. amb therefore
cannot reject at declaration time, and must never require editing the other repository.

- A `projects` table in `board.db` (the only machine-global thing amb has) maps repo root to id,
  with `id` unique. First root to claim an id holds it.
- A second root claiming the same id is a **detected collision**, resolved by a **local alias**
  stored in the board: `amb project alias nestwatch-fork`. The committed `.amb` is untouched.
- **Never auto-suffix.** `api-2` invented silently is this project's documented worst failure
  shape. The collision is reported and waits.

**The case that looks like a collision and is not.** Two worktrees, or a second clone, declare the
same id at different roots and *should* share notes. Discriminate on the git remote read from
`.git/config` (no subprocess): same remote means same project, register the extra root; different
remote or no remote means a genuine collision. Getting this wrong in the permissive direction
silently mixes two projects' memory, which is a trust failure rather than an annoyance.

### A3 · Validation, and a changed threat model

- **Reserved ids rejected at `init`:** `candidate`, `pattern`, `decision`. D50 noted these collide
  with the id scheme and only degrade gracefully; now that ids are declared rather than derived,
  they become a clean rejection.
- **Shape-constrained:** lowercase, hyphenated, bounded. `slugify` already does this.
- **Security, and this is the part that changed.** Until now a project id was a directory basename:
  filesystem-constrained and chosen by you. It is now an arbitrary string in a committed file, so
  **cloning an untrusted repository makes it attacker-controlled input flowing into a filesystem
  path.** A hostile `.amb` declaring `id = "../../etc"` is the classic traversal vector.
  `safe_component` already blocks it and `a_project_name_can_never_walk_out_of_the_vault` already
  tests it, so the machinery is right, but the test must now cover **declared** ids, and the
  decision record should state that the guard was written against a much friendlier input.
- **Not on the hot path.** Register at first sight of a root; re-validate only when `.amb`'s
  content hash changes, reusing the change-detection machinery notes already have.

### A4 · The notice

Reuse `fail_loud_notice`'s discipline, which already exists and is already tested for waiting
rather than firing immediately: count sessions without a file, emit one actionable line after a
run, stop.

Two additions. The notice carries the **suggested content** (detected id and topics) so the user
accepts a proposal rather than starting a task. And `amb project init --skip` records a decline
that **sticks permanently**, mirroring the promote/decline symmetry already built. Unbounded
nagging is a per-session token cost, which is the axis north star three governs.

**Receipt for A:** acceptance versus decline rate on the notice, and whether any collision is ever
detected. If nobody accepts, the file is unwanted and Phase B needs a different membership source.

---

## Phase B · Scope as an axis

### B1 · The refactor, and why not the cheap version

Today `kind` does two jobs: it names the semantic type *and* encodes the scope. That conflation
survives two scopes and breaks on three.

- **Cheap version:** add `topics: [...]` alongside the existing kinds. Topics become a *filter* on
  a note that is still either global or project-scoped.
- **Clean version:** `kind` becomes semantic only (observation, candidate, decision) and `scope`
  carries `@@` / `@project` / `#topic`. `pattern` becomes "a decision at global scope".

**Take the clean version**, for one concrete reason: under the cheap version a Python principle is
a *global* note tagged python, which is semantically wrong and, more practically, leaves Phase D's
promotion router with no topic destination to route to. The cheap version also compounds the
conflation it declines to fix.

**The migration touches the two things most recently hardened:** D50's id scheme and D51's
`INJECTABLE` guard. That is a feature, because both carry tests written to catch exactly this
class. `every_kind_of_id_round_trips_including_the_ones_with_no_project` and
`candidates_are_not_in_the_injectable_set` are the safety net; extend both to the new axis before
changing anything.

### B2 · Grammar

```
@@          global
@nestwatch  project
#python     topic
```

`#` for topic is not only symmetry with `Address`. It is the Obsidian tag character, so `#python`
in a note body is simultaneously the scope and a working tag in the reading surface, and it removes
the ambiguity between a topic named `python` and a project named `python` that a flat namespace
would create.

### B3 · Topic definitions and membership

Two levels, deliberately:

- **Definitions are machine-wide.** A topic is a named set of path globs in global config. Python
  is Python everywhere.
- **Membership is per-project**, declared in `.amb`. The file says which topics apply; it never
  redefines them.

```toml
[memory.topics]
python  = ["**/*.py", "pyproject.toml"]
rust    = ["**/*.rs", "Cargo.toml"]
testing = ["tests/**", "**/*_test.*"]
```

**Detection reuses the definitions, so there is no second table.** The globs that define a topic
are the same data that detects it: detection is "does this repo contain files matching these
globs". That is what powers A4's suggestion, and it falls out of the model rather than sitting
beside it.

**Topic injection rides the existing path lane.** No new retrieval mechanism: `PreToolUse` already
matches paths, and the receipt already separates `injected_file` from `injected`, so topic recall
is measured for free by machinery that exists.

**The honest limit, stated in code.** Topics that are not path-shaped (security, performance,
api-design) cannot be auto-detected this way. They are reachable by explicit recall
(`amb memory recall --topic security`) or live at project or global scope. Write the limit down
rather than covering it with a heuristic.

**Receipt for B:** citation rate of topic-scoped notes against global ones. If topic scoping does
not improve precision, it is ceremony and should be withdrawn rather than tuned.

---

## Phase C · Force levels

Additive, no migration, and independently shippable. Can be pulled ahead of B if B stalls.

### C1 · Force must do something

**The trap this project has hit twice:** devt shipped a `keywords:` field nothing consumed and had
to trim it; D51 found `INJECTABLE` decorative while the behaviour was correct by accident. If
`force: rule` only changes an adjective in the rendered block, it is another inert field. Decide
the mechanical consequence before adding the field.

Given D52 refused blocking, force honestly does four things:

1. **Injection priority under budget.** Rules render first; advice is dropped first when the cap
   bites.
2. **Citation expectation.** A rule covering the file being edited is *expected* to be cited. A
   miss is recorded and surfaced in `amb memory status`. Reported, never denied, which is the same
   line shared roots sit on elsewhere and keeps D52 intact.
3. **Export eligibility.** Rules and decisions export to the repo; advice stays personal.
4. **A higher evidence bar**, which is Phase D.

**Receipt for C:** do rules get cited at a materially higher rate than advice? If not, force is not
changing behaviour and the levels are cosmetic.

---

## Phase D · Promotion router and force upgrade

### D1 · The topic rung

Today the destination router is binary and over-generalises: one project means project scope, two
or more means global. Deriving something in three Python repos is evidence for a Python principle,
not a universal one.

```
derived in 1 project                       → @project
derived in 3 projects sharing a topic      → #topic
derived in 3 projects sharing nothing      → @@
```

This is what Phase B's clean refactor buys, and it makes the ledger's arithmetic more honest, which
is what D49 rests on.

### D2 · Force upgrade needs different evidence

**The counting rule cannot be reused, and this is the important part.**

Existence promotion is safe by construction: candidates are never injected, so every derivation is
independent, and `independent()` enforces it.

Force upgrade operates on a note that is *already promoted*, therefore already being injected.
Every "we applied it again" happens after amb showed it. Counting those measures the echo the whole
design was built to avoid: a frequently shown note would become a rule by being frequently shown,
which is compliance bias rather than evidence of correctness.

**Repeated citation is evidence of relevance, which justifies prominence. It is not evidence of
correctness, which is what bindingness requires.**

The signal already exists. `Receipt.unprompted` counts citations of notes the session was never
shown, and it is already segregated "so it cannot inflate a ratio it is not evidence for". Pair it
with the cited-over-injected *rate*, since a note injected 100 times and cited 5 is weaker than one
injected 5 and cited 5.

```
advice   → decision : 3+ independent derivations        (unchanged)
decision → rule     : N unprompted citations + healthy rate
```

Same offer-and-approve UX, same one-per-offer discipline, same recorded decline.

### D3 · Extend the withdrawal condition

D49 withdraws the pipeline if approval degrades to a rubber stamp, or if promotions accumulate
while citations do not follow. Extend it: **rules accumulating while unprompted citations do not
follow** means the force ladder is manufacturing bindingness, and it is withdrawn rather than
tuned. `decline_rate` already exists as the mechanism; add the rule-tier numerator beside it.

---

## Instrumentation, without which none of the receipts exist

**Record scope and force on `note_events`** at injection and citation time. Every receipt above is
a ratio sliced by one of those two fields, and neither can be recovered afterwards. This is the one
implementation detail that must land in the same commit as the fields themselves.

---

## Decisions this needs

- **`.amb` is amb's first config file**, and the D11 carve-out that permits it (explicit command
  only, never on amb's initiative).
- **Project identity is declared, not derived.** Local registry enforces uniqueness; collisions
  resolve by local alias; same-remote is not a collision; auto-suffixing is refused.
- **Declared ids are untrusted input.** The threat model changed when the id stopped being a
  basename; `safe_component` coverage extends to declared ids.
- **Scope and kind are orthogonal.** `pattern` becomes a decision at global scope.
- **Force levels, and what each mechanically does.** A level with no consequence is not shipped.
- **Force upgrade uses unprompted citations, not derivation count**, because the note is already
  being injected.

---

## Risks

| Risk | Mitigation |
|---|---|
| Scope refactor breaks the id scheme or the anti-circularity guard | Extend D50 and D51's tests to the new axis **before** touching either |
| Silent project mixing on a false same-remote match | Remote comparison is exact; no remote means collision, not merge |
| Traversal via hostile `.amb` from a cloned repo | `safe_component` on declared ids, with the existing containment test extended |
| Force levels become decoration | Ship none of them without its mechanical consequence (C1) |
| Notice becomes nagging | Bounded run, and `--skip` sticks permanently |
| Topic ceremony with no precision gain | Phase B's receipt; withdraw rather than tune |

---

## Open questions

**Can a note carry several scopes?** A rule that applies to both `#python` and `@nestwatch`. A list
with OR matching is the obvious answer; whether it earns its complexity is unproven.

**Do topics nest?** `#python/testing`. Probably not worth it; flat until something demands
otherwise.

**Which topics are not path-shaped in your actual work?** That answer decides how much the
detection limit in B3 costs, and it is worth collecting before building rather than after.

**Does `.amb` belong in `.gitignore` for some projects?** A personal topic membership in a shared
repo may not be everyone's. Unresolved.

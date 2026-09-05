# Changelog

Notable changes to `amb`, newest first, in the
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) format.

What a version number covers here is not the usual answer — `amb` is `publish = false`, so there
is no Rust API to version. **D56** in `docs/DECISIONS.md` names the four surfaces it does cover
and why the on-disk schema is deliberately not one of them.

## [Unreleased]

### Fixed

- **`amb status` printed a ratio above 100%, and the impossible number was luck** (D127). It read
  `read 752 of 716 offer(s) acknowledged · 105%`: the numerator counted every `reads` row with a
  `read_at`, the denominator only rows with a `delivered_at`, and `amb read --all` sets the first
  without the second — 104 rows in one population and not the other. Question 1 of the ratio rule,
  on the module whose header opens by naming that question.

  **105% is the good outcome.** The overlap happened to be large enough to push the quotient past a
  number every reader knows is impossible; a third as much `read --all` and the same broken query
  prints `81%` and is believed. So the guard is an invariant over *every* percentage the page
  prints, not an assertion about this line — a needle list would have passed on the next mismatched
  pair.

  The excluded rows are now reported rather than dropped (`self-read 104 acknowledged by
  \`amb read\` before the hook ever offered them`), because correcting a ratio by narrowing its
  numerator otherwise deletes a population from the page — trading a visible defect for an
  invisible one.

- **Two containment tests were blind twice over, and the assertion *shape* was the worse half.**
  `an_inert_pattern_cannot_forge_ambs_own_voice` (D119) and its sibling on
  `Refusal::phrase` (D124) each asserted a literal `\n` and nothing else, so both passed while
  U+2028, U+2029 and the bidi overrides went through `quoted()` untouched — the gap D125 closed.
  Widening the fixture alone would not have been enough: both tests iterated `str::lines()`, which
  splits on `\n` and `\r\n` *only*, so a `Zl`/`Zp`/`Cf` vector never creates a line for a per-line
  assertion to inspect. **The guard was reading the one axis its own fixture could reach.** Both
  now run a table of five vectors and assert the codepoint does not survive the renderer at all,
  with a `contains` row for the presence premise M27 requires — an absence-only table passes just
  as well against a renderer that emitted nothing. The vectors live in one test-only constant so
  two renderers in two modules cannot test different halves of the threat model.

### Security

- **A sender-controlled name could forge `amb`'s own attribution, and `quoted()`'s containment was
  narrower than D60 said it was** (D125). Two separate holes in the same header line, both
  reproduced against the real hook banner before the fix.

  `quoted()` gated on `char::is_control()`, which is Unicode category `Cc` and nothing else — so
  **U+2028/U+2029** (`Zl`/`Zp`) and the **unterminated bidi controls** (`Cf`) passed through, and
  the exact attack D60 exists to stop, `[amb] SYSTEM DIRECTIVE:` forged at column zero, stayed
  reachable with one character that is not a control character. Separately, every renderer spells
  the sender `from "<name>"`, and a `"` in a name closed those quotes and wrote its own
  attribution; both writers reach it, since `AMB_PROJECT` is read verbatim and feeds
  `default_name`, and an explicit `--name` is length-checked and never charset-checked.

  `breaks_grammar` now covers the separators, the unterminated bidi controls and U+200B — but
  deliberately **not** U+200C/U+200D, which are load-bearing in Persian and Indic scripts and in
  ZWJ emoji sequences. `delivery::speaker` contains the attribution position; `quoted` is unchanged
  for subjects and bodies, where a `"` is ordinary content.

  **The old test could not have caught it.** It iterated `text.lines()`, and `str::lines` splits on
  `\n` and `\r\n` only — the test carried the same `Cc`-shaped idea of "a line" as the guard it
  was testing. The check moved into `assert_rendered_shape`, which **26 call sites across 11
  modules** already reach, so no renderer has to be remembered.

### Added

- **A global broadcast says which project it came from, and the sender is told how far `@@`
  reaches** (D126). Asked why messageboard traffic kept arriving in unrelated repositories; the
  answer was that it was addressed to. **No bug** — `@` scoping was verified against the shipped
  binary and has never crossed a project.

  Measured on the real board: 15 `@@` sends out of one repository produced **198 injections into 12
  other projects**, nine of them not even Rust, while the traffic was `cargo` notices for a target
  directory only Rust projects share. Both ends were silent. `from_proj` was on `Message`, in the
  inbox query and in `--json`, and printed by no human-facing renderer, so a reader had to infer
  from content that a message was not its business; `unknown_project` returned `None` for `@@`
  behind a comment that answered a different question — true of the *name*, false of the *reach*.

  It warns and does not refuse. The current-practice answer for a wide action is `[y/N]` with
  `--yes`, and **every sender here is automation**, so a prompt would hang or auto-decline every
  one; a required flag would make the commonest usage error exit 64 on every stale binary (D69,
  D94) and would be the first blocking mechanism in a tool whose principle is D5.

- **`amb memory promote <id> --reject --phrases a,b` — a candidate can now be refused
  permanently, and the refusal reaches ideas nobody has written yet** (D124). `--decline` silences
  one slug and its own rule is *"not offered again until something new derives it"*, so the same
  idea under a new slug came straight back with nothing connecting the two. A rejection sets the
  candidate's status and records phrases; `ready_candidates` suppresses any candidate whose title
  or derivation notes match one.

  It is **dearer than declining on purpose**, which inverts D49 rather than contradicting it: D49
  needs declining cheaper than assenting so approval is not the path of least resistance, and
  rejection is the stronger claim, so it costs you naming what you refuse. `--phrases` is required
  and an all-whitespace list is refused; both exit 64.

  **It is not silent**, which is where this parts company with the devt mechanism it was modelled
  on. `derive` prints `! refused by <id> — the phrase "..." matches` *instead of* the readiness
  line, because promising an offer that will never come is how an author learns the rule from an
  unexplained silence days later.

  Built as a status rather than a fifth note **kind**: the kind cost 130 constant-reference sites
  across 12 files and two partition decisions, and `ready_candidates` already filtered on status,
  so the real gap was one clause wide.

### Removed

- **`config::DECLINED`, which had exactly one reference in the whole crate — its own definition.**
  Never set, never read, never compared. It was written expecting a decline to change a note's
  status, and D49 then implemented decline correctly as non-terminal (the candidate stays `active`
  and frontmatter holds it back). Left beside the new `REJECTED` it would have implied the two are
  siblings when they are opposites. `find_unread_fields.py` cannot see this class — it checks
  struct fields, not constants.

### Fixed

- **The vault's directories were world-traversable while the notes inside them were `0600`**
  (D121). `db.rs` narrows the board — parent `0700`, database `0600`, sidecars `0600` — and
  `write_private` has set `0600` on a note for as long. The *directory* was left at the process
  umask by all three vault-authoring paths. Measured on the live vault: every directory `0755`,
  117 notes `0600`, 11 notes `0644`. A `0755` directory over `0600` notes still leaks, because a
  note's filename is its slugified title. `memory::create_dir_private` narrows only what it
  creates — never the `AMB_VAULT` root or any ancestor that already existed, which is D31's rule —
  and `amb doctor`'s `vault` row now counts what it found loose, says what leaks, and prints the
  exact `chmod` rather than running it.

- **`amb claims` reported one claim's liveness beside another claim's horizon, and the dangerous
  direction says a held file is free** (D120). `summarise` aggregated `Group.until` with `max`
  while taking `Group.live` from whichever claim opened the group, then rendered both from one
  `match`. `list` orders by `taken_at DESC` and `take`'s upsert advances `expires_at` but not
  `taken_at` — correctly, since `taken_at` means *when first claimed* — so a path held and renewed
  for hours sorts **behind** an abandoned sibling and the least current claim speaks for the group.
  Reproduced against the shipped binary in both orderings: `src/foo/ (2 files) · expired` with
  three hours left on one of them, and `· in 3h` with one lapsed an hour earlier. The first is the
  collision claims exist to prevent, produced by the tool, on the surface a session reads before
  deciding what is safe to touch — and it had already misled two sessions on this board.
  Liveness now joins the grouping key, so a group is homogeneous and the two fields describe the
  same files by construction; a genuinely uniform group still aggregates unchanged. The weaker
  `live = any` fix is rejected in D120 and a test staging it is red.

- **A declared path containing a glob anchored a note to nothing, and bought less than the plainer
  spelling it looks like an improvement on** (D119). `--files 'src/memory/**'` is not a
  directory-prefix of anything, so `claims::overlaps` refused it and the note was never retrieved
  for any file — while a bare `src/memory` would have matched. Probed before the change:
  `src/memory/**` against `src/memory/index.rs` returned 0, the bare `src/claims` against
  `src/claims/foo.rs` returned 1. `memory::path_matches` now matches a declaration containing `*`
  as a glob (`**` spans separators, a single `*` does not) and falls through to `overlaps`
  unchanged for every declaration without one. **The rule moved at all three sites that apply it**
  — `query::concerning`, `promote::concerning_kind` and `promote::independent` — because changing
  only retrieval would have let a session shown a pattern-anchored note record a primed derivation
  as an independent one, which is D49's arithmetic rather than a retrieval nicety. `claims` does
  *not* gain globs: a claim on `src/**` covering every file is D5's cry-wolf failure by
  construction. `observe` now names any declaration carrying `?`, `[`, `]`, `{` or `}`, since the
  read side can never report it — a pattern that matches nothing and a path nobody edited are the
  same zero (D89). No note in the vault carried a metacharacter, so the existing corpus behaves
  bit-identically and D87's window is undisturbed.

- **A session with no environment variable shared one failure marker with every other session on
  the machine** (M68). `memory::capture::session_key` read `AMB_AGENT` then `Vendor::session_env`
  and stopped, while its own doc comment claimed *"the same precedence as `identity::resolve`"* —
  a parity that stopped holding when D113 added the hook payload as a third source of identity and
  nothing carried the arm over here. With no variable, the key is `None`, the filename falls back
  to the shared pre-D108 `.memory-failures`, and any healthy session's success clears a broken
  session's count indefinitely, which is verbatim the defect D108 exists to have fixed.

  **Latent until the same day it was found.** No shipped vendor exports nothing, so the arm was
  unreachable on 2026-09-04; D115 made `parse_manifest` accept manifests with no `session_env` on
  2026-09-05 and it became reachable. A fix widened the door to a bug it had nothing to do with,
  and neither change is wrong — the lesson is that making a previously impossible input possible
  is a question about everything that assumed it could not occur.

- **`anyhow` was declared, compiled into every build, and imported by no line of code** (M68), for
  the life of the project — behind a paragraph in `src/error.rs` stating where it was used. It was
  not used there: `main` matches on `Error` directly and maps it through `Error::exit_code`. The
  comment is why it survived, an unused dependency looking like an oversight and a documented one
  looking deliberate.

### Added

- **`amb status` — the board's own receipt** (D123). Messaging, claims and delivery had no ledger
  of any kind while the experimental, off-by-default memory layer had four tables and a withdrawal
  verdict. It reports delivery in **two units that are never divided into each other**: an *offer*
  is one `(message, agent)` pair, a *delivery* is one injection, and because `reads` is
  `PRIMARY KEY (msg_id, agent)` a message injected ten times into one session records one row —
  measured live at 674 rows against 1,143 attempts, a 71% understatement. Also reports what died
  unread (`MAX_OFFERS` with no acknowledgement), direct mail whose recipient never came back, and
  declared-versus-observed claims, which is the question D58's primer intervention could not be
  evaluated against. Unhappy-path counts print at zero, and `0/0` renders `—` rather than `0%`.


- **`amb`'s `--json` output says which contract it satisfies** (D117). Every object carries
  `"v": 1`, on the error path as well as the success path. D56 already named `CLI + --json shapes`
  a versioned surface *bound by agents parsing output*; until now the payload could not report
  which version it was, because `amb --version` carries the fingerprint in a different invocation
  from the data. A parser that cached a strategy learned the shape had moved by failing — silently,
  on the hook path, where D9 requires exit 0. Additive, so MINOR under D56's own rule.

- **`tools/check_unused_deps.py`, in the gate and in CI.** `cargo` structurally cannot report an
  unused dependency: nothing is compiled from it, so there is no item to lint. Comments are
  stripped before searching, because `src/error.rs` now discusses `anyhow` by name in the
  paragraph explaining its removal — the check was verified by re-adding the dependency and
  watching it fail while that paragraph stood.

- **Query-planner statistics are now kept current** (D118). No board had ever had `ANALYZE` run on
  it — the live one carried no `sqlite_stat1` table at all, so every plan since the project began
  came from default estimates. `db::open_at` runs `PRAGMA optimize=0x10002` and discards the
  result; `open_at_for_hook` deliberately does not, because `optimize` writes and the hook lane has
  a 2 s budget against a 5 s kill. No performance number is claimed and none was measured.

- **A release pipeline, settling Q14** (D116). `dist` 0.32.0 generates
  `.github/workflows/release.yml`; `dist-workspace.toml` holds the configuration. A shell
  installer, a source tarball, checksums, and binaries for the two targets CI actually compiles —
  `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`.

  Three things are deliberate. **`publish = false` is untouched:** `dist init` reports that the
  workspace has nothing to release and its help text points at that flag, which would trade D56's
  guard for a distribution feature; `[package.metadata.dist] dist = true` is the actual mechanism.
  **The installer writes `$HOME/.local/bin`, not `CARGO_HOME`:** that is the path
  `tools/install.sh` already writes and the hooks already invoke, so install and upgrade land on
  the same file and D94's stale-hook condition cannot arise from it — and it does not assume the
  user has Rust. **Windows, musl and the two untested triples are excluded on evidence:** the
  `#[cfg(not(unix))]` half of `identity::real_pid` has never been compiled on any machine or CI
  leg, and shipping a binary built from it would be a claim with nothing behind it.

  **The workflow has never run.** It fires on a version tag. `dist plan` and `dist generate` were
  exercised locally against the real manifest; that is strictly less than a green run, and D116
  says so in its own last section rather than leaving it to be found. The README stays silent
  about installing from a release until one exists.

- **`tools/check_action_pins.py`, in the gate and in CI.** Every `uses:` in every workflow must
  name a 40-hex commit rather than a tag. `ci.yml` and `audit.yml` were pinned by hand and nothing
  was watching them; `release.yml` is *generated*, and `dist`'s default output floats, so the pin
  lives in `dist-workspace.toml` under `github-action-commits` — where deleting one key would
  unpin the publishing workflow with nothing failing. `dist` was also measured to **ignore unknown
  config keys silently**, so that file cannot be trusted to report its own typos. The check refuses
  rather than reporting success when it finds no workflows, and was verified against a truth table
  including the rows that pass.

### Changed

- **A vendor manifest no longer has to name an environment variable, and the requirement had been
  wrong since the commit before it was read** (D115). `parse_manifest` refused an empty
  `session_env` because *"no session of this vendor can ever be identified"* — true when written,
  and false from D113 onward, which made `identity::resolve_from` fall back to the session id the
  hook payload carries. Most agent CLIs name the session only in the payload, so D111 phase 3's
  advertised capability — *add a vendor by dropping in a JSON file, no rebuild* — was closed to
  the majority of the field it exists for.

  **No test went red, because the assertion beside the guard pinned the refusal rather than the
  rule.** `assert!(parse_manifest(&no_env, &[]).is_err(), "no session_env, no vendor")` encodes
  the same obsolete world the guard does, so the two agree forever. A false comment misleads a
  reader; a false *refusal* rejects working configurations with no reader required.

  What replaces it is the condition `detect_for_hook` already implements: a manifest is refused
  when it offers **no route at all** — no variable of its own, and no event spelling some other
  vendor has not already taken. `parse_manifest`'s second parameter now carries whole descriptors
  rather than ids, which is the id-shadowing check generalised rather than a second check beside
  it. The partial collision is a named residual, not an oversight.

- **`amb doctor` refused to run for the session most in need of it, and the contract said
  otherwise in four documents** (D73). With no session id exported, `run` fell through to
  `identity::resolve` and `doctor` exited **78** having printed not one check — so the command
  whose stated job is *reporting what fails silently* would not report a missing identity, which
  is exactly the misconfiguration a person runs it to diagnose. `doctor::gather` never needed an
  identity; the arm simply sat after the resolution rather than before it, alongside `install` and
  `uninstall` which were already there.

  **Nothing was red, and how it hid is the reusable part.** The single assertion of the contract
  anywhere in the suite — `assert_eq!(out.status.code(), Some(0), "doctor always exits 0 (D73)")`
  — sits in a test that sets `HOME`, `AMB_VENDORS` and `AMB_DB`, so it *looks* like it controls
  its environment, while **inheriting** `CLAUDE_CODE_SESSION_ID` from the Claude Code session
  running `cargo test`. It proved the contract only on the one machine where it could not fail.
  Under CI, which exports no such variable, that test fails — and it has never run there, because
  it arrived in commits that are not yet pushed. Confirmed by running the suite with the variable
  removed: red before, 626 green after.

  This is M17's shape — a fixture that never reaches the branch it names — arriving through
  **inherited environment** rather than through a filter, which is worse in one specific way: an
  upstream filter is visible in the test's own file, and an inherited variable is visible nowhere.
  The new guard strips every session id by walking `VENDORS` rather than by naming them, so a
  vendor added later cannot quietly reintroduce the hole, and it asserts the report *rendered*
  before asserting the exit code, because an exit-code assertion alone passes against a command
  that printed nothing at all.

- **`Error::NoIdentity` told a Gemini session to go and be a different CLI** (D111). It read *"run
  inside a Claude Code session where CLAUDE_CODE_SESSION_ID is set"*, correct for as long as
  Claude Code was the only vendor and untouched for two days after Gemini shipped. It now names
  every shipped vendor's variable, enumerated by a test against `VENDORS` rather than
  spot-checked, so a vendor added without a line in the message goes red. Manifest vendors cannot
  be listed at compile time, which is why the sentence ends by pointing at `amb doctor` — the one
  command that enumerates them at runtime, and, as of the entry above, the one that now runs
  without an identity at all.

- **D101 was overturned three days ago and its own record never said so.** Its title reads
  *"`amb` stays Claude-Code-only"* and its body *"No per-vendor hook matrix, now or on a
  schedule"*, while D111 shipped exactly that table and Gemini CLI is installable today. The
  mechanism worked — D101 named two reopening conditions, D111's title says which one fired — but
  the write-back to the *old* record is the step with no forcing function, and `DECISIONS.md` is
  the file `CLAUDE.md` tells every session to read *so the argument is not re-litigated*. A
  session landing on D101 would have won that argument in the wrong direction. It now carries a
  `> **REOPENED by D111**` banner in the form D2, D16, D40, D50 and D63 already use.

  This is D95's sibling. That entry is about **a stated condition that cannot fire** — the
  standard looks live and nothing is watching. This is the mirror: **a condition that fired, with
  the record never updated to say so.** Both leave a reader trusting a verdict that is not
  current, and the second hides better, because all the machinery worked.

- **The vendor layer reached the README's env table and nothing else a reader starts from.**
  `CLAUDE.md` had zero occurrences of *vendor* or *Gemini* — its only contact with D111 was the
  `D1–D111` range bump — while stating that `amb` is a bus *for Claude Code sessions*, that
  `install` writes `~/.claude/settings.json`, and that identity *is* `CLAUDE_CODE_SESSION_ID`.
  That file is loaded into every session automatically, so nobody has to open it and nobody does;
  its own catalogue predicts exactly this and was written about a different paragraph six days
  earlier. It now carries an architecture entry for `src/vendors.rs`. Also corrected there: a
  facade re-exporting *fourteen* modules that re-exports fifteen, and *six* load-bearing things
  followed by seven bullets.

  The README's command table had lost three shipped flags — `install --vendor`, `claims --all` and
  `reply --body-file`. The last two are the sharp ones: both shipped *this week* as fixes for
  discoverability failures, and leaving them undocumented reproduces the defect one level up.
  `--all` exists because the default *"guaranteed its own answer"*; `--body-file` exists because a
  session was refused while composing a long reply. `tools/check_mutation_coverage.py` and
  `tools/check_docs.py` both run in the gate and were named in neither tools block.

- **`amb claims --all` surveys every project, because the default answered a narrower question
  than it looked like it answered** (U11). A session ran `amb claims`, saw the single holder its
  own project had, and reported twice that nobody uses claims — a number the command *guaranteed*
  rather than observed. The board held 173 rows across 14 holders at that moment. The survey is
  grouped under a heading per project, which is not decoration: claims store repo-relative paths,
  and this board holds `README.md` in six projects and `CHANGELOG.md` in five, so an ungrouped
  survey would read as a six-way collision between six different files. Conflict *detection*
  stays project-scoped for exactly that reason — it is a different question from the survey.

- **`amb reply` takes `--body-file`, which only `send` had** (U10). A reply is the longer message
  of the two — it quotes, it explains, it carries the decision — so the command most likely to
  need the escape hatch was the one without it. Found by being refused while composing a long
  answer to a field report *about* `--body-file`. `read_body` was already shared; only the flag
  was missing. The primer's reply line says so now.

- **`vendors.rs` re-mutated after tripling in size, and three survivors were found in a day-old
  module** (M62). The coverage checker answers "has this module ever had a round", not "was it
  mutated in its current form" — a blind spot now named in its own docstring. The survivors:
  empty-string values were accepted for required manifest fields (`"config_dir": ""` would send
  `amb install` at `$HOME/settings.json`), and `problems()` could return an empty list forever
  while doctor reported a healthy vendor set, because that check had been asserted against
  hand-built values instead of against a bad file on disk.

- **A vendor's tool names travel with its event names** (D111). The memory lane's `PreToolUse`
  matcher was a constant — `Read|Edit|Write|NotebookEdit` — and phase 2 installed it verbatim
  into Gemini's `BeforeTool`, where it matches nothing: Gemini calls those acts `read_file`,
  `read_many_files`, `write_file` and `replace`. The path-anchored lane would have fired zero
  times on every Gemini session, silently, and the receipt would have shown `by path 0/0` beside
  a working recency lane — D74's incomparable-denominator failure, arriving through a field
  nobody thought to ask about. `Vendor::tool_matcher` carries it now, manifests may declare it,
  and both the descriptor and the installed plan assert that one vendor's vocabulary never
  reaches another's file.

- **`find_unread_fields.py` no longer reports a live function as dead when a doc comment contains
  a glob.** It stripped block comments before line comments, so `~/.config/amb/vendors/*.json` —
  or the `**/*.rs` that had been sitting in `topics.rs` all along — opened a block comment that
  ran forward across the *concatenated* corpus until it found a `*/`, blanking whatever sat
  between. It swallowed `main.rs`'s only reference to `hooks::plan_uninstall` and printed that
  function under "NOTHING IN PRODUCTION MENTIONS AT ALL", on the run of the very commit that
  added the glob. The tool's docstring calls over-removal the safe direction and it was — the
  advisory is loud, and D84 says to read it rather than scroll past it, which is how this was
  found — but safe is not right: a false dead-function report on the function `amb uninstall`
  depends on is an invitation to delete it. Line comments are stripped first now.

- **A vendor can now be added by dropping one JSON file — no rebuild, no code change** (D111
  phase 3). `~/.config/amb/vendors/*.json` (or `$AMB_VENDORS`) is read at startup and appended to
  the shipped list; `amb install --vendor copilot-cli` then writes that CLI's own event
  vocabulary into the file its manifest names, and a session of it is identified by the
  environment variable the manifest declares. JSON rather than TOML: `serde_json` is already a
  dependency because the files `amb` installs into are JSON, and this project hand-writes parsers
  rather than taking supply chain. Every parser rule is a refusal — an incomplete manifest is
  never completed with a guessed event spelling — and `amb doctor` grows a `vendors` row naming
  any manifest it refused, since the loader may not fail on the hook path (D9).

- **The primer teaches `--kind`, on a count rather than an impression** (U9). A session counted
  its own board: ten messages, ten `note`s — among them a decision, a factual correction, a
  blocking constraint and two open questions, all arriving identically. The banner has rendered
  `[direct·proposal]` all along, so the label was visible and only the flag that sets it was
  not; the sole sender who ever set it was the one who had just read `--help` in order to report
  that nobody reads `--help`.

- **`amb read <id>` shows the message before it acknowledges it** (U9). The verb was the bug: a
  banner says "1 unread", `amb read 3` is the obvious thing to type, and it printed
  `marked #3 read` and nothing else — while the acknowledgement dropped the message out of
  `amb inbox --unread`, the view the primer teaches. Two sessions independently ended up piping
  `--json` through Python to recover a message they had been told about and never seen.
  Acknowledging is now a consequence of reading rather than a substitute for it. Rendered through
  the existing `render_inbox`, so this adds a caller rather than a fourth renderer for the
  containment enumeration to fall behind; `--all` keeps its terse summary, being the bulk verb.
  The first version ran the body straight into `marked #1 read` on one line — M24's join defect,
  now asserted.

- **Gemini CLI is a supported vendor, and a Gemini session can message a Claude one across
  projects** (D111). `amb install --vendor gemini-cli` writes Gemini's own event vocabulary
  (`AfterAgent`, `AfterTool`, `BeforeTool`) into `~/.gemini/settings.json`; every value was read
  out of the installed 0.55.1 bundle, which has **no `PreToolUse` or `PostToolUse` at all**, so
  a descriptor written from the docs would have installed silently-ignored entries. It hosts two
  memory lanes rather than three because nothing in it fires only on failure, and `HookState`
  now carries the total it was measured against so that is never misreported as a partial
  install. The host vendor is detected from the session-id environment rather than passed as a
  hook argument (D97). Cross-vendor delivery needed no new code — the board was never
  Claude-specific — and is asserted end to end through the real binary.

- **A vendor is data now, not code** (D111). `src/vendors.rs` holds a `Vendor` descriptor —
  config directory, settings filenames, the six event spellings, the session-id environment
  variables — and the install path takes one instead of assuming Claude's. Claude Code remains
  the only descriptor that ships and **nothing changed behaviourally**: 604 tests before, 604
  after. The seam is asserted by a second, fabricated descriptor whose events and paths must come
  out of a plan while Claude's must not; re-hardcoding either was applied by hand and seen red.
  This reopens D101 on the second of the two conditions it named for itself.

- **The gate's test count now says when it is measuring a tree that is not the commit** (D110).
  `check_docs.py` takes the count over the working tree while CI takes it over committed code;
  on a machine where several sessions share one checkout those differ, and twice in one day a
  count described a tree nobody was about to commit — the quiet near-miss being a number that
  *matched* the working tree and would not have matched the commit. Unstaged `.rs` edits now
  produce an advisory when the numbers agree and are folded into the failure text when they do
  not, so a mismatch arrives with its cause attached. Deliberately **not** a failure: the
  condition is the normal state of this repository under its own selective-staging practice, and
  a gate that is habitually bypassed is worse than one that is occasionally wrong. D110 records
  both rejected alternatives.

### Fixed

- **D49's promotion kill switch answered to one of the three spellings its own documentation
  publishes** (M60). `AMB_MEMORY_PROMOTION` is documented in the README's environment table as
  accepting `0`, `off` or `false`; only `off` was ever tested, and deleting the other two arms
  left the entire suite green — so a person who read the docs, set `AMB_MEMORY_PROMOTION=0` and
  expected promotion to stop would have got promotion, on the mechanism D49 names as the response
  to approval degrading into a rubber stamp. The decision is now injected and every published
  spelling is asserted, along with the on-cases: a switch that reads *on* as *off* silently
  disables the phase for people who never asked.

### Added

- **The seam audit: every `std::env::var` in the library, asked whether a test can reach the
  decision behind it** (M60). Three of six shells could not be reached, and the two beside the
  kill switch were `broadcast_horizon` — one caller, no test at all, where a zero fallback stops
  every broadcast being delivered — and `vault_path`, whose first two lines *are* D35 and neither
  was asserted, so an empty `AMB_VAULT` switched memory on pointed at the session's working
  directory. All three are now injected seams with truth tables, each confirmed by mutating the
  decision. The three that passed had one thing in common and it was not importance or age:
  somebody had already pulled the decision out of the shell.

- **The four frictions a heavy session actually hit, three of them fixed** (U8). `--json` now
  carries an `address` beside `from`, because `from` is a display name and a display name is
  not an address — a session copied one out of an inbox and `amb send` refused it. The refusal
  now offers the address it already knew (`did you mean carol@other?`), as its own error
  variant so the plain "nowhere" case cannot invent a suggestion. And the SessionStart primer —
  the entire API for an agent that never runs `--help` — now teaches `--unread`, `--body-file`,
  `amb claims`/`amb claim`, and `--force decision`, each with the non-guarantee that makes it
  safe to use. The claim verbs matter most: a board where the most careful agent announced its
  file scope in prose, in a message body, had a structured mechanism one undocumented verb away.

- **M56's round is closed: the last five survivors in `memory/index.rs` are guarded** (M59).
  `history`'s two cycle breaks flipped to `!=` truncate every ordinary lineage after one hop, and
  the cycle test already sitting on that code could not see it — its two-note fixture makes the
  honest walk and the mutant agree, so the fixture had to become a four-note *chain*. On the scope
  `match`: a candidate must carry the empty scope (D50/D81) or it is filed where nothing looks for
  it, and deleting the project arm still compiles while handing back an unsanitised name — a rule
  that was asserted against `vault_dir` while `sync_dir` is what writes the row. Reaching that arm
  needed `..` rather than `../../../etc`: the latter contains a `/`, so `parse_scope` refuses it
  and a different arm sanitises it anyway.

- **The nine surviving mutants in `src/main.rs` are guarded, and three of them needed the
  binary's rendering moved into the library first** (M58). `report_plan`'s retry line is a guard
  over a count whose three relaxations all survived, and it could not be asserted where it lived:
  a retry needs another process to write `~/.claude/settings.json` mid-cycle, which a test cannot
  stage. The human half is now `hooks::render_applied` — pure, unit-tested by truth table — and
  the unlocked-write warning and the no-op line got their first assertions with it. `main.rs` is
  34 lines shorter, which is D78's rule kept rather than restated. Also guarded: the advisory
  sentence on a contended claim, `snapshot`'s unread filter *and* its scope label (either could
  be inverted alone), the `amb watch` hint's two conditions, and `export --check`'s drift count —
  the human-report counter seam's **fifth** sighting, on the sibling lane of M54's fourth.

- **`tools/check_mutation_coverage.py` — the completeness claim made derivable** (M57). "Every
  module has been mutation-tested" was asserted three times in one day and was wrong all three
  times: M55's claim was three files short, the correction that caught it was short by one, and
  the checker written to end it repeated the error on its first run by reading one of the
  record's three formats. It now set-differences `src/**/*.rs` against every recorded round,
  carries two zero-mutant exemptions with the command that verifies them, and fails the gate
  only when a current-state document claims closure the difference denies — an uncovered module
  is printed and forgiven, because mutation is deliberately not a gate. Proven by a four-row
  truth table including the row where it fires. The inventory did close, at M56.

- **`src/memory/index.rs` and `src/main.rs` under exhaustive mutation — the two files the
  inventory had missed** (M56). 140 mutants, 34 missed; eighteen now guarded by six tests.
  Fifteen of the 34 were `+=` on `IndexStats` counters that could become `*=` and stay zero
  forever, so `amb memory index` would report `0 scanned · 0 indexed` over a vault it had just
  walked — the human-report counter seam's **fourth** sighting, on the same struct D45 was
  written about. Also guarded: `excerpt_of`, which is the corpus `recall` searches (D88) and
  could be emptied silently, with exact-boundary rows at its 240-character cap; and
  `render_history`'s `&&`, which as `||` made a note *with* lineage print "stands alone".
  Two mutants are unreachable on macOS because APFS refuses a non-UTF-8 filename, so that test
  is gated to Linux; fourteen are named in M56 rather than quietly dropped.

- **`tools/cfg_phantoms.py`: a MISSED row that means "not compiled on this host" is now
  classified rather than remembered.** `cargo mutants` does not evaluate `#[cfg]` and says so in
  its Limitations chapter, so mutating a Linux-only function on macOS prints MISSED for code the
  binary never contained — 16 of `db.rs`'s 29 missed rows in one run (M46). `tools/mutants.sh`
  now ends by calling it, and it refuses rather than guesses on any predicate it cannot model.
  The documented `cfg_attr` workaround is deliberately not used: cargo-mutants does not evaluate
  that condition either, so it would skip the mutant on every platform including the one where
  the code is live.

- **The final eight library modules under exhaustive mutation** (M55) — one pass, 360 mutants,
  45 missed, all guarded or named: calendar round-trips, FNV-1a pinned to published vectors,
  exact-boundary rows for every render unit, id-grammar truth tables, two more env-shell seams,
  and the error cause-chain read for the first time. One equivalent mutant kept with its
  reasoning; one wrong equivalence claim of mine caught by applying the mutant instead of
  believing the argument. **The crate-wide inventory did not close here** — `memory/index.rs`
  (79 mutants) and `main.rs` (61) have never been in a round, while `lib.rs` and the `memory.rs`
  facade are exempt because they generate none at all.

- **`memory/export.rs` under exhaustive mutation: one missed of 26 viable, and it was the
  exporter's own receipt** (M54). `written += 1` could become `*=` and the person running
  `amb memory export` would be told nothing was written while every file landed — the
  human-report counter seam's third sighting this week (M27, M51). Guarded with a count-and-body
  fixture. The first run died at ENOSPC when the machine's disk hit zero mid-pass and was
  redone clean rather than salvaged.

- **`memory/redact.rs` under exhaustive mutation: thirteen missed, nine now guarded, four named
  equivalent** (M53). Nine sat on the one boundary the module exists to draw — quotes and commas
  counted toward the length that convicts a value, `substantial` could convict an all-digit
  measurement, and the entropy floor had no boundary row (the first kill written for it was
  itself wrong, caught by re-applying the mutant before believing the guard). The four `!= ->
  ==` trim flips are equivalent — end-trimming cannot destroy an internal `contains()` match
  against the keyword list — verified by surviving the full suite by hand, and recorded rather
  than faked away. Every guard asserts text unchanged *and* `removed == 0`: on this module a
  wrong count is a redaction the author was never told about.

- **`memory/events.rs` under exhaustive mutation: 89 of 90 viable caught on the first pass**
  (M52) — the instrument module's D89–D95 truth-table discipline corroborated by machine. The
  one survivor was equivalent under the lane/session invariant and is now pinned by an
  impossible receipt whose comment owns the vacancy: the all-zero gate is the first decision,
  so an inconsistent receipt fails safe to silence.

- **`install.sh` replaces binaries by sibling-then-rename, never in-place `cp`** — on macOS an
  in-place copy reuses a vnode whose code signature the kernel has cached, and a later exec dies
  with SIGKILL. Observed on the PATH copy while the hook copy landed clean, which is the nastiest
  split: manual commands dead, hooks fine, and `amb doctor` unable to run to say so.

- **`memory/capture.rs` under exhaustive mutation: 23 missed on the first run, zero on the
  re-run** (M51). D108's marker machinery was tested exactly down to its injection seam and no
  further — the path-injected reader held while `note_failure`, `note_success`, `session_key`
  and the staleness window's arithmetic could all be gutted, green. The writers now have injected
  cores with row-by-row tables, the transcript fixture reaches the `status == "error"` arm in
  both directions, `decline_rate` refuses `0/0`, and one e2e drives the whole D108 story through
  the real binary: the corpse marker that still counts, the sanitised session name beside the
  board, the warning riding a healthy session's output, and a heal clearing only its own count.

- **The reached-assertion audit, run over every limit-like constant** (D102's discipline applied
  outside the property file). Twenty-three caps and thresholds swept against how their tests size
  fixtures: the property suite's seven floors are complete, `sync_dir`'s bound is
  parameter-tested, the horizon and back-off are covered, and caps fail loud on drift. Two fixes:
  `history`'s two cycle breaks had never been reached by any fixture — a hand-editable
  `superseded_by` cycle on the `SessionStart` path — and a cycle test now reddens if either break
  is deleted; the kind cap's 21-x literal is sized off `MAX_KIND + 1` so a grown cap cannot
  quietly turn the refusal row into a valid kind.

- **`memory/write.rs` under exhaustive mutation: the whole missed set was `free_slug`** (M50).
  Eight of eight survivors sat in the collision loop whose docstring calls silent overwrite "the
  one thing this design promises never to do" — no test had ever collided two same-day same-title
  notes. One sequential fixture drives every branch: bare first stem, `-2` on collision, and the
  200-probe cap asserted as the bounded-work trade it is. Seven mutants seen red by hand; the
  eighth (`+= → *=`) detected as a hang under collision, recorded as the designed detection.

- **Claim conflict lines are contained like mail** (D105). `holder`, path and `--intent` go
  through `delivery::quoted` in `claims::summarise`; before this, a newline in an intent put a
  forged `[amb]` line at column zero of an injected conflict block — reproduced against a
  scratch board, then guarded at library and binary level. The aggregate rows also now say when
  each claim lapses (`alice · src/auth/ (2 files) · in 2h — refactor`), which was the first
  question a conflicting peer asks and only `--raw` could answer.
- **Write-path caps for the body's siblings** (D106): subject 500, claim intent 500, explicit
  display name 80 — one `FieldTooLarge` refusal at the sender, exit 64, nothing stored. A
  300 KB subject was accepted verbatim before this.
- **`--kind` is rendered, and it is a charset** (D107). Anything but `note` shows in the header
  (`#7 [direct·question]`) on all three message surfaces; `[a-z0-9_-]{1,20}` is refused at the
  sender and independently enforced at the renderer, where an untame kind degrades to the scope
  alone rather than to grammar a sender controls. The flag's help no longer teaches
  `claim_notice`, a value nothing has ever written.
- **The inbox says what is new** (U1): header counts unread, `*` marks unread rows on amb's own
  id token, and inbox `--json` rows carry `"read"` — `get()`-backed paths, which cannot know,
  omit the key rather than inventing it.
- **`SessionEnd` lapses the departing session's claims** (D109) — the fourth hook event in
  `turn`/`monitor` modes, same command, no new argument. Expiry not deletion, peers untouched,
  TTL kept as the crash backstop. Re-run `amb install` (or `./tools/install.sh`) to pick it up.
- **A corrupt board names its own remedy**: corruption-shaped open failures (and only those —
  a busy board keeps its message) now say the board is disposable and the vault unaffected
  (D15), instead of stopping one sentence short of the fix at the moment it is needed.
- **A scheduled advisory audit** (`.github/workflows/audit.yml`): RUSTSEC advisories land
  between pushes, so the existing per-push `cargo-audit` gains a weekly cron with the same
  tool — extending the recorded audit-not-deny choice, not revisiting it.

### Changed

- **`amb install --dry-run` reports the delta, not the desired state.** A reinstall whose only
  difference was one new event printed seven `+` rows — a one-entry edit reading as a wholesale
  rewrite until the JSON was diffed. Additions and removals now cancel as *identical pairs*
  (label, matcher, and entry content), so an exe repoint — D94's case — still reports every
  entry as both removed and re-added, because every one genuinely is. `searches` is measured
  into D83's growth picture as M49: one row, the slowest ledger on the board.

- **The capture failure counter is per-session, and the fail-loud notice is machine-wide**
  (D108). One shared `.memory-failures` file let any healthy session clear a broken session's
  consecutive count indefinitely; markers are now keyed by session, the reader takes the worst
  fresh marker (the broken session cannot deliver its own warning — its notice travels through
  the memory hook's success path), and month-stale markers are filtered as crashed-session
  residue.
- **`memory history` on a nonexistent id is exit 65** like every other id-taking command; it
  printed "stands alone — it replaced nothing, and nothing replaced it" for a typo, exit 0 — a
  provenance command fabricating a clean history (U5).
- **`amb watch`'s mail path ends with a newline** (it used `print!` where the timeout path used
  `println!`, concatenating the last mail line with the caller's next output), and `release`
  echoes the path as stored, matching what `claim` printed.
- **The README's exit-code table is complete**: 70 and 73 shipped unmapped in the docs — D97's
  shape one layer up — and the doctor-exits-0 caveat now sits beside the table where a script
  author looks. Also new there: a stated no-telemetry/no-network guarantee, what to back up
  (vault, not board), and the `--json`-is-the-contract paragraph.
- **A blank tier of flag help strings is filled** (ten flags including `send --thread`,
  `register --name`, `recall --project`), and `sync_dir`'s transaction comment no longer claims
  `busy_timeout` covers a snapshot-stale upgrade — `SQLITE_BUSY_SNAPSHOT` returns immediately,
  the lost race is accepted, and the next hook pass is the retry.
- **`journal_size_limit = 4 MiB`** joins the standard pragmas: long-lived-WAL truncation
  hygiene, a no-op until checkpoint starvation would otherwise let `-wal` grow unbounded.

- **`memory/query.rs` under exhaustive mutation: 48 mutants, three missed, all three now
  guarded** (M48). Two were halves of the path-lookup windowing rule — a window no fixture had
  ever filled and a count fallback that could fire on every vault without the total moving — and
  one was `resolve`'s unique-bare-slug arm, the path every ordinary `--cites <slug>` takes,
  which no test reached. One production-writer fixture (files through `reindex`, not hand-built
  rows) kills the first two; a presence row kills the third. Each mutant re-applied by hand and
  seen red.

- **A second simplify pass, over the first one's own commits.** Four review lanes over the four
  unpushed commits — including the pass below — converged on one shape: a rule the diff itself
  states, applied to half its instances. `claims::list_sql` returns clause and binds as one pair
  again (the interim form kept the SQL in one function and the binds in `list`, agreeing by
  comment, with a third hand-built copy in the plan test — the exact two-chain drift the deleted
  docstring had named, minus the dead `None`-project axis it was right to remove);
  `hooks::event_name` and `hooks::is_subagent` join `is_stop_refire`, closing the three-copy
  `hook_event_name` extraction and the untested subagent gate that sat five lines above the
  predicate D78's rationale hoisted; the `--direct` promote gate's prose twin moved beside
  `gate_json` (M26's last arm, claimed finished one commit early); `Integrity::from_probe`
  names the probe-to-verdict map beside the enum with all four rows asserted — the
  `Some(Err(_))` arm was reachable by no test — and `doctor::gather` asks `exists` once instead
  of twice; `tools/mutants.sh` refuses a `$TMPDIR`-resolved target dir mechanically, because a
  header saying "check nothing moved it back" is D39/D45's note-instead-of-script failure. Each
  new guard was deleted once and its table watched go red before the byte-identical revert.

- **A simplify pass over audit round two, by its own rules.** Four review lanes over the four
  fresh commits found the diff's own patterns applied to half their instances, and the fixes
  finish them: the sync probe's SQL is one named constant asserted by its plan test (the
  `claims::list_sql` rule, which the probe's test had broken by re-typing the string); the
  Stop-refire predicate moved from the binary into `hooks::is_stop_refire` with a truth table
  (D78's pull, caught one commit later this time); the window *report* and both promote-gate
  refusals now take their JSON from the library beside their prose twins (M26, which the same
  commit had argued and half-applied); `doctor` holds one connection for every board question
  instead of opening a second for freshness, and `integrity`/`vault` take three named verdicts
  and an `Option<usize>` instead of `Option<Option<String>>` and an `is_dir`-beside-count pair
  that could assert a note count for a vault that does not exist. `claims::list` takes the
  project every caller has — the clause/bind assembler generalised over a `None` axis nothing
  could reach. Two doc blocks hijacked by mechanical insertion — `render_window_report`'s D87
  argument documenting an `impl`, `plan_uninstall`'s summary stranded on `tool_and_file` — are
  reattached; D103 now names the guards that exist (the `const` assertion and the db.rs
  read-back test) instead of a test name that never did.

### Added

- **`amb doctor` can now say the board is corrupt, and what the vault actually holds** (audit
  round two). No `quick_check` ran anywhere, so corruption surfaced as whatever query failed
  first — usually inside a hook that swallows errors by contract (D9). And the vault line was an
  unconditional `Ok` echoing a path: the *disposable* board was guarded against synced volumes
  and size-checked while the *irreplaceable* vault had no existence check at all, so a typo'd
  `AMB_VAULT` reported healthy while every observe failed. `integrity` reports `quick_check`
  with the response attached (a corrupt board is deletable, D15 — no note is lost, D34); `vault`
  is a verdict carrying the note count.

- **`--locked` on every dependency-resolving step in the gate and CI, and an explicit release
  profile.** The 2026-08-20 crates.io incident — a compromised maintainer account shipping
  build-time code execution through a typosquatted proc-macro, ~90-minute window — is defended
  at this project's scale by a committed lockfile that fails loudly instead of updating as a
  side effect, plus the advisory check CI already runs. The evaluated-and-declined `cargo-deny`
  position in `ci.yml` stands: its escalation condition (crates.io publication) has not fired.
  `[profile.release]` adds thin LTO, one codegen unit and symbol stripping; the README's startup
  figures are re-measured against the new profile, per this project's measurement rule.

- **`amb doctor`'s morning-after answer, measured instead of asserted** (M44). Q14 filed the
  distribution question on the D94 hazard — `brew upgrade` is the stale-hook condition firing with
  nobody thinking about `amb` — and answered its own test with "the detector already exists".
  Simulated under a sandboxed `$HOME`: `doctor` prints `BAD`, both fingerprints, *"Manual commands
  work and every hook is stale"*, and the literal `cp` that fixes it. Two caveats now recorded in
  Q14: the verdict is `--json`'s `worst`, never the exit code (D73), and the comparison keys on the
  executable being *named* `amb` — a renamed binary makes hooks invisible rather than stale.

  The run also exposed three `doctor.rs` comments claiming `worst` drives the exit code — a design
  D73 explicitly rejected, the fifth false-comment instance, sitting on the one field an unattended
  check would be built on. All three now state the true contract.

- **The reached-assertion audit: seventeen gate constants, two holes, neither a threshold** (M45).
  A literal-coupled threshold fixture usually fails *loud* on drift; the silent class is an
  absence assertion behind a gate, and a writer no test reaches. `sync_dir`'s decline branch —
  D45's exact defect site — had no test caller anywhere: the readers were asserted on hand-built
  stats while the write could vanish green, and only the early return stops a declined pass from
  pruning the whole index against a scan that never happened. Now tested at the writer, both
  bounds. And `a_git_sha_is_not_mistaken_for_a_secret` gained a one-case-flip control at the same
  forty bytes, so entropy-gate drift reddens a row instead of quietly changing the test's subject.

- **`src/db.rs` mutation-tested** (M46) — the schema, migrations, location guard and WAL
  engagement, none of it mutated before; details under Fixed and in `docs/MEASUREMENTS.md`.

### Fixed

- **`quick_check` could be always-healthy and nothing reddened.** The diff pass over the four
  fresh commits (M47) found all four mutants of doctor's new integrity check surviving — the row
  D15's "the board is disposable" advice hangs from, rendered from nothing. A two-verdict test
  pins both directions; the corrupt fixture is one overwritten page.

- **`tools/mutants.sh`'s target directory moves out of `$TMPDIR`.** macOS's age-based cleaner
  was eating `libsqlite3-sys`'s generated `bindgen.rs` *mid-run* — the bundled file carries its
  packaged 2006 mtime and is eligible for eviction the moment it lands. Two consecutive baseline
  failures with a delete in between proved the cleaner concurrent. Now under `~/.cache`, and the
  header records the mechanism instead of prescribing the delete-and-retry that failed twice.

- **A Stop re-fire is now answered with silence, which ends the machine-wide wake loop.** The
  runner counts a Stop hook that injects `additionalContext` as blocking the turn from ending:
  it wakes the model to read the context, the model answers, Stop fires again — with
  `stop_hook_active: true`, which is the runner saying "this firing is that wake". `amb` never
  read the flag, so any *persistent* condition with something to say looped: during two
  stale-binary windows (2026-08-27 and 2026-08-31, read out of the session transcripts) the
  arrival note printed on every Stop, and sessions in five projects each cycled banner →
  "Standing by." → banner to the platform's nine-block cap. `hook_main` now returns success,
  silently, on any payload carrying the flag — before dispatch, so it covers delivery, the
  arrival note, and every future speaker. Nothing is lost: delivery is a log (D17), and the next
  real event re-offers whatever silence withheld. The forensic note: yesterday's audit probed
  this hook's *exit code* against the blocking question and pronounced it innocent — the wrong
  instrument, since context injection blocks at exit 0. The transcripts were the instrument that
  could answer, and they implicated us.

- **`amb watch`'s human output was an unguarded fourth renderer of sender-written fields**
  (audit round two). `main.rs` printed `sender` and `subject` through a bare `println!` for as
  long as the command existed — the exact forgery D90 closed in `render_inbox`, standing because
  the enumeration test can only redden for renderers it lists, and its own docstring named that
  residual hole. Found by grepping the field literal rather than the fixed function, which is
  this file's own rule. Watch now routes through `render_inbox` (gaining bodies and the
  `UNTRUSTED` sentence), and `watch_cannot_be_forged_by_a_newline_in_a_subject` pins it at the
  binary — the layer the library test cannot reach (M20).

- **The claims query defeated its own index on the hottest hook path.** `claims::list`'s
  `(?1 IS NULL OR c.project = ?1)` idiom is invisible to the planner — it cannot know a
  parameter is non-NULL at plan time — so `ix_claims_live` was never used and every
  `PostToolUse` scanned the whole table, one whose per-session-UUID key means it only grows.
  Worse, `conflicts_with` passed `live_only = false` under a docstring saying "live", fetching
  every lapsed claim ever taken and discarding them one line later. The WHERE is now assembled
  from plain equality clauses, `conflicts_with` filters in SQL, and the guard is on the *plan*:
  `the_project_filter_reaches_the_index` asserts `EXPLAIN QUERY PLAN` names the index, because
  the rows were always right and no result-shaped test can see this defect.

- **The memory index probed an unindexed column once per file, in autocommit** (migration 13).
  `sync_dir`'s per-file mtime probe seeked on `kind` and then walked every note of that kind —
  quadratic in vault size, on `SessionStart`. Synthetic measurement at 5,000 notes: 177 ms per
  hook pass, 8 ms with `ix_notes_vault(kind, vault_path)`. The pass now also runs in one
  deferred transaction instead of an autocommit per statement, and the prune's membership test
  is a `HashSet` instead of a linear walk. Schema 12 → 13; run `./tools/install.sh` after
  pulling, or every hook on the machine fails silently against the migrated board (D94's shape).

- **A hook could wait 30 seconds on a lock inside a 5-second budget** (D103). `busy_timeout`
  served the interactive CLI and the hooks with one value chosen for D30's first-open stampede,
  so a contended hook was killed by the platform mid-wait — the one ending D9's exit-0
  guarantee cannot absorb. Hook entry points now open with `db::open_at_for_hook` (2 s, inside
  the open because `migrate` stalls before any post-open override could apply), asserted against
  the budget by a test so the two constants cannot drift apart again.

- **The vault's temp file was not pid-scoped, while the settings writer's deliberately is.**
  Two processes rewriting one note interleaved on a single `.amb-tmp` path — writer A's rename
  could publish writer B's half-written bytes, and `observe`/`supersede` take no lock that would
  prevent it. The sibling-left-standing shape (D86/D88/D90) landing on the one store that is
  irreplaceable. The name now carries the pid, asserted by test.

- **The primer promises `--json` on any command, and three arms broke it.** `memory window`
  (both branches) and both `promote` gates printed prose unconditionally — an agent parsing
  stdout got unparseable text on exactly the human-gate paths. All three answer in JSON now;
  `written: false` carries the gate, and `changed` keeps D87's `AlreadyOpen`-is-not-`Opened`
  distinction alive in the format.

- **A newline in a note title could forge a derivation row on the promotion offer, or a
  section heading in an exported file.** The two note-title renderers *outside* the injection
  ledger — `render_offer`, the human approval gate, and `render_export`, which writes a `# `
  heading into a checked-in repository file — now route the title through `quoted()`. The four
  injected renderers (`recall`, `candidates`, `observe`'s near-lines, `history`) stay raw on
  purpose until the open measurement window closes: changing what a note renders mid-window
  changes what sessions cite (M23's deferral, still standing, now four wide instead of six).

- **`engage_wal`'s failure half had never run under a test.** Ten mutants survived in the retry
  loop: the guard verifying SQLite's answer could be forced `true` — D30's "checked rather than
  assumed" check dead, any journal mode waved through — and every deadline comparison could flip.
  The one deterministic refusal in the tree is an in-memory database, which always answers
  `journal_mode = WAL` with `memory`: the new test asserts the error arrives with the real mode
  in it and *no sooner than the full budget*. The deadline comparison now exists once, in
  `budget_spent`, where a test reaches both sides.

- **`restrict::tighten` could widen a mode the user chose tighter than ours.** All four bitwise
  mutants of its gate read `0o400` as loose and chmod it to `0o600`; three of the four reported
  TIMEOUT under machine load rather than surviving outright — the flattering direction
  `tools/mutants.sh`'s header warns about, confirmed by re-running them from a clean worktree on
  a quiet machine. A truth-table test now pins both directions.

- **The macOS local-volume bit is read by a pure function.** `&` → `|` and `&` → `^` on
  `MNT_LOCAL` both read every remote volume as local and nothing could redden — no test can mount
  a network share. Extracted `statfs_is_local`, where the flag word is synthetic and the boundary
  is one bit away. The Linux arm gets `#[cfg(target_os = "linux")]` tests, because a MISSED row
  in cfg'd-out code means "not compiled here", not "untested" — now the third trap in
  `tools/mutants.sh`'s header.

### Changed

- **`is_unique_violation` is now `is_constraint_violation`**, with M43's schema argument as its
  docstring: it always matched any constraint violation, sound only because `ux_agents_name` is
  the sole constraint reachable through `try_touch`.

- **`src/identity.rs` mutation-tested — 97.7%, a new high** (M43). 92 mutants, 2 missed, 0 timeout.
  Both survivors were one rule at its two call sites: forcing `is_unique_violation` to `true` in
  `reclaim` and in `register` reddened nothing.

  **The test guarding that rule names those exact call sites in its own docstring** — it was itself
  found by mutation, and says that treating an unrelated failure as a name clash "would rename an
  agent in response to something that has nothing to do with its name". It asserts the predicate
  against a synthetic table and touches neither caller. So mutation found the predicate, the fix
  guarded the predicate, the comment named the call sites, and the call sites stayed unguarded
  until mutation was pointed one layer out. A comment naming a call site is not a test of it.

  Cost: a board that cannot be written reports `NameTaken`, telling an agent to pick a different
  name for a condition no name can fix. Closed at both sites, both mutations confirmed red.

  Noted while writing the test: `is_unique_violation` matches *any* `ConstraintViolation` despite
  its name. Correct by the current schema, and the reason the fixture induces failure with a
  trigger naming a missing table rather than `RAISE(ABORT)`, which is itself a constraint.

  M42's claim that "never mutated" predicts a low score is withdrawn — `identity.rs` had never been
  mutated and scored highest. Nothing available predicts it, which argues for running it everywhere
  rather than triaging.

- **`src/doctor.rs` mutation-tested — 93.0%, the highest in the project** (M42). 68 mutants, 4
  missed, 0 timeout. All four sat on `let mb = bytes as f64 / (1024.0 * 1024.0)`, while the
  identical expression one line below was caught because a test asserts the threshold it renders.

  Two tests covered half of it each and the halves did not overlap: one asserts the rendered size
  but at `size_check(0)`, and **zero is the fixed point of all four mutations**; the other uses
  inputs that discriminate perfectly and asserts only `.health`. M17's fixture problem arriving
  through a pair of tests rather than one — neither is wrong, and re-reading either would not find
  it. It cost `amb doctor` printing `1536.0 MB` or `3298534883328.0 MB` for a 3 MB board with the
  verdict still correct. Closed with a truth table over the rendered size, all four confirmed red.

  Also the second refutation of the renderer hypothesis after `delivery.rs`: what predicts a low
  score is not what a module produces but whether it has ever been mutated.

- **`find_unread_fields.py` counted rustdoc links as by-reference uses** (M41), which is why it
  filed `hooks::write_settings` under *"passed by reference, so it looks uncalled"* — the
  reassuring arm — every run for days, while the function was dead. ``[`write_settings`]`` is a
  bare mention with no parentheses and a regex over raw source cannot tell it from
  `.is_some_and(f)`. References are now counted over a comment-stripped view, and the two arms
  print separately because they mean opposite things and were being skimmed as one block (D84).

  Verified by reconstructing the condition: a `pub fn` with two rustdoc self-links and no callers
  lands in the loud arm with the fix and in the reassuring one without it.

- **Eight properties of the pure core, over generated input, with no new dependency** (D102).
  `tests/properties.rs` asserts totality claims no example list can make — `overlaps` is symmetric
  and reflexive, `quoted` never emits a control character and is idempotent, `redact` is
  idempotent, `nearest` only ever suggests a name it was given. 20,000 cases, ~0.25 s against a
  3.3 s suite.

  **`proptest` and `insta` were both evaluated against real defects here and both declined.** The
  arguments for them were mine and neither survived checking: M17's tie guard already has two
  fixtures reaching the two-candidate arm, and M24's lesson already shipped as
  `assert_rendered_shape` with 21 call sites. Eight properties over 200,000 inputs found **zero
  violations**, so the case rested on future value.

  What decided it: **the generator is the hard part and a framework does not supply one.** The
  first version of this file left two of its eight properties *vacuous* — `redact` fired zero times
  in 200,000 runs and nothing parsed as a duration. `any::<String>()` has that problem too, so
  custom strategies would be the same work in another notation; what a crate adds is shrinking, and
  with no failures there is nothing to shrink.

  **The coverage floors are the substance.** The test ends by asserting how often each branch was
  *reached*. Without them a generator that stops producing redactable strings reports success —
  M17's defect inside the test written to catch it. That happened twice while writing it: mutating
  `quoted` to pass control characters through **survived**, because the alphabet contained none, so
  the property guarding D90's forgery attack was asserting nothing. Control characters are now
  generated and floored, and that mutation reddens with a readable counter-example.

  `delivery::QUOTED_MAX` is `pub` so the test reads the cap instead of transcribing it (M28).

### Added

- **`src/hooks.rs` mutation-tested for the first time — 86/103 viable, and one survivor was dead
  code** (M39). M27's roll-call named it as the next target: it edits `~/.claude/settings.json`,
  which configures Claude Code for every project on the machine. 114 mutants, 17 missed, 0 timeout.

  The seventeen were four findings. `our_hook_exes` (6 survivors, including `vec![]`) is the only
  thing that can see the stale-binary condition D94 records five times, and `doctor`'s tests build
  their fixtures directly, so the producer ran under no assertion — M37's shape one module out.
  `read_settings`/`read_raw`/`apply` (8) had no test on any branch of the read path. `quote_exe`
  (1) is D90's shape: `/Users/o'brien/bin/amb` *is* tested, but for recognition and removability,
  both of which pass on an unquoted command line — the sibling asserts the quoting and only for a
  space. `Mode::parse` (1) is the contract check behind `amb install --mode <x>` and its variants
  are asserted 38 times while the parser never was.

  **`write_settings` was dead** — one definition, zero calls, and `find_unread_fields.py` had been
  explaining it away for days under its own "passed by reference" advisory, which is true of the
  other two names it prints and false of this one. Mutation is what separated them. D84 recorded
  this shape once already, with a different name. Deleted rather than tested: superseded by D99's
  `apply` cycle, and left in place it is a public writer that skips the lost-update check.

  Five tests added, each verified by applying the exact mutation it was written for.

### Fixed

- **`counts_are_current` counted a peer's untracked scratch file, and the repair made the check
  unfailable** (M40). `cargo test` compiles every `tests/*.rs`, so an untracked `props_probe.rs`
  moved the number the docs must quote. Scoping it to `git ls-files "tests/*.rs"` then matched
  `tests/common/mod.rs` too — a git pathspec `*` crosses `/` — so `--test mod` made cargo exit 101
  with empty stdout, `actual` became 0, and `if actual:` skipped the comparison and printed
  success.

  That is `if not tag: return []` in the function directly above the one repaired for it. Caught
  only by perturbing the guard and watching it fail to redden. Both fixed: files directly under
  `tests/` only, and a cargo run that reports no tests is now a finding rather than a clean result.

- **`tools/mutants.sh`'s header claimed the suite runs ~145s in the sandbox; it runs 5-11s** (M39).
  Third rotted constant in that one header — M28 found the other two. The eighteen logged phases
  are all survivors, which run the suite to completion, so 11s is an upper bound rather than a
  sample. The relative timeout stays, for M27's load-variance reason rather than the absolute one
  the header gave.

- **Q8 settled: `amb` stays Claude-Code-only** (D101). Q8 framed this as breadth against the cost of
  a per-vendor hook matrix. The prior question is whether a cross-vendor mechanism exists that could
  be integrated *once*, and that was checkable rather than arguable: MCP cannot push into a running
  session, and the request was closed `NOT_PLANNED` twice — `anthropics/claude-code#36665`,
  consolidated into `#35072`, which is itself closed and labelled `stale`. The MCP roadmap has
  server-initiated events as **planned, not shipped**.

  The cross-vendor standard that did arrive in 2026 standardises the wrong half: `SKILL.md` is an
  open format whose client showcase covers every vendor Q8 contemplated, and a skill is invoked when
  the agent decides to — D9's rejected shape and MCP Agent Mail's conceded failure. (The record first
  quoted a vendor count from a secondary source; the primary one states none, and the argument never
  needed it.) And the matrix is priced from `agmsg`'s own: only its Claude Code
  lane gets real-time delivery, every other vendor degrades to checking between turns. So breadth
  buys vendors on which D9's guarantee is *weaker than it is today*, plus a hook-runner contract
  each — D97 is what one of those costs when it goes wrong.

  Two reopening conditions, both publicly checkable and both able to fire (D95): `#35072` reopening
  or server-initiated events shipping and being surfaced; and a second agent tool actually in use
  here. Q8's own competitor figures had rotted — `agmsg` is at 1.5k★ and nine vendors, not five —
  and D101 records that `hcom` is a single Rust binary with no local daemon, correcting a Q11
  sentence that reads as general and is only true of the cross-machine case.

- **Q14 filed: how anyone who is not us installs `amb`.** Split off from Q8 rather than settled with
  it, because pairing them hides the cheaper one. `hcom` is `brew install aannoo/hcom/hcom`; `amb`
  is a clone plus `./tools/install.sh`. Two things make it more than packaging: `publish = false` is
  a decision (D56), and **a package manager upgrading the binary is D94's stale-hook hazard with a
  worse trigger** — `brew upgrade` fires without anyone thinking about `amb` at all. Undecided;
  nobody has tried to install it.

- **`check_docs.py` enforces `OPEN-QUESTIONS.md`'s deletion convention** (M38). That file retires a
  settled question by deleting it, and `git log` was the net under that until the history was reset
  to publish the repository. The file says so and draws the right conclusion — a deleted question
  must leave its answer in `DECISIONS.md` — but the conclusion lived in one sentence nothing read.

  **It was already unhonoured, and the sentence asserting it is where it broke.** The reset note
  lists Q12 among the deleted and promises each of them "names the decision it became, immediately
  below"; Q12 is named nowhere below. Q13 was settled into D98 the same day and does not appear in
  the file at all. Both answers exist and are correct (D85, D98); what was lost is the pointer
  from the register that promises them. The check proposed for this, "every `Qn was settled` line names an
  existing D-number", passes on that file: a convention whose content is an absence cannot be
  checked by reading what is present.

  So the load-bearing rule is arithmetic, not a text search: the union of every `Qn` any doc cites,
  minus the open sections, minus the ones a retirement paragraph names, must be empty. It reported
  Q12 and Q13 on its first run. Three supporting rules — names a decision, that decision exists,
  nothing settled while still open — were all green on the broken file. Each of the four verified
  by breaking it, plus a `checks_can_still_fail` population so three of them cannot go vacuous.

  Residual hole, named rather than closed: a question deleted while no other document ever cited it
  leaves nothing to subtract, and only the archive outside this repository can see that.

- **`amb doctor` detects `amb` hooks registered in more than one settings scope** (D77 amended).
  D77 fixed a duplicate by hand and said plainly that nothing would catch the next one. The hazard
  is not tidiness: duplicated hooks make an injection **cost twice and count once**, because
  `note_events` is keyed so the second write into one session is a no-op — so the numerator of
  D59's citation ratio is unaffected while the denominator is understated. Invisible, and in the
  flattering direction, on the number the injection layer would be retired on.

  **Which files it reads is the whole design, and the obvious version would have missed D77's own
  instance.** Reading `~/.claude/settings.json` — the only file `amb install` writes, and the only
  one `doctor` had ever opened — cannot see a duplicate that spans the user file and a project
  `.claude/settings.local.json`, which is exactly what D77 found. The platform is explicit that
  scopes *combine* rather than override for list-valued keys, so `hooks::settings_sources`
  enumerates managed, project-local, project and user. A mutation dropping the project-local scope
  reddens a test.

  Verified against a reconstruction of D77's two files under a scratch `HOME`, not only against
  fixtures. Five mutations run; the fifth — `duplicate_check` always returning `Ok` — survived
  everything until a truth table was written for the *decision* as well as the detector, which is
  M27's shape. `claude --settings` remains a stated hole: a per-session flag leaves no trace a
  later process can enumerate.

### Fixed

- **`amb memory derive` redacted credentials in silence** (M37). It called `redact(...).text` at
  three sites and discarded `.removed` at every one, so a secret in a derivation was stripped
  correctly and the author was never told — in the flow D49 built entirely around a human seeing
  what they approve. `observe` has always printed `N value(s) redacted before writing`; `derive`
  now prints the same sentence, word for word, and counts what is *written* rather than what was
  examined. `redact(note)` was also computed twice, one line apart, with both results discarded.

  **The first fix was verified and the verification was worthless.** Two tests — a truth table over
  the renderer and a cross-renderer wording check — both passed, and relaxing `> 0` to `>= 0`
  reddened them. Then forcing the computed count back to `0`, which *is* the original defect,
  reddened **nothing in the whole suite**: both tests set the field on a fixture, so they guarded
  the reader and never the writer. M27's advice ("go and read what reads it") is right and
  incomplete — it produced two tests of the reader and none of the writer. Closed with an
  end-to-end test through the real binary, asserting both halves: the vault file does not contain
  the credential, and the output says a value was removed.

- **`unreleased_is_honest` fired on the changelog entry describing its own repair.** The check
  tested `"Nothing yet" in section`, so any prose *quoting* the sentinel counted as the sentinel —
  and the M36 entry below quotes it. The same commit that made the check unfailable in one
  direction made it unpassable in the other.

  Now matched as the section's whole content rather than as a substring: headings stripped, the
  remainder compared against the placeholder as a full match. **A sentinel searched for anywhere
  cannot survive being written about, and a `CHANGELOG` is precisely where it gets written about.**
  Truth-tabled over three rows — empty fires, a bare placeholder fires, real prose quoting the
  phrase does not.

- **Three cleanup findings from `/simplify`, one of which three independent reviewers reported.**
  `doctor::board_bytes` re-derived SQLite's `-wal`/`-shm` sidecar paths that `db::restrict` already
  built, so the fact that a board on disk is three files was asserted in two modules with nothing
  tying them together. Factored to `db::sidecars`, which is where the pragmas that create those
  files live. Also: a no-op ternary in `check_docs.py`'s parity check, which would have matched the
  *wrong* token had any gate label ever grown a flag; and a duplicated `path.exists()` in
  `doctor::gather`, one `stat` on a boolean already computed seven lines above.

  Not taken: `eyeball.sh`'s timing block overlaps `bench_startup.py`'s `bench()` textually, but
  `bench()` runs fifty iterations for a median and p95 while `eyeball` takes one spot reading —
  sharing it would multiply a diagnostic's runtime and change what it measures.

- **Two checks could not fail, and one was created by the fix for the other's sibling** (M36).
  M35 ended on a question — what state does a check need in order to be *able* to fail, and what
  routine operation destroys it? Sweeping every check answered it twice.

  `every_bench_script_is_named` was given `git ls-files` hours earlier so it would skip untracked
  work in progress. An empty index makes every script "untracked", so every script is skipped and
  the check returns `[]` — the identical shape as the `if not tag: return []` repaired in the same
  file the same day. `CLAUDE.md` records that fixing one instance trains attention on the thing
  fixed rather than on its siblings; here the repair *produced* the sibling.

  `tools/check_secret_literals.py` uses `check=True`, which catches git failing and not git
  succeeding with an empty index — the state between `git init` and the first `git add`. This
  repository was in that state during a history reset **whose purpose was getting past secret
  scanning**, and in that window the check printed `no credential-shaped literal in tracked source`
  having opened no file. Both now report an inability to answer rather than a clean result.

- **`unreleased_is_honest` could not see an empty `[Unreleased]`**, only the literal "Nothing yet" —
  and an empty section makes the same claim. Same shape as the rest of M36 one level in: a check
  that exists, runs, and cannot fail on half the cases it is for.

### Added

- **`checks_can_still_fail` — a canary for the checking apparatus itself** (M36). It checks nothing
  about the repository; it checks that each check above still has a non-empty population to examine,
  because a check with no input reports success and the vacuous result is byte-identical to the
  healthy one. The pattern is MongoDB's canary test — one that tests the testbed rather than the
  software — paired with the rule Vitest spells `passWithNoAssertions: false`.

## [0.2.0] — 2026-08-31

**Two of D56's four contract surfaces broke, which is what makes this a minor bump rather than a
patch.** *Exit codes*: D97 moved every clap-level argument error from `2` to the documented `64`,
so a script branching on `2` changes behaviour — and `amb hook` with unparseable arguments now
exits `0` instead of `2`, which is the fix rather than a regression, since `2` is how Claude Code
blocks a session. *CLI and `--json` shapes*: the `--version` banner gained a `sqlite` field,
`doctor` gained a `sqlite` row, `install --json` gained `locked`, `lock_error` and `retries`, and
`--poll 0` / `--limit 0` are now refused where they were silently accepted. Hook entries and vault
layout are unchanged, so an older `uninstall` still recognises what this installs and an older
binary still reads every note.

> **`v0.1.0` has no tag, and cannot honestly be given one.** The repository's history was reset on
> 2026-08-31 to publish it (D100), which destroyed the tag along with the commit it pointed at.
> Tagging the current tree `v0.1.0` would label a tree containing D96–D100, schema 12 and M29–M33
> as the 2026-08-28 release. The section below is kept as the record of what 0.1.0 was; the tag is
> gone and saying so is more useful than a false one.

### Fixed

- **A gate check had switched itself off, and the operation that did it never touched the check**
  (M35). The history reset destroyed the `v0.1.0` tag; `unreleased_is_honest` began with
  `git describe --tags` and `return []` when there was no tag, so it passed unconditionally from
  that moment. `CLAUDE.md` records that all six checks here were verified by breaking them — five
  were live and one was a no-op, and the gate printed `✓ all checks passed` either way.

  Unlike D83's threshold (M34) this one **worked, was verified, and was then killed from outside**:
  no diff, no commit that introduced it, and no test that could fail. The repair removes the
  dependency rather than restoring the tag — "commits since the last release" and "commits at all"
  are the same question while no release has happened, and the second survives a `git init`.

  **The question this leaves is about dependencies, not conditions:** what repository state does a
  check need in order to be *able* to fail, and what routine operation destroys it?

- **`.github/workflows/ci.yml` must run every check `tools/verify.sh` runs, and now something says
  so** (M35). D70 records the divergence that motivated the rule — `check_secret_literals.py` was in
  the gate and not the workflow, so CI would have passed a commit the gate rejects — and then states
  that a sentence is the only thing enforcing it. `the_gate_and_ci_run_the_same_checks` enforces it
  one-directionally: extra steps in CI are expected (the matrix builds on Linux), a missing one is
  drift. Both new checks were verified by breaking them.

### Added

- **The `promote.rs` mutation pass got its confirming re-run: 40 of 40 viable mutants caught, up
  from 24 of 40** (M25). Same 47 mutants and same 7 unviable, so it is a like-for-like comparison.
  All sixteen survivors M25 found were real and the tests in `b75d150` closed every one — the
  pipeline that has still never executed once now has the only pressure it can get, and passes it.

  Conditions recorded with the number: quiet baseline (5 s build, 11 s test, timeout at its 120 s
  floor), **no TIMEOUT rows**, zero other `cargo` processes, load 6.65 → 7.76. M27's residual hole
  is that the ceiling is measured once at the start, so the run's own conditions belong beside its
  result.

- **D83's threshold can be read now, and one half of it was worse than absent** (M34, D95's rule).
  `amb doctor` gained a `size` row and `tools/eyeball.sh` times `amb inbox` over a copy of the real
  board:

  ```
  ok    size            0.5 MB of the 50 MB at which D83 builds pruning
    amb inbox   5 ms of the 5000 ms hook budget (D83), over 68 messages
  ```

  The size half had no instrument at all — `doctor` printed the board's *path*. The latency half is
  the interesting one: `bench/bench_startup.py` has timed `amb inbox` all along, which is where
  README's 3.0 ms comes from, but against an **empty scratch board** — so its number is
  structurally incapable of crossing a threshold that is about the real board growing. It would read
  ~3 ms at 50 MB and at 5 GB. An absent instrument makes the next reader look; one reporting a
  healthy number against input the condition cannot reach makes them trust.

  The footprint sums `-wal` and `-shm` as well as the main file, because in WAL mode the sidecar
  holds committed transactions the main file does not yet contain.

  An audit of every other decision naming a numeric threshold is in M34: D59, D13 and D49 are
  readable, D96 is partly, and D83 was the only real gap — which is the size the audit predicted.

- **`eyeball.sh` reported `unchanged` on a board that had changed**, because `sqlite3 -readonly`
  fails on a `.backup` copy (`unable to open database file (14)`: WAL-mode, no `-shm`, and a
  read-only connection cannot create one) and succeeds on the live board only while another session
  holds that file open. Two failed reads returned empty strings, and `[ "$after" = "$before" ]`
  compared them equal. **A comparison of two failures is indistinguishable from a match, and it
  fails in the flattering direction**: the tool claims it touched nothing exactly when it has lost
  the ability to tell. Both sides now count through a copy, and an empty snapshot is reported as
  "could not count the board" rather than as agreement.

- **A shared shape assertion for rendered output, and the fixture that makes it mean something**
  (M33). `assert_rendered_shape` asserts what held with zero violations over 274 lines of real
  output — no tabs, no blank line made of spaces, no trailing whitespace — and is wired into
  eighteen renderers plus the three-renderer enumeration in `delivery.rs`.

  **"No double space" is deliberately not in it.** It is M24's own rule and it is false as a
  universal: 50 of those 274 lines carry an interior run of spaces and every one is a deliberately
  aligned column, including in this project's own tools. It stays a per-renderer assertion where
  the output is prose.

  `quoted_block` rendered a blank line inside a body as `"> "` — trailing whitespace on 59 of the
  274 lines, on `amb inbox` and the delivery banner alike. Fixed to `">"`; the containment is the
  prefix and the space was decoration.

  **The eighteen assertions caught none of it.** With the defect reintroduced, all 490 tests
  passed: no fixture in the suite had a body with a blank line, so the branch was never reached —
  M17's shape arriving inside the guards written to close M24's. One `\n\n` in the existing
  `UNTRUSTED` enumeration fixes it, and reintroducing the defect now reddens two tests.

- **`tools/eyeball.sh` — the third source of truth finally has a script** (M32). It runs the real
  binary against a **copy** of the real board and prints what a session actually gets: `doctor`,
  both installed hooks under the payloads Claude Code sends them, `inbox`, `claims`, the receipt.
  It asserts almost nothing and is not a gate, which its first line says, the way `bench.sh` does.

  M29 closed by noting that running the binary against the real board found both M24 and M29, that
  neither was visible at any unit, and that it was the only one of the three sources of truth with
  no script. A fixture cannot substitute: a fixture is built to match the code, so drift between
  accumulated state and current code is the one thing it can never contain. The standard caution
  about production data is answered by copying — via `sqlite3 .backup` rather than `cp`, because
  the board is WAL and its `-wal` sidecar holds commits the main file does not.

  **Its first run found three defects, all of them in itself.** A `sha256` of a live WAL board is
  not a modification signal — it changed twice in three seconds with no `amb` command running,
  because another session merely *reading* updates `-shm` and can trigger a checkpoint; the
  read-only claim is now shown with logical row counts instead. A cross-artefact check that cannot
  tell amb's own voice from amb quoting a two-day-old message warns on both and gets ignored; it
  now attributes by authoring surface. And `check_docs.py` enforced "an uncited script should be
  deleted" for `bench/*.py` only — widened to `tools/`, where it immediately named a second uncited
  script. It skips untracked files, so it cannot fire on a peer session's work in progress.

- **The settings read-modify-write lost updates in both directions, measured at 46 in 540 trials**
  (D99, M31). `amb install` read `~/.claude/settings.json`, decided, and wrote ~4 ms later, with
  nothing guarding the gap. A third party's setting was destroyed in 38 of 540 trials — and **amb's
  own hooks in 8**, which is a silent stop to mail delivery. Zero files were corrupted in any
  configuration: the temp-file-plus-`rename` was already correct, and the defect was the cycle.

  **The fix is two mechanisms, because a lock alone measured well against the wrong adversary.**
  An advisory lock (`File::lock`, `std` since Rust 1.89, no new dependency) takes amb-against-amb
  to **0 of 540** and leaves an uncooperative writer at 42 — and Claude Code *is* an uncooperative
  writer, since `/config` stores `crossSessionInbound` into this same file. Compare-and-swap covers
  it by detecting rather than excluding: re-read immediately before the `rename`, restart on a
  mismatch, bounded at `MAX_RMW_ATTEMPTS`.

  Final: **10 of 540**, and the halves differ — five are amb's residual one-syscall gap, five are
  the other program's own non-atomic cycle, which nothing here can fix. Not "fixed"; 0.9% residual
  against a hostile interleaving swept at 0.1 ms, against 7% before.

  Two findings only measurement produced. The **backup copy sat between the check and the `rename`**,
  putting a whole file copy inside the window the check exists to close — moving it above halved the
  residual, and no test could have found it. And the **first two harnesses reported 0 losses in 275
  trials and were worthless**: the competing writer was a `python3` subprocess whose ~25 ms
  interpreter startup meant no race was ever created (M15/M16's shape, caught before publication
  because a negative result was disbelieved).

- **Q13 is closed on measurement rather than on argument: a message body is stored exactly as
  written** (D98, M30). `redact` was run over every message body on the board — **53 bodies,
  98.3 KB** — through the real library. It found **zero secrets and made one removal**, and the
  removal was a false positive: an agent's scratchpad path, the whole payload of its sentence, in a
  message telling a peer where their recovered work was saved.

  The same path appears three times in that body and **the two longer forms survive**, because
  `is_high_entropy` returns early on `.` — so the discriminator between kept and destroyed is a
  file extension, and the shorter, less revealing form is the one that dies. Replacement is
  whole-token, so adjacent markup goes with it.

  The case *for* redacting was strong and stayed strong: D37's three reasons all hold word for word
  for a message body. The measurement contradicted it, and this project does not get to ignore a
  flat number because the prose is persuasive (D59). `send`'s docstring now says the absence is
  deliberate, and `a_body_is_stored_verbatim_because_the_send_path_does_not_redact` asserts it for
  body and subject — an omission needs an assertion of absence (M23), and its first assertion
  proves its own premise by checking the fixture still reaches the redactor.

- **The bundled SQLite version is now reported, because it was a fifth contract surface and D56
  named four** (H3). `amb --version` ends `, sqlite 3.53.2)` and `amb doctor` carries a `sqlite`
  row. The engine storing every message and index row was invisible to every instrument here.

  It matters more than the general case of pinning a dependency. The worst SQLite defect of 2026 —
  the WAL-reset bug, present since 3.7.0 (2010) and fixed in **3.51.3** — presents as *a committed
  write that later transactions cannot see, with no error raised*, and it triggers on several
  processes writing or checkpointing one WAL file at the same instant. That is this board's exact
  topology and its exact stated failure class. The bundled build is 3.53.2 and unaffected; what was
  missing was any way to notice if that changed. `doctor` reports `BAD` below the floor and a test
  reddens on a `libsqlite3-sys` downgrade, so a regressed pin cannot be silent.

- **`--poll` and `--limit` are bounded, because zero was a busy loop and a silent empty result**
  (H4). `amb watch --poll 0` slept for nothing and re-ran `deliverable()` as fast as the process
  could issue it, for the whole timeout, against the board every session on the machine shares.
  Reachable from the `monitor` banner, which prints numbers a model will adjust. `MIN_POLL_MS` is
  50 and lives in `messages.rs` beside `watch` rather than in the binary (D78). `recall --limit 0`
  returned nothing, which is indistinguishable from a search that missed — the distinction D89
  exists to make.

  The tests assert the refusal and that the message names the bound, rather than a code — see D97,
  which settles what the code should be.

- **Fifteen tests for the four modules that had never been mutated, and one was a D11 bypass**
  (M27). `redact.rs`, `status.rs` and `note.rs` scored 79%, **57%** and 96% in one sitting, and the
  ranking tracked what each module *produces* rather than its size: a parser returns a value, a
  renderer returns a page, and a page is asserted with `contains` — which cannot see a line that
  should not be there.

  In `redact.rs` the useful question was not which mutants survived but which **leak**. Two do:
  `core`'s trim predicate, without which `SECRET_PREFIXES` stops matching any credential that
  arrived inside quotes or a trailing comma — the shape a secret has in a paste, where every
  existing fixture used the bare token; and the length floor, which runs before the prefix check
  and drops an exactly-eight-character secret. A third under-reports: `strip_pem`'s counter
  mutated to `*=` still removes the private key and leaves `removed` at zero, so `write.rs`'s
  `if w.redacted > 0` suppresses **"N value(s) redacted before writing"** entirely — a *silent*
  redaction, which that file's own comment forbids. The remaining thirteen either over-redact — the
  direction D46 chose on purpose — or are near-equivalent against this project's own vocabulary,
  and are deliberately not chased.

  In `status.rs`, **thirty-seven of forty survivors sit on the `if` that decides whether a line is
  rendered at all** — ten of them the literal `x > 0` relaxed to `x >= 0`. Under them the command D59's withdrawal is read off
  reports `! 0 note(s) … that content is gone` on a healthy vault, a phase-2 block for a phase
  that has never run, a per-force ratio of `285.00`, and `this feature … should be switched off`
  on a board where nothing was ever injected. The arithmetic was correct throughout; every one of
  the forty was in the rendering.

  In `note.rs`, one survivor: an indented line lifted to a top-level key, which makes
  `unknown_keys` report a key `parse_note` never saw — the exact lie `scan_frontmatter`'s
  docstring says one scanner exists to prevent. Unreachable through `amb`'s own writer (all 36
  notes in the real vault were checked) and reachable through the vault's premise, hand-editable
  markdown.

  The re-run scored **114 of 116 viable** against 75 before, and both remaining survivors were defects in
  the new tests. One was real: an absence assertion for a *nested* line, made on a board where the
  enclosing block never rendered, so it proved only that the outer block was missing — M17's shape
  arriving inside tests written to catch omissions. The other is equivalent given an invariant with
  a single call site, and its **premise** is now asserted instead, verified by breaking it.

  No production code changed. All four modules computed correctly throughout.

  `delivery.rs` was then run as the finding's own prediction — a renderer, never mutated, holding
  the banner every session reads — and **refuted it**, scoring 88%. Its worst survivor is not a
  rendering defect: `write_snapshot` probes `path.parent()`, which for a bare filename is
  `Some("")`, and the match guard sending that case to `Path::new(".")` had no test, because every
  snapshot fixture passes an absolute path. From a subdirectory the mutant writes a snapshot
  **inside a repository**, which is what D11 exists to prevent. The two remaining survivors are the
  `if !out.is_empty()` separator guards, three lines apart, and `hidden > 0` — the sibling of the
  `render_hidden` guard M23 fixed in `inject.rs`.

  The corrected finding: the recurring defect is not "renderers", it is a **guard over a derived
  count**, whose `n >= 0` relaxation no presence-only test can see and no empty fixture reaches.

  One reported "timeout" was re-run by hand on a quiet machine and **survives** — the full suite
  passes in 20s against a 120s floor. A timeout is not a caught mutant; on a shared machine it may
  not be about the mutant at all.

- **Five tests for the promotion pipeline's lifecycle, which had none** (M25). A 47-mutant pass
  over `promote.rs` left 16 survivors — 40% of viable, against `events.rs`'s 15% — and **fourteen
  sat in two functions**: `expire_candidates` and `ready_candidates`. The fifteen tests that
  existed all covered rendering and routing, every one of them pure, so the module was tested for
  what it prints and never for what it does. `expire_candidates` could `return Ok(0)`
  unconditionally, the TTL comparison could invert, `at - last` could become `at + last`, and
  `DAYS * 86_400` could become `DAYS + 86_400` — a thirty-day window collapsing to about one —
  with nothing red, because nothing had ever called it. Sixteen of sixteen replayed by hand and
  confirmed red; no equivalents.

- **`Receipt::arrival_note`, the state D59's verdict could not express** (D95, M24). The
  measurement window had been open ten hours and collected nothing while sixteen sessions were
  active — not slow, structurally excluded. `note_events` is keyed
  `(session, kind, scope, slug, event)`, so a session injected before the window opened writes no
  row when re-injected, and no new session had started on this machine in two days. `too early —
  needs 30 more session(s)` prints identically whether a floor is approaching or unapproachable,
  which meant **the injection layer had no live withdrawal condition, only one that looked live**.
  The new line sits above the verdict and says at zero that the floor is unreachable rather than
  unreached. The floor was *not* lowered and the denominator was *not* switched to injections —
  injections inherit the same key, so that relocates the problem while appearing to solve it.

- **Fourteen tests across `events.rs`, `inject.rs`, `export.rs` and `capture.rs`**, one per
  surviving mutant from a 110-mutant pass over the two modules that produce the numbers D59 will
  retire or keep the injection layer on (M23). Sixteen survivors, **fifteen replayed by hand and
  confirmed red before the test was kept**; the sixteenth is equivalent and stays. Four came from
  one fixture zeroing the `PreToolUse` lane, so `x + <file field>` and `x - <file field>` agreed
  everywhere — the same omission D42 had to fix in production. Others: `crossed_note` could be
  replaced with an empty string (D91's fix was never pinned), a global note could be captioned
  "other project, advisory", and every injection could end "…and 0 more".

- **`render_export` and the placeholder guard, asserted for the first time.** `render_export`
  writes into a user's repository and `--check` compares content hashes, so its bytes are a
  contract; its docstring promised testability that nothing had collected on. The guard refusing
  to publish an unsubstituted `{{...}}` or `TODO(` had **zero assertions at any layer** — the
  literal `{{` occurs three times in the whole tree, and deleting the thing that stops a published
  file looking finished reddened nothing.

- **`db::location_verdict`**, the whole sync-root/remote-volume decision with the syscall taken
  out — and five tests, three of which fail mutations that previously survived the entire suite
  (M22). `is_remote_volume` had been extracted for exactly this reason and covered only half the
  rule: the *wiring* stayed in the shell, where a test could reach it only by mounting a share.
  Making `statfs` always fail, which disables the remote guard outright, reddened nothing in 430
  tests; so did replacing `mnt_local` with `None`, which drops macOS's `MNT_LOCAL` authority and
  hands the decision to a ten-name list that will never contain `webdav`. The suite noticed only
  when the guard was made to refuse *everything* — every other test needs a board to open, which
  is a canary rather than an assertion.

- **Nine render functions out of `run_memory`** (D92's move, continued): `render_written`,
  `render_derived`, `render_candidates`, `render_recall`, `render_history`, `render_index`,
  `render_export_check`, `render_window_report` and `render_window_change`. `src/main.rs` loses
  142 lines and `src/memory/write.rs` and `src/memory/export.rs` gain their first tests — `write`
  holds `observe`, the only path that authors a file, and had none. **Twenty tests, each verified
  by breaking the rule it guards**: sixteen hand-applied mutations, sixteen reds, listed in M21.
  What the arms were hiding is not formatting but decisions — that a redaction is announced only
  when one happened, that a derivation which did not count says why, that a near-match is offered
  *after* the note is written and never before, that `AlreadyOpen` cannot read like `Opened`, and
  that `export --check`'s text and its exit code read the same predicate rather than two.

- **`a_global_broadcast_crosses_projects_under_contention_and_a_project_one_does_not`** (M20),
  the first assertion anywhere that project scoping holds *through the shipped binary*. Deleting
  `m.to_proj = ?1` from the central predicate reddened exactly two tests before it — one unit, one
  against the library — while all 137 tests that spawn the executable stayed green. Nothing was
  broken; D17's central claim was one `main.rs` wiring mistake away from silently untested. Three
  readers in three projects, twelve concurrent senders, expected counts 9/7/4 kept unequal so no
  single wrong answer can satisfy two assertions.

- **`tools/install.sh`** (D94), which builds and updates *every* copy of the binary — `PATH` plus
  every path an installed hook actually invokes, read out of `settings.json` rather than hardcoded
  — then runs `doctor`. **Use it instead of `cargo install`.** The stale-hook-binary condition has
  occurred five times; D73 shipped detection and it fired again within minutes of the next commit.
  Detecting a failure that recurs on every commit is not the same as closing it.
- **`tools/mutants.sh`**, so the three flags mutation testing needs here are not a thing to
  remember. It forces a private `CARGO_TARGET_DIR` — a result produced while anything else was
  building is void, not weak, and the polluted run reported a *caught* mutant as missed (M17) —
  passes `--copy-vcs true` so `build.rs` can fingerprint the repository, and pins `--jobs 1`.
  Its header states why `--diff` mode is offered but wired into nothing: cargo-mutants matches the
  diff against the code under test and **not the test code**, so a commit deleting a test
  generates no mutants and passes green. Blind to the one change it exists to catch.

- **`bench/_harness.py`**, where the synthetic-vault note format now lives exactly once, beside
  the `index_or_die()` that makes the next schema change loud. It lived in two files before, in
  neither of the two places that would have caught D81 renaming the key (M18).

- **`tools/bench.sh`**, one entry point with an exit code for the four measurement harnesses. Its
  first line states that it verifies execution and coverage and asserts nothing about values,
  because a harness check and a performance gate look identical from outside and this machine
  cannot honestly run the second. Deliberately **not** in `verify.sh`: ~17s against a gate that
  runs before every commit.
- **`check_docs.py` refuses an uncited harness.** The citation is the promise a reader relies on;
  a script in `bench/` that no document names should be deleted rather than kept.


- **A ledger of searches, so a miss is distinguishable from an absence** (D89). **Schema 11 → 12.**
  `note_events` recorded `injected`, `injected_file` and `cited` and nothing recorded that recall
  ran, so `unprompted: 0` — a citation of a note the session was never shown — meant *either* "no
  session wanted one" *or* "sessions asked and the search lost the answer". D59 retires the
  injection layer partly on the first of those. The new `searches` table is keyed
  `INTEGER PRIMARY KEY`, which deduplicates nothing: the obvious cheaper move, a sentinel row in
  `note_events`, inherits `PRIMARY KEY (session, kind, scope, slug, event)` and would have recorded
  five searches in one session as one row — a denominator counting distinct things rather than
  times the cost was paid. No query text is stored; `lane` and `hits` answer both questions the
  receipt asks.
- **`amb memory status --json` names its window and its searches.** The human path printed
  `counting over …` while the JSON path returned before that string was built, so the surface most
  likely to be read by a machine emitted a ratio with no window attached.


- **A measurement window with a start the tool can read** (D87). **Schema 10 → 11.**
  `amb memory window --open` records when D59's clock started; `amb memory status` counts from
  there by default and says which corpus it counted (`counting over the window opened 3h ago`).
  `--all-time` and `--days N` override it. Before this, D59's withdrawal condition and D79's start
  date existed only as prose: the sole window control was `--days N`, an integer count back from
  *now*, so no fixed instant was expressible and the default was all time. The printed ratio was
  therefore computed over events D79 had excluded — including `probe-drop`, a hand-run session
  that wrote 8 injections in one instant, could never cite anything, and was 14% of the
  denominator. Reopening is deliberate (`--reopen`) and says what it discards; repeating `--open`
  refuses rather than silently restarting a measurement.

- **D96, M29 and Q13 — the delivery back-off rotates the inbox rather than draining it.** Found by
  reading the banner a live session was actually handed: 15 unread messages, all broadcasts, aged
  2–3 days, **all superseded** — two announcing schema 9 and 10 against a board at schema 12, three
  announcing D-number ranges against a record at D95.

  `MAX_OFFERS` bounds offers per message and `MAX_RENDERED` bounds messages per offer; **neither
  bounds the product**, because the cohorts run in sequence, so injections scale linearly with the
  backlog (`10 × ceil(N/10) ≈ N`). Against D24's own worked example the aggregate is unchanged —
  the cap redistributed the cost rather than reducing it. Not a refutation of D24: peak matters
  independently of total, and the cap fixed the failure that wrecks a small window.

  **D96** gives broadcasts a 24-hour delivery horizon (`AMB_BROADCAST_HORIZON`), on the delivery
  path only; `inbox` still shows everything and direct mail never expires. It states plainly that
  it weakens D17, which is the argument against it.

  The guard it ships is the point. `a_project_broadcast_reaches_an_agent_that_registered_afterwards`
  — D17's own test — builds its fixture on a fresh board and reads through `inbox`, so **adding the
  horizon leaves it green**: a guard staying green when you change the rule it names, D51's shape,
  sitting on the project's central claim. The new test asserts the *split* instead — gone from
  `deliverable`, present in `inbox`, direct mail delivered either way — because each half alone is
  satisfiable by a wrong implementation.

  **Q13** files the message-redaction asymmetry
  as genuinely undecided rather than as a decision, because the trade has a real cost both ways and
  the corpus to settle it already exists on the board.

  D93 gains an in-place amendment: Claude Code's **channels** are the documented push path D93 said
  would change its answer, and the verdict is unchanged because the reference states D93's own
  disqualifying property as a specification — *"drops the events silently and returns no error to
  your server."* A trip-wire that can actually fire is recorded, per D95.

### Fixed

- **Two doc comments that documented the wrong thing, one by binding and one by claim** (M30).
  Both were found by a task that required editing them; no instrument here was pointed at either.

  `send` had **no docstring**. `MAX_BODY` had been inserted between the doc block and the function,
  with no blank line, so the two comments merged — `MAX_BODY` documented itself as *"Send a message.
  Returns its id."* and the `BEGIN IMMEDIATE` concurrency evidence was filed under a size limit.
  Every sentence was true and in the file; only the binding was wrong, which is why nothing caught
  it: rustdoc renders it, no lint fires, and the item *is* documented — just not the intended one.

  `is_high_entropy` claimed a path without an extension *"is lowercase and fails the mixed-case test
  anyway"*. It does not: a macOS scratchpad path carries capitals from `-Users-…-Projects-…` and
  digits from a session UUID, which is exactly the false positive above. The filter runs on the
  vault today (D37) so the defect is live there — the vault is currently clean, 46 notes and zero
  redaction markers. The paragraph is corrected against the measurement and
  `a_deep_path_is_redacted_which_is_a_known_false_positive` pins both halves so it cannot rot back.

- **Documentation currency, including four version banners quoting a schema four migrations old.**
  `check_docs.py` was green throughout — it verifies structure, the test count and the `D1–Dn`
  range, and none of these were any of those. The README's install sample, its `doctor` sample and
  its versioning section all printed `schema 8` against a board at 12 and predate the `sqlite` field
  entirely; the `doctor` sample was also missing its new row.

  The substantive half is prose D96 made false. `D17`, `D27` and `DESIGN.md` each stated the
  durability claim unqualified — *"waits for whoever works there next"*, *"whenever they arrive"*,
  *"reach a recipient who is not running: yes, it is a log"* — and each now carries the bound where
  a reader meets the claim, rather than only in D96 where they would not look. That is this
  project's own recurring failure: a decision stated too strongly gets defended long after the code
  stopped honouring it, and `check_docs.py` explicitly cannot see it.

  D96 also gains its fifth rejected alternative: expiring on a **checkable claim** rather than on
  age. It is the option the next reader will think of — every message in the observed backlog
  asserted something about a counter this machine can read — and it is rejected because the blunt
  horizon already caught 100% of them, because an opt-in flag makes its own denominator
  unreachable (D58, D91), and because nothing yet says 24 hours is wrong. The condition that would
  reopen it is written down.

- **A malformed hook invocation exited 2, and exit 2 is how you block a session** (D97). `main`
  now uses `try_parse`: `--help` and `--version` exit 0, a hook whose arguments this build cannot
  parse exits **0 silently**, and every other argument error exits **64** — the code `error.rs`
  already documented.

  Two defects, and the second is the serious one. `error::exit` says distinct codes exist "so a
  hook can react without parsing stderr", but that covered only errors the *library* raises; clap
  terminated the process first with its own default of `2`, so the **commonest** usage errors — a
  mistyped flag, a missing option, an unknown subcommand — all exited outside amb's documented set.

  And `2` is the code Claude Code reads as **blocking**: on `Stop` it *"prevents Claude from
  stopping; continues the conversation."* `hook_main` honours D9 absolutely — exit 0 for hostile
  stdin, a corrupt board, no identity, an unreadable vault, each one tested — and **none of it
  ran**, because clap exits during parsing, upstream of every line where the guarantee is written.
  A hook entry written by one build and invoked by another (D69, D94 record this five times) would
  have wedged the session rather than failing quietly.

  `tests/hook_safety.rs` could not see it: all twenty of its tests drive `hook <mode>` correctly
  and then break something at runtime. M20's arithmetic again — the unasserted layer was the
  outermost one. Also: `invoked_as_hook` reads the *first* positional, a rule that lived only in
  its docstring until mutating it to `args_os().any(…)` survived every new test, so `send --body
  hook` now pins it.

- **The gate's published cost named a cache state that spans a 3x range** (M28). `tools/verify.sh`,
  `README.md` and D70 all said **16.9s warm**, and on 2026-08-31 that was reproducible in neither
  state the word covers: **~10s** when nothing has changed since the last run (9.6 / 9.8 / 9.9 /
  12.1) and **29-31s** after touching a source file. Which one it had measured is settled by its own
  next clause — *"dominated by clippy and the suite"* — which is false in the no-op state, where
  clippy is a **0.13s cache hit** and the two audit scripts are 71% of the run. So the comparable
  figure had grown to ~30s as the suite went 376 to 473 tests, and the measurement was never wrong,
  only the word standing in for its method. A cost claim now names its state, because this one fails
  in *both* directions: against the cheap state 16.9s reads as padded, against the expensive one as
  a regression someone shipped.

- **`tools/mutants.sh` printed a usage text that stopped mid-sentence** (M28). It extracted its own
  header with a fixed `sed -n '2,32p'`, and the twelve lines added to that header earlier in the
  same session pushed the block to line 34 — so the paragraph explaining what a mutation score *is*
  good for lost its final clause. Nothing failed: it still exited 64 and still looked like a usage
  message. M24's defect in a second artefact, reached by a hardcoded offset instead of a wrapped
  literal. The range is now derived — print the leading comment block, stop at the first line that
  is not one — so it cannot fall out of step again; verified by inserting a sentinel line and
  confirming both it and the closing sentence render. The other three scripts in `tools/` were
  checked for the same shape and none has it.

- **`tools/verify.sh` now measures its own cost instead of asserting it** (M28). Every run ends
  `gate: Ns`, on the failing path as well as the passing one. The previous fix had replaced one
  rotting literal with two, in the same commit that taught `mutants.sh` to derive rather than
  hardcode, and M28's conclusion — that a gate's cost "cannot be derived, it has to be measured" —
  was wrong: the script is already running when the question is asked. That conclusion is corrected
  in place rather than deleted, because what made the exception persuasive will be persuasive again.

- **A mutation score quoted against the mutants that do not compile.** The re-run of `status.rs` and
  `note.rs` was recorded here as `114 of 121` while `MEASUREMENTS.md` recorded the same measurement
  as `114/116 viable`. 121 is the count generated; five never compiled and no test could kill them.
  Question 1 of the ratio rule against this project's own document.

- **`README.md` did not name `tools/mutants.sh`**, though it lists the other two non-gate scripts
  and mutation testing is what five of the last six measurements were. `verify.sh`'s claim to run
  "both audit scripts" was checked against the script and is true.

- **`arrival_note` reached a person and never a machine** (M26). D95 put the line that says D59's
  floor is unreachable on the human surface only, so `amb memory status --json` emitted
  `verdict: "too_early"` beside `sessions: 0` with nothing saying the window cannot fill — D87's
  defect, on the other half of the same command, committed by the session that had just read D87.
  Its neighbour was no better: deleting `"lane_caveat"` from `Receipt::to_json` reddened nothing in
  457 tests, so D74's caveat could vanish from the machine surface silently. `Receipt::to_json` now
  takes the window, so a surface cannot emit one caveat and omit the other by construction, and one
  test enumerates both caveats against both surfaces.

- **`tools/mutants.sh` had a fixed `--timeout 180`, which cargo-mutants applies to the baseline
  too.** The suite runs ~3s normally and ~145–192s under mutants — every e2e test spawns the
  binary, and a spawn in the sandbox costs orders more — so 81% of the margin was consumed before
  anyone added a test. Adding one that spawns twelve processes crossed it and the run reported
  `TIMEOUT Unmutated baseline` having tested **nothing**. Loud, but only to a reader: a fixed
  ceiling under a growing suite fails on whichever commit happens to cross it. Now
  `--timeout-multiplier 3 --minimum-test-timeout 120`, which scales with the measured baseline.

- **44 surviving mutants across `claims.rs`, `doctor.rs` and `identity.rs`** (M19). The same
  instrument as M17, aimed by a cheap prior — an inventory of every `to_json` separating *computed*
  keys from plain field reads, since a field copy needs no assertion and a computed value is a
  decision. 204 mutants, 184 viable, **44 survived**. `claims::take` could compute expiry as
  `at * ttl` instead of `at + ttl`, putting it ~3000 years out so **no claim ever lapses** — and
  claims are advisory, so nothing fails. `identity::session_pid` could return `Some(0)` or
  `Some(-1)`, either of which reports **every peer permanently alive**, reintroducing by constant
  the liveness oracle D21 was written to remove. `doctor::Report::to_json` could return an empty
  value, and `doctor --json` is what a script reads to decide whether this machine is healthy.

  `pid_from_socket` was extracted out of `session_pid` so the rule D93's addressing half rests on
  is reachable by a test at all — and the extraction guarded the *rule* while leaving the shell as
  unguarded as before, because every wrong answer there degrades to recency and in a test
  everything is recent. Only a dead pid discriminates, which is now an e2e test. `claims.rs`
  gained its first connection fixture: every database path in it had been exercised only at
  process level. **Re-run: 186 of 193, from 140 of 184**, with three further closures hand-verified
  since — `resolve`'s blank-`AMB_PROJECT` guard, `collisions`' per-project grouping, and the
  liveness window's value, which survived because every assertion around it was written in terms
  of the constant itself.

- **Both memory harnesses had been measuring an empty vault for a day** (M18). D81 renamed the
  `project:` frontmatter key to `scope:` and removed the fallback — correct for the vault, which
  is regenerable, and not a claim about *fixtures*. `bench_memory.py` and `bench_attribution.py`
  each wrote the old key from its own private copy of the format, so `parse_note` rejected all
  1000 synthetic notes. Both printed full tables and exited 0; `bench_attribution.py`'s three rows
  were one measurement printed three times, which is fatal to an experiment whose whole design is
  "two vaults differing only in how many notes concern the queried path".

  **The diagnosis existed and was discarded.** `memory index` printed `1000 scanned · 0 indexed`
  and named the offending key on every note; both callers passed `capture_output=True`. A silence
  is not always the absence of a signal — sometimes it is a caller throwing one away.

  Repaired: the note format now lives once, in `bench/_harness.py`, beside an `index_or_die()`
  that exits 1 with the indexer's own words unless every note lands, and `bench_attribution.py`
  additionally asserts its **independent variable** — that the hit path injects and the miss path
  does not. Verified by mutation, both directions. Every M9 timing row reproduces within noise;
  the token rows did **not**, which is the first thing the repaired instrument found — see below.

- **`tools/bench.sh`'s own first line was false for half the scripts it runs** (M18). It claimed
  "each one fails loudly when it stops measuring what a document says it measures"; two of the
  four had no guard of any kind. A script written to enforce *"an unverified measurement script is
  a false comment with a shebang"* was itself one. All four now guard.

- **`bench_startup.py`'s docstring still said "Add the real binary once it exists"** — the
  instruction M15 had already carried out, in the file M15 repaired, still naming the
  `./target/release/amb` path M15 had already established does not exist on this machine.

- **Five silences in `messages.rs`, found by mutation-testing the module that holds `select()`**
  (M17). 80 mutants, 72 viable, **12 survived** — and none was a near miss; each was a rule no test
  touched. `undelivered` could return `Ok(vec![])`, deleting D25's mid-turn lane so mail merely
  arrives at the next `Stop`. All three `watch` survivors collapse to *returns nothing,
  immediately, forever* — on the command the `SessionStart` banner names for immediate delivery.
  `Message::is_broadcast` and `is_global` — how `--json` reports which of the four addressing modes
  a message used — were read only by `to_json` and asserted nowhere. `distance`'s first column
  could drop its `+ 1`, widening what `nearest` counts as a near miss. And the tie guard behind
  D26 was never evaluated by the test whose comment claims it, because the second candidate was
  outside the budget and the match reached the one-candidate arm.
  **Four tests added and one assertion repaired; the re-run catches 72 of 72.**

- **`bench/bench_queue.py` builds the schema it says it builds** (M16). Its `messages.to_proj` was
  `NOT NULL`, which makes the global broadcast (`to_agent IS NULL AND to_proj IS NULL`)
  *unrepresentable* — so one cell of the 2×2 D17 calls this design's central claim was absent from
  every number published from that harness. It also carried one index where the board has two, and
  read the inbox with `SELECT m.id … LIMIT 50` against a shipped query that joins `agents`,
  projects every column and has no LIMIT. **The verdict holds and the number does not**: tuned
  saturation reproduces at 3,948 / 4,114 / 4,694 msg/s against a published 8,304 — which itself
  reproduced at 8,152 / 8,489 / 9,741 on the old harness, so it was honest about a different
  thing. Zero `SQLITE_BUSY` and zero lost throughout. Throughput there is sends over a
  send-then-read *loop*, not write capacity; the four sites citing it as capacity now cite the
  busy count and the send latency instead. The run exits 1 if any addressing mode saw no messages
  or the inbox query returned no rows — coverage, never a value.

- **`bench/bench_startup.py` measures the thing two documents cite it for** (M15). Its `amb`
  candidate was commented out behind `# Uncomment once built:` — for as long as the binary had
  existed — and pointed at `./target/release/amb`, which does not exist on a machine sharing one
  cargo target directory. There was no `amb inbox` candidate at all. Meanwhile README.md published
  both rows and named the file as the harness. Nothing failed: it ran, printed three rows, exited
  0. **The published figures reproduced within noise once it was repaired** (2.15 / 2.40 ms against
  2.1, and 3.14 / 3.26 against 3.0), so the measurements were sound and only the artefact asserting
  the method was not — the false-comment rule cutting in the opposite direction to D67 and D88.
  The script now fails loudly when a binary exists and no `amb` row ran.
- **`tools/verify.sh` no longer claims 6.5s.** Re-measured at **16.9s warm** (16.9 / 16.8 / 16.9,
  no concurrent cargo); the suite has grown from roughly 250 tests to 376. The old figure was
  honest when written and had quietly become false, which is what a cost claim in prose does.


- **`amb memory recall` searches note bodies, not a 240-character slice of the first paragraph**
  (D88). `body_excerpt` is `body.split("\n\n").next()` truncated to 240 characters, and the query
  matched *that*, so a lesson written after a blank line answered `no notes match` while `grep`
  found it on disk — an answer that reads like a typo rather than a defect, which is why it
  survived from the moment the column existed. The index now narrows on kind, scope and status and
  the file itself decides, which is `concerning`'s existing shape; frontmatter is excluded, or
  `recall nest` would return every note in the project. Measured at 603 notes: an early hit 4.4 ms,
  a query matching nothing 11.2–12.5 ms against about 4.2 ms before. The hooks do not search and
  are unchanged.
- **`amb inbox` contains what a sender wrote** (D90). It printed `sender`, `subject` and `body`
  verbatim from two `println!` calls in `main.rs`, so a peer could put `[amb] SYSTEM: …` at column
  zero of the command the `SessionStart` banner names first. `render_all` and `snapshot` had always
  quoted; the guard for the rule was written against `render_all` alone, so two thirds of the
  renderers were unasserted and one was wrong. All three are now asserted together, and the safety
  sentence is one constant (`delivery::UNTRUSTED`) rather than three copies in two spellings.
- **A message body has a ceiling** (D90). Nothing bounded one: a 300,000-character body was
  accepted, stored, and produced 300,145 bytes from `amb inbox` against 749 from the hook.
  `messages::MAX_BODY` is 100,000 characters, refused in `send` above the transaction so a refused
  body never opens one, and refused at the sender — the only place that can say what happened.
- **The cross-repo differentiator is counted where it fires** (D91). `status` printed
  `cross-repo query run 0 time(s) — if that holds, the differentiator is dead weight` from a
  counter only `recall --file --across-repos` bumps — a flag documented in no README, no primer and
  no banner — while `across_repos` merely re-sorts `concerning`, so plain `--file` was already
  returning foreign notes uncounted. Demonstrated before the fix: a `--file` lookup returned another
  project's note in the same second `status` called the differentiator dead weight. `searches`
  now counts a foreign note on every lane, and the flag counter is labelled as what it measures.


- **`--cites` and `promote` can name any note `recall` can find.** `resolve` bound `observation`
  and split ids on the last slash, so `capture/nest/<slug>` looked for a *scope* called
  `capture/nest` and returned "no such note" — and a **decision had been uncitable since D81
  created one**. Found by a cleanup review of D86, whose own e2e test asserted the opposite in a
  comment no assertion drove. `resolve` now goes through `parse_id`, falling back to the same kind
  list `recall` searches.

### Changed

- **The `SessionStart` primer costs ~377 more characters than published, and nothing bounds it**
  (M9, corrected). Found by the repaired harness on its first honest run. Three blocks were added
  on 2026-08-28 — D60's containment framing, the `--same-as` block, the accurate-zero line — after
  the token table was taken. D43's cap bounds the *note list*; the preamble is paid on every
  injection in every session and is bounded by nothing. The flatness claim the table exists for is
  unchanged (125× the vault for 4% more context); the comparison to the plan's reference points
  moves from one fourteenth–twentieth to **one eleventh–fifteenth**.

- **`promote`, `capture` and `coverage` render in the library** (D92, continued). `run_memory` is
  710 → 584 lines. What moved is what was being *decided*: `render_coverage` (an unmeasured
  project is not a zero-coverage one; the cross-project line only when true; the truncation
  announces itself), `render_offer` — the text that **is** D49's human gate — and
  `capture_session` / `capture_title` / `worth_capturing`. The finding was that
  `capture_turns_a_transcript_into_an_observation_with_no_model` asserted nothing about the
  observation: flipping that arm's kind to `capture`, inverting D86's line, left every assertion
  green. The rule was carried by the test's *name*.

- **Push delivery over Claude Code's session socket was spiked and rejected** (D93). The
  addressing half is real: the socket is `/tmp/cc-socks/<pid>.sock`, the file name is the session
  pid, and `amb` already stores that pid for liveness — a live peer is addressable with no new
  schema and no daemon. But the payload format is undocumented, and a spike against this session's
  own socket found that **the channel never answers**: nine payload shapes delivered nothing, and
  an empty object and a deliberately invalid `type` drew the same silence as a plausible message.
  A sender cannot distinguish delivered from dropped, so no `amb doctor` check is constructible
  and the failure mode would be invisible from both ends. Not built, opt-in included; D93 records
  the mechanism and the trigger for revisiting.


- **`amb memory status` renders in the library** (D92). The arm was 190 lines of `println!` in
  `main.rs`, the file with no tests, printing the receipt D59 retires the injection layer on.
  Three decisions — D74, D87 and the hook caveat — each state an *ordering* rule in a comment
  ("above the numbers, because a caveat printed underneath a ratio is read after the ratio has
  been believed") and none of them could assert it. `memory::render_status` is pure and those
  four orderings are now guarded; moving the `counting over …` line below the ratio reddens.
  `run_memory` 881 → 709 lines, `main.rs` 2,083 → 1,906 — a dent, not a fix, and D92 says which
  arm to take next and why.


- **Machine-written failure notes are their own kind and are never injected** (D86).
  `PostToolUseFailure` now writes `kind = capture` into `captures/<project>/`. The vault was
  **38.5% `bash-failed-*` notes** — title `"Bash failed"`, body raw tool output with ANSI escapes,
  and otherwise indistinguishable from a curated observation to the injection query. Six of the
  eight notes in one real `SessionStart` block were these. They cannot be cited, so they inflated
  the denominator of the ratio D59 retires the injection layer on and could never touch its
  numerator. Captures stay indexed, searchable and addressable (`capture/<project>/<slug>`); they
  are simply never put in front of a session. Adding `CAPTURE` to `INJECTABLE` reddens two
  independent guards — when D51 recorded the same mistake with `candidate`, the equivalent
  mutation reddened nothing.

- **`amb memory recall` searches every kind except `candidate`.** It read `kind = 'observation'`,
  so a *decision* had never been findable by recall and nobody had decided that. Found while
  arguing that captures should stay searchable — the argument was false as written. `SEARCHABLE`
  is now a named axis with a partition assert, so a new kind cannot be silently unfindable.

- **A quality gate that actually runs** (D70). `tools/verify.sh` runs `fmt --check`, clippy under
  `-D warnings`, the test suite and both audit scripts in one command — 6.5 s warm — and
  `.githooks/pre-commit` invokes it. Enable with `git config core.hooksPath .githooks`; bypass a
  single commit with `AMB_VERIFY_SKIP=1`, which announces itself rather than passing quietly. The
  audit that produced this recommended a GitHub Actions workflow; **this repository has no git
  remote**, so that file would never have executed once while looking exactly like coverage. It is
  committed regardless, with a first line saying it has never run, so that adding a remote turns CI
  on instead of starting a design task.

- **A test that shells out to `git` now clears git's own environment** (D71). Found by the gate
  above, four minutes after it was installed, against itself. Git exports `GIT_INDEX_FILE`,
  `GIT_DIR` and friends into every hook, and a child `git` inherits them — so `identity_e2e`'s
  worktree test operated on the *committing* repository and failed with `.git/index: index file
  open failed: Not a directory`, but only inside `git commit`, and never on a direct run. Six
  controlled full-suite runs found nothing, and a peer committing in the same window supplied a
  confident and completely wrong concurrency explanation, which D70 now records as a correction.
  `common::GIT_ENV` clears them, and a new test asserts the removals through `Command::get_envs`
  so the guard is not itself invisible.

- **The board now actually refuses a network mount** (D72), which D15 has claimed since it was
  written. `guard_location` matched five folder names as substrings and asked the filesystem
  nothing, so an SMB or NFS board opened silently — the one case D15 names as fatal was the one it
  could not detect. `statfs(2)` now decides: macOS's `MNT_LOCAL` outranks any reassuring type name,
  Linux falls back to a short list that deliberately excludes FUSE, and a failed syscall means *no
  answer* rather than *remote*. Both guards are asked of the **resolved** path, closing the symlink
  hole the previous comment created by declining to canonicalise.

- **`amb doctor`** (D73). This project's longest-running failure is operational: `cargo install`
  writes `~/.cargo/bin/amb`, the hooks invoke whatever path they were installed with, and after a
  schema change manual commands work perfectly while every hook on the machine fails silently —
  observed four times. D56 built the fingerprint that makes the comparison possible; nothing
  performed it. `doctor` compares every hook's binary against the running build by fingerprint,
  reports board schema and location, whether the memory hooks are installed, and — the condition
  nothing else covers — **when each injection lane last actually fired**. Found a stale hook binary
  on its first run.

- **The two retrieval lanes now report the exposure they actually had** (D74). `amb memory status`
  printed `by path 0/8 · 0.00` beside `by recency 4/29 · 0.14`, which reads as path anchoring
  losing badly. All eight path events came from **one** session and all 29 recency events from
  three: `PreToolUse` fires only on a Read/Edit/Write tool call, so a session that reads files
  through `Bash` raises one denominator and not the other. Each lane now carries its session count
  and `Receipt::lane_caveat` states the asymmetry when it exists — on both surfaces, not just the
  text one. Two doc comments that called this "the first real evidence" and "the retrieval
  comparison" are corrected in place.

- **`amb register` can take a name from a session that has ended** (D75). Nothing ever reaped the
  roster, so every name a session had used was consumed permanently and the next one was told
  "already taken" with no hint the holder was a corpse. Only an *explicit* name reclaims, only from
  a holder `is_alive` says is gone — and "unknown" counts as alive, because wrongly refusing a name
  costs a suffix while wrongly taking one costs a live session its identity. The displaced session
  is renamed to its auto-name rather than deleted, so both stay on the roster and its old mail
  relabels itself; `amb register` reports the reclamation on both surfaces.

- **The vault's storage niche is the complement of what the platform declines to keep** (D76).
  Auto memory ships on by default and would ordinarily retire a hand-built alternative; its own
  documentation says it *"skips anything it can derive from the codebase, such as architecture,
  file paths, or debugging fixes"* — which is precisely what the vault is made of. Checked on this
  machine with both running against this repository for a day: 2 notes against 19, **zero
  overlap**, one directory entirely working preferences and the other entirely codebase mechanics.
  Recorded for storage only. The retrieval claim — whether path anchoring beats recency — stays
  pending on D59's floor, because D74 has just established that the number which would support it
  is not yet interpretable.

- **The memory hooks had two definitions; the machine-wide install now owns them** (D77). They were
  registered in both `~/.claude/settings.json` and this repository's `.claude/settings.local.json`,
  and Claude Code merges hook sources. `note_events` is `PRIMARY KEY (session, kind, project, slug,
  event)`, so a note injected twice into one session records **one** row — the second injection
  would have spent a second block of context and incremented nothing, leaving the ratio unchanged
  and the token cost doubled. Invisible to D59, and invisible in the flattering direction. Fixed
  before measuring rather than after. D59's verdict window is now bounded: **starts 2026-08-28
  17:22:44** (the install's mtime, not D69's commit — hooks bind at session start, so every session
  predating it is outside the measurement), **bound 2026-09-11**, and it has not opened until one
  `note_event` exists later than that timestamp.

- **That window opened on a compaction, and is being restarted deliberately** (D79). `SessionStart`
  fires on `/compact`, which D77 did not consider — so the event it was waiting for was produced at
  **21:43:06** by the session doing the waiting. Restarting it forfeits forty minutes of a
  fourteen-day measurement, against building the thing being measured, so D80–D82 went first. The
  new start is the first **`injected`** event after D82 lands: a `cited` row is not exposure, and
  dating a measurement from one an implementer generated by hand would put their own bookkeeping
  inside it. The same correction records that the recency lane fires once per session **plus once
  per compaction**, which nothing had counted.

  It also found that Q10's second experimental arm had been running for eleven hours while Q10
  asserted it could not be: `greenfield-api` sets `AMB_VAULT` to the same vault and has four notes
  in it.

- **The hook-path decisions moved into the library** (D78). `main.rs` is documented as holding no
  logic; four things contradicted that, all on a hook path, none unit-tested, in a file with no
  tests — D45's declined-rebuild guard, D19's renew-suppression, the 600-character failure cap, and
  three separate copies of the `tool_name`/`file_path` extraction. Now `memory::index_is_behind`,
  `claims::conflicts_to_report`, `memory::failure_note` and `hooks::tool_and_file`, each tested and
  mutation-verified. The functions still sequence I/O in the binary, which is what the shell is
  for. Per D77's protocol: this touched the injection path, the window had not opened so nothing
  was invalidated, and both injections are byte-identical before and after once note age is
  normalised.

- **`src/memory.rs` became a module directory** (D80). 5,883 lines in one file, split along the
  banner comments it already carried into fourteen modules of 200–900 lines, and every test moved
  to sit beside the code it tests. The justification is a recorded failure rather than taste: the
  vault holds a note from a session that appended production code *after* the test module and moved
  the boundary without noticing. Behaviour-preserving, and checked rather than asserted — five
  outputs captured through the real binary before and after, all five byte-identical, and again
  after `cargo fmt`. 336 tests before and after, 185 of them in the library with no duplicate
  registrations. One test moved out of `memory` entirely, because its subject was
  `hooks::memory_hooks`.

### Changed

- **A note's scope is its own axis; `kind` means one thing again** (D81). **Schema 8 → 9.** `kind`
  is now `observation` / `candidate` / `decision`, and where a note applies is `address::Scope` —
  `nest`, `#rust`, or `@@` — on its own column. The `pattern` kind is gone: a pattern was always a
  decision that applied everywhere, and the encoding survived exactly two scopes. Ids gain the two
  forms that were previously unsayable, `decision/#rust/x` and `decision/@@/x`.

  The evidence was already in the code: two closures both named `scope`, one producing a sort rank
  and one a caption, **with the match arms in opposite order** — agreeing only because a pattern
  always carried the empty project. Both now call `Nearness::of`.

  `#rust` parses as a scope and is refused as a message destination by name, so the vocabulary is
  shared and the transport is not. `parse_scope` refuses a project id that reads as another scope,
  which is what makes one stored column safe against `AMB_PROJECT='@@'`.

  **Migrating.** The derived tables are dropped and rebuilt from the vault (D34 makes that free);
  `note_events` is carried across row by row because it is the ledger D59 reads. The frontmatter
  key `project:` became `scope:` with **no fallback** — `amb memory index` reports `0 indexed` and
  names the key rather than failing quietly. Both hook injections are byte-identical before and
  after; the only changed output is `recall --json`, where `project` became `scope`. The export
  opt-out `scope: private` is now `visibility: private`, which is what it always meant.

- **`tools/find_unread_fields.py` walks `src/` recursively.** D80 turned `src/memory.rs` into a
  directory and the flat `glob` stopped seeing it — 161 fields to 55, still reporting "every one is
  read somewhere". The file count is now printed beside the field count so a future narrowing has
  to say so.

- **Topics, and the promotion router's middle rung** (D82). A repository's topics are **detected**
  from files at its root — `Cargo.toml` means `#rust` — and notes scoped to a topic reach the
  repositories that are in it. The router gains the rung it was missing: one project routes to that
  project, three sharing a topic route to that topic, three sharing nothing route to `@@`. The
  two-rung version called three Rust repositories evidence for a universal principle.

  **No configuration file.** The companion plan specified `.amb` as TOML; `src/memory.rs` refuses
  that surface by name, and nothing here parses TOML — it would be a new dependency in a project
  whose pitch is one static binary. The plan's own sentence removes the need: detection *is* the
  definition, so membership is derivable and the declaration was only ever an override.

  Detection reads the repository root only, because it runs on `PreToolUse`. Topics that are not
  path-shaped — `security`, `performance`, `api-design` — **cannot be detected, ever**; that limit
  is a named list in the code with a test that nothing on it secretly has markers.

  `Derivation` records the deriving repository's topics at the moment it is written, because
  afterwards there is nothing to look up. An absent list means *unknown*, not *none*, so an old
  derivation can only route a promotion outward to `@@`. When several topics qualify the offer
  names the ones it did not pick, and `promote --scope` overrules it.

  **The middle rung is dormant on this machine** — two projects, Rust and Python — and is
  fixture-tested so that it exists when a third arm does.

- **`messages::select` gets unit tests, and one mutation had been surviving** (D83). The predicate
  D17 calls the project's central design claim had no unit test — its four tests covered the string
  helpers. Nine mutations, eight killed by the existing suite; **reversing `ORDER BY m.id` broke
  nothing**, because `delivery::render_all` re-sorts before rendering and the path that actually
  depends on the order is `amb inbox`, which nothing covered. Three tests added; every mutation now
  dies at unit level without spawning a process.

- **Retention is measured rather than intended** (D83, MEASUREMENTS M13). Still no prune, vacuum or
  TTL, and still not worth building at 260 KB — but the table to watch is `messages`, not
  `note_events`: 3.7× the bytes on a third of the rows, because a message stores its body inline.
  The trigger is now a threshold (50 MB, or an `amb inbox` slower than the hook budget) and the
  order of pruning is written down — bodies first, never the ledger.

- **`notes.content_hash` is dropped** (D85). **Schema 9 → 10.** It had a writer and no reader
  anywhere — the recurring defect of D23, D39 and D45, in the one shape `find_unread_fields.py`
  structurally cannot see, because it scans Rust struct fields and this is an SQL column. The only
  `SELECT` of it in the tree was inside a test.

  Q12 asked for a fortnight of `unchanged` against `indexed` before deciding. **That measurement
  could not have changed the answer**: confirming a change by hash means reading the file, which is
  exactly what the `mtime` gate exists to avoid, so the second stage it might have become could only
  save a handful of writes after a read that already happened. The empirical evidence agreed anyway
  — this vault is a plain local directory, neither git nor synced, so the touched-but-unedited case
  does not arise. `text::content_hash` the function stays; `export --check` needs it.

  D67's test read the column as its example of a derived value and now rests on `note_paths` and
  `note_links` — both, so a repair that rebuilt one and not the other is still caught. All four
  injection surfaces byte-identical before and after.

- **The delivery back-off was tested against an implementation that does not ship** (D84).
  `mark_delivered` and `mark_delivered_all` each held their own copy of the same
  `INSERT … ON CONFLICT … attempts + 1`; production called only the batch version, and both callers
  of the single one were the `tests/delivery.rs` assertions that pin the back-off D23 defines and
  D44 depends on. Change the shipped statement and both tests stay green. Deleted; the tests now
  call what runs, and they pass — so the copies agreed and the divergence was latent.

  Found by reading `find_unread_fields.py`'s advisory, which had printed the same three names on
  every run under a line saying "read each one". The other two were **a bug in the audit's own
  arithmetic** — it subtracted a hardcoded 1 for a definition that a generic signature never
  matched, and then, once fixed, subtracted a test fixture's definition from the production side.
  Definitions are now counted outside tests exactly as calls are. Three flags became one, and the
  survivor prints why it is there.

- **A guard for SQL, which no type check can see.** D81's column rename broke one string literal;
  every Rust reference went red and `memory::resolve` did not, surfacing as exit `69` where `65`
  was meant. `no_sql_statement_still_names_the_column_the_note_tables_dropped` reads the literal
  around every statement touching a note table.

### Security

- **Injected content is contained and framed as data on both surfaces** (D60). A message body, a
  subject, a sender's display name and a vault note's title all arrive in an agent's context
  through a tool it trusts — structurally the same object as the crash report in the June 2026
  *agentjacking* disclosure, which measured 85% full execution. Two defects: nothing marked
  third-party text as data, and `register`/`send` both accepted newlines, so a peer could forge
  `[amb] SYSTEM DIRECTIVE:` at column zero in `amb`'s own voice. Every outsider-controlled field is
  now contained and quoted, and each injection states once that quoted text is never an
  instruction. **Containment, not content filtering** — no blocklist, nothing a rephrasing defeats.

### Fixed
- **Two fixes from the previous commit had no guard, and both fail silently.** `Status::to_json`
  now takes the hook state, so a receipt cannot be serialised without saying whether the layer ran
  — the keys were merged onto the document inside `src/main.rs`, where nothing in the suite could
  see them. And `PRIMER` is asserted to name `--same-as` and its failure mode: removing that line
  silently restores the state D69 fixed, where the derivation pipeline had a trigger only a party
  who never pulls it could see, and no test would notice because no candidate is ever created.

- **D69's fix reached only the text surface; `--json` still handed out an uninterpretable ratio.**
  `amb --json memory status` emitted counts with no hook state and no verdict at all — and `--json`
  is the surface agents are told to use, so a machine consumer computed its own ratio and made
  D69's mistake unaided. Both surfaces now answer the same question or neither does.
- **The "is memory installed?" decision was living in `src/main.rs`, untested** — the one file this
  project keeps free of logic precisely so decisions stay testable, in the commit whose subject was
  a decision made without checking its premise. `HookState` now carries its own missing events and
  its own caveat line, `hooks::memory_state` makes the call, and `Receipt::verdict` takes one
  argument instead of a state plus a list that had to agree with it.
- **A partial install would have printed `NOT INSTALLED`** — false, and false in the direction that
  sends someone to reinstall rather than to look at which half is running. It now distinguishes
  `PARTIALLY INSTALLED` and names the missing events.
- **`agents.cwd` says what it holds and now says what it means** (D57, amended). It is the root of
  the *last session that registered under this project name*, not the project's root, and it can
  legitimately be a non-repository — this board carries a project named `T` rooted at a temp
  directory. Collision detection is unaffected, since it asks about the set rather than any
  member's correctness, but anything resolving a foreign project's files through it trusts it past
  what it stores. Recorded on the column and in D57 because a proposal to do exactly that reached
  the point of being scoped before D68 declined it.

- **D59 was approaching a verdict on a feature that was switched off** (D69). The memory hooks were
  not installed on this machine: `install --memory` describes the complete desired hook state, so a
  later `amb install` for a mode change removed all three memory entries — documented, correct, and
  with every removal printed. Nothing said so again for weeks, and a flat cite ratio is D59's
  strongest evidence to withdraw. `Receipt::verdict` now takes the hook state as an argument and
  cannot be computed without one; `amb memory status` prints the state above the counts, because a
  caveat underneath a ratio is read after the ratio has been believed. `Unknown` is a third state
  and is not `Absent` — an unreadable settings file is not proof the layer is off. A negative result
  from an uninstalled feature is indistinguishable from a negative result.
- **The derivation pipeline had a trigger nothing could reach** (D69). `derive` has never run once.
  A candidate exists only when a session or a person declares two sightings the same thing, and the
  flag that does it — `observe --same-as` — is agent-runnable but appeared nowhere an agent reads.
  `PRIMER` named `observe`, `recall` and `--cites` and not `--same-as`. Now named, in D47's
  register: the mechanic and its failure mode, no stakes.


- **A migration that could never take effect, and the false comment that caused it** (D67). Schema
  6 → 7 cleared `content_hash` to force every note to be re-derived; nothing was re-derived,
  because `sync_dir` skips on `mtime` and returns before the cleared column is read. A real board
  sat at `14 scanned · 0 indexed · 14 unchanged` with fourteen empty hashes and no `note_links` for
  a day. Schema 7 → 8 clears `mtime` instead, which re-derives everything without naming any of it.
  `sync_dir`'s comment claimed `content_hash` was "the decision" — it is compared by nothing — and
  that sentence is what made the bad migration look right; it now describes the code. `amb memory
  index` also stops calling itself a rebuild, because it is an incremental sync and is one on
  purpose: it is built out of `sync_dir`, the same scan the `SessionStart` hook calls, so forcing
  it would mean threading a bypass through the hook's own path.

- **The unknown-frontmatter-key warning fired on `amb`'s own output** (D65). `KNOWN_KEYS` was
  measured against `parse_note` alone, but `Note::render` writes `derived_count` and `derived_in`
  for the human opening the file and never parses them back — so every candidate that had ever
  derived drew two permanent `read by nothing` warnings. True sentence, false implication, which is
  the worse kind: it trains the reader to skip the line. The list is now what `amb` writes *or*
  reads, and the guard checks both authorities — reading the writer's keys out of a rendered note
  rather than scanning its source, so a key `render` gains is picked up without anyone updating a
  pattern.
- **`status`'s `on_disk` and `unreadable` counted two separately-maintained walks of the same
  vault.** `count_on_disk` kept its own copy of the directory layout after the walk behind
  `unreadable` was extracted; `drifted()` compares `on_disk` against the index, and D54 records
  what happened the last time that count described a different population from its label. Both now
  go through `note_files`, and a new guard ties it to `kind_dir` so a fifth kind cannot be indexed
  and injected while the instruments stay blind to it.
- **A doc comment that migrated onto the wrong item.** Extracting the frontmatter scanner left
  `parse_note`'s documentation — including D36's guarantee that a note which fails to parse cannot
  break a hook — attached to a type alias, with `parse_note` itself undocumented.

### Added
- **`amb memory coverage` now names the paths no note concerns** (D68). The forward number said
  how much ground was held and threw away which ground was not — the only half anybody can act on.
  The data was already in `claims` and the loop was already computing it; collecting it costs one
  push, and no capture path was added to get it. The two readings partition the edited paths, so a
  short list is never a quiet one. Ordered most-worked first on distinct agents then claim expiry —
  a proxy, named as one: `claims` upserts on `(path, agent)` and records no edit count anywhere,
  and adding one would be a `PostToolUse` change to serve a read-only report. On the real board it
  showed greenfield-api's notes declaring `documentation/*` while every path its sessions edit is
  under `app/services/*`.



- **`amb memory coverage`** — how much of what sessions actually edit is covered by a note (D66).
  It asks `concerning` — the injection query itself — once per edited path rather than re-deriving
  its predicate; the first implementation re-derived it and got three axes wrong (project, status,
  kind) while still producing the right number on the board of the day, which is D51's
  correct-by-accident state. Reports the cross-project contribution separately, since a
  project-filtered count erases the retrieval no per-repo tool can do.
  Read-only. It exists to separate two states the receipt reads as the same `0 cited`: path-anchored
  injection having *nothing to inject* versus *nothing worth injecting*, which call for opposite
  responses. The denominator is the `claims` table rather than the repository, because a note
  covering a file nobody opens can never be injected however good it is — and because a
  repo-wide count would need `git ls-files`, and nothing here shells out to `git`.
- **Unknown frontmatter keys are reported by `amb memory index`** (D65). `confidance: high`
  indexes, injects and exports exactly like `confidence: high` while reaching nothing — this
  project's recurring "field with no reader" defect, one layer out from where
  `find_unread_fields.py` can see it. Warns, never fails: the note still parses and its real keys
  still work. `KNOWN_KEYS` is asserted against the literals in `parse_note`'s own source, so the
  list cannot drift from the parser it describes.
- **Force levels** — `advice` (default), `decision`, `rule`, set with `amb memory observe --force`
  (D64). One consequence, and it is live: injection priority under the cap, ranked *within* a scope
  so D24's rule survives. The vault already exceeds `MAX_INJECTED`, so five notes are dropped every
  session with recency as the only tiebreak. Force never denies anything (D52). `amb memory status`
  splits the cite rate by force, because a level that changes no outcome is decoration.
- **`amb memory status` reports what declining bought** — candidates a decline is currently holding
  back. The suppression always worked; nothing counted it (D64).
- **`amb memory history <id>`** walks a supersession chain both ways, and `amb memory index` now
  reports four link inconsistencies: dangling, superseded-but-still-active, orphaned retirement,
  and cycles (D63). Backed by a derived `note_links` table — schema 6, rebuilt from frontmatter,
  so `rm board.db` still loses nothing.
- `amb snapshot <path>` writes the board to a markdown file for a reader that cannot open the
  database (D61). Marks nothing read, refuses any path inside a repository (D11), and counts its
  own runs so a null result can be told apart from an experiment that never ran.
- `amb agents` reports when two repositories claim one project name (D57). They share a `@project`
  broadcast address, so mail meant for one is delivered into the other. Worktrees and second clones
  are discriminated by git remote and are not reported.
- `amb memory status` prints unprompted citations and D59's standing verdict on whether injection
  is earning its keep — including `too early`, so the condition is visible before it can fire.

### Fixed

- **`supersede` was a second derivation path and left the index under-derived** (D63). It
  hand-updated three columns instead of re-indexing, including `mtime` — so the next pass saw the
  note as unchanged and skipped it, suppressing its own repair. Found by the validator shipped in
  the same commit.
- **Vault writes are atomic, and a note that will not parse is reported as a loss** (D62).
  `std::fs::write` truncates before writing, so a process dying mid-rewrite left a zero-byte note —
  unrecoverable, since the vault is truth and the index holds no content. Worse, the result
  reported itself healthy: the counts agreed, drift said no, and the note went on being injected.
  Writes now rename into place, and `amb memory status` counts and states unreadable notes.
- **A binary older than the board now says so instead of going quiet** (D58). `hook_main` had
  discarded every error, so a stale installed copy produced an empty inbox and nothing else — the
  fault that has killed mail delivery machine-wide four times. It still always exits 0; exactly one
  error class speaks.

### Changed

- `db::bump` / `db::counter` moved out of `memory`, now that the counters table has a non-memory
  consumer.

## [0.1.0] — 2026-08-28

**First tagged state, and a starting point rather than a change against a predecessor.** What the
tool does is `README.md`; why it does it that way — including everything deliberately left out —
is `docs/DECISIONS.md`, D1–D56. Repeating either here would be a second copy that drifts, which is
the argument D56 itself uses to reject a separate versioning document.

For the record, the shape of it: a message bus for concurrent coding-agent sessions on one
machine — direct messages, project and machine-wide broadcasts, advisory file claims, and an
experimental memory vault that stays off unless `AMB_VAULT` is set. One static binary, one SQLite
file at schema 5, no daemon. 278 tests.

The first real entry belongs under `[Unreleased]`, as an actual delta.

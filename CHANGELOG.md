# Changelog

Notable changes to `amb`, newest first, in the
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) format.

What a version number covers here is not the usual answer — `amb` is `publish = false`, so there
is no Rust API to version. **D56** in `docs/DECISIONS.md` names the four surfaces it does cover
and why the on-disk schema is deliberately not one of them.

## [Unreleased]

### Added

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

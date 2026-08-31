//! Why the hooks are not doing what you think they are doing.
//!
//! **This project's failures are silences, and its longest-running one is operational rather than
//! in the code.** `cargo install` writes `~/.cargo/bin/amb`; the hooks in `~/.claude/settings.json`
//! invoke whatever path they were installed with — on this machine `~/.local/bin/amb`. After a
//! schema change, manual `amb` commands work perfectly while **every hook on the machine fails
//! silently**, which is precisely why it goes unnoticed. Observed four times. D56 built the
//! fingerprint that makes the comparison possible; until now nothing performed the comparison.
//!
//! D69 added a second instance of the same shape from the other side: `amb install --memory`
//! describes the *complete* desired hook state, so a later `amb install` for an unrelated mode
//! change removed all three memory entries, and D59 spent weeks accumulating withdrawal evidence
//! about a feature that was switched off. [`crate::hooks::HookState`] answers that one — but it
//! cannot answer the stale-binary one, because a stale binary is still "ours" by
//! [`crate::hooks::command_is_ours`], which matches the file name and never the path (D28).
//!
//! # Shape
//!
//! Every decision here is a pure function over facts someone else gathered, because the facts are
//! the untestable part: a settings file, a subprocess, a clock. [`gather`] is the only thing that
//! touches the world, and it is deliberately dull.

use crate::db;
use crate::hooks;
use crate::version;
use serde_json::{Value, json};

/// How bad a finding is.
///
/// Three levels rather than a boolean, because "I could not tell" is a real answer and must not
/// be rendered as either good news or bad. D69 made the same distinction for `HookState::Unknown`
/// and for the same reason: an unreadable settings file is not evidence that anything is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    /// Something is off but the tool still works, or the answer is unknown.
    Warn,
    /// Something is broken now, silently.
    Bad,
}

impl Health {
    /// The marker a human scans for down the left-hand column.
    pub fn glyph(self) -> &'static str {
        match self {
            Health::Ok => "ok  ",
            Health::Warn => "warn",
            Health::Bad => "BAD ",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Health::Ok => "ok",
            Health::Warn => "warn",
            Health::Bad => "bad",
        }
    }
}

/// One question asked and answered.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub health: Health,
    pub detail: String,
}

impl Check {
    fn new(name: &'static str, health: Health, detail: impl Into<String>) -> Self {
        Check {
            name,
            health,
            detail: detail.into(),
        }
    }
}

/// Everything `doctor` found, in the order a reader should meet it.
#[derive(Debug, Clone)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// The worst health in the report — what the exit code and the summary line are built on.
    pub fn worst(&self) -> Health {
        if self.checks.iter().any(|c| c.health == Health::Bad) {
            Health::Bad
        } else if self.checks.iter().any(|c| c.health == Health::Warn) {
            Health::Warn
        } else {
            Health::Ok
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "worst": self.worst().as_str(),
            "checks": self.checks.iter().map(|c| json!({
                "name": c.name,
                "health": c.health.as_str(),
                "detail": c.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

/// What a hook entry points at, and what that binary says it is.
///
/// `banner` is `None` when the executable could not be run at all — missing, or not executable.
/// That is a distinct and worse condition than a mismatch: the hook is not merely stale, it is
/// dead, and every session on the machine is invoking something that cannot start.
#[derive(Debug, Clone)]
pub struct HookBinary {
    pub event: String,
    pub exe: String,
    pub banner: Option<String>,
}

/// Do the binaries the hooks invoke match the one that is running?
///
/// **The check this command exists for.** Compared on D56's fingerprint rather than on the path,
/// because two paths are not a problem — `~/.local/bin/amb` being a *copy* of the current build is
/// the intended arrangement — and one path is not a guarantee either. The fingerprint is the only
/// thing that answers the question actually being asked.
pub fn build_check(running: &str, hooks: &[HookBinary]) -> Check {
    if hooks.is_empty() {
        return Check::new(
            "binary",
            Health::Warn,
            "no amb hooks are installed, so nothing is delivered automatically — `amb install`",
        );
    }
    let dead: Vec<&HookBinary> = hooks.iter().filter(|h| h.banner.is_none()).collect();
    if let Some(h) = dead.first() {
        return Check::new(
            "binary",
            Health::Bad,
            format!(
                "the {} hook invokes {} and it could not be run at all — every session on this \
                 machine is calling a binary that does not start",
                h.event, h.exe
            ),
        );
    }
    let stale: Vec<&HookBinary> = hooks
        .iter()
        .filter(|h| h.banner.as_deref().is_some_and(|b| b != running))
        .collect();
    match stale.first() {
        None => Check::new(
            "binary",
            Health::Ok,
            format!("every hook runs this build — {running}"),
        ),
        Some(h) => Check::new(
            "binary",
            Health::Bad,
            format!(
                "the {} hook runs {}\n         which reports  {}\n         but this build is  \
                 {}\n         Manual commands work and every hook is stale. Copy it: cp \"$(command \
                 -v amb)\" {}",
                h.event,
                h.exe,
                h.banner.as_deref().unwrap_or("?"),
                running,
                h.exe
            ),
        ),
    }
}

/// Does the board on disk match the schema this binary knows?
///
/// A board *newer* than the binary is the stale-binary failure seen from the board's side, and it
/// is the one D58 already breaks silence for. A board older is ordinary — the next open migrates
/// it — so it is not a finding.
pub fn schema_check(board: Option<i64>, binary: i64) -> Check {
    match board {
        None => Check::new(
            "schema",
            Health::Ok,
            "no board yet; one is created on first use",
        ),
        Some(v) if v > binary => Check::new(
            "schema",
            Health::Bad,
            format!(
                "the board is at schema {v} and this binary knows {binary} — it was written by a \
                 newer amb. Every hook running this build is failing"
            ),
        ),
        Some(v) if v < binary => Check::new(
            "schema",
            Health::Ok,
            format!("board at schema {v}; the next open migrates it to {binary}"),
        ),
        Some(v) => Check::new(
            "schema",
            Health::Ok,
            format!("board and binary agree at {v}"),
        ),
    }
}

/// `amb` hooks the platform will run more than once per event.
///
/// **The row D77 could have used and did not have.** Duplicated hooks make every injection happen
/// twice and count once — `note_events` is keyed so a second injection into one session writes no
/// row — so the cost doubles while the denominator does not, and D59's citation ratio improves for
/// free. Invisible, and in the flattering direction.
///
/// Pure, over what [`crate::hooks::duplicate_hooks`] found, so both halves are testable apart.
pub fn duplicate_check(dupes: &[hooks::DuplicateHook]) -> Check {
    if dupes.is_empty() {
        return Check::new(
            "hook dupes",
            Health::Ok,
            "no amb hook is registered in more than one settings scope",
        );
    }
    // `Bad`, not `Warn`: this one silently corrupts the number D59's withdrawal is read off, and
    // a stale binary — the other `Bad` here — is no worse.
    let detail = dupes
        .iter()
        .map(|d| {
            format!(
                "{} runs {}x ({})",
                d.event,
                d.sources.len(),
                d.sources.join(" + ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Check::new(
        "hook dupes",
        Health::Bad,
        format!(
            "{detail} — each fires every time, so injections cost double and count once, \
             flattering D59's ratio (D77). Remove the entry from all but one scope"
        ),
    )
}

/// The storage engine this build carries, and whether it is past the fix that matters.
///
/// **Pure, and takes the version rather than reading it, so the boundary cases are testable.**
/// [`gather`] passes `crate::version::sqlite()`.
///
/// The floor is 3.51.3, where the WAL-reset bug was fixed after fourteen years. It matters here
/// more than in most SQLite applications because the trigger is amb's normal operating condition —
/// several unrelated processes writing or checkpointing one WAL file at once — and the symptom is
/// a committed write that later transactions cannot see, raising no error. A lost message and an
/// empty inbox look identical.
///
/// An unparseable version is a warning rather than a failure: the board still works, and refusing
/// to report anything because the string had an unexpected shape would be worse than saying so.
pub fn sqlite_check(version: &str) -> Check {
    /// 3.51.3 in SQLite's own encoding: major*1e6 + minor*1e3 + patch.
    const WAL_RESET_FIXED_IN: u32 = 3_051_003;

    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return Check::new(
            "sqlite",
            Health::Warn,
            format!("bundled sqlite reports {version:?}, which is not major.minor.patch"),
        );
    }
    let n = parts[0] * 1_000_000 + parts[1] * 1_000 + parts[2];
    if n < WAL_RESET_FIXED_IN {
        return Check::new(
            "sqlite",
            Health::Bad,
            format!(
                "bundled sqlite {version} predates the WAL-reset fix in 3.51.3 — under concurrent \
                 processes a committed write can become invisible to later reads with no error. \
                 Update libsqlite3-sys"
            ),
        );
    }
    Check::new(
        "sqlite",
        Health::Ok,
        format!("bundled sqlite {version}, past the 3.51.3 WAL-reset fix"),
    )
}

/// Has a delivery surface actually produced an event recently, or is it merely installed?
///
/// **Installed is three of the four conditions.** `doctor` can see that an entry exists, points at
/// the right binary and has the right shape; whether the event ever *arrives* is a fourth thing,
/// and it is the one D69's uninstall was invisible to. A `PreToolUse` entry installed this morning
/// whose last event is two days old is visibly not firing.
///
/// **Silence is not automatically a fault**, which is why an absent last-event is only a warning
/// and names the ambiguity. `amb`'s own `PreToolUse` memory hook matches `Read|Edit|Write|
/// NotebookEdit`, so a session that reads files through `Bash` produces none of these events by
/// construction — the lane is not broken, it was never invoked.
pub fn freshness_check(name: &'static str, last: Option<f64>, now: f64, installed: bool) -> Check {
    if !installed {
        return Check::new(name, Health::Warn, "not installed, so nothing can arrive");
    }
    match last {
        None => Check::new(
            name,
            Health::Warn,
            "installed, but no event has ever been recorded — either it has not fired yet or it \
             is not firing",
        ),
        Some(ts) => {
            let hours = (now - ts) / 3600.0;
            let age = if hours < 1.0 {
                format!("{:.0} minute(s) ago", (now - ts) / 60.0)
            } else if hours < 48.0 {
                format!("{hours:.1} hour(s) ago")
            } else {
                format!("{:.1} day(s) ago", hours / 24.0)
            };
            // Deliberately never `Bad`. A quiet lane is normal — see the doc comment — and a
            // doctor that cries about ordinary silence is a doctor nobody runs.
            let health = if hours > 168.0 {
                Health::Warn
            } else {
                Health::Ok
            };
            Check::new(name, health, format!("last event {age}"))
        }
    }
}

/// Is the board somewhere SQLite can actually be trusted?
///
/// Reuses [`db::guard_location`] rather than re-deriving the rule, so `doctor` cannot disagree
/// with what `open` will do — the failure mode where a diagnostic passes and the tool still
/// refuses.
pub fn location_check(path: &std::path::Path) -> Check {
    match db::guard_location(path) {
        Ok(()) => Check::new("board", Health::Ok, format!("{}", path.display())),
        Err(e) => Check::new("board", Health::Bad, e.to_string()),
    }
}

/// How big the board is, against the size D83 says to build pruning at.
///
/// **Three files, not one, and that is not pedantry.** In WAL mode the `-wal` sidecar holds
/// committed transactions the main file does not yet contain, so `metadata(path)` alone understates
/// a busy board. This project measured the consequence from the other side: the main file's own
/// bytes change under concurrent *readers*, because a read updates `-shm` and can trigger a
/// checkpoint (M32). Disk footprint is what "the board passes 50 MB" means to a person, so the
/// sidecars are summed rather than ignored.
///
/// Fires **at** the threshold and not one byte past it. A strict `>` would make the number D83
/// actually names the last value that does *not* trigger, which reads wrong to everyone who has
/// only read the decision.
pub fn size_check(bytes: u64) -> Check {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    let limit = db::PRUNE_AT_BYTES as f64 / (1024.0 * 1024.0);
    if bytes >= db::PRUNE_AT_BYTES {
        Check::new(
            "size",
            Health::Warn,
            format!(
                "{mb:.1} MB — past D83's {limit:.0} MB. Build pruning, and prune `messages` \
                 bodies before the ledger: the ledger is the only record that a session was \
                 shown a note"
            ),
        )
    } else {
        Check::new(
            "size",
            Health::Ok,
            format!("{mb:.1} MB of the {limit:.0} MB at which D83 builds pruning"),
        )
    }
}

/// Every byte the board occupies, including the WAL sidecars. Unreadable files count as zero,
/// because a doctor that refuses to report a number it partly knows is less useful than one that
/// under-reports and keeps going.
fn board_bytes(path: &std::path::Path) -> u64 {
    let mut total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    for side in db::sidecars(path) {
        total += std::fs::metadata(side).map(|m| m.len()).unwrap_or(0);
    }
    total
}

/// Read the world, then hand it to the pure functions above.
///
/// Every fallible read degrades to a `Warn` rather than aborting: a doctor that stops at the first
/// thing it cannot read is useless precisely when it is needed.
pub fn gather(now: f64) -> Report {
    let mut checks = Vec::new();
    let running = version::banner();

    // --- the binaries the hooks invoke -------------------------------------------------
    let settings = hooks::settings_path().and_then(|p| hooks::read_settings(&p));
    match &settings {
        Err(e) => checks.push(Check::new(
            "binary",
            Health::Warn,
            format!("could not read ~/.claude/settings.json: {e}"),
        )),
        Ok(v) => {
            let entries: Vec<HookBinary> = hooks::our_hook_exes(v)
                .into_iter()
                .map(|(event, exe)| {
                    let banner = std::process::Command::new(&exe)
                        .arg("--version")
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .trim()
                                .trim_start_matches("amb ")
                                .to_string()
                        });
                    HookBinary { event, exe, banner }
                })
                .collect();
            checks.push(build_check(running, &entries));

            // Every scope the platform merges, not just this one: D77's duplicate spanned
            // `~/.claude/settings.json` and a project `.claude/settings.local.json`, so a check
            // reading one file could not have seen the defect it exists for.
            let home = std::env::var("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            let cwd = std::env::current_dir().unwrap_or_default();
            let loaded: Vec<(String, Value)> = hooks::settings_sources(&home, &cwd)
                .into_iter()
                .filter_map(|(label, path)| hooks::read_settings(&path).ok().map(|v| (label, v)))
                .collect();
            checks.push(duplicate_check(&hooks::duplicate_hooks(&loaded)));

            let (installed, missing) = hooks::memory_hooks(v);
            checks.push(if missing.is_empty() {
                Check::new(
                    "hooks",
                    Health::Ok,
                    format!("memory hooks installed on {}", installed.join(", ")),
                )
            } else {
                Check::new(
                    "hooks",
                    Health::Warn,
                    format!(
                        "memory hooks missing on {} — `amb install --memory` restores them. A \
                         later `amb install` without --memory removes all three (D69)",
                        missing.join(", ")
                    ),
                )
            });
        }
    }

    // --- the engine underneath the board -----------------------------------------------
    checks.push(sqlite_check(crate::version::sqlite()));

    // --- the board ---------------------------------------------------------------------
    match db::db_path() {
        Err(e) => checks.push(Check::new("board", Health::Warn, e.to_string())),
        Ok(path) => {
            checks.push(location_check(&path));
            // Once: the schema check and the size check ask the same question of the same path.
            let exists = path.exists();
            let on_disk = if exists {
                db::open_at(&path).ok().and_then(|c| {
                    c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                        .ok()
                })
            } else {
                None
            };
            checks.push(schema_check(on_disk, db::SCHEMA_VERSION));
            if exists {
                checks.push(size_check(board_bytes(&path)));
            }
        }
    }

    // --- the memory layer, and whether its lanes are actually firing --------------------
    match crate::memory::vault_path() {
        None => checks.push(Check::new(
            "vault",
            Health::Ok,
            "AMB_VAULT unset — memory is off, which is the default (D35)",
        )),
        Some(v) => {
            checks.push(Check::new("vault", Health::Ok, format!("{}", v.display())));
            let memory_on = settings
                .as_ref()
                .ok()
                .map(|s| hooks::memory_hooks(s).1.is_empty())
                .unwrap_or(false);
            if let Ok(path) = db::db_path()
                && path.exists()
                && let Ok(conn) = db::open_at(&path)
            {
                for (name, event) in [
                    ("inject:session", "injected"),
                    ("inject:file", "injected_file"),
                ] {
                    let last: Option<f64> = conn
                        .query_row(
                            "SELECT max(ts) FROM note_events WHERE event = ?1",
                            [event],
                            |r| r.get(0),
                        )
                        .ok()
                        .flatten();
                    checks.push(freshness_check(name, last, now, memory_on));
                }
            }
        }
    }

    Report { checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A truth table, because the guard is a comparison and comparisons relax silently.**
    ///
    /// M27 found thirty-seven of forty survivors in one renderer sitting on the `if` that decides
    /// whether a line renders at all, ten of them the literal edit `x > 0` -> `x >= 0`. A
    /// presence-only test cannot see that relaxation. A row on each side of the boundary fails in
    /// both directions, and the `at` row is the one that pins which operator this is.
    #[test]
    fn the_size_row_fires_at_the_threshold_and_not_before() {
        let at = db::PRUNE_AT_BYTES;
        for (bytes, expected, why) in [
            (0u64, Health::Ok, "an empty board"),
            (at - 1, Health::Ok, "one byte below the threshold"),
            (at, Health::Warn, "exactly the number D83 names"),
            (at * 2, Health::Warn, "well past it"),
        ] {
            assert_eq!(size_check(bytes).health, expected, "{why}");
        }
    }

    /// **The row must state both numbers, or D83 is still unreadable.**
    ///
    /// D95's rule is that a stated threshold needs something able to say whether it is *reachable*,
    /// not merely something that reports a size. A row printing `0.5 MB` alone would leave the
    /// reader to go and look 50 MB up, which is the work this row exists to remove.
    #[test]
    fn the_size_row_names_the_threshold_as_well_as_the_size() {
        let d = size_check(0).detail;
        assert!(d.contains("50 MB"), "the threshold is missing: {d}");
        assert!(d.contains("0.0 MB"), "the current size is missing: {d}");
        assert!(d.contains("D83"), "nothing points at the decision: {d}");
    }

    /// **The sidecars are part of the board, and a fixture with only a main file cannot see it.**
    ///
    /// In WAL mode the `-wal` file holds committed transactions the main file does not yet contain,
    /// so summing one file understates a busy board. The second half of this test is the fixture
    /// that reaches the branch — M17's rule, applied before rather than after: without those two
    /// `write`s the loop over the suffixes could be deleted and the first assertion would still
    /// pass.
    #[test]
    fn the_board_size_includes_the_wal_sidecars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("board.db");
        std::fs::write(&db, vec![0u8; 100]).expect("main");
        assert_eq!(board_bytes(&db), 100, "the main file alone");

        std::fs::write(dir.path().join("board.db-wal"), vec![0u8; 250]).expect("wal");
        std::fs::write(dir.path().join("board.db-shm"), vec![0u8; 30]).expect("shm");
        assert_eq!(
            board_bytes(&db),
            380,
            "a WAL board's committed bytes were not counted"
        );
    }

    /// A board that is not there is zero bytes, not a panic — `gather` reports every failure it
    /// can survive rather than aborting on the first.
    #[test]
    fn a_missing_board_is_zero_rather_than_an_error() {
        assert_eq!(board_bytes(std::path::Path::new("/no/such/board.db")), 0);
    }

    /// The three levels reach a reader as three distinct markers.
    ///
    /// **Found by mutation: `glyph` could return `""` for all three and nothing went red.** It is
    /// the column a human scans, and D69's whole argument is that "I could not tell" must not be
    /// rendered as either good news or bad — a rule that only holds if the renderings differ.
    /// Asserted as a set rather than as three literals, so the wording stays free to change while
    /// the distinction cannot collapse.
    #[test]
    fn each_health_level_is_a_distinct_marker() {
        let all = [Health::Ok, Health::Warn, Health::Bad];
        let glyphs: Vec<&str> = all.iter().map(|h| h.glyph()).collect();
        let names: Vec<&str> = all.iter().map(|h| h.as_str()).collect();
        for set in [&glyphs, &names] {
            for (i, a) in set.iter().enumerate() {
                assert!(!a.trim().is_empty(), "a level rendered as nothing: {set:?}");
                for b in &set[i + 1..] {
                    assert_ne!(a, b, "two levels render identically: {set:?}");
                }
            }
        }
        // The one literal worth pinning: a reader greps for it, and lowercasing it to match the
        // others would be a readability change that quietly removes the alarm.
        assert_eq!(Health::Bad.glyph().trim(), "BAD");
    }

    /// `worst` is the exit code, and the JSON surface carries every check.
    ///
    /// **Found by mutation: `Report::to_json` could return `Default::default()` — an empty
    /// value — and nothing went red.** Five keys, none asserted anywhere. `doctor --json` is what
    /// a script reads to decide whether this machine is healthy, so an empty document is a
    /// health check that answers every question with silence.
    ///
    /// `worst` is asserted through all three levels because it drives the exit code: reporting
    /// `Bad` as `Ok` is the failure that matters, and it needs the precedence to be wrong in only
    /// one direction to happen.
    #[test]
    fn the_report_surfaces_every_check_and_the_worst_of_them() {
        let report = |levels: &[Health]| Report {
            checks: levels
                .iter()
                .enumerate()
                .map(|(i, h)| Check::new("binary", *h, format!("detail {i}")))
                .collect(),
        };

        assert_eq!(report(&[Health::Ok, Health::Ok]).worst(), Health::Ok);
        assert_eq!(report(&[Health::Ok, Health::Warn]).worst(), Health::Warn);
        assert_eq!(
            report(&[Health::Warn, Health::Bad]).worst(),
            Health::Bad,
            "one broken thing outranks any number of warnings"
        );

        let doc = report(&[Health::Ok, Health::Bad]).to_json();
        assert_eq!(doc["worst"], Health::Bad.as_str());
        let checks = doc["checks"].as_array().expect("checks is an array");
        assert_eq!(checks.len(), 2, "every check reaches the machine surface");
        assert_eq!(checks[0]["name"], "binary");
        assert_eq!(checks[0]["health"], Health::Ok.as_str());
        assert_eq!(checks[1]["health"], Health::Bad.as_str());
        assert_eq!(checks[1]["detail"], "detail 1");
    }

    /// Schema drift is only `Bad` in the direction that breaks every hook on the machine.
    ///
    /// **Found by mutation: five survivors on one guard.** Three of the four arms report `Ok`, so
    /// `health` alone cannot tell them apart — the *detail* is the whole answer, and nothing
    /// asserted it. That matters because this is the check for the condition that has recurred
    /// five times (D73, D94): a board written by a newer `amb` than the binary the hooks invoke.
    /// Reporting "board and binary agree at 11" while they do not is the precise failure.
    #[test]
    fn schema_drift_is_reported_in_the_one_direction_that_breaks_hooks() {
        let bad = schema_check(Some(13), 12);
        assert_eq!(bad.health, Health::Bad);
        assert!(bad.detail.contains("newer amb"), "{}", bad.detail);

        // Older is ordinary: the next open migrates it. Ok, but it must say so rather than claim
        // agreement — flipping `<` to `<=` or to `==` swaps these two messages silently.
        let older = schema_check(Some(11), 12);
        assert_eq!(older.health, Health::Ok);
        assert!(
            older.detail.contains("migrates it to 12"),
            "{}",
            older.detail
        );

        let agreed = schema_check(Some(12), 12);
        assert_eq!(agreed.health, Health::Ok);
        assert!(agreed.detail.contains("agree at 12"), "{}", agreed.detail);

        let none = schema_check(None, 12);
        assert_eq!(none.health, Health::Ok);
        assert!(none.detail.contains("no board yet"), "{}", none.detail);
    }

    /// `duplicate_check`'s verdict, both directions.
    ///
    /// **Written because mutating it to `if true` — always Ok — survived every other test here.**
    /// `duplicate_hooks` has a full truth table, but that covers the *detector*; nothing covered
    /// the *decision*, so a check that had stopped escalating would have reported a healthy
    /// machine while D77's defect ran. M27's shape: presence-only coverage cannot see a guard that
    /// silently stopped firing.
    #[test]
    fn duplicate_hooks_escalate_and_an_empty_list_does_not() {
        let ok = duplicate_check(&[]);
        assert_eq!(ok.health, Health::Ok);
        assert!(ok.detail.contains("no amb hook"), "{}", ok.detail);

        let bad = duplicate_check(&[hooks::DuplicateHook {
            event: "SessionStart".into(),
            command: "/bin/amb hook memory".into(),
            sources: vec!["user".into(), "project local".into()],
        }]);
        assert_eq!(
            bad.health,
            Health::Bad,
            "a duplicate corrupts D59's ratio silently; it is not a warning"
        );
        // The reader has to know which files to edit, so both scopes must be named.
        assert!(bad.detail.contains("user"), "{}", bad.detail);
        assert!(bad.detail.contains("project local"), "{}", bad.detail);
        assert!(bad.detail.contains("SessionStart"), "{}", bad.detail);
        assert!(
            bad.detail.contains("2x"),
            "the multiplier is the point: {}",
            bad.detail
        );
    }

    /// A truth table across the 3.51.3 boundary, not a list of needles.
    ///
    /// **Every row is a presence row, deliberately.** M27 records that an absence-only assertion
    /// carries an unproven premise — it passes when the enclosing block never rendered. Each row
    /// here asserts a health *and* a phrase, so a `sqlite_check` that stopped producing a check at
    /// all fails rather than passing vacuously.
    ///
    /// The two rows either side of the boundary are the point: `>=` relaxed to `>` flips 3.51.3
    /// itself from Ok to Bad, and `<` relaxed to `<=` flips it the other way.
    #[test]
    fn the_sqlite_floor_is_the_wal_reset_fix_and_the_boundary_is_pinned() {
        for (version, want, phrase) in [
            // Fourteen years of affected releases, and the one this board would have shipped on.
            ("3.7.0", Health::Bad, "predates the WAL-reset fix"),
            ("3.51.2", Health::Bad, "predates the WAL-reset fix"),
            // The fix itself. Ok, and this row is what an off-by-one reddens.
            ("3.51.3", Health::Ok, "past the 3.51.3 WAL-reset fix"),
            ("3.53.2", Health::Ok, "past the 3.51.3 WAL-reset fix"),
            ("4.0.0", Health::Ok, "past the 3.51.3 WAL-reset fix"),
            // Minor and patch must not be compared lexically: "3.9.0" > "3.51.3" as strings.
            ("3.9.0", Health::Bad, "predates the WAL-reset fix"),
            // A shape it cannot judge is a warning, never a silent pass.
            ("not-a-version", Health::Warn, "not major.minor.patch"),
            ("3.51", Health::Warn, "not major.minor.patch"),
        ] {
            let c = sqlite_check(version);
            assert_eq!(c.health, want, "{version} judged wrong: {}", c.detail);
            assert!(
                c.detail.contains(phrase),
                "{version} did not say {phrase:?}: {}",
                c.detail
            );
        }
    }

    /// The engine is reported at all, from the real build.
    ///
    /// `gather` assembling a report that silently omits the row is the failure this guards, and it
    /// is the half the truth table above cannot see — that table proves `sqlite_check` is correct,
    /// not that anybody calls it.
    #[test]
    fn the_report_names_the_storage_engine() {
        let report = gather(1_700_000_000.0);
        let found = report.checks.iter().find(|c| c.name == "sqlite");
        let found = found.expect("doctor must report the bundled sqlite build");
        assert!(
            found.detail.contains(crate::version::sqlite()),
            "the row does not carry the version actually compiled in: {}",
            found.detail
        );
    }

    /// Freshness reads in the coarsest useful unit, and never cries about ordinary silence.
    ///
    /// **Found by mutation: five survivors across three comparisons.** Each is a boundary — the
    /// minute/hour cut, the hour/day cut, and the week at which a quiet lane becomes worth
    /// mentioning — and every one was asserted at no point on either side of itself.
    ///
    /// The last is the one with teeth: `>` becoming `>=` is invisible except at exactly a week,
    /// and this check is *deliberately never `Bad`* because a doctor that alarms on ordinary
    /// silence is a doctor nobody runs.
    #[test]
    fn freshness_reads_in_the_coarsest_unit_and_warns_only_after_a_week() {
        let at = |hours: f64| freshness_check("inject:session", Some(0.0), hours * 3600.0, true);

        assert!(
            at(0.5).detail.contains("30 minute(s) ago"),
            "{}",
            at(0.5).detail
        );
        assert!(
            at(1.0).detail.contains("1.0 hour(s) ago"),
            "{}",
            at(1.0).detail
        );
        assert!(
            at(10.0).detail.contains("10.0 hour(s) ago"),
            "{}",
            at(10.0).detail
        );
        assert!(
            at(48.0).detail.contains("2.0 day(s) ago"),
            "{}",
            at(48.0).detail
        );

        assert_eq!(
            at(168.0).health,
            Health::Ok,
            "a week exactly is still quiet, not stale"
        );
        assert_eq!(at(169.0).health, Health::Warn);
        assert_ne!(
            at(1000.0).health,
            Health::Bad,
            "a quiet lane is never an emergency"
        );

        // The two non-numeric answers, which are warnings that name their own ambiguity rather
        // than findings.
        let never = freshness_check("inject:file", None, 0.0, true);
        assert_eq!(never.health, Health::Warn);
        assert!(never.detail.contains("not firing"), "{}", never.detail);
        let off = freshness_check("inject:file", None, 0.0, false);
        assert_eq!(off.health, Health::Warn);
        assert!(off.detail.contains("not installed"), "{}", off.detail);
    }

    fn hook(event: &str, exe: &str, banner: Option<&str>) -> HookBinary {
        HookBinary {
            event: event.to_string(),
            exe: exe.to_string(),
            banner: banner.map(str::to_string),
        }
    }

    /// Two paths are not a problem; two *builds* are. This is the four-occurrence failure.
    #[test]
    fn a_hook_running_a_different_build_is_the_finding_not_a_different_path() {
        let here = "0.1.0 (abc1234 2026-08-28, schema 8)";
        let copy = hook("Stop", "/Users/x/.local/bin/amb", Some(here));
        assert_eq!(
            build_check(here, &[copy]).health,
            Health::Ok,
            "a copy of the same build at another path is the intended arrangement"
        );

        let stale = hook(
            "Stop",
            "/Users/x/.local/bin/amb",
            Some("0.1.0 (999aaaa 2026-08-01, schema 4)"),
        );
        let c = build_check(here, &[stale]);
        assert_eq!(c.health, Health::Bad);
        assert!(c.detail.contains("schema 4"), "names what the hook runs");
        assert!(c.detail.contains("cp "), "says how to fix it");
    }

    /// A hook pointing at something that will not start is worse than one pointing at an old
    /// build, and must not be reported as merely stale.
    #[test]
    fn a_hook_binary_that_cannot_run_outranks_a_stale_one() {
        let here = "0.1.0 (abc1234 2026-08-28, schema 8)";
        let c = build_check(here, &[hook("Stop", "/gone/amb", None)]);
        assert_eq!(c.health, Health::Bad);
        assert!(c.detail.contains("could not be run"));
    }

    #[test]
    fn no_hooks_at_all_is_a_warning_not_a_pass() {
        let c = build_check("x", &[]);
        assert_eq!(c.health, Health::Warn);
        assert!(c.detail.contains("amb install"));
    }

    /// A board newer than the binary is the stale-binary failure from the other side.
    #[test]
    fn a_board_newer_than_the_binary_is_bad_and_older_is_routine() {
        assert_eq!(schema_check(Some(9), 8).health, Health::Bad);
        assert_eq!(schema_check(Some(7), 8).health, Health::Ok);
        assert_eq!(schema_check(Some(8), 8).health, Health::Ok);
        assert_eq!(schema_check(None, 8).health, Health::Ok);
    }

    /// Installed is three of four conditions; this is the fourth.
    #[test]
    fn an_installed_lane_that_has_never_fired_is_distinguished_from_one_that_is_off() {
        let now = 1_000_000.0;
        assert_eq!(
            freshness_check("x", None, now, false).health,
            Health::Warn,
            "not installed"
        );
        let never = freshness_check("x", None, now, true);
        assert_eq!(never.health, Health::Warn);
        assert!(
            never.detail.contains("not firing"),
            "installed-but-silent must say so; that is D69's failure"
        );
        assert_eq!(
            freshness_check("x", Some(now - 60.0), now, true).health,
            Health::Ok
        );
    }

    /// Quiet is normal for the path lane, so age alone never escalates past a warning.
    #[test]
    fn an_old_event_warns_and_never_reports_bad() {
        let now = 1_000_000.0;
        let old = freshness_check("x", Some(now - 3600.0 * 24.0 * 30.0), now, true);
        assert_eq!(old.health, Health::Warn);
        assert!(old.detail.contains("day(s) ago"));
    }

    /// The summary must take the worst answer, or a `BAD` line scrolls past under an `ok`.
    #[test]
    fn the_summary_takes_the_worst_check() {
        let r = |h: Health| Report {
            checks: vec![Check::new("a", Health::Ok, ""), Check::new("b", h, "")],
        };
        assert_eq!(r(Health::Ok).worst(), Health::Ok);
        assert_eq!(r(Health::Warn).worst(), Health::Warn);
        assert_eq!(r(Health::Bad).worst(), Health::Bad);
    }
}

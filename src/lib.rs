//! `amb` — a message bus for concurrent coding-agent sessions on one machine.
//!
//! One SQLite file, no daemon. Direct messages, project-wide broadcasts and advisory file
//! claims, across more than one repository.
//!
//! # Shape
//!
//! Everything lives here rather than in `main.rs`, so tests exercise real code paths instead of
//! shelling out for every assertion. The binary is a thin shell: parse arguments, call in here,
//! map an [`error::Error`] to an exit code.
//!
//! The crate is `publish = false`, so library and binary share one package. The usual argument
//! for splitting them — decoupling a published library's version from a binary's — does not
//! apply to a private tool.
//!
//! # Where the reasoning lives
//!
//! `docs/DECISIONS.md` holds what was decided *and what was rejected*. Modules cite decisions by
//! number (D5, D13) rather than restating them, so there is one copy of each argument.

pub mod address;
pub mod claims;
pub mod db;
pub mod delivery;
pub mod doctor;
pub mod duration;
pub mod error;
pub mod hooks;
pub mod identity;
pub mod memory;
pub mod messages;
pub mod status;
pub mod vendors;
pub mod version;

pub use error::{Error, Result};

/// The `--json` contract version, carried on every object the binary prints (D117).
///
/// **In the library rather than in `main.rs`, so the one test that asserts it cannot transcribe
/// it** (M28: two constants rotted here because a second copy existed to drift from). The binary
/// stamps it; `tests/versioning.rs` requires that moving it is documented; `tests/cli_e2e.rs`
/// checks every command carries it. Three readers, one definition.
///
/// # Why it exists at all
///
/// **D56 names `--json` a versioned surface “bound by agents parsing output”, and until D117 the
/// output could not say which version it satisfied.** `amb --version` has carried a full
/// fingerprint since D56 — `amb 0.2.1-rc.1 (de69dfc 2026-09-06, schema 15, sqlite 3.53.2)` — and
/// it travels in a *different invocation* from the data. A program that parses `amb inbox --json`
/// and caches a strategy therefore had no way to notice the shape moving under it; it found out
/// by failing, which on the hook path D9 makes silent.
///
/// # What the number means
///
/// **Independent of the package version** — D56 keeps `PRAGMA user_version` off the release
/// number's list for the same reason: two things with different compatibility rules need two
/// numbers. `0.2.1` may ship for a reason no parser can observe.
///
/// It moves when a field a reader could be relying on **changes meaning or leaves**. Adding a
/// field does not move it, and that is what makes adding one safe.
///
/// | v | Changed |
/// |---|---|
/// | 1 | D117, 2026-09-05. Every object carries `v`, on the success and error paths alike. |
/// | 2 | D137, 2026-09-06. **A list-shaped command returns a window.** `amb inbox --json` and `amb claims --json` both report `count` as what the object carries rather than everything selected, with `total`, `hidden` and `limit` beside it (`unread` too, on the inbox). **`limit` is `0` when the caller asked for no limit**, so a reader seeing `limit: 0` beside a non-zero `count` is looking at an unwindowed answer rather than an empty one. `body` is untouched and still whole. |
pub const JSON_CONTRACT: u64 = 2;

/// Assert a query's plan reaches the named index — for guards where the *plan* is the rule.
///
/// Two defects arrived in one audit that were invisible to every result-shaped assertion,
/// because the rows were always right and only the access path was wrong: `claims::list`'s
/// `(?1 IS NULL OR …)` idiom defeated `ix_claims_live` on the `PostToolUse` path, and the memory
/// sync's per-file probe walked every note of a kind. `EXPLAIN QUERY PLAN` is the one surface
/// that class shows on, so it gets the same treatment [`assert_rendered_shape`] gives rendered
/// text: one helper, so the recipe — column 3 is `detail`, and EXPLAIN still counts the
/// statement's parameters so placeholders must be bound — is written once.
#[cfg(test)]
pub(crate) fn assert_query_plan_uses(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: Vec<rusqlite::types::Value>,
    index: &str,
) {
    let plan = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = conn.prepare(&plan).expect("planning the query");
    let details: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(binds), |r| r.get::<_, String>(3))
        .expect("reading the plan")
        .flatten()
        .collect();
    assert!(
        details.iter().any(|d| d.contains(index)),
        "the plan never reaches {index} — scanned instead: {details:?}"
    );
}

/// Shape invariants every rendered artefact must satisfy, asserted as a class rather than as needles.
///
/// **M24 is why this exists.** A wrapped string literal kept its indentation and rendered
/// `"before it          opened cannot enter one"`. Every `contains` assertion passed, because each
/// needle sat on one side of the damage — `contains` describes points, and the defect was in the
/// space between them. The conclusion recorded then was that a rendered artefact needs at least one
/// *whole-shape* assertion, and that the shape matters more than the specific rule.
///
/// **What is NOT here is the interesting part.** "A rendered line has no double space" is M24's own
/// rule and it is deliberately absent, because it is false as a universal: measured over 274 lines
/// of real `amb` output, 50 carried an interior run of spaces and every one was a deliberately
/// aligned column (`board  /Users/…` beside `copy   /var/…`). A rule with a legitimate exception on
/// a fifth of its input is one people switch off, so it stays a *per-renderer* assertion where the
/// output is prose — `events.rs` keeps it — and never a global one (M33).
///
/// What is here is the set that held with zero violations across that same corpus, so each one is
/// a real constraint rather than an aspiration.
#[cfg(test)]
pub(crate) fn assert_rendered_shape(label: &str, rendered: &str) {
    for (n, line) in rendered.lines().enumerate() {
        let where_ = format!("{label}, line {}", n + 1);
        assert!(
            !line.contains('\t'),
            "{where_}: a tab reached rendered output, which no terminal width agrees on: {line:?}"
        );
        assert!(
            line.is_empty() || !line.trim().is_empty(),
            "{where_}: a blank line made of spaces rather than nothing: {line:?}"
        );
        assert_eq!(
            line.trim_end(),
            line,
            "{where_}: trailing whitespace, invisible in review and visible in a diff"
        );
        // **`str::lines` splits on `\n` and `\r\n`, so this loop cannot see a line it does not
        // already believe in** — which is exactly how the hole D125 fixes stayed open. The guard
        // gated on `char::is_control()` and the test iterated `.lines()`: both carried the same
        // `Cc`-shaped definition of "a line", so a U+2028 was invisible to the assertion for the
        // same reason it was invisible to the containment. A test that shares its subject's blind
        // spot passes for the wrong reason, and this is the check that does not.
        //
        // Asserted here rather than in `delivery`'s own tests because 26 call sites across 11
        // modules already reach this function; keying on the property rather than enumerating the
        // renderers is what stops the next one being added without it (M23).
        if let Some(c) = line.chars().find(|c| crate::delivery::breaks_grammar(*c)) {
            panic!(
                "{where_}: U+{:04X} survived into rendered output, and `str::lines` cannot see \
                 the break it makes: {line:?}",
                c as u32
            );
        }
    }
}

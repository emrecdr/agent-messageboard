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
pub mod vendors;
pub mod version;

pub use error::{Error, Result};

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
    }
}

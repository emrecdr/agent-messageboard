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
pub mod version;

pub use error::{Error, Result};

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

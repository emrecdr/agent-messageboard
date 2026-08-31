//! The release version is a claim about four contract surfaces (D56), and claims drift.
//!
//! Two things go stale silently here. A version bumped in `Cargo.toml` and forgotten in the
//! changelog leaves the release undocumented — the ordinary failure every project has. The second
//! is specific to this tool: `--version` exists to diagnose a **stale installed binary**, which
//! has broken mail delivery machine-wide three times, so a banner that quietly stopped carrying
//! the commit or the schema would remove the diagnostic exactly when it is needed, and look
//! entirely normal until then.
//!
//! What the banner *says* is asserted without a process, in `src/version.rs`. What is left here
//! is the part that genuinely needs one: that clap serves that banner rather than its own.

mod common;
use common::Board;

/// Compile-time, so a deleted or unreadable changelog is a build failure rather than a test that
/// quietly reads an empty string and passes.
const CHANGELOG: &str = include_str!("../CHANGELOG.md");

#[test]
fn the_changelog_documents_the_version_being_built() {
    let heading = format!("## [{}] — ", env!("CARGO_PKG_VERSION"));
    assert!(
        CHANGELOG.contains(&heading),
        "Cargo.toml is at {} but CHANGELOG.md has no `{heading}` heading. Bumping the version is \
         half of a release; the other half is saying what changed.",
        env!("CARGO_PKG_VERSION"),
    );
}

#[test]
fn the_changelog_keeps_a_place_for_the_next_release() {
    assert!(
        CHANGELOG.contains("## [Unreleased]"),
        "CHANGELOG.md lost its Unreleased section — the place a change lands before it has a \
         version number. Without it the next entry gets written under the last release.",
    );
}

/// Exact equality rather than a set of `contains` checks: clap reverting to its own `version`
/// still prints something that looks like a version, and every substring assertion that would
/// catch it is one `amb::version` already makes without a process.
#[test]
fn the_binary_serves_the_library_banner() {
    let out = Board::new().run("alice", &["--version"]);
    assert_eq!(
        out.trim(),
        format!("amb {}", amb::version::banner()),
        "--version is not the banner the library assembled, so the build fingerprint never \
         reaches the one surface that reports it",
    );
}

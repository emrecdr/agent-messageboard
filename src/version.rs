//! This build's own identity: release, commit and schema.
//!
//! **Why a binary needs to be able to name itself: D56.**
//!
//! `cargo::rustc-env` from `build.rs` reaches every target in the package, so `env!` resolves
//! here exactly as it does in the binary — which is what lets this live in the library at all.

use crate::db::SCHEMA_VERSION;
use std::sync::LazyLock;

/// The SQLite build compiled into this binary.
///
/// **The fifth contract surface, and D56 named four.** The engine that stores every message and
/// every index row was invisible to every instrument here: absent from `--version`, from
/// `doctor`, and from `--json`. That is a gap this project is specifically exposed to. The worst
/// SQLite defect of 2026 — the WAL-reset bug, present from 3.7.0 and fixed in 3.51.3 — presents as
/// *a committed write that later transactions cannot see, with no error raised*, and it triggers
/// on several processes writing or checkpointing one WAL file at the same instant. That is this
/// tool's exact topology and its exact stated failure class.
///
/// Reported rather than asserted. A build is not refused for being old — the version is put where
/// a person and `doctor` can read it, so a regressed pin is visible instead of silent.
pub fn sqlite() -> &'static str {
    rusqlite::version()
}

/// `0.1.0 (b839c02 2026-08-28, schema 5, sqlite 3.53.2)` — the body of what `amb --version` prints.
///
/// A `LazyLock` rather than a `const`, because `concat!` takes literals only and a const form
/// would need [`SCHEMA_VERSION`] transcribed into a second string that can drift. It is also what
/// clap wants — a `&'static str` — without enabling clap's `string` feature, which would compile
/// an owned-string path across the whole of clap for one line of output.
static BANNER: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{} ({}, schema {}, sqlite {})",
        env!("CARGO_PKG_VERSION"),
        env!("AMB_BUILD_ID"),
        SCHEMA_VERSION,
        sqlite(),
    )
});

/// The version banner, for clap and for anything reporting which build it is.
pub fn banner() -> &'static str {
    &BANNER
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each field asserted positively and by name. A banner that silently lost one would still
    /// look like a version string, which is the failure this whole surface exists to prevent.
    #[test]
    fn the_banner_names_the_release_the_commit_and_the_schema() {
        let b = banner();
        assert!(
            b.starts_with(env!("CARGO_PKG_VERSION")),
            "banner does not lead with the release: {b:?}"
        );
        assert!(
            b.contains(&format!(", schema {SCHEMA_VERSION},")),
            "banner does not name the schema, so a binary that lags the board cannot say so: \
             {b:?}"
        );
        assert!(
            b.ends_with(&format!(", sqlite {})", sqlite())),
            "banner does not name the storage engine, so a regressed SQLite pin is invisible \
             — the failure mode being a committed write later reads cannot see: {b:?}"
        );

        // Against the stamp itself rather than a shape parsed back out of the banner: this
        // fails if the banner stops carrying what `build.rs` produced, which a hex-digit check
        // on a re-split substring only approximates.
        assert!(
            b.contains(env!("AMB_BUILD_ID")),
            "banner carries no commit, so two builds of one release are indistinguishable: {b:?}"
        );
    }

    /// The bundled engine is at or past the WAL-reset fix.
    ///
    /// **Not a restatement of the pin — a property with a reason.** The WAL-reset bug was present
    /// from SQLite 3.7.0 (2010) and fixed in **3.51.3** (2026-03-13). It corrupts exactly this
    /// tool's topology — several processes writing or checkpointing one WAL file at the same
    /// instant — and it presents as *a committed write that later transactions cannot see, with no
    /// error raised*. `CLAUDE.md`'s first convention is that this project's failures are silences;
    /// this is one, shipped by a dependency, and nothing else here would notice it.
    ///
    /// Asserted against the version actually compiled in, so a `libsqlite3-sys` downgrade reddens
    /// this rather than passing quietly.
    #[test]
    fn the_bundled_sqlite_is_past_the_wal_reset_fix() {
        /// 3.51.3, in SQLite's own `SQLITE_VERSION_NUMBER` encoding: major*1e6 + minor*1e3 + patch.
        const WAL_RESET_FIXED_IN: u32 = 3_051_003;

        let v = sqlite();
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        assert_eq!(
            parts.len(),
            3,
            "sqlite version is not major.minor.patch, so the floor below cannot be checked: {v:?}"
        );
        let n = parts[0] * 1_000_000 + parts[1] * 1_000 + parts[2];
        assert!(
            n >= WAL_RESET_FIXED_IN,
            "bundled sqlite {v} predates the WAL-reset fix in 3.51.3 — a committed write can \
             become invisible to later transactions with no error raised, which is precisely \
             this board's failure mode under concurrent processes"
        );
    }

    /// Guards `build.rs` falling back in a tree where git is available — which would leave every
    /// build reporting the same fingerprint. Would fail for a build from a source tarball; there
    /// is no remote to produce one, and this repository is the only place tests run.
    #[test]
    fn the_build_was_fingerprinted_from_this_repository() {
        assert!(
            !banner().contains("no git"),
            "build.rs could not read this repository: {:?}",
            banner()
        );
    }
}

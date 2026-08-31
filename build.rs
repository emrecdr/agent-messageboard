//! Stamps the binary with the commit it was built from.
//!
//! **Why the release number is not enough, and what the fingerprint is for: D56.** What follows
//! is how it is obtained.
//!
//! It emits one variable, `AMB_BUILD_ID`, already formatted. A single pre-rendered string rather
//! than separate sha and date fields, because there is no shape that suits both a git checkout
//! and a source tarball — the fallback has to be a whole string, not an empty field leaving
//! `amb 0.1.0 ( , schema 5)` behind.
//!
//! # Staying accurate
//!
//! A build script is re-run only when something it declared changes, so every input to the stamp
//! has to be declared or the stamp goes quietly stale — D56's own failure mode, one level up.
//!
//! `.git/HEAD` and the ref it points at cover committing and switching branches; `packed-refs`
//! covers a ref `git gc` has folded away. `src`, `Cargo.toml` and `Cargo.lock` cover the `dirty`
//! marker, which is a property of the working tree rather than of git's own files.
//!
//! **Declaring `src` is the expensive line, and it buys the `dirty` marker.** Cargo attaches
//! build-script output to the whole package, so touching `src/main.rs` re-runs this script and
//! dirties the library and all eight test binaries with it: `cargo test --no-run` goes from
//! 0.62-0.73 s to 3.08-3.31 s, about 5x on the inner loop (`MEASUREMENTS.md` M12). Without the watch the
//! marker would under-report — reporting clean for a binary built from uncommitted source, which
//! is the one thing it exists to catch — so the cost is the feature's price, not an oversight.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    for path in watch_list() {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    println!("cargo::rustc-env=AMB_BUILD_ID={}", build_id());
}

/// Everything whose change invalidates the stamp. Non-existent paths are dropped: cargo treats a
/// missing declared path as perpetually changed, which would re-run this script on every build.
fn watch_list() -> Vec<PathBuf> {
    let git = Path::new(".git");
    let mut paths = vec![
        PathBuf::from("src"),
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        git.join("HEAD"),
        git.join("packed-refs"),
    ];
    // `symbolic-ref` yields `refs/heads/main`, and fails on a detached HEAD — where there is no
    // ref to watch and `.git/HEAD` itself carries every change, so nothing is missed.
    if let Some(r) = git_cmd(&["symbolic-ref", "--quiet", "HEAD"]) {
        paths.push(git.join(r));
    }
    paths.retain(|p| p.exists());
    paths
}

/// `b839c02 2026-08-28`, plus ` dirty` when built over uncommitted tracked changes.
///
/// Falls back to `no git` rather than to an empty or invented value: a build from a source
/// tarball is a legitimate build, and a fingerprint that says it does not know is worth more than
/// one that guesses.
fn build_id() -> String {
    // One `git log` renders both fields, so there is no way for the sha and the date to describe
    // different commits, and no second process spawn to pay for.
    let Some(stamp) = git_cmd(&["log", "-1", "--abbrev=7", "--date=short", "--format=%h %cd"])
    else {
        return "no git".into();
    };
    // `--untracked-files=no` matches what `git describe --dirty` counts, so a scratch file in the
    // working directory does not mark an otherwise clean build.
    let dirty =
        git_cmd(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    format!("{stamp}{}", if dirty { " dirty" } else { "" })
}

/// Run `git`, returning trimmed stdout, or `None` for any failure at all — git missing, not a
/// repository, a non-zero exit. Every caller has a fallback for all of them.
fn git_cmd(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

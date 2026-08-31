//! Memory: a vault of markdown notes, and a disposable index over it.
//!
//! **The vault is truth; `board.db` holds only a derived index.** The test that keeps this
//! design out of the shape `DECISIONS.md` D2 rejected, and it must stay true: `rm board.db`
//! loses zero notes. Nothing here stores note *content* in SQLite — only what is needed to find
//! a file, order it, and judge how stale it is.
//!
//! # Why this is not a fourth database
//!
//! One board means one open path, one sync-root guard (D15) and one concurrency configuration.
//! The index tables arrive as migration 2 -> 3 and are purely additive, so a memory-unaware
//! binary still opens a memory-aware board — which matters because hooks invoke a *copy* of this
//! binary from `~/.claude/settings.json`, and a copy lags the tree it was built from.
//!
//! # Configuration is one environment variable, deliberately
//!
//! `amb` has no config file, and this layer is not what introduces one by accident. `AMB_VAULT`
//! names a directory the user already keeps notes in; **unset means memory is off**, which is
//! also the kill switch. There is no default, because a wrong default creates a directory
//! nobody asked for and starts filling it.
//!
//! # Shape
//!
//! The **pure core** is [`config`], [`id`], [`text`], [`redact`], [`note`] and [`inject`]: slugs,
//! redaction, frontmatter, ordering, the cap. No filesystem, no database, no environment, and
//! that is where the decisions are. The **shell** is everything below it.
//!
//! | Module | Holds |
//! |---|---|
//! | [`config`] | Kinds, force, lifecycle, thresholds, and the environment they read |
//! | [`id`] | [`NoteId`] and the indexed row shape (D50) |
//! | [`text`] | Slugs, ages, hashes, the civil calendar |
//! | [`redact`] | Named secret shapes, never an entropy threshold (D46) |
//! | [`note`] | The file: frontmatter, the derivation ledger, parsing |
//! | [`inject`] | Ordering, the cap, and saying what the cap hid (D24, D43) |
//! | [`index`] | Syncing the vault into SQLite and deriving links |
//! | [`query`] | The two retrieval lanes, search, id resolution (D42) |
//! | [`events`] | The ledger and the receipt over it (D59, D74) |
//! | [`write`] | `observe` and `supersede` — the only paths that author files |
//! | [`status`] | Capture health, unknown keys, coverage |
//! | [`promote`] | Phase 2: candidates, derivation, promotion (D49, D51) |
//! | [`export`] | Phase 3: publishing a decision into the repo it governs (D49) |
//! | [`capture`] | Phase 4: transcript facts and the fail-loud counter (D52) |
//! | [`topics`] | The middle rung: what a repository *is*, and what that scopes (D82) |
//!
//! **This is one module split into files, not fourteen modules.** The facade re-exports
//! everything, so every caller still writes `memory::observe`, and a submodule reaches its
//! siblings through `use super::*` exactly as it reached the rest of the file before. What the
//! split buys is that a test now sits beside the code it tests: the vault records a session
//! appending production code *after* the test module and moving the boundary without noticing,
//! which is a defect a 5,883-line file makes easy and fourteen files of 200–600 make hard.

use crate::claims;
use crate::error::{Error, Result, io, sql};
use crate::identity::Identity;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

mod capture;
mod config;
mod events;
mod export;
mod id;
mod index;
mod inject;
mod note;
mod promote;
mod query;
mod redact;
mod status;
mod text;
mod topics;
mod write;

pub use capture::*;
pub use config::*;
pub use events::*;
pub use export::*;
pub use id::*;
pub use index::*;
pub use inject::*;
pub use note::*;
pub use promote::*;
pub use query::*;
pub use redact::*;
pub use status::*;
pub use text::*;
pub use topics::*;
pub use write::*;

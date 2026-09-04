//! Database location, open path and schema.
//!
//! One SQLite file outside every repository (`DECISIONS.md` D15). The pragmas in
//! [`apply_pragmas`] are the configuration under which 17 concurrent processes sent 1,700
//! messages with **zero `SQLITE_BUSY` and zero lost** (`MEASUREMENTS.md` M1, corrected by M16) —
//! they are not decoration. M1's `msg/s` is a send-then-read loop rate and is not the claim here.

use crate::error::{Error, Result, io, sql};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Path fragments that indicate a file-sync or network volume.
///
/// SQLite's locking primitives are not reliably honoured on these, and the failure mode is
/// silent corruption rather than a visible error — so the guard refuses rather than warns (D15).
/// Verified 2026-08-27 that `$HOME` on the target machine is local APFS, so this is insurance
/// against a future move rather than a live problem.
const SYNC_ROOT_MARKERS: &[&str] = &[
    "/Mobile Documents/",
    "/Dropbox/",
    "/Google Drive/",
    "/GoogleDrive/",
    "/OneDrive/",
];

/// Resolve the database path. `AMB_DB` overrides it, which is what tests use so they never touch
/// the real board.
pub fn db_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("AMB_DB") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").map_err(|_| Error::NoIdentity)?;
    Ok(PathBuf::from(home)
        .join(".agent-messageboard")
        .join("board.db"))
}

/// Filesystem types that put the database on another host.
///
/// **Consulted only where the kernel has no direct answer** — Linux, which has no `MNT_LOCAL`.
/// macOS is asked rather than guessed (see [`volume_of`]), and a list is only ever the fallback.
///
/// **Conservatively short, and `fuse` is deliberately absent.** `sshfs` and `rclone` are FUSE and
/// are remote; so are `gocryptfs` and `ntfs-3g`, which are local. A blanket refusal on FUSE would
/// lock a user out of a perfectly good board, and D28 already records that a false positive here
/// costs more than a missed detection: it takes away something that was working.
const NETWORK_FSTYPES: &[&str] = &[
    "nfs", "nfs4", "smb", "smb2", "smbfs", "cifs", "afs", "afpfs", "9p", "ncpfs", "coda",
];

/// Whether a volume is on another host — the decision, with the syscall taken out.
///
/// **Two authorities, and the kernel's wins.** macOS answers directly with `MNT_LOCAL`, which is
/// true for every locally-stored filesystem and false for every remote one, so `mnt_local` is
/// `Some` there and the type name is only used for the error message. Linux has no equivalent
/// flag, so `mnt_local` is `None` and the type name decides.
///
/// Separated out because the syscall half cannot be tested — this project cannot mount NFS in a
/// unit test, and a guard with no test that it fires is the thing CLAUDE.md warns about. The
/// decision *can* be tested exhaustively, so it is the part that holds the logic.
pub fn is_remote_volume(fstype: &str, mnt_local: Option<bool>) -> bool {
    match mnt_local {
        Some(local) => !local,
        None => NETWORK_FSTYPES
            .iter()
            .any(|n| fstype.eq_ignore_ascii_case(n)),
    }
}

/// The nearest ancestor of `path` that exists, canonicalised.
///
/// **`statfs` needs a path that exists and the board usually does not yet**, because this runs
/// before the file is created. A file's volume is its parent directory's volume, so walking up
/// asks the same question of the same filesystem.
///
/// **Canonicalising is what closes the symlink hole**, and is the reason this no longer checks the
/// path as given. `~/board.db` symlinked into `~/Dropbox/` passes every substring test in
/// [`SYNC_ROOT_MARKERS`] and lands on Dropbox regardless. The previous comment here said the
/// canonical form was avoided because the file may not exist — true of the file, false of its
/// parent, and the distinction is the whole fix.
fn nearest_existing(path: &Path) -> Option<std::path::PathBuf> {
    let mut p = path;
    loop {
        if let Ok(real) = p.canonicalize() {
            return Some(real);
        }
        p = p.parent()?;
    }
}

/// Ask the kernel what volume `path` is on: its filesystem type, and whether it is local.
///
/// Returns `None` when the question cannot be asked — an unsupported platform, or a `statfs` that
/// failed. **`None` means "no answer", never "local"**, and [`guard_location`] treats it as
/// permission to continue: refusing a board because a syscall failed would take a working tool
/// away on the strength of no evidence.
#[cfg(target_os = "macos")]
fn volume_of(path: &Path) -> Option<(String, Option<bool>)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path that outlives the call, and `buf` is an owned,
    // correctly-sized `statfs` for the kernel to fill. Failure is reported by the return value,
    // and `buf` is only read when it reports success.
    if unsafe { libc::statfs(c.as_ptr(), &mut buf) } != 0 {
        return None;
    }
    // SAFETY: on success the kernel writes a NUL-terminated type name into this fixed array.
    let name = unsafe { std::ffi::CStr::from_ptr(buf.f_fstypename.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Some((name, Some(statfs_is_local(buf.f_flags))))
}

/// Whether a `statfs` flag word marks the volume local.
///
/// Pure and separate because no test can mount a network volume: inline, the `&` isolating
/// `MNT_LOCAL` could become `|` or `^` — both read every remote volume as local, waving the
/// board past D28's guard — and nothing could redden (M46). Here the word is synthetic and the
/// boundary is one bit away.
#[cfg(target_os = "macos")]
fn statfs_is_local(f_flags: u32) -> bool {
    f_flags & libc::MNT_LOCAL as u32 != 0
}

#[cfg(target_os = "linux")]
fn volume_of(path: &Path) -> Option<(String, Option<bool>)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::statfs(c.as_ptr(), &mut buf) } != 0 {
        return None;
    }
    // No `MNT_LOCAL` on Linux, so the name carries the decision and the flag is `None`.
    Some((fstype_name(buf.f_type as i64), None))
}

/// Map a Linux `statfs.f_type` magic to the name [`NETWORK_FSTYPES`] uses.
///
/// Unknown magics come back as hex rather than as a guess, so an unrecognised filesystem reads as
/// "not on the list" and a board on it opens.
#[cfg(target_os = "linux")]
fn fstype_name(magic: i64) -> String {
    match magic {
        0x6969 => "nfs".to_string(),
        0x517b => "smb".to_string(),
        0xfe53_4d42 => "smb2".to_string(),
        0xff53_4d42 => "cifs".to_string(),
        0x5346_414f => "afs".to_string(),
        0x0102_1997 => "9p".to_string(),
        0x564c => "ncpfs".to_string(),
        0x7373 => "coda".to_string(),
        other => format!("0x{other:x}"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn volume_of(_path: &Path) -> Option<(String, Option<bool>)> {
    None
}

/// Refuse a database path on a synced or network volume.
///
/// **Two guards, and the second is the real one.** [`SYNC_ROOT_MARKERS`] is a fast path that names
/// the product in the error — "it is inside a synced volume (Dropbox)" is a more useful sentence
/// than a filesystem type — but a substring list can only ever recognise default folder names.
/// [`volume_of`] asks the kernel, which is what makes D15's sentence about network mounts true:
/// before this, that clause described nothing in the code, and an SMB board opened without a word.
///
/// **Both are asked of the resolved path**, so a symlink into a sync root or onto a share is
/// caught. The marker test is also applied to the path *as written*, because a user who typed
/// `~/Dropbox/board.db` should be told about Dropbox even if it is not yet a real directory.
pub fn guard_location(path: &Path) -> Result<()> {
    let resolved = nearest_existing(path);
    let volume = resolved.as_deref().and_then(volume_of);
    let resolved = resolved.as_ref().map(|p| p.to_string_lossy());
    location_verdict(
        &path.to_string_lossy(),
        resolved.as_deref(),
        volume.as_ref().map(|(f, l)| (f.as_str(), *l)),
    )
}

/// The whole location decision, with the syscall and the filesystem taken out.
///
/// **[`is_remote_volume`] was already extracted for this reason and covered only half of it.** The
/// marker check, the precedence between the two refusals, and — the one that matters — *whether
/// the kernel's answer is consulted at all* stayed in the shell, where a test could reach them
/// only by mounting a share.
///
/// **That gap was measured, not suspected** (M22). Two mutations to the shell survived all 430
/// tests: making `statfs` always fail, which disables the remote guard entirely, and replacing
/// `mnt_local` with `None` at the call site, which drops macOS's `MNT_LOCAL` authority and hands
/// the whole decision to a ten-name list. Neither reddened anything. The suite noticed only when
/// the guard was made to refuse *everything*, and that is every other test needing a board to
/// open — a canary, not an assertion.
///
/// `resolved` is `None` when no ancestor of the path exists; `volume` is `None` when the kernel
/// could not be asked. **Both mean "no answer" and both permit the board** (D28): refusing on the
/// strength of no evidence takes away a tool that was working.
pub fn location_verdict(
    as_written: &str,
    resolved: Option<&str>,
    volume: Option<(&str, Option<bool>)>,
) -> Result<()> {
    let named = |s: &str| {
        SYNC_ROOT_MARKERS
            .iter()
            .find(|m| s.contains(**m))
            .map(|m| m.trim_matches('/').to_string())
    };
    // **The marker is checked first, and against the path as written.** "It is inside a synced
    // volume (Dropbox)" is a more useful sentence than a filesystem type, and a user who typed
    // `~/Dropbox/board.db` should be told about Dropbox even if it is not yet a real directory.
    if let Some(marker) = named(as_written).or_else(|| resolved.and_then(named)) {
        return Err(Error::SyncedVolume {
            path: as_written.to_string(),
            marker,
        });
    }
    if let Some((fstype, mnt_local)) = volume
        && is_remote_volume(fstype, mnt_local)
    {
        return Err(Error::RemoteVolume {
            path: as_written.to_string(),
            fstype: fstype.to_string(),
        });
    }
    Ok(())
}

/// Open the board: guard the location, create the directory, apply pragmas, ensure the schema.
pub fn open() -> Result<Connection> {
    open_at(&db_path()?)
}

/// Explains the directory to whoever finds it.
///
/// `DESIGN.md` requires this: the data lives outside every repository while the protocol is
/// documented inside them, so without a note here the file is an unexplained database in a
/// home directory.
const SIBLING_README: &str = "\
# agent-messageboard data

`board.db` is coordination state for concurrent coding-agent sessions on this machine: messages
between sessions, and advisory file claims.

**It is ephemeral and safe to delete.** Claims expire; unread messages are re-offered. Nothing
here is a record worth keeping — architectural decisions and findings live in the repositories
they govern, deliberately (see the project's DECISIONS.md, D2).

**Never commit it anywhere.** A lease in git history is worse than worthless, because it reads
as current.

Created by `amb`. Remove its hooks with `amb uninstall`.
";

/// Open a specific path. Separate from [`open`] so tests can point at a temporary file without
/// reaching through the environment.
///
/// **Keeps the query planner's statistics current, and only this path does** (D118). Before it,
/// `ANALYZE` had never run on any board: the live one carried no `sqlite_stat1` table at all, so
/// every plan since the project began was chosen from defaults. SQLite's own guidance names this
/// architecture — *"applications with short-lived database connections should run `PRAGMA
/// optimize` once, just prior to closing each database connection"* — and every `amb` invocation
/// is a short-lived connection.
///
/// **Not before closing, and not on the hook path, for two separate reasons.**
///
/// Plain `PRAGMA optimize` only considers tables *this connection has already used*, so at open
/// it is a guaranteed no-op; `0x10002` is the documented mask that makes it examine every table
/// instead. Running it at close would be the faithful form and costs a structural change — the
/// connection is dropped implicitly at the end of a 1,300-line match with many early returns —
/// which is not worth it for a gain nothing has measured.
///
/// The hook path is excluded because `optimize` *writes*: when statistics are stale it runs
/// `ANALYZE`, which takes the write lock. D30 measured twelve concurrent processes contending on
/// a single first open, hooks get a 2 s budget against the platform's 5 s kill (see
/// [`HOOK_BUSY_TIMEOUT_MS`]), and D9 forbids the one ending where the platform kills us mid-wait.
/// Interactive commands run often enough — `inbox`, `claims`, `send` — to keep statistics fresh
/// without ever putting a write on the lane that must not stall.
///
/// Best effort in the strongest sense: the result is discarded. A board that cannot be analysed
/// is a board that still works, and this must never be the thing that fails an open.
pub fn open_at(path: &Path) -> Result<Connection> {
    let conn = open_at_with(path, INTERACTIVE_BUSY_TIMEOUT_MS)?;
    let _ = conn.execute_batch("PRAGMA optimize=0x10002;");
    Ok(conn)
}

/// How long an *interactive* open may wait on a busy board, in milliseconds.
///
/// Chosen for D30's first-open stampede: converting a fresh file to WAL takes a brief exclusive
/// lock, twelve concurrent processes contend, and a human watching a prompt would rather wait
/// than see "database is locked". A hook cannot afford this value — see
/// [`HOOK_BUSY_TIMEOUT_MS`].
pub const INTERACTIVE_BUSY_TIMEOUT_MS: u64 = 30_000;

/// How long a *hook's* open may wait, in milliseconds — and it must be less than the budget.
///
/// The platform gives a hook entry 5 s of wall clock and then kills it. `busy_timeout` was one
/// value for both callers, so under contention a hook could sit parked in a 30 s wait while its
/// own budget lapsed — terminated mid-wait by the platform rather than exiting 0 on its own
/// terms, which is the one ending D9 forbids. The wait has to be inside the open itself, not
/// applied after: `migrate` runs during the open and takes the write lock, so an override set
/// on the returned connection would arrive after the stall it exists to bound.
///
/// 2 s leaves most of the budget for the actual work. A lock still held after 2 s means another
/// process is mid-migration; this event's delivery is lost and the next one finds a current
/// board — losing one beat is recoverable, being killed is not ours to handle. Asserted against
/// the hook budget by a `const` assertion beside `HOOK_TIMEOUT_SECS` in `hooks.rs`.
pub const HOOK_BUSY_TIMEOUT_MS: u64 = 2_000;

/// [`open_at`], with the wait budget a hook can actually afford.
pub fn open_at_for_hook(path: &Path) -> Result<Connection> {
    open_at_with(path, HOOK_BUSY_TIMEOUT_MS)
}

fn open_at_with(path: &Path, busy_ms: u64) -> Result<Connection> {
    guard_location(path)?;

    // Whether *we* brought the containing directory into existence. Load-bearing: it decides
    // whether [`restrict`] may narrow its permissions. See that function for why.
    let mut ours = false;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        ours = !parent.exists();
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
        // Written once, never overwritten: if someone has edited it, that is their note now.
        let readme = parent.join("README.md");
        if !readme.exists() {
            let _ = std::fs::write(&readme, SIBLING_README);
        }
    }
    let mut conn = Connection::open(path).map_err(sql(format!("opening {}", path.display())))?;
    apply_pragmas(&conn, busy_ms).map_err(|e| corruption_hint(e, path))?;
    migrate(&mut conn, path).map_err(|e| corruption_hint(e, path))?;
    restrict(path, ours);
    Ok(conn)
}

/// Rewrite a corruption-shaped failure into the one error that says what to do.
///
/// "file is not a database" is accurate and stops one sentence short of the fix, at the exact
/// moment someone needs it (U9): the board is disposable (D15), and nothing else in the message
/// said so. Only genuine corruption codes are rewritten — a locked or busy board keeps its own
/// message, because "move the file aside" is destructive advice against a database that is
/// merely in use.
fn corruption_hint(e: Error, path: &Path) -> Error {
    use rusqlite::ErrorCode::{DatabaseCorrupt, NotADatabase};
    if let Error::Sqlite {
        source: rusqlite::Error::SqliteFailure(f, _),
        ..
    } = &e
        && matches!(f.code, DatabaseCorrupt | NotADatabase)
    {
        return Error::CorruptBoard {
            path: path.display().to_string(),
        };
    }
    e
}

/// SQLite's own consistency check, reduced to what a caller can act on: `None` means healthy.
///
/// `quick_check(1)` — the first finding is enough, because the response to any finding is the
/// same (D15: the board is disposable) and a full enumeration on a corrupt file can be slow
/// exactly when the answer is already known.
pub fn quick_check(conn: &Connection) -> Result<Option<String>> {
    let verdict: String = conn
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(sql("running quick_check"))?;
    Ok(if verdict == "ok" { None } else { Some(verdict) })
}

/// Record that something happened. Never fails a caller — a counter that could break the thing it
/// measures would be worse than no counter.
///
/// **Board infrastructure rather than a memory detail**, which is where it moved when a second,
/// non-memory consumer arrived (`amb snapshot`, D61). A `memory::bump` called from a bus command
/// is the kind of muddle that gets "helpfully" rearranged later by someone who cannot tell whether
/// the coupling was meant.
///
/// The table keeps the name `memory_counters`. Renaming it would cost a migration for no
/// behavioural gain, which is the same trade `agents.cwd` already took after D20 renamed the
/// concept but not the column.
pub fn bump(conn: &Connection, name: &str, at: f64) {
    let _ = conn.execute(
        "INSERT INTO memory_counters (name, count, last_at) VALUES (?1, 1, ?2)
         ON CONFLICT(name) DO UPDATE SET count = count + 1, last_at = ?2",
        rusqlite::params![name, at],
    );
}

/// Read a counter. Absent is zero — a counter that has never fired and one that does not exist
/// are the same fact to every caller.
pub fn counter(conn: &Connection, name: &str) -> i64 {
    conn.query_row(
        "SELECT count FROM memory_counters WHERE name = ?1",
        rusqlite::params![name],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// The board size at which D83 says pruning gets built.
///
/// **D83 wrote the number down so it would be a threshold rather than an intention.** It was
/// unreadable for two days regardless: `doctor` printed the board's *path* and never its size, so
/// nothing on this machine could say whether 50 MB was near or far. That is exactly D95's shape,
/// and D95's rule is that a stated condition nothing can evaluate is **worse** than no condition —
/// an absent threshold makes the next reader check, a dead one makes them trust.
/// [`crate::doctor::size_check`] is what evaluates it now (M34).
pub const PRUNE_AT_BYTES: u64 = 50 * 1024 * 1024;

/// The schema version this binary expects.
///
/// Equal to `MIGRATIONS.len()`, asserted by a test rather than computed, so that bumping one
/// without the other is caught rather than silently accepted.
pub const SCHEMA_VERSION: i64 = 13;

/// Migrations, applied in order from whatever version the board is already at.
///
/// **Entry `i` moves a board from version `i` to version `i + 1`. Never remove or reorder one** —
/// the position *is* the version number, so an edit here silently re-points every board that has
/// already run it.
///
/// Version 0 is a board created before versioning existed, or a file that does not exist yet.
/// Both are served by the same baseline, because every statement in `schema.sql` is
/// `IF NOT EXISTS`.
const MIGRATIONS: &[&str] = &[
    // 0 -> 1 · baseline
    include_str!("schema.sql"),
    // 1 -> 2 · offer counting moves to `reads`, where delivery actually happens.
    //
    // `messages.attempts` counted offers per *message* while a message is offered per
    // *recipient*: a broadcast to five agents advanced it five times every turn, so the
    // dead-letter threshold D6 asks for would have silenced it **for everyone** because one
    // agent never acknowledged it. That is precisely the property D17 exists to protect — one
    // row, consumed independently by each reader — so the counter belongs beside the read state,
    // not beside the message (D23).
    //
    // `DROP COLUMN` rather than a table rebuild: neither column is indexed or constrained, so
    // SQLite rewrites in place. A rebuild would have to drop `messages`, and `reads` references
    // it `ON DELETE CASCADE` — verified to wipe every row of read state.
    "ALTER TABLE reads ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE messages DROP COLUMN attempts;
     ALTER TABLE messages DROP COLUMN failed_at;",
    // 2 -> 3 · the memory index (`MEMORY-DESIGN.md` #5).
    //
    // **Purely additive, and that is the precondition this ladder states for itself.** Three new
    // tables, nothing dropped, nothing altered — so a board written by a memory-aware binary is
    // opened without complaint by the memory-unaware ones still running during a rollout. That
    // matters more here than for 1 -> 2: hooks invoke whatever binary `~/.claude/settings.json`
    // names, which is a *copy*, and a copy lags the tree it was built from.
    //
    // **The vault is the truth and this is only an index.** Every column here is either an
    // identity, a retrieval key, or a display convenience. Note *content* is deliberately absent:
    // `rm board.db` must lose zero notes, and it cannot lose what was never stored (D34).
    "CREATE TABLE notes (
       slug          TEXT NOT NULL,
       kind          TEXT NOT NULL,
       -- NOT NULL, and patterns carry '' rather than NULL. SQLite does not compare NULLs as
       -- equal, and — unlike every other SQL engine — permits them in a PRIMARY KEY, so a
       -- nullable column here would let two rows share a key and silently defeat the upsert.
       project       TEXT NOT NULL,
       vault_path    TEXT NOT NULL,
       title         TEXT NOT NULL,
       status        TEXT NOT NULL,
       created       REAL NOT NULL,
       derived_count INTEGER NOT NULL DEFAULT 0,
       body_excerpt  TEXT,
       content_hash  TEXT NOT NULL,
       mtime         REAL NOT NULL,
       indexed_at    REAL NOT NULL,
       PRIMARY KEY (kind, project, slug)
     );
     CREATE INDEX ix_notes_project ON notes(project, kind, status);

     CREATE TABLE note_paths (
       kind      TEXT NOT NULL,
       slug      TEXT NOT NULL,
       project   TEXT NOT NULL,
       path_glob TEXT NOT NULL,
       PRIMARY KEY (kind, project, slug, path_glob),
       FOREIGN KEY (kind, project, slug) REFERENCES notes(kind, project, slug) ON DELETE CASCADE
     );
     CREATE INDEX ix_note_path ON note_paths(path_glob);

     -- Deliberately carries NO foreign key to `notes`, unlike `note_paths` above. It records
     -- what a *session* was shown, and that measurement stays true after the note it names is
     -- deleted from the vault. A cascade here would quietly delete the evidence that Phase 1
     -- works every time a note was retired, which is the opposite of what a ledger is for.
     CREATE TABLE note_events (
       session TEXT NOT NULL,
       kind    TEXT NOT NULL,
       project TEXT NOT NULL,
       slug    TEXT NOT NULL,
       event   TEXT NOT NULL,
       ts      REAL NOT NULL,
       PRIMARY KEY (session, kind, project, slug, event)
     );
     CREATE INDEX ix_note_events_slug ON note_events(kind, project, slug, event);",
    // 3 -> 4 · offer counting for claim conflicts (D44).
    //
    // The third delivery path finally gets what the other two already had. D23 counts mail offers
    // in `reads.attempts`; D19 makes `PostToolUse` silent on a renewed claim. The `Stop` sweep had
    // neither, so a conflict with a session that had already ended was re-injected at every turn
    // boundary until its four-hour lease ran out.
    //
    // Keyed on the holder's `taken_at`, which `claims::take` leaves alone when a claim is merely
    // extended and replaces when one is released and taken afresh. So renewing is not news and
    // re-taking is.
    "CREATE TABLE claim_notices (
       agent    TEXT NOT NULL,
       path     TEXT NOT NULL,
       holder   TEXT NOT NULL,
       taken_at REAL NOT NULL,
       count    INTEGER NOT NULL DEFAULT 0,
       last_at  REAL NOT NULL,
       PRIMARY KEY (agent, path, holder)
     );",
    // 4 -> 5 · the counters the plan's "What each phase must measure" table asks for.
    //
    // **Two of the four receipts are derivable and two are not.** Whether candidates reach the
    // threshold, and the decline rate, are both properties of the vault — the candidate files
    // record `derived_count`, `declined_after` and `promoted_to`, so counting them needs no
    // storage. But "did `export --check` ever fire" and "was the cross-repo query ever run" are
    // *events*: they leave no trace in any file, and a feature nobody uses looks identical to one
    // that is quietly working.
    //
    // Deliberately not `note_events`: these are not about a note, and giving them a synthetic slug
    // to fit that table would make both harder to read.
    "CREATE TABLE memory_counters (
       name    TEXT PRIMARY KEY,
       count   INTEGER NOT NULL DEFAULT 0,
       last_at REAL NOT NULL
     );",
    // 5 -> 6 · the one edge the vault already has, made queryable.
    //
    // **`amb` has exactly one relationship between notes and cannot traverse it.** `superseded_by`
    // is written into frontmatter and shows up in the index only as `status = 'superseded'`, so
    // nothing can answer "what replaced this" or "what did this replace" without reading files.
    // That is the gap — not an absence of link *types*, of which the vault needs far fewer than
    // the published vocabularies offer.
    //
    // **Derived, and therefore consistent with D34.** The file stays truth; this is rebuilt from
    // frontmatter by `reindex`, exactly like `status` and `derived_count` already are. `rm
    // board.db` still loses zero notes. The row carries the *target* as text rather than as a
    // foreign key, because a target may legitimately not exist yet — or ever, if a note was
    // deleted — and the dangling case is something to *report* (D63's validation) rather than
    // something to make unrepresentable.
    "CREATE TABLE note_links (
       kind        TEXT NOT NULL,
       project     TEXT NOT NULL,
       slug        TEXT NOT NULL,
       rel         TEXT NOT NULL,   -- `supersedes`, and nothing else until something consumes more
       target      TEXT NOT NULL,   -- a note id as written: `project/slug`, or `kind/slug`
       PRIMARY KEY (kind, project, slug, rel, target)
     );
     CREATE INDEX ix_note_links_target ON note_links(target, rel);",
    // 6 -> 7 · force, and the instrumentation that makes it evaluable.
    //
    // **The consumer exists before the field does, which is why this ships now.** `MAX_INJECTED`
    // is 8 and the vault holds more, so the live injection reports "8 of 13 ... and 5 more" —
    // five notes are dropped every session and *recency is currently the only reason* one
    // survives. That is a ranking decision made with no input from how much the note matters.
    //
    // **`note_events.force` records force at the moment of the event, rather than being joined
    // from `notes` at read time.** A note's force can change; joining would silently re-attribute
    // every historical injection to its current level, so "are rules cited more than advice"
    // would be answered about a past that never happened. The column is denormalised on purpose,
    // and this is the reason.
    "ALTER TABLE notes ADD COLUMN force TEXT NOT NULL DEFAULT 'advice';
     ALTER TABLE note_events ADD COLUMN force TEXT NOT NULL DEFAULT 'advice';
     UPDATE notes SET content_hash = '';",
    // 7 -> 8 · repair the previous migration, which invalidated the wrong column.
    //
    // **The rule this encodes: invalidate the *gate*, not the derived value.** Migration 6 -> 7
    // cleared `content_hash` to force every note to be re-derived. It never happened. `sync_dir`
    // gates on `mtime` and returns before `content_hash` is read at all, so a file whose mtime
    // had not changed was skipped and its cleared column stayed cleared — a migration that
    // suppressed its own repair. Observed on a real board: `14 scanned · 0 indexed · 14
    // unchanged`, fourteen empty hashes, and `note_links` never rebuilt, for a whole day.
    //
    // This is D63's shape a second time. There, `supersede` hand-updated `mtime` after writing a
    // file and so suppressed the reindex that would have corrected it. Same gate, same silence,
    // opposite direction — which is why the lesson is written here rather than in a commit
    // message.
    //
    // Zeroing `mtime` makes every note look newer on disk than the index believes, so the next
    // pass re-reads and re-derives all of it: `content_hash`, `note_paths` and `note_links`
    // together. One extra full scan, once, and only for boards that already exist.
    "UPDATE notes SET mtime = 0;",
    // 8 -> 9 · scope becomes an axis, and `kind` goes back to meaning one thing (D81).
    //
    // **`project` becomes `scope`, and `pattern` stops being a kind.** A pattern was always a
    // decision that applied everywhere; the kind was carrying the scope because there were exactly
    // two of them, and it broke on the third.
    //
    // **The derived tables are dropped rather than altered, and that is the design's own claim
    // being cashed in.** D34 says the vault is truth and `rm board.db` loses zero notes, so
    // dropping `notes`, `note_paths` and `note_links` loses nothing either — the next `sync_dir`
    // rebuilds all three from the files. An `ALTER` here would be more code, more ways to be
    // subtly wrong about a composite primary key, and a claim that the index held something the
    // vault did not.
    //
    // **`note_events` is rebuilt rather than dropped, because it is not derived.** It is the
    // ledger D59's withdrawal condition reads, and no file anywhere records that a session was
    // shown a note. Dropping it would destroy the measurement, so the rows are carried across
    // with the two renames applied: `project` becomes `scope`, and a `pattern` event becomes a
    // `decision` at `@@` — which is what it always was.
    //
    // **Rebuilt, not `ALTER`ed with the old column left behind.** Adding `scope` beside `project`
    // would leave a column nothing reads, which is this codebase's recurring defect (D23, D39,
    // D45) in the one place `tools/find_unread_fields.py` cannot see it — the script checks Rust
    // struct fields, and a dead SQL column is invisible to it.
    //
    // **`INSERT OR IGNORE`, because the primary key narrows.** The old key carried `project` and
    // the new one carries `scope`, and every pattern row's project collapses to `@@`: two pattern
    // events for one session and slug that differed only in a project column patterns never
    // populated would now collide. There were none, and the `OR IGNORE` is there so that if there
    // ever were, the migration drops a duplicate rather than failing every session's next hook.
    "DROP TABLE IF EXISTS note_paths;
     DROP TABLE IF EXISTS note_links;
     DROP TABLE IF EXISTS notes;

     CREATE TABLE notes (
       slug          TEXT NOT NULL,
       kind          TEXT NOT NULL,
       -- NOT NULL, and a candidate carries '' rather than NULL. SQLite does not compare NULLs as
       -- equal, and — unlike every other SQL engine — permits them in a PRIMARY KEY, so a
       -- nullable column here would let two rows share a key and silently defeat the upsert.
       -- The value is `address::Scope`'s stored form: a bare project id, '#topic', or '@@'.
       scope         TEXT NOT NULL,
       vault_path    TEXT NOT NULL,
       title         TEXT NOT NULL,
       status        TEXT NOT NULL,
       created       REAL NOT NULL,
       derived_count INTEGER NOT NULL DEFAULT 0,
       force         TEXT NOT NULL DEFAULT 'advice',
       body_excerpt  TEXT,
       content_hash  TEXT NOT NULL,
       mtime         REAL NOT NULL,
       indexed_at    REAL NOT NULL,
       PRIMARY KEY (kind, scope, slug)
     );
     CREATE INDEX ix_notes_scope ON notes(scope, kind, status);

     CREATE TABLE note_paths (
       kind      TEXT NOT NULL,
       slug      TEXT NOT NULL,
       scope     TEXT NOT NULL,
       path_glob TEXT NOT NULL,
       PRIMARY KEY (kind, scope, slug, path_glob),
       FOREIGN KEY (kind, scope, slug) REFERENCES notes(kind, scope, slug) ON DELETE CASCADE
     );
     CREATE INDEX ix_note_path ON note_paths(path_glob);

     CREATE TABLE note_links (
       kind    TEXT NOT NULL,
       scope   TEXT NOT NULL,
       slug    TEXT NOT NULL,
       rel     TEXT NOT NULL,
       target  TEXT NOT NULL,
       PRIMARY KEY (kind, scope, slug, rel, target)
     );
     CREATE INDEX ix_note_links_target ON note_links(target, rel);

     CREATE TABLE note_events_new (
       session TEXT NOT NULL,
       kind    TEXT NOT NULL,
       scope   TEXT NOT NULL,
       slug    TEXT NOT NULL,
       event   TEXT NOT NULL,
       ts      REAL NOT NULL,
       force   TEXT NOT NULL DEFAULT 'advice',
       PRIMARY KEY (session, kind, scope, slug, event)
     );
     INSERT OR IGNORE INTO note_events_new (session, kind, scope, slug, event, ts, force)
       SELECT session,
              CASE kind WHEN 'pattern' THEN 'decision' ELSE kind END,
              CASE kind WHEN 'pattern' THEN '@@' ELSE project END,
              slug, event, ts, force
         FROM note_events;
     DROP TABLE note_events;
     ALTER TABLE note_events_new RENAME TO note_events;
     CREATE INDEX ix_note_events_slug ON note_events(kind, scope, slug, event);",
    // 9 -> 10 · drop `notes.content_hash`, which had a writer and no reader (D85).
    //
    // **The defect D23, D39 and D45 each record, in the one shape `find_unread_fields.py` cannot
    // see**: that script scans Rust struct fields, and this is an SQL column. It was written on
    // every index pass and consulted by nothing — not `export --check`, which hashes file content
    // at read time and never touches the column, and not the skip, which compares `mtime` alone.
    //
    // **The second stage it might have become cannot pay for itself, and that is structural
    // rather than a matter of rate.** Confirming a change by hash requires *reading the file*,
    // which is precisely what the `mtime` gate exists to avoid; once the file is read, parsing it
    // is the next thing anyway. So the stage could only ever save the handful of writes after a
    // read that already happened. Q12 asked for a fortnight of `unchanged` against `indexed` to
    // decide this; the measurement cannot change the answer.
    //
    // `text::content_hash` the *function* stays — `export --check` compares content hashes rather
    // than timestamps, and that is D49's promise.
    "ALTER TABLE notes DROP COLUMN content_hash;",
    // 10 -> 11 · the measurement window gets somewhere to live (D87).
    //
    // **D59 named a withdrawal condition, D79 named when its clock starts, and nothing anywhere
    // could say whether it had.** `receipt` already took a `since`, but the only way to supply one
    // was `amb memory status --days N` — an integer count of days back from *now*, so the window
    // slid forward daily and could not express "since a fixed instant". The default was `None`,
    // meaning all time. So the number a reader saw was computed over a corpus D79 had explicitly
    // excluded, including a hand-run probe session that could never cite anything.
    //
    // This is the shape D54 records and D58 names: a condition written in prose and computed
    // nowhere. The receipt is the one instrument here whose being wrong *retires a feature*, so
    // it is the last place a documented rule should exist only as documentation.
    //
    // **On the board rather than in the vault, and that is deliberate.** D15 makes the board
    // disposable and D34 makes the vault the truth — but `note_events` is already board-only and
    // cannot be rebuilt from anything, and the window is meaningless without the ledger it
    // windows. Storing it beside the thing it describes means `rm board.db` loses a measurement
    // that was already lost, rather than leaving a start date pointing at events that are gone.
    //
    // Named rather than implicit: `OPEN-QUESTIONS.md` Q10 says its arm "gets its own date and its
    // own decision", so a second window is a documented future need and not speculation.
    "CREATE TABLE measurement_window (
       name      TEXT PRIMARY KEY,
       opened_at REAL NOT NULL
     );",
    // 11 -> 12 · a search that finds nothing leaves a trace (D89).
    //
    // **The receipt could not tell a miss from an absence, and one of those retires a feature.**
    // `note_events` records `injected`, `injected_file` and `cited`; nothing recorded that recall
    // ran. So `unprompted: 0` — a note used without having been shown, reachable through
    // `recall` — meant *either* "no session wanted a note it had not been seen" *or* "sessions
    // asked and the search lost the answer". D88 proves the second was happening: `recall` matched
    // a 240-character excerpt, so a lesson in a note's second paragraph answered `no notes match`.
    //
    // **`INTEGER PRIMARY KEY`, which deduplicates nothing, and that is the whole design.** The
    // obvious cheaper move — a sentinel row in `note_events` — inherits
    // `PRIMARY KEY (session, kind, scope, slug, event)`, so five searches in one session would
    // record one row. That is exactly the failure CLAUDE.md's second question names: a
    // denominator counting *distinct things* rather than *times the cost was paid*, which
    // understates the cost while the numerator is untouched and improves the ratio for free.
    // Here the cost is one search, so one search is one row.
    //
    // **`ts`, so D87's window can scope it.** A `memory_counters` bump is smaller and wrong: it
    // is monotonic, `bump` overwrites, and a number that cannot be windowed cannot be read
    // against the window the verdict is computed over.
    //
    // **No query text.** The two questions the receipt asks are "was recall reached for" and
    // "did it find anything", and `lane` and `hits` answer both. A query is agent-written text
    // that can carry a secret; the vault redacts for that reason, and collecting search terms
    // would add a surface to protect in order to answer a question nobody asked.
    // `foreign` is the cross-repo differentiator, counted where it actually happens. The
    // existing `cross_repo_query` counter is bumped only by `recall --file --across-repos` — a
    // flag that appears in `DECISIONS.md` and `OPEN-QUESTIONS.md` and in no README, no primer and
    // no banner, so no agent and no reader has ever been told it exists. Meanwhile `across_repos`
    // *calls* `concerning` and only re-sorts it, so plain `--file` already returns foreign notes.
    // The counter therefore measured "did anyone use this undocumented flag" while `status`
    // printed it as "the differentiator is dead weight" — one unit of the denominator and the
    // claim being made were not the same sentence, which is question 1 of CLAUDE.md's rule.
    "CREATE TABLE searches (
       id      INTEGER PRIMARY KEY,
       session TEXT NOT NULL,
       ts      REAL NOT NULL,
       lane    TEXT NOT NULL,
       hits    INTEGER NOT NULL,
       foreign_hits INTEGER NOT NULL DEFAULT 0
     );
     CREATE INDEX ix_searches_ts ON searches(ts);",
    // 12 -> 13 · the index sync's per-file probe gets an index (audit round two).
    //
    // `sync_dir` asks `SELECT mtime FROM notes WHERE kind = ?1 AND vault_path = ?2` once per
    // markdown file it scans, on the `SessionStart` hook path (the string lives once, as
    // `SYNC_PROBE_SQL` beside `sync_dir`, and the plan test asserts that copy). The primary key is
    // `(kind, scope, slug)`, so that probe seeks on `kind` and then walks every note of the kind
    // to match `vault_path` — per file, so the pass is quadratic in vault size. Measured on a
    // synthetic 5,000-note index: one 500-file hook pass costs 177 ms unindexed and 8 ms with
    // this index (measured with the probe still re-prepared per file; `sync_dir` has since
    // cached the statement, so 8 ms is an upper bound); a full reindex, 1.49 s. `AUTO_INDEX_LIMIT` bounds files per directory, not
    // rows in `notes`, so a large vault taxes every repository's session start without it.
    //
    // The prune DELETEs key on the same `(kind, vault_path)` pair and get the index too. The
    // prune *listing* does not — its `LIKE ?2 || '%'` is an expression, which the LIKE
    // optimisation cannot see through — and stays a one-scan-per-directory cost on purpose:
    // rewriting it as a range hack buys nothing the per-file probes did not already cost more.
    //
    // Not UNIQUE, deliberately. One file is one row today, but the schema's identity is
    // `(kind, scope, slug)` and `sync_dir` rewrites scope from the directory; making a second
    // claim about identity here would give conflicts two arbiters (D51's shape — a rule enforced
    // in two places is enforced by whichever one fires first).
    "CREATE INDEX ix_notes_vault ON notes(kind, vault_path);",
];

/// Bring the board up to [`SCHEMA_VERSION`], or explain why it cannot be.
///
/// Keyed on `PRAGMA user_version` — an integer at a fixed offset in the file — rather than a
/// bookkeeping table, which is the standard approach for an embedded single binary and needs no
/// dependency. `rusqlite_migration` does the same thing with more machinery; it was not adopted
/// because amb's migrations are additive by construction (D15 makes the whole board disposable)
/// and this is forty lines that can be read in one sitting.
///
/// **It was expected to pay for itself, and it does not — measured, M7.** Before this, four
/// `CREATE TABLE IF NOT EXISTS` and three indexes ran on every open, and skipping them looked
/// like the saving `MEASUREMENTS.md` M5 identified. Interleaved against a build from `HEAD` the
/// difference is invisible. Migrations are here because a schema change is otherwise silently
/// inert on every board that already exists (D22), not because they made anything faster.
pub fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    // The unlocked read, which is the fast path and the common one: an already-current board
    // does one `PRAGMA` and stops without ever taking a lock.
    let found: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql("reading the schema version"))?;
    if found == SCHEMA_VERSION {
        return Ok(());
    }
    check_not_newer(found, path)?;

    // **`Immediate`, and the version is read again inside it.** These are N unrelated processes
    // with no common parent, and a schema upgrade rolls out to all of them at once — every live
    // session's next hook. Under a deferred transaction all of them read the old version, all of
    // them then apply the same migration, one wins and the rest fail on a column that is already
    // gone. Measured before the fix: **8 of 10 concurrent processes failed to open the board**
    // (D30). Taking the write lock up front makes the losers re-read the *new* version and
    // no-op, which is the same `BEGIN IMMEDIATE` reasoning `messages::send` already uses.
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql("opening the migration transaction"))?;

    let found: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql("re-reading the schema version under the write lock"))?;
    if found == SCHEMA_VERSION {
        return Ok(()); // Someone else migrated while we waited. The transaction rolls back.
    }
    check_not_newer(found, path)?;

    // One transaction for the whole ladder, so a board is never left half-migrated. SQLite
    // treats `user_version` as ordinary transactional state, so the stamp rolls back with it.
    for (i, stmt) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        tx.execute_batch(stmt)
            .map_err(sql(format!("applying migration {} -> {}", i, i + 1)))?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
        .map_err(sql("stamping the schema version"))?;
    tx.commit().map_err(sql("committing the migration"))?;
    Ok(())
}

/// Refuse a board a newer binary has already upgraded.
///
/// Refusing beats guessing: the newer schema may mean something this binary would misread rather
/// than fail on, and a misread board is a silence.
fn check_not_newer(found: i64, path: &Path) -> Result<()> {
    if found > SCHEMA_VERSION {
        return Err(Error::SchemaVersion {
            path: path.display().to_string(),
            found,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// The `-wal` and `-shm` files SQLite keeps beside a board in WAL mode.
///
/// **One place that knows this naming, because two grew independently.** `restrict` built these
/// paths to `chmod` them and `doctor::board_bytes` built them again to `stat` them — the same
/// `OsString`-append written twice, so the suffix list was asserted in two places and a third
/// caller would have made a third copy. The engine creates both files itself, after `open`, which
/// is why anything reasoning about the board's footprint or its permissions has to name them.
///
/// Returned rather than iterated so a caller decides what to do with each: the two existing
/// callers want different things from the same two paths.
pub fn sidecars(path: &Path) -> [PathBuf; 2] {
    ["-wal", "-shm"].map(|suffix| {
        let mut side = path.as_os_str().to_os_string();
        side.push(suffix);
        PathBuf::from(side)
    })
}

/// Restrict the board to the user who owns it. Best-effort: never fails an open.
///
/// Created under the ambient umask, `board.db` lands at 0644 — every message between every
/// session on the machine, and the path each agent is working on, readable by any other local
/// user. The platform's own equivalent channel (a session's inbox socket) is 0600 for exactly
/// this reason, and amb carries the same class of content.
///
/// **The directory bit is the load-bearing one.** SQLite creates `-wal` and `-shm` itself, later,
/// under the same umask; 0700 on the containing directory covers files this function never sees.
/// Tightening is idempotent, so an existing board is repaired on its next open rather than
/// staying wrong forever.
///
/// Failures are swallowed deliberately. A board on a filesystem without Unix modes still works;
/// refusing to open it because the permissions could not be narrowed would trade a privacy
/// improvement for an outage.
///
/// **The directory is narrowed only when `own_dir` — only when this call created it.** An early
/// version tightened it unconditionally, and that quietly chmodded a directory belonging to
/// someone else: `AMB_DB=~/scratch/board.db` took `~/scratch` from 0755 to 0700, and `AMB_DB`
/// pointing anywhere the user already keeps files would do the same. `CLAUDE.md` documents
/// `AMB_DB=/tmp/t.db` as the way to drive the binary by hand, which aims this squarely at `/tmp`.
/// Narrowing our own directory is hardening; narrowing somebody else's is a side effect they
/// never asked for, and the file mode below already protects the data either way (D31).
#[cfg(unix)]
fn restrict(path: &Path, own_dir: bool) {
    use std::os::unix::fs::PermissionsExt;

    fn tighten(p: &Path, mode: u32) {
        if let Ok(meta) = std::fs::metadata(p) {
            let current = meta.permissions().mode() & 0o777;
            if current & 0o077 != 0 {
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
            }
        }
    }

    if own_dir
        && let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tighten(parent, 0o700);
    }
    tighten(path, 0o600);
    for sibling in sidecars(path) {
        tighten(&sibling, 0o600);
    }
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _own_dir: bool) {}

/// Apply the four pragmas from `DESIGN.md`. Runs on every open.
///
/// `journal_mode` returns a row, so it needs `query_row`; a plain `execute` fails with
/// `ExecuteReturnedResults`. The result is checked rather than assumed — WAL silently failing to
/// engage would turn the measured latency profile into the untuned one (M1) with no other sign.
///
/// `busy_ms` is the caller's wait budget — [`INTERACTIVE_BUSY_TIMEOUT_MS`] or
/// [`HOOK_BUSY_TIMEOUT_MS`], whose doc carries the argument for there being two.
pub fn apply_pragmas(conn: &Connection, busy_ms: u64) -> Result<()> {
    // **First, before anything that can block.** Converting a fresh file to WAL takes a brief
    // exclusive lock, so several processes opening a new board at the same moment contend — and
    // with no busy timeout yet in force the losers get `SQLITE_BUSY` immediately and the whole
    // command fails. Measured before the fix: **10 of 12 concurrent first-opens failed** with
    // "database error while setting journal_mode" (D30). The timeout was always here; it was
    // simply installed one statement too late.
    conn.busy_timeout(std::time::Duration::from_millis(busy_ms))
        .map_err(sql("setting busy_timeout"))?;
    engage_wal(conn)?;
    // The standard long-lived-WAL hygiene: after a checkpoint, a `-wal` file larger than this
    // is truncated instead of retained for reuse. A board pinned open by many concurrent
    // readers can otherwise let the WAL grow without bound (checkpoint starvation) — not
    // observed here at 745 KB of board, but the failure arrives silently and the pragma is a
    // no-op until it matters. 4 MiB: several times the whole board today.
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; \
         PRAGMA journal_size_limit = 4194304;",
    )
    .map_err(sql(
        "setting synchronous, foreign_keys and journal_size_limit",
    ))?;
    Ok(())
}

/// How long to keep trying to convert a fresh board to WAL before giving up.
///
/// Comfortably inside the hook's five-second budget, and only ever reached on the one open that
/// races another process for a brand-new file.
const WAL_RETRY_BUDGET: std::time::Duration = std::time::Duration::from_millis(500);

/// Put the board in WAL mode, tolerating other processes opening it at the same moment.
///
/// **`busy_timeout` is not enough here**, which was the surprise. Switching journal mode needs a
/// brief *exclusive* lock, and SQLite declines to invoke the busy handler for it — it returns
/// `SQLITE_BUSY` immediately rather than risk a deadlock against a connection that already holds
/// a shared lock. Moving the timeout earlier took a 12-way first-open race from 10 failures to
/// 2; the remaining two need an actual retry (D30).
///
/// The read comes first and does most of the work: once *any* process has converted the file,
/// every later open finds WAL already engaged, needs no lock, and skips the write entirely.
fn engage_wal(conn: &Connection) -> Result<()> {
    let deadline = std::time::Instant::now() + WAL_RETRY_BUDGET;
    let mut backoff = std::time::Duration::from_millis(2);
    loop {
        // Already converted — by us on a previous open, or by whoever won this race.
        let current: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(sql("reading journal_mode"))?;
        if current.eq_ignore_ascii_case("wal") {
            return Ok(());
        }

        let attempt = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0));
        match attempt {
            Ok(mode) if mode.eq_ignore_ascii_case("wal") => return Ok(()),
            // Still not WAL with the budget spent. Contention resolves well inside it, so what
            // is left is a refusal retrying cannot fix — a read-only file, or a filesystem that
            // does not support WAL — and it is reported as the mode actually in force rather
            // than as a timeout, because that is the part that says what to do about it.
            Ok(mode) if budget_spent(deadline) => {
                return Err(Error::PragmaRefused {
                    pragma: "journal_mode = WAL".into(),
                    got: mode,
                });
            }
            Err(e) if budget_spent(deadline) => {
                return Err(sql("setting journal_mode")(e));
            }
            _ => {}
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(std::time::Duration::from_millis(50));
    }
}

/// Whether the retry budget behind `deadline` is exhausted.
///
/// Named so the comparison exists once and where a test can reach it: inline in two match
/// guards, `>=` flipped to `<` in either and survived, because those arms only run when a
/// conversion attempt fails and no unit fixture loses a race on cue (M46).
fn budget_spent(deadline: std::time::Instant) -> bool {
    std::time::Instant::now() >= deadline
}

// `init_schema` used to live here and ran on every open. [`migrate`] replaced it: the schema is
// now migration 0 -> 1, applied once and skipped thereafter.

/// Seconds since the Unix epoch, as stored in every timestamp column.
///
/// Wall-clock, deliberately. `expires_at` is compared across unrelated processes and must survive
/// reboots, and no persistable monotonic base does both. It is also semantically right: if the
/// machine sleeps for three hours the session was not working, and its claim *should* lapse (D13).
pub fn now() -> Result<f64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::ClockBeforeEpoch)?
        .as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WAL truncation limit must actually be installed, read back off a real connection —
    /// a stated ceiling nothing can check is a comment with a number in it (D95).
    #[test]
    fn the_wal_keeps_a_truncation_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = open_at(&dir.path().join("board.db")).expect("open");
        let limit: i64 = conn
            .query_row("PRAGMA journal_size_limit", [], |r| r.get(0))
            .expect("readable");
        assert_eq!(limit, 4_194_304);
    }

    /// U9: "file is not a database" is accurate and stops one sentence short of the fix. The
    /// rewrite must fire only for corruption-shaped codes — a busy board rewritten this way
    /// would hand out destructive advice ("move the file aside") against a database in use,
    /// which is why `corruption_hint` matches codes and never message text.
    #[test]
    fn a_garbage_board_names_its_own_remedy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        std::fs::write(&path, "this was never a database").expect("write");
        let err = open_at(&path).expect_err("garbage cannot open");
        assert!(matches!(err, Error::CorruptBoard { .. }), "{err:?}");
        let msg = err.to_string();
        assert!(
            msg.contains("disposable") && msg.contains("vault"),
            "the remedy and the reassurance both belong in the message: {msg}"
        );
    }

    /// Each open variant must actually install its own wait budget, asserted through
    /// `PRAGMA busy_timeout` because rusqlite has no getter. This is what reddens if
    /// [`open_at_for_hook`] stops overriding — the cross-constant test in `hooks.rs` checks the
    /// *numbers* against the budget and cannot see whether either number is ever applied.
    #[test]
    fn each_open_variant_installs_its_own_wait_budget() {
        let dir = tempfile::tempdir().expect("tempdir");
        let read_timeout = |conn: &Connection| -> u64 {
            let got: i64 = conn
                .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
                .expect("reading busy_timeout");
            got as u64
        };
        let cli = open_at(&dir.path().join("cli.db")).expect("interactive open");
        assert_eq!(read_timeout(&cli), INTERACTIVE_BUSY_TIMEOUT_MS);
        let hook = open_at_for_hook(&dir.path().join("hook.db")).expect("hook open");
        assert_eq!(read_timeout(&hook), HOOK_BUSY_TIMEOUT_MS);
    }

    /// Both verdicts of SQLite's own consistency check, because all four of its mutants
    /// survived the diff pass over the commit that added it (M47): always-healthy,
    /// always-corrupt and a flipped comparison all render doctor's integrity row from nothing.
    ///
    /// The corrupt fixture is one overwritten page. `quick_check` answers it with a finding
    /// row, not an error — probed with the sqlite3 CLI before this test relied on it.
    #[test]
    fn quick_check_tells_a_healthy_board_from_a_corrupted_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("board.db");
        let conn = open_at(&p).expect("open");
        assert_eq!(quick_check(&conn).expect("healthy check"), None);
        drop(conn);

        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .expect("reopen");
        f.seek(SeekFrom::Start(4096)).expect("seek");
        f.write_all(&[0xff; 4096]).expect("corrupt one page");
        drop(f);

        let conn = Connection::open(&p).expect("raw open; open_at would rightly flinch");
        let verdict = quick_check(&conn).expect("the check answers corruption, not errors on it");
        assert!(
            verdict.is_some(),
            "a corrupted page must not read as healthy"
        );
    }

    /// The kernel flag word is one bit; only `&` reads that bit alone. `|` and `^` both answer
    /// "local" for a remote volume's word — the two mutants that survived while this arithmetic
    /// lived inline, unreachable because no test can mount a network share (M46).
    #[test]
    #[cfg(target_os = "macos")]
    fn the_local_bit_is_read_alone_and_not_smeared_across_the_word() {
        let local = libc::MNT_LOCAL as u32;
        let rdonly = libc::MNT_RDONLY as u32;
        assert!(statfs_is_local(local));
        assert!(statfs_is_local(local | rdonly));
        assert!(!statfs_is_local(0));
        // A remote mount's word is busy, not zero: other flags set, `MNT_LOCAL` clear.
        assert!(!statfs_is_local(rdonly));
    }

    /// The Linux magic table, asserted where it compiles. On a macOS run every mutant of these
    /// lines reports MISSED because the code is never built — a row that reads as "untested" and
    /// means "not present" (M46). CI's Linux leg is the assertor.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_statfs_magic_maps_to_the_name_the_network_list_uses() {
        for (magic, name) in [
            (0x6969i64, "nfs"),
            (0x517b, "smb"),
            (0xfe53_4d42, "smb2"),
            (0xff53_4d42, "cifs"),
            (0x5346_414f, "afs"),
            (0x0102_1997, "9p"),
            (0x564c, "ncpfs"),
            (0x7373, "coda"),
        ] {
            assert_eq!(fstype_name(magic), name);
        }
        assert_eq!(
            fstype_name(0xdead),
            "0xdead",
            "an unknown magic comes back as hex, never as a guess"
        );
    }

    /// The root volume answers with a real name and no locality claim — Linux has no
    /// `MNT_LOCAL`, so the name carries the whole decision there.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_root_volume_answers_with_a_name_and_no_locality_claim() {
        let (name, local) = volume_of(std::path::Path::new("/")).expect("statfs on / succeeds");
        // "xyzzy" is cargo-mutants' canonical String replacement: the clause kills the
        // replaced-body mutants of `volume_of` and `fstype_name`, not a real fstype.
        assert!(!name.is_empty() && name != "xyzzy", "{name}");
        assert_eq!(local, None);
    }

    /// `tighten` widens nothing: a mode the user chose *tighter* than ours is left alone.
    ///
    /// The gate is `current & 0o077 != 0` — group/other bits set — and all four bitwise mutants
    /// of it and of the `& 0o777` mask above it call 0o400 "loose" and chmod it to 0o600. Three
    /// of the four timed out under load rather than surviving outright, which is how close this
    /// came to being filed as caught (M46).
    #[test]
    #[cfg(unix)]
    fn a_mode_tighter_than_ours_is_not_widened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("board.db");
        std::fs::write(&p, b"x").expect("file");
        let mode_after = |m: u32| {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(m)).expect("chmod");
            restrict(&p, false);
            std::fs::metadata(&p).expect("meta").permissions().mode() & 0o777
        };
        assert_eq!(
            mode_after(0o400),
            0o400,
            "restrict must not widen a deliberately tight mode"
        );

        // The loose direction still tightens — the row proving the gate above was consulted,
        // without which the assertion above is satisfied by `restrict` doing nothing at all.
        assert_eq!(
            mode_after(0o644),
            0o600,
            "a group-readable board is tightened"
        );
    }

    /// A database that cannot convert answers with its real mode, after the whole budget —
    /// never silently in the wrong mode, and never instantly.
    ///
    /// The one deterministic refusal in the tree: an in-memory database always answers
    /// `journal_mode = WAL` with `memory`. Ten mutants lived in this loop because nothing
    /// exercised a failed conversion (M46). The error row kills the dead-check mutant — the
    /// guard at the conversion site forced `true`, D30's "checked rather than assumed" check
    /// gone, any mode waved through. The elapsed floor kills every fast-fail mutant: a deadline
    /// already in the past, or a comparison flipped so refusal skips the retries.
    ///
    /// Named residue, not covered here: the `Err`-arm guards are reachable only by losing a
    /// real race mid-conversion (no fixture errors on cue — `query_only` was probed and the
    /// conversion succeeds through it), and `backoff * 2` degrading to a busy-spin changes no
    /// outcome, only CPU on a contended path.
    #[test]
    fn a_refused_wal_conversion_reports_the_mode_after_the_full_budget() {
        let conn = Connection::open_in_memory().expect("memory db");
        let start = std::time::Instant::now();
        let err = engage_wal(&conn).expect_err("memory databases cannot enter WAL");
        assert!(
            start.elapsed() >= WAL_RETRY_BUDGET,
            "gave up after {:?}, inside the {WAL_RETRY_BUDGET:?} budget",
            start.elapsed()
        );
        assert!(
            matches!(&err, Error::PragmaRefused { got, .. } if got.eq_ignore_ascii_case("memory")),
            "{err:?}"
        );
    }

    /// Both sides of the budget line, as close as an `Instant` can put them.
    #[test]
    fn the_budget_is_spent_exactly_when_the_deadline_has_passed() {
        let now = std::time::Instant::now();
        assert!(budget_spent(now - std::time::Duration::from_millis(1)));
        assert!(!budget_spent(now + std::time::Duration::from_secs(3600)));
    }

    /// **The kernel's answer is the authority on macOS, and nothing asserted that it was read.**
    ///
    /// `MNT_LOCAL` is clear for every remote filesystem, whatever it is called — that is the whole
    /// reason the flag beats the name list. `webdav` is not in `NETWORK_FSTYPES` and never will
    /// be; the list is deliberately short (D28). So a volume the kernel calls non-local must be
    /// refused on the flag alone, and one it calls local must be allowed however its type reads.
    ///
    /// Measured before it existed (M22): replacing `mnt_local` with `None` at the call site
    /// reddened nothing in 430 tests. This is the assertion that mutation now fails.
    #[test]
    fn the_kernels_verdict_outranks_the_name_list_in_both_directions() {
        assert!(matches!(
            location_verdict(
                "/Volumes/share/board.db",
                Some("/Volumes/share"),
                Some(("webdav", Some(false)))
            ),
            Err(Error::RemoteVolume { .. })
        ));
        // And the other direction, which is the one a name list gets wrong: `smbfs` is on the
        // list, but if the kernel says the mount is local then it is local.
        assert!(
            location_verdict("/w/board.db", Some("/w"), Some(("smbfs", Some(true)))).is_ok(),
            "a flag saying local must beat a name that reads remote"
        );
    }

    /// **No answer permits the board; it never refuses it** (D28).
    ///
    /// Two distinct silences reach this: no ancestor of the path exists, so there was nothing to
    /// ask about, and the kernel was asked and failed. Both used to be unreachable in a test —
    /// making `statfs` always fail survived all 430 tests.
    #[test]
    fn a_volume_that_cannot_be_asked_about_permits_the_board_rather_than_refusing_it() {
        assert!(location_verdict("/w/board.db", Some("/w"), None).is_ok());
        assert!(location_verdict("board.db", None, None).is_ok());
    }

    /// The marker outranks the filesystem type, because it names the product the user recognises.
    #[test]
    fn a_synced_marker_is_reported_in_preference_to_the_filesystem_underneath_it() {
        let e = location_verdict(
            "/Users/x/Dropbox/board.db",
            Some("/Users/x/Dropbox"),
            Some(("nfs", Some(false))),
        )
        .expect_err("both refusals apply");
        match e {
            Error::SyncedVolume { marker, .. } => assert_eq!(marker, "Dropbox"),
            other => panic!("the marker must win: {other:?}"),
        }
    }

    /// A marker reachable only through the resolved path — the symlink case — still refuses, and
    /// the error names the path the *user* typed rather than the one it resolved to.
    #[test]
    fn a_marker_found_only_after_resolving_still_refuses_and_quotes_what_was_typed() {
        let e = location_verdict("/Users/x/board.db", Some("/Users/x/Dropbox/board.db"), None)
            .expect_err("the resolved path is inside a sync root");
        match e {
            Error::SyncedVolume { path, marker } => {
                assert_eq!(marker, "Dropbox");
                assert_eq!(
                    path, "/Users/x/board.db",
                    "the user's own words, not the target"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    /// The shell's own failure mode, on the platforms that have a syscall to fail.
    ///
    /// **`None` must mean "not answered", never "local".** `statfs` on a path that does not exist
    /// returns -1, and the only thing that distinguishes that from a local answer is this branch.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn statfs_failing_is_no_answer_and_statfs_succeeding_is_one() {
        assert!(
            volume_of(Path::new("/no/such/path/on/any/machine/board.db")).is_none(),
            "a failed syscall must not be reported as a verdict"
        );
        let (fstype, _) = volume_of(Path::new("/")).expect("the root volume answers");
        assert!(
            !fstype.is_empty(),
            "and a successful one carries a type name"
        );
    }

    /// The kernel's answer is the authority, and it overrides a reassuring name.
    ///
    /// `MNT_LOCAL` is set for every locally-stored filesystem and clear for every remote one, so
    /// on macOS the type name is only ever used to write the error message. The third case is the
    /// one that matters: a volume calling itself `apfs` that the kernel says is not local is
    /// remote, and trusting the name there is how a disk image over SMB gets through.
    #[test]
    fn the_kernel_outranks_the_filesystem_name() {
        assert!(!is_remote_volume("apfs", Some(true)));
        assert!(is_remote_volume("smbfs", Some(false)));
        assert!(is_remote_volume("apfs", Some(false)));
        assert!(!is_remote_volume("nfs", Some(true)));
    }

    /// Where there is no flag — Linux — the name decides, and the list is deliberately short.
    #[test]
    fn without_a_flag_the_name_decides_and_fuse_is_not_on_the_list() {
        assert!(is_remote_volume("nfs", None));
        assert!(is_remote_volume("NFS", None), "matched case-insensitively");
        assert!(is_remote_volume("cifs", None));
        assert!(!is_remote_volume("ext4", None));
        assert!(!is_remote_volume("apfs", None));
        // An unrecognised magic arrives as hex and must read as "not on the list", never as a
        // guess in either direction.
        assert!(!is_remote_volume("0xdeadbeef", None));
        // FUSE covers sshfs (remote) and gocryptfs (local) alike. Refusing it would take a
        // working board away from the local case, which D28 rates worse than a missed detection.
        assert!(!is_remote_volume("fuse", None));
    }

    /// A symlink into a sync root is caught, which the substring test alone could not do.
    ///
    /// This is the hole the old comment created by declining to canonicalise: the path as written
    /// contains no marker at all, and only the resolved form does.
    #[test]
    fn a_symlink_into_a_sync_root_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("Dropbox").join("nested");
        std::fs::create_dir_all(&real).expect("mkdir");
        let link = dir.path().join("innocent");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let via_link = link.join("board.db");
        assert!(
            !via_link.to_string_lossy().contains("Dropbox"),
            "the written path must not name Dropbox, or this proves nothing"
        );
        assert!(
            matches!(guard_location(&via_link), Err(Error::SyncedVolume { .. })),
            "a symlink into a sync root must be refused through its resolved path"
        );
    }

    /// The board's own directory must still open. A guard that refuses everything is not a guard.
    #[test]
    fn an_ordinary_local_path_is_still_allowed() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(guard_location(&dir.path().join("board.db")).is_ok());
    }

    #[test]
    fn synced_paths_are_refused() {
        let cases = [
            "/Users/me/Library/Mobile Documents/com~apple~CloudDocs/board.db",
            "/Users/me/Dropbox/board.db",
            "/Users/me/OneDrive/board.db",
        ];
        for c in cases {
            assert!(
                matches!(
                    guard_location(Path::new(c)),
                    Err(Error::SyncedVolume { .. })
                ),
                "expected {c} to be refused"
            );
        }
    }

    #[test]
    fn opening_a_board_explains_itself_to_whoever_finds_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("board.db");
        let _ = open_at(&path).expect("open");

        let readme = std::fs::read_to_string(dir.path().join("nested").join("README.md"))
            .expect("a sibling README must be written");
        assert!(
            readme.contains("safe to delete"),
            "it must say the data is ephemeral"
        );
        assert!(
            readme.contains("Never commit"),
            "and that it must not be committed"
        );
    }

    #[test]
    fn an_existing_readme_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("README.md"), "my own notes").expect("seed");
        let _ = open_at(&dir.path().join("board.db")).expect("open");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).expect("read"),
            "my own notes",
            "someone else's note is theirs to keep"
        );
    }

    #[test]
    fn the_version_constant_matches_the_ladder() {
        // Bumping one without the other would leave a board stamped at a version whose
        // migrations never ran — the exact silent divergence versioning exists to prevent.
        assert_eq!(
            SCHEMA_VERSION as usize,
            MIGRATIONS.len(),
            "SCHEMA_VERSION must equal the number of migrations"
        );
    }

    /// **A renamed column escapes every check the compiler makes**, and this is the guard for it.
    ///
    /// D81 renamed `notes.project` to `notes.scope`. Every Rust reference went red immediately;
    /// one SQL string did not, because a SQL string is text. `memory::resolve` went on selecting
    /// `project`, and the only thing that noticed was an end-to-end test asserting an exit code —
    /// `amb memory observe --cites` came back `69 board unavailable` instead of `65 no such
    /// note`, which reads as an outage rather than as a typo.
    ///
    /// So the note tables get a spelling check. Heuristic on purpose: it looks for the old column
    /// name near a note-table name, over-reports rather than under-reports, and names the file so
    /// the fix is one line away. `agents`, `claims` and `messages` all have a legitimate `project`
    /// column and are untouched by it.
    #[test]
    fn no_sql_statement_still_names_the_column_the_note_tables_dropped() {
        const SOURCES: &[(&str, &str)] = &[
            ("src/db.rs", include_str!("db.rs")),
            ("src/memory/index.rs", include_str!("memory/index.rs")),
            ("src/memory/query.rs", include_str!("memory/query.rs")),
            ("src/memory/events.rs", include_str!("memory/events.rs")),
            ("src/memory/status.rs", include_str!("memory/status.rs")),
            ("src/memory/promote.rs", include_str!("memory/promote.rs")),
            ("src/memory/export.rs", include_str!("memory/export.rs")),
            ("src/memory/write.rs", include_str!("memory/write.rs")),
            ("src/memory/capture.rs", include_str!("memory/capture.rs")),
        ];
        // Where a statement touching a note table begins. The window after it is what gets read.
        const ANCHORS: &[&str] = &[
            "FROM notes",
            "INTO notes",
            "UPDATE notes",
            "FROM note_paths",
            "INTO note_paths",
            "FROM note_links",
            "INTO note_links",
            "FROM note_events",
            "INTO note_events",
        ];
        // **Read the string literal, not a window of source.** The first version took 400
        // characters either side of the anchor and flagged eight sites, every one of them a Rust
        // *variable* called `project` bound as a parameter — which is correct code: the column is
        // `scope` and the value put in it is a project id. A guard that cries wolf on correct code
        // gets deleted, so this looks only inside the quoted statement.
        let literal_around = |src: &str, at: usize| -> Option<String> {
            let start = src[..at].rfind('"')?;
            let end = at + src[at..].find('"')?;
            Some(src[start + 1..end].to_string())
        };
        let mut bad: Vec<String> = Vec::new();
        for (name, src) in SOURCES {
            for anchor in ANCHORS {
                for (i, _) in src.match_indices(anchor) {
                    let Some(stmt) = literal_around(src, i) else {
                        continue;
                    };
                    // The migration ladder is the one place the old name is *required*, because it
                    // is what reads a schema-8 board.
                    // Two exemptions, both marked in the SQL itself rather than by file name:
                    // the migration's own rebuild, and a test staging the board it migrates.
                    if stmt.contains("note_events_new") || stmt.contains("-- schema-8") {
                        continue;
                    }
                    if stmt.contains("project") {
                        bad.push(format!("{name}: {}", stmt.replace('\n', " ").trim()));
                    }
                }
            }
        }
        bad.sort();
        bad.dedup();
        assert!(
            bad.is_empty(),
            "a note-table statement still names `project`, which no longer exists — the \
             compiler cannot see inside a SQL string, so this is the only thing that will: {bad:?}"
        );
    }

    /// Put an already-migrated board back into the shape a **real** schema-8 board has.
    ///
    /// Rewinding `user_version` alone is not enough once a migration changes a table rather than
    /// adding to it: a fresh board is created at the current version, so replaying 8 -> 9 over it
    /// reads a `project` column this version never creates. Both 8 -> 9 tests need the genuine
    /// old shape, and needing it twice is what makes it a function rather than a copy.
    fn stage_schema_8(conn: &Connection) {
        conn.execute_batch(
            // **Rewinding the stamp is not rewinding the board.** `open_at` has already run the
            // whole ladder, so everything migrations 9 and later created is still here; a
            // migration that re-creates it then fails with "already exists" rather than being
            // replayed. Every table added after 8 has to be dropped here too — which is the
            // maintenance cost of replaying a ladder from the middle, and it is cheaper than a
            // migration written to be idempotent, because `IF NOT EXISTS` would let a genuinely
            // skipped migration pass silently.
            "PRAGMA user_version = 8;
             DROP TABLE notes;
             DROP TABLE IF EXISTS note_paths;
             DROP TABLE IF EXISTS note_links;
             DROP TABLE note_events;
             DROP TABLE IF EXISTS measurement_window;
             DROP TABLE IF EXISTS searches;
             CREATE TABLE notes (
               slug TEXT NOT NULL, kind TEXT NOT NULL, project TEXT NOT NULL,
               vault_path TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL,
               created REAL NOT NULL, derived_count INTEGER NOT NULL DEFAULT 0,
               force TEXT NOT NULL DEFAULT 'advice', body_excerpt TEXT,
               content_hash TEXT NOT NULL, mtime REAL NOT NULL, indexed_at REAL NOT NULL,
               PRIMARY KEY (kind, project, slug));
             CREATE TABLE note_events (
               session TEXT NOT NULL, kind TEXT NOT NULL, project TEXT NOT NULL,
               slug TEXT NOT NULL, event TEXT NOT NULL, ts REAL NOT NULL,
               force TEXT NOT NULL DEFAULT 'advice',
               PRIMARY KEY (session, kind, project, slug, event));",
        )
        .expect("stage a schema-8 board");
    }

    /// The 7 -> 8 repair, and the rule it recorded, now that 8 -> 9 has subsumed its effect.
    ///
    /// A board carrying a note with a live `mtime` and an emptied `content_hash` is exactly what
    /// migration 6 -> 7 left behind, and the whole defect was that nothing would ever fix it.
    ///
    /// **This used to run the ladder and read `mtime` back, and it cannot any more**, because
    /// 8 -> 9 drops and rebuilds `notes` — no row survives the replay to have its gate inspected.
    /// That is not a weakening: dropping the derived tables is a *stronger* re-derive than zeroing
    /// the gate, and it is checkable directly. So the test asserts both halves separately — the
    /// rule 7 -> 8 wrote down, and the outcome 8 -> 9 now produces — rather than pretending an
    /// end-to-end read still works.
    #[test]
    fn the_repair_migration_clears_the_gate_so_a_stale_note_is_re_derived() {
        // The rule, read off the migration that recorded it: invalidate the **gate**, not the
        // derived value. Clearing `content_hash` was the bug, because `sync_dir` returns on
        // `mtime` before `content_hash` is ever read.
        let repair = MIGRATIONS[7];
        assert!(
            repair.contains("mtime = 0"),
            "7 -> 8 must clear the gate: {repair}"
        );
        assert!(
            !repair.contains("content_hash = ''"),
            "clearing the derived value is the defect this migration repaired: {repair}"
        );

        // And the outcome 8 -> 9 produces, which is stronger than clearing a gate: the derived
        // tables are rebuilt, so nothing can survive stale. Asserted against a board staged in
        // the real schema-8 shape rather than by rewinding `user_version` on a fresh one — a
        // fresh board is already at 9, and replaying 8 -> 9 over it would be reading a `project`
        // column that this version never creates.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        let conn = open_at(&path).expect("open");
        stage_schema_8(&conn);
        conn.execute_batch(
            "-- schema-8 shape on purpose: this is the board 8 -> 9 has to read.
             INSERT INTO notes (kind, project, slug, vault_path, title, status, created,
                                derived_count, body_excerpt, content_hash, mtime, indexed_at)
             VALUES ('observation','nest','n','p','t','active',0,0,'','',1700000000.0,0);",
        )
        .expect("a stale note");
        drop(conn);

        let conn = open_at(&path).expect("reopen");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
            .expect("count");
        assert_eq!(
            rows, 0,
            "8 -> 9 rebuilds the derived tables, so every note is re-read from the vault — a row \
             surviving here would be one the migration failed to invalidate"
        );
        // And the rebuilt table is the new shape, not the old one under a new version stamp.
        let cols: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('notes') WHERE name = 'scope'",
                [],
                |r| r.get(0),
            )
            .expect("read columns");
        assert_eq!(cols, 1, "notes.scope must exist after the ladder");
    }

    /// The ledger is not derived, so 8 -> 9 has to carry it across rather than drop it.
    ///
    /// **This is the half of the migration that could lose something.** `notes` can be rebuilt
    /// from the vault; `note_events` cannot be rebuilt from anything, and it is what D59's
    /// withdrawal condition reads. A pattern event has to arrive as a decision at `@@`, because
    /// that is what it always was.
    #[test]
    fn the_ledger_survives_the_scope_migration_and_a_pattern_event_becomes_a_global_decision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        let conn = open_at(&path).expect("open");
        stage_schema_8(&conn);
        conn.execute_batch(
            "INSERT INTO note_events VALUES ('s','observation','nest','n','injected',1.0,'advice');
             INSERT INTO note_events VALUES ('s','pattern','','p','cited',2.0,'rule');",
        )
        .expect("stage a schema-8 ledger");
        drop(conn);

        let conn = open_at(&path).expect("reopen");
        let mut stmt = conn
            .prepare("SELECT kind, scope, slug, force FROM note_events ORDER BY ts")
            .expect("prepare");
        let rows: Vec<(String, String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .expect("query")
            .flatten()
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    "observation".to_string(),
                    "nest".to_string(),
                    "n".to_string(),
                    "advice".to_string()
                ),
                (
                    "decision".to_string(),
                    "@@".to_string(),
                    "p".to_string(),
                    "rule".to_string()
                ),
            ],
            "the ledger must survive with its scope and force intact"
        );
    }

    #[test]
    fn a_fresh_board_is_stamped_and_a_second_open_does_not_re_migrate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        let conn = open_at(&path).expect("open");
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read version");
        assert_eq!(v, SCHEMA_VERSION, "a new board is stamped current");

        // Drop a table, then reopen. If the schema were still re-asserted on every open it
        // would come back; the point of versioning is that it does not.
        conn.execute_batch("DROP TABLE claims").expect("drop");
        drop(conn);
        let conn = open_at(&path).expect("reopen");
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='claims'",
                [],
                |r| r.get(0),
            )
            .expect("query");
        assert_eq!(
            found, 0,
            "an already-current board must skip the schema work entirely"
        );
    }

    #[test]
    fn a_board_from_before_versioning_is_migrated_in_place() {
        // The upgrade path that actually matters: an existing board is at user_version 0 with
        // every table already present. It must be stamped, not rebuilt, and must keep its rows.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        {
            let conn = Connection::open(&path).expect("create");
            conn.execute_batch(include_str!("schema.sql"))
                .expect("schema");
            conn.execute_batch("PRAGMA user_version = 0")
                .expect("unstamp");
            conn.execute(
                "INSERT INTO agents (id, name, project, first_seen, last_seen)
                 VALUES ('a', 'alice', 'nest', 1.0, 1.0)",
                [],
            )
            .expect("seed");
        }
        let conn = open_at(&path).expect("open an old board");
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("version");
        assert_eq!(v, SCHEMA_VERSION);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM agents", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 1, "existing rows must survive the migration");
    }

    #[test]
    fn a_board_from_a_newer_binary_is_refused_rather_than_guessed_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        let _ = open_at(&path).expect("create");
        {
            let conn = Connection::open(&path).expect("reopen");
            conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 5))
                .expect("stamp forward");
        }
        assert!(
            matches!(open_at(&path), Err(Error::SchemaVersion { .. })),
            "an older binary must refuse a board it may misread"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_board_is_not_readable_by_other_users() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("board.db");
        let conn = open_at(&path).expect("open");
        // Force a write so the WAL exists and can be checked too.
        conn.execute_batch(
            "INSERT INTO agents (id, name, project, first_seen, last_seen)
             VALUES ('a','a','p',1.0,1.0)",
        )
        .expect("write");
        drop(conn);
        let _ = open_at(&path).expect("reopen so late-created siblings are tightened");

        let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(
            mode(path.parent().expect("parent")) & 0o077,
            0,
            "the directory must not be traversable by anyone else"
        );
        assert_eq!(mode(&path) & 0o077, 0, "nor the database readable");
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_we_did_not_create_is_left_alone() {
        // D31. Tightening our own directory is hardening; tightening one the user already had is
        // a side effect they never asked for — and `CLAUDE.md` points `AMB_DB` at `/tmp`.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let theirs = dir.path().join("scratch");
        std::fs::create_dir(&theirs).expect("mkdir");
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let path = theirs.join("board.db");
        let _ = open_at(&path).expect("open");

        let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(
            mode(&theirs),
            0o755,
            "a pre-existing directory must keep the permissions its owner chose"
        );
        assert_eq!(
            mode(&path) & 0o077,
            0,
            "while the board file itself is still narrowed — that is what protects the data"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_world_readable_board_is_repaired_on_open() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("board.db");
        let _ = open_at(&path).expect("open");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("widen");
        let _ = open_at(&path).expect("reopen");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "a board already on disk must be repaired");
    }

    #[test]
    fn a_local_path_is_allowed() {
        assert!(guard_location(Path::new("/Users/me/.agent-messageboard/board.db")).is_ok());
    }

    #[test]
    fn open_applies_wal_and_creates_every_table() {
        let dir = std::env::temp_dir().join(format!("amb-test-{}", std::process::id()));
        let path = dir.join("board.db");
        let _ = std::fs::remove_dir_all(&dir);
        let conn = open_at(&path).expect("open should succeed");

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("journal_mode should be readable");
        assert_eq!(
            mode.to_lowercase(),
            "wal",
            "WAL must actually be engaged, not just requested"
        );

        for table in [
            "agents",
            "messages",
            "reads",
            "claims",
            "notes",
            "note_paths",
            "note_events",
            "claim_notices",
            "memory_counters",
        ] {
            let found: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .expect("sqlite_master should be queryable");
            assert_eq!(found, 1, "table {table} should exist");
        }

        // Idempotent: opening again must not fail on existing tables.
        drop(conn);
        open_at(&path).expect("second open should succeed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

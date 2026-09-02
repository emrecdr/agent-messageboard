//! The index: syncing the vault into SQLite, and the links it derives.
//!
//! The imperative shell. The index stores no note content (D34), so `rm
//! board.db` loses zero notes.

use super::*;

/// Every column the injection and the CLI need, in one statement.
///
/// `group_concat` over newlines rather than a second query per note: paths cannot contain a
/// newline, and a hook that issued nine queries to render eight notes would be spending the
/// budget D9 protects.
pub(crate) const SELECT_NOTE: &str = "\
    SELECT n.kind, n.scope, n.slug, n.title, n.status, n.created, n.vault_path, n.body_excerpt,
           (SELECT group_concat(p.path_glob, char(10)) FROM note_paths p
             WHERE p.kind = n.kind AND p.scope = n.scope AND p.slug = n.slug),
           n.force
      FROM notes n";

pub(crate) fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedNote> {
    let paths: Option<String> = row.get(8)?;
    Ok(IndexedNote {
        id: NoteId {
            kind: row.get(0)?,
            scope: row.get(1)?,
            slug: row.get(2)?,
        },
        title: row.get(3)?,
        status: row.get(4)?,
        created: row.get(5)?,
        vault_path: row.get(6)?,
        excerpt: row.get(7)?,
        paths: paths
            .map(|p| p.lines().map(str::to_string).collect())
            .unwrap_or_default(),
        force: row.get(9)?,
    })
}

/// The vault-relative directory a note lives in, from its kind and its scope.
///
/// **The directory *is* the scope, written out for a human** — someone browsing the vault in
/// Obsidian sees `topics/rust/` and `global/` and knows what they are looking at without reading
/// any frontmatter. The sigils stay out of path names deliberately: `#` and `@` are legal in a
/// filename and hostile in a shell, and the one place the grammar buys nothing is the one place
/// it costs quoting.
///
/// A candidate is flat, because a candidate has no scope yet ([`UNSCOPED`]) — filing it under one
/// scope would be a claim its ledger has not made.
///
/// | kind, scope | directory |
/// |---|---|
/// | observation, `nest` | `projects/nest/` |
/// | decision, `nest` | `decisions/nest/` |
/// | decision, `#rust` | `topics/rust/` |
/// | decision, `@@` | `global/` |
/// | candidate | `candidates/` |
pub fn vault_dir(kind: &str, scope: &str) -> String {
    use crate::address::Scope;
    if kind == CANDIDATE {
        return "candidates".to_string();
    }
    let place = match crate::address::parse_scope(scope) {
        Ok(Scope::Global) => return "global".to_string(),
        Ok(Scope::Topic(t)) => return format!("topics/{}", safe_component(&t)),
        Ok(Scope::Project(p)) => p,
        // Unparseable is not a reason to write outside the vault. `safe_component` is what makes
        // the fallback safe rather than merely tolerable.
        Err(_) => scope.to_string(),
    };
    match kind {
        DECISION => format!("decisions/{}", safe_component(&place)),
        CAPTURE => format!("captures/{}", safe_component(&place)),
        _ => format!("projects/{}", safe_component(&place)),
    }
}

/// The vault-relative path of one note.
pub fn vault_rel(kind: &str, scope: &str, slug: &str) -> String {
    format!("{}/{slug}.md", vault_dir(kind, scope))
}

/// The first paragraph, capped. A display convenience and never the authority — the file is.
fn excerpt_of(body: &str) -> Option<String> {
    let first = body.split("\n\n").next()?.trim();
    if first.is_empty() {
        return None;
    }
    let mut s: String = first.chars().take(240).collect();
    if first.chars().count() > 240 {
        s.push('\u{2026}');
    }
    Some(s.replace('\n', " "))
}

/// Write one note's row and its paths.
///
/// Explicit upsert rather than `INSERT OR REPLACE`: replace deletes and re-inserts, which fires
/// `note_paths`' `ON DELETE CASCADE` as a side effect. Relying on that would make the path
/// rewrite below invisible to a reader and dependent on a foreign-key action staying enabled.
pub(crate) fn upsert(
    conn: &Connection,
    note: &Note,
    vault_rel: &str,
    mtime: f64,
    at: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO notes
           (kind, scope, slug, vault_path, title, status, created, derived_count, force,
            body_excerpt, mtime, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?11, ?12, ?8, ?9, ?10)
         ON CONFLICT(kind, scope, slug) DO UPDATE SET
           vault_path = excluded.vault_path, title = excluded.title, status = excluded.status,
           derived_count = excluded.derived_count, force = excluded.force,
           created = excluded.created, body_excerpt = excluded.body_excerpt,
           mtime = excluded.mtime, indexed_at = excluded.indexed_at",
        params![
            note.id.kind,
            note.id.scope,
            note.id.slug,
            vault_rel,
            note.title,
            note.status,
            note.created,
            excerpt_of(&note.body),
            mtime,
            at,
            note.derivations.len() as i64,
            note.force,
        ],
    )
    .map_err(sql("indexing a note"))?;

    conn.execute(
        "DELETE FROM note_paths WHERE kind = ?1 AND scope = ?2 AND slug = ?3",
        params![note.id.kind, note.id.scope, note.id.slug],
    )
    .map_err(sql("clearing a note's paths"))?;
    for p in &note.files {
        conn.execute(
            "INSERT OR IGNORE INTO note_paths (kind, scope, slug, path_glob)
             VALUES (?1, ?2, ?3, ?4)",
            params![note.id.kind, note.id.scope, note.id.slug, p],
        )
        .map_err(sql("indexing a note's paths"))?;
    }

    // Links, rebuilt from frontmatter on every pass exactly as paths are. Cleared first so an
    // edge removed from a file disappears from the index rather than lingering — the vault is
    // truth, and an index row that outlives its source is a claim nothing supports (D63).
    conn.execute(
        "DELETE FROM note_links WHERE kind = ?1 AND scope = ?2 AND slug = ?3",
        params![note.id.kind, note.id.scope, note.id.slug],
    )
    .map_err(sql("clearing a note's links"))?;
    if let Some(target) = &note.superseded_by {
        // Stored on the *superseded* note pointing forward, which is the direction the file
        // records. `ix_note_links_target` makes the reverse walk one index lookup, so both
        // directions are cheap without storing the edge twice and risking half of it going stale.
        conn.execute(
            "INSERT OR IGNORE INTO note_links (kind, scope, slug, rel, target)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                note.id.kind,
                note.id.scope,
                note.id.slug,
                REL_SUPERSEDED_BY,
                target.trim()
            ],
        )
        .map_err(sql("indexing a note's links"))?;
    }
    Ok(())
}

/// One step of a supersession chain, in whichever direction it was walked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    pub title: String,
    pub status: String,
}

/// Walk a supersession chain **both ways** from one note.
///
/// **`amb` could retire a note and then not tell you why or what replaced it.** The edge was in the
/// file and nowhere queryable, so the answer required opening markdown by hand. Returns
/// `(ancestors, descendants)`: what this note replaced, oldest first, and what replaced it.
///
/// Bounded by a step budget rather than trusting the data to be acyclic. A cycle is one of the
/// four things [`validate_links`] reports, so it is a state this codebase expects to *find*, and a
/// traversal that hangs on the malformed input its own validator exists to detect would be a poor
/// way to learn that.
pub fn history(conn: &Connection, id: &NoteId) -> Result<(Vec<Step>, Vec<Step>)> {
    const MAX_STEPS: usize = 64;

    let row = |target: &str| -> Result<Option<Step>> {
        let mut stmt = conn
            .prepare(
                "SELECT scope, kind, slug, title, status FROM notes
                  WHERE (scope || '/' || slug) = ?1 OR (kind || '/' || slug) = ?1 OR slug = ?1",
            )
            .map_err(sql("resolving a link target"))?;
        let mut rows = stmt
            .query_map(params![target], |r| {
                Ok(Step {
                    id: NoteId {
                        kind: r.get::<_, String>(1)?,
                        scope: r.get::<_, String>(0)?,
                        slug: r.get::<_, String>(2)?,
                    }
                    .display(),
                    title: r.get(3)?,
                    status: r.get(4)?,
                })
            })
            .map_err(sql("resolving a link target"))?;
        rows.next()
            .transpose()
            .map_err(sql("reading a link target"))
    };

    // The subject must exist before its provenance is narrated. Without this, a typo'd id
    // printed "stands alone — it replaced nothing, and nothing replaced it": a provenance
    // command fabricating a clean history for a note that is not there, exit 0 — this
    // project's failures are silences, and that was one (U5). Every other id-taking command
    // answers a miss with the same error and exit 65.
    if row(&id.display())?.is_none() {
        return Err(Error::NoSuchNote(id.display()));
    }

    // Forward: what replaced this, following `superseded_by` until nothing does.
    let mut descendants = Vec::new();
    let mut cur = id.clone();
    for _ in 0..MAX_STEPS {
        let next: Option<String> = conn
            .query_row(
                "SELECT target FROM note_links
                  WHERE kind = ?1 AND scope = ?2 AND slug = ?3 AND rel = ?4",
                params![cur.kind, cur.scope, cur.slug, REL_SUPERSEDED_BY],
                |r| r.get(0),
            )
            .ok();
        let Some(next) = next else { break };
        let Some(step) = row(&next)? else {
            // A dangling target still belongs in the answer: "replaced by something that is not
            // here" is the fact, and hiding it would make a broken chain look like a complete one.
            descendants.push(Step {
                id: next,
                title: "(no such note)".into(),
                status: "dangling".into(),
            });
            break;
        };
        if descendants.iter().any(|s: &Step| s.id == step.id) {
            break; // a cycle; validate_links reports it
        }
        cur = parse_id(&step.id).unwrap_or(cur);
        descendants.push(step);
    }

    // Backward: what this replaced, via the target index.
    let mut ancestors = Vec::new();
    let mut want = id.display();
    for _ in 0..MAX_STEPS {
        let prev: Option<(String, String, String)> = conn
            .query_row(
                "SELECT kind, scope, slug FROM note_links WHERE target = ?1 AND rel = ?2",
                params![want, REL_SUPERSEDED_BY],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((kind, scope, slug)) = prev else {
            break;
        };
        let pid = NoteId { kind, scope, slug };
        let Some(step) = row(&pid.display())? else {
            break;
        };
        if ancestors.iter().any(|s: &Step| s.id == step.id) {
            break;
        }
        want = step.id.clone();
        ancestors.push(step);
    }
    ancestors.reverse();
    Ok((ancestors, descendants))
}

/// One `note_links` row joined to the status of both ends: (kind, scope, slug, target,
/// target's status, own status). Named because the tuple is wide enough to misread.
type LinkRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// Something the link graph says that cannot be true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkProblem {
    pub note: String,
    pub kind: String,
    pub detail: String,
}

/// The four ways a supersession chain can be inconsistent, each checked deterministically.
///
/// **Shipped with the traversal rather than after it**, on the evidence that this class finds real
/// defects immediately: devt's equivalent unknown-key check found two ghosts the day it shipped,
/// and four of this scope's own audits each found something.
///
/// Every check is mechanical. Nothing here asks whether two notes *feel* inconsistent, which needs
/// a model, which the write path refuses.
pub fn validate_links(conn: &Connection) -> Result<Vec<LinkProblem>> {
    let mut out = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT l.kind, l.scope, l.slug, l.target,
                    (SELECT status FROM notes n
                      WHERE (n.scope || '/' || n.slug) = l.target
                         OR (n.kind || '/' || n.slug) = l.target OR n.slug = l.target),
                    (SELECT status FROM notes s
                      WHERE s.kind = l.kind AND s.scope = l.scope AND s.slug = l.slug)
               FROM note_links l WHERE l.rel = ?1",
        )
        .map_err(sql("validating links"))?;
    let rows: Vec<LinkRow> = stmt
        .query_map(params![REL_SUPERSEDED_BY], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(sql("validating links"))?
        .flatten()
        .collect();
    drop(stmt);

    for (kind, scope, slug, target, target_status, own_status) in &rows {
        let id = NoteId {
            kind: kind.clone(),
            scope: scope.clone(),
            slug: slug.clone(),
        }
        .display();
        // Dangling: it says it was replaced by something that is not in the vault.
        if target_status.is_none() {
            out.push(LinkProblem {
                note: id.clone(),
                kind: "dangling".into(),
                detail: format!("superseded_by {target}, which is not a note in the vault"),
            });
        }
        // The contradiction: a note that has been replaced is still being treated as current, so
        // both it and its successor are injectable and the model picks. This is the state D40
        // exists to make impossible, asserted rather than assumed.
        if own_status.as_deref() == Some(ACTIVE) {
            out.push(LinkProblem {
                note: id.clone(),
                kind: "supersedes-but-active".into(),
                detail: format!(
                    "declares superseded_by {target} yet is still active, so both are injectable"
                ),
            });
        }
        // A cycle, found by walking forward from here and arriving back.
        if let Some(start) = parse_id(&id) {
            let (_, fwd) = history(conn, &start)?;
            if fwd.iter().any(|s| s.id == id) {
                out.push(LinkProblem {
                    note: id.clone(),
                    kind: "cycle".into(),
                    detail: "its supersession chain returns to itself".into(),
                });
            }
        }
    }

    // Orphaned retirement: retired, but nothing says what replaced it. The note is silently gone
    // from injection with no successor to follow, which is indistinguishable from a mistake.
    let mut stmt = conn
        .prepare(
            "SELECT kind, scope, slug FROM notes n WHERE n.status = ?1
              AND NOT EXISTS (SELECT 1 FROM note_links l
                               WHERE l.kind = n.kind AND l.scope = n.scope
                                 AND l.slug = n.slug AND l.rel = ?2)",
        )
        .map_err(sql("checking retired notes"))?;
    let orphans: Vec<String> = stmt
        .query_map(params![SUPERSEDED, REL_SUPERSEDED_BY], |r| {
            Ok(NoteId {
                kind: r.get(0)?,
                scope: r.get(1)?,
                slug: r.get(2)?,
            }
            .display())
        })
        .map_err(sql("checking retired notes"))?
        .flatten()
        .collect();
    drop(stmt);
    for id in orphans {
        out.push(LinkProblem {
            note: id,
            kind: "orphaned-retirement".into(),
            detail: "is superseded but names no successor".into(),
        });
    }
    Ok(out)
}

/// The only relationship in the index, and it is named rather than spelled inline.
///
/// **One type, because one type has a consumer.** The published vocabularies offer eight or more;
/// `depends_on` was declined because `note_paths` already answers the axis anyone actually asks on
/// — *what concerns this file* — and nothing asks *what concerns this note*. `conflicts_with` was
/// declined because a conflict is a note with its own lifecycle, and that note **is** the edge;
/// storing a symmetric edge as well would be one fact in two places. Both are recorded in D63 so
/// the omission reads as a decision.
pub const REL_SUPERSEDED_BY: &str = "superseded_by";

/// What an indexing pass did. Reported rather than assumed — the failure this whole layer is
/// most exposed to is believing it captured something it did not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexStats {
    pub scanned: usize,
    pub indexed: usize,
    pub unchanged: usize,
    pub unreadable: usize,
    pub pruned: usize,
    /// True when the directory was larger than [`AUTO_INDEX_LIMIT`] and the pass declined.
    pub skipped: bool,
}

/// Index one scope directory, re-reading only what changed.
///
/// **`mtime` is the only gate.** A file whose mtime the index already knows costs one `stat` and
/// is skipped entirely; a file whose mtime differs is re-read, re-parsed and re-upserted whether
/// or not its bytes actually changed. This comment used to claim `content_hash` was "the
/// decision", and a migration was written trusting that (see the 7 -> 8 entry in
/// `db::MIGRATIONS`, which exists to repair it). That column was written here, read by nothing,
/// and dropped in D85 — so there is no longer a second signal on this path to mistake for a gate.
/// `text::content_hash` the *function* survives, for `export --check` alone.
///
/// The consequence worth knowing before changing anything here: **clearing a derived column does
/// not cause it to be re-derived.** Only changing `mtime` does. Anything that invalidates index
/// state has to invalidate the gate.
///
/// **Takes a directory name, not a scope name, and the difference is load-bearing.** A note's
/// `scope:` frontmatter is honoured only when it sanitises back to the directory the file
/// actually sits in; otherwise the directory wins. Disk outranks status — and a note indexed
/// under a scope name nobody queries is invisible rather than wrong, which is this scope's
/// worst failure shape.
pub fn sync_dir(
    conn: &Connection,
    vault: &Path,
    kind: &str,
    scope: &str,
    at: f64,
    limit: Option<usize>,
) -> Result<IndexStats> {
    let mut stats = IndexStats::default();
    let rel_dir = vault_dir(kind, scope);
    let dir = vault.join(&rel_dir);
    if !dir.is_dir() {
        return Ok(stats);
    }
    // A candidate carries the empty scope, which is what the schema comment requires: SQLite
    // permits NULLs in a composite primary key and does not compare them equal, so the absence
    // is `''` rather than `NULL` (D50, D81).
    //
    // **Only the project form is sanitised**, because the other two are already closed sets:
    // `@@` is a constant and `#rust` came through `parse_scope`, which refuses `/`. Running
    // `safe_component` over them would rewrite `@@` into something no query looks for.
    let scope = match crate::address::parse_scope(scope) {
        _ if kind == CANDIDATE => String::new(),
        Ok(crate::address::Scope::Project(p)) => safe_component(&p),
        Ok(other) => other.as_str(),
        Err(_) => safe_component(scope),
    };
    let entries = std::fs::read_dir(&dir).map_err(io(format!("reading {}", dir.display())))?;
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    stats.scanned = files.len();
    if let Some(max) = limit
        && files.len() > max
    {
        stats.skipped = true;
        return Ok(stats);
    }

    // One transaction per directory rather than one autocommit per statement — a 1,000-note
    // reindex was 1,000+ separate WAL commits. Not fsyncs: under WAL with `synchronous=NORMAL`
    // a commit deliberately skips the sync (an earlier draft of this comment claimed otherwise,
    // which is the false-mechanism class this project catalogues). What N commits do pay is N
    // wal-index lock acquisitions and N commit frames where one would do. Deferred, not
    // `IMMEDIATE`: the common pass finds nothing changed and never writes, so a read transaction
    // takes no lock another session's hook would wait behind; the first upsert upgrades it.
    // `busy_timeout` covers that upgrade **only while nothing has committed since this
    // transaction's first read** — against a stale snapshot the upgrade returns
    // `SQLITE_BUSY_SNAPSHOT` immediately and the timeout is never consulted, because waiting
    // cannot freshen a snapshot; the transaction would have to restart. (A previous version of
    // this comment said the timeout covers it "like any other write" — false in exactly the
    // racing case the deferred choice reasons about, the catalogued class again.) The lost race
    // is accepted rather than retried: it needs two sessions syncing one vault in the same
    // instant, the hook swallows the error, the index self-heals on the next hook pass, and
    // D103's 2 s budget is better spent on the sync than on a retry loop.
    // `unchecked_transaction` because this function holds `&Connection`, and rusqlite's default
    // drop behaviour is the rollback an early `?` wants.
    let tx = conn
        .unchecked_transaction()
        .map_err(sql("opening the sync transaction"))?;
    let prefix = format!("{rel_dir}/");
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for file in files {
        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
            stats.unreadable += 1;
            continue;
        };
        let rel = format!("{prefix}{stem}.md");
        seen.insert(rel.clone());
        let mtime = file_mtime(&file);
        // `prepare_cached`: this runs once per file, and migration 13's index fixed the
        // *execution* while the prepare was still being paid per iteration — the same
        // eight-token SELECT compiled up to `AUTO_INDEX_LIMIT` times per hook pass.
        let known: Option<f64> = conn
            .prepare_cached(SYNC_PROBE_SQL)
            .and_then(|mut s| s.query_row(params![kind, rel], |r| r.get(0)))
            .ok();
        if known == Some(mtime) {
            stats.unchanged += 1;
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            stats.unreadable += 1;
            continue;
        };
        // A note that will not parse is counted and skipped, never fatal. This runs inside a
        // hook, and one malformed file in a vault must not cost a session its memory.
        let Some(mut note) = parse_note(&text, stem, mtime) else {
            stats.unreadable += 1;
            continue;
        };
        // The directory decides both, because disk outranks status: a note's own frontmatter can
        // disagree with where it actually sits, and the filesystem is the thing that is true.
        if safe_component(&note.id.scope) != scope {
            note.id.scope = scope.clone();
        }
        note.id.slug = stem.to_string();
        note.id.kind = kind.to_string();
        upsert(conn, &note, &rel, mtime, at)?;
        stats.indexed += 1;
    }

    // Prune what the vault no longer has. The index is derived; a row without a file is a lie
    // that would keep being injected. Keyed on `vault_path`, so it holds even when a note's
    // frontmatter names a different scope than the directory it sits in.
    let mut stmt = conn
        .prepare("SELECT vault_path FROM notes WHERE kind = ?1 AND vault_path LIKE ?2 || '%'")
        .map_err(sql("listing indexed notes"))?;
    let indexed: Vec<String> = stmt
        .query_map(params![kind, prefix], |r| r.get(0))
        .map_err(sql("listing indexed notes"))?
        .flatten()
        .collect();
    drop(stmt);
    for path in indexed {
        if !seen.contains(&path) {
            conn.execute(
                "DELETE FROM notes WHERE kind = ?1 AND vault_path = ?2",
                params![kind, path],
            )
            .map_err(sql("pruning a note"))?;
            stats.pruned += 1;
        }
    }
    tx.commit().map_err(sql("committing the sync"))?;
    Ok(stats)
}

/// The per-file probe [`sync_dir`] runs, held as a named constant because its plan test must
/// assert the exact string production prepares — a re-typed copy in the test would stay green
/// against a query nothing runs (`claims::list_sql` records the same rule, same commit).
const SYNC_PROBE_SQL: &str = "SELECT mtime FROM notes WHERE kind = ?1 AND vault_path = ?2";

pub(crate) fn file_mtime(p: &Path) -> f64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The vault directories whose *subdirectories* each name a scope, and how to read that name back.
///
/// Separated from [`reindex`] so the walk reads as a table: `projects/nest` is the project `nest`,
/// `topics/rust` is the scope `#rust`. The two flat directories — `global/` and `candidates/` —
/// are one scope each and are listed inline at the call site.
type ScopeOfDir = fn(&str) -> String;
pub(crate) const NESTED_DIRS: [(&str, &str, ScopeOfDir); 4] = [
    (OBSERVATION, "projects", |p| p.to_string()),
    (DECISION, "decisions", |p| p.to_string()),
    (DECISION, "topics", |t| {
        format!("{}{t}", crate::address::TOPIC_SIGIL)
    }),
    // Captures are walked like observations and injected like nothing (D86). Indexing them is
    // what keeps `amb memory recall` able to find them; `INJECTABLE` is what keeps them out of a
    // session's context. The two are separate on purpose — dropping them from the walk instead
    // would make them unsearchable, which is a different and worse answer.
    (CAPTURE, "captures", |p| p.to_string()),
];

/// The two directories that hold notes directly, with no scope directory between.
///
/// **Named so the walk can be checked against `vault_dir`.** It was an inline array literal, and
/// `a_note_of_every_kind_is_seen_by_the_vault_walk` could therefore only compare `vault_dir`
/// against itself and `note_files` — never against the walk that `reindex` performs, which is the
/// one whose prune loop deletes rows.
pub(crate) const FLAT_DIRS: [(&str, &str); 2] =
    [(DECISION, crate::address::GLOBAL), (CANDIDATE, UNSCOPED)];

/// Rebuild the whole index from the vault. **This is the proof that the vault is truth**: delete
/// `board.db` entirely, run this, and nothing is lost but the ledger's measurements.
///
/// Walks all four layouts. A directory that does not exist costs one failed `is_dir`, so a vault
/// holding only observations pays nothing for the kinds it has not grown yet.
pub fn reindex(conn: &Connection, vault: &Path, at: f64) -> Result<IndexStats> {
    let mut total = IndexStats::default();
    let mut seen_dirs: Vec<String> = Vec::new();

    let mut run = |kind: &str, scope: &str, total: &mut IndexStats| -> Result<()> {
        seen_dirs.push(vault_dir(kind, scope));
        // No limit: an explicit `amb memory index` is a user waiting at a terminal, not a hook
        // spending a session's five seconds.
        let s = sync_dir(conn, vault, kind, scope, at, None)?;
        total.scanned += s.scanned;
        total.indexed += s.indexed;
        total.unchanged += s.unchanged;
        total.unreadable += s.unreadable;
        total.pruned += s.pruned;
        Ok(())
    };

    // **Every directory `vault_dir` can produce has to be walked back**, and the two lists are
    // separated by more than a slash: the nested ones name a scope per subdirectory, the flat
    // ones *are* one scope. `a_note_of_every_kind_is_seen_by_the_vault_walk` reads `vault_dir`'s
    // string literals and compares them against `NESTED_DIRS` and `FLAT_DIRS`, so adding a
    // directory there and forgetting it here goes red.
    //
    // **That sentence was false until D86's review.** The test compared `vault_dir` against its
    // own pair list and against `note_files`, and never read these tables at all — so the one
    // walk whose omission *deletes* rows was the one walk unguarded, under a comment saying it
    // was covered. The prune set below is built from this loop: a directory missing here is not
    // merely unindexed, it is emptied.
    for (kind, parent, to_scope) in NESTED_DIRS {
        let root = vault.join(parent);
        if !root.is_dir() {
            continue;
        }
        let mut dirs: Vec<String> = std::fs::read_dir(&root)
            .map_err(io(format!("reading {}", root.display())))?
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        dirs.sort();
        for d in dirs {
            run(kind, &to_scope(&d), &mut total)?;
        }
    }
    for (kind, scope) in FLAT_DIRS {
        run(kind, scope, &mut total)?;
    }

    // Rows whose directory is gone entirely — `sync_dir` only ever sees directories that still
    // exist, so it cannot notice these.
    let mut stmt = conn
        .prepare("SELECT kind, vault_path FROM notes")
        .map_err(sql("listing indexed notes"))?;
    let known: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sql("listing indexed notes"))?
        .flatten()
        .collect();
    drop(stmt);
    for (kind, path) in known {
        let dir = path.rsplit_once('/').map(|(d, _)| d.to_string());
        if !dir.is_some_and(|d| seen_dirs.contains(&d)) {
            conn.execute(
                "DELETE FROM notes WHERE kind = ?1 AND vault_path = ?2",
                params![kind, path],
            )
            .map_err(sql("pruning a note"))?;
            total.pruned += 1;
        }
    }
    Ok(total)
}

/// `amb memory history`'s human output: what this note replaced, and what replaced it.
///
/// **Standing alone is stated, not implied by silence.** An id with no lineage and an id that
/// does not exist would otherwise print the same nothing, and this project's failures are
/// silences.
pub fn render_history(id: &NoteId, before: &[Step], after: &[Step]) -> String {
    if before.is_empty() && after.is_empty() {
        return format!(
            "{} stands alone — it replaced nothing, and nothing replaced it\n",
            id.display()
        );
    }
    let mut out = String::new();
    for s in before {
        out.push_str(&format!("  {} — {} [{}]\n", s.id, s.title, s.status));
        out.push_str("    ↓\n");
    }
    out.push_str(&format!("> {}\n", id.display()));
    for s in after {
        out.push_str("    ↓\n");
        out.push_str(&format!("  {} — {} [{}]\n", s.id, s.title, s.status));
    }
    out
}

/// `amb memory index`'s human output.
///
/// **Two of these lines are warnings that must never become failures, and one is the reverse.**
/// A broken link and an unread frontmatter key leave the note indexed and the file intact, so
/// they are reported and the command still succeeds. `unreadable` is different: those notes are
/// *not* in the index, and a vault silently missing notes is what this layer is most exposed to
/// (D62). It was printed only when non-zero and that stays true — a constant `0 unreadable` line
/// is noise — but it is the one number here whose absence is load-bearing.
pub fn render_index(
    stats: &IndexStats,
    problems: &[LinkProblem],
    unknown: &[UnknownKey],
) -> String {
    let mut out = format!(
        "{} scanned · {} indexed · {} unchanged · {} pruned\n",
        stats.scanned, stats.indexed, stats.unchanged, stats.pruned
    );
    for p in problems {
        out.push_str(&format!("  ! {} — {} ({})\n", p.note, p.detail, p.kind));
    }
    for u in unknown {
        out.push_str(&format!(
            "  ? {} — frontmatter key `{}` is read by nothing\n",
            u.note, u.key
        ));
    }
    if stats.unreadable > 0 {
        out.push_str(&format!(
            "  {} file(s) could not be read or parsed and were skipped\n",
            stats.unreadable
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str) -> Step {
        Step {
            id: id.into(),
            title: "t".into(),
            status: ACTIVE.into(),
        }
    }

    /// **The index receipt is a counter whose only reader is a person** — the fourth sighting of
    /// the shape M27 named, after `Redacted.removed`, `capture.rs`'s marker and `export.rs`'s
    /// `written`. Every `+=` in `sync_dir` and `reindex` could become `*=` and stay zero forever,
    /// because nothing asserted the numbers themselves: `amb memory index` would print
    /// `0 scanned · 0 indexed` over a vault it had just walked in full, and the `--json` lane —
    /// a declared stable contract — would say the same to a script.
    ///
    /// The struct is `IndexStats`, which is where D45 already found the inverse defect (a field
    /// with no reader at all, so a 501-note vault reported itself empty). Same struct, opposite
    /// half: there the reader was missing, here the assertion was.
    ///
    /// `-=` is caught too, and by arithmetic rather than by this test's cleverness: these are
    /// `usize`, so a decrement from zero panics in debug.
    #[test]
    fn the_index_receipt_counts_the_work_and_the_line_reports_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let proj = dir.path().join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&proj).expect("vault dir");
        for slug in ["a", "b"] {
            std::fs::write(
                proj.join(format!("{slug}.md")),
                format!("---\nscope: nest\ntitle: t{slug}\nstatus: active\n---\nbody {slug}\n"),
            )
            .expect("note");
        }
        // Not parseable: counted as unreadable, never fatal.
        std::fs::write(proj.join("junk.md"), "no frontmatter here\n").expect("junk");

        let first = reindex(&conn, dir.path(), 0.0).expect("first pass");
        assert_eq!(
            (
                first.scanned,
                first.indexed,
                first.unchanged,
                first.unreadable
            ),
            (3, 2, 0, 1),
            "a first pass indexes what it read and counts what it could not: {first:?}"
        );

        // Second pass over an unchanged vault: the same files, none re-indexed.
        let second = reindex(&conn, dir.path(), 0.0).expect("second pass");
        assert_eq!(
            (second.scanned, second.indexed, second.unchanged),
            (3, 0, 2),
            "an unchanged vault is walked and not rewritten: {second:?}"
        );

        // The numbers reach a person through this line, which is the whole reason they matter.
        let line = render_index(&second, &[], &[]);
        assert!(
            line.starts_with("3 scanned · 0 indexed · 2 unchanged · 0 pruned"),
            "the receipt reports the pass it just made: {line:?}"
        );

        // Deleting the file behind an indexed note is what `pruned` counts.
        std::fs::remove_file(proj.join("a.md")).expect("rm");
        let third = reindex(&conn, dir.path(), 0.0).expect("third pass");
        assert_eq!(
            third.pruned, 1,
            "a note whose file left is pruned: {third:?}"
        );
    }

    /// **Three ways a file in the vault can be uncountable, and each has its own increment.** The
    /// receipt test above reaches only the parse failure; mutating either of the other two stayed
    /// green, so `unreadable` was a number with one third of its writers asserted.
    ///
    /// The walk filters on the `.md` extension alone, which is what makes the first two
    /// reachable at all: a *directory* named `x.md` passes the filter and fails `read_to_string`,
    /// and a filename that is not UTF-8 passes it and fails `to_str`. Neither is hypothetical —
    /// a vault is a directory a person edits, and `mkdir notes.md` is a slip, not an attack.
    #[test]
    fn every_way_a_vault_file_is_unreadable_reaches_the_same_counter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let proj = dir.path().join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&proj).expect("vault dir");
        // A directory wearing the extension: passes the filter, cannot be read as a string.
        std::fs::create_dir(proj.join("a-directory.md")).expect("dir");
        // Bytes that are not UTF-8 inside a file that is otherwise ordinary.
        std::fs::write(proj.join("bad-bytes.md"), [0xff, 0xfe, 0x00, 0x9f]).expect("bytes");

        let stats = reindex(&conn, dir.path(), 0.0).expect("index");
        assert_eq!(
            (stats.scanned, stats.indexed, stats.unreadable),
            (2, 0, 2),
            "both are scanned, neither is indexed, both are counted: {stats:?}"
        );
    }

    /// The same counter through the one path needing a filename the OS allows and Rust cannot
    /// name — `to_str` returning `None`.
    ///
    /// **Linux-only because macOS refuses to create the fixture, which is a fact about the
    /// filesystem rather than about the code.** APFS validates filenames as UTF-8 and returns
    /// `EILSEQ`, so this branch cannot be reached from a test on this machine at all — the
    /// mutation that deletes its counter survives here and is killable only on the other leg.
    /// That is a third category beside "real survivor" and "not compiled here": the code *is*
    /// compiled, and the input is what the platform forbids. `tools/cfg_phantoms.py` classifies
    /// by `cfg` and will therefore call this row real on macOS, correctly — the row is a
    /// question for CI's Linux leg, and this test is the answer it finds there.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_filename_that_is_not_utf8_is_counted_rather_than_skipped_silently() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let proj = dir.path().join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&proj).expect("vault dir");
        let name = std::ffi::OsStr::from_bytes(b"\xff\xfe-bad.md");
        std::fs::write(proj.join(name), "---\nscope: nest\ntitle: t\n---\nbody\n").expect("write");

        let stats = reindex(&conn, dir.path(), 0.0).expect("index");
        assert_eq!(
            (stats.scanned, stats.unreadable),
            (1, 1),
            "a name Rust cannot render is counted, not dropped: {stats:?}"
        );
    }

    /// **The directory outranks the frontmatter, and the comment saying so had no test.**
    /// `safe_component(&note.id.scope) != scope` is what makes a note's *location* the truth when
    /// its own `scope:` key disagrees. Relaxed to `==`, the correction fires only when there is
    /// nothing to correct: a note sitting in `nest/` while claiming `scope: elsewhere` gets
    /// indexed under `elsewhere`, so it is invisible to the project that owns it and appears
    /// under one that does not — D17's central claim, broken by one operator.
    #[test]
    fn the_directory_decides_the_scope_when_the_frontmatter_disagrees() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let proj = dir.path().join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&proj).expect("vault dir");
        std::fs::write(
            proj.join("liar.md"),
            "---\nscope: elsewhere\ntitle: t\nstatus: active\n---\nbody\n",
        )
        .expect("note");
        reindex(&conn, dir.path(), 0.0).expect("index");

        let scopes: Vec<String> = conn
            .prepare("SELECT scope FROM notes WHERE slug = 'liar'")
            .and_then(|mut s| s.query_map([], |r| r.get(0)).and_then(|m| m.collect()))
            .expect("query");
        assert_eq!(
            scopes,
            vec!["nest".to_string()],
            "indexed under the directory that holds it, never under the scope it claimed"
        );
    }

    /// **`excerpt_of` is the corpus `recall` actually searches** (D88), so a mutant that empties
    /// it does not damage a display convenience — it silently deletes most of what search can
    /// find, and every existing test still passes because the *notes* are all still there.
    ///
    /// The boundary rows are the point. `first.chars().count() > 240` decides the ellipsis, and
    /// `==`, `<` and `>=` all survived: exactly 240 characters must not be marked truncated and
    /// 241 must, which is the only pair of fixtures that separates the four operators.
    #[test]
    fn the_excerpt_is_the_first_paragraph_and_the_cap_is_exact() {
        assert_eq!(
            excerpt_of("first para\nsecond line\n\nlater para").as_deref(),
            Some("first para second line"),
            "the first paragraph, newlines flattened, and nothing from the next"
        );
        assert_eq!(
            excerpt_of("").as_deref(),
            None,
            "nothing to excerpt is None"
        );
        assert_eq!(
            excerpt_of("   \n\n rest ").as_deref(),
            None,
            "blank first para is None"
        );

        let at_cap = "x".repeat(240);
        assert_eq!(
            excerpt_of(&at_cap).as_deref(),
            Some(at_cap.as_str()),
            "exactly 240 characters is not truncated and gets no ellipsis"
        );
        let over_cap = "y".repeat(241);
        let got = excerpt_of(&over_cap).expect("241 chars excerpts");
        assert_eq!(got.chars().count(), 241, "240 kept plus the ellipsis");
        assert!(
            got.ends_with('\u{2026}'),
            "241 characters is marked truncated: {got:?}"
        );
    }

    /// **"stands alone" is a sentence written against a silence, and `||` turns it into one.**
    /// The docstring on [`render_history`] says an id with no lineage and an id that does not
    /// exist would otherwise print the same nothing. Relaxing its `&&` to `||` makes a note with
    /// real lineage print that same reassurance — the provenance a person asked for, replaced by
    /// a claim that there is none, at exit 0.
    ///
    /// A truth table rather than one row: the `expected == true` line proves the renderer still
    /// reaches the sentence at all, which an absence-only assertion cannot (M27).
    #[test]
    fn only_a_note_with_no_lineage_at_all_stands_alone() {
        let id = NoteId::observation("nest", "a");
        for (before, after, alone) in [
            (vec![], vec![], true),
            (vec![step("nest/older")], vec![], false),
            (vec![], vec![step("nest/newer")], false),
            (vec![step("nest/older")], vec![step("nest/newer")], false),
        ] {
            let out = render_history(&id, &before, &after);
            assert_eq!(
                out.contains("stands alone"),
                alone,
                "before={} after={} produced {out:?}",
                before.len(),
                after.len()
            );
        }
    }

    /// U5: the renderer's "stands alone" sentence was written so absence would not print as
    /// nothing — and then `history` never checked existence, so a typo'd id printed that same
    /// sentence as a clean provenance, exit 0. The command must miss like every other
    /// id-taking command before the renderer gets a say.
    /// **A supersession cycle is answered, bounded, and short — by the cycle break, not the
    /// budget.** Neither guard in [`history`]'s walks had ever been reached: every fixture was a
    /// straight chain, so the explicit `any(|s| s.id == step.id)` break and the `MAX_STEPS`
    /// budget behind it were both unobserved (the reached-assertion audit, D102's discipline
    /// applied outside the property file). Links come from frontmatter a person can hand-edit,
    /// and this runs on the `SessionStart` path — unbounded here is a hook burning its whole
    /// budget on every fire.
    ///
    /// The `<= 2` is the teeth: deleting either cycle break keeps this green on termination but
    /// turns the answer into `MAX_STEPS` repeated rows, and the length bound reddens. The
    /// presence row proves the walk engaged the cycle rather than stopping at the door.
    #[test]
    fn a_supersession_cycle_terminates_the_walk_without_flooding_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let proj = dir.path().join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&proj).expect("vault dir");
        std::fs::write(
            proj.join("a.md"),
            "---\nscope: nest\ntitle: t\nstatus: superseded\nsuperseded_by: nest/b\n---\nbody\n",
        )
        .expect("a");
        std::fs::write(
            proj.join("b.md"),
            "---\nscope: nest\ntitle: t\nstatus: superseded\nsuperseded_by: nest/a\n---\nbody\n",
        )
        .expect("b");
        crate::memory::reindex(&conn, dir.path(), 0.0).expect("index");

        let (ancestors, descendants) =
            history(&conn, &NoteId::observation("nest", "a")).expect("a cycle is answered");
        assert!(
            descendants.iter().any(|s| s.id == "nest/b"),
            "the walk engaged the cycle: {descendants:?}"
        );
        assert!(
            descendants.len() <= 2 && ancestors.len() <= 2,
            "the cycle break answers, not the step budget: {} down, {} up",
            descendants.len(),
            ancestors.len()
        );
    }

    #[test]
    fn history_of_a_nonexistent_note_is_a_miss_not_a_clean_lineage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let err = history(&conn, &NoteId::observation("nest", "no-such-note"))
            .expect_err("an id that resolves to nothing");
        assert!(matches!(err, Error::NoSuchNote(_)), "{err:?}");

        conn.execute(
            "INSERT INTO notes (slug, kind, scope, vault_path, title, status, created,
                                mtime, indexed_at)
             VALUES ('real', 'observation', 'nest', 'p/real.md', 't', 'active', 1.0, 1.0, 1.0)",
            [],
        )
        .expect("seed");
        let (before, after) = history(&conn, &NoteId::observation("nest", "real"))
            .expect("a real note with no lineage is fine");
        assert!(before.is_empty() && after.is_empty());
    }

    /// **Standing alone is a sentence.** Otherwise a note with no lineage and an id that does not
    /// exist print the same nothing, and this project's failures are silences.
    #[test]
    fn a_note_with_no_lineage_says_so_instead_of_printing_nothing() {
        let out = render_history(&NoteId::observation("nest", "a"), &[], &[]);
        crate::assert_rendered_shape("render_history", &out);
        assert!(out.contains("stands alone"), "{out}");
        assert!(out.contains("nest/a"), "{out}");
    }

    /// The subject sits between what it replaced and what replaced it, and the arrows say which
    /// way time runs. A test that only counted lines would pass with the two halves swapped.
    #[test]
    fn lineage_reads_downwards_with_the_subject_in_the_middle() {
        let out = render_history(
            &NoteId::observation("nest", "middle"),
            &[step("nest/older")],
            &[step("nest/newer")],
        );
        let older = out.find("nest/older").expect("the replaced note");
        let subject = out.find("> nest/middle").expect("the subject");
        let newer = out.find("nest/newer").expect("the replacement");
        assert!(older < subject && subject < newer, "{out}");
        assert_eq!(out.matches('↓').count(), 2, "one arrow per hop: {out}");
    }

    /// The per-file probe must reach `ix_notes_vault`, and the guard is on the *plan*.
    ///
    /// `sync_dir` runs this SELECT once per markdown file, on the `SessionStart` hook path.
    /// Before migration 13 it seeked on `kind` via the primary key and then walked every note of
    /// that kind per probe — quadratic in vault size, and invisible to every result-shaped
    /// assertion because the rows were always right. Dropping the index from the migration
    /// reddens this.
    #[test]
    fn the_sync_probe_reaches_the_vault_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        crate::assert_query_plan_uses(
            &conn,
            SYNC_PROBE_SQL,
            vec![
                "observation".to_string().into(),
                "nest/x.md".to_string().into(),
            ],
            "ix_notes_vault",
        );
    }

    fn stats(unreadable: usize) -> IndexStats {
        IndexStats {
            scanned: 10,
            indexed: 3,
            unchanged: 7,
            unreadable,
            pruned: 0,
            skipped: false,
        }
    }

    /// **`unreadable` is the one count here whose silence is load-bearing.**
    ///
    /// A broken link and an unread frontmatter key leave the note indexed; those are warnings. An
    /// unreadable file is *not in the index*, which is D62's failure and the one this layer is
    /// most exposed to. It is printed only when non-zero — a constant `0` line is noise — so the
    /// absence of the line is itself a claim, and both directions are asserted.
    #[test]
    fn an_unreadable_note_is_reported_and_a_clean_pass_stays_quiet() {
        let clean = render_index(&stats(0), &[], &[]);
        crate::assert_rendered_shape("render_index", &clean);
        assert!(clean.starts_with("10 scanned · 3 indexed · 7 unchanged · 0 pruned"));
        assert!(!clean.contains("could not be read"), "{clean}");

        let broken = render_index(&stats(2), &[], &[]);
        assert!(broken.contains("2 file(s) could not be read"), "{broken}");
    }

    /// **The decline above the limit is written here and read two layers away, and only the
    /// readers were tested.** `index_is_behind` is asserted on a hand-built `IndexStats` (D78)
    /// and the banner renders from its answer — but no test called `sync_dir` with a limit at
    /// all, so the write at the top of that chain could vanish and everything downstream stayed
    /// green while a 501-note vault reported itself empty again (D45). M20's arithmetic: three
    /// layers carry the rule, and the untested one was the writer.
    ///
    /// Three rows and an omission: over the limit declines and says how big the vault is, *at*
    /// the limit indexes (the bound is `>`, and that row is what reddens `>=`), and a declined
    /// pass prunes nothing — the prune below the gate compares the index to a scan that never
    /// happened, and the early return is all that protects it.
    #[test]
    fn a_vault_past_the_limit_declines_loudly_and_prunes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let vault = dir.path().join("vault");
        let notes_dir = vault.join(vault_dir(OBSERVATION, "nest"));
        std::fs::create_dir_all(&notes_dir).expect("vault dir");
        for slug in ["a", "b", "c"] {
            std::fs::write(
                notes_dir.join(format!("{slug}.md")),
                "---\nscope: nest\ntitle: t\n---\nbody\n",
            )
            .expect("note");
        }

        // Seed without a limit, so the declined pass below has rows it could damage.
        let seeded = sync_dir(&conn, &vault, OBSERVATION, "nest", 1.0, None).expect("seed");
        assert_eq!((seeded.indexed, seeded.skipped), (3, false));

        let declined =
            sync_dir(&conn, &vault, OBSERVATION, "nest", 2.0, Some(2)).expect("declined");
        assert!(declined.skipped, "3 files over a limit of 2 must decline");
        assert_eq!(
            declined.scanned, 3,
            "the decline still reports the size it declined at"
        );
        assert_eq!(declined.indexed, 0);
        let kept: i64 = conn
            .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
            .expect("count");
        assert_eq!(
            kept, 3,
            "a declined pass must not prune the index against an empty scan"
        );

        // The bound is `>`: a vault exactly at the limit is indexed, not declined.
        let at_limit =
            sync_dir(&conn, &vault, OBSERVATION, "nest", 3.0, Some(3)).expect("at limit");
        assert_eq!((at_limit.skipped, at_limit.unchanged), (false, 3));
    }

    /// Two warning classes, two markers, and neither may be mistaken for the other: `!` is a link
    /// pointing at nothing, `?` is a key nothing reads. Both leave the command succeeding.
    #[test]
    fn a_broken_link_and_an_unread_key_are_distinguishable_warnings() {
        let out = render_index(
            &stats(0),
            &[LinkProblem {
                note: "nest/a".into(),
                kind: "cites".into(),
                detail: "no such note".into(),
            }],
            &[UnknownKey {
                note: "nest/b.md".into(),
                key: "projekt".into(),
            }],
        );
        assert!(out.contains("  ! nest/a — no such note (cites)"), "{out}");
        assert!(
            out.contains("  ? nest/b.md — frontmatter key `projekt` is read by nothing"),
            "{out}"
        );
    }

    /// **Asserted against `vault_dir`, which is what production calls.**
    ///
    /// This tested `project_dir` — and after `observe` was routed through `vault_dir` (D86),
    /// nothing in production called `project_dir` at all. So the rule that a hostile project name
    /// cannot walk out of the vault was being proved about code that no longer ran, while the
    /// live path went unguarded. D84's finding exactly, caught by `find_unread_fields.py`'s
    /// advisory in the commit that caused it. `project_dir` is deleted rather than kept for the
    /// test.
    ///
    /// Every arm, because `vault_dir` has five and a single-arm test would leave four unproved.
    #[test]
    fn a_hostile_scope_name_stays_inside_the_vault_whatever_the_kind() {
        let hostile = "../../../etc";
        for (kind, expect) in [
            (OBSERVATION, "projects/"),
            (DECISION, "decisions/"),
            (CAPTURE, "captures/"),
        ] {
            let dir = vault_dir(kind, hostile);
            // **One component after the parent is the property**, not the absence of dots.
            // `safe_component` rewrites the separators rather than the characters, so a hostile
            // name becomes `-..-..-etc` — which cannot traverse, because traversal needs a `/`
            // that makes `..` its own segment. Asserting "no dots" would have failed on safe
            // output and taught the next reader to weaken the check.
            let rest = dir.strip_prefix(expect).expect("wrong parent");
            assert!(
                !rest.contains('/') && rest != ".." && rest != ".",
                "{kind} escaped: {dir}"
            );
        }
        // The two that ignore the scope entirely still must not take it from the caller.
        assert_eq!(vault_dir(CANDIDATE, hostile), "candidates");
        // A hostile *topic* falls through to the project arm rather than `topics/` — the sigil
        // does not survive sanitisation — and that is fine: the property is containment, not
        // which parent it lands under.
        for scope in ["#../../etc", "../..", "..", "@@/../x"] {
            for kind in [OBSERVATION, DECISION, CANDIDATE, CAPTURE] {
                let dir = vault_dir(kind, scope);
                assert!(
                    dir.split('/')
                        .all(|c| c != ".." && c != "." && !c.is_empty()),
                    "{kind}/{scope} produced a traversable path: {dir}"
                );
                assert!(
                    dir.split('/').count() <= 2,
                    "{kind}/{scope} produced more nesting than the layout has: {dir}"
                );
            }
        }
        assert_eq!(vault_dir(DECISION, crate::address::GLOBAL), "global");
    }
    /// The defect and its repair, asserted as a pair, because either half alone misleads.
    ///
    /// Migration 6 -> 7 cleared `content_hash` to force a re-derive and nothing happened: the
    /// gate is `mtime`, and `sync_dir` returns before the cleared column is ever looked at. A
    /// test that only proved the repair works would leave the next person free to write the same
    /// migration again, so the first half pins the behaviour that made it necessary.
    #[test]
    fn clearing_a_derived_column_does_not_re_derive_it_but_clearing_the_gate_does() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vault = dir.path().join("vault");
        let proj = vault.join("projects").join("nest");
        std::fs::create_dir_all(&proj).expect("mkdir");
        std::fs::write(
            proj.join("a.md"),
            "---\nscope: nest\ntitle: t\nstatus: superseded\nsuperseded_by: nest/b\n\
             files:\n  - src/x.rs\n---\nbody\n",
        )
        .expect("write");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");

        // **Two derived tables, not one column.** This used to read `notes.content_hash`, which
        // D85 dropped for having no reader — so the test that taught D67's lesson had to stop
        // resting on it. `note_paths` and `note_links` are what the index actually derives from a
        // file, and using both keeps this as strong as it was: a repair that rebuilt one and not
        // the other would still be caught.
        let derived = |c: &Connection| -> (i64, i64) {
            (
                c.query_row("SELECT count(*) FROM note_paths", [], |r| r.get(0))
                    .expect("paths"),
                c.query_row("SELECT count(*) FROM note_links", [], |r| r.get(0))
                    .expect("links"),
            )
        };

        reindex(&conn, &vault, 1.0).expect("first index");
        assert_eq!(
            derived(&conn),
            (1, 1),
            "the first pass derives the path and the supersession edge"
        );

        // Exactly what migration 6 -> 7 did: invalidate the derived values, leave `mtime` alone.
        conn.execute_batch("DELETE FROM note_paths; DELETE FROM note_links;")
            .expect("simulate the bad migration");
        reindex(&conn, &vault, 2.0).expect("second index");
        assert_eq!(
            derived(&conn),
            (0, 0),
            "clearing a derived value must NOT repair it — if this ever passes by repairing, the \
             gate has moved and migration 7 -> 8 has become unnecessary rather than wrong"
        );

        // Migration 7 -> 8. One column, and it is the one the skip is decided on.
        conn.execute_batch("UPDATE notes SET mtime = 0;")
            .expect("simulate the repair");
        reindex(&conn, &vault, 3.0).expect("third index");
        assert_eq!(
            derived(&conn),
            (1, 1),
            "clearing the gate re-derives both in the same pass"
        );
    }
}

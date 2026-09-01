//! Writing a note into the vault, and superseding one.
//!
//! The only paths that author files, and both run on explicit invocation.

use super::*;

// ── Writing ─────────────────────────────────────────────────────────────────

/// What `amb memory observe` was asked to record.
///
/// **Structured, not free text**, following the shape claude-mem's corpus validates rather than
/// the one it abandoned: across 80,264 rows its `title`, `facts` and `files_modified` columns are
/// 100% filled and its free-text `text` column is filled 0% of the time. `--summary "…"` was this
/// plan's first design and is the one the incumbent stopped using.
#[derive(Debug, Clone)]
pub struct Observation<'a> {
    /// [`OBSERVATION`] or [`CAPTURE`] — both are project-scoped, and they differ only in whether
    /// a session may be shown them (D86).
    ///
    /// **Explicit at every call site rather than defaulted.** A default here would mean the
    /// `PostToolUseFailure` path silently reverting to `observation` if the field were ever
    /// dropped from that construction, which is precisely the failure the kind exists to prevent
    /// and precisely the kind that leaves no trace.
    pub kind: &'a str,
    pub title: &'a str,
    pub learned: &'a str,
    pub project: &'a str,
    pub files: &'a [String],
    pub cites: &'a [String],
    pub supersedes: Option<&'a str>,
    /// How binding the note is. `advice` unless the caller says otherwise.
    pub force: &'a str,
}

/// What a write actually did — including how much of it was thrown away.
#[derive(Debug, Clone)]
pub struct Written {
    pub id: NoteId,
    pub path: PathBuf,
    /// Values the redactor removed. **Reported rather than silent**: a surprising redaction is
    /// visible to the author while they are still in the session that made it.
    pub redacted: usize,
    pub cited: Vec<NoteId>,
    pub superseded: Option<NoteId>,
}

impl Written {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id.display(),
            "path": self.path.display().to_string(),
            "redacted": self.redacted,
            "cites": self.cited.iter().map(NoteId::display).collect::<Vec<_>>(),
            "supersedes": self.superseded.as_ref().map(NoteId::display),
        })
    }
}

/// Write one observation to the vault and index it.
///
/// Order matters and is not incidental: **everything that can fail is resolved before anything is
/// written.** An unknown `--cites` id must not leave a note on disk that the index disagrees
/// with, because the file is the authority and a half-applied write would make it lie.
pub fn observe(
    conn: &Connection,
    me: &Identity,
    obs: &Observation<'_>,
    at: f64,
) -> Result<Written> {
    let vault = require_vault()?;

    let cited = obs
        .cites
        .iter()
        .map(|c| resolve(conn, c))
        .collect::<Result<Vec<_>>>()?;
    let superseded = obs.supersedes.map(|s| resolve(conn, s)).transpose()?;

    // Redaction happens before the slug, so a secret in a title cannot survive as a filename.
    let title = redact(obs.title);
    let learned = redact(obs.learned);
    let redacted = title.removed + learned.removed;

    // **`vault_dir` decides, not this function.** These two lines used to spell `projects/…`
    // themselves — once as a `PathBuf` here and once as a `String` for `rel` below — so the
    // layout was written down in three places and only one of them was the authority the vault
    // walk is tested against. D86 needed a second directory and that divergence is what would
    // have made a capture land in `projects/` with `captures/` in its index row.
    let dir = vault.join(vault_dir(obs.kind, obs.project));
    std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;

    let (slug, path) = free_slug(&dir, &format_date(at), &slugify(&title.text));
    let note = Note {
        id: NoteId::scoped(obs.kind, obs.project, &slug),
        title: title.text.clone(),
        status: ACTIVE.to_string(),
        created: at,
        session: Some(me.id.clone()),
        agent: Some(me.name.clone()),
        files: obs.files.to_vec(),
        cites: cited.iter().map(NoteId::display).collect(),
        supersedes: superseded.as_ref().map(NoteId::display),
        superseded_by: None,
        promoted_from: None,
        promoted_to: None,
        visibility: None,
        force: obs.force.to_string(),
        declined_at: None,
        declined_after: None,
        derivations: Vec::new(),
        body: learned.text,
    };

    let rendered = note.render();
    write_private(&path, &rendered)?;
    let rel = vault_rel(obs.kind, obs.project, &slug);
    upsert(conn, &note, &rel, file_mtime(&path), at)?;

    record_cited(conn, &me.id, &cited, at)?;
    if let Some(old) = &superseded {
        supersede(conn, &vault, old, &note.id, at)?;
    }
    Ok(Written {
        id: note.id,
        path,
        redacted,
        cited,
        superseded,
    })
}

/// A stem nobody is using yet.
///
/// Two observations on the same day with the same title are a real case — the same lesson
/// learned twice — and silently overwriting the first would lose a note, which is the one thing
/// this design promises never to do.
fn free_slug(dir: &Path, date: &str, base: &str) -> (String, PathBuf) {
    let mut n = 1;
    loop {
        let slug = if n == 1 {
            format!("{date}-{base}")
        } else {
            format!("{date}-{base}-{n}")
        };
        let path = dir.join(format!("{slug}.md"));
        if !path.exists() || n > 200 {
            return (slug, path);
        }
        n += 1;
    }
}

/// The temporary sibling one process writes before renaming over `path`.
///
/// A function rather than an inline `format!` so the pid-scoping is a property a test can hold
/// — the race it prevents needs two processes to demonstrate, the name needs none.
fn tmp_for(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!(
        "{}.amb-tmp.{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("md"),
        std::process::id()
    ))
}

/// Write a note readable only by its owner.
///
/// **The file mode is ours to choose; the directory's is not (D31).** `amb` creates files inside
/// a directory the user chose and points Obsidian at — one that may well be a git repository or
/// a sync root — so its mode is never touched. The file carries session observations, which is
/// the same class of content the board is 0600 for.
/// **Written to a sibling temporary file and renamed, because `std::fs::write` truncates first.**
/// That is `open(O_TRUNC)` followed by `write`, so between the two the file is zero bytes — and
/// the vault is truth while the index stores no note content (D34), so a process that dies in that
/// window destroys the note permanently. Every write here except the first is a *rewrite* of an
/// existing note: `derive` adds a strike, `promote` archives a candidate, `supersede` retires one.
/// A crash mid-`derive` would have lost weeks of accumulated derivations to save a rename.
///
/// `rename(2)` is atomic within a filesystem, so a reader sees either the old note or the new one
/// and never a partial one. The temporary file is a sibling rather than in `/tmp` for exactly that
/// reason — a cross-device rename is not atomic and would fall back to a copy.
///
/// The mode is set on the temporary file *before* the rename, so the note is never briefly
/// world-readable under a name anything is watching.
pub(crate) fn write_private(path: &Path, contents: &str) -> Result<()> {
    // Pid-scoped, like the settings writer in `hooks.rs` and unlike this line until audit round
    // two. A fixed temp name means two processes rewriting the same note interleave on one path
    // — writer A's rename can publish writer B's half-written bytes — and `observe` and
    // `supersede` take no board lock, so nothing upstream prevents that. The settings writer
    // pid-scoped its temp on purpose the day it was written; this one is the sibling that was
    // left standing (D86/D88/D90's shape). An orphaned `.amb-tmp.<pid>` from a crash is inert:
    // the index scans only `.md` files.
    let tmp = tmp_for(path);
    std::fs::write(&tmp, contents).map_err(io(format!("writing {}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        // The rename is what makes this atomic; if it fails the temporary file is the only
        // evidence, so it is left rather than cleaned up silently.
        io(format!(
            "renaming {} into place at {}",
            tmp.display(),
            path.display()
        ))(e)
    })
}

/// Retire a note, in the file first and the index second.
///
/// **Contradiction had no representation at all before this, and that was the worst of the three
/// options.** A vault holds "we use X" and later "we moved off X"; with no supersession both are
/// injected and the model picks. Detecting contradiction automatically is out of scope;
/// representing it is not optional (D40).
pub fn supersede(
    conn: &Connection,
    vault: &Path,
    old: &NoteId,
    by: &NoteId,
    at: f64,
) -> Result<()> {
    let rel: String = conn
        .query_row(
            "SELECT vault_path FROM notes WHERE kind = ?1 AND scope = ?2 AND slug = ?3",
            params![old.kind, old.scope, old.slug],
            |r| r.get(0),
        )
        .map_err(sql("locating the note to supersede"))?;
    let path = vault.join(&rel);

    // The file first: it is the authority, and an index that says `superseded` over a file that
    // does not is the drift `--check` exists to catch on the export side.
    // **Re-derived through `upsert` rather than hand-updated, and that is a correction.** It used to `UPDATE`
    // three columns it knew about, which made it a *second* derivation path beside `index_note` —
    // one that could not know about anything derived later. It could not: `note_links` was added
    // in schema 6 and this function is the only writer of the one edge that exists, so the link
    // was never indexed and `history` returned nothing for a chain the files described perfectly.
    // Caught by the validator shipped in the same commit, which is the argument for shipping them
    // together (D63).
    let rel = path
        .strip_prefix(vault)
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned();
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Some(mut note) = parse_note(&text, &old.slug, at)
    {
        note.status = SUPERSEDED.to_string();
        note.superseded_by = Some(by.display());
        let rendered = note.render();
        write_private(&path, &rendered)?;
        upsert(conn, &note, &rel, file_mtime(&path), at)?;
    }
    Ok(())
}

/// The whole of `amb memory observe`'s human output, as text.
///
/// **Pure, for D92's reason.** Four decisions lived in `run_memory`, in the one file with no
/// tests: whether a redaction is announced at all, whether a derivation counted or was
/// downgraded to a citation, whether near candidates are offered, and whether a supersession is
/// named. Each is a sentence a reader believes and each was one deleted `if` from silently
/// changing.
///
/// **The order is part of the contract.** `near` is emitted after the note's own lines because a
/// near-match shown here is itself an injection (see [`near_candidates`]) — the author is being
/// told about a candidate they did not know about, *after* writing, never before thinking.
pub fn render_written(w: &Written, derived: Option<&Derived>, near: &[IndexedNote]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "recorded {} → {}\n",
        w.id.display(),
        w.path.display()
    ));
    // Said out loud. A redaction the author cannot see is one they cannot correct, and they are
    // still in the session that wrote it.
    if w.redacted > 0 {
        out.push_str(&format!(
            "  {} value(s) redacted before writing\n",
            w.redacted
        ));
    }
    for c in &w.cited {
        out.push_str(&format!("  cites {}\n", c.display()));
    }
    if let Some(d) = derived {
        if d.independent {
            out.push_str(&format!(
                "  {} → {}/{} independent derivation(s)\n",
                d.id.display(),
                d.count,
                threshold()
            ));
        } else {
            out.push_str(&format!(
                "  {} → not counted: this session was already shown something about these \
                 paths, so it is a citation\n",
                d.id.display()
            ));
        }
    }
    for n in near {
        out.push_str(&format!("  near: {} — {}\n", n.id.display(), n.title));
        out.push_str(&format!("        link it with `--same-as {}`\n", n.id.slug));
    }
    if let Some(old) = &w.superseded {
        out.push_str(&format!(
            "  superseded {} — it will not be injected again\n",
            old.display()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    /// The temp name must carry the pid, or two writers interleave on one path.
    ///
    /// Asserted on the constructed name rather than on a race: forcing the race takes two
    /// processes and a scheduler's cooperation, while the property that prevents it is a string
    /// this test can hold. Removing the pid from [`super::tmp_for`] reddens this.
    #[test]
    fn the_temp_name_is_scoped_to_this_process() {
        let tmp = super::tmp_for(std::path::Path::new("/v/notes/a-slug.md"));
        let name = tmp.file_name().and_then(|n| n.to_str()).expect("utf8 name");
        assert!(
            name.ends_with(&format!(".amb-tmp.{}", std::process::id())),
            "another process picks the same temp path: {name}"
        );
    }

    use super::*;

    fn written() -> Written {
        Written {
            id: NoteId::observation("nest", "a-thing"),
            path: PathBuf::from("/v/projects/nest/a-thing.md"),
            redacted: 0,
            cited: Vec::new(),
            superseded: None,
        }
    }

    fn near(slug: &str) -> IndexedNote {
        IndexedNote {
            id: NoteId::candidate(slug),
            title: "something close".into(),
            status: ACTIVE.into(),
            created: 0.0,
            vault_path: format!("candidates/{slug}.md"),
            excerpt: None,
            paths: vec!["src/lib.rs".into()],
            force: ADVICE.into(),
        }
    }

    /// **A redaction is announced only when one happened, and it must be announced.**
    ///
    /// Both halves are rules. A constant `0 value(s) redacted` line is noise that trains the
    /// author to skip the paragraph the real one appears in; a missing line means a value the
    /// author cannot see and therefore cannot correct, while they are still in the session that
    /// wrote it. Delete the `if` and the first assertion reddens; delete the body and the second.
    #[test]
    fn a_redaction_is_announced_when_it_happened_and_never_otherwise() {
        assert!(!render_written(&written(), None, &[]).contains("redacted"));
        let mut w = written();
        w.redacted = 2;
        assert!(render_written(&w, None, &[]).contains("2 value(s) redacted before writing"));
    }

    /// The same rule `render_derived` guards, on the other surface that states it.
    ///
    /// D90's arithmetic: this sentence has two renderers, so it needs two assertions. Fixing one
    /// and not the other is exactly how `quoted()` stayed unguarded on `amb inbox`.
    #[test]
    fn observe_states_the_counting_rule_the_same_way_derive_does() {
        let d = Derived {
            id: NoteId::candidate("lock-order"),
            created: false,
            independent: false,
            count: 1,
            projects: vec!["nest".into()],
            path: PathBuf::from("/v/candidates/lock-order.md"),
            redacted: 0,
        };
        let out = render_written(&written(), Some(&d), &[]);
        crate::assert_rendered_shape("render_written", &out);
        assert!(out.contains("not counted"), "{out}");
        assert!(out.contains("it is a citation"), "{out}");

        let independent = Derived {
            independent: true,
            count: 2,
            ..d
        };
        let out = render_written(&written(), Some(&independent), &[]);
        assert!(
            out.contains(&format!("2/{} independent derivation(s)", threshold())),
            "and the count is shown against the threshold it is racing: {out}"
        );
    }

    /// **A near-match is offered after the note is written, never before.**
    ///
    /// The order is the rule, not decoration: showing a candidate *first* would be injecting
    /// context into a writer who is still deciding, which is what `INJECTABLE` exists to prevent.
    /// Shown afterwards it is a linking affordance offered to someone who has already thought.
    #[test]
    fn a_near_match_is_offered_after_the_note_and_names_the_flag_that_links_it() {
        let out = render_written(&written(), None, &[near("lock-order")]);
        let recorded = out.find("recorded ").expect("the note's own line");
        let offered = out.find("  near: ").expect("the offer");
        assert!(recorded < offered, "the offer must follow the note: {out}");
        assert!(
            out.contains("--same-as lock-order"),
            "and name the flag that acts on it: {out}"
        );
    }

    #[test]
    fn a_supersession_is_named_and_says_what_it_costs_the_old_note() {
        let mut w = written();
        assert!(!render_written(&w, None, &[]).contains("superseded"));
        w.superseded = Some(NoteId::observation("nest", "the-old-one"));
        let out = render_written(&w, None, &[]);
        assert!(out.contains("superseded nest/the-old-one"), "{out}");
        assert!(
            out.contains("will not be injected again"),
            "the consequence, not just the fact: {out}"
        );
    }
}

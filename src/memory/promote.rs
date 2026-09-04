//! Phase 2 — candidates, the derivation ledger and promotion (D49).
//!
//! Candidates are never injected (D51), which is what makes a rediscovery
//! evidence rather than the system's own echo. Promotion never writes without
//! a person saying yes.

use super::*;

/// Releases the vault lock however the caller leaves — including through an early `?`.
///
/// Without this, one error return would hold the board's write lock until the process exited, and
/// every other session's memory hook would block behind it. A hook that hangs is exactly what D41
/// separated the entries to prevent.
struct LockGuard<'a>(&'a Connection);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        // The filesystem write has already happened and no rollback can undo it, so this commits
        // rather than rolls back: the transaction exists to order writers, not to make them atomic.
        let _ = self.0.execute_batch("COMMIT");
    }
}

/// Whether a derivation recorded in this session is *independent*.
///
/// > A derivation is independent when **nothing injected into that session** — observation,
/// > decision or pattern — concerned the paths the new note concerns. Anything else is a citation.
///
/// **The earlier draft claimed candidates were independent "by construction" because they are
/// never injected. That was false and it was load-bearing:** observations are injected, and so are
/// decisions and patterns, so an agent that read an injected note about auth lock ordering and
/// then proposed a candidate about auth lock ordering produced a citation wearing a derivation's
/// clothes. The guard covered one of three injected kinds.
///
/// Checkable only because Phase 1's ledger records what each session was shown — which is why the
/// id echo had to move into Phase 1 before this phase could exist at all.
pub fn independent(conn: &Connection, session: &str, paths: &[String]) -> Result<bool> {
    if paths.is_empty() {
        // Nothing to be contaminated by. A note about no particular file cannot have been primed
        // by a path-anchored injection.
        return Ok(true);
    }
    let mut stmt = conn
        .prepare(
            "SELECT p.path_glob FROM note_events e
               JOIN note_paths p
                 ON p.kind = e.kind AND p.scope = e.scope AND p.slug = e.slug
              WHERE e.session = ?1 AND e.event IN ('injected', 'injected_file')",
        )
        .map_err(sql("checking derivation independence"))?;
    let shown: Vec<String> = stmt
        .query_map(params![session], |r| r.get(0))
        .map_err(sql("checking derivation independence"))?
        .flatten()
        .collect();
    // **The same predicate the path lane retrieves with — necessarily the same one, or this
    // check answers a different question than the one that did the priming.** It was
    // `claims::overlaps` while retrieval was too; when retrieval learned patterns this had to
    // learn them in the same breath, because a session shown a note declaring `src/memory/**`
    // would otherwise pass here as though it had never been shown anything about
    // `src/memory/index.rs`, and the ledger would record a primed derivation as independent.
    // That is D49's arithmetic rather than a retrieval nicety, which is why the three sites move
    // together. `src/a` still must not be taken to have primed a note about `src/abc.rs`.
    Ok(!shown
        .iter()
        .any(|g| paths.iter().any(|p| path_matches(g, p))))
}

/// Candidates concerning these paths, for the `observe`-time linking affordance.
///
/// **Dedup is an affordance, not an algorithm.** Deciding that a new note "is the same as" an
/// existing candidate was the hardest unsolved piece of this phase; it does not need solving.
/// Retrieval is already path-anchored, so the candidates touching these paths are a free query —
/// show them with their ids and let the caller echo one with `--same-as`. A miss creates a
/// visible duplicate, which is mergeable; a wrong merge is neither visible nor reversible.
pub fn candidates_concerning(conn: &Connection, paths: &[String]) -> Result<Vec<IndexedNote>> {
    let mut out: Vec<IndexedNote> = Vec::new();
    for path in paths {
        for n in concerning_kind(conn, CANDIDATE, path)? {
            if !out.iter().any(|k| k.id == n.id) {
                out.push(n);
            }
        }
    }
    Ok(out)
}

/// [`concerning`] for one kind. Shares the over-select-then-confirm shape.
fn concerning_kind(conn: &Connection, kind: &str, path: &str) -> Result<Vec<IndexedNote>> {
    let sql_text = format!(
        "{SELECT_NOTE}
          WHERE n.kind = ?1 AND n.status = ?2 AND {}
          ORDER BY n.created DESC LIMIT {PATH_LOOKUP_WINDOW}",
        path_prefilter("?3")
    );
    let mut stmt = conn
        .prepare(&sql_text)
        .map_err(sql("preparing a candidate lookup"))?;
    let rows: Vec<IndexedNote> = stmt
        .query_map(params![kind, ACTIVE, path], row_to_note)
        .map_err(sql("running a candidate lookup"))?
        .flatten()
        .filter(|n| n.paths.iter().any(|g| path_matches(g, path)))
        .collect();
    Ok(rows)
}

/// What recording a derivation did.
#[derive(Debug, Clone)]
pub struct Derived {
    pub id: NoteId,
    pub created: bool,
    /// False when the session had already been shown something about these paths, so this is a
    /// citation rather than a derivation. **The count does not move.**
    pub independent: bool,
    pub count: usize,
    pub projects: Vec<String>,
    pub path: PathBuf,
    /// How many values `redact` removed from what this call actually wrote.
    ///
    /// **Rendered, and that is what makes it load-bearing rather than bookkeeping** (M27). `write.rs`
    /// prints `"N value(s) redacted before writing"` under `if w.redacted > 0` and its comment gives
    /// the reason: a redaction the author cannot see is one they cannot correct. `derive` called
    /// `redact(...).text` at three sites and threw `.removed` away at every one, so promotion's
    /// ledger redacted **silently** — in the one flow D49 designed entirely around a human seeing
    /// what they approve.
    pub redacted: usize,
}

/// Record a derivation against a candidate, creating it if it does not exist.
///
/// **The count only moves for an independent derivation.** A dependent one is still written to the
/// file as evidence — with `independent: false` visible in the caller's report — because throwing
/// it away would hide the fact that the thing keeps coming up; it simply does not earn a strike.
pub fn derive(
    conn: &Connection,
    me: &Identity,
    slug: &str,
    title: &str,
    paths: &[String],
    note: &str,
    at: f64,
) -> Result<Derived> {
    let vault = require_vault()?;
    let id = NoteId::candidate(&slugify(slug));
    let rel = vault_rel(CANDIDATE, "", &id.slug);
    let path = vault.join(&rel);
    let independent = independent(conn, &me.id, paths)?;

    // Everything from here to the write is one critical section: read the current count, add to
    // it, write it back. Two processes interleaving inside it lose a strike each time.
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(sql("taking the vault lock for derive"))?;
    let _guard = LockGuard(conn);
    derive_locked(
        conn,
        me,
        &id,
        &rel,
        &path,
        title,
        paths,
        note,
        independent,
        at,
    )
}

#[allow(clippy::too_many_arguments)]
fn derive_locked(
    conn: &Connection,
    me: &Identity,
    id: &NoteId,
    rel: &str,
    path: &Path,
    title: &str,
    paths: &[String],
    note: &str,
    independent: bool,
    at: f64,
) -> Result<Derived> {
    let id = id.clone();
    let existing = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| parse_note(&t, &id.slug, at));
    let created = existing.is_none();

    // Redacted once each, not three times. The two `redact(note)` calls this replaces computed the
    // same answer twice and discarded both counts.
    let title_r = redact(title);
    let note_r = redact(note);

    let mut candidate = existing.unwrap_or_else(|| Note {
        id: id.clone(),
        title: title_r.text.clone(),
        status: ACTIVE.into(),
        created: at,
        session: Some(me.id.clone()),
        agent: Some(me.name.clone()),
        files: Vec::new(),
        cites: Vec::new(),
        supersedes: None,
        superseded_by: None,
        promoted_from: None,
        promoted_to: None,
        visibility: None,
        force: ADVICE.to_string(),
        declined_at: None,
        declined_after: None,
        derivations: Vec::new(),
        body: note_r.text.clone(),
    });
    candidate.id = id.clone();

    // **Counted for what is written, not for what was examined.** An existing candidate keeps its
    // stored title and body, so the only new text on that path is the derivation line — counting
    // the whole note there would report removals that never reached the file, and a number that
    // overstates is as untrustworthy as one that understates.
    let redacted = if created {
        title_r.removed + note_r.removed
    } else if independent {
        redact(note.lines().next().unwrap_or("")).removed
    } else {
        0
    };

    // Paths accumulate: a candidate is the union of what its derivations concerned, which is what
    // makes the next path lookup find it.
    for p in paths {
        if !candidate.files.contains(p) {
            candidate.files.push(p.clone());
        }
    }
    if independent {
        candidate.derivations.push(Derivation {
            ts: at,
            project: me.project.clone(),
            session: me.id.clone(),
            note: note_r.text.lines().next().unwrap_or("").to_string(),
            // Detected here, from the repository this session is standing in, because this is the
            // only moment anything knows where that repository is (D82).
            topics: crate::memory::detect(Path::new(&me.root)),
        });
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let rendered = candidate.render();
    write_private(path, &rendered)?;
    upsert(conn, &candidate, rel, file_mtime(path), at)?;

    let mut projects: Vec<String> = candidate
        .derivations
        .iter()
        .map(|d| d.project.clone())
        .collect();
    projects.sort();
    projects.dedup();
    Ok(Derived {
        id,
        created,
        independent,
        count: candidate.derivations.len(),
        projects,
        path: path.to_path_buf(),
        redacted,
    })
}

/// Candidates that have reached the threshold and are not declined, expired or promoted.
///
/// **A declined candidate does not come back until it derives again.** Without that, declining
/// costs more than assenting and approval becomes the path of least resistance — which is D16's
/// objection returning through the back door (D49).
pub fn ready_candidates(conn: &Connection, vault: &Path, at: f64) -> Result<Vec<Note>> {
    let mut stmt = conn
        .prepare(
            "SELECT vault_path FROM notes
              WHERE kind = ?1 AND status = ?2 AND derived_count >= ?3
              ORDER BY derived_count DESC, slug",
        )
        .map_err(sql("listing candidates"))?;
    let paths: Vec<String> = stmt
        .query_map(params![CANDIDATE, ACTIVE, threshold() as i64], |r| r.get(0))
        .map_err(sql("listing candidates"))?
        .flatten()
        .collect();
    drop(stmt);

    let mut out = Vec::new();
    for rel in paths {
        let p = vault.join(&rel);
        let Some(note) = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| parse_note(&t, stem_of(&rel), at))
        else {
            continue;
        };
        // Declined, and nothing has derived since — stay quiet. Compared by count rather than by
        // time: frontmatter holds whole seconds, so a decline and a derivation in the same second
        // were indistinguishable and the candidate went silent permanently.
        if note
            .declined_after
            .is_some_and(|c| note.derivations.len() <= c)
        {
            continue;
        }
        let last = note.derivations.iter().map(|d| d.ts).fold(0.0, f64::max);
        if at - last > CANDIDATE_TTL_DAYS * 86_400.0 {
            continue;
        }
        out.push(note);
    }
    Ok(out)
}

fn stem_of(rel: &str) -> &str {
    rel.rsplit_once('/')
        .map_or(rel, |(_, f)| f)
        .trim_end_matches(".md")
}

/// Where a promoted candidate lands, decided by the ledger rather than by the user's mood.
///
/// **This is the capability no per-repo tool has**, and it is why the ledger records projects
/// rather than sessions: a thing noticed in one project is that project's decision, and a thing
/// noticed in two is a principle that belongs to nobody's repository in particular.
/// **Every promotion is a decision now; only the scope varies** (D81). The kind used to move too
/// — one project produced a `decision`, more than one produced a `pattern` — which is the
/// conflation this router made most visible, because a router that picks a *type* based on *where*
/// something applies is a router with the wrong return value.
///
/// **Three rungs, not two** (D82):
///
/// ```text
/// derived in 1 project                   -> that project
/// derived in 3 projects sharing a topic  -> that topic
/// derived in 3 projects sharing nothing  -> @@
/// ```
///
/// The two-rung version called three Rust repositories evidence for a *universal* principle, which
/// is a stronger claim than the ledger made. D49's argument rests on that arithmetic being honest.
///
/// **The middle rung is dormant on a machine with one project, and that is not a defect.** It
/// needs several projects that share a topic, and it is built and tested against fixtures now so
/// that it exists on the day a second and third arm do. Saying so beats a future reader finding a
/// branch that has never executed and assuming it is broken.
pub fn destination(note: &Note) -> Routed {
    use crate::address::Scope;
    let mut seen: Vec<(&str, &Vec<String>)> = Vec::new();
    for d in &note.derivations {
        if !seen.iter().any(|(p, _)| *p == d.project) {
            seen.push((d.project.as_str(), &d.topics));
        }
    }
    seen.sort_by_key(|(p, _)| *p);

    if let [(only, _)] = seen.as_slice() {
        return Routed {
            scope: Scope::Project((*only).to_string()),
            alternatives: Vec::new(),
        };
    }
    let per_project: Vec<Vec<String>> = seen.iter().map(|(_, ts)| (*ts).clone()).collect();
    let shared = crate::memory::shared(&per_project);
    match shared.split_first() {
        // **The first shared topic wins and the rest are named.** Three repositories that are all
        // Rust *and* all Docker support either reading, and there is no evidence to choose
        // between them — so the choice is `TOPICS` order, which is stable and written down, and
        // the offer shows what else qualified. An arbitrary pick that stayed silent would be a
        // decision made by a sort.
        Some((first, rest)) => Routed {
            scope: Scope::Topic((*first).clone()),
            alternatives: rest.iter().map(|t| Scope::Topic(t.clone())).collect(),
        },
        None => Routed {
            scope: Scope::Global,
            alternatives: Vec::new(),
        },
    }
}

/// Where the ledger says a promotion lands, and what else it could defensibly have said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub scope: crate::address::Scope,
    /// Other scopes the same evidence supports, shown in the offer so the person approving can
    /// see that a choice was made rather than discovered. Overridden with `promote --scope`.
    pub alternatives: Vec<crate::address::Scope>,
}

/// The offer as JSON — the same evidence as [`render_offer`], because the gate must survive
/// the format. An earlier form of this lived in `main.rs` and emitted `derivations: len()` — a
/// *count*, which is the one thing D49 says an approval must never be reduced to. Beside the
/// prose renderer so the two cannot drift apart in different files (M26 is what that costs).
pub fn offer_json(candidate: &Note, routed: &Routed) -> serde_json::Value {
    serde_json::json!({
        "id": candidate.id.display(),
        "title": candidate.title,
        "derivations": candidate
            .derivations
            .iter()
            .map(|d| {
                serde_json::json!({ "ts": d.ts, "project": d.project, "note": d.note })
            })
            .collect::<Vec<_>>(),
        "scope": routed.scope.as_str(),
        "alternatives": routed
            .alternatives
            .iter()
            .map(crate::address::Scope::as_str)
            .collect::<Vec<_>>(),
    })
}

/// The refusal envelope every promote gate answers with under `--json`.
///
/// `written: false` is the load-bearing field — the machine-readable statement that the gate
/// held (D49) — and it is spelled in exactly one place so a third gate arm cannot spell it
/// differently. `confirm` names the flags that would open this particular gate; the offer rides
/// along when there is one to show (M26 is what two copies of this in `main.rs` would cost).
pub fn gate_json(confirm: &str, offer: Option<serde_json::Value>) -> serde_json::Value {
    let mut refusal = serde_json::json!({ "written": false, "confirm": confirm });
    if let Some(o) = offer {
        refusal["offer"] = o;
    }
    refusal
}

/// The `--direct` gate's prose twin, beside [`gate_json`] so one gate's two formats live in one
/// file — the drift M26 names is exactly what a prose copy of this stranded in `main.rs` costs.
pub fn render_direct_gate() -> &'static str {
    "direct promotion skips the derivation ledger entirely, so there is \
     nothing to read.\n  confirm with --direct --yes"
}

/// The offer `amb memory promote` prints when `--yes` was not given.
///
/// **Pure, and separate from the write, because this text *is* the human gate.** The threshold
/// produces this; a person produces the write. D49 revises D16 on exactly that separation, so the
/// three rules inside it are load-bearing and were previously unassertable:
///
/// - **One candidate, and its derivations spelled out rather than counted.** A batch with a single
///   confirmation is a rubber stamp, and a rubber stamp is D16's defect with extra steps.
/// - **The scope the router chose is named**, because that is what the person is being asked to
///   approve. Under the old two-rung version this line could only say "cross-project", which was
///   the conflation showing through the one surface a human actually reads (D81).
/// - **Alternatives are named, not silently resolved.** When several topics are shared by every
///   deriving project the evidence supports either reading, and the approver should see that a
///   choice was made rather than discovered (D82).
pub fn render_offer(candidate: &Note, routed: &Routed) -> String {
    // `quoted`, because the title is author-written text and this line is a human approval
    // gate: a newline in a title could append a forged derivation line — or a forged consent
    // sentence — to the one surface whose whole job is showing the person what they are
    // approving (M23's shape). Outside the injection ledger, so guarding it disturbs no open
    // measurement window; the injected renderers wait for the window on purpose.
    let mut out = format!(
        "{} — {}\n",
        candidate.id.display(),
        crate::delivery::quoted(&candidate.title)
    );
    for d in &candidate.derivations {
        out.push_str(&format!(
            "  {} · {} — {}\n",
            format_date(d.ts),
            d.project,
            d.note
        ));
    }
    out.push_str(&format!(
        "\n  {} derivation(s) in {}\n",
        candidate.derivations.len(),
        projects_of(candidate).join(", ")
    ));
    out.push_str(&format!("  would become a decision at {}\n", routed.scope));
    if !routed.alternatives.is_empty() {
        let alts: Vec<String> = routed.alternatives.iter().map(|s| s.to_string()).collect();
        out.push_str(&format!(
            "  the same evidence also supports {} — use --scope to pick one\n",
            alts.join(", ")
        ));
    }
    out.push_str(&format!(
        "\n  The count measures rediscovery, not truth. Read the derivations above.\n\
         \n  approve: amb memory promote {} --yes\n  decline: amb memory promote {} --decline\n",
        candidate.id.display(),
        candidate.id.display()
    ));
    out
}

/// Promote a candidate into a decision or a pattern.
///
/// **Called only after a person has said yes.** The threshold produces an *offer*; this produces
/// the write. Nothing in this module calls it on its own initiative, and that separation is the
/// whole of what reconciles D16 (D49).
///
/// The candidate is **archived, never deleted**: the derivation ledger is the evidence the
/// promotion rested on, and destroying it would leave a decision whose justification is gone.
/// `override_scope` is the answer to "the router chose wrong". The evidence decides by default;
/// a person who can see the derivations can overrule it, and D49 already puts a person in this
/// path. Without it the only recourse would be editing the promoted file afterwards, which loses
/// the record that a choice was made.
pub fn promote(
    conn: &Connection,
    me: &Identity,
    id: &NoteId,
    override_scope: Option<crate::address::Scope>,
    at: f64,
) -> Result<Note> {
    // Same critical section as `derive`: read, modify, write (D55).
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(sql("taking the vault lock for promote"))?;
    let _guard = LockGuard(conn);
    let vault = require_vault()?;
    let rel = vault_rel(CANDIDATE, "", &id.slug);
    let path = vault.join(&rel);
    let text = std::fs::read_to_string(&path).map_err(io(format!("reading {}", path.display())))?;
    let candidate =
        parse_note(&text, &id.slug, at).ok_or_else(|| Error::NoSuchNote(id.display()))?;

    let scope = override_scope.unwrap_or_else(|| destination(&candidate).scope);
    let promoted = Note {
        id: NoteId::decision(&scope, &candidate.id.slug),
        title: candidate.title.clone(),
        status: ACTIVE.into(),
        created: at,
        session: Some(me.id.clone()),
        agent: Some(me.name.clone()),
        files: candidate.files.clone(),
        cites: Vec::new(),
        supersedes: None,
        superseded_by: None,
        promoted_from: Some(candidate.id.display()),
        promoted_to: None,
        // A promoted decision inherits the candidate's scope, so marking a candidate private
        // keeps its decision out of the repository too.
        visibility: candidate.visibility.clone(),
        // Inherited, like scope. A promotion changes a note's *lifecycle*, not how
        // binding it is — resetting it here would silently demote every rule that
        // earned its way through the ledger.
        force: candidate.force.clone(),
        declined_at: None,
        declined_after: None,
        // The ledger carries over, so the promoted note states its own evidence.
        derivations: candidate.derivations.clone(),
        body: candidate.body.clone(),
    };

    let new_rel = vault_rel(&promoted.id.kind, &promoted.id.scope, &promoted.id.slug);
    let new_path = vault.join(&new_rel);
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let rendered = promoted.render();
    write_private(&new_path, &rendered)?;
    upsert(conn, &promoted, &new_rel, file_mtime(&new_path), at)?;

    let mut archived = candidate;
    archived.status = PROMOTED.into();
    archived.promoted_to = Some(promoted.id.display());
    let arch_text = archived.render();
    write_private(&path, &arch_text)?;
    upsert(conn, &archived, &rel, file_mtime(&path), at)?;
    Ok(promoted)
}

/// Record that the user declined a candidate.
///
/// **Declining has to be cheaper than assenting**, or approval becomes the path of least
/// resistance and the human gate stops being a gate. So it is one command, it is recorded, and the
/// candidate stays alive — it simply is not offered again until something new derives it.
pub fn decline(conn: &Connection, id: &NoteId, at: f64) -> Result<()> {
    // Same critical section as `derive`: read, modify, write (D55).
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(sql("taking the vault lock for decline"))?;
    let _guard = LockGuard(conn);
    let vault = require_vault()?;
    let rel = vault_rel(CANDIDATE, "", &id.slug);
    let path = vault.join(&rel);
    let text = std::fs::read_to_string(&path).map_err(io(format!("reading {}", path.display())))?;
    let mut note =
        parse_note(&text, &id.slug, at).ok_or_else(|| Error::NoSuchNote(id.display()))?;
    note.declined_at = Some(at);
    note.declined_after = Some(note.derivations.len());
    let rendered = note.render();
    write_private(&path, &rendered)?;
    upsert(conn, &note, &rel, file_mtime(&path), at)
}

/// Retire candidates that have gone [`CANDIDATE_TTL_DAYS`] without a new derivation.
///
/// **Unpromoted is not permanent.** A candidate nobody rediscovers for a month was noticed once
/// and is not a pattern; leaving it to accumulate would make the candidate list unreadable, which
/// is how the offer stops being read.
pub fn expire_candidates(conn: &Connection, vault: &Path, at: f64) -> Result<usize> {
    let mut expired = 0;
    let mut stmt = conn
        .prepare("SELECT vault_path FROM notes WHERE kind = ?1 AND status = ?2")
        .map_err(sql("listing candidates"))?;
    let paths: Vec<String> = stmt
        .query_map(params![CANDIDATE, ACTIVE], |r| r.get(0))
        .map_err(sql("listing candidates"))?
        .flatten()
        .collect();
    drop(stmt);
    for rel in paths {
        let p = vault.join(&rel);
        let Some(mut note) = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| parse_note(&t, stem_of(&rel), at))
        else {
            continue;
        };
        let last = note
            .derivations
            .iter()
            .map(|d| d.ts)
            .fold(note.created, f64::max);
        if at - last <= CANDIDATE_TTL_DAYS * 86_400.0 {
            continue;
        }
        note.status = EXPIRED.into();
        let rendered = note.render();
        write_private(&p, &rendered)?;
        upsert(conn, &note, &rel, file_mtime(&p), at)?;
        expired += 1;
    }
    Ok(expired)
}

/// `amb memory derive`'s human output.
///
/// **The non-independent branch is the one worth guarding.** A silent non-increment looks like a
/// bug, so the reason is said out loud — and that sentence is the counting rule the whole
/// promotion gate rests on. It lived in `run_memory` where nothing could assert it.
pub fn render_derived(d: &Derived) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} {}\n",
        if d.created { "opened" } else { "updated" },
        d.id.display()
    ));
    if d.independent {
        out.push_str(&format!(
            "  {} independent derivation(s) across {}\n",
            d.count,
            d.projects.join(", ")
        ));
    } else {
        out.push_str(
            "  recorded, but NOT counted: this session was already shown something about these \
             paths, so it is a citation rather than a derivation\n",
        );
    }
    // Word-for-word what `write.rs` prints, because two spellings of one guarantee is two
    // guarantees to keep in step.
    if d.redacted > 0 {
        out.push_str(&format!(
            "  {} value(s) redacted before writing\n",
            d.redacted
        ));
    }
    if d.count >= PROMOTION_THRESHOLD {
        out.push_str(&format!(
            "  ready to offer — `amb memory promote {}`\n",
            d.id.display()
        ));
    }
    out
}

/// `amb memory candidates`' human output.
///
/// Empty is its own sentence rather than no output at all: a command that prints nothing is
/// indistinguishable from one that failed to run, which is this project's whole failure mode.
pub fn render_candidates(notes: &[Note]) -> String {
    if notes.is_empty() {
        return "no candidates\n".into();
    }
    let mut out = String::new();
    for n in notes {
        out.push_str(&format!(
            "{} · {}/{} in {}{} — {}\n",
            n.id.display(),
            n.derivations.len(),
            PROMOTION_THRESHOLD,
            projects_of(n).join(", "),
            if n.status == ACTIVE {
                String::new()
            } else {
                format!(" [{}]", n.status)
            },
            n.title
        ));
    }
    out
}

#[cfg(test)]
mod tests {

    fn derived(created: bool, independent: bool, count: usize) -> Derived {
        Derived {
            id: NoteId::candidate("lock-order"),
            created,
            independent,
            count,
            projects: vec!["nest".into(), "mobile".into()],
            path: PathBuf::from("/v/candidates/lock-order.md"),
            redacted: 0,
        }
    }

    /// **A redaction the author cannot see is one they cannot correct**, and `derive` hid every
    /// one: `redact(...).text` at three call sites, `.removed` discarded at all three.
    ///
    /// A truth table rather than a needle list, because M27 found ten of forty survivors in one
    /// renderer sitting on the `x > 0` -> `x >= 0` edit and a presence-only test cannot see that
    /// relaxation. The `0` row is the absence; the other two prove its premise, since if the line
    /// stopped rendering at all the `0` row would still pass and this would guard nothing (M23).
    #[test]
    fn a_silent_redaction_is_impossible_because_the_count_is_rendered() {
        for (n, expected) in [(0usize, false), (1, true), (7, true)] {
            let mut d = derived(true, true, 3);
            d.redacted = n;
            let out = render_derived(&d);
            assert_eq!(
                out.contains("value(s) redacted before writing"),
                expected,
                "redacted={n} rendered:\n{out}"
            );
            if expected {
                assert!(
                    out.contains(&format!("{n} value(s)")),
                    "wrong count:\n{out}"
                );
            }
        }
    }

    /// The notice is word-for-word the one the `observe` path prints.
    ///
    /// Two spellings of one guarantee is two guarantees to keep in step. This asserts the same
    /// needle against both renderers, so a change to either wording goes red rather than quietly
    /// producing two sentences that mean the same thing differently.
    #[test]
    fn the_redaction_notice_matches_the_one_the_observe_path_prints() {
        let mut d = derived(true, true, 3);
        d.redacted = 2;
        let from_derive = render_derived(&d);
        let from_observe = crate::memory::render_written(
            &crate::memory::Written {
                id: NoteId::candidate("x"),
                path: PathBuf::from("/v/x.md"),
                redacted: 2,
                inert_paths: Vec::new(),
                cited: Vec::new(),
                superseded: None,
            },
            None,
            &[],
        );
        let needle = "2 value(s) redacted before writing";
        assert!(from_derive.contains(needle), "derive:\n{from_derive}");
        assert!(from_observe.contains(needle), "observe:\n{from_observe}");
    }

    /// **A non-independent derivation must say it did not count.**
    ///
    /// The count not moving is the promotion gate's whole rule, and a silent non-increment is
    /// indistinguishable from a bug. Delete the `else` and this reddens.
    #[test]
    fn a_derivation_that_did_not_count_says_so_and_names_the_reason() {
        let counted = render_derived(&derived(false, true, 2));
        crate::assert_rendered_shape("render_derived", &counted);
        assert!(counted.contains("2 independent derivation(s)"), "{counted}");
        assert!(!counted.contains("NOT counted"), "{counted}");

        let not = render_derived(&derived(false, false, 2));
        assert!(not.contains("NOT counted"), "{not}");
        assert!(
            not.contains("citation rather than a derivation"),
            "the reason, not just the fact: {not}"
        );
        assert!(
            !not.contains("independent derivation(s) across"),
            "and it must not also claim the count moved: {not}"
        );
    }

    /// Opening and updating are different words, and the offer appears exactly at the threshold.
    #[test]
    fn the_offer_appears_at_the_threshold_and_not_one_below_it() {
        assert!(render_derived(&derived(true, true, 1)).starts_with("opened "));
        assert!(render_derived(&derived(false, true, 1)).starts_with("updated "));

        let below = render_derived(&derived(false, true, PROMOTION_THRESHOLD - 1));
        assert!(!below.contains("ready to offer"), "{below}");
        let at = render_derived(&derived(false, true, PROMOTION_THRESHOLD));
        assert!(at.contains("ready to offer"), "{at}");
        assert!(
            at.contains("amb memory promote candidate/lock-order"),
            "and it names the command that acts on it: {at}"
        );
    }

    /// **An empty list is a sentence, not an absence.** No output at all is what a command that
    /// failed to run looks like, and this project's failures are silences.
    #[test]
    fn no_candidates_is_said_rather_than_printed_as_nothing() {
        assert_eq!(render_candidates(&[]), "no candidates\n");
    }

    #[test]
    fn a_retired_candidate_carries_its_status_and_an_active_one_carries_none() {
        let mut n = candidate_derived_in(&[("nest", &[]), ("mobile", &[])]);
        let active = render_candidates(std::slice::from_ref(&n));
        crate::assert_rendered_shape("render_candidates", &active);
        // Sorted, not in derivation order — so the same candidate renders identically whatever
        // order the ledger happened to accumulate. Asserted because a caller reading this line
        // as a chronology would be wrong about it.
        assert!(active.contains("2/3 in mobile, nest — t"), "{active}");
        assert!(
            !active.contains('['),
            "an active candidate is unadorned: {active}"
        );

        n.status = "archived".into();
        assert!(render_candidates(&[n]).contains("[archived]"));
    }
    use super::*;
    use crate::address::Scope;

    /// The JSON offer lists the derivations, never merely counts them.
    ///
    /// D49: "one candidate per offer, derivations shown rather than counted" — and the first
    /// JSON form of this gate, built inline in `main.rs`, emitted `derivations: len()`. The
    /// format changed and the rule did not; this pins the rule to the format.
    #[test]
    fn the_json_offer_lists_derivations_rather_than_counting() {
        let c = candidate_derived_in(&[("nest", &["rust"]), ("amb", &["rust"])]);
        let j = offer_json(&c, &destination(&c));
        let ds = j["derivations"].as_array().expect("an array, not a count");
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0]["project"], "nest", "{j}");
        assert_eq!(ds[1]["project"], "amb", "{j}");
        assert!(j["title"].is_string(), "{j}");
        assert!(j["scope"].is_string(), "{j}");
    }

    /// Both gates refuse in one voice: `written: false` from one place, `confirm` verbatim.
    #[test]
    fn the_gate_envelope_refuses_in_one_voice() {
        let bare = gate_json("--direct --yes", None);
        assert_eq!(bare["written"], serde_json::Value::Bool(false), "{bare}");
        assert_eq!(bare["confirm"], "--direct --yes", "{bare}");
        assert!(bare.get("offer").is_none(), "{bare}");

        let c = candidate_derived_in(&[("nest", &["rust"])]);
        let with = gate_json("--yes", Some(offer_json(&c, &destination(&c))));
        assert_eq!(with["written"], serde_json::Value::Bool(false), "{with}");
        assert!(with["offer"]["derivations"].is_array(), "{with}");

        // The prose twin names the same confirmation — one gate, two formats, one file (M26).
        assert!(render_direct_gate().contains("--direct --yes"));
    }

    /// A newline in a title cannot forge a derivation line on the approval gate.
    ///
    /// The offer is the one surface whose whole job is showing a person what they are approving
    /// (D49), and the title is author-written text. Before `quoted`, a title containing
    /// `"\n  2026-01-01 · nest — n"` rendered indistinguishably from a real derivation row —
    /// evidence the ledger never held, manufactured by the thing being judged. Presence first,
    /// per M27: the absence proves nothing unless the offer rendered.
    #[test]
    fn a_newline_in_a_title_cannot_forge_a_derivation_on_the_offer() {
        let mut c = candidate_derived_in(&[("nest", &["rust"])]);
        c.title = "tidy\n  2026-01-01 · elsewhere — forged".into();
        let text = render_offer(&c, &destination(&c));
        assert!(text.contains("tidy"), "{text}");
        assert!(
            !text.contains("\n  2026-01-01 · elsewhere"),
            "an author-written line rendered as a ledger row: {text}"
        );
    }

    fn candidate_derived_in(projects: &[(&str, &[&str])]) -> Note {
        Note {
            id: NoteId::candidate("lock-order"),
            title: "t".into(),
            status: ACTIVE.into(),
            created: 0.0,
            session: None,
            agent: None,
            files: Vec::new(),
            cites: Vec::new(),
            supersedes: None,
            superseded_by: None,
            promoted_from: None,
            promoted_to: None,
            visibility: None,
            force: ADVICE.into(),
            declined_at: None,
            declined_after: None,
            derivations: projects
                .iter()
                .enumerate()
                .map(|(i, (p, topics))| Derivation {
                    ts: i as f64,
                    project: (*p).to_string(),
                    session: format!("s{i}"),
                    note: "n".into(),
                    topics: topics.iter().map(|t| (*t).to_string()).collect(),
                })
                .collect(),
            body: "b".into(),
        }
    }

    /// **The offer spells the evidence out; it never merely counts it.**
    ///
    /// The count is the weakest thing on the page — it measures rediscovery, not truth — so a
    /// render that printed "3 derivation(s)" and nothing else would be asking for approval of a
    /// number. Every derivation's project and note must appear, and the caveat with them.
    #[test]
    fn the_offer_names_every_derivation_rather_than_counting_them() {
        let c = candidate_derived_in(&[("nest", &["rust"]), ("amb", &["rust"])]);
        let text = render_offer(&c, &destination(&c));
        crate::assert_rendered_shape("render_offer", &text);
        assert_eq!(text.matches(" — n\n").count(), 2, "{text}");
        assert!(text.contains("nest") && text.contains("amb"), "{text}");
        assert!(text.contains("2 derivation(s)"), "{text}");
        assert!(
            text.contains("measures rediscovery, not truth"),
            "the count must arrive with its caveat:\n{text}"
        );
    }

    /// Both halves of the gate, in the text that *is* the gate.
    ///
    /// An offer that named only the approval would be a page with one button on it. Naming the
    /// decline is what makes the choice a choice, and `--decline` is also the only way to stop a
    /// candidate being offered again.
    #[test]
    fn the_offer_names_both_ways_out_and_writes_nothing_by_existing() {
        let c = candidate_derived_in(&[("nest", &["rust"])]);
        let text = render_offer(&c, &destination(&c));
        assert!(text.contains("--yes"), "{text}");
        assert!(text.contains("--decline"), "{text}");
    }

    /// **The scope is named, and D81 is why.**
    ///
    /// The router's answer is what the person is being asked to approve. Under the two-rung
    /// version this line could only say "cross-project" — the conflation showing through the one
    /// surface a human actually reads. It reddens if the line stops naming the routed scope.
    #[test]
    fn the_offer_names_the_scope_the_router_actually_chose() {
        let topic = candidate_derived_in(&[
            ("nest", &["rust"]),
            ("amb", &["rust"]),
            ("devt", &["rust", "docker"]),
        ]);
        let text = render_offer(&topic, &destination(&topic));
        assert!(
            text.contains(&format!(
                "would become a decision at {}",
                Scope::Topic("rust".into())
            )),
            "{text}"
        );
    }

    /// **A choice made, not discovered (D82).**
    ///
    /// When the deriving projects share more than one topic the evidence supports either reading,
    /// and silently taking the first is the router deciding something it has no grounds to
    /// decide. Delete the `alternatives` block and the second half reddens while the first stays
    /// green — which is the point: a single-topic offer must *not* carry the line.
    #[test]
    fn a_tie_between_topics_is_named_in_the_offer_and_a_single_topic_is_not() {
        let one =
            candidate_derived_in(&[("nest", &["rust"]), ("amb", &["rust"]), ("devt", &["rust"])]);
        let quiet = render_offer(&one, &destination(&one));
        assert!(!quiet.contains("also supports"), "{quiet}");

        // `docker`, not an invented name: `shared` intersects against `TOPICS`, so a topic that
        // is not in that table is filtered out and the tie never forms. The first draft of this
        // test used `cli` and the guard below is what caught it.
        let tied = candidate_derived_in(&[
            ("nest", &["rust", "docker"]),
            ("amb", &["rust", "docker"]),
            ("devt", &["rust", "docker"]),
        ]);
        let routed = destination(&tied);
        assert!(
            !routed.alternatives.is_empty(),
            "fixture no longer produces a tie, so the assertion below would pass for free"
        );
        let text = render_offer(&tied, &routed);
        assert!(text.contains("also supports"), "{text}");
        assert!(text.contains("--scope"), "{text}");
    }

    /// The three rungs, which is the whole of D82's argument as a table.
    #[test]
    fn the_router_routes_on_what_the_deriving_projects_share() {
        // One project: its own decision. Nothing is being generalised.
        assert_eq!(
            destination(&candidate_derived_in(&[("nest", &["rust"])])).scope,
            Scope::Project("nest".into())
        );
        // Three Rust repositories: evidence for a Rust principle, not a universal one. This is
        // the rung that did not exist, and the reason the two-rung version over-claimed.
        assert_eq!(
            destination(&candidate_derived_in(&[
                ("nest", &["rust"]),
                ("amb", &["rust"]),
                ("devt", &["rust", "docker"]),
            ]))
            .scope,
            Scope::Topic("rust".into())
        );
        // Three unrelated repositories: genuinely universal, and only now does `@@` mean that.
        assert_eq!(
            destination(&candidate_derived_in(&[
                ("nest", &["rust"]),
                ("api", &["python"]),
                ("web", &["typescript"]),
            ]))
            .scope,
            Scope::Global
        );
    }

    /// A derivation that predates topics is *unknown*, not *none*, and the router has to fail
    /// outward.
    ///
    /// Routing it inward would file a principle under a topic nobody ever observed that project to
    /// be in — a claim the ledger did not make, which is the exact defect the middle rung exists
    /// to remove. Failing outward over-generalises, which is what the router already did.
    #[test]
    fn a_derivation_with_no_recorded_topics_can_only_route_outward() {
        assert_eq!(
            destination(&candidate_derived_in(&[
                ("nest", &["rust"]),
                ("amb", &["rust"]),
                ("old", &[]),
            ]))
            .scope,
            Scope::Global
        );
    }

    /// **The same project deriving three times is still one project.** The threshold counts
    /// distinct projects, and a router that counted derivations would promote a thing one
    /// repository noticed three times to a cross-project principle.
    #[test]
    fn repeated_derivations_in_one_project_do_not_reach_across_projects() {
        assert_eq!(
            destination(&candidate_derived_in(&[
                ("nest", &["rust"]),
                ("nest", &["rust"]),
                ("nest", &["rust"]),
            ]))
            .scope,
            Scope::Project("nest".into())
        );
    }

    /// When several topics qualify the choice is deterministic and the rest are *named*, so the
    /// person approving sees that a choice was made rather than discovered.
    #[test]
    fn an_ambiguous_topic_route_names_what_it_did_not_pick() {
        let routed = destination(&candidate_derived_in(&[
            ("nest", &["rust", "docker"]),
            ("amb", &["docker", "rust"]),
        ]));
        assert_eq!(routed.scope, Scope::Topic("rust".into()));
        assert_eq!(routed.alternatives, vec![Scope::Topic("docker".into())]);
    }

    /// An unambiguous route offers no alternatives, or the offer would nag on every promotion.
    #[test]
    fn an_unambiguous_route_names_no_alternatives() {
        for c in [
            candidate_derived_in(&[("nest", &["rust"])]),
            candidate_derived_in(&[("nest", &["rust"]), ("amb", &["rust"])]),
            candidate_derived_in(&[("nest", &["rust"]), ("api", &["python"])]),
        ] {
            assert!(
                destination(&c).alternatives.is_empty(),
                "{:?} should not offer alternatives",
                destination(&c).scope
            );
        }
    }

    /// A vault with a board, and one candidate staged into it whose derivations land at `ages`
    /// days before `NOW`. Returns the vault dir and the connection.
    ///
    /// **These tests needed a shell and the module had none.** Every existing test here is about
    /// what promotion *prints* — the offer, the router, the candidate list — and all of them are
    /// pure. Fourteen of sixteen surviving mutants were in the two functions that read the vault
    /// and write it back (M25).
    fn vault_with(candidates: &[(&str, &[f64])]) -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        for (slug, ages) in candidates {
            let mut n = candidate_derived_in(&[]);
            n.id = NoteId::candidate(slug);
            n.created = NOW - 400.0 * 86_400.0;
            n.derivations = ages
                .iter()
                .enumerate()
                .map(|(i, d)| Derivation {
                    ts: NOW - d * 86_400.0,
                    project: format!("p{i}"),
                    session: format!("s{i}"),
                    note: "derived".into(),
                    topics: Vec::new(),
                })
                .collect();
            let rel = crate::memory::vault_rel(CANDIDATE, &n.id.scope, slug);
            let path = dir.path().join(&rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, n.render()).expect("write");
        }
        crate::memory::reindex(&conn, dir.path(), NOW).expect("index");
        (dir, conn)
    }

    /// A fixed instant, so a test never races the clock it is asserting about.
    const NOW: f64 = 1_800_000_000.0;

    fn status_of(conn: &Connection, slug: &str) -> String {
        conn.query_row(
            "SELECT status FROM notes WHERE kind = ?1 AND slug = ?2",
            params![CANDIDATE, slug],
            |r| r.get(0),
        )
        .expect("the candidate is indexed")
    }

    /// **The TTL expires what is stale and, more importantly, does not expire what is not.**
    ///
    /// Nine of sixteen survivors lived here (M25): the whole function could return `Ok(0)`, the
    /// comparison could invert, `at - last` could become `at + last` or `at / last`, the TTL could
    /// be `DAYS + 86_400` instead of `DAYS * 86_400`, and the counter could count down. None of it
    /// reddened anything, because nothing had ever called this function.
    ///
    /// The fresh candidate is **ten days** old on purpose. `DAYS + 86_400` is a threshold of about
    /// one day, so a one-hour-old fixture would have satisfied the mutant too; ten days sits
    /// between the mutant's threshold and the real one and separates them.
    #[test]
    fn the_candidate_ttl_retires_the_stale_and_leaves_everything_else_alone() {
        let (dir, c) = vault_with(&[
            ("long-gone", &[90.0, 60.0]),
            ("also-gone", &[45.0]),
            ("still-warm", &[10.0]),
        ]);

        let n = expire_candidates(&c, dir.path(), NOW).expect("expire runs");
        assert_eq!(n, 2, "the count is what was expired, not a constant");
        assert_eq!(status_of(&c, "long-gone"), EXPIRED);
        assert_eq!(status_of(&c, "also-gone"), EXPIRED);
        assert_eq!(
            status_of(&c, "still-warm"),
            ACTIVE,
            "a candidate derived inside the TTL must survive — this is the assertion the \
             arithmetic mutants fail"
        );

        // Idempotent: nothing is left to expire, and the count says so rather than recounting
        // what it already retired.
        assert_eq!(
            expire_candidates(&c, dir.path(), NOW).expect("expire runs"),
            0
        );
    }

    /// The boundary, asserted because the two halves of this rule live in two functions and must
    /// agree about it. `ready_candidates` skips when `at - last > TTL` and `expire_candidates`
    /// retires on the same condition, so *exactly* at the TTL a candidate is alive in both.
    ///
    /// **Three derivations, and both functions called — the first version of this test had one
    /// derivation and called only `expire_candidates`.** It passed, and `>` → `>=` in
    /// `ready_candidates` survived it: the fixture could not reach that function's TTL check at
    /// all, because the `derived_count >= threshold()` filter in its SQL drops a one-derivation
    /// candidate before the comparison runs. A test whose name says "both halves" while its
    /// fixture reaches one is the defect this project has already catalogued, written fresh.
    #[test]
    fn a_candidate_exactly_at_its_ttl_is_alive_in_both_halves_of_the_rule() {
        let t = CANDIDATE_TTL_DAYS;
        let (dir, c) = vault_with(&[("on-the-line", &[t, t, t])]);

        // Exact by construction: `NOW` and `TTL * 86_400` are both integers well inside f64's
        // exact range, so `at - last` is equal to the threshold rather than near it.
        assert_eq!(
            expire_candidates(&c, dir.path(), NOW).expect("expire runs"),
            0,
            "exactly at the TTL is not yet past it"
        );
        assert_eq!(status_of(&c, "on-the-line"), ACTIVE);

        let ready = ready_candidates(&c, dir.path(), NOW).expect("ready runs");
        assert_eq!(
            ready.iter().map(|n| n.id.slug.as_str()).collect::<Vec<_>>(),
            vec!["on-the-line"],
            "and the other half of the rule agrees: still offered at the boundary"
        );
    }

    /// **The offer's own TTL filter, which is the same rule written a second time.**
    ///
    /// Four survivors here (M25). A stale candidate that `expire_candidates` has not yet reached
    /// must not be offered, and a live one must be — and the second half is what the `*`→`+`
    /// mutant breaks, since it shrinks a thirty-day window to about one day.
    #[test]
    fn a_stale_candidate_is_not_offered_and_a_live_one_is() {
        let (dir, c) = vault_with(&[
            ("stale", &[90.0, 91.0, 92.0]),
            ("live", &[10.0, 11.0, 12.0]),
        ]);

        let ready = ready_candidates(&c, dir.path(), NOW).expect("ready runs");
        let slugs: Vec<&str> = ready.iter().map(|n| n.id.slug.as_str()).collect();
        assert_eq!(
            slugs,
            vec!["live"],
            "a candidate nobody has rediscovered for ninety days is not a pattern"
        );
    }

    /// **A dedup is an omission, so only an absence guards it — and only a presence guards the
    /// other side.** Flipping `k.id == n.id` to `!=` turns "skip what is already here" into
    /// "return the first candidate and nothing else", which no positive assertion about the first
    /// one can see (M25).
    #[test]
    fn two_candidates_on_one_path_both_come_back_and_neither_comes_back_twice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        for slug in ["first", "second"] {
            let mut n = candidate_derived_in(&[]);
            n.id = NoteId::candidate(slug);
            n.created = NOW;
            // Two paths on each, so a note reachable twice is also a note that must appear once.
            n.files = vec!["src/lock.rs".into(), "src/order.rs".into()];
            let rel = crate::memory::vault_rel(CANDIDATE, &n.id.scope, slug);
            let path = dir.path().join(&rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, n.render()).expect("write");
        }
        crate::memory::reindex(&conn, dir.path(), NOW).expect("index");

        let hits = candidates_concerning(
            &conn,
            &["src/lock.rs".to_string(), "src/order.rs".to_string()],
        )
        .expect("query runs");
        let mut slugs: Vec<&str> = hits.iter().map(|n| n.id.slug.as_str()).collect();
        slugs.sort_unstable();
        assert_eq!(
            slugs,
            vec!["first", "second"],
            "both candidates, each exactly once"
        );
    }

    /// The filename-to-slug step, which decides the identity a note is parsed under when its
    /// frontmatter has no `id:`. Both constant-return mutants survived (M25).
    #[test]
    fn a_vault_path_reduces_to_the_slug_the_note_is_identified_by() {
        assert_eq!(stem_of("candidates/lock-order.md"), "lock-order");
        assert_eq!(
            stem_of("lock-order.md"),
            "lock-order",
            "no directory at all"
        );
        assert_eq!(
            stem_of("projects/nest/deep/a-note.md"),
            "a-note",
            "only the last segment"
        );
    }
}

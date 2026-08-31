//! Reading the index: the two retrieval lanes, search and resolution.
//!
//! `recent_for_project` is the recency lane and `concerning` the path lane;
//! D42 counts them separately because they are the two retrieval modes being
//! compared.

use super::*;

// ── Retrieval ───────────────────────────────────────────────────────────────

/// Every scope a session standing in `project` may be shown notes from.
///
/// **One list, one `IN`, no branch per scope.** This is D17's claim about the bus — four
/// addressing modes are one query — applied to the axis D81 created. The statement it replaces
/// read `(n.project = ?1 OR n.project = '')`, where `''` silently meant *a pattern*: the scope
/// was not a value being compared, it was an absence being pattern-matched, and a third scope had
/// nowhere to go.
///
/// **D82 appends the project's topics here and nowhere else**, which is the point of returning a
/// list rather than writing two disjuncts: the topic rung cost one line at the one place that
/// decides what a session can see.
///
/// Ordered nearest-first — the project, then its topics, then everywhere — which is the order
/// [`Nearness`] ranks them in. The SQL does not care; a reader does.
pub fn visible_scopes(project: &str, topics: &[String]) -> Vec<String> {
    let mut out = vec![project.to_string()];
    out.extend(
        topics
            .iter()
            .map(|t| format!("{}{t}", crate::address::TOPIC_SIGIL)),
    );
    out.push(crate::address::GLOBAL.to_string());
    out
}

/// `?n, ?n+1, …` — numbered so a dynamic `IN` list can sit beside fixed parameters without
/// either having to know how many the other used.
fn holes(start: usize, n: usize) -> String {
    (start..start + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The most recent active notes visible from one project.
pub fn recent_for_project(
    conn: &Connection,
    project: &str,
    topics: &[String],
    limit: usize,
) -> Result<Vec<IndexedNote>> {
    // Observations and decisions at any scope this project can see. **`candidate` is absent by
    // construction** — the `IN` list is `INJECTABLE`, and a candidate that could be shown could
    // argue for its own promotion (D49).
    let kinds = injectable_sql();
    // Force ranks *within* the promoted tier and above recency: this is the statement that decides
    // which notes exist as far as the injection is concerned, because of the `LIMIT`.
    let force_order = force_order_sql("n.force");
    let scopes = visible_scopes(project, topics);
    let in_scope = holes(3, scopes.len());
    let sql_text = format!(
        "{SELECT_NOTE}
          WHERE n.kind IN ({kinds}) AND n.status = ?1 AND n.scope IN ({in_scope})
          ORDER BY (n.kind = '{OBSERVATION}'), {force_order}, n.created DESC LIMIT ?2"
    );
    let limit = limit as i64;
    let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&ACTIVE, &limit];
    binds.extend(scopes.iter().map(|s| s as &dyn rusqlite::ToSql));
    let mut stmt = conn.prepare(&sql_text).map_err(sql("preparing a recall"))?;
    let rows = stmt
        .query_map(binds.as_slice(), row_to_note)
        .map_err(sql("running a recall"))?;
    Ok(rows.flatten().collect())
}

/// How many active notes exist — in one project, or everywhere.
///
/// The per-project count is what makes the cap honest: the caller selects with a `LIMIT`, so
/// this is the only thing that knows how many notes the injection did not show.
pub fn count_active(conn: &Connection, project: Option<&str>, topics: &[String]) -> Result<usize> {
    // **Counted over exactly what could be shown**, or the header lies. When injection grew to
    // include decisions and patterns, this still counted observations in one project and the
    // block rendered "2 of 1 note(s)" — and D43 makes this number the one that says how many were
    // hidden, so a wrong count is a wrong cap admission, not a cosmetic slip.
    // **Built from `injectable_sql` rather than rebuilt inline**, which it was: this function held
    // its own copy of the same four lines, so D51's "one source, so they cannot disagree again"
    // was true of the path lookup and the recency lane and not of the count that admits what they
    // hid. Exactly the divergence D51 recorded, in a third place, found while separating the axes.
    let kinds = injectable_sql();
    let scopes = project
        .map(|p| visible_scopes(p, topics))
        .unwrap_or_default();
    // `None` means everywhere, which is a *missing* filter rather than a wider one — an empty
    // `IN ()` matches nothing, so the clause has to disappear rather than be given no values.
    let where_scope = if scopes.is_empty() {
        String::new()
    } else {
        format!(" AND scope IN ({})", holes(2, scopes.len()))
    };
    let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&ACTIVE];
    binds.extend(scopes.iter().map(|s| s as &dyn rusqlite::ToSql));
    let n: i64 = conn
        .query_row(
            &format!(
                "SELECT count(*) FROM notes WHERE kind IN ({kinds}) AND status = ?1{where_scope}"
            ),
            binds.as_slice(),
            |r| r.get(0),
        )
        .map_err(sql("counting notes"))?;
    Ok(n as usize)
}

/// How many rows a path lookup will pull back before it stops counting exactly.
///
/// Eight times the cap. Below this the "and N more" line is **exact**; above it, the count falls
/// back to a `count(*)` over the same loose predicate, which can over-count only in the case the
/// Rust filter exists for (`src/auth` against `src/authz.rs`). Bounded work on the hottest hook
/// in the system matters more than an exact number in a case where "and 992 more" is already the
/// least of the reader's problems.
pub(crate) const PATH_LOOKUP_WINDOW: usize = MAX_INJECTED * 8;

/// How many notes the index holds for one project, whatever their status.
///
/// Compared against what is on disk to decide whether the index is behind. `count_active` cannot
/// answer that: a superseded note is a file on disk and not an active row, so the two legitimately
/// differ and the difference would read as drift.
pub fn count_indexed(conn: &Connection, project: &str) -> Result<usize> {
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM notes WHERE kind = ?1 AND scope = ?2",
            params![OBSERVATION, project],
            |r| r.get(0),
        )
        .map_err(sql("counting indexed notes"))?;
    Ok(n as usize)
}

/// The `IN (...)` fragment naming every kind that may be shown.
///
/// **Built from [`INJECTABLE`] rather than written out**, because it was written out — once here
/// and once as a literal in the path lookup — and a mutation test proved the two had already
/// diverged in effect. Adding `candidate` to `INJECTABLE` did *not* leak candidates into an
/// injection, which sounds reassuring and is the opposite: the exclusion was being done by a
/// project filter that happens not to match the empty project, so the guard named in the code was
/// not the guard doing the work. One source, so they cannot disagree again (D51).
pub(crate) fn injectable_sql() -> String {
    kinds_sql(INJECTABLE)
}

/// The same fragment for any kind list, so the second axis cannot grow its own quoting rules.
pub(crate) fn kinds_sql(kinds: &[&str]) -> String {
    kinds
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// How many notes the index has not seen, or `None` when it is current — D45's guard.
///
/// **A declined rebuild must be *stated*, never rendered as an empty vault.** Above
/// [`AUTO_INDEX_LIMIT`] `sync_dir` declines to rebuild and sets `skipped`; a vault the index has
/// never seen then renders as "no prior observations", which is a confident lie. The comparison
/// against `indexed` is what stops the notice firing when the index happens to be current anyway —
/// a vault kept up to date by hand does not nag.
///
/// **Extracted because it was a decision living in `src/main.rs`** with no unit test, on the
/// `SessionStart` path. `IndexStats::skipped` is the field D45 was written about: it had no reader
/// at all, and a 501-note vault reported itself empty.
pub fn index_is_behind(stats: &IndexStats, indexed: usize) -> Option<usize> {
    (stats.skipped && stats.scanned != indexed).then_some(stats.scanned)
}

/// The title and body of a note recording a failed tool call.
///
/// **The cap is the whole decision.** An error payload can be an entire compiler run, and a note
/// is not a log — without a bound one failure fills the vault and then the injection budget D24
/// exists to protect. Counted in `chars`, not bytes, so a multi-byte payload cannot be cut through
/// a character.
pub fn failure_note(tool: &str, detail: &str) -> (String, String) {
    (
        format!("{tool} failed"),
        detail.chars().take(FAILURE_DETAIL_MAX).collect(),
    )
}

/// The longest error detail kept from a failed tool call.
pub const FAILURE_DETAIL_MAX: usize = 600;

/// Notes concerning a path, in any project on this machine, and how many there are in total.
///
/// **The SQL over-selects and [`claims::overlaps`] decides.** That predicate is already the
/// segment-aware rule claims use — `src/a` must not cover `src/abc.rs` — and it is already
/// tested. Re-expressing it in SQL would be a second copy of a rule with a known sharp edge, so
/// the query narrows with `LIKE` for the index's sake and the pure function has the final say.
///
/// **Bounded — and the bound did not make it faster, which is worth saying out loud.** The
/// unbounded version fetched every matching note, each with a `group_concat` subquery, to display
/// eight. Windowing that fetch was expected to pay and **measured as no change at all**
/// (`MEASUREMENTS.md` M9): at 1,000 notes about one path the cost is the counting scan, not the
/// rows. The window stays because unbounded work on the hook that fires before every file tool
/// call is a hazard whatever today's constant factor is — but it is a design property, not a
/// speedup, and quoting it as one would be the error M5 and M7 already record.
///
/// The total travels back separately for the same reason `render_session` takes one (D43): a
/// renderer handed a windowed slice cannot know what the window hid.
pub fn concerning(conn: &Connection, path: &str) -> Result<(Vec<IndexedNote>, usize)> {
    let kinds = injectable_sql();
    let matches = &format!(
        "n.kind IN ({kinds}) AND n.status = ?1
            AND EXISTS (SELECT 1 FROM note_paths p
                         WHERE p.kind = n.kind AND p.scope = n.scope AND p.slug = n.slug
                           AND (p.path_glob = ?2 OR p.path_glob LIKE ?2 || '%'
                                OR ?2 LIKE p.path_glob || '%'))"
    );
    let sql_text =
        format!("{SELECT_NOTE} WHERE {matches} ORDER BY n.created DESC LIMIT {PATH_LOOKUP_WINDOW}");
    let mut stmt = conn
        .prepare(&sql_text)
        .map_err(sql("preparing a path lookup"))?;
    let window: Vec<IndexedNote> = stmt
        .query_map(params![ACTIVE, path], row_to_note)
        .map_err(sql("running a path lookup"))?
        .flatten()
        .collect();
    let exhausted = window.len() == PATH_LOOKUP_WINDOW;
    let found: Vec<IndexedNote> = window
        .into_iter()
        .filter(|n| n.paths.iter().any(|g| claims::overlaps(g, path)))
        .collect();

    // Only ask for a count when the window could not answer it. In every ordinary vault this
    // second query never runs.
    //
    // **It counts the coarse predicate, not the `overlaps` filter above, so it can overstate.** A
    // note whose glob shares a string prefix with `path` but not a segment boundary is counted
    // here and excluded from `found`. Making it exact means applying `overlaps` to every match,
    // which is the unbounded work the window exists to prevent — so the imprecision is the price
    // of the bound rather than an oversight. It only shows up past `PATH_LOOKUP_WINDOW` matches
    // for one path, and it overstates rather than hides, which is the safer direction for a
    // "…and N more" line. Said here because D67 is what happens when a comment omits this.
    let total = if exhausted {
        let n: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM notes n WHERE {matches}"),
                params![ACTIVE, path],
                |r| r.get(0),
            )
            .map_err(sql("counting a path lookup"))?;
        n as usize
    } else {
        found.len()
    };
    Ok((found, total))
}

/// Free-text recall over titles and note **bodies**.
///
/// **The index narrows; the file decides.** That is [`concerning`]'s pattern, for the same
/// reason: the column the SQL could match on does not hold the answer. `body_excerpt` is
/// `body.split("\n\n").next()` truncated to 240 characters, so matching it in SQL searched the
/// first paragraph of a note and nothing else — a word in paragraph two returned `no notes
/// match` while `grep` found it on disk, and that had been true since the column existed (D88).
///
/// The old comment here said "`LIKE`, not FTS5, and that is a scope decision rather than an
/// oversight", which framed the limit as lexical-versus-semantic. The limit was that most of the
/// note was never searched. **A false comment about a mechanism is worse than an absent one**:
/// this one made the gap look decided.
///
/// **Still not FTS5, and now for a reason that can be checked.** A contentless FTS5 table
/// (`content=''`) would satisfy D34 — it stores an index and returns NULL for every column, so
/// `rm board.db` still loses nothing — and it is the right answer at a corpus size this vault
/// has not reached. The old comment's own condition was "when the citation ledger says lexical
/// recall is what is missing", and until D89 the ledger could not say anything about recall at
/// all. Fix the defect, fix the instrument, then let the instrument choose.
///
/// **Strictly more than it returned before.** The excerpt is a prefix of the body, so every note
/// the old query matched this one matches too; an unreadable file falls back to the excerpt
/// rather than dropping the note.
pub fn search(
    conn: &Connection,
    query: Option<&str>,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<IndexedNote>> {
    // **[`SEARCHABLE`], not a literal.** This read `n.kind = ?1` bound to `OBSERVATION`, which
    // made every decision in the vault unfindable by `recall` — not by anyone's decision, just by
    // the kind the query was written against before there were others (D86).
    let kinds = kinds_sql(SEARCHABLE);
    // **No text predicate here, deliberately.** Narrowing on `body_excerpt` for the index's sake
    // is what `concerning` does with `path_glob` — but that column holds the whole value and this
    // one holds a prefix, so the same move would discard exactly the notes this function exists
    // to find. The ordering is the one the caller sees; `LIMIT` moves below the match.
    let sql_text = format!(
        "{SELECT_NOTE}
          WHERE n.kind IN ({kinds})
            AND (?1 IS NULL OR n.scope = ?1)
          ORDER BY (n.kind = '{CAPTURE}'), (n.status = 'active') DESC, n.created DESC"
    );
    let mut stmt = conn.prepare(&sql_text).map_err(sql("preparing a search"))?;
    let candidates = stmt
        .query_map(params![project], row_to_note)
        .map_err(sql("running a search"))?
        .flatten();

    let needle = query
        .map(|q| q.trim().to_lowercase())
        .filter(|q| !q.is_empty());
    let Some(needle) = needle else {
        return Ok(candidates.take(limit).collect());
    };

    let vault = require_vault()?;
    let mut found = Vec::with_capacity(limit);
    for n in candidates {
        // Stop at the caller's limit rather than reading the rest of the vault. Ordering is
        // decided by the query above, so this is the same slice `LIMIT` used to return — the
        // worst case is a query that matches nothing, which reads every candidate once.
        if found.len() == limit {
            break;
        }
        if note_matches(&n, &vault, &needle) {
            found.push(n);
        }
    }
    Ok(found)
}

/// Whether one note answers a query, reading its file when the title does not.
///
/// The file read is the shell; [`body_contains`] is the decision and is tested without one.
fn note_matches(n: &IndexedNote, vault: &Path, needle: &str) -> bool {
    if n.title.to_lowercase().contains(needle) {
        return true;
    }
    match std::fs::read_to_string(vault.join(&n.vault_path)) {
        Ok(text) => body_contains(&text, needle),
        // The vault is truth and this one row disagrees with it — an index entry whose file is
        // gone or unreadable. Falling back to the excerpt returns what the old query would have,
        // so a broken file narrows the search instead of emptying it.
        Err(_) => n
            .excerpt
            .as_deref()
            .is_some_and(|e| e.to_lowercase().contains(needle)),
    }
}

/// Whether a note file's **body** contains an already-lowercased `needle`.
///
/// **The body, not the whole file.** Frontmatter carries the id, the slug and every path the note
/// declares, so matching it would make `recall rs` return every note that touches a `.rs` file —
/// which is what `--file` is for, and answering it here would make two commands quietly the same.
pub fn body_contains(file_text: &str, needle: &str) -> bool {
    let body = split_frontmatter(file_text).map_or(file_text, |(_, body)| body);
    body.to_lowercase().contains(needle)
}

/// Turn a user-supplied id into an exact one, or say why it cannot.
///
/// Ambiguity is an error naming the candidates, never a guess. A wrong resolution here would
/// attach a citation to the wrong note and quietly corrupt the only receipt this feature has.
pub fn resolve(conn: &Connection, input: &str) -> Result<NoteId> {
    // **An id that names its kind is already unambiguous, so parse it rather than re-deriving
    // the shape from a `rsplit_once`.** This used `split_id` and bound `OBSERVATION`, so
    // `capture/nest/slug` split into a *scope* called `capture/nest`, matched nothing, and
    // returned "no such note" — while D86 recorded captures as addressable and the e2e test
    // asserted exactly that in a comment. A decision was equally unreachable, and had been since
    // D81 created one. `parse_id` is the function that already knows every id shape, and using
    // it here is what makes `display` and `--cites` agree.
    if let Some(id) = parse_id(input) {
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM notes WHERE kind = ?1 AND scope = ?2 AND slug = ?3",
                params![id.kind, id.scope, id.slug],
                |r| r.get(0),
            )
            .map_err(sql("resolving a note id"))?;
        if found == 0 {
            return Err(Error::NoSuchNote(input.to_string()));
        }
        return Ok(id);
    }
    // `parse_id` declined: either a bare slug, or something qualified that names no kind we
    // have. The second must stay an error rather than falling through to the slug search, or
    // `nonsense/x/y` would quietly resolve whatever note happened to be called `y`.
    let (qualifier, slug) = split_id(input);
    if qualifier.is_some() {
        return Err(Error::NoSuchNote(input.to_string()));
    }
    // A bare slug, against every kind a person can name — the same list `recall` searches, so
    // anything findable is citable. Ambiguity is an error, never a pick (D50).
    let kinds = kinds_sql(SEARCHABLE);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT kind, scope FROM notes WHERE kind IN ({kinds}) AND slug = ?1
              ORDER BY kind, scope"
        ))
        .map_err(sql("resolving a note id"))?;
    let found: Vec<NoteId> = stmt
        .query_map(params![slug], |r| {
            Ok(NoteId::scoped(
                &r.get::<_, String>(0)?,
                &r.get::<_, String>(1)?,
                &slug,
            ))
        })
        .map_err(sql("resolving a note id"))?
        .flatten()
        .collect();
    match found.len() {
        0 => Err(Error::NoSuchNote(input.to_string())),
        1 => Ok(found[0].clone()),
        // Shown as full ids rather than bare scopes: with more than one kind reachable, a list of
        // scopes no longer tells the caller what to type.
        _ => Err(Error::AmbiguousNote {
            slug,
            projects: found
                .iter()
                .map(NoteId::display)
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

/// `amb memory recall`'s human output.
///
/// **Pure, for D92's reason**, and the empty case is the one that matters. A search that found
/// nothing is the reading D89 exists to make possible — `no notes match` and no output at all are
/// opposite answers, and only one of them tells the reader the mechanism ran.
///
/// A retired note is shown with its status rather than hidden: `recall` is a person asking, and
/// the superseded thing is often exactly what they are looking for. That is the opposite of the
/// injection rule, where retired notes are excluded, and the divergence is deliberate.
pub fn render_recall(notes: &[IndexedNote], at: f64) -> String {
    if notes.is_empty() {
        return "no notes match\n".into();
    }
    let mut out = String::new();
    for n in notes {
        out.push_str(&format!(
            "{} · {}{} — {}\n",
            n.id.display(),
            age(n.created, at),
            if n.status == ACTIVE {
                String::new()
            } else {
                format!(" [{}]", n.status)
            },
            n.title
        ));
        if !n.paths.is_empty() {
            out.push_str(&format!("    {}\n", n.paths.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {

    fn recalled(slug: &str, status: &str, paths: &[&str]) -> IndexedNote {
        IndexedNote {
            id: NoteId::observation("nest", slug),
            title: "a title".into(),
            status: status.into(),
            created: 0.0,
            vault_path: format!("projects/nest/{slug}.md"),
            excerpt: None,
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
            force: ADVICE.into(),
        }
    }

    /// **A search that found nothing must say so.**
    ///
    /// D89 exists because "nobody asked" and "somebody asked and the search missed" were printing
    /// the same zero. This is that rule on the human surface: no output at all is what a command
    /// that never ran looks like.
    #[test]
    fn a_search_that_matched_nothing_says_so_rather_than_printing_nothing() {
        assert_eq!(render_recall(&[], 0.0), "no notes match\n");
    }

    /// **Retired notes are shown here, and hidden from injection.** The divergence is the point:
    /// a person asking `recall` is often looking for exactly the superseded thing, whereas a
    /// session being injected into has not asked for anything.
    #[test]
    fn recall_shows_a_retired_note_and_marks_it_as_retired() {
        let active = render_recall(&[recalled("a", ACTIVE, &[])], 0.0);
        crate::assert_rendered_shape("render_recall", &active);
        assert!(
            !active.contains('['),
            "an active note is unadorned: {active}"
        );

        let retired = render_recall(&[recalled("a", "superseded", &[])], 0.0);
        assert!(retired.contains("[superseded]"), "{retired}");
        assert!(
            retired.contains("nest/a"),
            "and it is still reachable, not filtered out: {retired}"
        );
    }

    #[test]
    fn the_paths_line_appears_only_when_the_note_declares_paths() {
        assert!(!render_recall(&[recalled("a", ACTIVE, &[])], 0.0).contains("    "));
        let with = render_recall(&[recalled("a", ACTIVE, &["src/a.rs", "src/b.rs"])], 0.0);
        assert!(with.contains("    src/a.rs, src/b.rs"), "{with}");
    }
    /// The body is searched and the frontmatter is not — without a filesystem.
    ///
    /// [`note_matches`] is the shell; this is the decision, so it is tested the way
    /// `address::parse` and `claims::overlaps` are.
    #[test]
    fn a_body_is_searched_and_a_header_is_not() {
        let file = "---\nid: \"nest/2026-08-29-x\"\nscope: \"nest\"\ntitle: \"a note\"\n---\n\n\
                    First paragraph.\n\nZEBRAFINCH is in the second.\n";
        assert!(body_contains(file, "first paragraph"), "case-insensitive");
        assert!(body_contains(file, "zebrafinch"), "past the blank line");
        assert!(
            !body_contains(file, "nest"),
            "the scope is frontmatter and must not match, or every note answers its own project"
        );
        assert!(
            !body_contains(file, "a note"),
            "the title is frontmatter here; the title is matched from the index row instead"
        );
        // A file with no frontmatter at all is still searchable rather than invisible.
        assert!(body_contains("bare text with ZEBRAFINCH", "zebrafinch"));
    }

    use super::*;

    /// D45: a declined rebuild is stated, never rendered as an empty vault.
    #[test]
    fn a_declined_rebuild_is_reported_unless_the_index_is_current_anyway() {
        let stats = |skipped, scanned| IndexStats {
            scanned,
            skipped,
            ..IndexStats::default()
        };
        assert_eq!(
            index_is_behind(&stats(true, 501), 0),
            Some(501),
            "a vault the index has never seen must not render as 'no prior observations'"
        );
        assert_eq!(
            index_is_behind(&stats(true, 501), 501),
            None,
            "an index kept current by hand does not nag about the bound"
        );
        assert_eq!(
            index_is_behind(&stats(false, 501), 0),
            None,
            "a rebuild that actually ran is not behind"
        );
    }
    /// The cap is the decision, and it counts characters so a multi-byte payload is not cut
    /// through one.
    #[test]
    fn a_failure_note_is_titled_and_capped() {
        let (title, detail) = failure_note("Bash", "boom");
        assert_eq!(title, "Bash failed");
        assert_eq!(detail, "boom");

        let long = "é".repeat(FAILURE_DETAIL_MAX * 2);
        let (_, detail) = failure_note("Edit", &long);
        assert_eq!(
            detail.chars().count(),
            FAILURE_DETAIL_MAX,
            "counted in chars, not bytes — a note is not a log"
        );
        assert!(detail.chars().all(|c| c == 'é'), "no character was split");
    }
}

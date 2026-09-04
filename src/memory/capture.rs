//! Phase 4 — capture health and the cross-repo axis.
//!
//! The fail-loud counter lives in a file beside the board, not a table,
//! because "the board could not be opened" is one of the failures it records.

use super::*;

/// Consecutive hook failures before the layer says something out loud.
///
/// **D9's silence is right for delivery and wrong as an unlimited policy for capture.** On the
/// delivery side the worst case is a message arriving a turn late. Here it is *months of believing
/// something is recording when it is not* — which is not hypothetical: claude-mem's own corpus has
/// 85 queue items and 43 sessions stuck in a non-terminal state from one fortnight, after which
/// the system ran three more months and added 80,000 observations without ever surfacing them.
///
/// Borrowed from `CLAUDE_MEM_HOOK_FAIL_LOUD_THRESHOLD`, which exists for exactly this.
pub const FAIL_LOUD_AFTER: i64 = 3;

/// A marker whose session has been silent this long is a crashed session's residue, not a live
/// outage — [`failure_count`] ignores it rather than reporting a months-dead session forever.
/// Reader-side, so nothing on the hook path pays a directory sweep.
const STALE_MARKER_SECS: u64 = 30 * 86_400;

/// Where **this session's** consecutive-failure count lives.
///
/// A file rather than a table, deliberately: the failure this counts includes *"the board could
/// not be opened"*, and a counter that needs the board to record that the board is broken cannot
/// record it. Sits beside the board, so `rm -rf ~/.agent-messageboard` clears it too.
///
/// **Per session, not per machine (D108).** The count means *consecutive failures of one
/// session's capture*, and one shared file made the threshold unreachable exactly when it
/// should fire: on a multi-session machine, any healthy session's success cleared a broken
/// session's count, indefinitely. A file per session also gives the read-modify-write below a
/// single writer — the residual race is one session's own parallel tool calls, where a lost
/// increment delays the notice by one failure instead of resetting it.
fn failure_marker(payload: Option<&str>) -> Option<PathBuf> {
    let path = crate::db::db_path().ok()?;
    Some(path.with_file_name(marker_name(session_key(payload).as_deref())))
}

/// The marker filename for one session — or the shared pre-D108 name when no session is known.
fn marker_name(session: Option<&str>) -> String {
    match session {
        Some(s) => format!(".memory-failures-{s}"),
        None => ".memory-failures".to_string(),
    }
}

/// This session's key, made filesystem-safe. The same precedence as [`crate::identity::resolve_from`]:
/// `AMB_AGENT` overrides, then the host CLI's own session variable from `Vendor::session_env`,
/// then — on the hook path, where one is in hand — the id the payload carries.
///
/// **That third arm is D113's, and this function did not have it for two days** (M68). The
/// sentence above used to end at the environment and claim parity with `identity::resolve`; D113
/// added the payload fallback there and nothing brought it here, so the comment asserted a parity
/// that had stopped holding. The consequence is D108 reversed: with no environment variable
/// `session_key` returns `None`, every session on the machine shares the one `.memory-failures`
/// file, and any healthy session's success clears a broken session's count indefinitely — which
/// is the precise defect D108 exists to have fixed.
///
/// Latent until D115, and that is the part worth keeping. No shipped vendor reaches it — Claude
/// Code and Gemini CLI both export a variable — so it was unreachable on 2026-09-04 and became
/// reachable on 2026-09-05, when `parse_manifest` started accepting vendors that export nothing.
/// **A fix can widen the door to a bug it has nothing to do with**, and neither change is wrong.
fn session_key(payload: Option<&str>) -> Option<String> {
    session_key_with(|k| std::env::var(k).ok(), payload)
}

/// [`session_key`] with the environment injected, so the precedence is testable.
///
/// The same seam `vendors::detect_with` has and for the same recorded reason: M51 found every
/// mutant of this module's env shell alive, because a test cannot set process environment without
/// racing the parallel runner. Adding the arm without adding the seam would have left the new
/// precedence in exactly the state M51 was written about.
fn session_key_with(env: impl Fn(&str) -> Option<String>, payload: Option<&str>) -> Option<String> {
    let raw = env("AMB_AGENT")
        .or_else(|| crate::vendors::detect_with(&env).session_id(&env))
        .or_else(|| payload.map(str::to_string))?;
    sanitise_key(&raw)
}

/// The filesystem-safe form of one session's key, or `None` when nothing usable remains.
///
/// Extracted from the env shell above so the charset rule is testable without touching the
/// process environment — the M51 pass found the whole marker shell unguarded: every mutant of
/// this map and of the emptiness gate survived, because only the path-injected reader below it
/// had tests.
fn sanitise_key(raw: &str) -> Option<String> {
    let safe: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    (!safe.is_empty()).then_some(safe)
}

/// Record that this session's memory hook failed, and return its consecutive count.
///
/// **Never fails the caller.** A capture layer that could not write its own failure counter must
/// still not break a session (D9).
pub fn note_failure(payload: Option<&str>) -> i64 {
    let Some(path) = failure_marker(payload) else {
        return 0;
    };
    bump_marker(&path)
}

/// Read-increment-write one marker. Unreadable or garbage content restarts from zero rather
/// than failing — the counter must never be the thing that breaks (D9). Path-injected for the
/// same reason as [`worst_recent_marker`]: the M51 pass proved the env shell alone left every
/// mutant of this arithmetic alive.
fn bump_marker(path: &std::path::Path) -> i64 {
    let n = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    let _ = std::fs::write(path, n.to_string());
    n
}

/// Record that this session's memory hook succeeded. Clears its own count — and only its own,
/// which is the point of D108 — so the threshold means *consecutive*.
pub fn note_success(payload: Option<&str>) {
    if let Some(path) = failure_marker(payload) {
        clear_marker(&path);
    }
}

/// Remove one marker; absent is already the goal state, not an error.
fn clear_marker(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

/// The worst live count on the machine, for [`status`] and the fail-loud notice.
///
/// The machine-wide **max**, deliberately, and not this session's own count: the notice travels
/// through the memory hook's *success* path, so the one session that cannot deliver its own
/// warning is exactly the broken one. Healthy sessions carrying the worst count is the only
/// route the warning has (D108) — the pre-D108 global file did this by accident, and keeping it
/// on purpose is what this comment records.
pub fn failure_count() -> i64 {
    // `None`, and it changes nothing: only the *directory* is read below, and every session's
    // marker shares it. Passing this session's id would name a file that is never opened.
    let Some(marker) = failure_marker(None) else {
        return 0;
    };
    let Some(dir) = marker.parent() else {
        return 0;
    };
    worst_recent_marker(dir, std::time::Duration::from_secs(STALE_MARKER_SECS))
}

/// The largest fresh `.memory-failures*` count in one directory. Path-injected so it is
/// testable without touching the process environment.
fn worst_recent_marker(dir: &std::path::Path, max_age: std::time::Duration) -> i64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".memory-failures")
        })
        .filter(|e| {
            // A marker nothing has touched in a month is a crashed session's residue; counting
            // it would report a dead outage as a live one, forever, to every session.
            e.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age <= max_age)
        })
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .max()
        .unwrap_or(0)
}

/// One line, emitted once the threshold is crossed, and never by failing the hook.
///
/// Goes out through the same `additionalContext` channel as everything else, because the agent is
/// the one who can act on it. **Saying nothing is the failure mode being fixed**; failing the hook
/// would be a different and worse one.
pub fn fail_loud_notice(count: i64) -> Option<String> {
    (count >= FAIL_LOUD_AFTER).then(|| {
        format!(
            "[amb memory] a session's memory hook on this machine has failed {count} times in a \
             row and is capturing nothing. Run `amb memory status` — with AMB_HOOK_DEBUG=1 to \
             see why."
        )
    })
}

/// Who has touched a path recently, in **any** project on this machine.
///
/// **The one capability no per-repo tool has**, and the reason the vault is central rather than
/// one-per-repository. `concerning` already searches every project; this is the surface that says
/// so, and it is separated from it so that "was the cross-repo axis ever used?" is a countable
/// question rather than an impression (`MEASUREMENTS.md` asks for exactly that).
pub fn across_repos(conn: &Connection, path: &str, home: &str) -> Result<Vec<IndexedNote>> {
    let (mut found, _) = concerning(conn, path)?;
    // Foreign first here, unlike injection: the caller asked the cross-repo question, so the
    // local answers are the ones they could already have got.
    found.sort_by_key(|n| (n.id.scope == home, std::cmp::Reverse(n.created as i64)));
    Ok(found)
}

/// Facts about a session, read out of its transcript. **No model involved.**
///
/// 4b's split, and the reference is explicit that the two halves are not interchangeable: a
/// *summary* must come from `last_assistant_message` because the transcript *"is written
/// asynchronously and may lag the in-memory conversation"*, while *facts* — which files were
/// touched, which commands failed — exist nowhere else. Lag costs completeness here, not accuracy,
/// which is the direction that is survivable.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SessionFacts {
    pub files: Vec<String>,
    pub failures: Vec<String>,
    pub tools: usize,
}

impl SessionFacts {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.failures.is_empty()
    }

    /// Whether there is anything here worth writing a note about.
    ///
    /// **A summary alone is enough**, and that is the rule worth pinning: a person who ran
    /// `amb memory capture --summary "..."` on a transcript the parser could make nothing of has
    /// still said something, and refusing to record it would discard the only part a machine did
    /// not write. The reverse — facts with no summary — is also enough, because the facts *are*
    /// the content.
    ///
    /// Empty on both counts writes nothing, said out loud rather than silently: a vault of
    /// contentless notes is how the injection cap starts hiding real ones.
    pub fn worth_capturing(&self, summary: Option<&str>) -> bool {
        !self.is_empty() || summary.is_some()
    }
}

/// The title a captured session gets.
///
/// The failures clause appears only when there were failures. An unconditional
/// `", 0 failure(s)"` would make every clean session's title read as a report about failure.
pub fn capture_title(facts: &SessionFacts) -> String {
    let failures = if facts.failures.is_empty() {
        String::new()
    } else {
        format!(", {} failure(s)", facts.failures.len())
    };
    format!("session touched {} file(s){failures}", facts.files.len())
}

/// Write one captured session into the vault.
///
/// **Deliberately an [`OBSERVATION`], not a [`CAPTURE`], and this function exists so that choice
/// is asserted rather than asserted-about.** Its body is machine-derived like a failure capture's,
/// but a person ran `amb memory capture` to make it — D86's line is whether *anything decided the
/// note was worth having*, and here something did. So it is injectable, and
/// `a_captured_session_is_an_observation_and_can_therefore_be_injected` reddens if the kind is
/// changed to one that is not.
///
/// [`ADVICE`], never binding: nothing becomes a rule without a person choosing it.
pub fn capture_session(
    conn: &Connection,
    me: &Identity,
    facts: &SessionFacts,
    summary: Option<&str>,
    at: f64,
) -> Result<Written> {
    observe(
        conn,
        me,
        &Observation {
            kind: OBSERVATION,
            title: &capture_title(facts),
            learned: &render_facts(facts, summary),
            project: &me.project,
            files: &facts.files,
            cites: &[],
            supersedes: None,
            force: ADVICE,
        },
        at,
    )
}

/// Parse a transcript into facts, ignoring everything it does not recognise.
///
/// **The transcript is an internal format with no compatibility promise**, so this is written to
/// degrade rather than break: every line is independent, a line that will not parse is skipped,
/// and a file whose whole shape has changed yields empty facts rather than an error. Pure, so the
/// parsing rules are testable without a session.
pub fn parse_transcript(text: &str, root: &str) -> SessionFacts {
    let mut facts = SessionFacts::default();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Tool calls appear in more than one shape across versions; look for the payload rather
        // than for a particular envelope.
        let Some(name) = find_str(&v, "tool_name").or_else(|| find_str(&v, "name")) else {
            continue;
        };
        facts.tools += 1;
        if let Some(path) = find_str(&v, "file_path")
            && let Some(rel) = claims::relative_to(root, &path)
            && !facts.files.contains(&rel)
        {
            facts.files.push(rel);
        }
        // A failure is disproportionately what is worth remembering, which is why it is picked
        // out rather than counted.
        let failed = v
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || find_str(&v, "status").is_some_and(|s| s == "error");
        if failed {
            let what = find_str(&v, "command").unwrap_or(name);
            let what: String = what.chars().take(120).collect();
            if !facts.failures.contains(&what) {
                facts.failures.push(what);
            }
        }
    }
    facts
}

/// Find a string field anywhere in a JSON value, breadth-insensitively.
///
/// The transcript nests tool payloads differently across versions; searching by key rather than
/// by path is what lets one parser survive a reshuffle.
fn find_str(v: &serde_json::Value, key: &str) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get(key) {
                return Some(s.clone());
            }
            map.values().find_map(|inner| find_str(inner, key))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_str(i, key)),
        _ => None,
    }
}

/// Render session facts as the body of an observation.
pub fn render_facts(facts: &SessionFacts, summary: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str(s.trim());
        out.push_str("\n\n");
    }
    if !facts.failures.is_empty() {
        out.push_str("Failed during this session:\n");
        for f in &facts.failures {
            out.push_str(&format!("- {f}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{} tool call(s); touched {} file(s).\n",
        facts.tools,
        facts.files.len()
    ));
    out
}

/// Candidates concerning these paths, **ledgered as shown**.
///
/// This is [`candidates_concerning`] wired to its purpose, which it was not: the query existed and
/// nothing called it. Showing a near-match is an *injection* — the caller now knows about a
/// candidate it did not know about — so it goes in the citation ledger, and a candidate derived
/// after seeing one is a citation rather than a derivation by the counting rule.
///
/// **Shown after the note is authored, never before it is thought.** That is what keeps it
/// consistent with candidates never being injected: it is a linking affordance offered to a writer
/// who has already written, not context handed to a reader who is still deciding.
pub fn near_candidates(
    conn: &Connection,
    me: &Identity,
    paths: &[String],
    at: f64,
) -> Result<Vec<IndexedNote>> {
    let found = candidates_concerning(conn, paths)?;
    let ids: Vec<NoteId> = found.iter().map(|n| n.id.clone()).collect();
    record_injected(conn, &me.id, &ids, at, Source::File)?;
    Ok(found)
}

/// Promote something without waiting for three derivations.
///
/// **The override the plan asks for**, because *"frequency favours trivia"* — a thing noticed once
/// can matter more than a thing noticed three times, and an arithmetic gate with no human override
/// is the count pretending to be judgement.
///
/// It still requires explicit confirmation, and the promoted file records that it arrived this way
/// with an empty ledger, so a reader can see the difference between *earned* and *asserted*.
pub fn promote_direct(conn: &Connection, me: &Identity, id: &NoteId, at: f64) -> Result<Note> {
    let vault = require_vault()?;
    let source = load(&vault, id, at)?;
    let promoted = Note {
        // A direct promotion carries no ledger, so there is nothing to route on: it lands at
        // the scope the id already named, or — for a candidate, which has none — the project the
        // person asking is standing in.
        id: NoteId {
            kind: DECISION.to_string(),
            scope: if id.scope.is_empty() {
                me.project.clone()
            } else {
                id.scope.clone()
            },
            slug: source.id.slug.clone(),
        },
        title: source.title.clone(),
        status: ACTIVE.into(),
        created: at,
        session: Some(me.id.clone()),
        agent: Some(me.name.clone()),
        files: source.files.clone(),
        cites: Vec::new(),
        supersedes: None,
        superseded_by: None,
        promoted_from: Some(source.id.display()),
        promoted_to: None,
        visibility: source.visibility.clone(),
        // Inherited, like scope. A promotion changes a note's *lifecycle*, not how
        // binding it is — resetting it here would silently demote every rule that
        // earned its way through the ledger.
        force: source.force.clone(),
        declined_at: None,
        declined_after: None,
        // Deliberately empty. A direct promotion has no derivation evidence, and inventing some
        // would make an assertion indistinguishable from a thing that was actually rediscovered.
        derivations: Vec::new(),
        body: source.body.clone(),
    };
    let rel = vault_rel(&promoted.id.kind, &promoted.id.scope, &promoted.id.slug);
    let path = vault.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let rendered = promoted.render();
    write_private(&path, &rendered)?;
    upsert(conn, &promoted, &rel, file_mtime(&path), at)?;
    Ok(promoted)
}

/// Counter names. String constants rather than an enum so a name in the database and a name in
/// the code cannot drift.
pub const COUNTER_EXPORT_CHECK: &str = "export_check_run";
pub const COUNTER_EXPORT_STALE: &str = "export_check_failed";
pub const COUNTER_CROSS_REPO: &str = "cross_repo_query";

/// The receipts the plan demands for Phases 2, 3 and 4b.
///
/// **D49 claimed the decline rate was observable and it was not.** That decision rests on it:
/// *"if approval degrades to a rubber stamp … the phase is withdrawn"*, and the stated way to see
/// that is the decline rate. A withdrawal condition nobody can evaluate is not a condition. This
/// is the same defect as the export comment in D53 — a claim asserted in prose that the code did
/// not implement — one level up, in a decision rather than a doc comment.
#[derive(Debug, Default, Clone)]
pub struct PhaseReceipts {
    /// Phase 2: have any candidates reached the threshold at all?
    pub candidates: usize,
    pub reached_threshold: usize,
    pub promoted: usize,
    pub declined: usize,
    /// **Candidates a decline is holding back right now**: past the threshold, so they would be
    /// offered, but declined and not derived since.
    ///
    /// The suppression itself already worked — `ready_candidates` has always skipped these — but
    /// nothing counted it, so an offer withheld was indistinguishable from an offer never earned.
    /// A silently withheld offer is this project's worst failure shape, and this is also the
    /// tombstone-ROI number that has never existed: what declining has actually bought (D64).
    pub suppressed: usize,
    /// Phase 3: has `--check` ever run, and has it ever fired?
    pub export_checks: i64,
    pub export_failures: i64,
    /// Phase 4b: is the differentiator ever used?
    pub cross_repo_queries: i64,
}

impl PhaseReceipts {
    /// Offers made, as far as the vault can tell: a candidate that reached the threshold was
    /// offered at least once.
    pub fn offers(&self) -> usize {
        self.promoted + self.declined
    }

    /// **The number D49's withdrawal condition is read off.** `None` when nothing has been
    /// offered — a rate over zero offers is not a low rate, it is no data, and reporting `0.00`
    /// there would read as "approval has become reflex" when nothing has been approved.
    pub fn decline_rate(&self) -> Option<f64> {
        (self.offers() > 0).then(|| self.declined as f64 / self.offers() as f64)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "candidates": self.candidates,
            "reached_threshold": self.reached_threshold,
            "promoted": self.promoted,
            "declined": self.declined,
            "decline_rate": self.decline_rate(),
            "export_checks": self.export_checks,
            "export_failures": self.export_failures,
            "cross_repo_queries": self.cross_repo_queries,
            "suppressed": self.suppressed,
        })
    }
}

/// Gather them. The vault answers two; the counters answer the two that leave no trace in a file.
pub fn phase_receipts(conn: &Connection, vault: &Path, at: f64) -> Result<PhaseReceipts> {
    let all = list_candidates(conn, vault, at, false)?;
    let t = threshold();
    Ok(PhaseReceipts {
        candidates: all.len(),
        reached_threshold: all.iter().filter(|n| n.derivations.len() >= t).count(),
        promoted: all.iter().filter(|n| n.status == PROMOTED).count(),
        declined: all.iter().filter(|n| n.declined_after.is_some()).count(),
        // The same predicate `ready_candidates` skips on, counted rather than reinvented: past
        // the threshold, declined, and nothing has derived since.
        suppressed: all
            .iter()
            .filter(|n| {
                n.derivations.len() >= t
                    && n.declined_after.is_some_and(|c| n.derivations.len() <= c)
            })
            .count(),
        export_checks: crate::db::counter(conn, COUNTER_EXPORT_CHECK),
        export_failures: crate::db::counter(conn, COUNTER_EXPORT_STALE),
        cross_repo_queries: crate::db::counter(conn, COUNTER_CROSS_REPO),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failures clause is conditional, and an unconditional one would misread every clean
    /// session's title as a report about failure.
    #[test]
    fn a_clean_session_title_says_nothing_about_failures() {
        let clean = SessionFacts {
            files: vec!["a.rs".into(), "b.rs".into()],
            failures: Vec::new(),
            tools: 9,
        };
        assert_eq!(capture_title(&clean), "session touched 2 file(s)");

        let broke = SessionFacts {
            failures: vec!["cargo test".into()],
            ..clean
        };
        assert_eq!(
            capture_title(&broke),
            "session touched 2 file(s), 1 failure(s)"
        );
    }

    /// **A summary alone is enough**, and that is the half a `&&` gets wrong.
    ///
    /// A person who ran `capture --summary "..."` against a transcript the parser could make
    /// nothing of has still said something, and it is the only part of the note a machine did not
    /// write. Flip the `||` to `&&` and this reddens on that case alone — which is the case that
    /// matters, because the other three are unchanged.
    #[test]
    fn a_summary_alone_is_worth_capturing_and_nothing_at_all_is_not() {
        let nothing = SessionFacts::default();
        let something = SessionFacts {
            files: vec!["a.rs".into()],
            ..SessionFacts::default()
        };
        assert!(!nothing.worth_capturing(None), "empty and unexplained");
        assert!(
            nothing.worth_capturing(Some("I learned a thing")),
            "a summary is content"
        );
        assert!(something.worth_capturing(None), "facts are content");
        assert!(something.worth_capturing(Some("both")));
    }

    #[test]
    fn a_transcript_yields_files_and_failures_without_a_model() {
        let text = [
            r#"{"tool_name":"Read","tool_input":{"file_path":"/repo/src/a.rs"}}"#,
            r#"{"tool_name":"Edit","tool_input":{"file_path":"/repo/src/b.rs"}}"#,
            r#"{"tool_name":"Bash","tool_input":{"command":"cargo test"},"is_error":true}"#,
            r#"{"tool_name":"Read","tool_input":{"file_path":"/elsewhere/c.rs"}}"#,
            // Both rows of the *other* failure signal. The `status == "error"` arm was reached
            // by no fixture — every failure here came in through `is_error`, so flipping that
            // comparison survived the suite (M51, M17's shape).
            r#"{"tool_name":"Bash","tool_input":{"command":"sqlite3 broke"},"status":"error"}"#,
            r#"{"tool_name":"Bash","tool_input":{"command":"cargo fmt"},"status":"success"}"#,
        ]
        .join("\n");
        let f = parse_transcript(&text, "/repo");
        assert_eq!(f.failures, vec!["cargo test", "sqlite3 broke"]);
        assert_eq!(f.tools, 6);
    }
    #[test]
    fn a_transcript_in_an_unrecognised_shape_yields_nothing_rather_than_failing() {
        // The transcript is an internal format with no compatibility promise. A parser that
        // errors on a reshuffle takes a hook with it; one that returns empty facts loses
        // completeness, which is the survivable direction.
        for text in [
            "",
            "not json at all",
            "{}",
            r#"{"unexpected":"shape"}"#,
            "null\n[]\n{\"a\":[{\"b\":1}]}",
            r#"{"tool_name":"Read"}"#,
        ] {
            let f = parse_transcript(text, "/repo");
            assert!(f.files.is_empty(), "{text:?}");
            assert!(f.failures.is_empty(), "{text:?}");
        }
    }
    #[test]
    fn a_nested_payload_is_still_found_when_the_envelope_moves() {
        // Searched by key rather than by path, so one parser survives the tool payload being
        // nested a level deeper in a later version.
        // One line: the parser reads a transcript line by line, so a test spanning two lines is
        // testing something the format never produces.
        let deep = concat!(
            r#"{"type":"tool_use","message":{"content":"#,
            r#"[{"tool_name":"Write","tool_input":{"file_path":"/repo/src/deep.rs"}}]}}"#
        );
        let f = parse_transcript(deep, "/repo");
        assert_eq!(f.files, vec!["src/deep.rs"]);
    }
    #[test]
    fn a_failure_detail_is_capped_because_a_note_is_not_a_log() {
        let huge = "x".repeat(5000);
        let line = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"{huge}"}},"is_error":true}}"#
        );
        let f = parse_transcript(&line, "/repo");
        assert_eq!(f.failures.len(), 1);
        assert!(f.failures[0].len() <= 120, "got {}", f.failures[0].len());
    }
    /// **The precedence D113 added to identity and this module did not get** (M68). Four rows,
    /// and the third is the one that was missing: a vendor exporting no session variable is
    /// identified by the id its hook payload carries, exactly as `identity::resolve_from` does.
    ///
    /// The last two assertions are the *consequence* rather than the mechanism, and they are why
    /// this is a defect rather than an inconsistency. With no key at all every session on the
    /// machine writes one shared marker file, so any healthy session's success clears a broken
    /// session's count — the state D108 exists to have fixed, reachable again through a door
    /// D115 opened on the same day this was found.
    #[test]
    fn the_marker_key_falls_back_to_the_payload_before_it_falls_back_to_sharing() {
        let none = |_: &str| -> Option<String> { None };
        let vendor_env = |k: &str| (k == "CLAUDE_CODE_SESSION_ID").then(|| "from-env".to_string());
        let flag = |k: &str| (k == "AMB_AGENT").then(|| "from-flag".to_string());

        assert_eq!(
            session_key_with(flag, Some("from-payload")).as_deref(),
            Some("from-flag"),
            "AMB_AGENT outranks everything, as it does in identity::resolve_from"
        );
        assert_eq!(
            session_key_with(vendor_env, Some("from-payload")).as_deref(),
            Some("from-env"),
            "the vendor's own variable outranks the payload — D113 put the payload last so that \
             adding it changes nothing for a vendor that already worked"
        );
        assert_eq!(
            session_key_with(none, Some("from-payload")).as_deref(),
            Some("from-payload"),
            "and with no variable anywhere, the payload names the session"
        );
        assert_eq!(
            session_key_with(none, None),
            None,
            "nothing at all still degrades rather than panicking"
        );

        assert_eq!(
            marker_name(session_key_with(none, Some("s1")).as_deref()),
            ".memory-failures-s1",
            "a payload-only session gets its own marker file"
        );
        assert_eq!(
            marker_name(session_key_with(none, None).as_deref()),
            ".memory-failures",
            "and only a session nothing can name at all falls back to the shared one"
        );
    }

    /// D108's two rules in one table: the reader takes the machine's worst *fresh* marker (a
    /// healthy session must carry a broken sibling's count, or the notice can never travel),
    /// and a marker a month silent is a crashed session's residue, not a live outage.
    #[test]
    fn the_worst_fresh_marker_wins_and_a_stale_one_is_residue() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".memory-failures-alice"), "2").expect("write");
        std::fs::write(dir.path().join(".memory-failures-bob"), "7").expect("write");
        std::fs::write(dir.path().join("board.db"), "not a marker").expect("write");
        let day = std::time::Duration::from_secs(86_400);
        assert_eq!(worst_recent_marker(dir.path(), day), 7);

        // Age bob's marker past the horizon: the residue stops counting, alice's 2 remains.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(90 * 86_400);
        let f = std::fs::File::options()
            .append(true)
            .open(dir.path().join(".memory-failures-bob"))
            .expect("open");
        f.set_modified(old).expect("age");
        assert_eq!(worst_recent_marker(dir.path(), day), 2);

        // An empty directory reports quiet, not an error.
        let empty = tempfile::tempdir().expect("tempdir");
        assert_eq!(worst_recent_marker(empty.path(), day), 0);
    }

    /// The filename is the session key, so two sessions cannot clear each other's count —
    /// and no session at all degrades to the shared pre-D108 name rather than to a panic.
    /// The marker's whole lifecycle, against a real file: consecutive counts, garbage
    /// restarting from one, and cleared meaning the count starts over. Every one of these rows
    /// is a mutant that survived M51 — `note_failure` could return 0, 1 or -1, its `+` could be
    /// `-` or `*`, and `note_success` could do nothing, because only the pure readers had tests.
    #[test]
    fn the_marker_counts_consecutively_and_clearing_restarts_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join(".memory-failures-x");
        assert_eq!(bump_marker(&p), 1);
        assert_eq!(bump_marker(&p), 2);
        assert_eq!(bump_marker(&p), 3, "consecutive, not idempotent");
        std::fs::write(&p, "not a number").expect("garbage");
        assert_eq!(
            bump_marker(&p),
            1,
            "garbage restarts the count rather than failing"
        );
        clear_marker(&p);
        assert!(!p.exists(), "cleared is gone");
        clear_marker(&p); // absent is the goal state, not an error
        assert_eq!(bump_marker(&p), 1, "cleared means consecutive starts over");
    }

    /// The charset map row by row: kept, mapped, truncated, refused.
    #[test]
    fn a_session_key_is_made_filesystem_safe_or_refused() {
        assert_eq!(
            sanitise_key("uuid.broken/x"),
            Some("uuid-broken-x".into()),
            "anything outside [a-zA-Z0-9_-] becomes a dash"
        );
        assert_eq!(
            sanitise_key("a_b-1"),
            Some("a_b-1".into()),
            "underscore and dash pass through"
        );
        assert_eq!(
            sanitise_key(""),
            None,
            "nothing usable is None, never Some(\"\")"
        );
        let long = sanitise_key(&"a".repeat(70)).expect("long keys are truncated, not refused");
        assert_eq!(long.len(), 64);
    }

    /// **Zero offers is no data, not a low rate** — the number D49's withdrawal condition is
    /// read off. `> 0` relaxing to `>= 0` turns "nothing has been offered" into `0/0`, and the
    /// division flipping to `*` turns one decline in two offers into 2.00; both survived M51.
    #[test]
    fn a_decline_rate_over_zero_offers_is_no_data_not_a_low_rate() {
        let two = PhaseReceipts {
            promoted: 1,
            declined: 1,
            ..PhaseReceipts::default()
        };
        assert_eq!(two.decline_rate(), Some(0.5));
        assert_eq!(
            PhaseReceipts::default().decline_rate(),
            None,
            "a rate over zero offers would read as approval-become-reflex"
        );
    }

    #[test]
    fn the_marker_is_keyed_by_session() {
        assert_eq!(marker_name(Some("abc-123")), ".memory-failures-abc-123");
        assert_eq!(marker_name(None), ".memory-failures");
        assert_ne!(marker_name(Some("a")), marker_name(Some("b")));
    }

    #[test]
    fn the_fail_loud_notice_waits_for_a_run_of_failures_and_then_says_what_to_do() {
        // D9's silence is right for delivery and wrong as an unlimited policy for capture: the
        // worst case here is months of believing something is recording when it is not.
        assert!(fail_loud_notice(0).is_none());
        assert!(fail_loud_notice(FAIL_LOUD_AFTER - 1).is_none());
        let notice = fail_loud_notice(FAIL_LOUD_AFTER).expect("speaks at the threshold");
        assert!(notice.contains("capturing nothing"), "{notice}");
        assert!(
            notice.contains("amb memory status"),
            "and says what to run: {notice}"
        );
    }

    /// **The body of a capture note, and the second of two renderers the crate never asserted.**
    ///
    /// A capture is machine-written scrollback: never injected (D86, enforced by `INJECTABLE`),
    /// but indexed and searchable, which makes this text the only thing a later session can find.
    /// Three rules live here and none was pinned until M23 — the failures section is conditional,
    /// each failure is written out rather than counted, and the tool and file totals are
    /// unconditional so a clean session still records what it did.
    #[test]
    fn a_capture_body_writes_each_failure_out_and_omits_the_section_when_there_were_none() {
        let clean = SessionFacts {
            files: vec!["a.rs".into(), "b.rs".into()],
            failures: Vec::new(),
            tools: 9,
        };
        let out = render_facts(&clean, None);
        crate::assert_rendered_shape("render_facts", &out);
        assert!(
            !out.contains("Failed during this session"),
            "a clean session must not carry an empty failure heading: {out}"
        );
        assert_eq!(
            out, "9 tool call(s); touched 2 file(s).\n",
            "and the totals are unconditional — a capture with nothing to report still reports"
        );

        let broke = SessionFacts {
            failures: vec!["cargo test: 3 failed".into(), "clippy: 1 error".into()],
            ..clean
        };
        let out = render_facts(&broke, None);
        // Each one, not a count. A count is not searchable, and searchable is the whole reason
        // this kind stays in the index while being excluded from injection.
        assert!(out.contains("- cargo test: 3 failed"), "{out}");
        assert!(out.contains("- clippy: 1 error"), "{out}");
        assert!(
            out.contains("9 tool call(s); touched 2 file(s)."),
            "the totals survive the failure section: {out}"
        );
    }

    /// A summary leads, so the sentence a person wrote is the first thing a reader meets rather
    /// than a machine's tally. Trimmed, because it arrives from a CLI argument.
    #[test]
    fn a_summary_leads_the_capture_and_its_surrounding_whitespace_is_not_published() {
        let facts = SessionFacts {
            files: vec!["a.rs".into()],
            failures: vec!["boom".into()],
            tools: 1,
        };
        let out = render_facts(&facts, Some("  the lock order was wrong  \n"));
        assert!(
            out.starts_with("the lock order was wrong\n\n"),
            "the summary leads and is trimmed: {out:?}"
        );
        assert!(
            out.find("the lock order").expect("summary")
                < out.find("Failed during").expect("failures"),
            "a person's sentence before the machine's: {out}"
        );
    }
}

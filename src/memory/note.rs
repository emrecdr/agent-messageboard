//! The note itself: frontmatter, the derivation ledger, and parsing.
//!
//! The vault is truth (D34). Everything here is about the *file* — what is
//! written into it and what is read back out.

use super::*;

// ── Frontmatter ─────────────────────────────────────────────────────────────

/// A note as it is written to disk and read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub status: String,
    pub created: f64,
    pub session: Option<String>,
    pub agent: Option<String>,
    pub files: Vec<String>,
    pub cites: Vec<String>,
    pub supersedes: Option<String>,
    pub superseded_by: Option<String>,
    /// **The derivation ledger lives in the file, not only in the index** — otherwise
    /// `rm board.db` destroys the evidence a promotion was offered on, and D34 promises it does
    /// not. `notes.derived_count` mirrors this; where they disagree the file wins.
    pub derivations: Vec<Derivation>,
    /// Set on a promoted decision or pattern, naming the candidate it came from.
    pub promoted_from: Option<String>,
    /// Set on an archived candidate, naming what it became.
    pub promoted_to: Option<String>,
    /// `private` keeps a decision out of `export`. Anything else, including absent, publishes.
    pub visibility: Option<String>,
    /// How binding this is: `advice` (the default), `decision`, or `rule`. Absent means `advice`,
    /// so every note written before the field existed keeps behaving exactly as it did.
    pub force: String,
    /// When the user last declined this candidate — for the human reading the file.
    pub declined_at: Option<f64>,
    /// **How many derivations it had when it was declined**, which is what "not re-offered until
    /// it derives again" actually means.
    ///
    /// Comparing timestamps instead was wrong and the tests caught it: frontmatter stores whole
    /// seconds, so a decline and a derivation in the same second compare *equal* and the candidate
    /// stayed silent forever. A count has no resolution to lose, and it says the rule directly.
    pub declined_after: Option<usize>,
    pub body: String,
}

/// One occurrence of a thing being noticed independently.
///
/// **Projects, not sessions** — `derived_in` counts distinct projects because that is what decides
/// the destination: one project makes a project decision, two or more make a personal pattern.
/// Sessions would over-count the same insight had twice in one afternoon.
#[derive(Debug, Clone, PartialEq)]
pub struct Derivation {
    pub ts: f64,
    pub project: String,
    pub session: String,
    pub note: String,
    /// What the deriving repository *was* — its topics, at the moment the derivation was recorded.
    ///
    /// **Recorded rather than looked up later, because later there is nothing to look up.** A
    /// derivation names a project; detecting that project's topics needs its repository root, and
    /// the only session that ever had it is the one that recorded this. Asking afterwards would
    /// mean the router could route on the topics of *this* machine's checkout of a project rather
    /// than on what was true when the thing was noticed.
    ///
    /// This is D74's lesson applied before the fact instead of after: the axis the router decides
    /// on is written down at the moment the evidence is created, so "do topic-scoped notes get
    /// cited more than global ones" stays answerable.
    pub topics: Vec<String>,
}

impl Derivation {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ts": format_ts(self.ts),
            "project": self.project,
            "session": self.session,
            "note": self.note,
            "topics": self.topics,
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Self> {
        Some(Derivation {
            ts: v.get("ts").and_then(|x| x.as_str()).and_then(parse_ts)?,
            project: v.get("project")?.as_str()?.to_string(),
            session: v
                .get("session")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            note: v
                .get("note")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            // Absent means "recorded before topics existed", which is honestly *unknown* rather
            // than *none* — and the router treats an unknown as sharing nothing, so an old
            // derivation can only ever route a promotion outward to `@@`, never wrongly inward
            // to a topic it was never observed to be in.
            topics: v
                .get("topics")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// Emit a scalar as a JSON string.
///
/// **JSON is a subset of YAML 1.2, so this is valid frontmatter that Obsidian reads** — and it
/// round-trips exactly, with no quoting rules to get wrong. A title containing a colon, a quote
/// or a `#` is the ordinary case here, not an edge one, and hand-rolled YAML quoting is where
/// that would have broken silently.
fn yaml_scalar(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

fn yaml_read(raw: &str) -> String {
    let raw = raw.trim();
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(s)) => s,
        _ => raw.trim_matches('"').to_string(),
    }
}

impl Note {
    /// The complete file: frontmatter, then prose.
    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        out.push_str(&format!("id: {}\n", yaml_scalar(&self.id.display())));
        out.push_str(&format!("kind: {}\n", yaml_scalar(&self.id.kind)));
        out.push_str(&format!("scope: {}\n", yaml_scalar(&self.id.scope)));
        out.push_str(&format!("title: {}\n", yaml_scalar(&self.title)));
        out.push_str(&format!("status: {}\n", yaml_scalar(&self.status)));
        out.push_str(&format!(
            "created: {}\n",
            yaml_scalar(&format_ts(self.created))
        ));
        if let Some(s) = &self.session {
            out.push_str(&format!("session: {}\n", yaml_scalar(s)));
        }
        if let Some(a) = &self.agent {
            out.push_str(&format!("agent: {}\n", yaml_scalar(a)));
        }
        push_list(&mut out, "files", &self.files);
        push_list(&mut out, "cites", &self.cites);
        if let Some(s) = &self.supersedes {
            out.push_str(&format!("supersedes: {}\n", yaml_scalar(s)));
        }
        if let Some(s) = &self.superseded_by {
            out.push_str(&format!("superseded_by: {}\n", yaml_scalar(s)));
        }
        if let Some(s) = &self.promoted_from {
            out.push_str(&format!("promoted_from: {}\n", yaml_scalar(s)));
        }
        if let Some(s) = &self.promoted_to {
            out.push_str(&format!("promoted_to: {}\n", yaml_scalar(s)));
        }
        if self.force != ADVICE {
            out.push_str(&format!("force: {}\n", yaml_scalar(&self.force)));
        }
        if let Some(s) = &self.visibility {
            out.push_str(&format!("visibility: {}\n", yaml_scalar(s)));
        }
        if let Some(t) = self.declined_at {
            out.push_str(&format!("declined_at: {}\n", yaml_scalar(&format_ts(t))));
        }
        if let Some(c) = self.declined_after {
            out.push_str(&format!("declined_after: {c}\n"));
        }
        if !self.derivations.is_empty() {
            // The count and the spread are written out beside the ledger rather than left to be
            // recomputed: a human opening this file sees *why* it is being offered without
            // having to add up the list underneath.
            out.push_str(&format!("derived_count: {}\n", self.derivations.len()));
            let mut projects: Vec<&str> = self
                .derivations
                .iter()
                .map(|d| d.project.as_str())
                .collect();
            projects.sort_unstable();
            projects.dedup();
            push_list(
                &mut out,
                "derived_in",
                &projects
                    .iter()
                    .map(|p| (*p).to_string())
                    .collect::<Vec<_>>(),
            );
            // Each entry is a JSON object, which is also a YAML flow mapping — the same trick the
            // scalars use, so the ledger round-trips exactly and Obsidian still renders it.
            out.push_str("derivations:\n");
            for d in &self.derivations {
                out.push_str(&format!("  - {}\n", d.to_json()));
            }
        }
        out.push_str("---\n\n");
        out.push_str(self.body.trim());
        out.push('\n');
        out
    }
}

fn push_list(out: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(key);
    out.push_str(":\n");
    for i in items {
        out.push_str(&format!("  - {}\n", yaml_scalar(i)));
    }
}

/// A note's frontmatter, scanned once: scalar entries and list entries, in file order.
type Frontmatter = (Vec<(String, String)>, Vec<(String, Vec<String>)>);

/// Split a note into its frontmatter block and its body. `None` when it has no frontmatter.
pub(crate) fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (front, body) = rest.split_at(end);
    Some((
        front,
        body.trim_start_matches("\n---").trim_start_matches('\n'),
    ))
}

/// Read every key the frontmatter declares, applying exactly the rules `parse_note` applies.
///
/// **Extracted so `parse_note` and [`unknown_keys`] cannot disagree about what counts as a key.**
/// The warning's whole claim is "no reader consults this one"; a second, subtly different scanner
/// would report keys `parse_note` never saw as ghosts and stay silent about real ones. A warning
/// that lies is worse than no warning, and one scanner is the only way to make that impossible.
pub(crate) fn scan_frontmatter(front: &str) -> Frontmatter {
    let mut scalars: Vec<(String, String)> = Vec::new();
    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<String> = None;
    for line in front.lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some(key) = &current
                && let Some(entry) = lists.iter_mut().find(|(k, _)| k == key)
            {
                entry.1.push(yaml_read(item));
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() || line.starts_with(char::is_whitespace) {
            continue;
        }
        if value.trim().is_empty() {
            lists.push((key.clone(), Vec::new()));
            current = Some(key);
        } else {
            scalars.push((key, yaml_read(value)));
            current = None;
        }
    }
    (scalars, lists)
}

/// Read a note back from disk.
///
/// A deliberately small YAML subset — `key: scalar` and `key:` followed by `  - item` — because
/// this parser only ever reads what [`Note::render`] writes, plus whatever a human types into
/// Obsidian. Anything it cannot understand is ignored rather than fatal: a note that fails to
/// parse must not be able to break a hook (D36).
pub fn parse_note(text: &str, fallback_slug: &str, fallback_mtime: f64) -> Option<Note> {
    let (front, body) = split_frontmatter(text)?;
    let (scalars, lists) = scan_frontmatter(front);

    let get = |k: &str| -> Option<String> {
        scalars
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };
    let list = |k: &str| -> Vec<String> {
        lists
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };

    // **The frontmatter key is `scope`, and there is no `project:` fallback** (D81). No
    // backward compatibility to carry: the vault is regenerable, and a key that means two things
    // in two files is the drift this whole change removes.
    let scope = get("scope")?;
    let title = get("title")?;
    let kind = get("kind").unwrap_or_else(|| OBSERVATION.to_string());
    // The filename is the fallback identity, not an error: a note typed straight into Obsidian
    // has no `id:` line, and refusing to index it would make the vault's truth conditional on
    // amb having written it.
    let slug = match get("id") {
        Some(id) => split_id(&id).1,
        None => fallback_slug.to_string(),
    };
    let slug = if slug.is_empty() {
        fallback_slug.to_string()
    } else {
        slug
    };
    Some(Note {
        id: NoteId { kind, scope, slug },
        title,
        status: get("status").unwrap_or_else(|| ACTIVE.to_string()),
        created: get("created")
            .and_then(|s| parse_ts(&s))
            .unwrap_or(fallback_mtime),
        session: get("session"),
        agent: get("agent"),
        files: list("files"),
        cites: list("cites"),
        supersedes: get("supersedes"),
        superseded_by: get("superseded_by"),
        promoted_from: get("promoted_from"),
        promoted_to: get("promoted_to"),
        visibility: get("visibility"),
        // An unrecognised force reads as `advice` rather than failing the note: the vault is
        // hand-editable markdown, and one typo should not remove a note from the index entirely.
        force: get("force")
            .map(|f| f.trim().to_string())
            .filter(|f| FORCES.contains(&f.as_str()))
            .unwrap_or_else(|| ADVICE.to_string()),
        declined_at: get("declined_at").and_then(|s| parse_ts(&s)),
        declined_after: get("declined_after").and_then(|s| s.trim().parse().ok()),
        derivations: lists
            .iter()
            .find(|(k, _)| k == "derivations")
            .map(|(_, items)| {
                items
                    .iter()
                    .filter_map(|raw| serde_json::from_str(raw).ok())
                    .filter_map(|v: serde_json::Value| Derivation::from_json(&v))
                    .collect()
            })
            .unwrap_or_default(),
        body: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Note {
        Note {
            id: NoteId::observation("amb", "2026-08-27-thing"),
            // Every character class that hand-rolled YAML quoting gets wrong.
            title: "render_all: \"caps\" at ten — #10, 50% of {sixty}".to_string(),
            status: ACTIVE.to_string(),
            created: 1_787_000_000.0,
            session: Some("sess-1".into()),
            agent: Some("amb-builder".into()),
            files: vec!["src/delivery.rs".into(), "src/messages.rs".into()],
            cites: vec!["amb/2026-08-26-earlier".into()],
            supersedes: Some("amb/2026-08-20-older".into()),
            superseded_by: None,
            promoted_from: None,
            promoted_to: None,
            visibility: None,
            force: ADVICE.to_string(),
            declined_at: None,
            declined_after: None,
            derivations: Vec::new(),
            body: "The body.\n\nWith a second paragraph.".to_string(),
        }
    }
    #[test]
    fn a_note_round_trips_through_its_own_frontmatter() {
        let n = sample();
        let text = n.render();
        let back = parse_note(&text, "2026-08-27-thing", 0.0).expect("should parse");
        assert_eq!(back, n);
    }
    #[test]
    fn the_rendered_note_is_readable_markdown_not_an_escaped_blob() {
        let text = sample().render();
        assert!(text.starts_with("---\n"), "{text}");
        assert!(text.contains("\n---\n\n"), "frontmatter is closed: {text}");
        assert!(text.trim_end().ends_with("With a second paragraph."));
        assert!(text.contains("  - \"src/delivery.rs\""), "{text}");
    }
    #[test]
    fn a_note_typed_by_hand_in_obsidian_still_indexes() {
        // No `id:`, no `created:`, no lists — what a human actually writes.
        let text = "---\nscope: amb\ntitle: something I noticed\n---\n\nprose\n";
        let n = parse_note(text, "2026-08-27-by-hand", 1234.0).expect("should parse");
        assert_eq!(
            n.id.slug, "2026-08-27-by-hand",
            "falls back to the filename"
        );
        assert_eq!(n.created, 1234.0, "falls back to the file mtime");
        assert_eq!(n.status, ACTIVE);
        assert_eq!(n.body, "prose");
    }
    #[test]
    fn a_malformed_note_is_none_rather_than_a_panic() {
        // This runs inside a hook. One bad file in a vault must not cost a session its memory.
        for bad in [
            "",
            "no frontmatter at all",
            "---\nunterminated: yes\n",
            "---\n---\n",
            "---\nscope: amb\n---\n", // no title
            "---\ntitle: t\n---\n",   // no project
        ] {
            assert!(parse_note(bad, "slug", 0.0).is_none(), "{bad:?} parsed");
        }
    }
    fn derivation(project: &str, note: &str) -> Derivation {
        Derivation {
            ts: 1_787_000_000.0,
            project: project.to_string(),
            session: "sess-1".to_string(),
            note: note.to_string(),
            topics: vec!["rust".to_string()],
        }
    }
    #[test]
    fn a_candidates_derivation_ledger_round_trips_through_its_own_file() {
        // It lives in the file because `rm board.db` must not destroy the evidence a promotion
        // was offered on — D34 promises the vault is truth, and a promotion ledger only in the
        // index would make that false for the thing it matters most for.
        let mut n = sample();
        n.id = NoteId {
            kind: CANDIDATE.into(),
            scope: UNSCOPED.into(),
            slug: "auth-lock-ordering".into(),
        };
        n.derivations = vec![
            derivation("nestwatch", "noticed while fixing the login race"),
            derivation("amb", "same shape in the claims path"),
            derivation("devt", "third time; it is a pattern"),
        ];
        let text = n.render();
        let back = parse_note(&text, "auth-lock-ordering", 0.0).expect("parses");
        assert_eq!(back.derivations, n.derivations);
        assert_eq!(back, n);
    }
    #[test]
    fn the_file_states_the_count_and_the_spread_a_human_needs_to_judge_it() {
        // "The offer shows the derivations, not just the count" — and the file a human opens has
        // to do the same, or approving means adding up a list by hand.
        let mut n = sample();
        n.derivations = vec![
            derivation("nestwatch", "one"),
            derivation("amb", "two"),
            derivation("nestwatch", "three, same project again"),
        ];
        let text = n.render();
        assert!(text.contains("derived_count: 3"), "{text}");
        assert!(text.contains("derived_in:"), "{text}");
        assert!(text.contains("\"amb\""), "{text}");
        assert!(text.contains("\"nestwatch\""), "{text}");
        assert_eq!(
            text.matches("\"nestwatch\"").count(),
            3,
            "deduped in derived_in, kept in the ledger: {text}"
        );
    }

    /// **An indented line belongs to the key above it, and lifting it makes the warning lie**
    /// (M27).
    ///
    /// `scan_frontmatter` is deliberately the *only* frontmatter scanner, so that `unknown_keys`
    /// can never report a key `parse_note` never saw — its own docstring says a warning that lies
    /// is worse than no warning. Narrowing `||` to `&&` in its skip guard does precisely that: a
    /// nested `path:` under `files:` becomes a top-level scalar and is reported as unread, on a
    /// note whose every real key is known.
    ///
    /// `amb`'s own writer never emits an indented `key: value` — list items go out as `  - x` and
    /// hit the branch above — and all 36 notes in the real vault are clean, so this is unreachable
    /// through `render`. It is reachable through the vault's premise: hand-editable markdown that
    /// a person and Obsidian both write into, which is the case `unknown_keys` exists to serve.
    ///
    /// Asserted at both layers the rule passes through (M20). The scanner being right and the
    /// warning being right are two claims, and the second is the one a reader acts on.
    #[test]
    fn an_indented_line_is_part_of_the_value_above_it_and_never_a_key_of_its_own() {
        let front = "files:\n  path: src/a.rs\nstatus: active\n";

        let (scalars, lists) = scan_frontmatter(front);
        assert!(
            !scalars.iter().any(|(k, _)| k == "path"),
            "a nested key was lifted to the top level: {scalars:?}"
        );
        assert_eq!(
            lists.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            vec!["files"],
            "and the key it was nested under is still the list it opened"
        );

        let dir = tempfile::tempdir().expect("temp dir");
        let proj = dir.path().join("projects").join("nest");
        std::fs::create_dir_all(&proj).expect("mkdir");
        std::fs::write(proj.join("a.md"), format!("---\n{front}---\nbody\n")).expect("write");
        assert_eq!(
            unknown_keys(dir.path()),
            vec![],
            "the warning named a key no reader ever saw"
        );
    }
}

//! A note's identity, and the row shape the index returns.
//!
//! [`NoteId`] is the primary key and the thing an agent echoes back in `--cites`. D50 is why an
//! id names its kind; D81 is why the middle segment is a **scope** rather than a project, so that
//! `decision/@@/x` and `decision/#rust/x` are sayable at all.

use super::*;

/// A note's identity: the primary key, and what an agent echoes back in `--cites`.
///
/// `scope` holds [`crate::address::Scope`]'s stored form — a bare project id, `#topic`, or `@@` —
/// and is empty for a candidate, which has not earned one. See [`UNSCOPED`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NoteId {
    pub kind: String,
    pub scope: String,
    pub slug: String,
}

/// A candidate's scope, which is deliberately nothing.
///
/// **Not a default, an absence.** Where a candidate is eventually filed is the question its
/// derivation ledger exists to answer, and `destination` answers it at promotion time. Writing
/// `@@` or the deriving project here would be an answer nobody has earned yet, and it would be
/// indistinguishable afterwards from one the router actually made.
pub const UNSCOPED: &str = "";

impl NoteId {
    /// A note of any kind, in any scope. **The one constructor that takes the kind as data**, and
    /// therefore the only one `observe` can call — it writes two kinds now, chosen at runtime.
    ///
    /// The named constructors below delegate here rather than repeating the literal. Before this
    /// they did not, so `observe` built a fifth copy inline and `NoteId::capture` was left with
    /// no production caller at all — D84's shape, and invisible to `find_unread_fields.py`,
    /// whose `^pub fn` scan never reaches an indented `impl` method.
    pub fn scoped(kind: &str, scope: &str, slug: &str) -> Self {
        NoteId {
            kind: kind.to_string(),
            scope: scope.to_string(),
            slug: slug.to_string(),
        }
    }

    pub fn observation(project: &str, slug: &str) -> Self {
        Self::scoped(OBSERVATION, project, slug)
    }

    /// A decision at some scope — a project, a topic, or everywhere.
    pub fn decision(scope: &crate::address::Scope, slug: &str) -> Self {
        Self::scoped(DECISION, &scope.as_str(), slug)
    }

    pub fn candidate(slug: &str) -> Self {
        Self::scoped(CANDIDATE, UNSCOPED, slug)
    }

    /// A machine-written failure capture, scoped to the project it happened in (D86).
    pub fn capture(project: &str, slug: &str) -> Self {
        Self::scoped(CAPTURE, project, slug)
    }

    /// The scope this note applies at, or `None` for a candidate.
    pub fn scope(&self) -> Option<crate::address::Scope> {
        crate::address::parse_scope(&self.scope).ok()
    }

    /// What is rendered, and what `--cites`, `--same-as` and `promote` accept.
    ///
    /// **Always qualified, never context-dependent.** An id that shortens when the note is local
    /// would give the same note two spellings, and the ledger counts on one.
    ///
    /// **The shape depends on the kind, because not every kind has a scope.** A candidate carries
    /// [`UNSCOPED`]; formatting it the observation way produced ids beginning with a bare `/`,
    /// which then failed to parse back. Found by round-tripping a candidate rather than by review
    /// (D50).
    ///
    /// | kind | id |
    /// |---|---|
    /// | observation | `agent-messageboard/2026-08-28-a-thing` |
    /// | candidate | `candidate/auth-lock-ordering` |
    /// | decision, project | `decision/agent-messageboard/auth-lock-ordering` |
    /// | decision, topic | `decision/#rust/take-locks-in-declaration-order` |
    /// | decision, global | `decision/@@/name-things-for-what-they-do` |
    /// | capture | `capture/agent-messageboard/2026-08-28-bash-failed` |
    ///
    /// **The last two are what D81 bought.** A pattern used to be its own kind, which is what
    /// made "a decision about Rust" unsayable: the kind was carrying the scope, and it only had
    /// room for two.
    pub fn display(&self) -> String {
        match self.kind.as_str() {
            // The bare form: an observation's id is scope and slug, with the kind implied.
            OBSERVATION => format!("{}/{}", self.scope, self.slug),
            // **The one scopeless kind, named rather than left to the catch-all.** A candidate
            // carries `UNSCOPED`, so naming its scope would render `candidate//slug`.
            CANDIDATE => format!("{CANDIDATE}/{}", self.slug),
            // **Everything else is scoped, and the default is deliberately the scoped shape.**
            // This arm used to be candidate's, so any kind added without touching this function
            // silently dropped its scope and round-tripped into the wrong `NoteId` — D50's bug
            // re-armed for the next kind rather than fixed. Defaulting the other way makes the
            // failure loud: a scopeless kind added without a arm here renders a doubled slash.
            k => format!("{k}/{}/{}", self.scope, self.slug),
        }
    }
}

/// Read an id back into its parts, or `None` for a bare slug the index must disambiguate.
///
/// **A project literally named `candidate` or `decision` is resolved in favour of the kind**,
/// which is the only ambiguity left in the scheme. It fails visibly — the lookup returns "no such
/// note" rather than the wrong one — and `resolve` falls back to a slug search, so the note is
/// still reachable. The scope sigils cannot collide the same way, because `parse_scope` refuses a
/// project id that reads as one.
pub fn parse_id(input: &str) -> Option<NoteId> {
    let parts: Vec<&str> = input.trim().split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        // **Driven by `KINDS`, so a new scoped kind parses without editing this.** The two
        // exclusions are the two shapes that are not three-part: an observation is `scope/slug`
        // and a candidate is `candidate/slug`. Exactly the inverse of `display`.
        [k, scope, slug] if KINDS.contains(k) && *k != OBSERVATION && *k != CANDIDATE => {
            Some(NoteId::scoped(k, scope, slug))
        }
        [k, slug] if *k == CANDIDATE => Some(NoteId::candidate(slug)),
        [scope, slug] => Some(NoteId::observation(scope, slug)),
        _ => None,
    }
}

/// Split a user-supplied id into an optional project and a slug.
///
/// A bare slug is accepted and resolved against the index, which errors on ambiguity rather than
/// picking. Exact lookup with an explicit failure, never a fuzzy match — a wrong merge is
/// invisible where a duplicate is not.
pub fn split_id(s: &str) -> (Option<String>, String) {
    match s.trim().rsplit_once('/') {
        Some((proj, slug)) if !proj.is_empty() && !slug.is_empty() => {
            (Some(proj.to_string()), slug.to_string())
        }
        _ => (None, s.trim().to_string()),
    }
}

/// A note as the index knows it. Content lives in the file; this is enough to find and judge it.
#[derive(Debug, Clone)]
pub struct IndexedNote {
    pub id: NoteId,
    pub title: String,
    pub status: String,
    pub created: f64,
    pub vault_path: String,
    pub excerpt: Option<String>,
    pub paths: Vec<String>,
    /// `advice`, `decision` or `rule` — the tiebreak under the injection cap (D64).
    pub force: String,
}

impl IndexedNote {
    pub fn to_json(&self, at: f64) -> serde_json::Value {
        serde_json::json!({
            "id": self.id.display(),
            "kind": self.id.kind,
            "scope": self.id.scope,
            "slug": self.id.slug,
            "title": self.title,
            "status": self.status,
            "created": self.created,
            "age": age(self.created, at),
            "vault_path": self.vault_path,
            "files": self.paths,
            "excerpt": self.excerpt,
            "force": self.force,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three-part arm's guard, row by row (M55): the two excluded kinds fall through to
    /// `None` — an observation is `scope/slug` and a candidate `candidate/slug`, so their
    /// three-part spellings are refusals, not parses — and an unknown kind refuses too. Each
    /// `&&` flip and the guard-to-`true` mutant admits exactly one of these rows.
    #[test]
    fn a_three_part_id_parses_only_for_the_kinds_that_have_three_parts() {
        assert!(
            parse_id("decision/nest/x").is_some(),
            "the reach row: a real scoped kind parses"
        );
        assert!(parse_id("observation/nest/x").is_none());
        assert!(parse_id("candidate/nest/x").is_none());
        assert!(parse_id("nonsense/nest/x").is_none());
    }

    /// `split_id`'s emptiness guard: a leading or trailing slash is not a qualifier, and the
    /// `&&` flips would each manufacture one from half an id.
    #[test]
    fn an_empty_half_around_a_slash_is_no_qualifier_at_all() {
        assert_eq!(split_id("a/b"), (Some("a".into()), "b".into()));
        assert_eq!(split_id("/x"), (None, "/x".into()));
        assert_eq!(split_id("x/"), (None, "x/".into()));
    }

    #[test]
    fn an_id_is_always_qualified_and_splits_back() {
        let id = NoteId::observation("nest", "2026-08-27-thing");
        assert_eq!(id.display(), "nest/2026-08-27-thing");
        assert_eq!(
            split_id("nest/2026-08-27-thing"),
            (Some("nest".into()), "2026-08-27-thing".into())
        );
        assert_eq!(
            split_id("2026-08-27-thing"),
            (None, "2026-08-27-thing".into())
        );
    }
    /// D50's guard, extended to D81's axis before the axis moved anything — which is the
    /// sequencing the plan for this refactor asked for by name.
    #[test]
    fn every_kind_of_id_round_trips_including_the_ones_with_no_project() {
        use crate::address::Scope;
        // The bug this exists for: a candidate carries scope = "", so formatting it the
        // observation way produced "/auth-lock-ordering" — a leading slash that would not parse
        // back. Caught by round-tripping a candidate, not by reading the code (D50).
        for id in [
            NoteId::observation("agent-messageboard", "2026-08-28-a-thing"),
            NoteId::candidate("auth-lock"),
            NoteId::decision(&Scope::Project("amb".into()), "auth-lock"),
            // The two D81 added. A pattern used to be a kind; it is a decision at global scope.
            NoteId::decision(&Scope::Global, "lock-order"),
            NoteId::decision(&Scope::Topic("rust".into()), "lock-order"),
            // D86's kind. It is scoped like an observation but named like a decision, which is
            // the combination the scopeless fallback silently got wrong.
            NoteId::capture("agent-messageboard", "2026-08-28-bash-failed"),
        ] {
            let shown = id.display();
            assert!(
                !shown.starts_with('/'),
                "id must not begin with a slash: {shown:?}"
            );
            assert!(
                !shown.contains("//"),
                "id must not contain an empty segment: {shown:?}"
            );
            assert_eq!(
                parse_id(&shown),
                Some(id.clone()),
                "round trip failed for {shown:?}"
            );
        }

        // **The list above is hand-written, so it cannot notice a *new* kind** — and a new kind
        // is the drift that matters, because `display`'s catch-all will happily render one it has
        // never been told about. Driving off `KINDS` is what makes forgetting impossible, the
        // same way `a_note_of_every_kind_is_seen_by_the_vault_walk` drives off `vault_dir`.
        for kind in KINDS {
            let scope = if *kind == CANDIDATE { UNSCOPED } else { "nest" };
            let id = NoteId::scoped(kind, scope, "2026-08-29-a-thing");
            let shown = id.display();
            assert!(
                !shown.starts_with('/') && !shown.contains("//"),
                "{kind} renders a malformed id: {shown:?}"
            );
            assert_eq!(
                parse_id(&shown),
                Some(id),
                "{kind} does not round trip: {shown:?}"
            );
        }
    }

    #[test]
    fn an_id_names_its_kind_whenever_the_kind_is_not_the_default() {
        use crate::address::Scope;
        assert_eq!(NoteId::candidate("x").display(), "candidate/x");
        assert_eq!(
            NoteId::decision(&Scope::Project("amb".into()), "x").display(),
            "decision/amb/x"
        );
        assert_eq!(
            NoteId::decision(&Scope::Global, "x").display(),
            "decision/@@/x"
        );
        assert_eq!(
            NoteId::decision(&Scope::Topic("rust".into()), "x").display(),
            "decision/#rust/x"
        );
        // An observation keeps the shape Phase 1 shipped, so no id already written changes.
        assert_eq!(NoteId::observation("amb", "x").display(), "amb/x");
    }

    /// The scope is readable back off the id, and a candidate's absence is an absence rather
    /// than a scope that happens to be empty.
    #[test]
    fn a_candidate_has_no_scope_and_everything_else_does() {
        use crate::address::Scope;
        assert_eq!(NoteId::candidate("x").scope(), None);
        assert_eq!(
            NoteId::observation("amb", "x").scope(),
            Some(Scope::Project("amb".into()))
        );
        assert_eq!(
            NoteId::decision(&Scope::Global, "x").scope(),
            Some(Scope::Global)
        );
        assert_eq!(
            NoteId::decision(&Scope::Topic("rust".into()), "x").scope(),
            Some(Scope::Topic("rust".into()))
        );
    }
}

//! Address parsing.
//!
//! Pure: no I/O, no database, no environment. That is deliberate — this is the "decide what to
//! do" half of the functional-core/imperative-shell split, so the whole addressing model can be
//! tested exhaustively without a filesystem in sight.
//!
//! The four forms come from `DESIGN.md` and match the `repo#ID` convention the sibling repos
//! already use for cross-repo citation, so there is one addressing idea to learn rather than two.

use crate::error::{Error, Result};

/// A parsed destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// `alice` or `alice@nestwatch` — one named agent, in my project or a named one.
    Agent {
        name: String,
        project: Option<String>,
    },
    /// `@` or `@nestwatch` — everyone in a project. Addresses a *place*, not a process: the
    /// message waits for whoever works there, whenever they arrive.
    Broadcast { project: Option<String> },
    /// `@@` — everyone in every registered project.
    ///
    /// Spelled `@@` rather than `@*` because `@*` is a glob: unquoted it fails in zsh with
    /// "no matches found", and zsh is the shell agents shell out through. Verified 2026-08-27.
    Everyone,
}

impl Address {
    /// The project this address scopes to: `Some` for a named or implicit project, `None` for
    /// `@@`, which becomes `to_proj IS NULL` and so reaches every project.
    pub fn project<'a>(&'a self, mine: &'a str) -> Option<&'a str> {
        match self {
            Address::Agent { project, .. } | Address::Broadcast { project } => {
                Some(project.as_deref().unwrap_or(mine))
            }
            Address::Everyone => None,
        }
    }

    /// The agent *name* to resolve, or `None` for a broadcast.
    ///
    /// A name, not an id: resolution against the roster happens in
    /// [`crate::messages::resolve_recipient`], because it needs the database.
    pub fn name(&self) -> Option<&str> {
        match self {
            Address::Agent { name, .. } => Some(name),
            Address::Broadcast { .. } | Address::Everyone => None,
        }
    }
}

/// **A place something applies to**, which is the addressing question minus the transport.
///
/// The bus asks *who should receive this*; memory asks *where does this apply*. Those are the same
/// question about a place, and only the second one has a third answer — so `Scope` is [`Address`]'s
/// two place-forms plus the topic, and lives here rather than in `memory` so there is one grammar
/// and one parser rather than two that drift (D81).
///
/// | Written | Means |
/// |---|---|
/// | `@@` | everywhere — a principle that belongs to no repository |
/// | `#rust` | a topic — true of Rust wherever Rust is |
/// | `nest` | one project |
///
/// `#` is the topic sigil for a reason beyond symmetry: it is Obsidian's tag character, so a
/// `#rust` written in a note's prose is simultaneously the scope and a working tag in the surface
/// the vault is read through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// `@@` — everywhere.
    Global,
    /// `#name` — a topic, which is not a place any session stands in.
    Topic(String),
    /// A bare project id.
    Project(String),
}

impl Scope {
    /// The stored form, which is also the written form. One string, so the column is greppable
    /// and the injection filter is a single `IN` list rather than a branch per scope.
    pub fn as_str(&self) -> String {
        match self {
            Scope::Global => GLOBAL.to_string(),
            Scope::Topic(t) => format!("{TOPIC_SIGIL}{t}"),
            Scope::Project(p) => p.clone(),
        }
    }

    /// The project this scope names, if it names one.
    pub fn project(&self) -> Option<&str> {
        match self {
            Scope::Project(p) => Some(p),
            Scope::Global | Scope::Topic(_) => None,
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

/// `@@`, spelled once so the parser and the writer cannot disagree.
pub const GLOBAL: &str = "@@";
/// The topic sigil.
pub const TOPIC_SIGIL: char = '#';

/// Parse a scope.
///
/// **A project id that would parse as something else is rejected, not silently reinterpreted.**
/// `AMB_PROJECT` is taken from the environment verbatim, so `AMB_PROJECT='@@'` is reachable, and
/// under one stored column it would file that session's notes as universal principles. D50
/// recorded three ids that "only degrade gracefully"; the sigils are the ones that do not, so
/// they are a clean refusal.
pub fn parse_scope(input: &str) -> Result<Scope> {
    let bad = |reason: &str| Error::BadAddress {
        input: input.to_string(),
        reason: reason.into(),
    };
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(bad("a scope cannot be empty"));
    }
    if trimmed == GLOBAL {
        return Ok(Scope::Global);
    }
    if let Some(topic) = trimmed.strip_prefix(TOPIC_SIGIL) {
        if topic.is_empty() {
            return Err(bad("'#' with no topic after it"));
        }
        if topic.contains(TOPIC_SIGIL) || topic.contains('@') || topic.contains('/') {
            return Err(bad("a topic name cannot contain '#', '@' or '/'"));
        }
        return Ok(Scope::Topic(topic.to_string()));
    }
    if trimmed.starts_with('@') {
        return Err(bad(
            "a project scope is written bare — 'nest', not '@nest'; '@@' is the global scope",
        ));
    }
    if trimmed.contains('/') {
        return Err(bad("a project id cannot contain '/'"));
    }
    Ok(Scope::Project(trimmed.to_string()))
}

/// Parse an address.
///
/// Rejects rather than guesses: an empty agent name before `@`, or more than one `@`, is a typo
/// worth reporting, not something to interpret charitably. A silently misrouted message is worse
/// than a rejected one, because nothing tells the sender.
pub fn parse(input: &str) -> Result<Address> {
    let bad = |reason: &str| Error::BadAddress {
        input: input.to_string(),
        reason: reason.into(),
    };

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(bad("it is empty"));
    }
    // Checked before the multiple-'@' guard below, which would otherwise reject it.
    if trimmed == GLOBAL {
        return Ok(Address::Everyone);
    }
    // **The vocabulary is shared; the transport is not.** A topic is a real scope and never a
    // destination — nobody stands in `#rust`, so there is no inbox to deliver to. Refused here,
    // by name, rather than falling through to "bad address": the whole reason `Scope` lives in
    // this module is that someone who learns one grammar has learned both, and that promise is
    // only kept if the error says which half they reached for.
    if trimmed.starts_with(TOPIC_SIGIL) {
        return Err(bad(
            "a topic is a memory scope, not a destination — nobody is in '#rust' to receive it. \
             Use '@@' for everyone, or '@project' for a place",
        ));
    }
    if trimmed.matches('@').count() > 1 {
        return Err(bad("it contains more than one '@'"));
    }

    match trimmed.split_once('@') {
        // "@" or "@project"
        Some(("", project)) => Ok(Address::Broadcast {
            project: non_empty(project),
        }),
        // "alice@project"
        Some((name, project)) => {
            let project = non_empty(project)
                .ok_or_else(|| bad("it has a name and an '@' but no project after it"))?;
            Ok(Address::Agent {
                name: name.to_string(),
                project: Some(project),
            })
        }
        // "alice"
        None => Ok(Address::Agent {
            name: trimmed.to_string(),
            project: None,
        }),
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_is_an_agent_in_my_project() {
        let a = parse("alice").expect("should parse");
        assert_eq!(
            a,
            Address::Agent {
                name: "alice".into(),
                project: None
            }
        );
        assert_eq!(a.project("mine"), Some("mine"));
        assert_eq!(a.name(), Some("alice"));
    }

    #[test]
    fn qualified_name_targets_another_project() {
        let a = parse("alice@nestwatch").expect("should parse");
        assert_eq!(a.project("mine"), Some("nestwatch"));
        assert_eq!(a.name(), Some("alice"));
    }

    #[test]
    fn bare_at_broadcasts_to_my_project() {
        let a = parse("@").expect("should parse");
        assert_eq!(a, Address::Broadcast { project: None });
        assert_eq!(a.project("mine"), Some("mine"));
        // None is what becomes `to_agent IS NULL`.
        assert_eq!(a.name(), None);
    }

    #[test]
    fn at_project_broadcasts_to_that_project() {
        let a = parse("@nestwatch").expect("should parse");
        assert_eq!(a.project("mine"), Some("nestwatch"));
        assert_eq!(a.name(), None);
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(
            parse("  alice@proj  ").expect("should parse").project("m"),
            Some("proj")
        );
    }

    #[test]
    fn empty_is_rejected() {
        assert!(matches!(parse("   "), Err(Error::BadAddress { .. })));
    }

    #[test]
    fn a_name_with_a_trailing_at_is_rejected_not_guessed() {
        // "alice@" could plausibly mean "alice in my project", but guessing here misroutes
        // silently. Reject it.
        assert!(matches!(parse("alice@"), Err(Error::BadAddress { .. })));
    }

    #[test]
    fn two_ats_are_rejected() {
        assert!(matches!(parse("a@b@c"), Err(Error::BadAddress { .. })));
    }

    #[test]
    fn double_at_reaches_every_project() {
        let a = parse("@@").expect("should parse");
        assert_eq!(a, Address::Everyone);
        // None on both axes: `to_agent IS NULL AND to_proj IS NULL`.
        assert_eq!(a.name(), None);
        assert_eq!(
            a.project("mine"),
            None,
            "@@ must not be scoped to the sender's project"
        );
    }

    #[test]
    fn double_at_is_not_confused_with_a_qualified_name() {
        assert!(matches!(parse("a@@b"), Err(Error::BadAddress { .. })));
        assert!(matches!(parse("@@@"), Err(Error::BadAddress { .. })));
    }

    #[test]
    fn a_scope_is_one_of_three_places() {
        assert_eq!(parse_scope("@@").expect("global"), Scope::Global);
        assert_eq!(
            parse_scope("#rust").expect("topic"),
            Scope::Topic("rust".into())
        );
        assert_eq!(
            parse_scope("nest").expect("project"),
            Scope::Project("nest".into())
        );
    }

    #[test]
    fn a_scope_round_trips_through_the_form_it_is_stored_in() {
        for written in ["@@", "#rust", "nest"] {
            let s = parse_scope(written).expect("parses");
            assert_eq!(s.as_str(), written, "{written} did not round-trip");
            assert_eq!(parse_scope(&s.as_str()).expect("re-parses"), s);
        }
    }

    #[test]
    fn only_a_project_scope_names_a_project() {
        assert_eq!(parse_scope("nest").expect("p").project(), Some("nest"));
        assert_eq!(parse_scope("@@").expect("g").project(), None);
        assert_eq!(parse_scope("#rust").expect("t").project(), None);
    }

    /// The reason a single stored column is safe. `AMB_PROJECT` is read verbatim, so a project
    /// literally called `@@` is reachable from the environment — and under one column it would
    /// file that session's notes as universal principles with nothing said.
    #[test]
    fn a_project_id_that_would_read_as_another_scope_is_refused() {
        for hostile in ["@@", "#rust", "@nest", "a/b", "#", ""] {
            let as_project = parse_scope(hostile)
                .ok()
                .and_then(|s| s.project().map(str::to_string));
            assert_ne!(
                as_project.as_deref(),
                Some(hostile),
                "{hostile:?} was accepted as a project id"
            );
        }
    }

    /// D81: shared vocabulary, refused transport. The error has to name the half that was reached
    /// for, or "one addressing idea to learn" is a claim the tool does not keep.
    #[test]
    fn a_topic_parses_as_a_scope_and_is_refused_as_a_destination() {
        assert!(parse_scope("#rust").is_ok());
        let err = parse("#rust").expect_err("a topic is not a destination");
        let said = err.to_string();
        assert!(said.contains("memory scope"), "{said}");
        assert!(said.contains("not a destination"), "{said}");
    }
}

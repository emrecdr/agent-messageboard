//! Constants, thresholds and the environment they read.
//!
//! Every knob the layer has, in one place, so a reader can see the whole
//! configuration surface without grepping. `AMB_VAULT` has no default: unset
//! means memory is off (D35).

use super::*;

/// The only kind of note Phase 1 writes. `candidate`, `decision` and `pattern` arrive with
/// Phases 2 and 3, which are blocked on `OPEN-QUESTIONS.md` Q10.
pub const OBSERVATION: &str = "observation";

/// The two kinds Phase 2 and Phase 3 add (D49).
///
/// **`kind` says what a note *is* and nothing else** (D81). It used to say where the note applied
/// as well — `pattern` meant global and `decision` meant project-scoped, with the type left
/// implied — and that encoding survives exactly two scopes. `pattern` is gone: a pattern was
/// always a decision at global scope, and now it is spelled that way. Where a note applies is
/// [`crate::address::Scope`], on its own column.
pub const CANDIDATE: &str = "candidate";
pub const DECISION: &str = "decision";

/// Machine-written scrollback from a failed tool call (D86).
///
/// **A kind, not a flag, because [`INJECTABLE`] is what enforces the exclusion** — D51's whole
/// finding was that a guard named in a comment while an unrelated filter did the work is correct
/// by accident. `candidate` is already excluded that way; a capture is excluded by the same
/// machinery rather than by a second one.
///
/// It is written by `PostToolUseFailure` with no model involved, so its body is raw tool output
/// and its title is `"<Tool> failed"`. That is worth keeping and worth searching — `amb memory
/// recall` reads every kind — and is not worth a slot in a session's context, which is the
/// distinction the kind draws.
pub const CAPTURE: &str = "capture";

/// Every kind, in the order a reader should meet them. The vault walk and the migration both need
/// this list, and a new kind that is added to one and not the other is the drift
/// `a_note_of_every_kind_is_seen_by_the_vault_walk` exists to catch.
pub const KINDS: &[&str] = &[OBSERVATION, CANDIDATE, DECISION, CAPTURE];

/// Kinds that may be put in front of an agent.
///
/// **`candidate` is deliberately absent and that is load-bearing** (D49). A candidate that could
/// be injected could make the case for its own promotion, and the counting rule would be measuring
/// its own echo. Guarded by `a_candidate_is_never_injected`.
///
/// D51 is why this is one constant built into SQL by one function rather than written out per
/// query: the two had already diverged in effect, and the guard that was named was not the guard
/// doing the work.
pub const INJECTABLE: &[&str] = &[OBSERVATION, DECISION];

/// The kinds that may never be put in front of an agent, named rather than left as a remainder.
///
/// **This exists so the partition is data instead of arithmetic.** The guard used to read
/// `KINDS.len() == INJECTABLE.len() + 1`, and that `1` is a magic number somebody has to
/// remember to bump — a new kind could be added, the assert adjusted to `+ 2`, and nothing would
/// have recorded *why* the new kind was excluded. Two lists that must partition `KINDS` force the
/// decision to be written down in the place the SQL is built from.
pub const NON_INJECTABLE: &[&str] = &[CANDIDATE, CAPTURE];

/// Kinds `amb memory recall` searches — a second axis, and it has to be one.
///
/// **Not injectable and not findable are different answers, and conflating them is how a note
/// becomes invisible rather than merely quiet.** `search` used to hardcode `kind = 'observation'`,
/// so a decision was already unfindable by recall and nobody had decided that; adding a kind that
/// is excluded from injection *by design* would have inherited the same silence and buried it
/// under a rule that sounds deliberate. A capture is written to be read later or it should not be
/// written at all (D86).
///
/// `candidate` is the exclusion: `amb memory promote --list` is its surface, and it reaches a
/// person through the gate D49 built rather than through a general query.
pub const SEARCHABLE: &[&str] = &[OBSERVATION, DECISION, CAPTURE];

/// How binding a note is, which is **not** the same axis as how far it has got.
///
/// Lifecycle answers *has this earned its place*; force answers *how much weight it should carry*,
/// and the two come apart constantly — something rediscovered three times may still be a
/// suggestion, and something recognised once may be non-negotiable. Folding force into lifecycle
/// would mean the only way to make a note binding is to make it old.
///
/// **Each level ships with a mechanical consequence or it does not ship.** The consequence today is
/// injection priority under budget, and that consumer is live rather than anticipated: the cap is
/// [`MAX_INJECTED`] and the vault already exceeds it, so notes are dropped every session with
/// recency as the only reason one survives.
///
/// **Force never denies anything.** A rule is expected and a miss is *reported*; it is never
/// refused. That line was drawn when the blocking mechanism was declined (D52) and it holds here:
/// the moment `amb` starts blocking it becomes a governance tool competing with a better one.
pub const ADVICE: &str = "advice";
pub const FORCE_DECISION: &str = "decision";
pub const RULE: &str = "rule";

/// Ranking weight, lowest first. Only ever consulted as a tiebreak *within* a scope.
pub fn force_rank(force: &str) -> u8 {
    match force {
        RULE => 0,
        FORCE_DECISION => 1,
        _ => 2,
    }
}

/// The levels, strongest first, for anything that needs to enumerate them.
pub const FORCES: &[&str] = &[RULE, FORCE_DECISION, ADVICE];

/// The same ranking as [`force_rank`], as a SQL `CASE`, **generated from [`FORCES`] rather than
/// written out**.
///
/// It has to exist in SQL at all because the selection is where the decision is made: the
/// injection query carries a `LIMIT`, so a note excluded there never reaches the Rust sort. The
/// first version of this put force only in `order_and_cap` and a `rule` that was the oldest of
/// thirteen notes was still dropped — the ordering was correct and applied to an already-truncated
/// list. Two rankings for one concept is exactly the drift this project keeps paying for, so the
/// second one is derived from the first.
pub fn force_order_sql(col: &str) -> String {
    let arms: Vec<String> = FORCES
        .iter()
        .enumerate()
        .map(|(i, f)| format!("WHEN '{f}' THEN {i}"))
        .collect();
    format!("CASE {col} {} ELSE {} END", arms.join(" "), FORCES.len())
}

pub const ACTIVE: &str = "active";
pub const SUPERSEDED: &str = "superseded";
/// A candidate that was promoted. Archived, never deleted — the evidence outlives the offer.
pub const PROMOTED: &str = "promoted";
/// A candidate the user declined. Not re-offered until it derives again.
pub const DECLINED: &str = "declined";
/// A candidate that went 30 days without re-derivation. Unpromoted is not permanent.
pub const EXPIRED: &str = "expired";

/// Independent derivations before a candidate is *offered*. Never before it is promoted — the
/// threshold produces an offer, and only a person produces a promotion (D49).
pub const PROMOTION_THRESHOLD: usize = 3;

/// The threshold in force, which `AMB_MEMORY_THRESHOLD` may override.
///
/// **The plan said "(config, default 3)" and this shipped as a bare constant.** Three is a guess —
/// the plan says so — and a guess that cannot be changed without a rebuild is not a threshold, it
/// is a decision pretending to be a parameter.
pub fn threshold() -> usize {
    threshold_from(std::env::var("AMB_MEMORY_THRESHOLD").ok())
}

/// The env shell's decision, injected — M51's seam pattern: the parse, the zero-refusal and the
/// default were all mutable while only the shell existed, because a test cannot set process env
/// without racing the parallel runner.
fn threshold_from(raw: Option<String>) -> usize {
    raw.and_then(|v| v.trim().parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(PROMOTION_THRESHOLD)
}

/// A candidate stops being offered after this long without a new derivation.
pub const CANDIDATE_TTL_DAYS: f64 = 30.0;

/// Whether the promotion pipeline is switched on. **The kill switch D49 names as the response to
/// approval degrading into a rubber stamp** — not a tuning knob.
pub fn promotion_enabled() -> bool {
    !matches!(
        std::env::var("AMB_MEMORY_PROMOTION").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The most notes one injection will spell out in full.
///
/// **Lower than mail's ten (D24), on purpose.** Mail is addressed to you and waiting; memory is
/// speculative, and an uncited injection is a permanent tax on every session. The number is a
/// starting guess and the citation ledger is what will correct it — see [`receipt`].
pub const MAX_INJECTED: usize = 8;

/// Tools whose use is never worth a memory lookup.
///
/// Borrowed from claude-mem's `CLAUDE_MEM_SKIP_TOOLS`, whose corpus shows what omitting it costs:
/// **197 observations per session**, most of them bookkeeping. Nothing worth remembering happens
/// in a `TodoWrite`.
///
/// **This is a live guard, not decoration.** `amb install --memory` writes a `PreToolUse` matcher
/// that already narrows to file tools — but hooks live in a file the user can edit, and a matcher
/// that is absent fires on *every* tool call. This is what makes that misconfiguration harmless
/// rather than a hook that runs a hundred times a turn.
pub const SKIP_TOOLS: &[&str] = &[
    "TodoWrite",
    "Skill",
    "AskUserQuestion",
    "Task",
    "Agent",
    "BashOutput",
    "KillShell",
    "ExitPlanMode",
];

/// Above this many files in one project directory, `SessionStart` stops rebuilding the index
/// automatically and [`status`] reports the drift instead.
///
/// The auto-rebuild exists because `DESIGN.md` and the board's own README call the database
/// disposable, so a user *will* delete it — and memory that silently stops working afterwards is
/// this project's documented worst failure shape. The bound exists because that rebuild runs
/// inside a hook with a five-second budget.
pub const AUTO_INDEX_LIMIT: usize = 500;

/// The vault directory, or `None` when memory is switched off.
///
/// No default (D35). A vault is somewhere a human points Obsidian at; guessing `~/vault` would
/// create a directory nobody asked for, and every other value this layer needs — the cap, the
/// skip list — has a defensible default in code precisely because it is not a path.
pub fn vault_path() -> Option<PathBuf> {
    let raw = std::env::var("AMB_VAULT").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // `~` is expanded here because this is read from an environment variable, where a shell may
    // never have had the chance to.
    if let Some(rest) = raw.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return Some(PathBuf::from(home).join(rest));
    }
    Some(PathBuf::from(raw))
}

/// The vault, or the error that names the variable to set.
pub fn require_vault() -> Result<PathBuf> {
    vault_path().ok_or(Error::NoVault)
}

/// The skip list, overridable for a session that wants a different one.
pub fn skip_tools() -> Vec<String> {
    match std::env::var("AMB_MEMORY_SKIP_TOOLS") {
        Ok(s) => parse_skip_list(&s),
        Err(_) => SKIP_TOOLS.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Split and tidy one comma-separated skip list — the emptiness filter lived behind env and its
/// `!` could vanish silently (M55).
fn parse_skip_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a tool call is one memory ignores.
pub fn should_skip(tool: &str, skip: &[String]) -> bool {
    tool.is_empty() || skip.iter().any(|s| s == tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D64's ordering as arithmetic: rule outranks decision outranks everything else, by the
    /// exact weights the ORDER BY consumes. Deleting either named arm or constant-replacing the
    /// function collapses a distinction the injection cap then silently reorders (M55).
    #[test]
    fn the_force_ranks_are_distinct_and_ordered_rule_first() {
        assert_eq!(force_rank(RULE), 0);
        assert_eq!(force_rank(FORCE_DECISION), 1);
        assert_eq!(force_rank(ADVICE), 2);
        assert_eq!(force_rank("anything-else"), 2);
    }

    /// The threshold seam, row by row: absent means the default, a real number wins, zero and
    /// garbage are refused back to the default. The zero row is the `> 0` guard (M55).
    #[test]
    fn a_threshold_comes_from_env_except_when_env_is_unusable() {
        assert_eq!(threshold_from(None), PROMOTION_THRESHOLD);
        assert_eq!(threshold_from(Some(" 5 ".into())), 5);
        assert_eq!(threshold_from(Some("0".into())), PROMOTION_THRESHOLD);
        assert_eq!(threshold_from(Some("junk".into())), PROMOTION_THRESHOLD);
    }

    /// The skip list drops empties rather than keeping only them (the deleted `!`, M55).
    #[test]
    fn a_skip_list_is_split_trimmed_and_never_carries_an_empty_entry() {
        assert_eq!(parse_skip_list("a, b ,,c,"), vec!["a", "b", "c"]);
        assert!(parse_skip_list(",, ,").is_empty());
    }

    #[test]
    fn noisy_tools_are_skipped_and_file_tools_are_not() {
        let skip: Vec<String> = SKIP_TOOLS.iter().map(|s| (*s).to_string()).collect();
        for noisy in ["TodoWrite", "Skill", "AskUserQuestion"] {
            assert!(should_skip(noisy, &skip), "{noisy}");
        }
        for real in ["Read", "Edit", "Write", "NotebookEdit"] {
            assert!(!should_skip(real, &skip), "{real}");
        }
        assert!(
            should_skip("", &skip),
            "a missing tool name is not a lookup"
        );
    }
    #[test]
    fn candidates_are_not_in_the_injectable_set() {
        // The anti-circularity rule, as data rather than as a comment. A candidate that could be
        // injected could argue for its own promotion (D49).
        assert!(INJECTABLE.contains(&OBSERVATION));
        assert!(INJECTABLE.contains(&DECISION));
        assert!(
            !INJECTABLE.contains(&CANDIDATE),
            "a candidate must never be shown"
        );
        // A capture is machine-written scrollback: nothing decided it was worth reading, so
        // nothing should spend a session's context on it (D86).
        assert!(
            !INJECTABLE.contains(&CAPTURE),
            "a capture must never be shown"
        );
        // **Every kind is in exactly one of the two lists** — no third state, and no kind that is
        // in neither because someone added it to `KINDS` and forgot to decide. This used to be
        // `KINDS.len() == INJECTABLE.len() + 1`, which a new kind passes by bumping the literal
        // to `2` without recording why: the arithmetic could be repaired without the decision
        // ever being made. Asserting the partition means the only way to satisfy it is to name
        // the new kind in one list or the other.
        for k in KINDS {
            assert_ne!(
                INJECTABLE.contains(k),
                NON_INJECTABLE.contains(k),
                "{k} must be in exactly one of INJECTABLE {INJECTABLE:?} and NON_INJECTABLE \
                 {NON_INJECTABLE:?} — a kind in both is a contradiction, a kind in neither is a \
                 decision nobody made"
            );
        }
        assert_eq!(
            KINDS.len(),
            INJECTABLE.len() + NON_INJECTABLE.len(),
            "a kind was listed twice, or one of the two lists names something absent from \
             {KINDS:?}"
        );
        // The second axis, asserted the same way. A kind that is in neither list is unfindable by
        // `recall` and nothing said so — which is how `decision` came to be unsearchable without
        // a decision (D86).
        // **The direction, pinned separately.** The loop below asserts every kind is accounted
        // for, but on its own it passes whether or not `CANDIDATE` is in `SEARCHABLE` — the
        // escape clause makes the exclusion it documents unguarded, which is D51's defect in the
        // guard written to prevent D51's defect. Delete this line and adding `CANDIDATE` to
        // `SEARCHABLE` goes green while `recall` starts returning candidates through a general
        // query, around D49's gate.
        assert!(
            !SEARCHABLE.contains(&CANDIDATE),
            "a candidate reaches a person through `promote --list`, not a general search"
        );
        for k in KINDS {
            assert!(
                SEARCHABLE.contains(k) || *k == CANDIDATE,
                "{k} is in no search surface at all: either add it to SEARCHABLE {SEARCHABLE:?} \
                 or say why it is reachable only through the promotion gate, as candidate is"
            );
        }
    }
}

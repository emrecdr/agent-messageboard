//! What differs between one agent CLI and another, held as data rather than as code.
//!
//! **The measurement that chose this shape.** Before this module, `amb`'s vendor-specific surface
//! was 16 lines of Claude-named production code across seven files plus about twenty sites
//! assuming Claude's settings shape — and every one of them differed in a *value*, never in an
//! algorithm. A `trait Vendor` with one implementation per CLI is the field's usual answer
//! (`agmsg` embeds its vendor constraints in conditional script logic and states it has no
//! declarative capability matrix; `hcom` hardcodes a hook set per vendor in a router), and it is
//! why neither can gain a vendor without a release. Dynamic dispatch buys nothing when the
//! variation is six fields, so this is a table.
//!
//! **What the field converged on, which is what makes a table sufficient.** Claude Code and
//! Gemini CLI now share a hook contract almost exactly: the same `hooks → event → [{matcher,
//! hooks:[{type,command}]}]` nesting, the same stdin field names (`session_id`,
//! `transcript_path`, `cwd`, `hook_event_name`), and the same
//! `hookSpecificOutput.additionalContext`. Copilot CLI accepts Claude's PascalCase event names as
//! aliases. The differences that remain are paths, event spellings and one envelope — data.
//!
//! **Deliberately not here yet.** No second vendor, no `id`/`label`, no manifest loader, no
//! runtime detection. This module carries exactly the fields production code reads today, because
//! `tools/find_unread_fields.py` is in the gate and a speculative field is a field nothing reads.
//! The format a user drops in a file is designed *after* a second vendor proves which fields are
//! real, which is the opposite order from the one that produces an unused config language.

/// The lifecycle events `amb` installs, each in its vendor's own spelling.
///
/// Named for what `amb` uses them for rather than for what any one vendor calls them: Claude's
/// `Stop`, Gemini's `AfterAgent` and Copilot's `agentStop` are one concept, and the concept is
/// the stable half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Events {
    /// A session begins. Mail waiting is delivered here.
    pub session_start: &'static str,
    /// The agent finished a turn — the portable delivery floor (D9 rejects `UserPromptSubmit`).
    pub turn_end: &'static str,
    /// A tool call completed. What makes claims *observed* rather than declared (D14).
    pub tool_post: &'static str,
    /// The session ended. Lapses this session's claims (D109).
    pub session_end: &'static str,
    /// Before a file tool runs — the memory layer's path-anchored lane.
    pub tool_pre: &'static str,
    /// A tool call failed. The capture lane's cheap half — and `None` where the CLI has no such
    /// event at all.
    ///
    /// **Optional because the first real second vendor proved it had to be.** Gemini CLI 0.55.1's
    /// bundle implements `BeforeTool` and `AfterTool` and nothing that fires only on failure;
    /// reading its docs suggested a mapping, and reading the shipped binary showed there was none
    /// to make. A vendor that cannot host a lane says so here, and `memory_events` installs the
    /// lanes it can rather than writing an event the runtime will silently ignore.
    pub tool_failed: Option<&'static str>,
}

/// One agent CLI, as the set of values `amb` needs to install itself into it and be found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vendor {
    /// What `--vendor` takes and `doctor` prints.
    pub id: &'static str,
    /// The product's own name, for a line a person reads.
    pub label: &'static str,
    /// The per-vendor directory, under `$HOME` and under a repository root alike.
    pub config_dir: &'static str,
    /// The settings file inside it.
    pub settings_file: &'static str,
    /// A project-scope file that overrides the shared one, where the vendor has one.
    pub local_settings_file: Option<&'static str>,
    /// A machine-managed settings file the platform ships, if any. Absolute, and
    /// platform-specific: a path that does not exist is simply skipped by every caller.
    pub managed_settings: Option<&'static str>,
    pub events: Events,
    /// Session-id environment variables, most specific first. `AMB_AGENT` overrides all of them
    /// and is not listed here, because it belongs to `amb` rather than to any vendor.
    pub session_env: &'static [&'static str],
}

/// Claude Code — the vendor `amb` was built against and, until a second one lands, the only one.
pub const CLAUDE_CODE: Vendor = Vendor {
    id: "claude-code",
    label: "Claude Code",
    config_dir: ".claude",
    settings_file: "settings.json",
    local_settings_file: Some("settings.local.json"),
    // macOS path; other platforms differ, and a missing file is skipped rather than reported.
    managed_settings: Some("/Library/Application Support/ClaudeCode/managed-settings.json"),
    events: Events {
        session_start: "SessionStart",
        turn_end: "Stop",
        tool_post: "PostToolUse",
        session_end: "SessionEnd",
        tool_pre: "PreToolUse",
        tool_failed: Some("PostToolUseFailure"),
    },
    session_env: &["CLAUDE_CODE_SESSION_ID"],
};

impl Vendor {
    /// The first session id this vendor's environment carries, if any.
    ///
    /// A list rather than one name because identity is where a second vendor arrives first: a
    /// session is whichever CLI exported an id, and `amb` never has to be told which.
    pub fn session_id_from_env(&self) -> Option<String> {
        self.session_id(|k| std::env::var(k).ok())
    }

    /// The same decision with the environment injected, because a test cannot set process env
    /// without racing the parallel runner — M51's finding, applied before the seam exists rather
    /// than after it survives a mutation pass.
    pub fn session_id(&self, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
        self.session_env
            .iter()
            .find_map(|k| lookup(k))
            .filter(|v| !v.trim().is_empty())
    }
}

/// Gemini CLI.
///
/// **Every value here was read out of the installed binary, not out of the documentation**, and
/// the two disagree in a way that matters. Gemini 0.55.1's bundle implements `SessionStart`,
/// `SessionEnd`, `BeforeAgent`, `AfterAgent`, `BeforeTool`, `AfterTool`, `BeforeModel`,
/// `AfterModel`, `BeforeToolSelection`, `Notification` and `PreCompress` — and contains **no
/// occurrence of `PreToolUse` or `PostToolUse` at all**. Installing Claude's spellings here would
/// have written entries the runtime ignores in silence, which is this project's least favourite
/// kind of failure and the reason the check was made against the shipped artefact.
///
/// It hosts two memory lanes rather than three: nothing in it fires only on a failed tool call.
///
/// The injection envelope is the same `hookSpecificOutput.additionalContext` Claude uses — 200
/// and 128 occurrences in the same bundle — so delivery needs no vendor branch at all.
pub const GEMINI_CLI: Vendor = Vendor {
    id: "gemini-cli",
    label: "Gemini CLI",
    config_dir: ".gemini",
    settings_file: "settings.json",
    // No `.local` variant and no managed-settings file are documented or present.
    local_settings_file: None,
    managed_settings: None,
    events: Events {
        session_start: "SessionStart",
        turn_end: "AfterAgent",
        tool_post: "AfterTool",
        session_end: "SessionEnd",
        tool_pre: "BeforeTool",
        tool_failed: None,
    },
    session_env: &["GEMINI_SESSION_ID"],
};

/// Every vendor `amb` can install into. Order is detection precedence.
pub const VENDORS: &[&Vendor] = &[&CLAUDE_CODE, &GEMINI_CLI];

/// The vendor named by `--vendor`, or `None` if nobody ships one by that name.
pub fn by_id(id: &str) -> Option<&'static Vendor> {
    VENDORS.iter().copied().find(|v| v.id == id)
}

/// The vendor whose session this process is running inside.
///
/// **Detected from the environment rather than passed as an argument, and that is a hook-safety
/// decision** (D97). Every hook entry `amb` installs is `<exe> hook <mode>`; adding a
/// `--vendor` token to it would put a new argument on the one path whose contract is that it
/// always exits 0, where an older binary meeting a newer entry cannot parse it and clap's exit
/// `2` reads as *blocking* to the runner. The session id already identifies the host, so nothing
/// has to be told.
///
/// Falls back to Claude Code, which is what a session with no vendor variable at all has always
/// been treated as.
pub fn detect() -> &'static Vendor {
    detect_with(|k| std::env::var(k).ok())
}

/// [`detect`] with the environment injected, so precedence is testable (M51).
pub fn detect_with(lookup: impl Fn(&str) -> Option<String>) -> &'static Vendor {
    VENDORS
        .iter()
        .copied()
        .find(|v| v.session_id(&lookup).is_some())
        .unwrap_or(&CLAUDE_CODE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Detection is how a second vendor arrives, so its precedence is asserted rather than
    /// assumed** — and with the environment injected, because a test that sets process env races
    /// the parallel runner (M51).
    ///
    /// The fallback row is the load-bearing one: a session with no vendor variable at all has
    /// always been treated as Claude Code, and every existing installation is that session.
    #[test]
    fn the_host_vendor_is_whichever_one_exported_a_session_id() {
        let only = |name: &'static str| move |k: &str| (k == name).then(|| "sess".to_string());
        assert_eq!(detect_with(only("GEMINI_SESSION_ID")).id, "gemini-cli");
        assert_eq!(
            detect_with(only("CLAUDE_CODE_SESSION_ID")).id,
            "claude-code"
        );
        assert_eq!(
            detect_with(|_| None).id,
            "claude-code",
            "a session with no vendor variable is what every install before D111 was"
        );
        assert_eq!(
            detect_with(|_| Some("   ".into())).id,
            "claude-code",
            "a blank id is not a detection"
        );
    }

    #[test]
    fn a_vendor_is_found_by_the_id_the_flag_takes_and_nothing_else_is() {
        assert_eq!(by_id("gemini-cli").expect("ships").label, "Gemini CLI");
        assert_eq!(by_id("claude-code").expect("ships").label, "Claude Code");
        assert!(by_id("copilot-cli").is_none(), "not shipped, not pretended");
        assert!(by_id("").is_none());
    }

    /// **Read out of the installed binary, and the binary disagreed with the documentation.**
    /// Gemini 0.55.1 has no event that fires only on a failed tool call, and no `PreToolUse` or
    /// `PostToolUse` at all. Both facts are asserted here because both were the difference
    /// between a working install and one whose entries the runtime ignores in silence.
    #[test]
    fn gemini_declares_the_lane_it_cannot_host_rather_than_borrowing_claudes_spelling() {
        assert_eq!(
            GEMINI_CLI.events.tool_failed, None,
            "it has no failure event"
        );
        for claudes in [
            CLAUDE_CODE.events.tool_pre,
            CLAUDE_CODE.events.tool_post,
            CLAUDE_CODE.events.turn_end,
        ] {
            let g = GEMINI_CLI.events;
            assert!(
                ![
                    g.session_start,
                    g.turn_end,
                    g.tool_post,
                    g.tool_pre,
                    g.session_end
                ]
                .contains(&claudes)
                    || claudes == "SessionStart",
                "{claudes} is Claude's spelling and Gemini's bundle does not contain it"
            );
        }
        assert_eq!(GEMINI_CLI.session_env, &["GEMINI_SESSION_ID"]);
    }
}

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
    /// A tool call failed. The capture lane's cheap half.
    pub tool_failed: &'static str,
}

/// One agent CLI, as the set of values `amb` needs to install itself into it and be found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vendor {
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
        tool_failed: "PostToolUseFailure",
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

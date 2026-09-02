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
//!
//! **That order held: the fields were proven by two shipped vendors before a file format
//! could freeze them, and only then did the loader arrive.** What a user drops in
//! `~/.config/amb/vendors/*.json` is exactly the struct below, because the struct was
//! measured rather than guessed.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::Value;

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
    /// Which tool calls the memory hook is asked about, in this CLI's **own** tool names.
    ///
    /// **A vendor's tool vocabulary is as much its own as its event names, and missing that was a
    /// live defect.** `Read|Edit|Write|NotebookEdit` is Claude's; Gemini calls the same acts
    /// `read_file`, `read_many_files`, `write_file` and `replace`, so the Claude matcher
    /// installed into Gemini matched nothing and the path-anchored lane would have fired zero
    /// times — silently, and in the shape D74 records: `by path 0/0` beside a working recency
    /// lane is not a low number, it is an incomparable one.
    ///
    /// `None` installs the hook with no matcher at all, for a CLI that has none. The cost is
    /// bounded anyway by `memory::SKIP_TOOLS` and by the injection itself, which is why the
    /// matcher is an optimisation rather than the guard.
    pub tool_matcher: Option<&'static str>,
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
    tool_matcher: Some("Read|Edit|Write|NotebookEdit"),
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
    // Gemini's own names, counted in the installed bundle: `replace` 65, `read_file` 28,
    // `read_many_files` 12, `write_file` 8. Claude's `NotebookEdit` appears zero times.
    tool_matcher: Some("read_file|read_many_files|write_file|replace"),
    session_env: &["GEMINI_SESSION_ID"],
};

/// Every vendor `amb` can install into. Order is detection precedence.
pub const VENDORS: &[&Vendor] = &[&CLAUDE_CODE, &GEMINI_CLI];

/// The vendor named by `--vendor`, or `None` if nobody ships one by that name.
pub fn by_id(id: &str) -> Option<&'static Vendor> {
    all().iter().copied().find(|v| v.id == id)
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
    all()
        .iter()
        .copied()
        .find(|v| v.session_id(&lookup).is_some())
        .unwrap_or(&CLAUDE_CODE)
}

// ── User-added vendors ──────────────────────────────────────────────────────

/// Where a user drops a vendor `amb` does not ship.
///
/// **JSON rather than TOML, reversing this work's own first plan.** TOML reads better, and taking
/// it costs a dependency on a project that hand-writes a civil calendar "thirty lines against a
/// dependency" and declined `proptest` after measuring what it would add (D102). `serde_json` is
/// already here because the files `amb` installs into *are* JSON, so a JSON manifest needs no
/// supply chain and no new parser at all. The readability argument did not survive the cost.
///
/// `$AMB_VENDORS` overrides, which is how a test reaches this without touching a real home.
pub fn manifest_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("AMB_VENDORS") {
        return (!dir.trim().is_empty()).then(|| PathBuf::from(dir));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("amb").join("vendors"))
}

/// A manifest that could not be used, and why. Reported by `doctor`; never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub file: String,
    pub detail: String,
}

/// Read one manifest document into a vendor, or say what is wrong with it.
///
/// **Pure, and the only place manifest semantics live** — the loader below is the shell. Every
/// rule is a refusal rather than a default: a manifest with no `turn_end` describes a vendor
/// whose mail never arrives, and inventing a spelling would install an entry the runtime ignores
/// in silence — the exact failure that reading Gemini's binary caught.
///
/// **A manifest may not take a shipped vendor's id.** Silent shadowing would let a file on disk
/// move where `amb install` writes with nothing saying so; the refusal names the collision.
pub fn parse_manifest(doc: &Value, shipped: &[&str]) -> Result<Vendor, String> {
    let req = |k: &str| -> Result<&'static str, String> {
        match doc.get(k).and_then(Value::as_str).map(str::trim) {
            Some(v) if !v.is_empty() => Ok(leak(v)),
            _ => Err(format!("missing or empty {k:?}")),
        }
    };
    let ev = |k: &str| -> Result<&'static str, String> {
        match doc
            .get("events")
            .and_then(|e| e.get(k))
            .and_then(Value::as_str)
            .map(str::trim)
        {
            Some(v) if !v.is_empty() => Ok(leak(v)),
            _ => Err(format!("missing or empty events.{k}")),
        }
    };

    let id = req("id")?;
    if shipped.contains(&id) {
        return Err(format!(
            "id {id:?} already belongs to another vendor; a manifest may not shadow one"
        ));
    }
    let session_env: Vec<&'static str> = doc
        .get("session_env")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(leak)
                .collect()
        })
        .unwrap_or_default();
    if session_env.is_empty() {
        return Err(
            "session_env must name at least one environment variable, or no session of this \
             vendor can ever be identified"
                .into(),
        );
    }

    Ok(Vendor {
        id,
        label: opt(doc, "label").unwrap_or(id),
        config_dir: req("config_dir")?,
        settings_file: req("settings_file")?,
        local_settings_file: opt(doc, "local_settings_file"),
        managed_settings: opt(doc, "managed_settings"),
        events: Events {
            session_start: ev("session_start")?,
            turn_end: ev("turn_end")?,
            tool_post: ev("tool_post")?,
            session_end: ev("session_end")?,
            tool_pre: ev("tool_pre")?,
            // Absent means the vendor has no such event — Gemini's case, and why the field is
            // optional at all. It is the one event whose absence is a fact rather than a mistake,
            // which is exactly why reading it from the wrong level was invisible: the first
            // version looked it up at the document root instead of under `events`, so every
            // manifest silently lost its capture lane and the dry-run printed two lanes where
            // three were declared. Nothing failed. The truth table below is what noticed.
            tool_failed: doc
                .get("events")
                .and_then(|e| e.get("tool_failed"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(leak),
        },
        tool_matcher: opt(doc, "tool_matcher"),
        session_env: Box::leak(session_env.into_boxed_slice()),
    })
}

fn opt(doc: &Value, key: &str) -> Option<&'static str> {
    doc.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(leak)
}

/// **Leaked deliberately, and the reason is the process rather than the data.** A descriptor is
/// `&'static` so the shipped ones can be `const` and every caller can hold a `Copy`; a manifest
/// read at runtime has no such lifetime. `amb` runs for milliseconds and loads these at most once
/// per process, so a few hundred bytes outliving the load is not a leak in any sense a person
/// would notice — where threading a lifetime through every field would change the type for the
/// shipped vendors too, and buy nothing back.
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Every vendor: the ones `amb` ships, then the ones a user dropped in a file.
///
/// **Loaded once per process, never on a schedule.** `detect` runs on the hook path, so the cost
/// has to be a `stat` on a directory that usually does not exist — which is what it is: the
/// `OnceLock` means at most one directory read per invocation, and the common case returns
/// before opening anything.
pub fn all() -> &'static [&'static Vendor] {
    &loaded().0
}

/// What went wrong while loading manifests. Empty on every machine that has none.
///
/// **Collected rather than raised, because this is read on the hook path** (D9). A broken
/// manifest must not be able to fail a hook; `doctor` is where a person is told.
pub fn problems() -> &'static [Problem] {
    &loaded().1
}

fn loaded() -> &'static (Vec<&'static Vendor>, Vec<Problem>) {
    static LOADED: OnceLock<(Vec<&'static Vendor>, Vec<Problem>)> = OnceLock::new();
    LOADED.get_or_init(|| {
        let mut vendors: Vec<&'static Vendor> = VENDORS.to_vec();
        let mut problems = Vec::new();
        let Some(dir) = manifest_dir() else {
            return (vendors, problems);
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // No directory is the ordinary case, not a problem worth reporting.
            return (vendors, problems);
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        // Sorted, so which of two colliding manifests is refused does not depend on the order
        // the filesystem happened to hand them back.
        files.sort();
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    problems.push(Problem {
                        file: name,
                        detail: format!("could not be read: {e}"),
                    });
                    continue;
                }
            };
            let doc: Value = match serde_json::from_str(&text) {
                Ok(d) => d,
                Err(e) => {
                    problems.push(Problem {
                        file: name,
                        detail: format!("is not valid JSON: {e}"),
                    });
                    continue;
                }
            };
            let taken: Vec<&str> = vendors.iter().map(|v| v.id).collect();
            match parse_manifest(&doc, &taken) {
                Ok(v) => vendors.push(Box::leak(Box::new(v))),
                Err(detail) => problems.push(Problem { file: name, detail }),
            }
        }
        (vendors, problems)
    })
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

    /// **A vendor's tool names are as much its own as its event names** — and this is the row
    /// that was wrong in a shipped commit. `Read|Edit|Write|NotebookEdit` went into Gemini's
    /// `BeforeTool` matcher verbatim, where it matches nothing: Gemini calls those acts
    /// `read_file`, `read_many_files`, `write_file` and `replace`, and `NotebookEdit` appears
    /// zero times in its bundle. The path-anchored lane would have fired zero times, silently,
    /// which is D74's shape — `by path 0/0` beside a working recency lane is not a low number,
    /// it is an incomparable one.
    ///
    /// The absence half is the load-bearing one: an assertion that Gemini's matcher merely
    /// *contains* its own names would pass on a matcher that also carried Claude's.
    #[test]
    fn each_vendors_matcher_is_written_in_its_own_tool_vocabulary() {
        let claude = CLAUDE_CODE
            .tool_matcher
            .expect("claude matches on tool names");
        let gemini = GEMINI_CLI.tool_matcher.expect("so does gemini");
        for mine in ["read_file", "write_file", "replace"] {
            assert!(gemini.contains(mine), "gemini's own tools: {gemini}");
        }
        for claudes in ["Read", "Edit", "Write", "NotebookEdit"] {
            assert!(
                !gemini.split('|').any(|t| t == claudes),
                "Claude's {claudes:?} in Gemini's matcher matches nothing there: {gemini}"
            );
        }
        assert!(
            claude.contains("NotebookEdit"),
            "claude keeps its own: {claude}"
        );
    }

    /// The manifest grammar, row by row. **Every rule is a refusal**, because the alternative to
    /// refusing is inventing an event spelling — and an invented spelling installs a hook entry
    /// the runtime ignores in silence, which is what reading Gemini's binary caught before it
    /// could ship.
    #[test]
    fn a_manifest_is_refused_rather_than_completed_with_guesses() {
        let full = serde_json::json!({
            "id": "acme-cli",
            "label": "ACME CLI",
            "config_dir": ".acme",
            "settings_file": "settings.json",
            "events": {
                "session_start": "Start", "turn_end": "Done", "tool_post": "AfterTool",
                "session_end": "End", "tool_pre": "BeforeTool", "tool_failed": "Failed"
            },
            "session_env": ["ACME_SESSION"]
        });
        let v = parse_manifest(&full, &["claude-code"]).expect("a complete manifest loads");
        assert_eq!(v.id, "acme-cli");
        assert_eq!(v.label, "ACME CLI");
        assert_eq!(v.events.turn_end, "Done");
        assert_eq!(v.events.tool_failed, Some("Failed"));
        assert_eq!(v.session_env, &["ACME_SESSION"]);

        // Each required key, removed one at a time: the reach rows for every refusal.
        for key in ["id", "config_dir", "settings_file"] {
            let mut doc = full.clone();
            doc.as_object_mut().expect("object").remove(key);
            let err = parse_manifest(&doc, &[]).expect_err("{key} is required");
            assert!(err.contains(key), "the refusal must name {key}: {err}");
        }
        for key in [
            "session_start",
            "turn_end",
            "tool_post",
            "session_end",
            "tool_pre",
        ] {
            let mut doc = full.clone();
            doc["events"].as_object_mut().expect("object").remove(key);
            let err = parse_manifest(&doc, &[]).expect_err("events.{key} is required");
            assert!(err.contains(key), "the refusal must name {key}: {err}");
        }

        // `tool_failed` is the one absence that is a fact rather than a mistake — Gemini's case.
        let mut no_fail = full.clone();
        no_fail["events"]
            .as_object_mut()
            .expect("object")
            .remove("tool_failed");
        let v = parse_manifest(&no_fail, &[]).expect("a vendor may simply lack a failure event");
        assert_eq!(v.events.tool_failed, None);

        // A vendor nothing can identify is refused: it would silently never be detected.
        let mut no_env = full.clone();
        no_env["session_env"] = serde_json::json!([]);
        assert!(
            parse_manifest(&no_env, &[]).is_err(),
            "no session_env, no vendor"
        );

        // And a manifest may not quietly take a shipped vendor's id.
        let mut shadow = full.clone();
        shadow["id"] = serde_json::json!("claude-code");
        let err = parse_manifest(&shadow, &["claude-code"]).expect_err("shadowing is refused");
        assert!(err.contains("shadow"), "{err}");

        // The label is the one field with a default, because an id is already a name.
        let mut no_label = full;
        no_label.as_object_mut().expect("object").remove("label");
        assert_eq!(
            parse_manifest(&no_label, &[])
                .expect("label is optional")
                .label,
            "acme-cli"
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

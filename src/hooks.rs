//! Delivery hooks: planning the edit, and applying it safely.
//!
//! Hooks are how an agent receives mail without remembering to look (`DECISIONS.md` D9). They
//! are installed once per machine into `~/.claude/settings.json`.
//!
//! # Why the transform is a pure function
//!
//! That file configures Claude Code for *every* project on the machine. Corrupting it does not
//! break `amb`; it breaks the user's entire tool. So the JSON edit is
//! [`plan_install`]/[`plan_uninstall`] — pure, total, and exhaustively testable with no
//! filesystem in sight — and everything touching disk is a thin shell around them.

use crate::error::{Error, Result, io};
use crate::vendors::Vendor;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// How the delivery hooks are wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `SessionStart` only: mail waiting when a session begins.
    Session,
    /// `SessionStart` + `Stop`: also at every turn boundary. The portable floor.
    Turn,
    /// `Turn`, plus a blocking `amb watch` for seconds-latency delivery.
    Monitor,
}

impl Mode {
    /// The hook events this mode installs.
    ///
    /// `Stop` rather than `UserPromptSubmit`, deliberately: the latter blocks the user's turn on
    /// a 30 s timeout, so a hung `amb` would hang the human. `Stop` cannot (D9).
    pub fn events(self, v: &Vendor) -> Vec<&'static str> {
        match self {
            Mode::Session => vec![v.events.session_start],
            // PostToolUse is what makes claims *observed* rather than declared (D14): the hook
            // sees every Edit and Write, so an agent never has to remember `amb claim`.
            // SessionEnd is the same coin's other face (D109): the session that recorded
            // claims releases them the moment it ends, instead of running out a four-hour TTL
            // that warns every peer off files nobody is touching. Best effort — the platform
            // does not fire it on a crash, so the TTL stays the truth.
            Mode::Turn | Mode::Monitor => vec![
                v.events.session_start,
                v.events.turn_end,
                v.events.tool_post,
                v.events.session_end,
            ],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Session => "session",
            Mode::Turn => "turn",
            Mode::Monitor => "monitor",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "session" => Some(Mode::Session),
            "turn" => Some(Mode::Turn),
            "monitor" => Some(Mode::Monitor),
            _ => None,
        }
    }
}

/// Timeout on each hook, in seconds.
///
/// Small on purpose. `amb` costs ~3 ms (`MEASUREMENTS.md` M5), so anything approaching this
/// means something is wrong, and the right response is for the hook to be killed rather than to
/// keep a session waiting.
const HOOK_TIMEOUT_SECS: u64 = 5;

// A hook's database wait must fit inside the wall-clock budget this module writes into
// `settings.json`. The two constants live in different modules and describe the same 5 seconds:
// this one is the platform's kill deadline, `db::HOOK_BUSY_TIMEOUT_MS` is how much of it an open
// may burn parked on a lock. They were never reconciled — the wait was 30 s inside a 5 s budget,
// so a contended hook was killed mid-wait rather than exiting 0, the one ending D9 forbids
// (D103). At most half, so the work the hook opened the board *for* keeps a full wait's worth of
// headroom. A `const` assertion rather than a test: the drift becomes a build failure, which is
// the strongest red available.
const _: () = assert!(
    crate::db::HOOK_BUSY_TIMEOUT_MS <= HOOK_TIMEOUT_SECS * 1000 / 2,
    "a hook may spend at most half its wall-clock budget waiting on a lock (D103)"
);

/// The outcome of planning an edit. `settings` is the file's new content.
#[derive(Debug, Clone)]
pub struct Plan {
    pub settings: Value,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl Plan {
    /// True when applying this plan would change nothing.
    ///
    /// **`added` and `removed` are trustworthy here only because [`settle`] clears them when the
    /// resulting document is byte-identical to the one it started from.** On their own they lied:
    /// [`plan_install`] strips its own entries and re-adds them unconditionally, so `added` was
    /// always populated and this was unreachable.
    ///
    /// The consequence was not cosmetic. `report_plan` writes whenever a plan is not a no-op, and
    /// the write path takes a fresh backup on every write — so a second `amb install`
    /// overwrote the only pre-`amb` backup with a post-`amb` copy, destroying the thing the
    /// backup exists for. Reproduced against a real settings file (D29).
    ///
    /// Found by a peer session on the board, which is the tool doing its job.
    pub fn is_noop(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Settle a plan against the document it started from.
///
/// Defining "changed" as "the bytes differ" is the only definition that cannot drift from what
/// applying the plan would actually do.
fn settle(
    settings: Value,
    mut added: Vec<String>,
    mut removed: Vec<String>,
    before: &Value,
) -> Plan {
    if settings == *before {
        added.clear();
        removed.clear();
    }
    Plan {
        settings,
        added,
        removed,
    }
}

/// The file name our installed hook command invokes.
const EXE_NAME: &str = "amb";

/// Whether a hook entry is one of ours.
///
/// Matched on the command string rather than a marker field, because Claude Code owns this
/// schema and may reject unknown keys.
fn is_ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(command_is_ours)
}

/// Whether a hook command line invokes *our* binary — the pure matching rule.
///
/// **Matched on the executable, at a path boundary.** The previous rule was
/// `c.contains("amb") && c.contains(" hook ")`, which claims
/// `/Users/lambert/bin/tool hook start` — so `amb uninstall` deleted a hook belonging to someone
/// else, and `amb install` deleted it too, since installing removes ours everywhere first. This
/// file configures Claude Code for *every* project on the machine; a false positive here costs a
/// stranger their tooling, which is a far worse failure than ours not being recognised (D28).
pub fn command_is_ours(command: &str) -> bool {
    // Split at the *last* ` hook `, so an install path that itself contains the word still
    // resolves: `/Users/x/my hook tools/amb hook turn` must find `amb`, not `my`.
    let Some((exe, mode)) = command.trim().rsplit_once(" hook ") else {
        return false;
    };
    // A mode is one bare token. Not checked against the known set, so a mode added in a later
    // version is still recognised as ours and can be uninstalled by an older binary.
    let mode = mode.trim();
    if mode.is_empty() || mode.split_whitespace().count() != 1 {
        return false;
    }
    // The `.exe` arm is derived rather than spelled out, so renaming EXE_NAME cannot leave a
    // stale second name behind that still matches somebody else's hook.
    std::path::Path::new(unquote(exe))
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == EXE_NAME || n.strip_suffix(".exe") == Some(EXE_NAME))
}

/// Strip one layer of surrounding shell quotes, undoing [`quote_exe`].
fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['\'', '"'] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Quote an executable path for the shell Claude Code runs hook commands through.
///
/// Without this, an install path containing a space produces a command line that runs the wrong
/// thing — silently, since a hook that fails is a hook that says nothing.
fn quote_exe(exe: &str) -> String {
    if exe
        .bytes()
        .any(|b| b.is_ascii_whitespace() || b == b'\'' || b == b'"')
    {
        // Single quotes are literal in POSIX shells; an embedded one is closed, escaped, reopened.
        format!("'{}'", exe.replace('\'', r"'\''"))
    } else {
        exe.to_string()
    }
}

fn hook_entry(exe: &str, event_arg: &str) -> Value {
    json!({
        "type": "command",
        "command": format!("{} hook {event_arg}", quote_exe(exe)),
        "timeout": HOOK_TIMEOUT_SECS,
    })
}

/// The argv token the memory hook is invoked with.
///
/// A *separate entry*, never an extra event on the delivery command. Hook timeouts are per entry,
/// so this is what keeps a memory layer that hangs from taking mail delivery with it — the
/// structural half of D9's guarantee rather than a discipline someone has to remember (D41).
/// `command_is_ours` matches on `<exe> hook <one-token>`, so an older binary still recognises and
/// can uninstall this without knowing what "memory" means.
pub const MEMORY_ARG: &str = "memory";

/// The events memory registers on, with the matcher each needs.
///
/// `PreToolUse` over `PostToolUse`: the point is to say what is known about a file *before* it is
/// opened, which is the strictest form of scoping the injection to its consumer. At
/// `SessionStart` the relevant file is a guess; here it is stated.
/// The memory lanes this vendor can host, with the matcher each needs.
///
/// **Length is a property of the vendor, not of `amb`.** A CLI with no failure event hosts two
/// lanes rather than three, and `HookState` carries the total it was measured against so a
/// complete two-lane install is never reported as a partial three-lane one.
fn memory_events(v: &Vendor) -> Vec<(&'static str, Option<&'static str>)> {
    let mut out = vec![
        (v.events.session_start, None),
        // The vendor's own tool names, never Claude's — see `Vendor::tool_matcher`. Narrowed
        // here as well as in `memory::SKIP_TOOLS`, and the redundancy is deliberate: this bounds
        // how often the process is spawned at all, while the skip list is what makes a
        // hand-edited or absent matcher harmless rather than a hook running on every tool call.
        (v.events.tool_pre, v.tool_matcher),
    ];
    // Phase 4b's cheap half. Failures are disproportionately what is worth remembering, and
    // capturing one needs no model, no transcript and no blocking — unlike 4a, which is
    // deliberately not installed.
    if let Some(failed) = v.events.tool_failed {
        out.push((failed, None));
    }
    out
}

/// Plan the installation of delivery hooks into an existing settings document.
///
/// Idempotent: installing twice adds nothing the second time. Non-destructive: other tools'
/// hooks in the same events are preserved, and only our own entries are replaced.
pub fn plan_install(existing: &Value, exe: &str, mode: Mode, memory: bool, v: &Vendor) -> Plan {
    let mut settings = existing.clone();
    // Ledgers, not labels: what was removed and what was added carry their content, so the
    // delta below can tell an identical re-add from a rewrite. Reduced to labels at the end.
    let mut added: Vec<(String, Option<Value>, Value)> = Vec::new();
    let mut removed: Vec<(String, Option<Value>, Value)> = Vec::new();

    if !settings.is_object() {
        settings = Value::Object(Map::new());
    }
    // Unreachable in practice — `settings` was just forced to an object — but expressed as a
    // fallible match rather than an unwrap, because this runs against a file we did not write.
    let Some(root) = settings.as_object_mut() else {
        return Plan {
            settings,
            added: Vec::new(),
            removed: Vec::new(),
        };
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let Some(hooks) = hooks.as_object_mut() else {
        return Plan {
            settings,
            added: Vec::new(),
            removed: Vec::new(),
        };
    };

    // Remove ours everywhere first, so switching modes cannot leave a stale event behind.
    removed.extend(strip_ours(hooks));

    let arg = mode.as_str();
    for event in mode.events(v) {
        // The same argument for every event: the hook learns which event fired from
        // `hook_event_name` in its stdin payload, so argv carries only the mode.
        push_entry(hooks, event, None, hook_entry(exe, arg), &mut added, "");
    }
    if memory {
        for (event, matcher) in memory_events(v) {
            push_entry(
                hooks,
                event,
                matcher,
                hook_entry(exe, MEMORY_ARG),
                &mut added,
                " (memory)",
            );
        }
    }

    // A removal whose *identical* entry is immediately re-added is not a change at all —
    // cancel the pair, from both lists. The first version cancelled only the removal, so a
    // reinstall whose sole difference was one new event printed the entire desired state as
    // `+` lines: seven rows for a one-entry edit, which reads as a wholesale rewrite until
    // someone diffs the JSON (it took exactly that to trust it once). Matched on content —
    // label, matcher, and the entry itself — not label alone: an entry re-added with a
    // different command or matcher is a genuine rewrite and keeps both its `-` and its `+`,
    // and label-only matching would have silenced precisely the exe-repoint case D94 exists
    // to catch. Position-based, so duplicates still cancel one-for-one.
    let mut surviving = Vec::new();
    for a in added {
        match removed.iter().position(|r| *r == a) {
            Some(i) => {
                removed.remove(i);
            }
            None => surviving.push(a),
        }
    }
    let added = surviving.into_iter().map(|(l, _, _)| l).collect();
    let removed = removed.into_iter().map(|(l, _, _)| l).collect();
    settle(settings, added, removed, existing)
}

/// Remove every entry belonging to us, returning one label per entry actually removed.
///
/// **One copy, because install and uninstall had two.** They were identical loops, and the
/// labelling below is the kind of change that gets applied to one of them and not the other.
///
/// Labels distinguish `SessionStart` from `SessionStart (memory)` because both live under the
/// same event. Reporting only the event name meant `amb install` (without `--memory`) said it
/// removed a `PreToolUse` hook and stayed silent about the `SessionStart` memory entry it also
/// took out — a summary that understates what was done, in exactly the area D29 was about.
fn strip_ours(hooks: &mut Map<String, Value>) -> Vec<(String, Option<Value>, Value)> {
    let mut removed = Vec::new();
    for (event, matchers) in hooks.iter_mut() {
        let Some(list) = matchers.as_array_mut() else {
            continue;
        };
        for matcher in list.iter_mut() {
            let scope = matcher.get("matcher").cloned();
            if let Some(inner) = matcher.get_mut("hooks").and_then(Value::as_array_mut) {
                inner.retain(|e| {
                    if is_ours(e) {
                        removed.push((label_of(event, e), scope.clone(), e.clone()));
                        false
                    } else {
                        true
                    }
                });
            }
        }
        // A matcher whose only hook was ours is now an empty shell; leaving it behind would
        // accumulate one per install.
        list.retain(|m| {
            m.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|h| !h.is_empty())
        });
    }
    removed
}

/// `SessionStart` or `SessionStart (memory)`, decided by the entry's own command line.
/// Whether a hook entry is the *memory* half rather than the delivery half.
///
/// One definition, because two callers now ask: the installer, to label a removal, and
/// [`memory_hooks`], to answer whether the layer is running at all. A second copy of this rule
/// could drift and make the receipt confidently wrong about its own instrumentation.
fn is_memory_entry(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .and_then(|c| c.trim().rsplit_once(" hook "))
        .is_some_and(|(_, mode)| mode.trim() == MEMORY_ARG)
}

/// Whether the memory hooks are installed, as far as `amb` can tell.
///
/// **`Unknown` is a distinct state and must stay one.** A settings file that cannot be read or
/// parsed is not evidence that memory is off, and collapsing it into `Incomplete` would replace
/// one confidently wrong reading with another.
///
/// **`Incomplete` carries the events, and covers "some" as well as "none".** The first version had
/// a bare `Absent` with the event list passed alongside it, which let a partial install print
/// `NOT INSTALLED` — false, and false in the direction that makes someone reinstall rather than
/// look. Whatever describes the state has to travel with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookState {
    Installed,
    Incomplete {
        missing: Vec<String>,
        /// How many lanes this vendor could have hosted. Travels with the state for the reason
        /// the paragraph above gives about `missing`: a two-lane vendor missing both is "NOT
        /// INSTALLED", and without the total that is indistinguishable from a three-lane vendor
        /// missing two.
        total: usize,
    },
    Unknown,
}

impl HookState {
    /// The line a human reads above the counts, or `None` when there is nothing to warn about.
    ///
    /// **In the library because it is a statement about the data, not a rendering choice.** The
    /// first version decided `installed` versus `absent` inside `src/main.rs`, which is the one
    /// file this project keeps free of logic precisely so decisions stay testable — and it was
    /// untested, in the commit whose whole subject was a decision made without checking its
    /// premise.
    pub fn caveat(&self) -> Option<String> {
        match self {
            HookState::Installed => None,
            HookState::Incomplete { missing, total } => Some(format!(
                "memory hooks: {} — missing {}. The counts that follow predate this and are not \
                 evidence about the corpus; run `amb install --memory` to restore them",
                if missing.len() == *total {
                    "NOT INSTALLED"
                } else {
                    "PARTIALLY INSTALLED"
                },
                missing.join(", ")
            )),
            // **The path is named by `doctor` rather than spelled here.** This string used to
            // read `~/.claude/settings.json`, which is one vendor's file and became a wrong
            // answer the moment there were two; `HookState` carries no vendor to substitute, and
            // threading one through `render_status` to fix a caveat line is the wrong depth. The
            // command that resolves the path already prints it.
            HookState::Unknown => Some(
                "memory hooks: unknown — this CLI's settings file could not be read, so whether \
                 injection is running is unverified; `amb doctor` names the file it tried"
                    .to_string(),
            ),
        }
    }

    /// A stable token for `--json`, so a machine consumer can branch on this without parsing prose.
    pub fn as_str(&self) -> &'static str {
        match self {
            HookState::Installed => "installed",
            HookState::Incomplete { .. } => "incomplete",
            HookState::Unknown => "unknown",
        }
    }
}

/// The memory hook state of a settings document. Pure, so every case is testable without a
/// filesystem — including the one that matters, a settings file with delivery hooks and no memory
/// ones, which is the state this machine was actually found in.
pub fn memory_state(settings: &Value, v: &Vendor) -> HookState {
    let (_, missing) = memory_hooks(settings, v);
    if missing.is_empty() {
        HookState::Installed
    } else {
        HookState::Incomplete {
            total: memory_events(v).len(),
            missing,
        }
    }
}

/// Which memory hook events are registered to our binary, and which are missing.
///
/// **This exists because a withdrawal condition could not tell "not working" from "not running".**
/// D59 withdraws the injection layer when the cite ratio stays flat. The same flat zero is
/// produced by a layer that is installed and useless and by one that was never installed — and
/// the second happened: `install --memory` describes the *complete* desired hook state, so a
/// later `amb install` for an unrelated mode change removed all three memory entries, correctly
/// and as documented. The removals were printed. Nobody was reading. Weeks of "evidence"
/// accumulated from a feature that was switched off, and D59 was measurably approaching a verdict
/// on it.
pub fn memory_hooks(settings: &Value, v: &Vendor) -> (Vec<String>, Vec<String>) {
    let mut installed = Vec::new();
    let mut missing = Vec::new();
    let hooks = settings.get("hooks").and_then(Value::as_object);
    for (event, _) in memory_events(v) {
        let present = hooks
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
            .is_some_and(|matchers| {
                matchers.iter().any(|m| {
                    m.get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|inner| inner.iter().any(|e| is_ours(e) && is_memory_entry(e)))
                })
            });
        if present {
            installed.push((*event).to_string());
        } else {
            missing.push((*event).to_string());
        }
    }
    (installed, missing)
}

fn label_of(event: &str, entry: &Value) -> String {
    if is_memory_entry(entry) {
        format!("{event} (memory)")
    } else {
        event.to_string()
    }
}

/// Append one entry under an event, creating the event's list if it is missing or malformed.
fn push_entry(
    hooks: &mut Map<String, Value>,
    event: &str,
    matcher: Option<&str>,
    entry: Value,
    added: &mut Vec<(String, Option<Value>, Value)>,
    label: &str,
) {
    let list = hooks
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !list.is_array() {
        *list = Value::Array(Vec::new());
    }
    let Some(list) = list.as_array_mut() else {
        return;
    };
    let mut wrapper = json!({ "hooks": [entry.clone()] });
    // Only written when there is one. An absent matcher and `"*"` mean the same thing to the
    // platform, and the delivery hooks have always been written without one.
    if let Some(m) = matcher
        && let Some(obj) = wrapper.as_object_mut()
    {
        obj.insert("matcher".to_string(), Value::String(m.to_string()));
    }
    list.push(wrapper);
    added.push((
        format!("{event}{label}"),
        matcher.map(|m| Value::String(m.to_string())),
        entry,
    ));
}

/// The two fields every file-scoped hook reads out of a Claude Code payload.
///
/// **One copy.** Three call sites in `src/main.rs` each dug `tool_name` and
/// `tool_input.file_path` out of the same `Value` with the same
/// `.and_then(Value::as_str).unwrap_or_default()` shape, which is three places to keep in step
/// with a schema this project does not own. Absent or wrongly-typed fields degrade to
/// `("", None)`, never to a panic: this runs inside somebody else's session and D9's guarantee is
/// that mail delivery never breaks one.
pub fn tool_and_file(input: &Value) -> (&str, Option<&str>) {
    let tool = input
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let file = input
        .get("tool_input")
        .and_then(|t| t.get("file_path"))
        .and_then(Value::as_str);
    (tool, file)
}

/// Why a tool call failed, as the payload chose to say it.
///
/// **A schema fallback chain, and D78's own explanation of why it was in `main.rs`**: the binary
/// is where the `Value` already is, so a function that needs one arrives there. Its siblings
/// [`tool_and_file`] and `memory::failure_note` were extracted; this was the one left standing,
/// which is the pattern this project keeps rediscovering under a different name each time.
///
/// Two keys because two vendors spell it differently and neither is guaranteed: `error` is the
/// direct form, `tool_response` the one that carries the tool's own output. A payload with
/// neither still produces a note — an untitled failure is a worse record than a vague one, and
/// silence here is the thing D52's counter exists to make impossible.
pub fn failure_detail(input: &Value) -> &str {
    input
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| input.get("tool_response").and_then(Value::as_str))
        .unwrap_or("no detail in the payload")
}

/// Which memory lane a hook event belongs to, for the vendor that sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLane {
    /// A failed tool call, recorded as a `capture` and never injected (D86).
    Capture,
    /// Before a file tool call — the path-anchored lane (D42).
    File,
    /// Session start — the recency lane.
    Session,
}

/// Route a hook event to its memory lane.
///
/// **A vendor's event vocabulary decides this, and a constant here would be a silent no-op on
/// every other vendor** — the defect D111 phase 2 shipped and M64 found, in a neighbouring
/// function. `tool_failed` is `Option` because Gemini CLI hosts two lanes rather than three:
/// nothing in it fires only on a failed tool call. So a vendor that cannot report failure must
/// never route to [`MemoryLane::Capture`], and `None == Some(event)` is false for every event,
/// which is what makes that fall through correctly rather than by accident.
///
/// Session is the fallback rather than a fourth arm: the memory hook is installed on exactly
/// three events, and an unexpected one injecting by recency is the harmless answer where
/// injecting nothing would be a lane that silently stopped firing.
pub fn memory_lane(v: &crate::vendors::Vendor, event: &str) -> MemoryLane {
    if v.events.tool_failed == Some(event) {
        MemoryLane::Capture
    } else if event == v.events.tool_pre {
        MemoryLane::File
    } else {
        MemoryLane::Session
    }
}

/// What the delivery hook does with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliverAction {
    /// A file was just written: record the claim and report conflicts in the same breath (D14).
    RecordEdit,
    /// The session is over: lapse its claims now rather than waiting out the TTL (D109).
    LapseClaims,
    /// Offer whatever mail is waiting. `start` distinguishes the session banner from a turn
    /// boundary — the banner is longer and only earns its context once.
    Offer { start: bool },
}

/// Route a hook event to what the delivery hook should do with it.
///
/// Extracted for the same reason as [`memory_lane`]: it is a decision about somebody else's
/// vocabulary, it lived in `main.rs` where no test could reach it, and its failure mode is a lane
/// that does nothing on a vendor nobody checked. Exhaustively testable across every shipped
/// descriptor now, which is what turns "we remembered to use the vendor's spelling" into
/// something the suite asserts.
pub fn deliver_action(v: &crate::vendors::Vendor, event: &str) -> DeliverAction {
    if event == v.events.tool_post {
        DeliverAction::RecordEdit
    } else if event == v.events.session_end {
        DeliverAction::LapseClaims
    } else {
        DeliverAction::Offer {
            start: event == v.events.session_start,
        }
    }
}

/// True when this Stop firing is the wake the hook's own previous output caused.
///
/// The runner counts a Stop hook that injects `additionalContext` as blocking the turn from
/// ending: it wakes the model to read the context, the model answers, Stop fires again — and
/// `stop_hook_active: true` is the runner saying this firing IS that wake. Answering it again is
/// a loop, and it happened at machine scale: during a stale-binary window the arrival note
/// printed on every Stop, so every session on the machine cycled banner → "Standing by." →
/// banner until the platform's block cap overrode — five projects at once, twice (2026-08-27 and
/// 2026-08-31, both read out of the transcripts). Only a literal `true` counts: absent, `false`
/// or wrongly-typed fields are a first firing, and this schema is not ours to assume more about.
pub fn is_stop_refire(input: &Value) -> bool {
    input.get("stop_hook_active").and_then(Value::as_bool) == Some(true)
}

/// The event this payload announces, with `SessionStart` standing in for everything else.
///
/// The same three-copy count that hoisted [`tool_and_file`] (D78): written out identically in
/// `hook_main`, `hook_memory` and `hook_deliver`, on a schema this project does not own. Absent
/// or wrongly-typed fields degrade to `"SessionStart"` — every consumer treats that as the
/// ordinary banner case, so an unknown event renders like a session opening, never like a tool
/// event it was not.
pub fn event_name(input: &Value) -> &str {
    input
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("SessionStart")
}

/// True when this payload comes from a subagent rather than the session itself.
///
/// A subagent is not a participant on the board: it has no independent inbox and would register
/// as a phantom peer, so every hook goes silent for it. The key's *presence* is the whole test —
/// a null or wrongly-typed `agent_id` still reads as a subagent, because only subagent payloads
/// carry the key at all, and assuming more of the schema is not ours to do.
pub fn is_subagent(input: &Value) -> bool {
    input.get("agent_id").is_some()
}

/// Every hook entry that belongs to us, as `(event, executable)`.
///
/// **The executable, specifically, and that is the point.** [`command_is_ours`] matches on the
/// file *name* so `uninstall` removes our hooks wherever they were installed from (D28) — correct
/// for uninstall, and exactly why nothing notices a hook pointing at a *different, older* `amb`.
/// That is the stale-binary failure this project has hit four times: manual commands work
/// perfectly while every hook on the machine runs last week's build. [`HookState`] cannot see it
/// either, because a stale binary is still "ours". Returning the path is what lets `doctor`
/// compare it against the build that is running.
pub fn our_hook_exes(settings: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return out;
    };
    for (event, list) in hooks {
        let Some(matchers) = list.as_array() else {
            continue;
        };
        for m in matchers {
            let Some(inner) = m.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for entry in inner {
                if !is_ours(entry) {
                    continue;
                }
                if let Some(cmd) = entry.get("command").and_then(Value::as_str)
                    && let Some((exe, _)) = cmd.trim().rsplit_once(" hook ")
                {
                    out.push((event.clone(), unquote(exe).to_string()));
                }
            }
        }
    }
    out.sort();
    out
}

/// Plan the removal of every hook entry belonging to us, leaving other tools' alone.
pub fn plan_uninstall(existing: &Value) -> Plan {
    let mut settings = existing.clone();
    let mut removed = Vec::new();

    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        removed.extend(strip_ours(hooks).into_iter().map(|(label, _, _)| label));
        let empty: Vec<String> = hooks
            .iter()
            .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();
        for k in empty {
            hooks.remove(&k);
        }
    }
    // Drop a `hooks` key that we emptied. An empty hooks object means nothing, and leaving it
    // behind makes uninstall non-reversible — the file would never return to its original shape.
    let now_empty = settings
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(serde_json::Map::is_empty);
    if now_empty && let Some(root) = settings.as_object_mut() {
        root.remove("hooks");
    }
    settle(settings, Vec::new(), removed, existing)
}

/// Every settings file whose hooks a CLI merges, in precedence order.
///
/// **Hooks are list-valued, and the platform combines lists rather than overriding them** —
/// *"when you set the same list key in more than one file, Claude Code combines the lists instead
/// of picking one"*. So every scope below contributes hooks that all run, which is the mechanism
/// behind D77: the memory hooks were registered in `~/.claude/settings.json` *and* in this
/// repository's `.claude/settings.local.json`, and both fired.
///
/// **`claude --settings` is deliberately absent and that is a stated hole, not an oversight.** It
/// is a per-session flag with no on-disk trace a later process can find, so nothing invoked from a
/// shell can enumerate it. A duplicate introduced that way is invisible here.
pub fn settings_sources(v: &Vendor, home: &Path, cwd: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::with_capacity(4);
    // Managed settings first, matching the platform's own precedence listing. Absolute and
    // platform-specific; a vendor that ships none says so, and a missing file is simply skipped
    // by the caller.
    if let Some(managed) = v.managed_settings {
        out.push(("managed".into(), PathBuf::from(managed)));
    }
    if let Some(local) = v.local_settings_file {
        out.push(("project local".into(), cwd.join(v.config_dir).join(local)));
    }
    out.push((
        "project".into(),
        cwd.join(v.config_dir).join(v.settings_file),
    ));
    out.push(("user".into(), home.join(v.config_dir).join(v.settings_file)));
    out
}

/// One `amb` hook command that will run more than once per event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateHook {
    pub event: String,
    pub command: String,
    /// The scope labels it was found in, in the order given. Repeats when one file lists it twice.
    pub sources: Vec<String>,
}

/// `amb` hook entries that the platform will run more than once for a single event.
///
/// **Pure, over already-parsed settings, because the finding has to be testable without four
/// files on disk** — and because D77's instance spanned two scopes, so a fixture has to be able
/// to express "the same command in two documents".
///
/// **Why this is worth a check at all, and it is not tidiness** (D77). Duplicated hooks make every
/// injection *happen* twice and *count* once: `note_events` is keyed
/// `(session, kind, scope, slug, event)`, so a note injected twice into one session records one
/// row. The cost doubles, the denominator does not, and the citation ratio D59 retires the
/// injection layer on improves for free. **The error is invisible and in the flattering
/// direction**, which is the only kind this project treats as urgent.
///
/// Keyed on `(event, command)` rather than on the executable: two entries naming the same binary
/// with different modes are two different jobs, and only an identical command line is a repeat.
pub fn duplicate_hooks(sources: &[(String, Value)]) -> Vec<DuplicateHook> {
    let mut seen: Vec<(String, String, Vec<String>)> = Vec::new();
    for (label, settings) in sources {
        let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
            continue;
        };
        for (event, list) in hooks {
            let Some(matchers) = list.as_array() else {
                continue;
            };
            for m in matchers {
                let Some(inner) = m.get("hooks").and_then(Value::as_array) else {
                    continue;
                };
                for entry in inner {
                    if !is_ours(entry) {
                        continue;
                    }
                    let Some(cmd) = entry.get("command").and_then(Value::as_str) else {
                        continue;
                    };
                    match seen.iter_mut().find(|(e, c, _)| e == event && c == cmd) {
                        Some((_, _, labels)) => labels.push(label.clone()),
                        None => seen.push((event.clone(), cmd.to_string(), vec![label.clone()])),
                    }
                }
            }
        }
    }
    let mut out: Vec<DuplicateHook> = seen
        .into_iter()
        .filter(|(_, _, labels)| labels.len() > 1)
        .map(|(event, command, sources)| DuplicateHook {
            event,
            command,
            sources,
        })
        .collect();
    out.sort_by(|a, b| (&a.event, &a.command).cmp(&(&b.event, &b.command)));
    out
}

/// The settings file the hooks are installed into, for one vendor.
pub fn settings_path(v: &Vendor) -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoIdentity)?;
    Ok(PathBuf::from(home).join(v.config_dir).join(v.settings_file))
}

/// Read a settings document, treating an absent file as an empty one.
pub fn read_settings(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).map_err(|source| Error::Json {
            context: path.display().to_string(),
            source,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(source) => Err(Error::Io {
            context: format!("reading {}", path.display()),
            source,
        }),
    }
}

/// An exclusive hold on `~/.claude/settings.json` for the length of a read-modify-write.
///
/// Dropping it releases the lock, so the guard's lifetime *is* the critical section.
pub struct SettingsLock {
    /// Held for its `Drop`. The lock releases when the descriptor closes.
    _file: std::fs::File,
}

/// Whether the lock was actually taken, or the reason it was not.
///
///
/// **Reported rather than swallowed.** A filesystem without working advisory locks still gets the
/// install; what it does not get is a silent claim of safety. `restrict` swallows its failures for
/// the same class of reason, but this one is user-invoked, so there is somebody present to read it.
pub enum LockState {
    Held(SettingsLock),
    Unavailable(String),
}

/// The lock file guarding the settings read-modify-write (D99).
///
/// A sibling rather than `settings.json` itself, because the write path replaces that file by
/// `rename`: a lock on the old inode says nothing about the new one, and the second process would
/// hold a descriptor for a file no longer at that path.
const LOCK_FILE: &str = ".amb-settings.lock";

/// Take the settings lock, blocking until it is free.
///
/// **The critical section is read + plan + write, not the write alone** (D99). The write itself
/// was already atomic — temp file plus `rename`, and 540 measured trials produced zero corrupt
/// files (M31), and [`write_if_unchanged`] still is. What was unguarded is the *cycle*: read at T, decide, write at T+ε, with another
/// writer free to land in between. Measured on this machine, that lost a third party's setting in
/// **38 of 540** runs and amb's **own hooks in 8**, the second being a silent stop to mail
/// delivery.
///
/// `File::lock` is `std` since Rust 1.89 and this crate pins 1.98, so this costs no dependency.
pub fn lock_settings(path: &Path) -> LockState {
    let Some(dir) = path.parent() else {
        return LockState::Unavailable("settings path has no parent directory".into());
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        return LockState::Unavailable(format!("creating {}: {e}", dir.display()));
    }
    let lock_path = dir.join(LOCK_FILE);
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => return LockState::Unavailable(format!("opening {}: {e}", lock_path.display())),
    };
    match file.lock() {
        Ok(()) => LockState::Held(SettingsLock { _file: file }),
        Err(e) => LockState::Unavailable(format!("locking {}: {e}", lock_path.display())),
    }
}

/// The exact bytes of a settings file, or `None` when it does not exist.
///
/// Separate from [`read_settings`] because the compare-and-swap in [`apply`] compares *bytes*, not
/// parsed JSON: two documents can be equal as `Value` and differ on disk, and it is the file
/// another process wrote that must be detected, not an equivalent one.
fn read_raw(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            context: format!("reading {}", path.display()),
            source,
        }),
    }
}

/// How many times [`apply`] re-runs its cycle when another process wrote first.
///
/// A bound rather than a loop: a settings file being rewritten faster than amb can read it is not
/// a condition retrying fixes, and spinning inside somebody's terminal is worse than saying so.
pub const MAX_RMW_ATTEMPTS: usize = 8;

/// What [`apply`] did, and under what protection.
#[derive(Debug)]
pub struct Applied {
    pub plan: Plan,
    /// Whether the advisory lock was held. Reported, because an unlocked write is weaker.
    pub locked: bool,
    pub lock_error: Option<String>,
    /// Times another process wrote during the cycle, forcing a re-read.
    pub retries: usize,
}

/// What `amb install`/`uninstall` prints after a cycle — the human half, as a string.
///
/// **This lived in `main.rs` and could not be tested there** (D78's rule, and its shape exactly:
/// the function was in the binary because that is where `Cli` already was). Its retry line is a
/// guard over a count, so all three of `> 0`'s relaxations survived mutation — `>= 0` announces
/// contention on every quiet install, and `== 0` or `< 0` silences a real one, which is precisely
/// what that line's own comment says must not happen (M56).
///
/// The JSON lane stays in the binary: it is a `serde_json` value assembled from `Cli`, and the
/// stability contract over it is asserted where the binary is driven.
pub fn render_applied(
    done: &Applied,
    path: &std::path::Path,
    dry_run: bool,
    label: &str,
) -> String {
    let plan = &done.plan;
    let mut out = String::new();
    // **Said, never swallowed** (D99). An unlocked write still happens — a filesystem without
    // working advisory locks should not lose its install — but the one thing it must not do is
    // report the same success as a locked one. This edits the file that configures Claude Code
    // for every project on the machine.
    if let Some(why) = &done.lock_error {
        out.push_str(&format!(
            "! could not lock {} ({why}) — the change was still written and still verified \
             unchanged before replacing the file, but two amb processes could interleave\n",
            path.display()
        ));
    }
    if done.retries > 0 {
        // Not a warning. It is the mechanism working, and staying silent about it would make a
        // contended settings file indistinguishable from a quiet one.
        out.push_str(&format!(
            "  another process wrote {} first; re-read and re-applied ({} time(s))\n",
            path.display(),
            done.retries
        ));
    }
    if plan.is_noop() {
        out.push_str(&format!("no change needed in {}\n", path.display()));
    } else {
        let verb = if dry_run { "would update" } else { "updated" };
        out.push_str(&format!("{verb} {}\n", path.display()));
        for e in &plan.added {
            out.push_str(&format!("  + {e} hook ({label})\n"));
        }
        for e in &plan.removed {
            out.push_str(&format!("  - {e} hook\n"));
        }
    }
    out
}

/// Read, plan and write `~/.claude/settings.json` as one guarded cycle (D99).
///
/// **Two protections, because they cover different writers, and this was measured rather than
/// reasoned** (M31).
///
/// 1. **An advisory lock** — [`lock_settings`]. It makes two `amb` processes serialise, and it
///    took a measured 46 lost updates in 540 trials to **zero**. It does nothing against a writer
///    that does not take it, and against a naive writer the same harness still lost 42 of 540 —
///    which is the whole point, because **Claude Code writes this file and will never take amb's
///    lock.** `/config` writes `crossSessionInbound` to user settings.
/// 2. **Compare-and-swap** — the bytes read at the start of the cycle are re-read immediately
///    before the rename, and a mismatch restarts the cycle rather than overwriting. This is what
///    covers the uncooperative writer, because it detects rather than excludes.
///
/// The residual window is between the final comparison and the `rename`, which is two syscalls
/// rather than the ~4 ms the whole cycle used to take. It is not zero, and saying it is zero would
/// be the kind of claim this file is full of corrections to.
pub fn apply(
    path: &Path,
    dry_run: bool,
    mut planner: impl FnMut(&Value) -> Plan,
) -> Result<Applied> {
    let lock = lock_settings(path);
    let (locked, lock_error) = match &lock {
        LockState::Held(_) => (true, None),
        LockState::Unavailable(why) => (false, Some(why.clone())),
    };

    for attempt in 0..MAX_RMW_ATTEMPTS {
        let before = read_raw(path)?;
        let value = match &before {
            None => json!({}),
            Some(s) if s.trim().is_empty() => json!({}),
            Some(s) => serde_json::from_str(s).map_err(|source| Error::Json {
                context: path.display().to_string(),
                source,
            })?,
        };
        let plan = planner(&value);

        // Nothing to write means nothing to race with.
        if dry_run || plan.is_noop() {
            return Ok(Applied {
                plan,
                locked,
                lock_error,
                retries: attempt,
            });
        }
        if write_if_unchanged(path, &plan.settings, before.as_deref())? {
            return Ok(Applied {
                plan,
                locked,
                lock_error,
                retries: attempt,
            });
        }
        // Somebody wrote while we were deciding. Their content is now the base for our plan.
    }
    Err(Error::Io {
        context: format!(
            "{} changed under every one of {MAX_RMW_ATTEMPTS} attempts to update it; another \
             process is rewriting it continuously",
            path.display()
        ),
        source: std::io::Error::other("settings file contended"),
    })
}

/// The temporary sibling one process writes before renaming over the settings file.
///
/// Deliberately NOT shared with `memory::write`'s twin: the two writers target disjoint files
/// that can never collide with each other, and the half that carries the actual guarantee — the
/// rename discipline around this name — differs between them (this one runs a compare-and-swap
/// re-read first, that one chmods first). What each site needs is its own pid assertion, not a
/// shared string builder; a function exists here only so a test can hold the property.
fn settings_tmp(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!("json.amb-tmp.{}", std::process::id()))
}

/// Write settings back, but only if the file still holds `seen`. Returns whether it wrote.
fn write_if_unchanged(path: &Path, value: &Value, seen: Option<&str>) -> Result<bool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let body = serde_json::to_string_pretty(value).map_err(|source| Error::Json {
        context: "serialising settings".into(),
        source,
    })?;
    let tmp = settings_tmp(path);
    {
        use std::io::Write;
        let mut f =
            std::fs::File::create(&tmp).map_err(io(format!("writing {}", tmp.display())))?;
        f.write_all(format!("{body}\n").as_bytes())
            .map_err(io(format!("writing {}", tmp.display())))?;
        // settings.json is the host CLI's whole configuration — a zero-length file here breaks the
        // user's entire tool, not just amb (the silent-loss shape this project exists to avoid).
        // The bytes are made durable before the rename below can publish the name: fsync of the
        // tmp file rules out the crash that leaves the file present but empty. The parent-dir fsync
        // that would additionally harden the rename itself is omitted deliberately — a lost rename
        // leaves the previous valid settings, which `amb install` simply rewrites, not a corrupt
        // file. `synchronous=NORMAL` on the disposable board (D15) is the opposite call for the
        // opposite kind of file; this one is neither amb's nor disposable.
        f.sync_all()
            .map_err(io(format!("flushing {}", tmp.display())))?;
    }

    // **The backup is taken before the check, deliberately, and this ordering was measured.**
    // Copying it *between* the check and the rename put a whole file copy inside the window the
    // check exists to close — measured at 12 lost updates in 540 trials against an uncooperative
    // writer, against 4 once the copy moved above it (M31). The backup is of the file we are about
    // to verify, so taking it first costs nothing but a discarded copy on the retry path.
    if seen.is_some() {
        let backup = path.with_extension("json.amb-backup");
        std::fs::copy(path, &backup).map_err(io(format!("writing backup {}", backup.display())))?;
    }

    // **The check goes as late as it can.** Anything read earlier is a claim about the past; this
    // is a claim about the instant before the rename, and what follows it is one syscall.
    //
    // The gap is not zero and this decision does not pretend it is. Closing it entirely needs an
    // atomic compare-and-rename the platform does not portably offer, and the residual is stated
    // in D99 with the number attached.
    if read_raw(path)?.as_deref() != seen {
        let _ = std::fs::remove_file(&tmp);
        return Ok(false);
    }
    std::fs::rename(&tmp, path).map_err(io(format!("replacing {}", path.display())))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    //! **These pin Claude Code's shape, and say so through these three shims.** Every assertion
    //! below predates the vendor descriptor and is a claim about *Claude's* settings document —
    //! its event spellings, its `hooks → matcher → hooks` nesting, its file paths. Threading the
    //! vendor through each of sixty-one call sites would have restated that constant sixty-one
    //! times without adding a claim. The vendor axis has its own tests, which drive a second
    //! descriptor rather than this one; if these shims ever disagree with those, the seam is
    //! what broke.
    use crate::vendors::CLAUDE_CODE;

    use crate::vendors::{GEMINI_CLI, VENDORS};

    /// **Every shipped vendor routes its own three events to three distinct lanes.** Enumerated
    /// rather than spot-checked: a vendor added to `VENDORS` whose spellings collide, or whose
    /// `tool_pre` was copied from another descriptor, fails here rather than becoming a lane that
    /// silently never fires. That is the defect class D111 phase 2 shipped and M64 found.
    #[test]
    fn every_vendors_events_reach_the_lane_they_name() {
        for v in VENDORS {
            assert_eq!(
                super::memory_lane(v, v.events.tool_pre),
                super::MemoryLane::File,
                "{}: tool_pre must be the path lane",
                v.label
            );
            assert_eq!(
                super::memory_lane(v, v.events.session_start),
                super::MemoryLane::Session,
                "{}: session_start must be the recency lane",
                v.label
            );
            assert_eq!(
                super::deliver_action(v, v.events.tool_post),
                super::DeliverAction::RecordEdit,
                "{}: tool_post must record the edit",
                v.label
            );
            assert_eq!(
                super::deliver_action(v, v.events.session_end),
                super::DeliverAction::LapseClaims,
                "{}: session_end must lapse claims (D109)",
                v.label
            );
            assert_eq!(
                super::deliver_action(v, v.events.session_start),
                super::DeliverAction::Offer { start: true },
                "{}: session_start is the banner",
                v.label
            );
            assert_eq!(
                super::deliver_action(v, v.events.turn_end),
                super::DeliverAction::Offer { start: false },
                "{}: a turn boundary offers without the banner",
                v.label
            );
        }
    }

    /// **A vendor with no failure event must never reach the capture lane, and `Option` is what
    /// makes that true rather than a comment.** Gemini CLI hosts two memory lanes rather than
    /// three: nothing in it fires only on a failed tool call. `None == Some(event)` is false for
    /// every event, so its `AfterTool` falls through to the recency lane instead of writing
    /// captures nobody asked for — asserted here because the alternative reads identically.
    #[test]
    fn a_vendor_that_cannot_report_failure_never_captures() {
        assert_eq!(
            super::memory_lane(&CLAUDE_CODE, "PostToolUseFailure"),
            super::MemoryLane::Capture,
            "Claude Code does report failure, and that is the lane"
        );
        for event in [
            "AfterTool",
            "BeforeTool",
            "SessionStart",
            "PostToolUseFailure",
        ] {
            assert_ne!(
                super::memory_lane(&GEMINI_CLI, event),
                super::MemoryLane::Capture,
                "Gemini has no failure event, so {event} must not capture"
            );
        }
    }

    /// One vendor's vocabulary must not steer another's hook — the same rule `tool_matcher`
    /// carries, applied to routing. Gemini's `AfterTool` reaching Claude's `RecordEdit` would be
    /// a claim recorded on an event Claude never sends.
    #[test]
    fn one_vendors_spelling_does_not_route_anothers_hook() {
        assert_eq!(
            super::deliver_action(&CLAUDE_CODE, "AfterTool"),
            super::DeliverAction::Offer { start: false },
            "Gemini's tool_post is not Claude's"
        );
        assert_eq!(
            super::deliver_action(&GEMINI_CLI, "PostToolUse"),
            super::DeliverAction::Offer { start: false },
            "and Claude's is not Gemini's"
        );
    }

    /// The payload chain, including the arm that exists so a failure is never untitled.
    #[test]
    fn a_failure_detail_prefers_error_then_the_tool_response_then_says_neither_was_there() {
        let j = serde_json::json!({"error": "boom", "tool_response": "ignored"});
        assert_eq!(super::failure_detail(&j), "boom", "error wins");
        let j = serde_json::json!({"tool_response": "fallback"});
        assert_eq!(super::failure_detail(&j), "fallback");
        let j = serde_json::json!({"error": 7, "tool_response": "fallback"});
        assert_eq!(
            super::failure_detail(&j),
            "fallback",
            "a wrongly-typed error falls through rather than stringifying a number"
        );
        assert_eq!(
            super::failure_detail(&serde_json::json!({})),
            "no detail in the payload",
            "an empty payload still yields a note (D52)"
        );
    }

    fn plan_install(existing: &Value, exe: &str, mode: Mode, memory: bool) -> Plan {
        super::plan_install(existing, exe, mode, memory, &CLAUDE_CODE)
    }
    fn memory_state(settings: &Value) -> HookState {
        super::memory_state(settings, &CLAUDE_CODE)
    }
    fn memory_hooks(settings: &Value) -> (Vec<String>, Vec<String>) {
        super::memory_hooks(settings, &CLAUDE_CODE)
    }
    fn settings_sources(home: &Path, cwd: &Path) -> Vec<(String, PathBuf)> {
        super::settings_sources(&CLAUDE_CODE, home, cwd)
    }

    /// A vendor that is not Claude, so the seam is asserted rather than assumed.
    const OTHER: crate::vendors::Vendor = crate::vendors::Vendor {
        id: "other",
        label: "Other CLI",
        config_dir: ".other",
        settings_file: "config.json",
        local_settings_file: None,
        managed_settings: None,
        events: crate::vendors::Events {
            session_start: "Awake",
            turn_end: "Rest",
            tool_post: "AfterTool",
            session_end: "Sleep",
            tool_pre: "BeforeTool",
            tool_failed: Some("ToolFailed"),
        },
        tool_matcher: Some("Grab|Stash"),
        edit_tools: &["Edit"],
        session_env: &["OTHER_SESSION_ID"],
    };

    /// The matcher installed is the vendor's, asserted through a *plan* rather than through the
    /// constant — the defect was that a correct constant reached the wrong vendor's file.
    #[test]
    fn a_vendors_memory_matcher_reaches_its_own_settings_and_claudes_does_not() {
        let plan = super::plan_install(&json!({}), "/bin/amb", Mode::Turn, true, &OTHER);
        let m = plan.settings["hooks"]["BeforeTool"][0]["matcher"]
            .as_str()
            .expect("the memory lane installs with its vendor's matcher");
        assert_eq!(m, "Grab|Stash");
        assert_ne!(
            m,
            CLAUDE_CODE.tool_matcher.expect("claude has one"),
            "Claude's tool vocabulary must not travel to another vendor"
        );
    }

    /// **The seam, driven by a second descriptor rather than by the one that shipped.**
    ///
    /// Every other test in this module pins Claude's shape, so all of them stay green if the
    /// descriptor's fields are quietly ignored and the constants re-hardcoded — which is exactly
    /// the regression the extraction exists to prevent, and exactly the kind this project keeps
    /// finding (a guard asserted against the caller that happens to be right). This installs for
    /// a fabricated vendor and asserts both halves: its spellings appear, and Claude's do not.
    /// The absence row is the load-bearing one.
    #[test]
    fn a_second_vendor_gets_its_own_events_and_paths_and_none_of_claudes() {
        let plan = super::plan_install(&json!({}), "/bin/amb", Mode::Turn, true, &OTHER);
        let hooks = plan
            .settings
            .get("hooks")
            .and_then(Value::as_object)
            .expect("a plan writes a hooks object");
        for mine in [
            "Awake",
            "Rest",
            "AfterTool",
            "Sleep",
            "BeforeTool",
            "ToolFailed",
        ] {
            assert!(
                hooks.contains_key(mine),
                "{mine} missing from {:?}",
                plan.settings
            );
        }
        for claudes in [
            "SessionStart",
            "Stop",
            "PostToolUse",
            "SessionEnd",
            "PreToolUse",
            "PostToolUseFailure",
        ] {
            assert!(
                !hooks.contains_key(claudes),
                "Claude's {claudes} leaked into another vendor's plan: {:?}",
                plan.settings
            );
        }

        // Paths follow the descriptor too, including the two scopes this vendor declines to have.
        let srcs = super::settings_sources(&OTHER, Path::new("/home/u"), Path::new("/repo"));
        let labels: Vec<&str> = srcs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            ["project", "user"],
            "a vendor with no managed and no local file must be given neither"
        );
        assert!(
            srcs.iter()
                .all(|(_, p)| p.to_string_lossy().contains(".other")),
            "{srcs:?}"
        );
    }

    /// The identity lane, with the environment injected so it cannot race the runner: the first
    /// name that answers wins, an empty answer is not an identity, and a vendor whose variables
    /// are all absent yields nothing rather than a blank id.
    #[test]
    fn a_session_id_comes_from_the_first_vendor_variable_that_answers() {
        let two = crate::vendors::Vendor {
            edit_tools: &["Edit"],
            session_env: &["FIRST", "SECOND"],
            ..OTHER
        };
        assert_eq!(
            two.session_id(|k| (k == "SECOND").then(|| "s".to_string())),
            Some("s".into()),
            "a later name still answers when the earlier one is absent"
        );
        assert_eq!(
            two.session_id(|k| Some(if k == "FIRST" { "f" } else { "s" }.to_string())),
            Some("f".into()),
            "and the order is the precedence"
        );
        assert_eq!(
            two.session_id(|_| Some("   ".into())),
            None,
            "blank is not an id"
        );
        assert_eq!(two.session_id(|_| None), None);
    }
    use super::*;

    fn applied(retries: usize, lock_error: Option<&str>, added: &[&str]) -> Applied {
        Applied {
            plan: Plan {
                settings: json!({}),
                added: added.iter().map(|s| (*s).to_string()).collect(),
                removed: vec![],
            },
            locked: lock_error.is_none(),
            lock_error: lock_error.map(str::to_string),
            retries,
        }
    }

    /// **A guard over a count, and all three relaxations survived** (M56). `done.retries > 0`
    /// decides whether an install says it lost a race and re-applied. The line's own comment
    /// states the stakes — silence would make a contended settings file indistinguishable from a
    /// quiet one — and nothing asserted either half, so `>= 0` announced contention on every
    /// quiet install and `== 0` / `< 0` silenced a real one.
    ///
    /// A truth table rather than needles, because both directions are the defect and the
    /// `expected == true` rows prove the renderer reached the line at all (M27's premise trap).
    #[test]
    fn the_retry_line_appears_exactly_when_a_write_was_retried() {
        let path = Path::new("/h/.claude/settings.json");
        for (retries, want) in [(0, false), (1, true), (2, true)] {
            let out = render_applied(&applied(retries, None, &["Stop"]), path, false, "turn");
            assert_eq!(
                out.contains("re-read and re-applied"),
                want,
                "retries={retries} produced {out:?}"
            );
            if want {
                assert!(
                    out.contains(&format!("({retries} time(s))")),
                    "the count itself is reported, not just that there was one: {out:?}"
                );
            }
        }
    }

    /// The other two lines the same renderer owns, and the reason they are asserted here rather
    /// than through the binary: an unlocked write and a no-op are both states a test can build
    /// and neither is a state a test can *provoke* — the lock failure needs a filesystem without
    /// advisory locks, and both were unreachable while this code lived in `main.rs`.
    #[test]
    fn an_unlocked_write_says_so_and_a_no_op_says_nothing_else() {
        let path = Path::new("/h/.claude/settings.json");

        let noisy = render_applied(
            &applied(0, Some("no locks here"), &["Stop"]),
            path,
            false,
            "turn",
        );
        assert!(
            noisy.starts_with("! could not lock"),
            "the weaker write is flagged: {noisy:?}"
        );
        assert!(
            noisy.contains("+ Stop hook (turn)"),
            "and the change is still reported: {noisy:?}"
        );

        let quiet = render_applied(&applied(0, None, &["Stop"]), path, false, "turn");
        assert!(
            !quiet.contains("could not lock"),
            "a locked write raises nothing: {quiet:?}"
        );

        let nothing = render_applied(&applied(0, None, &[]), path, false, "turn");
        assert_eq!(
            nothing,
            format!("no change needed in {}\n", path.display()),
            "a no-op says that and only that"
        );

        let dry = render_applied(&applied(0, None, &["Stop"]), path, true, "turn");
        assert!(
            dry.contains("would update"),
            "a dry run is conditional: {dry:?}"
        );
        assert!(
            !render_applied(&applied(0, None, &["Stop"]), path, false, "turn")
                .contains("would update"),
            "and a real one is not"
        );
    }

    /// The settings temp name must carry the pid, or two sessions interleave on one path.
    ///
    /// The higher-stakes twin of `memory::write`'s test: this file configures Claude Code for
    /// every project on the machine, and the pid was assumed by the cleanup test above and
    /// asserted by nothing.
    #[test]
    fn the_settings_temp_name_is_scoped_to_this_process() {
        let tmp = settings_tmp(Path::new("/h/.claude/settings.json"));
        let name = tmp.file_name().and_then(|n| n.to_str()).expect("utf8 name");
        assert!(
            name.ends_with(&format!(".amb-tmp.{}", std::process::id())),
            "another session picks the same temp path: {name}"
        );
    }

    /// A payload missing the fields, or carrying the wrong types, must degrade rather than panic.
    ///
    /// This runs inside somebody else's session; D9's guarantee is that delivery never breaks one.
    #[test]
    fn the_payload_reader_degrades_instead_of_panicking() {
        assert_eq!(tool_and_file(&json!({})), ("", None));
        assert_eq!(tool_and_file(&json!({"tool_name": 7})), ("", None));
        assert_eq!(
            tool_and_file(&json!({"tool_name": "Read", "tool_input": "not an object"})),
            ("Read", None)
        );
        assert_eq!(
            tool_and_file(&json!({"tool_name": "Edit", "tool_input": {"file_path": "src/a.rs"}})),
            ("Edit", Some("src/a.rs"))
        );
    }

    /// Only the runner's literal `true` is a re-fire; everything else is a first firing.
    ///
    /// Both directions in one table (M27): the `true` row kills an always-deliver guard, the
    /// rest kill always-silent — and the wrongly-typed row pins that a schema this project does
    /// not own degrades toward delivering, never toward dropping mail.
    #[test]
    fn only_a_true_stop_hook_active_reads_as_a_refire() {
        for (payload, expected) in [
            (json!({"stop_hook_active": true}), true),
            (json!({"stop_hook_active": false}), false),
            (json!({"hook_event_name": "Stop"}), false),
            (json!({"stop_hook_active": "true"}), false),
            (Value::Null, false),
        ] {
            assert_eq!(is_stop_refire(&payload), expected, "{payload}");
        }
    }

    /// The extraction degrades to `SessionStart`, never to a panic and never to a tool event.
    #[test]
    fn an_absent_or_alien_event_reads_as_session_start() {
        for (payload, expected) in [
            (json!({"hook_event_name": "Stop"}), "Stop"),
            (json!({"hook_event_name": "PostToolUse"}), "PostToolUse"),
            (json!({"hook_event_name": 7}), "SessionStart"),
            (json!({}), "SessionStart"),
            (Value::Null, "SessionStart"),
        ] {
            assert_eq!(event_name(&payload), expected, "{payload}");
        }
    }

    /// Presence of the key is the whole test: null still counts, absence never does. Both
    /// directions in one table (M27) — the present rows kill an always-participate mutant, the
    /// absent rows an always-silent one.
    #[test]
    fn only_the_agent_id_key_marks_a_subagent() {
        for (payload, expected) in [
            (json!({"agent_id": "abc123"}), true),
            (json!({"agent_id": null}), true),
            (json!({"session_id": "abc123"}), false),
            (Value::Null, false),
        ] {
            assert_eq!(is_subagent(&payload), expected, "{payload}");
        }
    }

    /// A partial install must not say "NOT INSTALLED", and every state must describe itself.
    ///
    /// **The wording is the mechanism here, not decoration.** This line is the only thing standing
    /// between a reader and D69's mistake, so it has to be true in every case it can reach. The
    /// first version decided `installed` versus `absent` in `src/main.rs` with the event list
    /// carried alongside, which made "some hooks missing" print as "NOT INSTALLED" — false, and
    /// false in the direction that sends someone to reinstall rather than to look at which half is
    /// running.
    #[test]
    fn a_partial_memory_install_is_not_described_as_no_install() {
        let exe = "/usr/local/bin/amb";

        let full = plan_install(&serde_json::json!({}), exe, Mode::Monitor, true);
        assert_eq!(memory_state(&full.settings), HookState::Installed);
        assert_eq!(
            memory_state(&full.settings).caveat(),
            None,
            "a healthy install must print nothing; a standing warning is one nobody reads"
        );

        let none = plan_install(&serde_json::json!({}), exe, Mode::Monitor, false);
        let total = memory_state(&none.settings);
        let line = total.caveat().expect("an absent layer must say so");
        assert!(line.contains("NOT INSTALLED"), "{line}");

        // Exactly the machine state that produced D69, minus one event: some, not none.
        let partial = HookState::Incomplete {
            missing: vec!["PreToolUse".to_string()],
            total: 3,
        };
        let line = partial.caveat().expect("a partial layer must say so");
        assert!(
            line.contains("PARTIALLY INSTALLED") && line.contains("PreToolUse"),
            "a partial install must name itself and the missing half, got: {line}"
        );
        assert!(
            !line.contains("NOT INSTALLED"),
            "partial is not absent, and saying so sends the reader to the wrong fix: {line}"
        );

        assert_eq!(HookState::Unknown.as_str(), "unknown");
        assert!(
            HookState::Unknown
                .caveat()
                .is_some_and(|l| l.contains("unverified"))
        );
    }

    fn commands(v: &Value, event: &str) -> Vec<String> {
        v["hooks"][event]
            .as_array()
            .map(|ms| {
                ms.iter()
                    .filter_map(|m| m["hooks"].as_array())
                    .flatten()
                    .filter_map(|h| h["command"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn install_into_an_empty_document_adds_both_events() {
        let p = plan_install(&json!({}), "/usr/local/bin/amb", Mode::Turn, false);
        assert_eq!(
            commands(&p.settings, "SessionStart"),
            ["/usr/local/bin/amb hook turn"]
        );
        assert_eq!(
            commands(&p.settings, "Stop"),
            ["/usr/local/bin/amb hook turn"]
        );
        assert!(!p.is_noop());
    }

    #[test]
    fn session_mode_installs_only_sessionstart() {
        let p = plan_install(&json!({}), "/bin/amb", Mode::Session, false);
        assert_eq!(commands(&p.settings, "SessionStart").len(), 1);
        assert!(
            commands(&p.settings, "Stop").is_empty(),
            "session mode must not touch Stop"
        );
    }

    #[test]
    fn installing_twice_is_idempotent() {
        let once = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let twice = plan_install(&once, "/bin/amb", Mode::Turn, false).settings;
        assert_eq!(once, twice, "a second install must change nothing");
        assert_eq!(
            commands(&twice, "SessionStart").len(),
            1,
            "and must not duplicate the entry"
        );
    }

    #[test]
    fn another_tools_hooks_survive_install_and_uninstall() {
        // The property that matters most: this file belongs to the user, not to us.
        let theirs = json!({
            "hooks": {
                "SessionStart": [{ "hooks": [
                    { "type": "command", "command": "bash /Users/x/.claude/hooks/herdr.sh session" }
                ]}],
                "PreToolUse": [{ "matcher": "Bash", "hooks": [
                    { "type": "command", "command": "node /plugin/run-hook.js guard.sh" }
                ]}]
            },
            "statusLine": { "type": "command", "command": "/Users/x/.claude/statusline.sh" }
        });

        let installed = plan_install(&theirs, "/bin/amb", Mode::Turn, false).settings;
        assert!(
            commands(&installed, "SessionStart")
                .iter()
                .any(|c| c.contains("herdr")),
            "their SessionStart hook must survive"
        );
        assert_eq!(
            commands(&installed, "PreToolUse").len(),
            1,
            "PreToolUse is untouched"
        );
        assert_eq!(
            installed["statusLine"], theirs["statusLine"],
            "unrelated keys are untouched"
        );

        let cleaned = plan_uninstall(&installed).settings;
        assert!(
            !commands(&cleaned, "SessionStart")
                .iter()
                .any(|c| c.contains("amb")),
            "ours must be gone"
        );
        assert!(
            commands(&cleaned, "SessionStart")
                .iter()
                .any(|c| c.contains("herdr")),
            "theirs must remain"
        );
        assert_eq!(cleaned["statusLine"], theirs["statusLine"]);
    }

    #[test]
    fn uninstall_restores_a_document_we_installed_into() {
        let before = json!({ "env": { "FOO": "1" } });
        let installed = plan_install(&before, "/bin/amb", Mode::Turn, false).settings;
        let after = plan_uninstall(&installed).settings;
        assert_eq!(
            after, before,
            "uninstall must return the document to its original shape"
        );
    }

    #[test]
    fn switching_mode_does_not_leave_a_stale_event() {
        let turn = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let session = plan_install(&turn, "/bin/amb", Mode::Session, false).settings;
        assert!(
            commands(&session, "Stop").is_empty(),
            "narrowing from turn to session must remove the Stop hook"
        );
    }

    #[test]
    fn a_corrupt_or_unexpected_document_does_not_panic() {
        // Never panic on someone's settings file: an array, a string, a null, a wrong-typed
        // `hooks` key. Producing something valid beats crashing on their config.
        for weird in [
            json!([1, 2, 3]),
            json!("nonsense"),
            json!(null),
            json!({"hooks": 7}),
        ] {
            let p = plan_install(&weird, "/bin/amb", Mode::Turn, false);
            assert!(
                p.settings["hooks"]["SessionStart"].is_array(),
                "got {:?}",
                p.settings
            );
            let _ = plan_uninstall(&weird);
        }
    }

    #[test]
    fn a_third_party_hook_whose_path_merely_contains_amb_is_not_ours() {
        // The defect this rule exists for. `contains("amb")` matched every one of these, and
        // `uninstall` deleted them — someone else's tooling, gone, with no error.
        for theirs in [
            "/Users/lambert/bin/tool hook start",
            "/opt/amber/run hook pre",
            "node /home/chamber/hooks/run-hook.js hook guard",
            "/usr/local/gambit hook session",
        ] {
            assert!(
                !command_is_ours(theirs),
                "{theirs} belongs to someone else and must survive"
            );
        }
    }

    #[test]
    fn our_own_command_is_recognised_however_it_is_spelled() {
        for ours in [
            "amb hook turn",
            "/usr/local/bin/amb hook session",
            "/Users/x/.cargo/bin/amb hook monitor",
            "'/Users/my name/bin/amb' hook turn",
            // A mode this binary does not know must still be removable, or switching versions
            // strands an entry nothing can clean up.
            "/usr/local/bin/amb hook some-future-mode",
        ] {
            assert!(command_is_ours(ours), "{ours} is ours");
        }
    }

    #[test]
    fn a_path_containing_the_word_hook_still_resolves_to_the_executable() {
        // Splitting at the *first* " hook " would take the exe as "/Users/x/my", not "amb".
        assert!(command_is_ours("/Users/x/my hook tools/amb hook turn"));
        assert!(!command_is_ours("/Users/x/my hook tools/other hook turn"));
    }

    #[test]
    fn a_command_that_is_not_a_hook_invocation_is_not_ours() {
        assert!(!command_is_ours("amb inbox --json"));
        assert!(!command_is_ours("amb hook"), "a mode is required");
        assert!(!command_is_ours("amb hook turn extra"), "one bare token");
        assert!(!command_is_ours(""));
    }

    #[test]
    fn a_third_party_hook_survives_a_real_install_and_uninstall_cycle() {
        // The end-to-end form of the same property: not just the predicate, but the plan.
        let theirs = json!({
            "hooks": { "SessionStart": [{ "hooks": [
                { "type": "command", "command": "/Users/lambert/bin/tool hook start" }
            ]}]}
        });
        let installed = plan_install(&theirs, "/usr/local/bin/amb", Mode::Turn, false).settings;
        assert!(
            commands(&installed, "SessionStart")
                .iter()
                .any(|c| c.contains("lambert")),
            "install must not delete it"
        );
        let cleaned = plan_uninstall(&installed).settings;
        assert_eq!(
            commands(&cleaned, "SessionStart"),
            ["/Users/lambert/bin/tool hook start"],
            "uninstall must leave exactly their hook behind"
        );
    }

    #[test]
    fn an_install_path_containing_a_space_is_quoted_and_still_round_trips() {
        let exe = "/Users/my name/bin/amb";
        let p = plan_install(&json!({}), exe, Mode::Session, false);
        let cmd = &commands(&p.settings, "SessionStart")[0];
        assert!(cmd.starts_with('\''), "the path must be quoted: {cmd}");
        assert!(
            command_is_ours(cmd),
            "and must still be recognised as ours: {cmd}"
        );
        assert!(
            plan_uninstall(&p.settings).settings.get("hooks").is_none(),
            "so uninstall can remove it again"
        );
    }

    #[test]
    fn an_install_path_containing_an_apostrophe_is_still_recognised() {
        // `unquote` is **not** a strict inverse of `quote_exe`: an embedded `'` is emitted as
        // `'\''`, and stripping one outer pair leaves that sequence in place. A reviewer read
        // that as "`/Users/o'brien/amb` makes amb unable to recognise its own hook, so uninstall
        // strands it and install duplicates it" — plausible, and wrong, because the only thing
        // consulted is `file_name`, and the escape lands in a *directory* component.
        //
        // Pinned rather than argued, since the next reader will have the same worry. O'Brien is
        // a real surname, so this is a live path shape, not a contrived one.
        for exe in [
            "/Users/o'brien/bin/amb",
            "/a'b'c/amb",
            "/Users/o'brien/my bin/amb",
        ] {
            let p = plan_install(&json!({}), exe, Mode::Session, false);
            let cmd = &commands(&p.settings, "SessionStart")[0];
            assert!(command_is_ours(cmd), "must be recognised as ours: {cmd}");
            assert!(
                plan_uninstall(&p.settings).settings.get("hooks").is_none(),
                "and removable again: {cmd}"
            );
        }
    }

    #[test]
    fn installing_twice_reports_no_change_the_second_time() {
        // D29. `installing_twice_is_idempotent` proved the *content* was stable and could not see
        // that the plan still claimed to have changed it — and `report_plan` writes whenever a
        // plan is not a no-op, taking a fresh backup each time.
        let before = json!({ "model": "opus" });
        let once = plan_install(&before, "/bin/amb", Mode::Turn, false);
        assert!(!once.is_noop(), "the first install really does change it");

        let twice = plan_install(&once.settings, "/bin/amb", Mode::Turn, false);
        assert!(
            twice.is_noop(),
            "the second must report no change, or it rewrites the file: added={:?}",
            twice.added
        );
        assert!(twice.added.is_empty() && twice.removed.is_empty());
    }

    #[test]
    fn uninstalling_twice_reports_no_change_the_second_time() {
        let installed = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let once = plan_uninstall(&installed);
        assert!(!once.is_noop());
        assert!(plan_uninstall(&once.settings).is_noop());
    }

    /// The dry-run's promise is the *delta*, in both directions. A one-event widening reports
    /// exactly the new entries and no removals; an exe repoint — the change D94 exists to catch
    /// — reports every entry as both removed and re-added, because every one genuinely is. The
    /// first version printed the whole desired state as `+` for any change at all, so a
    /// one-entry edit read as a seven-row rewrite.
    #[test]
    fn a_dry_run_reports_the_delta_not_the_desired_state() {
        // Pure widening: same mode, memory added. The four delivery entries are re-added
        // byte-identical, so they cancel; only the three memory entries are news. (A *mode*
        // switch is not this case — it rewrites every delivery entry's argv, and reporting
        // those as changes is correct.)
        let plain = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let widened = plan_install(&plain, "/bin/amb", Mode::Turn, true);
        assert_eq!(
            widened.added,
            [
                "SessionStart (memory)",
                "PreToolUse (memory)",
                "PostToolUseFailure (memory)"
            ],
            "only the genuinely new entries"
        );
        assert!(
            widened.removed.is_empty(),
            "the four unchanged delivery entries are not removals: {:?}",
            widened.removed
        );

        let turn = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let repointed = plan_install(&turn, "/usr/local/bin/amb", Mode::Turn, false);
        assert_eq!(
            repointed.added.len(),
            4,
            "a repoint rewrites every entry: {:?}",
            repointed.added
        );
        assert_eq!(
            repointed.removed.len(),
            4,
            "and says what it replaced: {:?}",
            repointed.removed
        );
    }

    #[test]
    fn a_real_change_is_still_reported() {
        // The guard must not swing the other way and silence genuine edits.
        let turn = plan_install(&json!({}), "/bin/amb", Mode::Turn, false).settings;
        let narrowed = plan_install(&turn, "/bin/amb", Mode::Session, false);
        assert!(!narrowed.is_noop(), "turn -> session is a real change");
        assert!(narrowed.removed.contains(&"Stop".to_string()));
    }

    #[test]
    fn uninstall_of_a_document_without_hooks_is_a_noop() {
        let doc = json!({ "model": "opus" });
        let p = plan_uninstall(&doc);
        assert!(p.is_noop());
        assert_eq!(p.settings, doc);
    }
    // ── Memory entries ──────────────────────────────────────────────────────

    #[test]
    fn memory_registers_its_own_entry_and_never_extends_the_delivery_command() {
        // D41, and it is structural rather than a convention: hook timeouts are per entry, so a
        // memory layer that hangs must be a separate entry or it burns delivery's budget too.
        let p = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let start = commands(&p.settings, "SessionStart");
        assert!(
            start.contains(&"/bin/amb hook turn".to_string()),
            "{start:?}"
        );
        assert!(
            start.contains(&"/bin/amb hook memory".to_string()),
            "{start:?}"
        );
        assert_eq!(
            start.len(),
            2,
            "two entries, not one merged command: {start:?}"
        );
        // Every entry carries its own timeout, which is what makes the isolation real.
        for list in p.settings["hooks"]["SessionStart"]
            .as_array()
            .into_iter()
            .flatten()
        {
            for e in list["hooks"].as_array().into_iter().flatten() {
                assert_eq!(e["timeout"], json!(HOOK_TIMEOUT_SECS), "{e}");
            }
        }
    }

    #[test]
    fn the_memory_pretooluse_entry_is_narrowed_to_file_tools() {
        let p = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let list = p.settings["hooks"]["PreToolUse"]
            .as_array()
            .expect("PreToolUse should exist");
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0]["matcher"],
            json!(CLAUDE_CODE.tool_matcher.expect("claude has one"))
        );
    }

    #[test]
    fn the_delivery_hooks_are_written_without_a_matcher_as_before() {
        // The matcher support added for memory must not have leaked into the mail entries: an
        // absent matcher and "*" mean the same thing, and changing these would rewrite every
        // existing install for no reason.
        let p = plan_install(&json!({}), "/bin/amb", Mode::Turn, false);
        for event in ["SessionStart", "Stop", "PostToolUse", "SessionEnd"] {
            for m in p.settings["hooks"][event].as_array().into_iter().flatten() {
                assert!(m.get("matcher").is_none(), "{event}: {m}");
            }
        }
    }

    #[test]
    fn installing_with_memory_twice_changes_nothing_the_second_time() {
        let once = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let twice = plan_install(&once.settings, "/bin/amb", Mode::Turn, true);
        assert_eq!(twice.settings, once.settings);
        assert!(
            twice.is_noop(),
            "added {:?} removed {:?}",
            twice.added,
            twice.removed
        );
    }

    #[test]
    fn installing_without_memory_takes_the_memory_entries_back_out() {
        let with = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let without = plan_install(&with.settings, "/bin/amb", Mode::Turn, false);
        let all: Vec<String> = [
            "SessionStart",
            "Stop",
            "PostToolUse",
            "SessionEnd",
            "PreToolUse",
        ]
        .iter()
        .flat_map(|e| commands(&without.settings, e))
        .collect();
        assert!(
            !all.iter().any(|c| c.ends_with(" hook memory")),
            "memory survived: {all:?}"
        );
        assert!(!without.is_noop(), "removing two hooks is a change");
    }

    #[test]
    fn a_removed_memory_entry_is_reported_by_name_not_swallowed() {
        // The summary must not say "PreToolUse" and stay silent about the SessionStart entry it
        // also removed: an install that understates what it did is the defect D29 was about.
        let with = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let without = plan_install(&with.settings, "/bin/amb", Mode::Turn, false);
        assert!(
            without
                .removed
                .contains(&"SessionStart (memory)".to_string()),
            "removed: {:?}",
            without.removed
        );
        assert!(
            without.removed.contains(&"PreToolUse (memory)".to_string()),
            "removed: {:?}",
            without.removed
        );
        assert!(
            !without.removed.contains(&"SessionStart".to_string()),
            "the delivery hook was re-added, so it is not a removal: {:?}",
            without.removed
        );
    }

    #[test]
    fn uninstall_removes_the_memory_entries_too() {
        let with = plan_install(&json!({}), "/bin/amb", Mode::Turn, true);
        let gone = plan_uninstall(&with.settings);
        assert_eq!(gone.settings.get("hooks"), None, "{}", gone.settings);
        assert!(
            gone.removed.iter().any(|r| r.contains("(memory)")),
            "{:?}",
            gone.removed
        );
    }

    #[test]
    fn an_older_binary_still_recognises_the_memory_hook_as_ours() {
        // `command_is_ours` matches `<exe> hook <one-token>` without checking the token against a
        // known set, so a binary that predates memory can still uninstall it. That is the only
        // reason a mixed-version machine can be cleaned up at all.
        assert!(command_is_ours("/bin/amb hook memory"));
        assert!(command_is_ours("'/Users/a b/amb' hook memory"));
        assert!(!command_is_ours("/bin/other hook memory"));
    }

    #[test]
    fn a_foreign_hook_under_pretooluse_survives_a_memory_install() {
        // This machine really has one: a `matcher: "Bash"` entry belonging to another tool.
        let theirs = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/other/rewrite.sh" }] }
            ]}
        });
        let after = plan_install(&theirs, "/bin/amb", Mode::Turn, true).settings;
        let cmds = commands(&after, "PreToolUse");
        assert!(cmds.contains(&"/other/rewrite.sh".to_string()), "{cmds:?}");
        assert!(
            cmds.contains(&"/bin/amb hook memory".to_string()),
            "{cmds:?}"
        );
    }

    /// The hook-state read itself, over a settings document rather than a filesystem.
    #[test]
    fn memory_hooks_are_detected_only_when_all_three_entries_are_ours() {
        let exe = "/usr/local/bin/amb";

        let full = plan_install(&serde_json::json!({}), exe, Mode::Monitor, true);
        let (installed, missing) = memory_hooks(&full.settings);
        assert_eq!(installed.len(), 3, "installed was {installed:?}");
        assert!(missing.is_empty(), "missing was {missing:?}");

        // Delivery-only is the state this machine was actually found in.
        let delivery = plan_install(&serde_json::json!({}), exe, Mode::Monitor, false);
        let (installed, missing) = memory_hooks(&delivery.settings);
        assert!(
            installed.is_empty(),
            "delivery hooks must never read as memory ones; got {installed:?}"
        );
        assert_eq!(missing.len(), 3);

        // Someone else's hook on the same event is not ours, however it is spelled.
        let stranger = serde_json::json!({
            "hooks": { "PreToolUse": [ { "hooks": [
                { "type": "command", "command": "/opt/other/amb-helper hook memory" }
            ] } ] }
        });
        let (installed, _) = memory_hooks(&stranger);
        assert!(
            installed.is_empty(),
            "a stranger's binary must not count as our memory hook (D28)"
        );
    }

    /// A competing write during the cycle is detected, and the plan is re-applied on top of it.
    ///
    /// **Deterministic, not timed.** The planner closure is called once per attempt, so writing to
    /// the file from inside it reproduces exactly the interleaving M31 had to hit with a 0.1 ms
    /// sweep: amb read at T, somebody wrote at T+ε, amb is about to write stale content.
    ///
    /// The assertion that matters is the third: the foreign key **survives**. A fix that merely
    /// retried and then still clobbered would pass a retry-count check and fail this one.
    #[test]
    fn a_competing_write_is_detected_and_the_plan_is_reapplied_over_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"mine\": 1}\n").expect("seed");

        let calls = std::cell::Cell::new(0usize);
        let done = apply(&path, false, |cur| {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 {
                // The uncooperative peer, landing between our read and our write.
                std::fs::write(&path, "{\"mine\": 1, \"theirs\": \"kept\"}\n").expect("peer write");
            }
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("apply");

        assert_eq!(
            done.retries, 1,
            "the competing write must force exactly one re-read"
        );
        assert_eq!(
            calls.get(),
            2,
            "and the plan must be recomputed, not reused"
        );

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read back"))
                .expect("valid json");
        assert_eq!(
            after["theirs"], "kept",
            "the competing writer's key must survive — this is the whole defect (M31)"
        );
        assert!(
            after.get("hooks").is_some(),
            "and our own change must still land: {after}"
        );
    }

    /// With nobody competing, the cycle runs once and reports no retries.
    ///
    /// The presence row for the test above. Without it, an `apply` that retried on *every* run
    /// would satisfy the retry assertion there and be badly wrong here.
    #[test]
    fn an_uncontended_apply_runs_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let done = apply(&path, false, |cur| {
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("apply");
        assert_eq!(done.retries, 0);
        assert!(done.locked, "a plain temp directory must support the lock");
        assert!(done.lock_error.is_none());
        assert!(path.exists(), "and it must actually have written");
    }

    /// A dry run reads and plans but writes nothing, contended or not.
    #[test]
    fn a_dry_run_writes_nothing_and_needs_no_retry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let done = apply(&path, true, |cur| {
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("apply");
        assert!(!done.plan.is_noop(), "the plan is still computed");
        assert_eq!(done.retries, 0);
        assert!(!path.exists(), "dry run must not create the file");
    }

    /// A file rewritten faster than amb can read it gives up and says so, rather than spinning.
    #[test]
    fn endless_contention_is_reported_rather_than_looped_forever() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"n\": 0}\n").expect("seed");

        let calls = std::cell::Cell::new(0usize);
        let err = apply(&path, false, |cur| {
            let n = calls.get();
            calls.set(n + 1);
            // Writes on *every* attempt, so the check can never succeed.
            // `n + 1`, so the very first peer write already differs from the seed. Writing
            // `n` made attempt 0 byte-identical to it, the check passed, and this test proved
            // nothing — the fixture never reached the branch it names (M17).
            std::fs::write(&path, format!("{{\"n\": {}}}\n", n + 1)).expect("peer write");
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect_err("must not succeed");

        assert_eq!(
            calls.get(),
            MAX_RMW_ATTEMPTS,
            "it must stop at the bound rather than spinning in somebody's terminal"
        );
        assert!(
            err.to_string().contains("changed under every one of"),
            "and name the condition: {err}"
        );

        // **The give-up path is the only one where cleanup is observable**, and that is why this
        // assertion lives here rather than in the retry test below. Within one process the temp
        // name is fixed, so a retry reuses and then renames away the same file — deleting the
        // cleanup survives that test entirely. Here nothing renames, so a leaked scratch file
        // stays beside the user's configuration. Confirmed by deleting `remove_file`.
        assert!(
            scratch_beside(dir.path()).is_empty(),
            "giving up must not leave scratch next to settings.json: {:?}",
            scratch_beside(dir.path())
        );
    }

    /// Temp files amb left in a directory.
    fn scratch_beside(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("readdir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("amb-tmp"))
            .collect()
    }

    /// The backup captures what was there *before* the change.
    #[test]
    fn the_backup_holds_the_previous_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"before\": true}\n").expect("seed");
        apply(&path, false, |cur| {
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("apply");
        let backup =
            std::fs::read_to_string(path.with_extension("json.amb-backup")).expect("backup exists");
        assert!(backup.contains("\"before\""), "got {backup}");
    }

    /// No temp file is left behind once a retry succeeds.
    ///
    /// **This cannot see a missing cleanup, and says so rather than implying coverage.** The temp
    /// name carries the pid, so both attempts in one process use it and the successful `rename`
    /// consumes it whether or not the discarded attempt removed it — deleting `remove_file` leaves
    /// this green. The give-up path is where that guard is observable, and
    /// `endless_contention_is_reported_rather_than_looped_forever` asserts it there.
    ///
    /// Kept because it pins the property a *future* per-attempt temp name would break.
    #[test]
    fn no_scratch_file_survives_a_retry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"mine\": 1}\n").expect("seed");
        let calls = std::cell::Cell::new(0usize);
        apply(&path, false, |cur| {
            let n = calls.get();
            calls.set(n + 1);
            if n == 0 {
                std::fs::write(&path, "{\"mine\": 2}\n").expect("peer write");
            }
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("apply");

        let leftovers = scratch_beside(dir.path());
        assert!(leftovers.is_empty(), "scratch left behind: {leftovers:?}");
    }

    /// Helper: a settings document with one `amb` hook on `event`.
    fn with_hook(event: &str, command: &str) -> Value {
        json!({"hooks": {event: [{"hooks": [{"type": "command", "command": command}]}]}})
    }

    /// D77's actual shape: the same entry in the user file and a project-local file.
    ///
    /// **A truth table, because the two halves fail differently.** A detector that reported every
    /// entry would satisfy any single positive row, and one that reported none would satisfy the
    /// negative rows — only having both proves it discriminates.
    #[test]
    fn a_hook_in_two_scopes_is_a_duplicate_and_one_in_one_scope_is_not() {
        let cmd = "/usr/local/bin/amb hook memory";

        // The defect: merged, so it fires twice per SessionStart.
        let dupes = duplicate_hooks(&[
            ("user".into(), with_hook("SessionStart", cmd)),
            ("project local".into(), with_hook("SessionStart", cmd)),
        ]);
        assert_eq!(dupes.len(), 1, "got {dupes:?}");
        assert_eq!(dupes[0].event, "SessionStart");
        assert_eq!(
            dupes[0].sources,
            vec!["user".to_string(), "project local".to_string()],
            "both scopes must be named, or the reader cannot know which file to edit"
        );

        // The healthy case, which is the row that stops this passing vacuously.
        assert!(
            duplicate_hooks(&[("user".into(), with_hook("SessionStart", cmd))]).is_empty(),
            "one scope is not a duplicate"
        );
    }

    /// Two `amb` entries that differ only in mode are two jobs, not one repeated.
    ///
    /// `amb hook turn` and `amb hook memory` are registered on the same event on purpose — D41
    /// requires memory to carry its own entry so its timeout is its own. Keying on the executable
    /// rather than the whole command line would report that deliberate arrangement as a fault.
    #[test]
    fn two_modes_on_one_event_are_not_a_duplicate() {
        let dupes = duplicate_hooks(&[(
            "user".into(),
            json!({"hooks": {"SessionStart": [
                {"hooks": [{"type": "command", "command": "/bin/amb hook turn"}]},
                {"hooks": [{"type": "command", "command": "/bin/amb hook memory"}]}
            ]}}),
        )]);
        assert!(
            dupes.is_empty(),
            "different modes are different jobs: {dupes:?}"
        );
    }

    /// The same command on two different events is not a duplicate either.
    #[test]
    fn one_command_on_two_events_is_not_a_duplicate() {
        let cmd = "/bin/amb hook turn";
        let dupes = duplicate_hooks(&[(
            "user".into(),
            json!({"hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": cmd}]}],
                "Stop":         [{"hooks": [{"type": "command", "command": cmd}]}]
            }}),
        )]);
        assert!(dupes.is_empty(), "per-event, not per-command: {dupes:?}");
    }

    /// One file listing the same entry twice is a duplicate too — merging is not the only cause.
    #[test]
    fn one_scope_listing_an_entry_twice_is_a_duplicate() {
        let cmd = "/bin/amb hook turn";
        let dupes = duplicate_hooks(&[(
            "user".into(),
            json!({"hooks": {"Stop": [
                {"hooks": [{"type": "command", "command": cmd}]},
                {"hooks": [{"type": "command", "command": cmd}]}
            ]}}),
        )]);
        assert_eq!(dupes.len(), 1, "got {dupes:?}");
        assert_eq!(
            dupes[0].sources,
            vec!["user".to_string(), "user".to_string()]
        );
    }

    /// A stranger's duplicated hook is not ours to report (D28).
    #[test]
    fn a_foreign_hook_registered_twice_is_not_reported() {
        let cmd = "/opt/othertool/tool hook start";
        let dupes = duplicate_hooks(&[
            ("user".into(), with_hook("Stop", cmd)),
            ("project".into(), with_hook("Stop", cmd)),
        ]);
        assert!(dupes.is_empty(), "only amb's own entries: {dupes:?}");
    }

    /// The sources list covers every scope the platform merges that a CLI can find.
    #[test]
    fn the_scope_list_covers_what_the_platform_merges() {
        let got = settings_sources(Path::new("/home/u"), Path::new("/repo"));
        let labels: Vec<&str> = got.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["managed", "project local", "project", "user"]);
        assert!(
            got.iter()
                .any(|(_, p)| p.ends_with(".claude/settings.local.json"))
        );
        assert!(
            got.iter()
                .any(|(_, p)| p == Path::new("/home/u/.claude/settings.json")),
            "the user file must be rooted at the HOME passed in, not the ambient one: {got:?}"
        );
    }

    /// Every mode the CLI documents must parse, and nothing else may.
    ///
    /// **The variants are asserted throughout this file; the parser that produces them was
    /// asserted nowhere.** `--mode` is a `String`, so `Mode::parse` *is* the contract check behind
    /// `amb install --mode <x>`, and its only caller is `src/main.rs`. Deleting the `"session"`
    /// arm reddened nothing (M39). That is M20's arithmetic — count the layers a rule passes
    /// through, count the layers that assert it, and suspect the outermost, because it is the one
    /// no cheap unit test happens to cover.
    ///
    /// A round trip rather than three literals, so a fourth mode cannot arrive with a parse arm
    /// spelled differently from its `as_str`.
    #[test]
    fn every_mode_round_trips_through_its_own_spelling() {
        for mode in [Mode::Session, Mode::Turn, Mode::Monitor] {
            let spelling = mode.as_str();
            assert_eq!(
                Mode::parse(spelling),
                Some(mode),
                "{spelling} must parse back to itself"
            );
            assert!(
                !mode.events(&crate::vendors::CLAUDE_CODE).is_empty(),
                "{spelling} installs no event"
            );
        }
        assert_eq!(Mode::parse("Session"), None, "matching is case-sensitive");
        assert_eq!(Mode::parse(""), None);
        assert_eq!(
            Mode::parse("watch"),
            None,
            "an unknown mode is rejected rather than defaulted"
        );
    }

    /// A path that needs quoting is quoted, whatever made it need quoting.
    ///
    /// **The apostrophe case was reached by a test and asserted by none.**
    /// `an_install_path_containing_an_apostrophe_is_still_recognised` installs
    /// `/Users/o'brien/bin/amb` and checks that `command_is_ours` accepts it and `plan_uninstall`
    /// removes it — both of which pass on an *unquoted* command line, because `unquote` is a no-op
    /// there and `file_name` is still `amb`. The sibling space test asserts the quoting. One rule,
    /// two instances, guarded at one of them: D90's shape, and `replace || with && in quote_exe`
    /// survived the whole suite (M39).
    ///
    /// What it costs is an unbalanced quote in `~/.claude/settings.json`, so the shell cannot parse
    /// the hook command and it never runs — the silent failure `quote_exe`'s own docstring names.
    #[test]
    fn a_path_that_needs_quoting_is_quoted_whatever_made_it_need_quoting() {
        // Each needs quoting for a different reason and none of them contains a space, which is
        // the only reason the existing tests ever exercise.
        for exe in [
            "/Users/o'brien/bin/amb",
            "/Users/say\"hi\"/amb",
            "/tmp/a\tb/amb",
        ] {
            let doc = plan_install(&json!({}), exe, Mode::Session, false).settings;
            let cmd = &commands(&doc, "SessionStart")[0];
            assert!(cmd.starts_with('\''), "must be quoted: {cmd}");
            assert!(command_is_ours(cmd), "and still recognised as ours: {cmd}");
        }
        // The control row, and it is what makes the rows above mean anything. Without it this
        // asserts only that quoting happens, never that it is *conditional* — and a `quote_exe`
        // that quoted unconditionally would pass every line above.
        let doc = plan_install(&json!({}), "/usr/local/bin/amb", Mode::Session, false).settings;
        let plain = &commands(&doc, "SessionStart")[0];
        assert!(
            !plain.starts_with('\''),
            "a plain path is left alone: {plain}"
        );
    }

    /// `doctor` reads this to find a hook invoking a *different* `amb`, and nothing here read it.
    ///
    /// **Six of this module's seventeen survivors were in this one function** (M39), including
    /// replacing its whole body with `vec![]`. It is the only thing that can see the stale-binary
    /// condition D94 records as having recurred five times: `command_is_ours` matches the file
    /// *name*, so a hook pointing at last month's build is still "ours" and `HookState` calls it
    /// `Installed`. Returning the path is what lets `doctor` compare fingerprints — and `doctor`'s
    /// own tests build their fixtures directly, so the producer between them ran under no
    /// assertion at all. M37's shape: the reader is tested, the writer is not.
    #[test]
    fn every_hook_of_ours_is_reported_with_the_executable_it_invokes() {
        let foreign = json!({
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "/other/tool hook go"}]}]
            }
        });
        let doc = plan_install(&foreign, "/opt/one/amb", Mode::Session, false).settings;
        assert_eq!(
            our_hook_exes(&doc),
            vec![("SessionStart".to_string(), "/opt/one/amb".to_string())],
            "ours reported with its path, and the foreign hook not reported at all"
        );

        // Unquoted, because `doctor` compares this against a path on disk. A quoted string would
        // never match and the stale-binary check would report BAD on a healthy install.
        let spaced = plan_install(&json!({}), "/Users/my name/amb", Mode::Session, false).settings;
        assert_eq!(
            our_hook_exes(&spaced),
            vec![("SessionStart".to_string(), "/Users/my name/amb".to_string())],
            "the quoting `plan_install` added is undone again here"
        );

        // Degrades rather than panicking, on documents we did not write.
        assert!(our_hook_exes(&json!({})).is_empty());
        assert!(our_hook_exes(&json!({"hooks": "nonsense"})).is_empty());
    }

    /// Reading settings must tell absent, empty, valid and unreadable apart.
    ///
    /// **The whole function replaced by `Ok(Default::default())` survived**, along with both of its
    /// guards in both directions — six mutants in one small function (M39). Its three callers are
    /// `doctor` twice and `main` once, all on paths no test drives, so every branch here was an
    /// unasserted answer to "is this machine configured", which is the question `doctor` exists to
    /// answer.
    #[test]
    fn a_settings_file_reads_as_absent_empty_valid_or_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Absent is an empty document rather than an error: no settings file is an ordinary state.
        assert_eq!(
            read_settings(&dir.path().join("nope.json")).expect("absent"),
            json!({})
        );

        // Empty and whitespace-only are the same as absent. `serde_json` rejects both, so without
        // the guard an editor that saved nothing turns every `amb install` into a parse error.
        for body in ["", "   \n\t "] {
            let path = dir.path().join("empty.json");
            std::fs::write(&path, body).expect("seed");
            assert_eq!(
                read_settings(&path).expect("empty"),
                json!({}),
                "body {body:?}"
            );
        }

        // Real content comes back, which is the row that sees a function turned into a constant.
        let path = dir.path().join("real.json");
        std::fs::write(&path, "{\"hooks\": {\"Stop\": []}}").expect("seed");
        assert_eq!(
            read_settings(&path).expect("valid"),
            json!({"hooks": {"Stop": []}})
        );

        // Malformed is an error, never an empty document. Reading somebody's unparseable settings
        // as `{}` and then writing our own over them is the worst outcome available here.
        std::fs::write(&path, "{not json").expect("seed");
        assert!(
            read_settings(&path).is_err(),
            "malformed must not read as empty"
        );

        // Present-but-unreadable is also an error, and this is the row that separates the
        // `NotFound` guard from `true`: a directory is not a missing file, and treating it as one
        // has `doctor` report a healthy empty configuration for a path it cannot read at all.
        assert!(
            read_settings(dir.path()).is_err(),
            "a directory is not an absent file"
        );
    }

    /// An existing but empty settings file installs cleanly, and an unreadable one fails on read.
    ///
    /// Two guards on the same distinction, one in [`apply`] and one in `read_raw`, both survivors
    /// (M39). The first decides whether an empty file is a document or a parse error; the second
    /// decides whether a read failure that is *not* a missing file may be treated as one — and if
    /// it may, `apply` proceeds to write over whatever it could not read.
    #[test]
    fn an_empty_settings_file_installs_and_an_unreadable_one_fails_before_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "\n").expect("seed");

        let done = apply(&path, false, |cur| {
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect("an empty settings file is a document with nothing in it");
        assert!(!done.plan.is_noop(), "it must actually install");
        assert!(
            std::fs::read_to_string(&path)
                .expect("written")
                .contains("SessionStart"),
            "and the hooks must reach the file"
        );

        // Asserting *where* it fails, not merely that it does. Both the correct code and the
        // mutant return an error for a directory — the mutant just gets there by trying to rename
        // a file over it. Only the context distinguishes them.
        let err = apply(dir.path(), false, |cur| {
            plan_install(cur, "/usr/local/bin/amb", Mode::Turn, false)
        })
        .expect_err("a directory is not a settings file");
        assert!(
            matches!(&err, Error::Io { context, .. } if context.contains("reading")),
            "must fail on the read rather than attempt a write: {err:?}"
        );
    }
}

//! The hook must never break a session.
//!
//! It is installed globally, so it runs in every Claude Code session on the machine — including
//! sessions in projects that have never heard of this tool. D9 makes that a hard requirement:
//! "mail delivery must never break a session." These tests assert it against the real binary,
//! under the conditions that would break a careless implementation.

mod common;
use common::Board;

const START: &str = r#"{"hook_event_name":"SessionStart"}"#;
const STOP: &str = r#"{"hook_event_name":"Stop"}"#;

/// The context a hook injected, or `None` when it stayed silent.
fn injected(out: &str) -> Option<String> {
    if out.trim().is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("hook emits valid JSON");
    Some(
        v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("context")
            .to_string(),
    )
}

#[test]
fn with_no_board_the_hook_says_nothing_and_creates_nothing() {
    // The property that makes a global install safe for people who never use amb.
    let b = Board::new();
    let (code, out) = b.hook("uuid-a", "turn", START);

    assert_eq!(code, 0, "a hook must always succeed");
    assert!(
        out.is_empty(),
        "and emit nothing when there is no board, got {out:?}"
    );
    assert!(
        !std::path::Path::new(&b.db).exists(),
        "it must not create a database for a session that never uses one"
    );
}

#[test]
fn with_mail_waiting_the_hook_emits_a_valid_envelope() {
    let b = Board::new();
    b.run("uuid-alice", &["register", "--name", "alice"]);
    b.run("uuid-bob", &["register", "--name", "bob"]);
    b.run(
        "uuid-alice",
        &[
            "send",
            "bob",
            "--subject",
            "wake up",
            "--body",
            "there is work",
        ],
    );

    let (code, out) = b.hook("uuid-bob", "turn", START);
    assert_eq!(code, 0);

    let ctx = injected(&out).expect("mail must be injected");
    assert!(
        ctx.contains("wake up"),
        "the message must be in the injected context"
    );
    assert!(
        ctx.contains("from \"alice\""),
        "and attributed by name, quoted — the name is the sender's to choose"
    );
    assert!(
        ctx.contains("amb reply"),
        "SessionStart must also teach the command surface"
    );
}

#[test]
fn a_stop_event_delivers_without_repeating_the_primer() {
    let b = Board::new();
    b.run("uuid-alice", &["register", "--name", "alice"]);
    b.run("uuid-bob", &["register", "--name", "bob"]);
    b.run(
        "uuid-alice",
        &["send", "bob", "--subject", "later", "--body", "b"],
    );

    let (code, out) = b.hook("uuid-bob", "turn", STOP);
    assert_eq!(code, 0);

    let ctx = injected(&out).expect("mail must be injected");
    assert!(ctx.contains("later"), "the message is delivered");
    assert!(
        !ctx.contains("You are on the agent messageboard"),
        "but the primer is not repeated"
    );
}

#[test]
fn an_empty_inbox_emits_nothing_on_stop() {
    // Silence is the common case at a turn boundary; it must cost nothing and add no context.
    let b = Board::new();
    b.run("uuid-bob", &["register", "--name", "bob"]);

    let (code, out) = b.hook("uuid-bob", "turn", STOP);
    assert_eq!(code, 0);
    assert!(
        out.is_empty(),
        "an empty inbox must inject nothing, got {out:?}"
    );
}

#[test]
fn a_subagent_is_ignored() {
    // A subagent has no independent inbox and would register as a phantom peer.
    let b = Board::new();
    b.run("uuid-alice", &["register", "--name", "alice"]);
    b.run("uuid-bob", &["register", "--name", "bob"]);
    b.run(
        "uuid-alice",
        &["send", "bob", "--subject", "s", "--body", "b"],
    );

    let (code, out) = b.hook(
        "uuid-bob",
        "turn",
        r#"{"hook_event_name":"SessionStart","agent_id":"sub-1"}"#,
    );
    assert_eq!(code, 0);
    assert!(
        out.is_empty(),
        "a subagent must receive nothing, got {out:?}"
    );
}

#[test]
fn hostile_stdin_never_produces_a_failure() {
    let b = Board::new();
    b.run("uuid-bob", &["register", "--name", "bob"]);

    for payload in [
        "",
        "not json at all",
        "{",
        "null",
        "[]",
        r#"{"hook_event_name":123}"#,
    ] {
        let (code, _) = b.hook("uuid-bob", "turn", payload);
        assert_eq!(code, 0, "payload {payload:?} must not fail the hook");
    }
}

#[test]
fn a_corrupt_database_does_not_fail_the_hook() {
    let b = Board::new();
    std::fs::write(&b.db, b"this is not a sqlite file at all").expect("write junk");

    let (code, out) = b.hook("uuid-b", "turn", START);
    assert_eq!(code, 0, "an unreadable board must not break the session");
    assert!(out.is_empty());
}

#[test]
fn a_missing_identity_does_not_fail_the_hook() {
    let b = Board::new();
    b.run("uuid-bob", &["register", "--name", "bob"]);

    // An empty agent means AMB_AGENT is removed entirely.
    let (code, out) = b.hook("", "turn", START);
    assert_eq!(
        code, 0,
        "a session with no identity must still start normally"
    );
    assert!(out.is_empty());
}

/// Seed a settings.json inside this board's private HOME.
fn seed_settings(b: &Board, body: &str) -> std::path::PathBuf {
    let path = b.home.join(".claude").join("settings.json");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, body).expect("seed settings");
    path
}

#[test]
fn install_then_uninstall_round_trips_a_real_settings_file() {
    let b = Board::new();
    let original = r#"{"model":"opus","hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"bash /other/tool.sh"}]}]}}"#;
    let settings = seed_settings(&b, original);

    b.run("uuid-a", &["install", "--mode", "turn"]);
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).expect("read")).expect("json");
    let dump = after.to_string();
    assert!(dump.contains("amb hook turn"), "our hook must be installed");
    assert!(
        dump.contains("/other/tool.sh"),
        "the other tool's hook must survive"
    );
    assert_eq!(after["model"], "opus", "unrelated settings must survive");
    assert!(
        settings.with_extension("json.amb-backup").exists(),
        "a backup must be written before touching the user's global config"
    );

    b.run("uuid-a", &["uninstall"]);
    let restored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).expect("read")).expect("json");
    assert_eq!(
        restored,
        serde_json::from_str::<serde_json::Value>(original).expect("json"),
        "uninstall must return the file to exactly its original content"
    );
}

#[test]
fn installing_does_not_create_a_board() {
    // What makes a machine-wide install safe: after `amb install`, every session on the machine
    // runs the hook, and every one of them short-circuits until somebody actually sends
    // something. Installing must therefore not create the database itself.
    let b = Board::new();
    b.run("", &["install", "--mode", "monitor"]);

    assert!(
        !std::path::Path::new(&b.db).exists(),
        "install must not create a board"
    );
    assert!(
        b.home.join(".claude").join("settings.json").exists(),
        "but it must write the hooks"
    );
}

#[test]
fn installing_needs_no_agent_identity() {
    // Installing is machine setup, not participation. Requiring an identity would make it fail
    // outside a Claude session, which is exactly where a user would run it.
    let b = Board::new();
    b.run("", &["uninstall"]);
}

#[test]
fn dry_run_writes_nothing() {
    let b = Board::new();
    let settings = seed_settings(&b, "{}");

    let v = b.json("uuid-a", &["install", "--dry-run"]);
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["changed"], true, "it must report what it would do");
    assert_eq!(
        std::fs::read_to_string(&settings).expect("read"),
        "{}",
        "but write nothing"
    );
}

// ── The memory layer must not be able to degrade delivery ────────────────────
//
// D9 says mail delivery never breaks a session. Memory puts new work — reading a vault that may
// sit on someone else's disk, parsing files a human edits — behind the same guarantee. The
// isolation is *structural*: memory registers its own hook entry, and hook timeouts are per
// entry, so a memory layer that hangs burns its own budget. These assert the parts a process can
// show; `hooks::tests` asserts the entry separation itself.

const MEMORY_START: &str = r#"{"hook_event_name":"SessionStart"}"#;

#[test]
fn the_memory_hook_with_no_board_says_nothing_and_creates_nothing() {
    let b = Board::new();
    let (code, out) = b.mem_hook("uuid-a", MEMORY_START);
    assert_eq!(code, 0, "a hook must always succeed");
    assert!(out.is_empty(), "no board, no output: {out:?}");
    assert!(
        !std::path::Path::new(&b.db).exists(),
        "and no database for a session that never uses one"
    );
    assert!(
        !b.vault.exists(),
        "nor a vault directory for a session that never records anything"
    );
}

#[test]
fn the_memory_hook_survives_hostile_stdin() {
    let b = Board::new();
    b.run("uuid-a", &["register"]); // a board now exists
    for payload in [
        "",
        "not json at all",
        "{}",
        "[]",
        "null",
        r#"{"hook_event_name":42}"#,
        r#"{"hook_event_name":"PreToolUse","tool_input":"not an object"}"#,
        r#"{"hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":null}}"#,
    ] {
        let (code, _) = b.mem_hook("uuid-a", payload);
        assert_eq!(code, 0, "payload {payload:?} must not fail the hook");
    }
}

#[test]
fn an_unreadable_vault_is_reported_as_an_outage_rather_than_an_empty_one() {
    // Believing you are capturing when you are not is the failure claude-mem's own corpus
    // demonstrates: 85 queue items and 43 sessions stuck from one fortnight, after which the
    // system ran three more months and added 80,000 rows without ever surfacing them.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let b = Board::new();
        // A board must exist first: with none, silence is the *correct* answer and would mask
        // what this test is about.
        b.run("uuid-a", &["register"]);
        let dir = b.vault.join("projects").join("nest");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("2026-01-01-x.md"),
            "---\nscope: nest\ntitle: t\n---\n\nx\n",
        )
        .expect("write");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).expect("chmod");

        let (code, out) = b.mem_hook("uuid-a", MEMORY_START);
        // Restore before any assertion can panic, or the temporary directory cannot be removed.
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));

        assert_eq!(code, 0, "an unreadable vault must not fail the hook");
        let text = injected(&out).expect("an outage is stated, not swallowed");
        assert!(text.contains("outage"), "{text}");
        assert!(
            !text.contains("no prior observations"),
            "an outage must not read as an empty vault: {text}"
        );
    }
}

#[test]
fn a_vault_path_that_is_a_file_does_not_fail_the_hook() {
    let b = Board::new();
    b.run("uuid-a", &["register"]);
    std::fs::write(&b.vault, "I am not a directory").expect("write");
    let (code, _) = b.mem_hook("uuid-a", MEMORY_START);
    assert_eq!(code, 0);
}

#[test]
fn a_broken_vault_does_not_stop_mail_from_being_delivered() {
    // The property D9 is actually about. Memory is a separate entry, so the delivery hook must
    // not so much as look at the vault — and this fails if anything ever merges the two.
    let b = Board::new();
    b.run("uuid-alice", &["register", "--name", "alice"]);
    b.run("uuid-bob", &["register", "--name", "bob"]);
    b.run(
        "uuid-alice",
        &[
            "send",
            "bob",
            "--subject",
            "still works",
            "--body",
            "mail is fine",
        ],
    );
    std::fs::write(&b.vault, "a file where a vault should be").expect("write");

    // The delivery hook, with AMB_VAULT pointed at that nonsense.
    let mut c = b.cmd_mem("uuid-bob");
    c.args(["hook", "turn"]);
    let (code, out) = common::with_stdin(c, STOP);
    assert_eq!(code, 0);
    let text = injected(&out).expect("mail must still be delivered");
    assert!(text.contains("still works"), "{text}");
}

#[test]
fn the_delivery_hook_never_writes_to_the_vault_and_the_memory_hook_never_marks_mail_delivered() {
    // Separation of the two concerns, asserted rather than assumed. Two hooks sharing one binary
    // is exactly the arrangement where one quietly starts doing the other's work.
    let b = Board::new();
    b.run("uuid-alice", &["register", "--name", "alice"]);
    b.run("uuid-bob", &["register", "--name", "bob"]);
    b.run(
        "uuid-alice",
        &[
            "send",
            "bob",
            "--subject",
            "unread",
            "--body",
            "still unread",
        ],
    );

    let mut c = b.cmd_mem("uuid-bob");
    c.args(["hook", "turn"]);
    common::with_stdin(c, STOP);
    assert!(
        !b.vault.exists(),
        "the delivery hook must not create a vault"
    );

    // And the memory hook must not consume Bob's mail: it is still deliverable afterwards.
    b.mem_hook("uuid-bob", MEMORY_START);
    let unread: i64 = b
        .sqlite()
        .query_row(
            "SELECT count(*) FROM reads WHERE agent = 'uuid-bob' AND delivered_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        unread, 1,
        "one offer, from the delivery hook only — memory must not touch read state"
    );
}

/// A binary older than the board must say so, and must still exit 0.
///
/// **This is the failure D48 records happening three times, and until now it was unreachable.**
/// `Error::SchemaVersion` fires only when the board is newer than the binary; `hook_main` used to
/// discard it and exit 0, so the session saw an empty inbox and nothing else. A detection that
/// cannot reach the person is not a detection (D58).
///
/// The board is stamped forward directly, which is exactly what a newer binary's migration does.
#[test]
fn a_binary_older_than_the_board_says_so_instead_of_going_quiet() {
    let b = Board::new();
    b.run("alice", &["register", "--name", "alice"]);

    let ahead = {
        let conn = b.sqlite();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read version");
        conn.execute_batch(&format!("PRAGMA user_version = {}", v + 1))
            .expect("stamp forward");
        v + 1
    };

    let (code, out) = b.hook("alice", "session", START);
    assert_eq!(
        code, 0,
        "D9 is absolute: a hook exits 0 even while reporting"
    );

    let ctx = injected(&out).expect("the session must be told, not left with an empty inbox");
    assert!(
        ctx.contains("not receiving mail"),
        "the notice must name the consequence, not just the condition: {ctx}"
    );
    assert!(
        ctx.contains(&format!("schema {ahead}")),
        "it must name the board's version: {ctx}"
    );
    assert!(
        ctx.contains("cargo install"),
        "a report without the fix leaves the reader where they started: {ctx}"
    );
    // The looping advice this notice exists to avoid. `Error::SchemaVersion` says deleting the
    // board is safe; here that is wrong, because the stale copy recreates it at the old version.
    assert!(
        !ctx.contains("deleting is safe") && !ctx.contains("safe to delete"),
        "the notice must not repeat the advice that loops: {ctx}"
    );
}

/// Every other failure stays quiet. The fix must not turn one silence into constant noise.
#[test]
fn an_ordinary_hook_failure_still_says_nothing() {
    let b = Board::new();
    b.run("alice", &["register", "--name", "alice"]);
    // A corrupt board fails on open, like any transient or unactionable error.
    std::fs::write(&b.db, b"this is not a database").expect("clobber the board");

    let (code, out) = b.hook("alice", "session", START);
    assert_eq!(code, 0);
    assert_eq!(
        injected(&out),
        None,
        "only a stale binary speaks; anything else would be noise in every session: {out:?}"
    );
}

/// A hook whose arguments this build cannot parse still exits 0 and still says nothing.
///
/// **The layer D9's guarantee did not reach** (D97). Every other test in this file drives
/// `hook <mode>` correctly and then breaks something at *runtime* — hostile stdin, a corrupt
/// board, no identity, an unreadable vault. All of those enter `hook_main`, which is where the
/// exit-0 rule is written and mutation-tested. **A malformed argv never gets there**: `clap`
/// terminates the process during parsing, and its failure code is `2`.
///
/// `2` is not an arbitrary number to Claude Code. It is the one code the hook runner treats as a
/// *blocking* error: on `Stop` it "prevents Claude from stopping; continues the conversation",
/// and on `PreToolUse` it blocks the tool call. So a hook entry carrying a flag this build does
/// not know — one written by another version, or edited by hand, which D69 and D94 record as a
/// recurring condition — would have wedged the session rather than failing quietly.
///
/// M20's arithmetic: count the layers a rule passes through, count the layers that assert it. The
/// missing one was the outermost, because a test that drives the binary correctly cannot see it.
#[test]
fn a_hook_invoked_with_arguments_this_build_cannot_parse_exits_zero_and_is_silent() {
    let b = Board::new();
    b.run("alice", &["register", "--name", "alice"]);

    for bad in [
        // A flag from a hook entry this build does not know.
        vec!["hook", "turn", "--mode", "turn"],
        vec!["hook", "--unknown-flag"],
        // No mode at all: a required positional is missing.
        vec!["hook"],
    ] {
        let out = b.try_run("alice", &bad);
        assert_eq!(
            out.status.code(),
            Some(0),
            "`amb {}` must exit 0 — exit 2 blocks a Stop hook and wedges the session",
            bad.join(" ")
        );
        assert!(
            out.stdout.is_empty(),
            "a hook that cannot parse must emit nothing; stdout on SessionStart becomes context: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    // The presence row, so this is a truth table rather than a list of absences (M27): a
    // well-formed hook still works. Without it, a binary that exited 0 and printed nothing for
    // *every* invocation would pass everything above.
    let (code, _) = b.hook("alice", "session", START);
    assert_eq!(code, 0, "a well-formed hook still runs");
}

/// A Stop re-fire (`stop_hook_active: true`) gets silence, even with mail waiting.
///
/// The runner counts a Stop hook that injects context as blocking the turn from ending: it wakes
/// the model, the model answers, Stop fires again with `stop_hook_active: true`. Answering that
/// firing loops — observed at machine scale during two stale-binary windows (2026-08-27 and
/// 2026-08-31), when the arrival note printed on every Stop and five projects' sessions each
/// cycled to the platform's nine-block cap. Nothing is lost to the silence: delivery is a log
/// (D17), and the presence row below proves the first firing still speaks.
#[test]
fn a_stop_refire_is_answered_with_silence() {
    let b = Board::new();
    b.run("uuid-eve", &["send", "@", "--subject", "s", "--body", "b"]);

    // Presence first (M27): the same board, the same mail, a *first* firing — the banner comes.
    let (code, out) = b.hook("uuid-alice", "turn", r#"{"hook_event_name":"Stop"}"#);
    assert_eq!(code, 0);
    assert!(
        out.contains("unread"),
        "the first firing must deliver: {out}"
    );

    // The re-fire: mail is still unread (nothing acknowledged it), and the answer is nothing.
    let (code, out) = b.hook(
        "uuid-bob",
        "turn",
        r#"{"hook_event_name":"Stop","stop_hook_active":true}"#,
    );
    assert_eq!(code, 0, "silence must still be success — D9");
    assert_eq!(
        out, "",
        "a re-fire answered with context is a wake loop: {out}"
    );
}

/// **The `amb watch` hint is two conditions and both survived** (M56). It is appended only at
/// `SessionStart` and only when the installed mode is `monitor`, because it tells the session to
/// run a blocking command under its Monitor tool — advice that is wrong for a session whose
/// hooks fire on `Stop`, and noise on every later event.
///
/// `&&` relaxed to `||` appends it to every `SessionStart` *and* every monitor-mode `Stop`;
/// `== "monitor"` flipped to `!=` inverts which install gets it. A three-row truth table is the
/// smallest fixture that separates them: turn+start kills the flip, monitor+stop kills the `||`,
/// and monitor+start proves the line is reachable rather than absent for some other reason.
#[test]
fn the_watch_hint_reaches_a_monitor_session_at_start_and_nobody_else() {
    let b = Board::new();
    b.run("uuid-a", &["register", "--name", "alice"]);
    b.run("uuid-b", &["register", "--name", "bob"]);

    for (mode, payload, want, why) in [
        (
            "monitor",
            START,
            true,
            "a monitor install is told how to get mail immediately",
        ),
        (
            "turn",
            START,
            false,
            "a turn install has no Monitor tool to run it under",
        ),
        (
            "monitor",
            STOP,
            false,
            "and the advice is for the opening of a session, not every turn",
        ),
    ] {
        // Fresh mail per row: delivery is a log, so an offer already made is not re-rendered.
        b.run("uuid-a", &["send", "bob", "--subject", "s", "--body", "b"]);
        let (code, out) = b.hook("uuid-b", mode, payload);
        assert_eq!(code, 0, "a hook must always succeed");
        let text = injected(&out).unwrap_or_default();
        assert_eq!(
            text.contains("amb watch --timeout 300"),
            want,
            "{why} — mode={mode} payload={payload} produced {text:?}"
        );
    }
}

/// **A vendor that exports no session variable was a silent no-op, and `doctor` said it was fine**
/// (D113).
///
/// Identity was environment-only. Claude Code and Gemini CLI both export a variable, so both
/// worked — and the other agent CLIs whose hooks could host `amb` today mostly do not: their
/// payloads carry the id and their environments carry nothing. On those, every hook fired, found
/// no identity, exited 0 under D9's guarantee and delivered nothing at all. `amb install --vendor`
/// would succeed over an installation that could never work, which is this project's signature
/// failure at the largest scale it has occurred.
///
/// Driven through the real binary with the environment genuinely stripped, because that is the
/// only place the defect existed: `resolve_from` and `payload_session_id` are both pure and
/// neither was wrong. The wiring was.
#[test]
fn a_session_whose_vendor_exports_no_variable_is_still_reached_through_its_payload() {
    let b = Board::new();
    b.run("uuid-sender", &["register", "--name", "sender"]);
    b.run(
        "uuid-sender",
        &[
            "send",
            "@",
            "--subject",
            "waiting",
            "--body",
            "for whoever arrives",
        ],
    );

    // No AMB_AGENT and no vendor variable — the shape of a Qwen, Codex or Copilot session.
    // `cmd_unscoped("")` already strips AMB_AGENT and Claude's variable; Gemini's is removed here
    // so the fixture is stripped of *every* shipped vendor's, not just the one this host exports.
    let mut cmd = b.cmd_unscoped("");
    cmd.args(["hook", "turn"])
        .env("AMB_PROJECT", "nest")
        .env_remove("GEMINI_SESSION_ID");
    let (code, out) = common::with_stdin(
        cmd,
        r#"{"hook_event_name":"SessionStart","session_id":"payload-only-1"}"#,
    );
    assert_eq!(code, 0, "the hook always exits 0 (D9)");
    assert!(
        out.contains("waiting"),
        "mail must reach a session identified only by its payload: {out}"
    );

    // And the guarantee is intact where there is genuinely nothing to identify: silence, not a
    // crash, and above all not a shared blank identity.
    let mut bare = b.cmd_unscoped("");
    bare.args(["hook", "turn"])
        .env("AMB_PROJECT", "nest")
        .env_remove("GEMINI_SESSION_ID");
    let (code, out) = common::with_stdin(bare, r#"{"hook_event_name":"SessionStart"}"#);
    assert_eq!(code, 0, "no identity anywhere is still exit 0");
    assert!(
        out.trim().is_empty(),
        "and says nothing rather than inventing an agent: {out:?}"
    );
}

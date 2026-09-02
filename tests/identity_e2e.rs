//! Which project a session is in, end to end through the binary.
//!
//! This is the suite that would have caught D20. The unit tests around `repo_root` prove the
//! walk finds a `.git`; only a test that runs the real binary from a subdirectory proves the
//! *consequences* — that a broadcast still arrives, and that two agents editing one file are
//! told about each other. Both of those failed silently before, which is why they are asserted
//! positively here rather than left to follow from the unit tests.

mod common;
use common::{Board, git};

/// Make the board's project directory a git repository, and return a subdirectory inside it.
fn repo_with_subdir(b: &Board, sub: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(b.cwd.join(".git")).expect("mkdir .git");
    let deep = b.cwd.join(sub);
    std::fs::create_dir_all(&deep).expect("mkdir subdir");
    deep
}

/// Run `amb` from an arbitrary directory, with the project *derived* rather than overridden.
///
/// The working directory is the only thing this suite varies, so that is all it builds; the
/// running and parsing come from the shared harness.
fn run_in(b: &Board, agent: &str, dir: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let mut c = b.cmd_unscoped(agent);
    c.current_dir(dir);
    common::json_from(c, args)
}

#[test]
fn a_subdirectory_is_the_same_project_as_the_repository_root() {
    // The defect: `cd src/auth` used to put a session in a project called `auth`.
    let b = Board::new();
    let deep = repo_with_subdir(&b, "src/auth");

    let at_root = run_in(&b, "uuid-root", &b.cwd, &["register"]);
    let in_deep = run_in(&b, "uuid-deep", &deep, &["register"]);

    assert_eq!(
        at_root["project"], in_deep["project"],
        "one repository is one project, wherever the session happens to be standing"
    );
    assert_eq!(
        at_root["project"],
        b.cwd
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned(),
        "and it is named after the repository root, not the cwd"
    );
}

#[test]
fn a_broadcast_from_the_root_reaches_an_agent_in_a_subdirectory() {
    // The user-visible half of D20. This returned count 0 before, with no error anywhere.
    let b = Board::new();
    let deep = repo_with_subdir(&b, "src/auth");
    run_in(&b, "uuid-root", &b.cwd, &["register", "--name", "root"]);
    run_in(&b, "uuid-deep", &deep, &["register", "--name", "deep"]);

    run_in(
        &b,
        "uuid-root",
        &b.cwd,
        &["send", "@", "--subject", "heads up", "--body", "x"],
    );

    let inbox = run_in(&b, "uuid-deep", &deep, &["inbox"]);
    assert_eq!(
        inbox["count"], 1,
        "a peer in a subdirectory is in the same place, so `@` must reach them"
    );
}

#[test]
fn two_agents_editing_one_file_from_different_directories_are_told_about_each_other() {
    // The sharpest consequence, and the whole reason this project exists. Observed claims are
    // recorded relative to the repository root, so the two agents record the *same* path and
    // the conflict is visible. Relative to the cwd they recorded `src/auth/login.rs` and
    // `login.rs`, which never compare equal — two agents on one file, neither warned.
    let b = Board::new();
    let deep = repo_with_subdir(&b, "src/auth");
    let file = b.cwd.join("src/auth/login.rs");
    std::fs::write(&file, "// shared\n").expect("write");
    let file = file.to_string_lossy().into_owned();

    run_in(&b, "uuid-root", &b.cwd, &["register", "--name", "root"]);
    run_in(&b, "uuid-deep", &deep, &["register", "--name", "deep"]);

    for (agent, dir) in [("uuid-root", b.cwd.clone()), ("uuid-deep", deep.clone())] {
        let payload = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Edit",
            "tool_input": { "file_path": file },
        })
        .to_string();
        let mut c = b.cmd_unscoped(agent);
        c.current_dir(&dir)
            .args(["hook", "turn"])
            .stdin(std::process::Stdio::piped());
        let mut child = c.spawn().expect("spawn");
        std::io::Write::write_all(child.stdin.as_mut().expect("stdin"), payload.as_bytes())
            .expect("write payload");
        assert!(
            child.wait().expect("wait").success(),
            "a hook always exits 0"
        );
    }

    let claims = run_in(&b, "uuid-root", &b.cwd, &["claims"]);
    let paths: Vec<&str> = claims["claims"]
        .as_array()
        .expect("array")
        .iter()
        .map(|c| c["path"].as_str().expect("path"))
        .collect();
    assert_eq!(
        paths,
        ["src/auth/login.rs", "src/auth/login.rs"],
        "both agents must record the same repository-relative path, got {paths:?}"
    );

    // And the awareness that path equality buys: each is told at the turn boundary.
    for (agent, dir) in [("uuid-root", b.cwd.clone()), ("uuid-deep", deep.clone())] {
        let mut c = b.cmd_unscoped(agent);
        c.current_dir(&dir)
            .args(["hook", "turn"])
            .stdin(std::process::Stdio::piped());
        let mut child = c.spawn().expect("spawn");
        std::io::Write::write_all(
            child.stdin.as_mut().expect("stdin"),
            br#"{"hook_event_name":"Stop"}"#,
        )
        .expect("write payload");
        let out = child.wait_with_output().expect("wait");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("also claimed"),
            "{agent} must be warned about the other, got {stdout:?}"
        );
    }
}

#[test]
fn a_worktree_is_its_own_project() {
    // Deliberate, and the argument is D20's. Two worktrees are two sets of files on disk; a
    // claim on `src/auth.rs` in one says nothing about the other, and warning across them would
    // be over-claiming — which D19 says teaches agents to ignore claims altogether.
    let b = Board::new();
    std::fs::create_dir_all(b.cwd.join(".git")).expect("mkdir .git");
    let wt = b.home.join("wt-feature");
    std::fs::create_dir_all(wt.join("src")).expect("mkdir worktree");
    std::fs::write(
        wt.join(".git"),
        "gitdir: /elsewhere/.git/worktrees/wt-feature",
    )
    .expect("gitfile");

    let main = run_in(&b, "uuid-main", &b.cwd, &["register"]);
    let feature = run_in(&b, "uuid-feat", &wt.join("src"), &["register"]);

    assert_ne!(
        main["project"], feature["project"],
        "separate working trees are separate places"
    );
    assert_eq!(
        feature["project"], "wt-feature",
        "and the worktree is named by its own root, found through the .git *file*"
    );
}

/// Two repositories that share a basename share a `@project` broadcast address, and nothing said so.
///
/// `messages::inbox` routes on `m.to_proj = ?1`, a string comparison, so mail addressed to `@api`
/// from one repository is delivered into sessions working an unrelated `api` next door. The vault
/// mixes their notes for the same reason. Both failures are silent, which is the shape this
/// project treats as its worst — so the roster reports the clash rather than leaving it to be
/// inferred from mail arriving in the wrong place.
#[test]
fn two_repositories_claiming_one_name_are_reported() {
    let b = Board::new();

    // Two distinct repository roots, each declaring the same project name — exactly what a
    // second `~/Projects/api` produces today, since the name is derived from the basename.
    let one = b.cwd.join("one");
    let two = b.cwd.join("two");
    for root in [&one, &two] {
        std::fs::create_dir_all(root.join(".git")).expect("mkdir repo");
    }
    for (agent, root) in [("uuid-one", &one), ("uuid-two", &two)] {
        let mut c = b.cmd_unscoped(agent);
        c.current_dir(root)
            .env("AMB_PROJECT", "api")
            .args(["register", "--name", agent]);
        let out = c.output().expect("amb runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Queried from inside one of the two repositories. Every command auto-creates its roster row
    // (identity is free), so asking from a *third* project would silently re-register the asking
    // agent under that project and dissolve the very collision under test.
    let ask = |args: &[&str]| {
        let mut c = b.cmd_unscoped("uuid-one");
        c.current_dir(&one).env("AMB_PROJECT", "api");
        common::json_from(c, args)
    };
    let ask_text = |args: &[&str]| {
        let mut c = b.cmd_unscoped("uuid-one");
        c.current_dir(&one).env("AMB_PROJECT", "api");
        let out = c.args(args).output().expect("amb runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let j = ask(&["agents"]);
    let clashes = j["collisions"].as_array().expect("a collisions array");
    assert_eq!(clashes.len(), 1, "one name is claimed twice: {j}");
    assert_eq!(clashes[0]["project"], "api");
    let roots = clashes[0]["roots"].as_array().expect("roots");
    assert_eq!(
        roots.len(),
        2,
        "both repositories must be named, not just the count"
    );

    // And the human surface must say it, not only `--json`. A counter nothing prints is this
    // project's recurring defect.
    let text = ask_text(&["agents"]);
    assert!(
        text.contains("both call themselves") && text.contains("\"api\""),
        "the roster hides the collision: {text}"
    );
    assert!(
        text.contains(&one.to_string_lossy().to_string()),
        "the warning must name which repositories collide: {text}"
    );

    // Narrowing the roster must not narrow away the warning — a collision is a property of the
    // machine, and `--project api` is precisely when you most need to be told the name is not one
    // place.
    let narrowed = ask(&["agents", "--project", "api"]);
    assert_eq!(
        narrowed["collisions"].as_array().map(Vec::len),
        Some(1),
        "filtering the roster silenced the warning: {narrowed}"
    );
}

/// A worktree and a second clone are the same project, and must not be reported.
///
/// Both have a different root and the same basename, so a distinct-root rule calls them a
/// collision — on a setup people use deliberately, every time. That is the failure mode that
/// teaches people to ignore warnings, and it is worse here than a missed detection would be,
/// because detection-only means a false positive is pure noise with nothing at stake.
///
/// The discriminator is the remote, read from `.git/config` — following `commondir` for a
/// worktree, whose `.git` is a file and whose config lives in the main repository.
#[test]
fn worktrees_and_second_clones_of_one_repository_are_not_a_collision() {
    let b = Board::new();
    // `common::git`, not a local closure: it clears the git environment this process may have
    // inherited. Running the suite from `.githooks/pre-commit` put `GIT_INDEX_FILE=.git/index`
    // in the environment, the child `git worktree add` below inherited it, and this test failed
    // 100% of the time inside the hook while passing every direct run. See `common::GIT_ENV`.

    // An origin, one clone of it, a second clone under a different parent, and a worktree whose
    // directory happens to share the basename. All four resolve to project "shared".
    let origin = b.cwd.join("origin");
    std::fs::create_dir_all(&origin).expect("mkdir");
    git(&["init", "-q"], &origin);
    git(&["config", "user.email", "t@t"], &origin);
    git(&["config", "user.name", "t"], &origin);
    std::fs::write(origin.join("a.txt"), "x").expect("write");
    git(&["add", "-A"], &origin);
    git(&["commit", "-qm", "init"], &origin);

    let one = b.cwd.join("one/shared");
    let two = b.cwd.join("two/shared");
    std::fs::create_dir_all(one.parent().expect("p")).expect("mkdir");
    std::fs::create_dir_all(two.parent().expect("p")).expect("mkdir");
    git(
        &[
            "clone",
            "-q",
            &origin.to_string_lossy(),
            &one.to_string_lossy(),
        ],
        &b.cwd,
    );
    git(
        &[
            "clone",
            "-q",
            &origin.to_string_lossy(),
            &two.to_string_lossy(),
        ],
        &b.cwd,
    );
    let wt = b.cwd.join("wt/shared");
    std::fs::create_dir_all(wt.parent().expect("p")).expect("mkdir");
    git(
        &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feat"],
        &one,
    );

    let register = |agent: &str, root: &std::path::Path| {
        let mut c = b.cmd_unscoped(agent);
        c.current_dir(root)
            .env("AMB_PROJECT", "shared")
            .args(["register", "--name", agent]);
        let out = c.output().expect("amb runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    register("uuid-one", &one);
    register("uuid-two", &two);
    register("uuid-wt", &wt);

    let mut c = b.cmd_unscoped("uuid-one");
    c.current_dir(&one).env("AMB_PROJECT", "shared");
    let j = common::json_from(c, &["agents"]);
    assert_eq!(
        j["collisions"].as_array().map(Vec::len),
        Some(0),
        "three checkouts of one repository are one project, not a collision: {j}"
    );

    // And the discriminator must not have disabled detection: an unrelated repository claiming the
    // same name still collides, because its remote differs.
    let other = b.cwd.join("other/shared");
    std::fs::create_dir_all(&other).expect("mkdir");
    git(&["init", "-q"], &other);
    register("uuid-other", &other);

    let mut c = b.cmd_unscoped("uuid-one");
    c.current_dir(&one).env("AMB_PROJECT", "shared");
    let j = common::json_from(c, &["agents"]);
    assert_eq!(
        j["collisions"].as_array().map(Vec::len),
        Some(1),
        "a genuinely unrelated repository must still be reported: {j}"
    );
    let roots = j["collisions"][0]["roots"].as_array().expect("roots");
    assert_eq!(
        roots.len(),
        2,
        "one entry per repository, not per checkout: {j}"
    );
}

/// The ordinary case must stay quiet, or the warning becomes noise nobody reads.
#[test]
fn distinct_projects_report_no_collision() {
    let b = Board::new();
    b.run("uuid-a", &["register", "--name", "alice"]);
    let j = b.json("uuid-a", &["agents"]);
    assert_eq!(j["collisions"].as_array().map(Vec::len), Some(0));
    assert!(
        !b.run("uuid-a", &["agents"])
            .contains("both call themselves")
    );
}

/// The environment guard is invisible under a plain `cargo test`, which is the problem.
///
/// `common::GIT_ENV` only matters when git's own hook environment is present — inside
/// `.githooks/pre-commit`, where nobody is watching a test list. Deleting the `env_remove` loop
/// therefore stays green everywhere except the one place it is load-bearing. This asserts the
/// removals positively so the guard is protected by something (D51, D71).
#[test]
fn the_git_helper_clears_the_environment_a_commit_hook_would_have_set() {
    let cmd = common::git_cmd(&["status"], std::path::Path::new("."));
    let cleared: Vec<String> = cmd
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();
    for var in common::GIT_ENV {
        assert!(
            cleared.iter().any(|c| c == var),
            "{var} is not cleared; a child git inherits it from .githooks/pre-commit and \
             operates on the committing repository instead of the test's"
        );
    }
}

/// A session's pid decides liveness, and the harness must actually deliver one.
///
/// **Found by mutation (M19): three survivors on `session_pid` itself.** Returning `None`,
/// `Some(0)` or `Some(-1)` unconditionally all left the suite green, because every one of them
/// degrades to `last_seen` recency — and in a test everything is recent, so everybody reads
/// alive. `kill(0, sig)` addresses the caller's whole process group and `kill(-1, sig)` addresses
/// every process the caller may signal, so two of the three would report **every peer on every
/// board permanently alive**: D21's liveness oracle, reintroduced by a constant.
///
/// Only the *dead* half discriminates, which is why it is asserted first. `AMB_SESSION_PID` is the
/// documented override the shared harness deliberately strips, so this is also the one place that
/// proves the override reaches `session_pid` at all.
#[test]
fn a_dead_session_pid_reads_as_gone_and_a_live_one_does_not() {
    let b = Board::new();

    // A pid nothing can be running under. `kill(2)` answers ESRCH, which is a real answer.
    let mut c = b.cmd("uuid-dead");
    c.env("AMB_SESSION_PID", "999999999");
    common::json_from(c, &["register"]);

    // This test process, which is provably running.
    let mut c = b.cmd("uuid-live");
    c.env("AMB_SESSION_PID", std::process::id().to_string());
    common::json_from(c, &["register"]);

    // **The querying command carries the live pid too**, because every command re-touches its own
    // roster row before doing anything else (D21). Asking without it would blank `uuid-live`'s pid
    // and leave the live half asserting recency rather than liveness.
    let mut c = b.cmd("uuid-live");
    c.env("AMB_SESSION_PID", std::process::id().to_string());
    let j = common::json_from(c, &["agents"]);
    let rows = j["agents"].as_array().expect("agents");
    // Matched on `id`: `name` is the derived display name, not what the test set.
    let alive = |id: &str| -> bool {
        rows.iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("no row for {id}: {j}"))["appears_alive"]
            .as_bool()
            .expect("appears_alive is a bool")
    };

    assert!(
        !alive("uuid-dead"),
        "a session whose pid is not running must read as gone, however recently it spoke: {j}"
    );
    assert!(
        alive("uuid-live"),
        "a running session must read as alive: {j}"
    );
}

/// A blank `AMB_PROJECT` falls back to the directory rather than being taken literally.
///
/// **Found by mutation (M19): the `!p.trim().is_empty()` guard could be `true`.** An exported but
/// empty variable — `export AMB_PROJECT=` in a shell profile, or a wrapper that sets it from an
/// unset value — would then become the project *name*, and a project named `"   "` addresses a
/// place no other session can type. Silent, and it would look like broadcasts simply not
/// arriving.
#[test]
fn a_blank_amb_project_falls_back_to_the_directory() {
    let b = Board::new();
    let expected = b
        .cwd
        .file_name()
        .expect("the board's directory has a name")
        .to_string_lossy()
        .into_owned();

    for blank in ["", "   ", "\t"] {
        let mut c = b.cmd_unscoped("uuid-blank");
        c.current_dir(&b.cwd).env("AMB_PROJECT", blank);
        let j = common::json_from(c, &["register"]);
        assert_eq!(
            j["project"].as_str(),
            Some(expected.as_str()),
            "AMB_PROJECT={blank:?} must fall back to the directory, not become the name: {j}"
        );
    }
}

/// **A Gemini session and a Claude session reach each other, across projects** (D111).
///
/// This is the requirement the vendor descriptor exists to serve, and it is asserted end to end
/// because every part of it is a *silence* when it breaks: a session whose CLI exported no
/// variable `amb` recognises simply has no identity, and "no identity" looks exactly like "no
/// mail" from the outside. The Gemini session here is identified by `GEMINI_SESSION_ID` alone —
/// `AMB_AGENT` is removed, and so is Claude's variable — so the only thing that can make this
/// pass is the descriptor list being consulted.
///
/// The reply address is asserted too: `from` is a display name, and a name resolves only inside
/// its own project (U8). Cross-project is the case where that distinction stops being academic.
#[test]
fn a_gemini_session_can_message_a_claude_session_in_another_project() {
    use std::process::Command;

    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("board.db");
    let run = |session_var: &str, session: &str, project: &str, args: &[&str]| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_amb"))
            .args(args)
            .current_dir(dir.path())
            .env("AMB_DB", &db)
            .env("AMB_PROJECT", project)
            .env(session_var, session)
            .env_remove("AMB_AGENT")
            .env_remove(if session_var == "GEMINI_SESSION_ID" {
                "CLAUDE_CODE_SESSION_ID"
            } else {
                "GEMINI_SESSION_ID"
            })
            .env_remove("CLAUDE_CODE_MESSAGING_SOCKET")
            .env_remove("AMB_VAULT")
            .output()
            .expect("amb runs");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let claude = |args: &[&str]| run("CLAUDE_CODE_SESSION_ID", "claude-sess", "beta", args);
    let gemini = |args: &[&str]| run("GEMINI_SESSION_ID", "gemini-sess", "alpha", args);

    claude(&["register", "--name", "bob"]);
    let registered = gemini(&["register", "--name", "gwen"]);
    assert!(
        registered.contains("gwen"),
        "a session identified only by GEMINI_SESSION_ID must still get an identity: {registered}"
    );

    let sent = gemini(&[
        "send",
        "bob@beta",
        "--subject",
        "cross-vendor",
        "--body",
        "from Gemini to Claude",
    ]);
    assert!(sent.contains("sent"), "{sent}");

    let inbox = claude(&["inbox"]);
    assert!(
        inbox.contains("cross-vendor") && inbox.contains("gwen"),
        "the Claude session must receive it: {inbox}"
    );

    let json = claude(&["inbox", "--json"]);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(
        doc["messages"][0]["address"], "gwen@alpha",
        "and be handed an address that resolves from here, not a bare name: {json}"
    );
}

/// **A vendor `amb` has never heard of, added by dropping one file** (D111 phase 3).
///
/// This is the requirement in its strongest form: no rebuild, no code change, no entry in any
/// list inside the binary. It runs through the real process because that is the only thing that
/// proves the loader is reached on the paths that matter — installation *and* identity — and
/// because `OnceLock` makes the load per-process, which a unit test cannot exercise twice.
///
/// The `postToolUseFailure` assertion is not decoration: the first version of the parser read
/// `tool_failed` from the document root instead of from `events`, so every manifest silently
/// lost its capture lane while the install still succeeded and printed two lanes where three
/// were declared. A count of installed lanes is what catches that; a "did it install" check is
/// not.
#[test]
fn a_vendor_amb_never_heard_of_installs_from_a_dropped_in_manifest() {
    use std::process::Command;

    let dir = tempfile::tempdir().expect("tempdir");
    let vendors = dir.path().join("vendors");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&vendors).expect("mkdir");
    std::fs::create_dir_all(&home).expect("mkdir");
    std::fs::write(
        vendors.join("acme.json"),
        r#"{
          "id": "acme-cli",
          "label": "ACME CLI",
          "config_dir": ".acme",
          "settings_file": "hooks/amb.json",
          "events": {
            "session_start": "Begin", "turn_end": "Done", "tool_post": "AfterTool",
            "session_end": "Finish", "tool_pre": "BeforeTool", "tool_failed": "ToolBroke"
          },
          "session_env": ["ACME_SESSION_ID"]
        }"#,
    )
    .expect("write manifest");

    let run = |args: &[&str], extra: &[(&str, &str)]| -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_amb"));
        c.args(args)
            .current_dir(dir.path())
            .env("HOME", &home)
            .env("AMB_VENDORS", &vendors)
            .env("AMB_DB", dir.path().join("board.db"))
            .env("AMB_PROJECT", "acme-proj")
            .env_remove("AMB_AGENT")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("GEMINI_SESSION_ID")
            .env_remove("CLAUDE_CODE_MESSAGING_SOCKET")
            .env_remove("AMB_VAULT");
        for (k, v) in extra {
            c.env(k, v);
        }
        let out = c.output().expect("amb runs");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // It installs, into its own file, with its own spellings — none of them Claude's.
    let plan = run(
        &[
            "install", "--vendor", "acme-cli", "--mode", "turn", "--memory",
        ],
        &[],
    );
    for mine in [
        "Begin",
        "Done",
        "AfterTool",
        "Finish",
        "BeforeTool",
        "ToolBroke",
    ] {
        assert!(plan.contains(mine), "{mine} missing from the plan: {plan}");
    }
    assert!(
        !plan.contains("SessionStart") && !plan.contains("PostToolUse"),
        "Claude's vocabulary leaked into a manifest vendor: {plan}"
    );
    let written = std::fs::read_to_string(home.join(".acme").join("hooks").join("amb.json"))
        .expect("it wrote to the path the manifest named");
    assert!(written.contains("ToolBroke"), "{written}");

    // And a session of that vendor is identified by the variable the manifest named.
    let who = run(
        &["register", "--name", "roadrunner"],
        &[("ACME_SESSION_ID", "acme-1")],
    );
    assert!(
        who.contains("roadrunner"),
        "a manifest vendor's session must get an identity: {who}"
    );
}

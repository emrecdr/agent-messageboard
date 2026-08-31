//! Shared harness for tests that drive the real `amb` binary.
//!
//! Three suites spawn the binary and each needs something different from it — concurrency wants
//! to `spawn` many children before waiting on any, hook tests want a private `HOME` and a stdin
//! payload, claims tests want a working directory and direct database access. What they *share*
//! is the environment contract: which variables identify an agent and where the board lives.
//!
//! Only that shared core lives here. The divergent parts stay in each suite — but "divergent"
//! means the *command*, not what is done with it. [`json_from`] and [`with_stdin`] therefore take
//! a `Command` the caller has already shaped, which is how three suites can share the running,
//! the success assertion and the parsing without this growing a flag for every caller.

#![allow(dead_code)] // Each suite uses a different subset; unused-here is not unused.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

pub const AMB: &str = env!("CARGO_BIN_EXE_amb");

/// The variables git exports into every hook it runs, which a child `git` then inherits.
///
/// **A test that shells out to `git` must clear these or it is not operating on the repository it
/// thinks it is.** `tools/verify.sh` runs this suite from `.githooks/pre-commit` (D70), where git
/// has already set `GIT_INDEX_FILE=.git/index` and `GIT_DIR=.git` for the *committing*
/// repository. A child `git worktree add`, given a temp repository through `current_dir`, still
/// reads those and fails with `.git/index: index file open failed: Not a directory`.
///
/// **It looked like flakiness and was not.** It reproduces 100% under
/// `GIT_INDEX_FILE=.git/index cargo test` and never otherwise, so it failed only inside the hook
/// and passed every direct run — including six full-suite runs used to rule out an unrelated
/// hypothesis. Same reasoning as the `AMB_*` and `CLAUDE_CODE_*` removals in `cmd_unscoped`
/// below: a spawned process inherits an ambient environment unless the test says otherwise.
pub const GIT_ENV: &[&str] = &[
    "GIT_INDEX_FILE",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_PREFIX",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// A `git` command in `cwd` with the ambient git environment cleared.
///
/// Split from [`git`] so the clearing is *observable*: `Command::get_envs` reports the removals,
/// which lets a test assert the guard exists without needing git's hook environment to be
/// present. Without that, deleting the loop below stays green under a plain `cargo test` and only
/// fails inside a commit — a guard nothing protects, which is D51 exactly.
pub fn git_cmd(args: &[&str], cwd: &std::path::Path) -> Command {
    let mut c = Command::new("git");
    c.args(args).current_dir(cwd);
    for v in GIT_ENV {
        c.env_remove(v);
    }
    c
}

/// Run `git` in `cwd` with the ambient git environment cleared, asserting it succeeded.
pub fn git(args: &[&str], cwd: &std::path::Path) {
    let out = git_cmd(args, cwd).output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A throwaway board, plus a project directory to run commands from.
pub struct Board {
    _dir: tempfile::TempDir,
    /// Value for `AMB_DB`.
    pub db: String,
    /// Working directory commands run in — the "project root" claims are relative to.
    pub cwd: PathBuf,
    /// Value for `HOME`, so a test can own its own `~/.claude/settings.json`.
    pub home: PathBuf,
    /// Value for `AMB_VAULT`. **Not exported by default** — see [`Board::cmd_unscoped`].
    pub vault: PathBuf,
}

impl Board {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("repo");
        std::fs::create_dir_all(&cwd).expect("mkdir repo");
        let db = dir.path().join("board.db").to_string_lossy().into_owned();
        let home = dir.path().to_path_buf();
        let vault = dir.path().join("vault");
        Self {
            _dir: dir,
            db,
            cwd,
            home,
            vault,
        }
    }

    /// A `Command` carrying the environment contract, ready for the caller to add args to.
    ///
    /// `CLAUDE_CODE_SESSION_ID` is removed so a test never accidentally inherits the identity of
    /// the session running it — which would make results depend on where the suite was run.
    pub fn cmd(&self, agent: &str) -> Command {
        let mut c = self.cmd_unscoped(agent);
        c.env("AMB_PROJECT", "nest");
        c
    }

    /// The same contract **without** `AMB_PROJECT`, so a suite can test how the project is
    /// derived rather than assert against an override that hides the derivation.
    pub fn cmd_unscoped(&self, agent: &str) -> Command {
        let mut c = Command::new(AMB);
        c.current_dir(&self.cwd)
            .env("AMB_DB", &self.db)
            .env_remove("AMB_PROJECT")
            .env("HOME", &self.home)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            // Without this every test agent inherits the socket — and therefore the session pid
            // — of the session running the suite, and they all report alive.
            .env_remove("CLAUDE_CODE_MESSAGING_SOCKET")
            .env_remove("AMB_SESSION_PID")
            // Memory is off unless a test asks for it. Removed rather than merely unset: a
            // developer with AMB_VAULT exported would otherwise have every suite writing notes
            // into their real vault, and the memory hooks would fire in tests about mail.
            .env_remove("AMB_VAULT")
            .env_remove("AMB_MEMORY_SKIP_TOOLS")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if agent.is_empty() {
            c.env_remove("AMB_AGENT")
        } else {
            c.env("AMB_AGENT", agent)
        };
        c
    }

    /// Run to completion without asserting success — for tests about failure.
    pub fn try_run(&self, agent: &str, args: &[&str]) -> Output {
        self.cmd(agent).args(args).output().expect("amb runs")
    }

    /// Run and require success, returning stdout.
    pub fn run(&self, agent: &str, args: &[&str]) -> String {
        let out = self.try_run(agent, args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Run with `--json` appended and parse the result.
    pub fn json(&self, agent: &str, args: &[&str]) -> serde_json::Value {
        json_from(self.cmd(agent), args)
    }

    /// Fire a hook with a stdin payload, returning (exit code, stdout).
    pub fn hook(&self, agent: &str, mode: &str, payload: &str) -> (i32, String) {
        let mut c = self.cmd(agent);
        c.args(["hook", mode]);
        with_stdin(c, payload)
    }

    /// Fire a `PostToolUse` hook as an agent would after using a tool on a file.
    pub fn tool_use(&self, agent: &str, tool: &str, file_path: &str) {
        let payload = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_name": tool,
            "tool_input": { "file_path": file_path },
        })
        .to_string();
        let (code, _) = self.hook(agent, "turn", &payload);
        assert_eq!(code, 0, "a hook must always succeed");
    }

    /// The environment contract **with** a vault, for the memory suite.
    pub fn cmd_mem(&self, agent: &str) -> Command {
        let mut c = self.cmd(agent);
        c.env("AMB_VAULT", &self.vault);
        c
    }

    /// Run a memory-enabled command and require success.
    pub fn mem(&self, agent: &str, args: &[&str]) -> String {
        let out = self.cmd_mem(agent).args(args).output().expect("amb runs");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Run a memory-enabled command without asserting success.
    pub fn try_mem(&self, agent: &str, args: &[&str]) -> Output {
        self.cmd_mem(agent).args(args).output().expect("amb runs")
    }

    /// Run a memory-enabled command with `--json` and parse it.
    pub fn mem_json(&self, agent: &str, args: &[&str]) -> serde_json::Value {
        json_from(self.cmd_mem(agent), args)
    }

    /// Fire the memory hook with a stdin payload.
    pub fn mem_hook(&self, agent: &str, payload: &str) -> (i32, String) {
        let mut c = self.cmd_mem(agent);
        c.args(["hook", "memory"]);
        with_stdin(c, payload)
    }

    /// Direct database access, for arranging state a CLI cannot reach.
    pub fn sqlite(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.db).expect("open board")
    }

    /// An absolute path inside the project.
    pub fn path(&self, rel: &str) -> String {
        self.cwd.join(rel).to_string_lossy().into_owned()
    }
}

// Free functions rather than `Board` methods, because a suite that needs to vary the command —
// a different working directory, a different set of environment overrides — can build its own
// `Command` from `cmd_unscoped` and still reuse the running and parsing. That was the part
// three suites had each written out for themselves.

/// Run a prepared command with `--json` appended, require success, and parse stdout.
pub fn json_from(mut c: Command, args: &[&str]) -> serde_json::Value {
    let mut with_json = args.to_vec();
    with_json.push("--json");
    let out = c.args(&with_json).output().expect("amb runs");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{args:?} emitted invalid JSON: {e}"))
}

/// Run a prepared command with a stdin payload, returning (exit code, stdout).
///
/// Does not assert success: every caller so far is testing what happens on a path that may
/// legitimately fail, and the exit code is the thing under test.
pub fn with_stdin(mut c: Command, stdin: &str) -> (i32, String) {
    let mut child = c.stdin(Stdio::piped()).spawn().expect("spawn amb");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("amb finishes");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

//! Who this session is, and which project it is working in.
//!
//! Identity is free. `CLAUDE_CODE_SESSION_ID` is present in the environment of every command a
//! Claude Code session shells out to, is inherited by subshells and fresh `exec`s, and equals the
//! name of that session's own transcript file. Verified 2026-08-27; see `DECISIONS.md` D12.

use crate::db::now;
use crate::error::{Error, Result, io, sql};
use rusqlite::{Connection, OptionalExtension, params};

/// A session's identity on the board.
#[derive(Debug, Clone)]
pub struct Identity {
    /// The routing key: the session UUID. Stable for the life of the session.
    pub id: String,
    /// Display name. Mutable, and never routed on — names are for humans, ids for routing.
    pub name: String,
    pub project: String,
    /// The **repository root**, not the working directory — everything a claim path is relative
    /// to. Named `root` rather than `cwd` because the two differ the moment a session runs
    /// `cd src/`, and that difference was a silent bug (D20).
    pub root: String,
}

/// The first six characters of the session UUID, matching what `ListAgents` shows in brackets.
pub fn short_ref(id: &str) -> String {
    id.chars().take(6).collect()
}

/// The working-tree root containing `start`, found by walking up for `.git`.
///
/// **`.git` is a file, not a directory, in a linked worktree and in a submodule** — it holds a
/// `gitdir:` pointer. So this checks existence rather than `is_dir`; testing for a directory
/// would silently fail in exactly the worktree setup `RESEARCH.md` R3 says these agents use.
///
/// The walk stops **below** `$HOME`. A dotfiles repository at `~/.git` is common, and without
/// this guard every project on the machine that is not itself a repository would collapse into
/// one namespace named after the home directory — strictly worse than the bug this fixes.
pub fn repo_root(start: &std::path::Path, home: Option<&str>) -> Option<std::path::PathBuf> {
    let home = home.map(std::path::Path::new);
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if home.is_some_and(|h| dir == h) {
            return None;
        }
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Resolve this session's identity from the environment.
///
/// `AMB_AGENT` overrides the session id — which is what lets a test, or a non-Claude caller, act
/// as a specific agent. `AMB_PROJECT` overrides the project, which otherwise defaults to the
/// working directory's name.
pub fn resolve() -> Result<Identity> {
    // `AMB_AGENT` first, then whichever session id the host CLI exported. The list lives on the
    // vendor descriptor, so a second CLI is recognised by adding a name to data — and identity is
    // where one arrives first, since a session is simply whoever exported an id.
    let id = std::env::var("AMB_AGENT")
        .ok()
        .or_else(|| crate::vendors::detect().session_id_from_env())
        .ok_or(Error::NoIdentity)?;
    if id.trim().is_empty() {
        return Err(Error::NoIdentity);
    }
    let cwd = std::env::current_dir().map_err(io("reading the working directory"))?;

    // The repository root, not the working directory. A session that has run `cd src/auth`
    // otherwise joins a *different* project, so `@` no longer reaches it and — worse — its
    // observed claims are recorded as `login.rs` while a peer at the root records
    // `src/auth/login.rs`, and the two never compare equal. Two agents editing one file, and
    // neither warned. See D20.
    let home = std::env::var("HOME").ok();
    let root = repo_root(&cwd, home.as_deref()).unwrap_or_else(|| cwd.clone());

    let project = match std::env::var("AMB_PROJECT") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
    };
    let name = default_name(&project, &id);
    Ok(Identity {
        id,
        name,
        project,
        root: root.to_string_lossy().into_owned(),
    })
}

/// Narrow a parsed integer to a pid `kill(2)` can be asked about.
///
/// Filtered here as well as in [`real_pid`], so a value `kill` would misread never reaches the
/// roster in the first place and `amb agents --json` never shows a `"pid": 0`.
fn sane_pid(p: i64) -> Option<i64> {
    (p > 0).then_some(p)
}

/// The pid encoded in a Claude Code session socket path, if there is one.
///
/// **Pure, and separated from the environment read on purpose.** This is the rule D93's whole
/// addressing argument rests on — the socket's file name *is* the session's pid — and it is an
/// **observed** detail of an undocumented format, so it is exactly the rule that has to degrade
/// rather than guess. Anything unparseable yields `None` and liveness falls back to `last_seen`
/// freshness. Mutation testing found six survivors here with the logic inside the env-reading
/// shell, where nothing could reach it (M19).
fn pid_from_socket(sock: &str) -> Option<i64> {
    std::path::Path::new(sock)
        .file_stem()?
        .to_str()?
        .parse()
        .ok()
        .and_then(sane_pid)
}

/// The process id of the Claude Code session this command runs inside, when knowable.
///
/// **Deliberately not `std::process::id()`.** That is the pid of this `amb` invocation, which
/// exits milliseconds later, so `kill(pid, 0)` against it asks a question about a corpse. And
/// because every command re-touches its own row before doing anything else, storing it made the
/// *calling* agent report alive every time and every peer a lottery on pid reuse — a liveness
/// oracle that answered only about itself. See D21.
///
/// Claude Code binds a per-session inbox socket and exports its path as
/// `CLAUDE_CODE_MESSAGING_SOCKET`, whose file name is the session's pid. The variable is
/// documented; the file-name format is an **observed** detail, so anything unparseable yields
/// `None` and liveness falls back to `last_seen` freshness rather than to a wrong answer.
///
/// `AMB_SESSION_PID` overrides it, so a test owns its own liveness instead of inheriting the
/// socket of the session running the suite.
pub fn session_pid() -> Option<i64> {
    if let Ok(p) = std::env::var("AMB_SESSION_PID") {
        return p.trim().parse().ok().and_then(sane_pid);
    }
    pid_from_socket(&std::env::var("CLAUDE_CODE_MESSAGING_SOCKET").ok()?)
}

/// The name given to a session that never called `register`.
///
/// Deliberately mirrors what `ListAgents` displays (`nestwatch-4e`), so a human reading
/// `amb agents` recognises the row without having registered anything.
pub fn default_name(project: &str, id: &str) -> String {
    format!("{}-{}", project, short_ref(id))
}

/// Short-ref widths tried, in order, when an auto-generated name is already taken.
///
/// **D12 promises auto-registration cannot fail** — *"forgetting to register is therefore not a
/// failure mode, only a less readable name"*. It could. [`default_name`] uses a six-character
/// ref, and `UNIQUE(project, name)` then locked the *second* session sharing that prefix out of
/// **every** command, including reading its own inbox: `amb inbox` exited 64 rather than
/// returning mail. Rare with random UUIDs and total when it happens (D32).
///
/// Widening keeps the promise, and the cost is exactly the one D12 says it should be: a name that
/// is less pretty. The last candidate is the full id, which is unique by construction, so the
/// walk always terminates in a usable name.
const REF_WIDTHS: &[usize] = &[6, 8, 12];

/// Every name an implicit registration may settle for, most readable first.
fn name_candidates(who: &Identity) -> Vec<String> {
    let mut out: Vec<String> = REF_WIDTHS
        .iter()
        .map(|w| {
            format!(
                "{}-{}",
                who.project,
                who.id.chars().take(*w).collect::<String>()
            )
        })
        .collect();
    out.push(format!("{}-{}", who.project, who.id));
    out.dedup();
    out
}

/// Record or refresh this session's roster row.
///
/// Called by *every* command, so forgetting to `register` is not a failure mode — only a less
/// readable name (D12). An explicit name overwrites; an implicit call leaves an existing name
/// alone, so auto-registration never clobbers one a session chose.
pub fn touch(conn: &Connection, who: &Identity, explicit_name: Option<&str>) -> Result<String> {
    register(conn, who, explicit_name).map(|r| r.name)
}

/// What a roster upsert settled on, and what it had to displace to get there.
#[derive(Debug, Clone)]
pub struct Registered {
    /// The name the row now holds.
    pub name: String,
    /// The auto-name a session that had ended was moved to, so this one could take its name.
    ///
    /// `None` on every ordinary registration. Carried rather than merely logged because a
    /// reclamation must be *visible*: two sessions answering to one name across a transcript
    /// history otherwise read as one continuous identity (D75).
    pub reclaimed_from: Option<String>,
}

/// Whether an explicit name may be taken from whoever currently holds it.
///
/// **Pure, so the rule is testable without a board and without a dead process.** Two conditions,
/// and both matter: the holder must be someone else — re-registering your own name is a renewal,
/// not a reclamation — and it must be provably gone by the same oracle `amb agents` uses.
///
/// `is_alive` degrades to recency when there is no usable pid, so "unknown" counts as alive here
/// and the name is *not* taken. That is the safe direction: wrongly refusing a name costs a
/// suffix, wrongly taking one costs a live session its identity (D21).
pub fn reclaimable(
    holder_id: &str,
    me: &str,
    holder_pid: Option<i64>,
    holder_last_seen: f64,
    at: f64,
) -> bool {
    holder_id != me && !is_alive(holder_pid, holder_last_seen, at)
}

/// Move a session that has ended off `name`, so a live one can take it.
///
/// **Renaming the dead holder rather than deleting its row is what keeps history readable.**
/// `messages` stores `from_agent` as an id and joins the display name at read time, so the old
/// session's past mail relabels itself to the auto-name automatically. Deleting the row would
/// leave those messages with no sender name at all.
fn reclaim(conn: &Connection, who: &Identity, name: &str, at: f64) -> Result<Option<String>> {
    let holder = conn
        .query_row(
            "SELECT id, pid, last_seen FROM agents WHERE project = ?1 AND name = ?2",
            params![who.project, name],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, f64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(sql("looking up the holder of a name"))?;
    let Some((id, pid, last_seen)) = holder else {
        return Ok(None);
    };
    if !reclaimable(&id, &who.id, pid, last_seen, at) {
        return Ok(None);
    }
    let fallback = default_name(&who.project, &id);
    // If the fallback is itself taken this returns a unique violation, the reclamation does not
    // happen, and the caller reports `NameTaken` as before. Failing closed is right: the point is
    // to free a name, not to start a cascade of renames.
    match conn.execute(
        "UPDATE agents SET name = ?1 WHERE id = ?2",
        params![fallback, id],
    ) {
        Ok(_) => Ok(Some(fallback)),
        Err(e) if is_constraint_violation(&e) => Ok(None),
        Err(e) => Err(Error::Sqlite {
            context: "renaming a session that has ended".into(),
            source: e,
        }),
    }
}

/// A display name's cap. Rendered on every mail header as `from "name"` and on every claim
/// line, so it gets `messages::MAX_SUBJECT`'s treatment at label scale (D106). Only an
/// *explicit* name is checked — the auto-generated candidates are ours and bounded by
/// construction.
pub const MAX_NAME: usize = 80;

/// The roster upsert, reporting anything it displaced.
///
/// [`touch`] is the same call for the callers that only need the name.
pub fn register(
    conn: &Connection,
    who: &Identity,
    explicit_name: Option<&str>,
) -> Result<Registered> {
    if let Some(n) = explicit_name {
        let chars = n.chars().count();
        if chars > MAX_NAME {
            return Err(Error::FieldTooLarge {
                field: "name",
                chars,
                max: MAX_NAME,
            });
        }
    }
    // An explicit name gets exactly one attempt: D18 requires a clash to surface as an error, to
    // the agent that can still choose another. An implicit one falls back rather than failing.
    let candidates = match explicit_name {
        Some(n) => vec![n.to_string()],
        None => name_candidates(who),
    };
    // Read the clock and the environment once rather than per attempt: retries are the same
    // visit to the board, by the same session, at the same moment. `now()` is also fallible,
    // which is awkward inside a loop that must return a raw SQLite error.
    let at = now()?;
    let pid = session_pid();
    let last = candidates.len() - 1;
    let mut reclaimed_from = None;
    for (i, candidate) in candidates.iter().enumerate() {
        match try_touch(conn, who, candidate, explicit_name, at, pid) {
            Ok(()) => break,
            Err(e) if is_constraint_violation(&e) && i < last => continue,
            Err(e) if is_constraint_violation(&e) => {
                // The holder may be a session that has ended. `ux_agents_name` is right — D18
                // needs a name to resolve to exactly one agent — but nothing ever reaped the
                // roster, so before this every name a session had ever used was consumed
                // permanently. Only an *explicit* name reclaims: an auto-name has a suffix ladder
                // to fall down and does not need to displace anybody (D75).
                if explicit_name.is_some()
                    && let Some(displaced) = reclaim(conn, who, candidate, at)?
                {
                    try_touch(conn, who, candidate, explicit_name, at, pid).map_err(|e| {
                        Error::Sqlite {
                            context: "recording the agent row after reclaiming a name".into(),
                            source: e,
                        }
                    })?;
                    reclaimed_from = Some(displaced);
                    break;
                }
                return Err(Error::NameTaken {
                    name: candidate.clone(),
                    project: who.project.clone(),
                });
            }
            Err(e) => {
                return Err(Error::Sqlite {
                    context: "recording the agent row".into(),
                    source: e,
                });
            }
        }
    }

    // Read the name back rather than assuming it: an auto-registering call leaves an existing
    // registered name alone, so the effective name is whatever the row now holds.
    let name = conn
        .query_row(
            "SELECT name FROM agents WHERE id = ?1",
            params![who.id],
            |r| r.get(0),
        )
        .map_err(sql("reading back the agent name"))?;
    Ok(Registered {
        name,
        reclaimed_from,
    })
}

/// One attempt at the roster upsert under a specific name.
fn try_touch(
    conn: &Connection,
    who: &Identity,
    name: &str,
    explicit_name: Option<&str>,
    at: f64,
    pid: Option<i64>,
) -> std::result::Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO agents (id, name, project, cwd, pid, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET
           last_seen = ?6,
           project   = ?3,
           cwd       = ?4,
           pid       = ?5,
           name      = COALESCE(?7, agents.name)",
        params![who.id, name, who.project, who.root, pid, at, explicit_name,],
    )?;
    Ok(())
}

/// Any SQLite constraint violation — not only a unique one, and the breadth is deliberate.
///
/// Every caller reads `true` as "the name is taken", which is sound *by the current schema*, not
/// by this match: `ux_agents_name` is the only constraint reachable through `try_touch`, because
/// the PRIMARY KEY is absorbed by `ON CONFLICT(id)` and `register` constructs every NOT NULL
/// value itself (M43). A schema change that adds a reachable constraint widens what "taken"
/// means here without touching this function — the tests below are what would notice.
fn is_constraint_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    )
}

/// A row from the roster.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub project: String,
    pub pid: Option<i64>,
    pub last_seen: f64,
}

/// How long after its last command an agent whose session pid is unknown is still assumed alive.
///
/// Reached only when [`session_pid`] returned `None` — an older Claude Code, a provider without
/// cross-session messaging, or a caller that is not a Claude session at all. Generous on purpose:
/// the cost of being wrong is a message sent to somebody who is not there, and the board keeps it
/// anyway because it is a log, not a queue (D17).
pub const ASSUMED_ALIVE_FOR_SECS: f64 = 15.0 * 60.0;

/// Narrow a stored pid to one `kill(2)` will treat as a single process.
///
/// **`kill` does not mean "one process" for every integer**, and the two exceptions both answer
/// *yes*: `kill(0, sig)` addresses the caller's whole process group and `kill(-1, sig)` addresses
/// every process the caller may signal. Either would report an agent permanently alive, and
/// `kill(-1, ...)` is an alarming syscall to reach by accident. A value past `pid_t` is rejected
/// too, because the cast would truncate — and `4294967296` truncates to exactly `0`.
fn real_pid(pid: i64) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|p| *p > 0)
}

impl AgentRow {
    /// Whether the session still exists.
    ///
    /// `kill(pid, 0)` is a real, kernel-backed answer rather than a heartbeat the client has to
    /// remember to send — but only once `pid` is the *session's* pid rather than the pid of the
    /// `amb` process that wrote the row (D21). It can still be fooled by pid reuse; under D5 the
    /// consequence of being wrong is a message sent to nobody, which is cheap.
    ///
    /// With no pid recorded the answer degrades to recency rather than to `false`, because
    /// "unknown" and "gone" are different and reporting a live peer as gone is what stops an
    /// agent from messaging them.
    pub fn appears_alive(&self, at: f64) -> bool {
        is_alive(self.pid, self.last_seen, at)
    }
}

/// Whether a session with this pid and last-seen time is still running.
///
/// **Extracted so there is one copy of the rule.** `claims::list` needs the same answer, to say
/// whether the holder of a conflicting claim is still around to be messaged — and a liveness rule
/// with a sharp edge (`real_pid` below rejects values where `kill` means something other than
/// "one process") is exactly the kind that must not be written twice.
pub fn is_alive(pid: Option<i64>, last_seen: f64, at: f64) -> bool {
    match pid.and_then(real_pid) {
        // SAFETY: `kill` with signal 0 performs the permission and existence check without
        // delivering a signal, so it cannot affect the target process. `real_pid` has already
        // rejected every value for which `kill` means something other than "one process";
        // an out-of-range survivor simply returns ESRCH.
        Some(pid) => unsafe { libc::kill(pid, 0) == 0 },
        // Not "no pid" — "no pid we can ask about". A nonsense value is as unknown as a
        // missing one, and both degrade to recency rather than to a wrong answer.
        None => at - last_seen < ASSUMED_ALIVE_FOR_SECS,
    }
}

impl AgentRow {
    pub fn to_json(&self, at: f64) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "ref": short_ref(&self.id),
            "name": self.name,
            "project": self.project,
            "pid": self.pid,
            "last_seen": self.last_seen,
            "appears_alive": self.appears_alive(at),
        })
    }
}

/// List the roster, most recently active first.
pub fn list(conn: &Connection, project: Option<&str>) -> Result<Vec<AgentRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, project, pid, last_seen FROM agents
             WHERE (?1 IS NULL OR project = ?1)
             ORDER BY last_seen DESC",
        )
        .map_err(sql("preparing the roster query"))?;
    let rows = stmt
        .query_map(params![project], |r| {
            Ok(AgentRow {
                id: r.get(0)?,
                name: r.get(1)?,
                project: r.get(2)?,
                pid: r.get(3)?,
                last_seen: r.get(4)?,
            })
        })
        .map_err(sql("running the roster query"))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql("reading a roster row"))
}

/// Two repositories claiming one project name.
///
/// **This is a bus defect before it is a memory one.** `messages::inbox` routes a broadcast with
/// `m.to_proj = ?1` — a string comparison — so two repositories whose roots share a basename share
/// a `@project` address, and mail meant for one is delivered into sessions working the other. The
/// vault has the same problem one layer up: notes filed under a colliding name mix two histories.
///
/// Neither failure announces itself. That is the shape `CLAUDE.md` names as this project's
/// recurring one, so the collision is *reported* rather than left to be inferred from mail that
/// arrives in the wrong repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    pub project: String,
    /// The distinct repository roots claiming it, sorted, always two or more.
    pub roots: Vec<String>,
}

/// The directory holding a repository's shared config, following a worktree's pointer.
///
/// A linked worktree's `.git` is a **file** reading `gitdir: <main>/.git/worktrees/<name>`, and its
/// remote lives in the main repository's config, not beside it. Git's own pointer back is the
/// `commondir` file in that directory — `../..` in practice, but read rather than assumed, since
/// it is git's documented indirection and not a layout to hard-code.
///
/// No subprocess. This runs on the `amb agents` path, and shelling out to `git` for a fact that is
/// two file reads away would put a process spawn between a person and their roster.
fn git_common_dir(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let pointer = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = std::path::PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(rel) => Some(gitdir.join(rel.trim())),
        // A gitdir with no `commondir` is not a worktree pointer; use it as it stands.
        Err(_) => Some(gitdir),
    }
}

/// A repository's remote URL — `origin` if it has one, otherwise the first remote declared.
///
/// Hand-parsed rather than shelled out to `git config`, for the reason above. The grammar needed is
/// small and total: section headers in brackets, `key = value` beneath them, and only `url` inside
/// a `[remote "..."]` section is read. Anything unparseable yields `None`, which is treated as
/// "no shared identity" — the conservative direction, since it reports a collision rather than
/// hiding one.
fn remote_url(root: &std::path::Path) -> Option<String> {
    let config = std::fs::read_to_string(git_common_dir(root)?.join("config")).ok()?;
    let mut in_remote = false;
    let mut is_origin = false;
    let mut first: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_remote = line.starts_with("[remote ");
            is_origin = line.starts_with("[remote \"origin\"");
            continue;
        }
        if !in_remote {
            continue;
        }
        if let Some(url) = line.strip_prefix("url") {
            let url = url.trim_start().strip_prefix('=')?.trim().to_string();
            if is_origin {
                return Some(url);
            }
            first.get_or_insert(url);
        }
    }
    first
}

/// Every project name claimed by more than one repository root.
///
/// **Derived from the roster rather than from a registry**, which is the whole design: the board is
/// already the one thing every unrelated session holds, and it already records each agent's root
/// (D20). A separate registry table would be a second copy of a fact the board has, and it could
/// only be written by a session that had *already* registered under the colliding name.
///
/// Detection only. Nothing is blocked and no name is invented — `amb` stays advisory, and a name
/// this function reports is still a name that works.
pub fn collisions(conn: &Connection) -> Result<Vec<Collision>> {
    let mut stmt = conn
        .prepare(
            "SELECT project, cwd FROM agents
             WHERE cwd IS NOT NULL AND cwd <> ''
             GROUP BY project, cwd
             ORDER BY project, cwd",
        )
        .map_err(sql("preparing the collision query"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(sql("running the collision query"))?;

    // Grouped here rather than with a HAVING clause so the roots themselves come back: naming
    // *which* two repositories collide is the whole value, and a count would only say that one did.
    let mut by_project: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let (project, root) = row.map_err(sql("reading a collision row"))?;
        match by_project.last_mut() {
            Some((p, roots)) if *p == project => roots.push(root),
            _ => by_project.push((project, vec![root])),
        }
    }
    // **Distinct roots are not distinct repositories**, and conflating them is the difference
    // between a warning worth reading and one that fires on every legitimate setup. Two worktrees
    // of one repository, and a second clone of it, have different roots, the same name, and
    // genuinely *should* share an address. The remote is the cheap discriminator: same remote,
    // same project. A root with no remote keys on its own path, so two unrelated local-only
    // repositories still collide — absent is not a match.
    //
    // Detection-only makes the error cost asymmetric, which is why a two-file-read heuristic is
    // proportionate here: a missed collision is the silence this exists to break, while a false
    // one is a warning that trains people to ignore warnings.
    Ok(by_project
        .into_iter()
        .filter_map(|(project, roots)| {
            let mut repos: Vec<(String, String)> = roots
                .into_iter()
                .map(|root| {
                    let key = remote_url(std::path::Path::new(&root))
                        .unwrap_or_else(|| format!("path:{root}"));
                    (key, root)
                })
                .collect();
            repos.sort();
            repos.dedup_by(|a, b| a.0 == b.0);
            (repos.len() > 1).then(|| Collision {
                project,
                roots: repos.into_iter().map(|(_, root)| root).collect(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The socket's file name is the pid, and everything else is `None`.
    ///
    /// **This is the rule D93's addressing half rests on**, and it was unreachable by any test
    /// until it was lifted out of the environment read — six mutants survived inside the shell,
    /// including returning `Some(0)` and `Some(-1)` unconditionally. Both matter: `kill(0, sig)`
    /// addresses the caller's whole process group and `kill(-1, sig)` addresses every process the
    /// caller may signal, so either would report every peer permanently alive.
    ///
    /// The format is **observed**, not documented, so the degradation is the feature: anything
    /// unparseable yields `None` and liveness falls back to `last_seen` freshness.
    #[test]
    fn a_socket_path_yields_its_pid_or_nothing_at_all() {
        assert_eq!(pid_from_socket("/tmp/cc-socks/12345.sock"), Some(12345));
        assert_eq!(pid_from_socket("/tmp/cc-socks/1.sock"), Some(1));

        for refused in [
            "/tmp/cc-socks/0.sock",  // kill(0) is the whole process group
            "/tmp/cc-socks/-1.sock", // kill(-1) is every process we may signal
            "/tmp/cc-socks/notapid.sock",
            "/tmp/cc-socks/.sock",
            "/tmp/cc-socks/",
            "",
        ] {
            assert_eq!(
                pid_from_socket(refused),
                None,
                "{refused:?} must degrade to recency, not to a wrong pid"
            );
        }
    }

    /// An unknown pid degrades to recency, and the window has a sharp edge.
    ///
    /// **Found by mutation: `<` could become `<=` and nothing saw it.** The edge is only visible
    /// at exactly [`ASSUMED_ALIVE_FOR_SECS`], which is the one instant no test named.
    ///
    /// The second half is the rule that makes the first safe: a pid `kill(2)` cannot treat as one
    /// process is **as unknown as no pid at all**, never as evidence of death. Reporting a live
    /// peer as gone is what stops an agent from messaging them.
    #[test]
    fn an_unknown_pid_degrades_to_recency_at_a_sharp_edge() {
        let now = 1_000_000.0;
        assert!(is_alive(None, now - ASSUMED_ALIVE_FOR_SECS + 1.0, now));
        assert!(
            !is_alive(None, now - ASSUMED_ALIVE_FOR_SECS, now),
            "exactly the window has elapsed, so it is over rather than still inside it"
        );
        assert!(!is_alive(None, now - ASSUMED_ALIVE_FOR_SECS - 1.0, now));

        // `4294967296` is in the list because it truncates to exactly 0 in a `pid_t` cast.
        // **Written as a number on purpose.** Every assertion above is expressed in terms of
        // `ASSUMED_ALIVE_FOR_SECS` itself, so `15.0 * 60.0` becoming `15.0 + 60.0` — a window of
        // 75 seconds rather than 15 minutes — satisfies all of them and survived mutation. The
        // constant's docstring calls the window *generous*; five minutes is the claim that makes
        // that word mean something.
        assert!(
            is_alive(None, now - 300.0, now),
            "a session that spoke five minutes ago is well inside a deliberately generous window"
        );

        for nonsense in [Some(0), Some(-1), Some(4_294_967_296)] {
            assert!(
                is_alive(nonsense, now - 1.0, now),
                "{nonsense:?} is unknown, and unknown degrades to recency"
            );
            assert!(
                !is_alive(nonsense, now - ASSUMED_ALIVE_FOR_SECS, now),
                "{nonsense:?} still expires by recency rather than answering forever"
            );
        }
    }

    /// The roster's machine surface carries the short ref and the liveness answer.
    ///
    /// **Found by mutation: `AgentRow::to_json` could return an empty value.** `amb agents --json`
    /// is how a peer is addressed — `ref` is the short form the banner tells agents to use — so an
    /// empty document is a roster that names nobody.
    #[test]
    fn the_roster_surface_carries_the_short_ref_and_the_liveness_answer() {
        let row = AgentRow {
            id: "c0a251-7f3e-4b1a".into(),
            name: "alice".into(),
            project: "nest".into(),
            pid: None,
            last_seen: 100.0,
        };
        let doc = row.to_json(101.0);
        assert_eq!(doc["id"], "c0a251-7f3e-4b1a");
        assert_eq!(doc["ref"], short_ref("c0a251-7f3e-4b1a"));
        assert_eq!(doc["name"], "alice");
        assert_eq!(doc["project"], "nest");
        assert_eq!(doc["appears_alive"], true);
        assert_eq!(
            row.to_json(100.0 + ASSUMED_ALIVE_FOR_SECS)["appears_alive"],
            false,
            "the surface reports liveness at the time asked, not at the time stored"
        );
    }

    /// Only a constraint violation is a name collision.
    ///
    /// **Found by mutation: `is_constraint_violation` could return `true` for every error.** Its two
    /// call sites use it as a match guard to decide whether to retry under a different name, so
    /// treating an unrelated failure — a missing table, a locked board — as a name clash would
    /// rename an agent in response to something that has nothing to do with its name.
    #[test]
    fn only_a_constraint_violation_is_a_name_collision() {
        let conn = Connection::open_in_memory().expect("in-memory board");
        conn.execute_batch(
            "CREATE TABLE t (a TEXT);
             CREATE UNIQUE INDEX ux_t ON t(a);
             INSERT INTO t VALUES ('taken');",
        )
        .expect("fixture");

        let clash = conn
            .execute("INSERT INTO t VALUES ('taken')", [])
            .expect_err("a duplicate must fail");
        assert!(is_constraint_violation(&clash), "{clash:?}");

        let unrelated = conn
            .execute("INSERT INTO no_such_table VALUES ('x')", [])
            .expect_err("a missing table must fail");
        assert!(
            !is_constraint_violation(&unrelated),
            "a missing table is not a name clash, and retrying under a new name cannot fix it"
        );
    }

    /// The same rule as the test above, asserted at the two call sites that test names.
    ///
    /// **That test was itself found by mutation, and it guarded the predicate only** (M43). Its
    /// docstring says exactly what would go wrong — *"its two call sites use it as a match guard
    /// … treating an unrelated failure as a name clash would rename an agent in response to
    /// something that has nothing to do with its name"* — and then checks `is_constraint_violation`
    /// against a synthetic table. Forcing either guard to `true` reddened nothing.
    ///
    /// M20's arithmetic, on a rule whose predicate layer was already guarded: count the layers a
    /// rule passes through, count the layers that assert it. A comment naming a call site is not
    /// a test of that call site, however precisely it describes the failure.
    ///
    /// A missing table is `SQLITE_ERROR`, not `SQLITE_CONSTRAINT`, which is the distinction the
    /// predicate draws and the one these guards have to preserve.
    #[test]
    fn a_broken_board_fails_rather_than_reporting_a_name_as_taken() {
        for explicit in [None, Some("alice")] {
            let dir = tempfile::tempdir().expect("tempdir");
            let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
            let who = Identity {
                id: "uuid-alice".into(),
                name: "alice".into(),
                project: "nest".into(),
                root: dir.path().to_string_lossy().into_owned(),
            };
            conn.execute_batch("DROP TABLE agents;")
                .expect("break the board");

            let err = register(&conn, &who, explicit)
                .expect_err("a board with no roster table cannot be registered into");
            match err {
                Error::Sqlite { ref context, .. } => assert_eq!(
                    context, "recording the agent row",
                    "it must fail where it actually failed, explicit={explicit:?}"
                ),
                other => panic!(
                    "a broken board must surface as a failure, not a name clash                      (explicit={explicit:?}): {other:?}"
                ),
            }
        }
    }

    /// D106: a display name is rendered as a label on every mail header and claim line, so an
    /// explicit one is capped where its author can still choose another. Auto-names are exempt
    /// by construction — the fallback ladder must never be able to fail on length.
    #[test]
    fn an_explicit_name_past_the_cap_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let who = Identity {
            id: "uuid-me".into(),
            name: "me".into(),
            project: "nest".into(),
            root: dir.path().to_string_lossy().into_owned(),
        };
        let long = "n".repeat(MAX_NAME + 1);
        let err = register(&conn, &who, Some(&long)).expect_err("past the cap");
        assert!(
            matches!(err, Error::FieldTooLarge { field: "name", .. }),
            "{err:?}"
        );
        let exact = "n".repeat(MAX_NAME);
        register(&conn, &who, Some(&exact)).expect("exactly at the cap is fine");
    }

    /// Reclamation must not swallow a real failure as "this name is not available".
    ///
    /// The sibling of the test above, at the other call site. `reclaim` returning `Ok(None)` means
    /// *the name stays taken*, and `register` then reports `NameTaken` — so a board that cannot be
    /// written reads to the agent as a naming problem it could fix by choosing another name.
    ///
    /// The trigger is how a failure is induced on the `UPDATE` while the `SELECT` above it still
    /// succeeds: its body names a table that does not exist, so it raises `SQLITE_ERROR` rather
    /// than a constraint violation. `RAISE(ABORT)` would not work here — that *is* a constraint
    /// violation, and `is_constraint_violation` matches any of them, which is worth knowing given the
    /// function's name says unique.
    #[test]
    fn a_failed_reclamation_is_an_error_and_not_a_name_that_stays_taken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let at = 1_000_000.0;
        conn.execute(
            "INSERT INTO agents (id, name, project, cwd, pid, first_seen, last_seen)
             VALUES ('uuid-dead', 'shared', 'nest', '/tmp', 999999999, ?1, ?1)",
            params![at - 10.0],
        )
        .expect("seed a holder whose session has ended");
        conn.execute_batch(
            "CREATE TRIGGER break_update BEFORE UPDATE ON agents BEGIN
               INSERT INTO no_such_table VALUES (1);
             END;",
        )
        .expect("arm the failing update");

        let who = Identity {
            id: "uuid-me".into(),
            name: "shared".into(),
            project: "nest".into(),
            root: dir.path().to_string_lossy().into_owned(),
        };
        let err = reclaim(&conn, &who, "shared", at)
            .expect_err("the rename failed, so the reclamation did not happen");
        assert!(
            matches!(err, Error::Sqlite { .. }),
            "a failed rename is a failure, not an unavailable name: {err:?}"
        );
    }

    /// The roster comes back, most recently active first.
    ///
    /// **Found by mutation: `list` could return an empty vector.** `amb agents` is how one session
    /// discovers another exists, and the `SessionStart` banner names it first — an empty roster is
    /// a board where nobody can address anybody, reported as an ordinary quiet board.
    #[test]
    fn the_roster_comes_back_and_the_project_filter_narrows_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let who = |id: &str, name: &str, project: &str| Identity {
            id: id.to_string(),
            name: name.to_string(),
            project: project.to_string(),
            root: dir.path().to_string_lossy().into_owned(),
        };
        for (id, name, project) in [
            ("uuid-alice", "alice", "nest"),
            ("uuid-bob", "bob", "nest"),
            ("uuid-carol", "carol", "elsewhere"),
        ] {
            touch(&conn, &who(id, name, project), Some(name)).expect("register");
        }

        let all = list(&conn, None).expect("roster");
        assert_eq!(all.len(), 3, "every registered agent is on the roster");
        assert!(
            all[0].last_seen >= all[all.len() - 1].last_seen,
            "most recently active first: {:?}",
            all.iter().map(|a| a.last_seen).collect::<Vec<_>>()
        );

        let here = list(&conn, Some("nest")).expect("roster");
        let names: Vec<&str> = here.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(here.len(), 2, "the project filter narrows: {names:?}");
        assert!(!names.contains(&"carol"), "{names:?}");
    }

    /// Two projects colliding are two collisions, not one run together.
    ///
    /// **Found by mutation: the grouping guard `*p == project` could be `true`.** The rows arrive
    /// `ORDER BY project, cwd`, so with the guard always true every row joins whichever group is
    /// last and four roots across two projects report as one project with four roots. The e2e
    /// fixture that covers this function uses a single project name, so it cannot see the
    /// difference — the same fixture-shaped blindness M19 is about.
    ///
    /// None of these roots exists on disk, which exercises the documented fallback: a root with
    /// no readable remote keys on its own path, so unrelated local-only repositories still
    /// collide. Absent is not a match.
    #[test]
    fn two_projects_colliding_are_two_collisions_not_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        for (id, name, project, root) in [
            ("uuid-a1", "a1", "alpha", "/nowhere/one/alpha"),
            ("uuid-a2", "a2", "alpha", "/nowhere/two/alpha"),
            ("uuid-b1", "b1", "beta", "/nowhere/one/beta"),
            ("uuid-b2", "b2", "beta", "/nowhere/two/beta"),
        ] {
            let who = Identity {
                id: id.into(),
                name: name.into(),
                project: project.into(),
                root: root.into(),
            };
            touch(&conn, &who, Some(name)).expect("register");
        }

        let found = collisions(&conn).expect("collisions");
        let names: Vec<&str> = found.iter().map(|c| c.project.as_str()).collect();
        assert_eq!(names, ["alpha", "beta"], "each project collides on its own");
        for c in &found {
            assert_eq!(c.roots.len(), 2, "two roots each, not four in one: {c:?}");
        }
    }

    /// A name stays taken when the holder's own fallback is taken too.
    ///
    /// **The scenario is described in a comment on `reclaim` and was tested nowhere**: *"If the
    /// fallback is itself taken this returns a unique violation, the reclamation does not happen,
    /// and the caller reports `NameTaken` as before. Failing closed is right: the point is to free
    /// a name, not to start a cascade of renames."*
    ///
    /// Mutation found the half that matters. With `is_constraint_violation` forced to `false` in that
    /// match, the clash stops being an ordinary outcome and becomes a raw SQLite error — so an
    /// agent asking for a taken name gets an internal failure instead of D18's clash, on the one
    /// path that is supposed to fail closed.
    ///
    /// The rows are inserted directly because the holder's *stored* pid is the input under test;
    /// going through `register` would read this process's environment instead.
    #[test]
    fn a_name_stays_taken_when_the_holders_own_fallback_is_taken_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let at = crate::db::now().expect("now");

        // A holder that has provably ended: a pid nothing is running under.
        conn.execute(
            "INSERT INTO agents(id, name, project, cwd, pid, first_seen, last_seen)
             VALUES ('uuid-dead-holder', 'shared', 'nest', '/x', 999999999, ?1, ?1)",
            params![at],
        )
        .expect("the dead holder");

        // And somebody already sitting on the name it would be renamed to.
        let fallback = default_name("nest", "uuid-dead-holder");
        conn.execute(
            "INSERT INTO agents(id, name, project, cwd, pid, first_seen, last_seen)
             VALUES ('uuid-blocker', ?2, 'nest', '/y', NULL, ?1, ?1)",
            params![at, fallback],
        )
        .expect("the blocker");

        let newcomer = Identity {
            id: "uuid-new".into(),
            name: "shared".into(),
            project: "nest".into(),
            root: "/z".into(),
        };
        let err = register(&conn, &newcomer, Some("shared")).expect_err("the name must stay taken");
        assert!(
            matches!(err, Error::NameTaken { .. }),
            "failing closed means D18's clash, not an internal error: {err}"
        );
    }

    /// A name is taken only from a session that has provably ended.
    ///
    /// **The unsafe direction is taking a live session's name**, so `is_alive`'s "unknown means
    /// alive" degradation is load-bearing here: a holder with no usable pid whose last command is
    /// recent keeps its name. Wrongly refusing costs a suffix; wrongly taking costs a live
    /// session its identity (D21, D75).
    #[test]
    fn a_name_is_reclaimed_only_from_a_session_that_has_ended() {
        let now = 1_000_000.0;
        let dead_pid = Some(999_999_999);

        assert!(
            reclaimable("them", "me", dead_pid, now - 10.0, now),
            "a dead holder's name is available even if it was seen a moment ago"
        );
        assert!(
            !reclaimable("me", "me", dead_pid, now - 10.0, now),
            "re-registering your own name is a renewal, never a reclamation"
        );
        // No pid, seen recently: unknown, and unknown must count as alive.
        assert!(
            !reclaimable("them", "me", None, now - 60.0, now),
            "a holder we cannot ask about keeps its name"
        );
        // No pid, long silent: past the assumed-alive window.
        assert!(reclaimable(
            "them",
            "me",
            None,
            now - ASSUMED_ALIVE_FOR_SECS - 1.0,
            now
        ));
    }

    #[test]
    fn short_ref_matches_the_listagents_display_width() {
        assert_eq!(short_ref("14e7b964-f5ac-4cb9-9191-9780a01cd1a4"), "14e7b9");
    }

    #[test]
    fn short_ref_does_not_panic_on_a_short_id() {
        assert_eq!(short_ref("ab"), "ab");
        assert_eq!(short_ref(""), "");
    }

    #[test]
    fn default_name_mirrors_listagents() {
        assert_eq!(
            default_name("nestwatch", "4beea2aa-0000-0000-0000-000000000000"),
            "nestwatch-4beea2"
        );
    }

    #[test]
    fn a_live_pid_reads_alive_and_a_bogus_one_does_not() {
        let me = AgentRow {
            id: "x".into(),
            name: "x".into(),
            project: "p".into(),
            pid: Some(std::process::id() as i64),
            last_seen: 0.0,
        };
        assert!(
            me.appears_alive(0.0),
            "the running test process must look alive"
        );

        let ghost = AgentRow {
            pid: Some(0x7FFF_FFF0),
            ..me.clone()
        };
        assert!(
            !ghost.appears_alive(0.0),
            "an implausible pid must not look alive"
        );
    }

    #[test]
    fn a_pid_kill_would_misread_is_treated_as_unknown() {
        // D21. `kill(0, 0)` asks about the caller's whole process group and `kill(-1, 0)` about
        // every process it may signal; both succeed, so either would report an agent alive
        // forever. 4294967296 is here because it truncates to exactly 0 in a 32-bit `pid_t`.
        let base = AgentRow {
            id: "x".into(),
            name: "x".into(),
            project: "p".into(),
            pid: None,
            last_seen: 0.0,
        };
        for bogus in [0, -1, -12345, 4_294_967_296, i64::MAX] {
            let row = AgentRow {
                pid: Some(bogus),
                ..base.clone()
            };
            assert!(
                !row.appears_alive(ASSUMED_ALIVE_FOR_SECS + 1.0),
                "pid {bogus} must not read as alive"
            );
        }
        // And a nonsense pid is *unknown*, not *gone*: inside the freshness window it still reads
        // alive, exactly as a missing pid would.
        let recent = AgentRow {
            pid: Some(0),
            last_seen: 1_000.0,
            ..base
        };
        assert!(recent.appears_alive(1_060.0));
    }

    #[test]
    fn an_unknown_pid_degrades_to_recency_not_to_gone() {
        // "unknown" and "gone" are different answers. Reporting a live peer as gone is what
        // stops an agent from messaging them at all.
        let row = AgentRow {
            id: "x".into(),
            name: "x".into(),
            project: "p".into(),
            pid: None,
            last_seen: 1_000.0,
        };
        assert!(row.appears_alive(1_060.0), "seen a minute ago");
        assert!(
            !row.appears_alive(1_000.0 + ASSUMED_ALIVE_FOR_SECS + 1.0),
            "and not once the window has passed"
        );
    }

    #[test]
    fn the_repo_root_is_found_from_a_subdirectory() {
        // The whole of D20 in one assertion: `cd src/auth` must not change which project you
        // are in.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
        let deep = root.join("src").join("auth");
        std::fs::create_dir_all(&deep).expect("mkdir deep");

        assert_eq!(repo_root(&deep, None).as_deref(), Some(root));
        assert_eq!(repo_root(root, None).as_deref(), Some(root));
    }

    #[test]
    fn a_worktree_is_found_even_though_its_dot_git_is_a_file() {
        // `git worktree add` writes a `.git` *file* holding a `gitdir:` pointer, and a submodule
        // does the same. An `is_dir` check here would silently fail for exactly the worktree
        // setup RESEARCH.md R3 says these agents run in.
        let dir = tempfile::tempdir().expect("tempdir");
        let wt = dir.path().join("wt-feature");
        std::fs::create_dir_all(wt.join("src")).expect("mkdir");
        std::fs::write(
            wt.join(".git"),
            "gitdir: /elsewhere/.git/worktrees/wt-feature",
        )
        .expect("write gitfile");

        assert_eq!(repo_root(&wt.join("src"), None).as_deref(), Some(&*wt));
    }

    #[test]
    fn the_walk_stops_below_home() {
        // A dotfiles repository at ~/.git is common. Without this guard every non-repo project
        // on the machine collapses into one namespace named after the home directory — strictly
        // worse than the bug the walk exists to fix.
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path();
        std::fs::create_dir_all(home.join(".git")).expect("mkdir ~/.git");
        let scratch = home.join("scratch");
        std::fs::create_dir_all(&scratch).expect("mkdir");

        let home_s = home.to_string_lossy().into_owned();
        assert_eq!(
            repo_root(&scratch, Some(&home_s)),
            None,
            "a dotfiles repo must not swallow every directory beneath it"
        );
        assert!(
            repo_root(&scratch, None).is_some(),
            "and without the guard it would — which is what the guard is for"
        );
    }

    #[test]
    fn a_directory_outside_any_repository_has_no_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Bounded by HOME so the walk cannot escape into a real repository above the tempdir.
        let home = dir.path().to_string_lossy().into_owned();
        let sub = dir.path().join("plain");
        std::fs::create_dir_all(&sub).expect("mkdir");
        assert_eq!(repo_root(&sub, Some(&home)), None);
    }
}

//! Thin binary: parse arguments, call into the library, map errors to exit codes.
//!
//! No logic lives here. That is what lets the tests exercise real code paths rather than
//! shelling out for every assertion, and it is why `amb`'s behaviour can be tested without a
//! process at all — except where the point *is* the process, as in the concurrency tests.

use amb::address;
use amb::claims;
use amb::db;
use amb::delivery;
use amb::doctor;
use amb::error::Error;
use amb::hooks;
use amb::identity;
use amb::memory;
use amb::messages::{self, Outgoing};
use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "amb",
    version = amb::version::banner(),
    about = "Message bus for concurrent agent sessions"
)]
struct Cli {
    /// Emit machine-readable JSON. Global, so an agent never has to remember which subcommands
    /// support it.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Send a message. The only write path — there is deliberately no outbox (D10).
    Send {
        /// `alice`, `alice@nestwatch`, `@nestwatch`, or `@` for everyone in this project.
        to: String,
        #[arg(long)]
        subject: String,
        /// The message. Use --body-file for anything multi-line.
        #[arg(long, required_unless_present = "body_file")]
        body: Option<String>,
        /// Read the body from a file, or from stdin with `-`.
        ///
        /// A body composed on the command line has to survive shell quoting, which is the
        /// highest-friction part of the client contract and the likeliest way to mangle a
        /// message. This is the escape hatch.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Free-form: note, question, proposal, claim_notice.
        #[arg(long, default_value = "note")]
        kind: String,
        #[arg(long)]
        thread: Option<String>,
        /// Stable caller-supplied id. Sending twice with the same one delivers once (D6).
        #[arg(long = "id")]
        ext_id: Option<String>,
    },
    /// Show messages addressed to this agent or broadcast to its project.
    Inbox {
        /// Hide messages this agent has already acknowledged with `amb read`.
        #[arg(long)]
        unread: bool,
    },
    /// Acknowledge messages. The only thing that marks one read (D9).
    Read {
        /// One or more ids. Acknowledging sixty messages used to mean sixty invocations.
        #[arg(required_unless_present = "all")]
        ids: Vec<i64>,
        /// Acknowledge everything currently unread.
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
    /// Reply to a message, keeping its thread. A reply to a broadcast goes to its sender.
    Reply {
        id: i64,
        #[arg(long)]
        body: String,
    },
    /// Record a display name for this session. Optional — every command registers (D12).
    Register {
        #[arg(long)]
        name: Option<String>,
    },
    /// Take an advisory claim on a path. Never blocks; reports any conflict (D5, D14).
    Claim {
        /// A file, or a directory prefix like `src/auth/` covering everything beneath it.
        path: String,
        /// Why, in a few words, so a peer can judge whether to wait or interrupt.
        #[arg(long)]
        intent: Option<String>,
        /// How long, e.g. 30m, 4h, 2d. Defaults to 4h; re-claiming extends it.
        #[arg(long)]
        ttl: Option<String>,
    },
    /// Release a claim held by this agent.
    Release { path: String },
    /// Show who holds what. Expired rows are shown too unless --live.
    Claims {
        #[arg(long)]
        project: Option<String>,
        /// Hide claims that have already lapsed.
        #[arg(long)]
        live: bool,
        /// One row per claim instead of one line per holder-and-directory.
        #[arg(long)]
        raw: bool,
    },
    /// Block until mail arrives. Backs `monitor` delivery mode; run it under a Monitor tool.
    Watch {
        /// Give up after this many seconds and exit successfully with nothing.
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        /// How often to re-check, in milliseconds.
        ///
        /// **Floored, because zero is a busy loop and the caller is usually a model.** The wait is
        /// `sleep(poll)` around a real query, so `--poll 0` sleeps for nothing and re-runs
        /// `deliverable()` as fast as the process can issue it — for the whole timeout, against
        /// the board every other session on this machine is using. The banner tells agents to run
        /// `amb watch --timeout 300 --json`, and a number in a banner is a number that gets tuned.
        /// Rejected by clap at parse time, which is already exit 64's contract.
        #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u64).range(messages::MIN_POLL_MS..))]
        poll: u64,
    },
    /// Install the delivery hooks into ~/.claude/settings.json (D9).
    Install {
        /// session = at session start only · turn = also between turns · monitor = also blocking.
        #[arg(long, default_value = "turn")]
        mode: String,
        /// Show the change without writing it.
        #[arg(long)]
        dry_run: bool,
        /// Also install the memory hooks, as their own entry with their own timeout.
        ///
        /// Off by default: this is an experiment, and `PreToolUse` fires on every file tool
        /// call. The flag describes the *complete* desired hook state, so a later `amb install`
        /// without it takes the memory hooks back out.
        #[arg(long)]
        memory: bool,
    },
    /// Remove the delivery hooks, leaving other tools' hooks untouched.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
    },
    /// Internal: invoked by the installed hooks. Never fails, never blocks a session.
    #[command(hide = true)]
    Hook {
        /// The installed mode: session, turn or monitor.
        mode: String,
    },
    /// Record and recall what past sessions learned. Needs `AMB_VAULT` — unset means off.
    Memory {
        #[command(subcommand)]
        what: MemoryCommand,
    },
    /// Write a markdown snapshot of the board to a file, for a reader that cannot open it.
    ///
    /// **Not a delivery**: nothing is marked read, and the path must be outside every repository
    /// (D11). Exists to answer one question cheaply — whether reading the board changes what a
    /// separate assistant says — before anything is built to serve it properly.
    Snapshot {
        /// Where to write it. No default: `amb` does not choose where to put a file (D35's rule).
        path: String,
        /// Include mail already read, not just unread.
        #[arg(long)]
        all: bool,
    },
    /// Report what is wrong with this installation, especially what fails silently.
    ///
    /// Assembles facts the library already computes. Always exits 0 — it reports a diagnosis, it
    /// is not itself a failure — so `--json` carries the verdict in `worst`.
    Doctor,

    /// List agents known to the board.
    Agents {
        #[arg(long)]
        project: Option<String>,
        /// Only agents whose process still exists.
        #[arg(long)]
        live: bool,
    },
}

/// The memory surface.
///
/// A subcommand tree rather than four top-level verbs, because this is an experiment behind a
/// kill switch and its commands should read as one thing that can be switched off. `amb memory`
/// with no vault configured fails with the variable's name in the message, never silently.
#[derive(Subcommand)]
enum MemoryCommand {
    /// Record one thing this session learned.
    Observe {
        /// One line. This becomes the note's filename and the line an injection renders.
        #[arg(long)]
        title: String,
        /// What was learned, in prose. Wrap anything sensitive in `<private>…</private>`.
        #[arg(long)]
        learned: String,
        /// Repo-relative paths this concerns — the primary retrieval key.
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
        /// Ids of notes that prompted this one. **This echo is the whole measurement.**
        #[arg(long, value_delimiter = ',')]
        cites: Vec<String>,
        /// Retire an earlier note this one contradicts.
        #[arg(long)]
        supersedes: Option<String>,
        /// How binding this is: `advice` (default), `decision`, or `rule`. Ranks it ahead of
        /// others under the injection cap. Never denies anything (D52, D64).
        #[arg(long, default_value = "advice")]
        force: String,
        /// This is another sighting of an existing candidate — the plan's dedup affordance.
        /// Replaces fuzzy matching with a checkable record; a miss makes a visible duplicate
        /// rather than a silent wrong merge.
        #[arg(long)]
        same_as: Option<String>,
        /// Record against another project. Defaults to this one.
        #[arg(long)]
        project: Option<String>,
    },
    /// Search the vault yourself.
    Recall {
        /// Matched against titles and excerpts. Omit to list the most recent.
        query: Option<String>,
        /// What is known about one path, in any project on this machine.
        #[arg(long)]
        file: Option<String>,
        /// Answer the cross-repo question explicitly: who touched this path, in any repo here.
        /// Foreign results first, because the local ones you already had.
        #[arg(long, requires = "file")]
        across_repos: bool,
        #[arg(long)]
        project: Option<String>,
        /// Every project, not just this one.
        #[arg(long, conflicts_with = "project")]
        all_projects: bool,
        /// Floored at 1: `--limit 0` returned nothing, which is indistinguishable from a search
        /// that missed — the distinction D89 exists to make.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..))]
        limit: u64,
    },
    /// Record that something was noticed again — the three-strikes ledger (Phase 2, D49).
    Derive {
        /// Slug of the candidate. Reuse it to add a strike; a new one starts a candidate.
        slug: String,
        #[arg(long)]
        title: String,
        /// What was noticed this time. One line of it goes into the ledger.
        #[arg(long)]
        note: String,
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
    },
    /// Candidates and how close each is to being offered.
    Candidates {
        /// Only those at the threshold and not declined since their last derivation.
        #[arg(long)]
        ready: bool,
    },
    /// Promote one candidate. **One at a time, and it shows you the derivations first.**
    Promote {
        /// A candidate id, e.g. `candidate/auth-lock-ordering`.
        id: String,
        /// Record that you declined it. Not re-offered until it derives again.
        #[arg(long, conflicts_with = "yes")]
        decline: bool,
        /// Confirm. Without this the offer is shown and nothing is written.
        #[arg(long)]
        yes: bool,
        /// Overrule where the ledger would file it: `nest`, `#rust`, or `@@`. The evidence
        /// decides by default; this is for when it decides wrong (D82).
        #[arg(long, value_name = "SCOPE")]
        scope: Option<String>,
        /// Promote something obviously important on first sight, without waiting for three
        /// derivations. **Frequency favours trivia, so judgement needs an override** — and this
        /// is the override, which is why it still requires --yes.
        #[arg(long)]
        direct: bool,
    },
    /// Publish a project's decisions into the repository they govern (Phase 3, D49).
    Export {
        /// Defaults to this project.
        project: Option<String>,
        /// Where the repository is. Defaults to the current one.
        #[arg(long)]
        repo: Option<String>,
        /// Report drift and exit non-zero instead of writing. Wire this into CI.
        #[arg(long)]
        check: bool,
    },
    /// Record what a session did, from its transcript. **No model involved** (Phase 4b).
    Capture {
        /// Transcript to read. Defaults to `$CLAUDE_CODE_TRANSCRIPT_PATH` when set.
        #[arg(long)]
        transcript: Option<String>,
        /// A one-line summary. On a `Stop` hook this is `last_assistant_message`, which the
        /// reference says to prefer over the transcript — that file is written asynchronously and
        /// may not yet contain the turn a hook is firing on.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Retire candidates that went 30 days without a new derivation.
    Expire,
    /// Show what a note replaced and what replaced it, walking the supersession chain.
    ///
    /// `amb` could retire a note and then not say why or what took its place — the edge was in the
    /// file and nowhere queryable (D63).
    History {
        /// A note id: `project/slug`, or `kind/slug` for a pattern.
        id: String,
    },
    /// Bring the index in step with the vault, and report what the notes say that cannot be true.
    ///
    /// Incremental: files whose mtime the index already knows are skipped, so this is not a
    /// forced rebuild — which is why D67's repair had to clear `mtime` rather than the derived
    /// column it wanted rebuilt.
    ///
    /// This walks every kind and every project, unbounded, because a person is waiting at a
    /// terminal. The `SessionStart` hook does *not* run it: it calls `sync_dir` for this
    /// project's observations alone, capped at `AUTO_INDEX_LIMIT`, because a full re-read every
    /// session would be unmeasured work inside D9's timing guarantee.
    Index,
    /// How much of what sessions actually edit is covered by a note. Read-only.
    Coverage {
        /// Which project to measure. Defaults to this one.
        #[arg(long)]
        project: Option<String>,
    },
    /// Is this actually capturing, and is anything injected ever used?
    Status {
        /// Window for the citation ratio, in days. Overrides the open measurement window;
        /// omit to use that, or `--all-time` for everything ever recorded.
        #[arg(long, conflicts_with = "all_time")]
        days: Option<u32>,
        /// Ignore the measurement window and count every event on the board.
        #[arg(long)]
        all_time: bool,
    },
    /// When D59's measurement window opened, and opening it (D87).
    Window {
        /// Start the window now. Refuses if one is already open.
        #[arg(long)]
        open: bool,
        /// Restart an already-open window, discarding what it had measured so far.
        ///
        /// Refused alongside `--open`: the two ask for opposite things about an existing window,
        /// and silently preferring one would discard a measurement nobody asked to discard.
        #[arg(long, conflicts_with = "open")]
        reopen: bool,
    },
}

/// Whether this process was invoked as a hook, decided from raw argv.
///
/// **Needed because the answer is required *before* the arguments parse** (D97). `hook_main`'s
/// exit-0 guarantee lives downstream of `Cli::parse`, so a malformed hook invocation never reaches
/// it — clap exits the process first, and clap's failure code is 2, the one code Claude Code reads
/// as *block*. On `Stop` that prevents the session from stopping.
///
/// Deliberately the first positional and nothing cleverer. `plan_install` writes
/// `<exe> hook <mode>`, so the token is at a known place; matching it anywhere in argv would let
/// `amb send bob --body hook` silence a real usage error.
fn invoked_as_hook() -> bool {
    std::env::args_os().nth(1).is_some_and(|a| a == "hook")
}

fn main() -> ExitCode {
    // **`try_parse`, not `parse`, and the reason is D9 rather than tidiness** (D97). `parse`
    // terminates the process itself on a bad argument, so neither branch below can run.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // `--help` and `--version` are modelled as errors by clap and are not failures. Its own
        // `use_stderr` is the discriminator: real errors go to stderr, these two to stdout.
        Err(e) if !e.use_stderr() => {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        // A hook with arguments this build cannot parse — an entry written by another version,
        // or edited by hand. **Silent, and exit 0**, exactly as every other hook failure is
        // (D9). Saying nothing is right here for the same reason it is in `hook_main`: this runs
        // inside somebody else's session and mail delivery must never break one.
        Err(_) if invoked_as_hook() => return ExitCode::SUCCESS,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(amb::error::exit::USAGE);
        }
    };

    // Intercepted before anything else. A hook runs in *every* session on this machine, so it
    // must never fail, never block and never create state for a user who does not use the board.
    if let Command::Hook { ref mode } = cli.command {
        return hook_main(mode);
    }

    let json = cli.json;
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // A caller that asked for JSON gets JSON here too, on stdout where its parser is
            // already looking. Prose on stderr left it with an empty stream and nothing to read.
            if json {
                print_json(&serde_json::json!({
                    "error": {
                        "kind": e.kind(),
                        "message": e.to_string(),
                        "causes": e.causes(),
                    },
                    "exit_code": e.exit_code(),
                }));
            }
            // Always on stderr as well: the human reading a terminal is a caller too, and the
            // full chain matters because the underlying SQLite or IO error is usually the part
            // that says what to do about it.
            eprintln!("amb: {e}");
            for cause in e.causes() {
                eprintln!("  caused by: {cause}");
            }
            ExitCode::from(e.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    // Hook management touches only ~/.claude/settings.json, never the board. Handled before
    // the database is opened, so `amb install` does not create one — the hooks then stay inert
    // for every session on the machine until somebody actually sends something (D9).
    match cli.command {
        Command::Install {
            ref mode,
            dry_run,
            memory,
        } => {
            let mode = hooks::Mode::parse(mode).ok_or_else(|| Error::BadAddress {
                input: mode.clone(),
                reason: "expected session, turn or monitor".into(),
            })?;
            let exe = std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "amb".to_string());
            let path = hooks::settings_path()?;
            // The whole read-plan-write cycle lives in the library, where it is testable and
            // where its retry loop is not logic sitting in the binary (D78, D99).
            let done = hooks::apply(&path, dry_run, |cur| {
                hooks::plan_install(cur, &exe, mode, memory)
            })?;
            return report_plan(&cli, &path, &done, dry_run, mode.as_str());
        }
        Command::Uninstall { dry_run } => {
            let path = hooks::settings_path()?;
            let done = hooks::apply(&path, dry_run, hooks::plan_uninstall)?;
            return report_plan(&cli, &path, &done, dry_run, "removed");
        }
        _ => {}
    }

    let mut me = identity::resolve()?;
    let mut conn = db::open()?;

    // Auto-registration, before anything else touches the board: a session that never called
    // `register` is still a first-class agent (D12).
    let explicit = match &cli.command {
        Command::Register { name } => name.as_deref(),
        _ => None,
    };
    // The effective name may differ from the computed default: a session that registered as
    // "alice" keeps that name on every later auto-registering call.
    let registration = identity::register(&conn, &me, explicit)?;
    me.name = registration.name.clone();

    match cli.command {
        Command::Send {
            ref to,
            ref subject,
            ref body,
            ref body_file,
            ref kind,
            ref thread,
            ref ext_id,
        } => {
            let body = read_body(body.as_deref(), body_file.as_deref())?;
            let addr = address::parse(to)?;
            // Resolve the human-written name to an agent id *before* writing anything. An
            // unknown name must fail here rather than be stored as an undeliverable row.
            let rcpt = messages::resolve_recipient(&conn, &addr, &me)?;

            // A broadcast to a project nobody has ever registered in is accepted — `@project`
            // addresses a *place*, and a place may be occupied tomorrow (D17). But that argument
            // covers a project that does not exist *yet*, not a transposed letter in one that
            // does, so say so rather than swallowing it (D26).
            let warning = match rcpt.agent_id {
                None => messages::unknown_project(&conn, rcpt.project.as_deref())?,
                Some(_) => None,
            };

            let id = messages::send(
                &mut conn,
                &me,
                &Outgoing {
                    to: &rcpt,
                    subject,
                    body: &body,
                    kind,
                    thread: thread.as_deref(),
                    ext_id: ext_id.as_deref(),
                },
            )?;
            if cli.json {
                print_json(&serde_json::json!({
                    "sent": id, "to": to, "warning": warning,
                }));
            } else {
                println!("sent #{id} to {to}");
                if let Some(w) = &warning {
                    println!("  note: {w}");
                }
            }
        }

        Command::Inbox { unread } => {
            let msgs = messages::inbox(&conn, &me, unread)?;
            if cli.json {
                let items: Vec<_> = msgs.iter().map(messages::Message::to_json).collect();
                print_json(
                    &serde_json::json!({ "agent": me.name, "count": items.len(), "messages": items }),
                );
            } else {
                println!("{}", delivery::render_inbox(&msgs, &me.name, &me.project));
            }
        }

        Command::Read { ref ids, all } => {
            let ids = if all {
                messages::mark_read_all(&mut conn, &me)?
            } else {
                messages::mark_read_many(&conn, &me, ids)?;
                ids.clone()
            };
            if cli.json {
                print_json(&serde_json::json!({ "read": ids, "count": ids.len() }));
            } else if ids.is_empty() {
                println!("nothing unread");
            } else {
                let list: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
                println!("marked {} read", list.join(" "));
            }
        }

        Command::Reply { id, ref body } => {
            let new_id = messages::reply(&mut conn, &me, id, body)?;
            if cli.json {
                print_json(&serde_json::json!({ "sent": new_id, "in_reply_to": id }));
            } else {
                println!("sent #{new_id} in reply to #{id}");
            }
        }

        Command::Register { ref name } => {
            let shown = name.clone().unwrap_or_else(|| me.name.clone());
            if cli.json {
                print_json(&serde_json::json!({
                    "id": me.id, "ref": identity::short_ref(&me.id),
                    "name": shown, "project": me.project,
                    // Absent on an ordinary registration. Named so a reader can tell two
                    // sessions apart rather than reading one continuous identity (D75).
                    "reclaimed_from": registration.reclaimed_from,
                }));
            } else {
                if let Some(displaced) = &registration.reclaimed_from {
                    println!(
                        "reclaimed {shown} from a session that has ended — it is now {displaced}"
                    );
                }
                println!(
                    "registered {} [{}] in {}",
                    shown,
                    identity::short_ref(&me.id),
                    me.project
                );
            }
        }

        Command::Claim {
            ref path,
            ref intent,
            ref ttl,
        } => {
            let ttl = match ttl {
                Some(t) => Some(amb::duration::parse(t)?),
                None => None,
            };
            let taken = claims::take(
                &conn,
                &me,
                path,
                intent.as_deref(),
                ttl,
                claims::Source::Declared,
            )?;
            let at = db::now()?;
            if cli.json {
                print_json(&serde_json::json!({
                    "path": taken.path,
                    "renewed": taken.renewed,
                    "expires_at": taken.expires_at,
                    "conflicts": taken.conflicts.iter().map(|c| c.to_json(at)).collect::<Vec<_>>(),
                }));
            } else {
                let verb = if taken.renewed { "extended" } else { "claimed" };
                println!(
                    "{verb} {} ({})",
                    taken.path,
                    amb::duration::humanise(taken.expires_at - at)
                );
                // Announced, never blocked. The agent decides what to do about it (D14).
                for line in claims::summarise(&taken.conflicts, at) {
                    println!("  ! also claimed by {line}");
                }
                if !taken.conflicts.is_empty() {
                    println!("  claims are advisory — message the holder before continuing");
                }
            }
        }

        Command::Release { ref path } => {
            claims::release(&conn, &me, path)?;
            if cli.json {
                print_json(&serde_json::json!({ "released": path }));
            } else {
                println!("released {path}");
            }
        }

        Command::Claims {
            ref project,
            live,
            raw,
        } => {
            let rows = claims::list(&conn, project.as_deref().or(Some(&me.project)), live)?;
            let at = db::now()?;
            if cli.json {
                let items: Vec<_> = rows.iter().map(|c| c.to_json(at)).collect();
                print_json(&serde_json::json!({ "count": items.len(), "claims": items }));
            } else if rows.is_empty() {
                println!("no claims");
            } else if raw {
                for c in &rows {
                    println!(
                        "{} · {} · {} · {}",
                        c.path,
                        c.holder(),
                        c.source,
                        amb::duration::humanise(c.remaining(at))
                    );
                }
            } else {
                for line in claims::summarise(&rows, at) {
                    println!("{line}");
                }
            }
        }

        Command::Watch { timeout, poll } => {
            let found = messages::watch(
                &mut conn,
                &me,
                std::time::Duration::from_secs(timeout),
                std::time::Duration::from_millis(poll),
            )?;
            if cli.json {
                let items: Vec<_> = found.iter().map(messages::Message::to_json).collect();
                print_json(&serde_json::json!({ "count": items.len(), "messages": items }));
            } else if found.is_empty() {
                println!("no mail within {timeout}s");
            } else {
                // Through `render_inbox`, never a bare `println!` of sender-written fields. A
                // raw loop here was the fourth renderer of `sender`/`subject` — the exact hole
                // D90 closed in `render_inbox`, standing in this file because the enumeration
                // test can only redden for renderers it lists (its docstring says so). Routing
                // through the guarded renderer puts watch inside that enumeration instead of
                // beside it, and delivers the body, which the bare loop never did.
                print!("{}", delivery::render_inbox(&found, &me.name, &me.project));
            }
        }

        Command::Install { .. } | Command::Uninstall { .. } | Command::Hook { .. } => {
            unreachable!("handled before the board is opened")
        }

        Command::Memory { ref what } => run_memory(&cli, &conn, &me, what)?,

        Command::Snapshot { ref path, all } => {
            let me = identity::resolve()?;
            identity::touch(&conn, &me, None)?;
            // `inbox` is a plain SELECT. Nothing below marks anything delivered or read.
            let msgs = messages::inbox(&conn, &me, !all)?;
            let names: Vec<String> = identity::list(&conn, None)?
                .iter()
                .map(|r| {
                    format!(
                        "{} [{}] · {}",
                        r.name,
                        identity::short_ref(&r.id),
                        r.project
                    )
                })
                .collect();
            let text = delivery::snapshot(&msgs, &names, &me.name, db::now()?, !all);
            let home = std::env::var("HOME").ok();
            delivery::write_snapshot(std::path::Path::new(path), &text, home.as_deref())?;
            // Bumped after the write, so a refused path does not count as an experiment that ran.
            db::bump(&conn, delivery::COUNTER_SNAPSHOT, db::now()?);
            let runs = db::counter(&conn, delivery::COUNTER_SNAPSHOT);
            if cli.json {
                print_json(&serde_json::json!({
                    "path": path, "messages": msgs.len(), "bytes": text.len(), "runs": runs,
                }));
            } else {
                println!("wrote {} message(s) to {path} (render #{runs})", msgs.len());
            }
        }
        Command::Doctor => {
            // Every judgement lives in `amb::doctor`; this prints. D70's audit found four
            // functions in this file making real decisions, which is the invariant that keeps
            // them testable without a process.
            let report = doctor::gather(db::now()?);
            if cli.json {
                print_json(&report.to_json());
            } else {
                for c in &report.checks {
                    println!("{}  {:<14}  {}", c.health.glyph(), c.name, c.detail);
                }
            }
        }
        Command::Agents { ref project, live } => {
            let rows = identity::list(&conn, project.as_deref())?;
            let at = db::now()?;
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|r| !live || r.appears_alive(at))
                .collect();
            // Reported unfiltered, deliberately: a collision is a property of the machine, and
            // `--project` or `--live` narrowing the roster must not narrow away the warning that
            // the name being narrowed on is ambiguous.
            let clashes = identity::collisions(&conn)?;
            if cli.json {
                let items: Vec<_> = rows.iter().map(|r| r.to_json(at)).collect();
                let clash_json: Vec<_> = clashes
                    .iter()
                    .map(|c| serde_json::json!({ "project": c.project, "roots": c.roots }))
                    .collect();
                print_json(&serde_json::json!({
                    "count": items.len(),
                    "agents": items,
                    "collisions": clash_json,
                }));
            } else {
                if rows.is_empty() {
                    println!("no agents registered");
                } else {
                    for r in &rows {
                        let state = if r.appears_alive(at) { "alive" } else { "gone" };
                        println!(
                            "{} [{}] · {} · {}",
                            r.name,
                            identity::short_ref(&r.id),
                            r.project,
                            state
                        );
                    }
                }
                for c in &clashes {
                    println!(
                        "\n! {} repositories both call themselves {:?}:",
                        c.roots.len(),
                        c.project
                    );
                    for root in &c.roots {
                        println!("    {root}");
                    }
                    println!(
                        "  They share a `@{}` broadcast address and a vault namespace. Set \
                         AMB_PROJECT in one repository's .claude/settings.json to separate them.",
                        c.project
                    );
                }
            }
        }
    }
    Ok(())
}

/// Apply or preview a hook plan, and say plainly what changed.
fn report_plan(
    cli: &Cli,
    path: &std::path::Path,
    done: &hooks::Applied,
    dry_run: bool,
    label: &str,
) -> Result<(), Error> {
    let plan = &done.plan;
    // **Said, never swallowed** (D99). An unlocked write still happens — a filesystem without
    // working advisory locks should not lose its install — but the one thing it must not do is
    // report the same success as a locked one. This edits the file that configures Claude Code
    // for every project on the machine.
    if cli.json {
        print_json(&serde_json::json!({
            "settings": path.display().to_string(),
            "mode": label,
            "added": plan.added,
            "removed": plan.removed,
            "changed": !plan.is_noop(),
            "dry_run": dry_run,
            "locked": done.locked,
            "lock_error": done.lock_error,
            "retries": done.retries,
        }));
        return Ok(());
    }
    if let Some(why) = &done.lock_error {
        println!(
            "! could not lock {} ({why}) — the change was still written and still verified \
             unchanged before replacing the file, but two amb processes could interleave",
            path.display()
        );
    }
    if done.retries > 0 {
        // Not a warning. It is the mechanism working, and staying silent about it would make a
        // contended settings file indistinguishable from a quiet one.
        println!(
            "  another process wrote {} first; re-read and re-applied ({} time(s))",
            path.display(),
            done.retries
        );
    }
    if plan.is_noop() {
        println!("no change needed in {}", path.display());
    } else {
        let verb = if dry_run { "would update" } else { "updated" };
        println!("{verb} {}", path.display());
        for e in &plan.added {
            println!("  + {e} hook ({label})");
        }
        for e in &plan.removed {
            println!("  - {e} hook");
        }
    }
    Ok(())
}

/// The hook entry point. Returns success in every circumstance, including failure.
///
/// D9 requires that mail delivery never break a session. That makes silence the correct
/// response to *any* problem here — a missing board, an unreadable database, a clock error.
/// Set `AMB_HOOK_DEBUG=1` to see why nothing was emitted.
fn hook_main(mode: &str) -> ExitCode {
    let mut raw = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw);
    let input: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);

    // A subagent is not a participant on the board: it has no independent inbox and would
    // register as a phantom peer. Borrowed from an existing hook on this machine that does the
    // same check for the same reason.
    if input.get("agent_id").is_some() {
        return ExitCode::SUCCESS;
    }

    // **A Stop re-fire gets silence, whatever this hook has to say.** The runner counts a Stop
    // hook that injects `additionalContext` as blocking the turn from ending: it wakes the model
    // to read the context, the model answers, Stop fires again — and `stop_hook_active: true` is
    // the runner saying this firing IS that wake. Answering it again is a loop. It happened, at
    // machine scale: during a stale-binary window the arrival note printed on every Stop, so
    // every session on the machine cycled banner → "Standing by." → banner, nine times each,
    // until the platform's block cap overrode — in five projects at once, twice (2026-08-27 and
    // 2026-08-31, both read out of the transcripts). Mail is unaffected: delivery is a log
    // (D17), so anything not said on this firing is re-offered on the next real event.
    if input.get("stop_hook_active").and_then(|v| v.as_bool()) == Some(true) {
        return ExitCode::SUCCESS;
    }

    // Memory is a separate entry in settings.json with its own timeout, so it is a separate
    // branch here: a memory layer that hangs must burn its own budget and take nothing with it.
    // D9's guarantee is structural, not a discipline (D41).
    let result = if mode == hooks::MEMORY_ARG {
        let r = hook_memory(&input);
        // Consecutive, so one blip does not accumulate toward the threshold and a real outage
        // does.
        match &r {
            Ok(()) => amb::memory::note_success(),
            Err(_) => {
                amb::memory::note_failure();
            }
        }
        r
    } else {
        hook_deliver(mode, &input)
    };
    // **Exit 0 and say nothing were one rule, and they are two.** D9 requires that a hook never
    // break a session; it does not require silence, and the success path a few lines down already
    // writes to stdout and exits 0. Conflating them made the one error that names a live,
    // machine-wide fault unreachable by the person it concerns (D58).
    //
    // Only the delivery hook speaks. A stale binary fails the memory hook identically, and both
    // are installed together, so reporting from both would say the same thing twice for one fault.
    if let Err(Error::SchemaVersion {
        ref path,
        found,
        expected,
    }) = result
        && mode != hooks::MEMORY_ARG
    {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "amb".to_string());
        let notice =
            delivery::stale_binary_notice(path, &exe, amb::version::banner(), found, expected);
        let event = input
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .unwrap_or("SessionStart");
        println!("{}", delivery::envelope(event, &notice));
    }
    if let Err(e) = result
        && std::env::var("AMB_HOOK_DEBUG").is_ok()
    {
        eprintln!("amb hook: {e}");
    }
    ExitCode::SUCCESS
}

/// Inject what past sessions recorded — at session start, and before a file is opened.
///
/// **Three no-ops before anything happens, in widening order of cost:** no board, no vault, and
/// for `PreToolUse` no file. The first is D9's rule (a globally installed hook must not create
/// state for a session that never uses the board); the second is this layer's kill switch, which
/// is the same rule one level in — memory off means silent, not absent.
///
/// Never returns an error to the caller that matters: [`hook_main`] discards it and exits 0.
fn hook_memory(input: &serde_json::Value) -> Result<(), Error> {
    let path = db::db_path()?;
    if !path.exists() {
        return Ok(());
    }
    let Some(vault) = memory::vault_path() else {
        return Ok(());
    };
    let event = input
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("SessionStart");
    let me = identity::resolve()?;
    let conn = db::open_at_for_hook(&path)?;
    let at = db::now()?;

    // Deliberately no `identity::touch` here. Memory is not a participant on the board, and this
    // runs before every file tool call — a roster write per `Read` is amplification for nothing.
    // A failure is disproportionately what is worth remembering, and capturing it needs none of
    // 4a's blocking machinery — it is a hook that reads a payload and writes a file. This is the
    // "worth noting for later" the plan flagged, taken now because it is the cheap half.
    if event == "PostToolUseFailure" {
        return capture_failure(&conn, &me, input, at);
    }

    // The source travels with the injection, because it decides which ledger the notes land in
    // and therefore which number the receipt divides. See `memory::Source`.
    let (injection, source) = if event == "PreToolUse" {
        (
            memory_for_file(&conn, &me, input, at)?,
            memory::Source::File,
        )
    } else {
        (
            Some(memory_for_session(&conn, &me, &vault, at)?),
            memory::Source::Session,
        )
    };

    let Some(injection) = injection else {
        return Ok(());
    };
    // The ledger is written by the read path, before the text is emitted. That ordering is the
    // point: recording an injection *is* how injection happens, so it cannot drift into a
    // decorative counter nobody increments (D39).
    memory::record_injected(&conn, &me.id, &injection.shown, at, source)?;
    let mut text = injection.text;
    // Fail-loud, once the count crosses the threshold, through the same channel as everything
    // else. D9's silence is right for delivery; as an unlimited policy for *capture* it is how
    // you come to believe you are recording for months while recording nothing.
    if let Some(notice) = memory::fail_loud_notice(memory::failure_count()) {
        text.push_str("\n\n");
        text.push_str(&notice);
    }
    println!("{}", delivery::envelope(event, &text));
    Ok(())
}

/// Record a tool failure as an observation. No model, no blocking, no transcript.
///
/// **Deliberately not 4a.** The plan's self-compression blocks a turn to ask a session to
/// summarise itself, which puts unmeasured LLM-adjacent work inside D9's guarantee. This is the
/// part of 4b needing none of it: the payload already names the tool and the error.
fn capture_failure(
    conn: &rusqlite::Connection,
    me: &identity::Identity,
    input: &serde_json::Value,
    at: f64,
) -> Result<(), Error> {
    let (tool, file) = hooks::tool_and_file(input);
    if memory::should_skip(tool, &memory::skip_tools()) {
        return Ok(());
    }
    let file = file.and_then(|f| claims::relative_to(&me.root, f));
    let detail = input
        .get("error")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("tool_response").and_then(|v| v.as_str()))
        .unwrap_or("no detail in the payload");
    // Title and cap are `memory::failure_note`'s decision, not this function's.
    let (title, detail) = memory::failure_note(tool, detail);

    memory::observe(
        conn,
        me,
        &memory::Observation {
            // D86. No mind decided this was worth recording — a hook fired on a non-zero exit —
            // so it is searchable and never injected.
            kind: memory::CAPTURE,
            title: &title,
            learned: &detail,
            project: &me.project,
            files: &file.map(|f| vec![f]).unwrap_or_default(),
            cites: &[],
            supersedes: None,
            // A captured failure is an observation, never a rule: nothing a session did badly
            // becomes binding on the next one without a person saying so.
            force: memory::ADVICE,
        },
        at,
    )?;
    Ok(())
}

/// The `SessionStart` block, after making sure the index still matches the vault.
///
/// **The re-index is why deleting `board.db` is survivable rather than silently disabling.**
/// `DESIGN.md` and the board's own README both call the database disposable, so a user will
/// eventually delete it; without this, memory would stop working and say nothing.
fn memory_for_session(
    conn: &rusqlite::Connection,
    me: &identity::Identity,
    vault: &std::path::Path,
    at: f64,
) -> Result<memory::Injection, Error> {
    let stats = match memory::sync_dir(
        conn,
        vault,
        memory::OBSERVATION,
        &me.project,
        at,
        Some(memory::AUTO_INDEX_LIMIT),
    ) {
        Ok(s) => s,
        // A vault that is configured but unreadable is an outage, and saying so is the whole
        // difference between this and claude-mem's three silent months.
        Err(e) => return Ok(memory::render_unavailable(vault, &e.to_string())),
    };
    // D45's guard, decided by `memory::index_is_behind` rather than here — it was the shape of
    // claude-mem's `relevance_count`, a field nobody read, and it belongs where it can be tested.
    let behind = memory::index_is_behind(&stats, memory::count_indexed(conn, &me.project)?);
    // Three numbers, because the renderer cannot derive the other two from the first: what to
    // show, how many exist for this project (so the cap can admit what it hid), and how many
    // exist at all.
    // What this repository *is*, from files at its root — a dozen `stat` calls, and the reason a
    // Rust principle reaches a Rust repository without anyone declaring anything (D82).
    let topics = memory::detect(std::path::Path::new(&me.root));
    let notes = memory::recent_for_project(conn, &me.project, &topics, memory::MAX_INJECTED)?;
    // **The same topics feed the count, or the cap admission lies.** D54's defect exactly: when
    // injection grew to include a scope the count did not, the header rendered "2 of 1 note(s)".
    let in_project = memory::count_active(conn, Some(&me.project), &topics)?;
    let in_vault = memory::count_active(conn, None, &topics)?;
    Ok(memory::render_session(
        &notes,
        &me.project,
        in_project,
        in_vault,
        behind,
        at,
    ))
}

/// The `PreToolUse` block: what is known about the file that is about to be opened.
///
/// **The strictest form of scoping an injection to its consumer.** At `SessionStart` the relevant
/// file is a guess; here the agent has just named it.
fn memory_for_file(
    conn: &rusqlite::Connection,
    me: &identity::Identity,
    input: &serde_json::Value,
    at: f64,
) -> Result<Option<memory::Injection>, Error> {
    let (tool, file) = hooks::tool_and_file(input);
    if memory::should_skip(tool, &memory::skip_tools()) {
        return Ok(None);
    }
    let Some(file) = file else {
        return Ok(None);
    };
    // Outside this repository is outside this lookup: `note_paths` holds repo-relative globs, so
    // an absolute path from another tree would match nothing and cost a query to find out.
    let Some(rel) = claims::relative_to(&me.root, file) else {
        return Ok(None);
    };
    let (notes, total) = memory::concerning(conn, &rel)?;
    Ok(memory::render_file(&notes, &me.project, &rel, total, at))
}

fn hook_deliver(mode: &str, input: &serde_json::Value) -> Result<(), Error> {
    // The no-op fast path. A globally installed hook runs in sessions that never touch the
    // board, and it must not create a database for them.
    let path = db::db_path()?;
    if !path.exists() {
        return Ok(());
    }

    let event = input
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("SessionStart");
    let me = identity::resolve()?;
    let mut conn = db::open_at_for_hook(&path)?;
    identity::touch(&conn, &me, None)?;

    // An edit that already happened: record the claim, and — since PostToolUse output *is*
    // injected into the model's context — say so now rather than at the next turn boundary.
    if event == "PostToolUse" {
        return post_tool_use(&mut conn, &me, input);
    }

    // deliverable(), not inbox(): automatic injection spends context the agent did not ask to
    // spend, so it backs off after MAX_OFFERS. An explicit `amb inbox` still shows everything.
    let unread = messages::deliverable(&conn, &me)?;
    let conflicts = claims::my_conflicts(&conn, &me)?;
    let is_start = event == "SessionStart";
    let Some(mut rendered) = delivery::render_all(&unread, &conflicts, db::now()?, is_start) else {
        return Ok(());
    };
    if is_start && mode == "monitor" {
        rendered.text.push_str(
            "\n\nFor immediate delivery, run `amb watch --timeout 300 --json` under your \
             Monitor tool; it blocks until mail arrives.",
        );
    }

    emit(&mut conn, &me, event, &rendered)
}

/// Inject a block of context, recording that the mail in it was offered.
///
/// The two delivery paths differ in *what* they select and *how* they render it; they agree
/// exactly here, and this is where D9's "record the offer, never a read" lives on the hook side.
/// Keeping it in one place is the point — when it was written out at both call sites, the two
/// copies had to be kept in step by hand.
fn emit(
    conn: &mut rusqlite::Connection,
    me: &identity::Identity,
    event: &str,
    rendered: &delivery::Rendered,
) -> Result<(), Error> {
    messages::mark_delivered_all(conn, me, &rendered.shown)?;
    // The same discipline for conflicts, against the set that was rendered rather than the set
    // that was selected (D33, D44). Without this the Stop sweep re-injects an identical warning
    // at every turn boundary until a four-hour lease runs out, for a session that has ended.
    claims::record_notices(conn, me, &rendered.conflicts_shown)?;
    println!("{}", delivery::envelope(event, &rendered.text));
    Ok(())
}

/// Handle a `PostToolUse` hook: record the edit, then deliver what is genuinely new.
///
/// **Mid-turn delivery, and it is the cheapest large improvement available here.** `SessionStart`
/// and `Stop` deliver at a session's start and at turn boundaries; an agent grinding through a
/// forty-minute autonomous turn received nothing in between. This hook fires after every tool
/// call, so it closes that gap with no daemon and no polling — using a hook `amb` already
/// installs and whose output it used to discard.
///
/// It was discarded on the belief that `PostToolUse` output is not injected into a model's
/// context. **Verified 2026-08-27, first-hand, on this machine:** a probe hook emitting
/// `hookSpecificOutput.additionalContext` on `PostToolUse` had its exact text appear in the
/// reading session's context. That is the same class of correction `MEASUREMENTS.md` M4 records —
/// a documentation summary standing in for an observation — so it was tested rather than trusted
/// a second time (D25).
///
/// **What keeps it from becoming noise** is that both halves are restricted to *new* facts:
/// mail that has never been offered, and a conflict only on an edit that took a claim rather
/// than renewing one. Re-editing the same contested file is silent; `Stop` remains the sweep.
fn post_tool_use(
    conn: &mut rusqlite::Connection,
    me: &identity::Identity,
    input: &serde_json::Value,
) -> Result<(), Error> {
    let conflicts = observe_edit(conn, me, input)?;
    let unread = messages::undelivered(conn, me)?;
    let Some(rendered) = delivery::render_all(&unread, &conflicts, db::now()?, false) else {
        return Ok(());
    };
    emit(conn, me, "PostToolUse", &rendered)
}

/// Record a claim from an edit the agent just performed, and report what it collided with.
///
/// The rule for *whether* this is a claim, and under what path, lives in
/// [`amb::claims::edited_path`]; all this does is perform the write.
///
/// Returns conflicts **only when the claim was newly taken**. Renewing a claim on a file this
/// agent has already been warned about says nothing new, and repeating it after every edit is
/// how an advisory system trains agents to ignore it (D19).
fn observe_edit(
    conn: &rusqlite::Connection,
    me: &identity::Identity,
    input: &serde_json::Value,
) -> Result<Vec<claims::Claim>, Error> {
    let (tool, file) = hooks::tool_and_file(input);
    let Some(rel) = claims::edited_path(&me.root, tool, file) else {
        return Ok(Vec::new());
    };
    let taken = claims::take(conn, me, &rel, None, None, claims::Source::Observed)?;
    // D19's rule is `claims::conflicts_to_report`, not an `if` here.
    Ok(claims::conflicts_to_report(&taken))
}

/// Resolve a message body from `--body`, or from a file or stdin via `--body-file`.
///
/// `clap` already enforces that one of the two is present; the final arm is the honest way to
/// say so without an `unwrap` in a binary other agents invoke from hooks.
fn read_body(body: Option<&str>, body_file: Option<&str>) -> Result<String, Error> {
    match (body, body_file) {
        (Some(b), _) => Ok(b.to_string()),
        (None, Some("-")) => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|source| {
                Error::Io {
                    context: "reading the body from stdin".into(),
                    source,
                }
            })?;
            Ok(s)
        }
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|source| Error::Io {
            context: format!("reading the body from {path}"),
            source,
        }),
        (None, None) => Err(Error::MissingBody),
    }
}

/// Print a JSON value, falling back to a valid JSON error object rather than panicking.
///
/// A hook feeds this straight into a model's context, so emitting malformed JSON is worse than
/// emitting an error — the reader cannot tell the difference between a bug and an empty inbox.
fn print_json(v: &serde_json::Value) {
    match serde_json::to_string(v) {
        Ok(s) => println!("{s}"),
        Err(e) => println!(r#"{{"error":"could not serialise output: {e}"}}"#),
    }
}

/// The `amb memory` arm.
///
/// Split out rather than inlined into [`run`]'s match because it is a subcommand tree of its own,
/// and because the kill switch reads better as one function that either has a vault or does not.
/// Like the rest of this file it holds no logic — every decision is in `amb::memory`.
fn run_memory(
    cli: &Cli,
    conn: &rusqlite::Connection,
    me: &identity::Identity,
    what: &MemoryCommand,
) -> Result<(), Error> {
    let at = db::now()?;
    match what {
        MemoryCommand::Observe {
            title,
            learned,
            files,
            cites,
            supersedes,
            same_as,
            project,
            force,
        } => {
            let project = project.as_deref().unwrap_or(&me.project);
            let derived = match same_as {
                Some(slug) => Some(memory::derive(conn, me, slug, title, files, learned, at)?),
                None => None,
            };
            let written = memory::observe(
                conn,
                me,
                &memory::Observation {
                    kind: memory::OBSERVATION,
                    title,
                    learned,
                    project,
                    files,
                    cites,
                    supersedes: supersedes.as_deref(),
                    force,
                },
                at,
            )?;
            if cli.json {
                print_json(&written.to_json());
            } else {
                // Queried only on the path that shows them: `near_candidates` records an
                // injection, and the JSON surface never displays these, so bumping the ledger
                // there would charge the layer for something nobody was shown.
                let near = memory::near_candidates(conn, me, files, at)?;
                print!(
                    "{}",
                    memory::render_written(&written, derived.as_ref(), &near)
                );
            }
        }

        MemoryCommand::Recall {
            query,
            file,
            across_repos,
            project,
            all_projects,
            limit,
        } => {
            let lane = match (file, across_repos) {
                (Some(_), true) => memory::LANE_ACROSS,
                (Some(_), false) => memory::LANE_PATH,
                (None, _) => memory::LANE_TEXT,
            };
            let notes = match file {
                // A path lookup deliberately ignores --project: "who touched this file, in any
                // repo on this machine" is the one question no per-repo tool can answer.
                // The total is discarded here: a human at a terminal is not being injected
                // into, so there is nothing to cap and nothing to admit hiding.
                Some(f) if *across_repos => {
                    // Counted, because "is the differentiator ever used?" is the plan's receipt
                    // for 4b and a feature nobody runs looks identical to one quietly working.
                    db::bump(conn, memory::COUNTER_CROSS_REPO, at);
                    memory::across_repos(conn, f, &me.project)?
                }
                Some(f) => memory::concerning(conn, f)?.0,
                None => {
                    let scope = if *all_projects {
                        None
                    } else {
                        Some(project.as_deref().unwrap_or(&me.project))
                    };
                    memory::search(conn, query.as_deref(), scope, *limit as usize)?
                }
            };
            // Recorded whatever the answer was, and *before* it is rendered: a search that
            // found nothing is the reading D89 exists to make possible, so it is the one that
            // must not be skipped.
            memory::record_search(conn, &me.id, lane, &notes, &me.project, at)?;
            if cli.json {
                let items: Vec<_> = notes.iter().map(|n| n.to_json(at)).collect();
                print_json(&serde_json::json!({ "count": items.len(), "notes": items }));
            } else {
                print!("{}", memory::render_recall(&notes, at));
            }
        }

        MemoryCommand::Derive {
            slug,
            title,
            note,
            files,
        } => {
            if !memory::promotion_enabled() {
                return Err(Error::BadAddress {
                    input: "AMB_MEMORY_PROMOTION".into(),
                    reason: "the promotion pipeline is switched off".into(),
                });
            }
            let d = memory::derive(conn, me, slug, title, files, note, at)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "id": d.id.display(),
                    "created": d.created,
                    "independent": d.independent,
                    "derived_count": d.count,
                    "derived_in": d.projects,
                    "ready": d.count >= memory::PROMOTION_THRESHOLD,
                    "path": d.path.display().to_string(),
                }));
            } else {
                print!("{}", memory::render_derived(&d));
            }
        }

        MemoryCommand::Candidates { ready } => {
            let vault = memory::require_vault()?;
            let notes = memory::list_candidates(conn, &vault, at, *ready)?;
            if cli.json {
                let items: Vec<_> = notes
                    .iter()
                    .map(|n| {
                        serde_json::json!({
                            "id": n.id.display(),
                            "title": n.title,
                            "status": n.status,
                            "derived_count": n.derivations.len(),
                            "derived_in": memory::projects_of(n),
                            "ready": n.derivations.len() >= memory::PROMOTION_THRESHOLD,
                            "declined": n.declined_at.is_some(),
                        })
                    })
                    .collect();
                print_json(&serde_json::json!({ "count": items.len(), "candidates": items }));
            } else {
                print!("{}", memory::render_candidates(&notes));
            }
        }

        MemoryCommand::Promote {
            id,
            decline,
            yes,
            direct,
            scope,
        } => {
            let vault = memory::require_vault()?;
            let note_id = memory::parse_id(id).ok_or_else(|| Error::NoSuchNote(id.clone()))?;
            if *decline {
                memory::decline(conn, &note_id, at)?;
                if cli.json {
                    print_json(&serde_json::json!({ "declined": note_id.display() }));
                } else {
                    println!(
                        "declined {} — not offered again until it derives",
                        note_id.display()
                    );
                }
                return Ok(());
            }
            if *direct {
                if !*yes {
                    if cli.json {
                        print_json(&serde_json::json!({
                            "written": false,
                            "confirm": "--direct --yes",
                        }));
                    } else {
                        println!(
                            "direct promotion skips the derivation ledger entirely, so there is \
                             nothing to read.\n  confirm with --direct --yes"
                        );
                    }
                    return Ok(());
                }
                let promoted = memory::promote_direct(conn, me, &note_id, at)?;
                if cli.json {
                    print_json(&serde_json::json!({ "promoted": promoted.id.display() }));
                } else {
                    println!("promoted {} directly", promoted.id.display());
                    println!("  no derivation ledger — the file records that it was direct");
                }
                return Ok(());
            }
            let candidate = memory::load(&vault, &note_id, at)?;
            if !*yes {
                // **The offer, and it deliberately costs something to accept.** One candidate,
                // its derivations spelled out rather than counted, and no write until --yes.
                // A batch with a single confirmation is a rubber stamp, and a rubber stamp is
                // D16's defect with extra steps (D49).
                //
                // `--json` too: the primer promises it on any command, and this gate was one of
                // three arms that broke that promise — an agent parsing stdout got prose on
                // exactly the human-gate paths. `written: false` is the load-bearing field; the
                // gate survives the format.
                let routed = memory::destination(&candidate);
                if cli.json {
                    print_json(&serde_json::json!({
                        "written": false,
                        "confirm": "--yes",
                        "offer": memory::offer_json(&candidate, &routed),
                    }));
                } else {
                    print!("{}", memory::render_offer(&candidate, &routed));
                }
                return Ok(());
            }
            let chosen = scope
                .as_deref()
                .map(amb::address::parse_scope)
                .transpose()?;
            let promoted = memory::promote(conn, me, &note_id, chosen, at)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "promoted": promoted.id.display(),
                    "from": note_id.display(),
                }));
            } else {
                println!("promoted {} → {}", note_id.display(), promoted.id.display());
                println!("  the candidate is archived, not deleted — it holds the evidence");
            }
        }

        MemoryCommand::Export {
            project,
            repo,
            check,
        } => {
            let vault = memory::require_vault()?;
            let project = project.as_deref().unwrap_or(&me.project);
            let repo = repo.clone().unwrap_or_else(|| me.root.clone());
            let repo = std::path::Path::new(&repo);
            let exports = memory::plan_export(conn, &vault, project, at)?;
            if *check {
                let st = memory::check_export(&exports, repo);
                // Both counted: the plan asks whether `--check` ever *fires*, and answering that
                // needs to know how often it *ran* — never firing over a thousand runs and never
                // firing because it was never run are opposite conclusions.
                db::bump(conn, memory::COUNTER_EXPORT_CHECK, at);
                if st.drifted() {
                    db::bump(conn, memory::COUNTER_EXPORT_STALE, at);
                }
                if cli.json {
                    print_json(&serde_json::json!({
                        "drifted": st.drifted(),
                        "current": st.current,
                        "stale": st.stale,
                        "missing": st.missing,
                    }));
                } else {
                    print!("{}", memory::render_export_check(&st, repo, project));
                }
                if st.drifted() {
                    // Non-zero so CI or a pre-commit hook fails on drift rather than reporting it
                    // into a log nobody reads.
                    return Err(Error::ExportStale {
                        count: st.stale.len() + st.missing.len(),
                        repo: repo.display().to_string(),
                    });
                }
                return Ok(());
            }
            let n = memory::write_export(&exports, repo)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "written": n,
                    "repo": repo.display().to_string(),
                    "files": exports.iter().map(|e| e.rel_path.clone()).collect::<Vec<_>>(),
                }));
            } else {
                println!("wrote {n} decision(s) into {}", repo.display());
                for e in &exports {
                    println!("  {}", e.rel_path);
                }
            }
        }

        MemoryCommand::Capture {
            transcript,
            summary,
        } => {
            let path = transcript
                .clone()
                .or_else(|| std::env::var("CLAUDE_CODE_TRANSCRIPT_PATH").ok())
                .ok_or_else(|| Error::BadAddress {
                    input: "--transcript".into(),
                    reason: "no transcript path given and CLAUDE_CODE_TRANSCRIPT_PATH is unset"
                        .into(),
                })?;
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let facts = memory::parse_transcript(&text, &me.root);
            if !facts.worth_capturing(summary.as_deref()) {
                // Nothing worth a note. Said out loud rather than writing an empty one, because a
                // vault of contentless notes is how the injection cap starts hiding real ones.
                if cli.json {
                    print_json(&serde_json::json!({ "captured": false, "reason": "no facts" }));
                } else {
                    println!("nothing to capture from {path}");
                }
                return Ok(());
            }
            let written = memory::capture_session(conn, me, &facts, summary.as_deref(), at)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "captured": true,
                    "id": written.id.display(),
                    "files": facts.files,
                    "failures": facts.failures,
                    "tools": facts.tools,
                }));
            } else {
                println!(
                    "captured {} → {}",
                    written.id.display(),
                    written.path.display()
                );
                println!(
                    "  {} tool call(s), {} file(s), {} failure(s)",
                    facts.tools,
                    facts.files.len(),
                    facts.failures.len()
                );
            }
        }

        MemoryCommand::Expire => {
            let vault = memory::require_vault()?;
            let n = memory::expire_candidates(conn, &vault, at)?;
            if cli.json {
                print_json(&serde_json::json!({ "expired": n }));
            } else {
                println!("{n} candidate(s) expired");
            }
        }

        MemoryCommand::History { id } => {
            memory::require_vault()?;
            let note = memory::parse_id(id).ok_or_else(|| Error::NoSuchNote(id.clone()))?;
            let (before, after) = memory::history(conn, &note)?;
            if cli.json {
                let step = |s: &memory::Step| serde_json::json!({"id": s.id, "title": s.title, "status": s.status});
                print_json(&serde_json::json!({
                    "id": note.display(),
                    "replaced": before.iter().map(step).collect::<Vec<_>>(),
                    "replaced_by": after.iter().map(step).collect::<Vec<_>>(),
                }));
            } else {
                print!("{}", memory::render_history(&note, &before, &after));
            }
        }
        MemoryCommand::Index => {
            let vault = memory::require_vault()?;
            let stats = memory::reindex(conn, &vault, at)?;
            if cli.json {
                print_json(&serde_json::json!({
                    "vault": vault.display().to_string(),
                    "scanned": stats.scanned,
                    "indexed": stats.indexed,
                    "unchanged": stats.unchanged,
                    "unreadable": stats.unreadable,
                    "pruned": stats.pruned,
                    "link_problems": memory::validate_links(conn)?
                        .iter()
                        .map(|p| serde_json::json!({
                            "note": p.note, "kind": p.kind, "detail": p.detail
                        }))
                        .collect::<Vec<_>>(),
                    "unknown_keys": memory::unknown_keys(&vault)
                        .iter()
                        .map(|u| serde_json::json!({ "note": u.note, "key": u.key }))
                        .collect::<Vec<_>>(),
                }));
            } else {
                print!(
                    "{}",
                    memory::render_index(
                        &stats,
                        &memory::validate_links(conn)?,
                        &memory::unknown_keys(&vault)
                    )
                );
            }
        }

        MemoryCommand::Coverage { project } => {
            let c = memory::coverage(conn, project.as_deref().unwrap_or(&me.project))?;
            if cli.json {
                print_json(&c.to_json());
            } else {
                print!("{}", memory::render_coverage(&c));
            }
        }

        MemoryCommand::Status { days, all_time } => {
            // **The open window is the default, and that is the whole point of D87.** `--days`
            // slides with `now` and can only express whole days, so it could never name the
            // instant D79 defined; leaving it as the only control meant the printed ratio was
            // computed over events the decision had excluded, including a hand-run probe.
            // The precedence is `memory::counting_window`'s, not this arm's — D78's rule, applied
            // when the decision was written rather than after it had drifted here untested.
            let open = memory::window_start(conn, memory::INJECTION_WINDOW)?;
            let (since, corpus) = memory::counting_window(*days, *all_time, open, at);
            let st = memory::status(conn, since)?;
            // Read, never assumed. An unreadable or absent settings file is `Unknown`, never
            // `Incomplete` — see `hooks::HookState`. Every decision about what this means lives in
            // the library; this arm reads a file and prints what it is given.
            let hook_state = match hooks::settings_path().and_then(|p| hooks::read_settings(&p)) {
                Ok(settings) => hooks::memory_state(&settings),
                Err(_) => hooks::HookState::Unknown,
            };
            if cli.json {
                print_json(&st.to_json(&hook_state));
                return Ok(());
            }
            // **Every line of this used to be here** — 190 of them, in the one file with no
            // tests, printing the instrument D59 retires a feature on (D92).
            println!(
                "{}",
                memory::render_status(&st, &corpus, &hook_state, memory::failure_count())
            );
        }

        MemoryCommand::Window { open, reopen } => {
            // **Reporting is the default and opening is the flag**, because a window is opened
            // once and read many times, and the destructive spelling should never be the one you
            // get by typing the noun.
            if !open && !reopen {
                // Read only on the path that uses it — `window_open` does its own lookup, so
                // fetching here unconditionally queried the row twice and discarded one.
                let since = memory::window_start(conn, memory::INJECTION_WINDOW)?;
                if cli.json {
                    print_json(&serde_json::json!({ "open": since.is_some(), "since": since }));
                } else {
                    print!("{}", memory::render_window_report(since, at));
                }
                return Ok(());
            }
            let change = memory::window_open(conn, memory::INJECTION_WINDOW, at, *reopen)?;
            if cli.json {
                print_json(&change.to_json());
            } else {
                print!("{}", memory::render_window_change(&change, at));
            }
        }
    }
    Ok(())
}

//! The instruments: capture health, unknown keys, and coverage.
//!
//! Read-only. Everything here answers "is this layer actually working", which
//! is the question D58 says a mechanism must be able to answer about itself.

use super::*;

// ── Status ──────────────────────────────────────────────────────────────────

/// Answers *is this actually capturing?* without reading a log.
///
/// Exists because the incumbent's own corpus is a demonstration of what its absence costs: 85
/// queue items and 43 sessions left in a non-terminal state on 2026-05-12, after which the system
/// ran three more months and added 80,000 observations without ever surfacing them.
#[derive(Debug, Clone)]
pub struct Status {
    pub vault: Option<PathBuf>,
    pub on_disk: usize,
    /// Files on disk that will not parse — a note whose content is gone (D62).
    pub unreadable: usize,
    pub indexed: usize,
    pub active: usize,
    pub superseded: usize,
    pub projects: Vec<(String, usize)>,
    pub receipt: Receipt,
    pub phases: PhaseReceipts,
    /// How often recall was reached for over the same window, and what it returned (D89).
    pub searches: Searches,
    /// When the window this was counted over opened, if one is open (D87).
    ///
    /// **Carried here so `--json` names it too.** The human path printed `counting over …` while
    /// `--json` returned before that string was built, so the surface most likely to be read by a
    /// machine emitted a ratio with no window attached — the exact defect D87 exists to remove,
    /// on the other half of the same command.
    pub window: Option<f64>,
}

impl Status {
    /// True when the vault holds notes the index has never seen. Visible drift beats a silent
    /// half-working index — `amb memory index` is the fix and this is what says to run it.
    /// **An unreadable note is drift even when the counts agree**, which is how the original
    /// version missed it: a zero-byte file is still one `.md` on disk and still one row in the
    /// index, so `on_disk == indexed` held while the note itself was gone.
    pub fn drifted(&self) -> bool {
        self.on_disk != self.indexed || self.unreadable > 0
    }

    /// **Takes the hook state, so a receipt cannot be serialised without saying whether the layer
    /// ran.** Exactly the argument `verdict` makes, applied to the surface that reaches a machine:
    /// `--json` is what an agent is told to use, and the first version of this emitted counts with
    /// no state and no verdict, leaving a consumer free to compute its own ratio and reach D59's
    /// conclusion unaided. Assembling it here rather than in `src/main.rs` is what makes that
    /// checkable — the keys were merged in the binary once, and nothing could go red when they
    /// were not.
    pub fn to_json(&self, hooks: &crate::hooks::HookState) -> serde_json::Value {
        let mut doc = serde_json::json!({
            "vault": self.vault.as_ref().map(|v| v.display().to_string()),
            "enabled": self.vault.is_some(),
            "on_disk": self.on_disk,
            "unreadable": self.unreadable,
            "indexed": self.indexed,
            "drifted": self.drifted(),
            "active": self.active,
            "superseded": self.superseded,
            "projects": self.projects.iter()
                .map(|(p, n)| serde_json::json!({ "project": p, "notes": n }))
                .collect::<Vec<_>>(),
            "receipt": self.receipt.to_json(self.window),
            "phases": self.phases.to_json(),
            "searches": {
                "ran": self.searches.ran,
                "answered": self.searches.answered,
                "sessions": self.searches.sessions,
                "crossed": self.searches.crossed,
            },
            "window_opened_at": self.window,
            "memory_hooks": hooks.as_str(),
            "verdict": self.receipt.verdict(hooks).as_str(),
        });
        if let crate::hooks::HookState::Incomplete { missing } = hooks {
            doc["memory_hooks_missing"] = missing.clone().into();
        }
        doc
    }
}

/// The whole of `amb memory status`, as text.
///
/// **Pure, and that is the point of moving it** (D78, D92). This was 190 lines of `println!`
/// inside `run_memory` — a 881-line function in the one file with no tests — and the receipt it
/// prints is the instrument D59 retires the injection layer on. Every rule about *how* a number
/// is read lived there: that the hook caveat prints above the ratio because a caveat underneath
/// one is read after the ratio has been believed; that `unprompted` prints at zero because a zero
/// is an answer; that the lane caveat travels with the lanes. None of it could be tested, and
/// three of those rules were added by decisions (D74, D87, D89) that could only assert them by
/// hand.
///
/// `failures` and `hooks` are passed in rather than read here, so the shell keeps the I/O and
/// this stays a function of its arguments.
pub fn render_status(
    st: &Status,
    corpus: &str,
    hooks: &crate::hooks::HookState,
    failures: i64,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let Some(vault) = st.vault.as_ref() else {
        return "memory is off — set AMB_VAULT to a directory for your notes".to_string();
    };
    let _ = writeln!(out, "vault {}", vault.display());
    let _ = writeln!(
        out,
        "{} note(s) on disk · {} indexed · {} active · {} superseded",
        st.on_disk, st.indexed, st.active, st.superseded
    );
    if st.drifted() {
        let _ = writeln!(
            out,
            "  ! the index disagrees with the vault — run `amb memory index`"
        );
    }
    // *Is this actually capturing?* answered without reading a log — the question claude-mem's
    // corpus shows nobody could answer for three months.
    if failures > 0 {
        let _ = writeln!(
            out,
            "  ! the memory hook has failed {failures} time(s) in a row{}",
            if failures >= FAIL_LOUD_AFTER {
                " — it is capturing nothing; run with AMB_HOOK_DEBUG=1"
            } else {
                ""
            }
        );
    }
    for (project, n) in &st.projects {
        let _ = writeln!(out, "  {project}: {n}");
    }
    // Loud, and above the receipt. A note whose file will not parse is content that is *gone* —
    // the vault is truth and the index holds none of it (D34) — so this is the one line in the
    // status that reports a loss rather than a measurement (D62).
    if st.unreadable > 0 {
        let _ = writeln!(
            out,
            "  ! {} note(s) on disk will not parse. The vault is truth and the index holds no \
             content, so that content is gone. `amb memory index` names them.",
            st.unreadable
        );
    }
    let r = &st.receipt;
    // Above the numbers, because it decides how to read them. A caveat printed underneath a ratio
    // is read after the ratio has already been believed.
    if let Some(line) = hooks.caveat() {
        let _ = writeln!(out, "  {line}");
    }
    // **Which corpus, above the numbers.** Same reason the hook caveat is printed first: a ratio
    // read without knowing its window is a different claim from the one meant, and this receipt's
    // floor retires a feature (D59). Before D87 this line could not exist.
    let _ = writeln!(out, "counting over {corpus}");
    let _ = writeln!(
        out,
        "receipt: {} injected · {} cited · ratio {:.2} over {} session(s)",
        r.injected + r.injected_file,
        r.cited + r.cited_after_file,
        r.ratio(),
        r.sessions
    );
    // Split by *how the note was retrieved*, which is the evidence available on whether path
    // anchoring beats recency for observations — the design's weakest-evidenced claim
    // (`MEMORY-DESIGN.md` §6). Each lane carries the session count it actually fired in, because
    // the two do not have the same exposure and the bare ratios read as a comparison they have
    // not earned (D74).
    if r.injected > 0 || r.injected_file > 0 {
        let _ = writeln!(
            out,
            "  by recency (session start): {}/{} · {:.2}  in {} session(s)",
            r.cited,
            r.injected,
            r.session_ratio(),
            r.recency_sessions
        );
        let _ = writeln!(
            out,
            "  by path (before a file):    {}/{} · {:.2}  in {} session(s)",
            r.cited_after_file,
            r.injected_file,
            r.file_ratio(),
            r.path_sessions
        );
        if let Some(caveat) = r.lane_caveat() {
            let _ = writeln!(out, "  ! {caveat}");
        }
    }
    // **The only citations that are not the system's own echo**, and the reason they are counted
    // apart rather than folded into `cited`. Printed even at zero, because a zero is the current
    // answer rather than a missing measurement (D47).
    let _ = writeln!(
        out,
        "  unprompted (never shown, used anyway): {}",
        r.unprompted
    );
    // Force ships with the number that decides whether it was worth adding (D64).
    for (force, injected, cited) in &r.by_force {
        let ratio = if *injected == 0 {
            0.0
        } else {
            *cited as f64 / *injected as f64
        };
        let _ = writeln!(out, "  as {force:<8}: {cited}/{injected} · {ratio:.2}");
    }
    // **Above the verdict, because it decides how to read it** — the same placement rule the hook
    // caveat and the corpus line follow. `too early` underneath would already have been believed
    // as progress by the time this qualified it (D95).
    if let Some(line) = r.arrival_note(st.window) {
        let _ = writeln!(out, "  {line}");
    }
    // D59's standing verdict, printed at every stage including "not yet" — a condition that only
    // becomes visible once it fires is one nobody can plan around (D54, D58).
    let _ = match r.verdict(hooks) {
        Verdict::TooEarly { sessions, injected } => writeln!(
            out,
            "  verdict: too early — needs {sessions} more session(s) and {injected} more \
             injection(s) before D59's floor means anything"
        ),
        Verdict::Earning => writeln!(
            out,
            "  verdict: injection is converting above D59's floor of {VERDICT_FLOOR:.2}"
        ),
        Verdict::RetrievalSuspect => writeln!(
            out,
            "  ! verdict: below D59's floor, but notes ARE reached for unprompted — a retrieval \
             problem, not a worthless corpus. Fix retrieval; do not withdraw"
        ),
        Verdict::Withdraw => writeln!(
            out,
            "  ! verdict: below D59's floor of {VERDICT_FLOOR:.2} and nothing was ever reached \
             for unprompted. D59 says withdraw the injection layer rather than extend it"
        ),
        Verdict::NotRunning { missing } => writeln!(
            out,
            "  ! verdict: none — the memory hooks are not installed ({}), so no ratio here is \
             evidence about the corpus. D59 cannot fire on a layer that has not run",
            missing.join(", ")
        ),
    };
    // The receipts the plan requires for Phases 2, 3 and 4b — the ones that say whether each
    // phase is doing anything, as opposed to whether it exists.
    let p = &st.phases;
    if p.candidates > 0 {
        let _ = writeln!(
            out,
            "phase 2: {} candidate(s), {} at the threshold, {} promoted, {} declined",
            p.candidates, p.reached_threshold, p.promoted, p.declined
        );
        if p.suppressed > 0 {
            let _ = writeln!(
                out,
                "  {} candidate(s) held back by a decline — offers that were earned and not made, \
                 which is what declining has bought",
                p.suppressed
            );
        }
        match p.decline_rate() {
            // D49's withdrawal condition is read off this number.
            Some(rate) => {
                let _ = writeln!(out, "  decline rate {rate:.2} over {} offer(s)", p.offers());
                if rate == 0.0 && p.offers() >= 3 {
                    let _ = writeln!(
                        out,
                        "  ! nothing has ever been declined — if approval has become reflex, D49 \
                         says withdraw the phase rather than patch it"
                    );
                }
            }
            None => {
                let _ = writeln!(
                    out,
                    "  no offers yet, so no decline rate — not a rate of zero"
                );
            }
        }
    }
    if p.export_checks > 0 || p.export_failures > 0 {
        let _ = writeln!(
            out,
            "phase 3: export --check run {} time(s), fired {} time(s)",
            p.export_checks, p.export_failures
        );
    }
    let _ = writeln!(
        out,
        "phase 4b: `--across-repos` run {} time(s) (the explicit surface)",
        p.cross_repo_queries
    );
    if r.unprompted > 0 {
        let _ = writeln!(
            out,
            "  {} cite(s) of notes this session was never shown",
            r.unprompted
        );
    }
    // Printed unconditionally and next to `unprompted`, because it is what makes that number
    // readable: reaching for a note and not finding one is not the same finding as never
    // reaching (D89).
    let _ = writeln!(out, "{}", st.searches.note(r.unprompted));
    // The differentiator's own line, counted where it fires rather than on the flag (D91).
    let _ = writeln!(out, "{}", st.searches.crossed_note());
    // The plan's own stopping rule, stated where the number is read rather than only in a
    // document nobody opens while looking at it.
    if r.injected + r.injected_file > 0 && r.cited + r.cited_after_file == 0 {
        let _ = writeln!(
            out,
            "  nothing injected has ever been cited — if that holds over two weeks of real \
             sessions, this feature has been answered and should be switched off"
        );
    }
    out.trim_end().to_string()
}

pub fn status(conn: &Connection, since: Option<f64>) -> Result<Status> {
    let vault = vault_path();
    let on_disk = vault.as_deref().map(count_on_disk).unwrap_or(0);
    let unreadable = vault.as_deref().map(count_unreadable).unwrap_or(0);
    // Every kind, to match what `count_on_disk` walks. `drifted()` compares the two, so they have
    // to describe the same set or it reports drift that is only a difference of definition.
    let count = |st: Option<&str>| -> Result<usize> {
        let n: i64 = match st {
            Some(s) => conn.query_row(
                "SELECT count(*) FROM notes WHERE status = ?1",
                params![s],
                |r| r.get(0),
            ),
            None => conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)),
        }
        .map_err(sql("counting notes"))?;
        Ok(n as usize)
    };
    let mut stmt = conn
        .prepare(
            "SELECT CASE WHEN scope = '' THEN kind ELSE scope END, count(*)
               FROM notes GROUP BY 1 ORDER BY 2 DESC",
        )
        .map_err(sql("summarising the vault"))?;
    let projects: Vec<(String, usize)> = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })
        .map_err(sql("summarising the vault"))?
        .flatten()
        .collect();
    drop(stmt);
    Ok(Status {
        vault,
        on_disk,
        unreadable,
        indexed: count(None)?,
        active: count(Some(ACTIVE))?,
        superseded: count(Some(SUPERSEDED))?,
        projects,
        receipt: receipt(conn, since)?,
        phases: match vault_path() {
            Some(v) => phase_receipts(conn, &v, crate::db::now()?)?,
            None => PhaseReceipts::default(),
        },
        searches: searches(conn, since)?,
        window: window_start(conn, INJECTION_WINDOW)?,
    })
}

/// Every `.md` in the vault, across all four kinds.
///
/// **It counted only `projects/` and the label said "notes on disk"**, so a vault holding
/// candidates and decisions reported them as absent — and `status` compared that against an index
/// count restricted the same way, so the two agreed while both understated. The same defect as the
/// "2 of 1 note(s)" header: a count that does not describe what it claims to (D54).
fn count_on_disk(vault: &Path) -> usize {
    note_files(vault).len()
}

/// Every `.md` under one directory. `.amb-tmp` siblings from an interrupted write are excluded by
/// the extension filter, which is why the temporary file carries one.
fn md_files(dir: &Path) -> impl Iterator<Item = std::path::PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
}

/// Notes on disk that `parse_note` cannot read — the same parse the indexer uses, so "readable"
/// means one thing rather than two that drift.
///
/// **This exists because a destroyed note used to report itself healthy.** A crash during a
/// rewrite left a zero-byte file; the reindex skipped it and said so, but the index row survived,
/// `status` reported `on_disk 1 · indexed 1 · drifted false`, and `SessionStart` went on injecting
/// a note whose body no longer existed. That is D45's defect inverted — there a full vault
/// reported itself empty, here a lost note reports itself present — and both are the silence this
/// project treats as its worst failure shape.
fn count_unreadable(vault: &Path) -> usize {
    note_files(vault)
        .iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .ok()
                .and_then(|t| parse_note(&t, "", 0.0))
                .is_none()
        })
        .count()
}

/// Every note file in the vault, across the flat kinds and the per-project ones.
///
/// **The one walk.** `status` counts what is here, `count_unreadable` parses it, and
/// `unknown_keys` scans its frontmatter — three numbers a reader compares against each other, so
/// a second copy of this walk is a way for them to describe different populations while looking
/// comparable. That happened once already: `count_on_disk` saw only `projects/`, so a vault of
/// candidates reported them absent and `drifted()` compared two counts that agreed by both being
/// wrong (D54).
///
/// Order is `read_dir` order and no caller may rely on it — two count it, and `unknown_keys`
/// imposes its own total order on what it reports.
fn note_files(vault: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for flat in ["candidates", "global"] {
        out.extend(md_files(&vault.join(flat)));
    }
    for nested in ["projects", "decisions", "topics", "captures"] {
        let Ok(dirs) = std::fs::read_dir(vault.join(nested)) else {
            continue;
        };
        for d in dirs.flatten().filter(|e| e.path().is_dir()) {
            out.extend(md_files(&d.path()));
        }
    }
    out
}

/// Every frontmatter key `amb` itself writes **or** reads, kept in step with both by
/// `every_frontmatter_key_is_accounted_for`.
///
/// **Written *or* read, not just read, and that distinction was a bug.** `derived_count` and
/// `derived_in` are emitted by `Note::render` for the human opening the file and are deliberately
/// never parsed back — the ledger underneath them is the authority. Measuring this list against
/// `parse_note` alone made `amb memory index` warn `read by nothing` about `amb`'s own output, on
/// every candidate that had ever derived. The text was true and the implication was false, which
/// is worse than a wrong warning: it trains the reader to skip the line, and the next genuinely
/// dead key arrives underneath a warning nobody reads any more.
///
/// **This is the list a warning is measured against, so a stale entry is not a cosmetic problem.**
/// A key here that nothing writes and nothing reads makes `unknown_keys` silent about a genuinely
/// dead field — the exact defect the warning exists to catch, hidden by the catcher.
pub(crate) const KNOWN_KEYS: &[&str] = &[
    "agent",
    "cites",
    "created",
    "declined_after",
    "declined_at",
    "derivations",
    "derived_count",
    "derived_in",
    "files",
    "force",
    "id",
    "kind",
    "promoted_from",
    "promoted_to",
    "scope",
    "session",
    "status",
    "superseded_by",
    "supersedes",
    "title",
    "visibility",
];

/// A frontmatter key present in a note that no reader consults.
///
/// `Ord` is derived and the fields are in report order, so `sort()` is the reporting order — a
/// hand-written comparator would keep compiling, and silently stop covering, a field added below.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnknownKey {
    /// Vault-relative path, because the fix is to edit that file.
    pub note: String,
    pub key: String,
}

/// Frontmatter keys nothing reads. **Deterministic, and it never fails a note.**
///
/// `amb` gained `force` recently and will gain more fields; the documented failure here is the
/// mirror of this project's recurring one. `find_unread_fields.py` catches a struct field with no
/// reader; this catches the same thing one layer out — a *file* recording something true that no
/// code will ever consult. Both are silent, and silence is this project's worst failure shape.
///
/// A typo'd key is the common case and it is not an error: the vault is hand-editable markdown
/// that Obsidian and a human both write into, so an unrecognised key costs a warning, never a
/// note. Unreadable files are skipped here and counted by `count_unreadable` instead, so one
/// broken file is reported once rather than twice.
pub fn unknown_keys(vault: &Path) -> Vec<UnknownKey> {
    let mut out = Vec::new();
    for path in note_files(vault) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((front, _)) = split_frontmatter(&text) else {
            continue;
        };
        let (scalars, lists) = scan_frontmatter(front);
        let note = path
            .strip_prefix(vault)
            .unwrap_or(&path)
            .display()
            .to_string();
        let mut keys: Vec<String> = scalars
            .iter()
            .map(|(k, _)| k.clone())
            .chain(lists.iter().map(|(k, _)| k.clone()))
            .collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                out.push(UnknownKey {
                    note: note.clone(),
                    key,
                });
            }
        }
    }
    out.sort();
    out
}

/// How much of what sessions actually edit is covered by a note claiming to concern it.
///
/// **Read-only, and it exists to separate two states the receipt cannot tell apart.** A project
/// where path-anchored injection has *nothing to inject* — barely any edited file carries a note —
/// and one where it has *nothing worth injecting* — files are covered and the notes still go
/// uncited — both read as `0 cited` in the receipt today, and they have opposite responses. The
/// first says write more notes; the second says stop injecting by path. `MEMORY-DESIGN.md` §6's
/// open question is which retrieval mode earns its context, and it cannot be answered while those
/// two are indistinguishable.
///
/// **The denominator is `claims`, not the repository, and that is the load-bearing choice.**
/// `amb` does not walk a repo it was not asked about, and "files a session actually touched" is a
/// truer population than "files that exist": a note covering a file nobody opens can never be
/// injected however good it is, and counting it as uncovered ground would overstate the gap.
/// It also means this needs no `git` invocation — the project reads git plumbing files directly
/// and shells out nowhere, which this preserves.
///
/// **The denominator is cumulative, with one hole.** An expired claim is left in the table rather
/// than deleted, so a path counts as edited long after the session that touched it ended — which
/// is what makes this comparable across weeks rather than a rolling window. `amb release` does
/// delete its row, so a released path stops counting as ever-edited. Coverage can therefore rise
/// without a single note being written. Read a change in it against `edited`, never alone.
// No `Eq`: `uncovered` carries a claim expiry, and a float has no total equality. `PartialEq`
// is what the type can honestly offer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    pub project: String,
    /// Distinct paths sessions have actually edited, from `claims`.
    pub edited: usize,
    /// Of those, how many at least one *injectable* note concerns — asked through the injection
    /// query itself, so this counts notes from any project, exactly as retrieval does.
    pub covered: usize,
    /// The subset of `covered` reached by a note belonging to this project. `covered -
    /// covered_here` is the cross-project contribution, which is the one claim here no per-repo
    /// tool can make and was previously invisible.
    pub covered_here: usize,
    /// Distinct path globs the project's notes declare.
    pub declared: usize,
    /// Notes no edited path reached, with the paths they declare — aimed where nobody works.
    pub unmatched: Vec<Unreached>,
    /// **The reverse reading: edited paths no injectable note concerns.**
    ///
    /// `covered` says how much ground is held; this says *which* ground is not, which is the
    /// actionable half. `covered + uncovered.len() == edited`, asserted by
    /// `every_edited_path_is_either_covered_or_reported`, so a path cannot fall out of both.
    ///
    /// Ordered by distinct agents, then by claim expiry, then by path. That is a proxy for "most
    /// worked", not a hotspot ranking — `claims` records no edit count and
    /// [`claims::EditedPath`] says why adding one would be the wrong trade.
    pub uncovered: Vec<claims::EditedPath>,
}

/// A note that path anchoring can never reach, and what it claims to concern.
///
/// **Carries the note, because the path alone was not actionable.** The first version listed globs;
/// a note declaring two paths with only one of them edited had its other path reported as "edited
/// by nobody", which read as the note being stranded when it was reachable. Naming the note makes
/// the reading unambiguous and tells you which file to go and fix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unreached {
    pub note: String,
    pub path: String,
}

impl Coverage {
    /// Covered as a fraction of edited. `None` when nothing has been edited, because zero over
    /// zero is not zero — a project with no claims yet has an *unknown* coverage, and rendering
    /// it as `0%` would read as a failure that has not happened.
    pub fn ratio(&self) -> Option<f64> {
        (self.edited > 0).then(|| self.covered as f64 / self.edited as f64)
    }
}

/// Compute [`Coverage`] for one project. Touches no filesystem and writes nothing.
pub fn coverage(conn: &Connection, project: &str) -> Result<Coverage> {
    // Asked of `claims`, which owns what a claim's lifetime means (D5) — the lapsed rows are
    // wanted here, and whether a lapsed row still exists is that module's decision to change.
    let edited = claims::edited_paths(conn, project)?;

    // **Asked through `concerning`, which *is* the injection query, rather than re-deriving its
    // predicate here.** An earlier version replicated it and got three axes wrong at once: it
    // filtered by project (injection deliberately does not — cross-project path anchoring is the
    // thing no per-repo tool can do), ignored `status` (a superseded note can never be injected),
    // and counted candidates (`INJECTABLE` excludes them, which is D51's whole point). Only the
    // glob semantics agreed, and by luck rather than intent — `concerning` applies
    // `claims::overlaps` as a post-filter behind its SQL prefix window, so the replica happened to
    // match. On the board of the day the number still came out right, because every note there was
    // an active observation and no two projects named the same path: correct by accident, which is
    // the state D51 exists to warn about. One call per edited path costs more than one join and
    // cannot drift.
    let mut covered = 0usize;
    let mut covered_here = 0usize;
    let mut reached: Vec<NoteId> = Vec::new();
    // The reverse reading costs nothing to collect: this loop already decides it per path and
    // used to throw the answer away, leaving `24 - 7` as an arithmetic exercise for the reader
    // with no way to find out *which* seventeen.
    let mut uncovered: Vec<claims::EditedPath> = Vec::new();
    for e in &edited {
        let (notes, _) = concerning(conn, &e.path)?;
        if notes.is_empty() {
            uncovered.push(e.clone());
            continue;
        }
        covered += 1;
        if notes.iter().any(|n| n.id.scope == project) {
            covered_here += 1;
        }
        for n in notes {
            if !reached.contains(&n.id) {
                reached.push(n.id);
            }
        }
    }
    // Most-worked first, on the only two signals `claims` carries. `path` last so the order is
    // total and two runs of an unchanged board render identically.
    uncovered.sort_by(|a, b| {
        b.agents
            .cmp(&a.agents)
            .then(b.claim_expires.total_cmp(&a.claim_expires))
            .then(a.path.cmp(&b.path))
    });

    // Declared paths belonging to notes that could actually be injected — same `kind` and
    // `status` filter as the query above, so a retired note's paths are not counted as ground
    // held.
    //
    // **Through `SELECT_NOTE` and `row_to_note`, not a hand-written join.** The `note_paths` key
    // relation (`kind`, `project`, `slug`) is already spelled out in `SELECT_NOTE`, `concerning`
    // and `concerning_kind`; a fourth copy here is the same mistake the comment forty lines above
    // records, one layer down. It also removes a hand-inlined `NoteId` equality — `NoteId` derives
    // `PartialEq`, and a field added to it would not reach a comparison written out by hand, which
    // fails by quietly matching more notes rather than by going red.
    let kinds = injectable_sql();
    let mut stmt = conn
        .prepare(&format!(
            "{SELECT_NOTE} WHERE n.scope = ?1 AND n.status = ?2 AND n.kind IN ({kinds})"
        ))
        .map_err(sql("reading declared paths"))?;
    let notes: Vec<IndexedNote> = stmt
        .query_map(params![project, ACTIVE], row_to_note)
        .map_err(sql("reading declared paths"))?
        .flatten()
        .collect();
    drop(stmt);

    let mut declared: Vec<String> = notes.iter().flat_map(|n| n.paths.clone()).collect();
    declared.sort();
    declared.dedup();

    // Unmatched is asked per *note*, not per glob: a note none of the edited paths reached can
    // never be injected by path, whatever it declares. Reported as its paths, since that is the
    // actionable half.
    let mut unmatched: Vec<Unreached> = notes
        .iter()
        .filter(|n| !reached.contains(&n.id))
        .flat_map(|n| {
            n.paths.iter().map(|g| Unreached {
                note: n.id.display(),
                path: g.clone(),
            })
        })
        .collect();
    // No `dedup`: `note_paths` is `PRIMARY KEY (kind, scope, slug, path_glob)`, so one note
    // cannot declare one glob twice and every `(note, path)` here is already distinct.
    unmatched.sort();

    Ok(Coverage {
        project: project.to_string(),
        edited: edited.len(),
        covered,
        covered_here,
        declared: declared.len(),
        unmatched,
        uncovered,
    })
}

/// How many uncovered paths the text rendering names before it says "and N more".
///
/// A **display** bound, not a query bound — [`coverage`] returns every one and
/// [`Coverage::to_json`] emits every one. Ten because the list is a prompt to go and write a
/// note, and a screenful of paths is a report nobody reads to the end.
///
/// A bound is also a place a number can go wrong: a list that stops without saying so reads as
/// the whole answer. [`render_coverage`] always prints the remainder when it truncates, and
/// `a_truncated_uncovered_list_says_how_many_it_dropped` reddens if that line is removed.
pub const UNCOVERED_SHOWN: usize = 10;

/// The whole of `amb memory coverage`, as text.
///
/// **Pure, for D92's reason.** Three rules lived in `run_memory` with no way to assert them:
/// that an unmeasured project is not a zero-coverage one, that the cross-project line appears
/// only when there is a cross-project contribution, and that truncation announces itself. Each is
/// a sentence a reader believes, and each was one deleted `if` away from silently changing.
pub fn render_coverage(c: &Coverage) -> String {
    let mut out = String::new();
    match c.ratio() {
        // Distinguished rather than collapsed to `0%`: no claims yet means unmeasured, and
        // reporting that as zero coverage names a problem that has not happened.
        None => out.push_str(&format!(
            "{}: no file has been edited under amb yet, so coverage is unmeasured\n",
            c.project
        )),
        Some(r) => out.push_str(&format!(
            "{}: {} of {} edited path(s) covered by a note · {:.0}%\n",
            c.project,
            c.covered,
            c.edited,
            r * 100.0
        )),
    }
    out.push_str(&format!("  {} path(s) declared by notes\n", c.declared));
    // Printed only when it happened, because the claim is specific: these paths are covered by a
    // note belonging to a *different* project, which is the retrieval no per-repo tool can do. A
    // constant `0 cross-project` line would be noise.
    if c.covered > c.covered_here {
        out.push_str(&format!(
            "  {} of those covered only by another project's note\n",
            c.covered - c.covered_here
        ));
    }
    for u in &c.unmatched {
        out.push_str(&format!(
            "  · {} — declared by {}, edited by nobody\n",
            u.path, u.note
        ));
    }
    // The reverse reading, and the actionable one: `covered` says how much ground is held, this
    // says which ground is not.
    if !c.uncovered.is_empty() {
        out.push_str(&format!(
            "  {} edited path(s) no note concerns:\n",
            c.uncovered.len()
        ));
        for u in c.uncovered.iter().take(UNCOVERED_SHOWN) {
            out.push_str(&format!(
                "  · {} — touched by {} agent(s)\n",
                u.path, u.agents
            ));
        }
        // Said out loud rather than trailing off. A list that stops without saying so reads as
        // the whole answer, which is how a bound becomes a wrong number.
        let rest = c.uncovered.len().saturating_sub(UNCOVERED_SHOWN);
        if rest > 0 {
            out.push_str(&format!("  … and {rest} more; --json lists them all\n"));
        }
    }
    out
}

impl Coverage {
    /// The machine surface. **Unbounded where the text is capped**: a caller parsing JSON is
    /// feeding something else, and a truncated list would be a wrong answer rather than a tidy
    /// one.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "project": self.project,
            "edited": self.edited,
            "covered": self.covered,
            "covered_here": self.covered_here,
            "declared": self.declared,
            "ratio": self.ratio(),
            "unmatched": self.unmatched
                .iter()
                .map(|u| serde_json::json!({ "note": u.note, "path": u.path }))
                .collect::<Vec<_>>(),
            "uncovered": self.uncovered
                .iter()
                .map(|u| serde_json::json!({
                    "path": u.path,
                    "agents": u.agents,
                    "claim_expires": u.claim_expires,
                }))
                .collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::hooks::HookState;

    /// A [`Coverage`] with `n` uncovered paths and nothing else going on.
    fn cov(edited: usize, covered: usize, here: usize, uncovered: usize) -> Coverage {
        Coverage {
            project: "nest".into(),
            edited,
            covered,
            covered_here: here,
            declared: 3,
            unmatched: Vec::new(),
            uncovered: (0..uncovered)
                .map(|i| claims::EditedPath {
                    path: format!("src/f{i}.rs"),
                    agents: 2,
                    claim_expires: 0.0,
                })
                .collect(),
        }
    }

    /// Zero over zero is not zero.
    ///
    /// The rule the `None` arm exists for: a project with no claims has *unknown* coverage, and
    /// printing `0%` names a failure that has not happened. Collapse the arm to a plain ratio and
    /// this reddens on the percent sign rather than on the wording.
    #[test]
    fn an_unmeasured_project_is_not_a_zero_coverage_one() {
        let text = render_coverage(&cov(0, 0, 0, 0));
        crate::assert_rendered_shape("render_coverage", &text);
        assert!(text.contains("unmeasured"), "{text}");
        assert!(
            !text.contains('%'),
            "an unmeasured project printed a percentage:\n{text}"
        );
    }

    /// The cross-project line is a specific claim, so it appears only when it is true.
    ///
    /// **It is the one claim here no per-repo tool can make**, which is exactly why a constant
    /// `0 cross-project` line would be worse than none — it would advertise the capability on
    /// every board that has never used it.
    #[test]
    fn the_cross_project_line_appears_only_when_a_foreign_note_covered_something() {
        let none = render_coverage(&cov(10, 6, 6, 4));
        assert!(!none.contains("another project's note"), "{none}");

        let some = render_coverage(&cov(10, 6, 4, 4));
        assert!(
            some.contains("2 of those covered only by another project's note"),
            "{some}"
        );
    }

    /// A bound that does not announce itself is how a list becomes a wrong number.
    ///
    /// Delete the `rest > 0` block and this reddens: the render still *looks* complete, naming
    /// ten paths and stopping. That is the failure mode — not a crash, a plausible short answer.
    #[test]
    fn a_truncated_uncovered_list_says_how_many_it_dropped() {
        let text = render_coverage(&cov(13, 0, 0, 13));
        assert!(
            text.contains("13 edited path(s) no note concerns"),
            "{text}"
        );
        assert_eq!(
            text.matches("  · src/f").count(),
            UNCOVERED_SHOWN,
            "the rendering named a number of paths other than its own bound:\n{text}"
        );
        assert!(text.contains("… and 3 more"), "{text}");
    }

    /// The machine surface is unbounded where the text is capped.
    ///
    /// Both halves matter, so both are asserted by count rather than by presence: a JSON consumer
    /// feeding something else needs every path, and a truncated list there is a wrong answer
    /// rather than a tidy one.
    #[test]
    fn the_json_surface_lists_every_uncovered_path_the_text_capped() {
        let c = cov(13, 0, 0, 13);
        assert_eq!(
            c.to_json()["uncovered"].as_array().expect("array").len(),
            13
        );
        assert_eq!(
            render_coverage(&c).matches("  · src/f").count(),
            UNCOVERED_SHOWN
        );
    }

    /// `ratio` is unknown at zero edited and a real fraction above it.
    #[test]
    fn the_ratio_is_unknown_at_zero_and_a_fraction_above_it() {
        assert_eq!(cov(0, 0, 0, 0).ratio(), None);
        assert_eq!(cov(4, 1, 1, 3).ratio(), Some(0.25));
    }

    /// A receipt with numbers in it, so the ordering rules have something to order.
    fn filled() -> Status {
        Status {
            vault: Some(std::path::PathBuf::from("/v")),
            on_disk: 3,
            unreadable: 0,
            indexed: 3,
            active: 3,
            superseded: 0,
            projects: vec![("nest".into(), 3)],
            receipt: Receipt {
                injected: 40,
                cited: 5,
                injected_file: 17,
                cited_after_file: 0,
                unprompted: 0,
                sessions: 3,
                recency_sessions: 3,
                path_sessions: 2,
                by_force: vec![("advice".into(), 57, 5)],
            },
            phases: PhaseReceipts::default(),
            searches: Searches::default(),
            window: None,
        }
    }

    fn line_of(text: &str, needle: &str) -> usize {
        text.lines()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not in:\n{text}"))
    }

    /// A caveat printed under a ratio is read after the ratio has been believed.
    ///
    /// **Three separate decisions state this rule in a comment and none could assert it** — D74's
    /// lane caveat, D87's corpus line and the hook caveat all say "above the numbers", and all
    /// three lived in 190 lines of `println!` inside `run_memory`, in the one file with no tests
    /// (D92). Moving the rendering into the library is what makes the rule checkable; this is the
    /// check.
    #[test]
    fn every_caveat_is_read_before_the_number_it_qualifies() {
        let st = filled();
        let out = render_status(
            &st,
            "all time — every event on the board",
            &HookState::Unknown,
            0,
        );
        crate::assert_rendered_shape("render_status", &out);

        assert!(
            line_of(&out, "counting over") < line_of(&out, "receipt:"),
            "which corpus must be known before the ratio is:\n{out}"
        );
        assert!(
            line_of(&out, "receipt:") < line_of(&out, "by recency"),
            "the headline ratio comes before the lane split:\n{out}"
        );
        assert!(
            line_of(&out, "by path") < line_of(&out, "the lanes are not directly comparable"),
            "D74's caveat sits with the lanes it is about:\n{out}"
        );
        assert!(
            line_of(&out, "recall:") > line_of(&out, "unprompted"),
            "D89's line exists to qualify `unprompted` and must be next to it:\n{out}"
        );
    }

    /// A zero is an answer, not a missing measurement (D47).
    #[test]
    fn unprompted_is_printed_at_zero() {
        let out = render_status(&filled(), "all time", &HookState::Unknown, 0);
        assert!(
            out.contains("unprompted (never shown, used anyway): 0"),
            "hiding a zero makes it look unmeasured:\n{out}"
        );
    }

    /// Memory off says so and says nothing else — no ratio computed over an absent corpus.
    #[test]
    fn no_vault_reports_only_that() {
        let st = Status {
            vault: None,
            ..filled()
        };
        let out = render_status(&st, "all time", &HookState::Unknown, 0);
        assert_eq!(
            out,
            "memory is off — set AMB_VAULT to a directory for your notes"
        );
    }

    /// A failing hook is reported before any number it would have produced.
    #[test]
    fn a_failing_hook_is_reported_above_the_receipt() {
        let out = render_status(&filled(), "all time", &HookState::Unknown, FAIL_LOUD_AFTER);
        assert!(out.contains("it is capturing nothing"), "{out}");
        assert!(
            line_of(&out, "has failed") < line_of(&out, "receipt:"),
            "a receipt read without knowing the hook is broken is a wrong reading:\n{out}"
        );
    }

    use super::*;

    /// `--json` carries the hook state and the verdict, or a machine consumer repeats D59.
    ///
    /// **Written because the first version of this fix could not be tested.** The keys were merged
    /// onto the document inside `src/main.rs`, so nothing in the suite could see them, and the
    /// surface an agent is actually told to use went a whole commit emitting a bare ratio. The
    /// merge now lives on `Status::to_json`, which is why this assertion can exist at all.
    #[test]
    fn the_machine_surface_states_whether_the_layer_ran() {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let st = status(&conn, None).expect("status");

        let missing = vec!["PreToolUse".to_string()];
        let doc = st.to_json(&crate::hooks::HookState::Incomplete {
            missing: missing.clone(),
        });
        assert_eq!(doc["memory_hooks"], "incomplete", "{doc}");
        assert_eq!(
            doc["verdict"], "not_running",
            "a consumer that gets counts without this computes its own ratio and reaches D59's \
             conclusion unaided: {doc}"
        );
        assert_eq!(doc["memory_hooks_missing"][0], "PreToolUse", "{doc}");

        let doc = st.to_json(&crate::hooks::HookState::Installed);
        assert_eq!(doc["memory_hooks"], "installed", "{doc}");
        assert!(
            doc["memory_hooks_missing"].is_null(),
            "a healthy install must not carry an empty missing list: {doc}"
        );
        assert_ne!(doc["verdict"], "not_running", "{doc}");
    }
    /// The anti-drift guard for [`KNOWN_KEYS`], and the reason the warning can be trusted.
    ///
    /// A hand-maintained second list of field names *will* drift from the code that reads them,
    /// and both directions of drift are harmful: a key `parse_note` reads but `KNOWN_KEYS` omits
    /// makes `unknown_keys` warn about a working field forever, and a key `KNOWN_KEYS` declares
    /// but nothing reads makes it stay silent about a dead one — the very defect it exists to
    /// find. So the list is checked against `parse_note`'s own source rather than against memory.
    #[test]
    fn every_frontmatter_key_is_accounted_for() {
        let src = include_str!("note.rs");
        let start = src
            .find("pub fn parse_note(")
            .expect("parse_note is in note.rs");
        let end = start + src[start..].find("\n}\n").expect("parse_note ends");
        let body = &src[start..end];

        let mut read: Vec<String> = Vec::new();
        for pat in ["get(\"", "list(\"", "k == \""] {
            let mut rest = body;
            while let Some(i) = rest.find(pat) {
                rest = &rest[i + pat.len()..];
                if let Some(j) = rest.find('"') {
                    read.push(rest[..j].to_string());
                }
            }
        }
        read.sort();
        read.dedup();

        // Without this the guard passes vacuously the moment the extraction stops matching —
        // a green test proving nothing, which this project treats as worse than no test.
        assert!(
            read.len() > 10,
            "extracted only {read:?} from parse_note — the scan broke, so the rest of this \
             assertion is meaningless"
        );

        // The other authority is `render`, and it writes two keys `parse_note` never reads:
        // `derived_count` and `derived_in`, both for the human opening the file. Measuring
        // KNOWN_KEYS against the reader alone is what made `amb memory index` warn about `amb`'s
        // own output. Taken from `render` itself rather than from a scan of its source, so a key
        // it gains is picked up here without anyone remembering to update a pattern.
        let note = Note {
            id: NoteId {
                kind: CANDIDATE.to_string(),
                scope: "nest".to_string(),
                slug: "s".to_string(),
            },
            title: "t".to_string(),
            status: CANDIDATE.to_string(),
            created: 1.0,
            session: Some("sess".to_string()),
            agent: Some("a".to_string()),
            files: vec!["src/x.rs".to_string()],
            cites: vec!["nest/other".to_string()],
            supersedes: Some("nest/old".to_string()),
            superseded_by: Some("nest/new".to_string()),
            derivations: vec![Derivation {
                ts: 1.0,
                project: "nest".to_string(),
                session: "sess".to_string(),
                note: "nest/n".to_string(),
                topics: vec!["rust".to_string()],
            }],
            promoted_from: Some("candidate/c".to_string()),
            promoted_to: Some("nest/d".to_string()),
            visibility: Some("private".to_string()),
            // `RULE`, not `ADVICE`: render omits `force` when it holds the default (D64), so
            // the default would leave this note one key short of fully populated — which the
            // equality below caught the first time it ran.
            force: RULE.to_string(),
            declined_at: Some(1.0),
            declined_after: Some(1),
            body: "body".to_string(),
        };
        let rendered = note.render();
        let (front, _) = split_frontmatter(&rendered).expect("render emits frontmatter");
        let (scalars, lists) = scan_frontmatter(front);
        let mut written: Vec<String> = scalars
            .iter()
            .map(|(k, _)| k.clone())
            .chain(lists.iter().map(|(k, _)| k.clone()))
            .collect();
        written.sort();
        written.dedup();

        // Same vacuity guard as above: a `Note` that stopped rendering most of its fields would
        // otherwise let this pass by writing almost nothing.
        assert!(
            written.len() > 10,
            "render emitted only {written:?} — the fully-populated note above stopped being \
             fully populated, so the rest of this assertion is meaningless"
        );

        let known: Vec<String> = KNOWN_KEYS.iter().map(|k| (*k).to_string()).collect();

        // A fully-populated note renders *every* key in the vocabulary, so this is equality
        // rather than containment — which also makes the population above self-checking. Drop a
        // field from the note and `written` shrinks below `known`, instead of the test quietly
        // measuring less than it claims to.
        assert_eq!(
            written, known,
            "KNOWN_KEYS and the keys render actually writes have diverged — a key amb emits but \
             omits from this list makes `amb memory index` warn about amb's own output"
        );

        // Everything `parse_note` reads must be in the vocabulary too. Containment, not equality:
        // `derived_count` and `derived_in` are written for a human and deliberately never read
        // back, so requiring the reader to cover the whole list is what produced the bug.
        let missing: Vec<&String> = read.iter().filter(|k| !known.contains(k)).collect();
        assert!(
            missing.is_empty(),
            "parse_note reads {missing:?}, which KNOWN_KEYS does not list — `unknown_keys` would \
             warn that a key it genuinely consults is read by nothing"
        );
    }
    #[test]
    fn a_frontmatter_key_nothing_reads_is_warned_about_and_the_note_still_parses() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vault = dir.path();
        let proj = vault.join("projects").join("nest");
        std::fs::create_dir_all(&proj).expect("mkdir");
        std::fs::write(
            proj.join("a.md"),
            "---\nscope: nest\ntitle: t\nconfidance: high\nfiles:\n  - src/x.rs\n---\nbody\n",
        )
        .expect("write");

        let found = unknown_keys(vault);
        assert_eq!(found.len(), 1, "expected exactly one ghost, got {found:?}");
        assert_eq!(found[0].key, "confidance");
        assert!(found[0].note.contains("a.md"), "{:?}", found[0].note);

        // Warn, never error: the note is still fully readable, and `files` — a real key sharing
        // the file with the typo — is untouched. This is the positive half, asserted explicitly
        // because a filter that rejected the whole note would also produce zero *wrong* warnings.
        let text = std::fs::read_to_string(proj.join("a.md")).expect("read");
        let note = parse_note(&text, "a", 0.0).expect("the note still parses");
        assert_eq!(note.title, "t");
        assert_eq!(note.files, vec!["src/x.rs".to_string()]);
    }
    /// A note at every (kind, scope) is a note `note_files` can see.
    ///
    /// **`vault_dir` and `note_files` hold the same layout knowledge and neither derives it from
    /// the other.** `vault_dir` is the authority — `vault_rel` and `reindex` both route through it
    /// — while `note_files` names those directories a second time, as string literals. Add a sixth
    /// directory and the indexer picks it up, but `unknown_keys` and `count_unreadable` go silently
    /// blind to it: notes that exist, are indexed, are injected, and are never checked. Not an
    /// error — an omission that reports success, which is this project's failure shape.
    ///
    /// **The cross-check had to change shape with the axis, and that is the interesting part.**
    /// It used to count `=>` arms in `kind_dir`, which worked while the layout was a function of
    /// `kind` alone. `vault_dir` branches on kind *and* scope, so arm-counting would now be a
    /// number with no meaning. What is actually invariant is the set of **top-level directories**
    /// the function can name, so that is what is compared — read out of its source, against what
    /// this test's own pairs produce.
    #[test]
    fn a_note_of_every_kind_is_seen_by_the_vault_walk() {
        use crate::address::GLOBAL;
        let pairs = [
            (OBSERVATION, "nest"),
            (DECISION, "nest"),
            (DECISION, "#rust"),
            (DECISION, GLOBAL),
            (CANDIDATE, UNSCOPED),
            (CAPTURE, "nest"),
        ];

        let dir = tempfile::tempdir().expect("temp dir");
        let vault = dir.path();
        for (kind, scope) in pairs {
            let rel = vault_rel(kind, scope, "n");
            let path = vault.join(&rel);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
            std::fs::write(&path, "---\ntitle: t\nconfidance: high\n---\nbody\n").expect("write");
        }

        let found = unknown_keys(vault);
        let seen: Vec<&str> = found.iter().map(|u| u.note.as_str()).collect();
        assert_eq!(
            found.len(),
            pairs.len(),
            "every directory must be walked; saw {seen:?}"
        );

        // The pair list above is local, so it cannot notice a *new* directory on its own — and a
        // new directory is the drift that matters. `vault_dir`'s string literals are the
        // authority.
        let src = include_str!("index.rs");
        let start = src
            .find("pub fn vault_dir(")
            .expect("vault_dir is in index.rs");
        let body = &src[start..start + src[start..].find("\n}\n").expect("vault_dir ends")];
        let mut declared: Vec<&str> = body
            .split('"')
            .skip(1)
            .step_by(2)
            .map(|lit| lit.split('/').next().unwrap_or(lit))
            .filter(|d| !d.is_empty() && !d.contains('{'))
            .collect();
        declared.sort_unstable();
        declared.dedup();

        let mut covered: Vec<&str> = pairs
            .iter()
            .map(|(k, s)| vault_dir(k, s))
            .map(|d| {
                let head = d.split('/').next().unwrap_or(&d).to_string();
                Box::leak(head.into_boxed_str()) as &str
            })
            .collect();
        covered.sort_unstable();
        covered.dedup();

        assert_eq!(
            declared, covered,
            "vault_dir can produce {declared:?} but this test only covers {covered:?} — add the \
             new (kind, scope) pair here, and make sure note_files walks the directory it gives"
        );

        // **The third walk, and until now the unguarded one.** `reindex`'s comment claimed this
        // test "counts `vault_dir`'s arms against this walk", and it did not: it compared
        // `vault_dir` against the pair list above and against `note_files`, never against
        // `NESTED_DIRS`. A kind added to `vault_dir` and forgotten there does not merely fail to
        // index — `reindex` builds its prune set from the same walk, so it would DELETE every row
        // under that directory, and `rm board.db && amb memory index` would lose notes. That is
        // D34's central claim, guarded by a sentence that was not true.
        let mut walked: Vec<&str> = crate::memory::index::NESTED_DIRS
            .iter()
            .map(|(_, parent, _)| *parent)
            .chain(crate::memory::index::FLAT_DIRS.iter().map(|(k, sc)| {
                match vault_dir(k, sc).split('/').next() {
                    Some(head) => Box::leak(head.to_string().into_boxed_str()) as &str,
                    None => "",
                }
            }))
            .collect();
        walked.sort_unstable();
        walked.dedup();
        assert_eq!(
            declared, walked,
            "vault_dir can produce {declared:?} but reindex walks {walked:?} — a directory in the \
             first and not the second is one whose notes reindex would prune"
        );
    }
    #[test]
    fn a_vault_of_well_formed_notes_produces_no_warnings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vault = dir.path();
        let proj = vault.join("projects").join("nest");
        std::fs::create_dir_all(&proj).expect("mkdir");
        // Builds its frontmatter *from* KNOWN_KEYS, so it cannot catch an omission from that
        // list — mutation testing showed it staying green when one was dropped, and the comment
        // here used to claim otherwise. `every_frontmatter_key_is_accounted_for` owns that.
        // What this owns is the filter's polarity: inverting `!contains` turns every correct
        // note into a warning, and that mutation does turn this red.
        let front: String = KNOWN_KEYS
            .iter()
            .map(|k| format!("{k}: v\n"))
            .collect::<Vec<_>>()
            .join("");
        std::fs::write(proj.join("a.md"), format!("---\n{front}---\nbody\n")).expect("write");
        assert_eq!(unknown_keys(vault), vec![]);
    }
    #[test]
    fn coverage_matches_a_directory_note_against_a_file_edited_inside_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        conn.execute(
            "INSERT INTO notes (kind, scope, slug, vault_path, title, status, created,
                                derived_count, body_excerpt, mtime, indexed_at)
             VALUES ('observation','nest','n','p','t','active',0,0,'',0,0)",
            [],
        )
        .expect("note");
        conn.execute(
            "INSERT INTO note_paths (kind, scope, slug, path_glob)
             VALUES ('observation','nest','n','src/core')",
            [],
        )
        .expect("path");
        for path in ["src/core/a.rs", "src/core/b.rs", "docs/x.md"] {
            conn.execute(
                "INSERT INTO claims (path, agent, project, taken_at, expires_at)
                 VALUES (?1, 'a', 'nest', 0, 0)",
                params![path],
            )
            .expect("claim");
        }

        let c = coverage(&conn, "nest").expect("coverage");
        assert_eq!(c.edited, 3);
        // The whole point of routing through `claims::overlaps`: an `=` join would score this 0.
        assert_eq!(c.covered, 2, "a directory note must cover files inside it");
        assert_eq!(c.declared, 1);
        assert!(c.unmatched.is_empty(), "{:?}", c.unmatched);
        assert_eq!(c.ratio(), Some(2.0 / 3.0));
    }
    /// Coverage must agree with the query that actually performs the injection, on every axis.
    ///
    /// The first version of `coverage` re-derived the predicate and got three axes wrong: it
    /// filtered by project, ignored `status`, and counted candidates. Glob semantics were the one
    /// axis it agreed on, and by luck rather than intent — `concerning` applies `claims::overlaps`
    /// as a post-filter behind its coarse SQL prefix window, so the replica happened to match. It
    /// still produced the right number on the board of the day, because every note there was an
    /// active observation — correct by accident, which is what D51 was written about. This pins
    /// each axis separately so that re-deriving the predicate cannot pass.
    #[test]
    fn coverage_counts_exactly_what_the_injection_query_would_return() {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let note = |kind: &str, project: &str, slug: &str, status: &str, glob: &str| {
            conn.execute(
                "INSERT INTO notes (kind, scope, slug, vault_path, title, status, created,
                                    derived_count, body_excerpt, mtime, indexed_at)
                 VALUES (?1,?2,?3,'p','t',?4,0,0,'',0,0)",
                params![kind, project, slug, status],
            )
            .expect("note");
            conn.execute(
                "INSERT INTO note_paths (kind, scope, slug, path_glob) VALUES (?1,?2,?3,?4)",
                params![kind, project, slug, glob],
            )
            .expect("path");
        };
        note(OBSERVATION, "other", "cross", ACTIVE, "src/cross.rs");
        note(OBSERVATION, "nest", "dead", SUPERSEDED, "src/dead.rs");
        note(CANDIDATE, "", "cand", ACTIVE, "src/cand.rs");
        note(DECISION, "nest", "dec", ACTIVE, "src/dec.rs");
        note(OBSERVATION, "nest", "own", ACTIVE, "src/own.rs");
        note(
            OBSERVATION,
            "nest",
            "nowhere",
            ACTIVE,
            "src/nobody-edits-this.rs",
        );

        let paths = [
            "src/cross.rs",
            "src/dead.rs",
            "src/cand.rs",
            "src/dec.rs",
            "src/own.rs",
        ];
        for path in paths {
            conn.execute(
                "INSERT INTO claims (path, agent, project, taken_at, expires_at)
                 VALUES (?1,'a','nest',0,0)",
                params![path],
            )
            .expect("claim");
        }

        let c = coverage(&conn, "nest").expect("coverage");
        assert_eq!(c.edited, 5);

        // The axis-by-axis claim, stated as the injection query's own answer so the two cannot
        // drift apart silently.
        for path in paths {
            let hit = !concerning(&conn, path).expect("concerning").0.is_empty();
            let expected = matches!(path, "src/cross.rs" | "src/dec.rs" | "src/own.rs");
            assert_eq!(hit, expected, "injection disagrees about {path}");
        }
        assert_eq!(
            c.covered, 3,
            "cross-project and non-observation notes count; superseded and candidates do not"
        );
        // The cross-project contribution, which the old project-filtered version erased.
        assert_eq!(
            c.covered_here, 2,
            "`other`'s note covers a path no nest note does"
        );
        assert_eq!(c.covered - c.covered_here, 1);

        // `declared` and `unmatched` use the same kind/status filter: the superseded note's path
        // is not ground this project holds, and the candidate is not this project's at all.
        assert_eq!(c.declared, 3, "own + dec + nowhere; not dead, not cand");
        assert_eq!(c.unmatched.len(), 1);
        assert_eq!(c.unmatched[0].path, "src/nobody-edits-this.rs");
        // Names the note, not just the path: the path alone did not say which file to go and fix.
        assert!(
            c.unmatched[0].note.contains("nowhere"),
            "{}",
            c.unmatched[0].note
        );
    }
    /// The reverse reading names the paths, and the two readings partition the population.
    ///
    /// **`covered + uncovered == edited` is the property worth pinning**, because the failure it
    /// forbids is silent: a path dropping out of both readings would leave the ratio right and
    /// the actionable list short, and nothing about the output would look wrong. Same shape as
    /// D45 — an instrument confidently describing a smaller population than it claims.
    #[test]
    fn every_edited_path_is_either_covered_or_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        conn.execute(
            "INSERT INTO notes (kind, scope, slug, vault_path, title, status, created,
                                derived_count, body_excerpt, mtime, indexed_at)
             VALUES ('observation','nest','n','p','t','active',0,0,'',0,0)",
            [],
        )
        .expect("note");
        conn.execute(
            "INSERT INTO note_paths (kind, scope, slug, path_glob)
             VALUES ('observation','nest','n','src/core')",
            [],
        )
        .expect("path");
        // `src/core/a.rs` is concerned by the note. The other two are not, and `docs/x.md` is
        // touched by two agents so it must sort above `README.md`, which is touched by one.
        for (path, agent) in [
            ("src/core/a.rs", "a"),
            ("docs/x.md", "a"),
            ("docs/x.md", "b"),
            ("README.md", "a"),
        ] {
            conn.execute(
                "INSERT INTO claims (path, agent, project, taken_at, expires_at)
                 VALUES (?1, ?2, 'nest', 0, 100)",
                params![path, agent],
            )
            .expect("claim");
        }

        let c = coverage(&conn, "nest").expect("coverage");
        assert_eq!(c.edited, 3);
        assert_eq!(c.covered, 1);
        assert_eq!(
            c.covered + c.uncovered.len(),
            c.edited,
            "every edited path must appear in exactly one of the two readings; \
             uncovered was {:?}",
            c.uncovered
        );

        let paths: Vec<&str> = c.uncovered.iter().map(|u| u.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["docs/x.md", "README.md"],
            "most-worked first: two agents outrank one, and a covered path is never listed"
        );
        assert_eq!(c.uncovered[0].agents, 2);
        assert_eq!(c.uncovered[1].agents, 1);
    }
    #[test]
    fn coverage_reports_unknown_rather_than_zero_when_nothing_has_been_edited() {
        let dir = tempfile::tempdir().expect("temp dir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let c = coverage(&conn, "nest").expect("coverage");
        assert_eq!(c.edited, 0);
        // Not `Some(0.0)`. An unmeasured project must not render as a failing one.
        assert_eq!(c.ratio(), None);
    }

    /// **D59's condition is unreachable on this machine, and the verdict could not say so** (D95).
    ///
    /// `too early — needs 30 more session(s)` reads as progress. It is printed identically when a
    /// floor is approaching and when nothing can approach it, and the second is the live case: the
    /// window had been open ten hours with zero events while sixteen sessions were active, because
    /// all sixteen were resumed and a resumed session writes no injection row (M24).
    #[test]
    fn a_window_nothing_can_enter_says_so_above_the_verdict_that_reads_as_progress() {
        // No window open: "arrival" means nothing over all time, and the line stays away.
        let quiet = render_status(&filled(), "all time", &HookState::Installed, 0);
        assert!(
            !quiet.contains("entered this window"),
            "silent where it would be noise:\n{quiet}"
        );

        let mut st = filled();
        st.window = Some(1.0);
        let out = render_status(&st, "the window opened 10h ago", &HookState::Installed, 0);
        assert!(
            out.contains("3 of 30 session(s) have entered this window"),
            "{out}"
        );
        assert!(
            line_of(&out, "entered this window") < line_of(&out, "verdict:"),
            "under the verdict it is read after the verdict has been believed:\n{out}"
        );

        st.receipt.sessions = 0;
        let stalled = render_status(&st, "the window opened 10h ago", &HookState::Installed, 0);
        assert!(
            stalled.contains("unreachable here rather than unreached"),
            "the state the verdict cannot express:\n{stalled}"
        );
        assert!(
            stalled.contains("verdict: too early"),
            "and it qualifies that verdict rather than replacing it:\n{stalled}"
        );
    }

    /// **Every caveat reaches both surfaces, enumerated rather than sampled** (M26).
    ///
    /// D90's arithmetic: grep the field, count the renderers, count the assertions. Both caveats
    /// render twice — `render_status` for a person and `Receipt::to_json` for a machine — and the
    /// machine half had no assertion of any kind. Deleting `"lane_caveat"` from `to_json` reddened
    /// nothing in 457 tests, and `arrival_note` had never been added there at all: the session
    /// that wrote D95's line put it on the human path only, which is **D87's own defect on the
    /// other half of the same command**, committed by the session that had just read D87.
    ///
    /// The machine surface is the one that matters most here. A reader sees `sessions: 0` beside
    /// `verdict: "too_early"` and concludes the window is filling slowly; nothing in the document
    /// says it cannot fill at all.
    ///
    /// Asserted against the JSON *value*, not a substring of the serialised document, so an
    /// escaping change cannot quietly turn this into a test of nothing. A caveat added without
    /// being listed here stays silent — the residual hole this pattern always has, named for the
    /// same reason `delivery::UNTRUSTED` names its own.
    #[test]
    fn every_caveat_reaches_the_human_surface_and_the_machine_one() {
        let mut st = filled();
        st.window = Some(1.0);

        // Both must actually fire in this fixture, or the loop below asserts over nothing — the
        // defect M17 catalogues, and the one this session committed once already.
        let caveats = [
            (
                "lane_caveat",
                st.receipt
                    .lane_caveat()
                    .expect("the fixture's lanes differ, so D74's caveat applies"),
            ),
            (
                "arrival_note",
                st.receipt
                    .arrival_note(st.window)
                    .expect("the fixture is below the floor, so D95's note applies"),
            ),
        ];

        let human = render_status(&st, "the window opened 10h ago", &HookState::Installed, 0);
        let machine = st.to_json(&HookState::Installed);

        for (key, text) in &caveats {
            assert!(
                human.contains(text.as_str()),
                "{key} never reaches a person:\n{human}"
            );
            assert_eq!(
                machine["receipt"][key].as_str(),
                Some(text.as_str()),
                "{key} never reaches a machine, which is the surface that reads a verdict without \
                 the paragraph around it"
            );
        }
    }

    /// A vault that exists and has nothing in it — the state of every board on the day memory is
    /// switched on, and the state no render test had.
    ///
    /// `filled()` is the only other fixture and it populates the receipt. Every conditional line
    /// in `render_status` is therefore only ever exercised in the direction that prints it.
    fn empty() -> Status {
        Status {
            vault: Some(std::path::PathBuf::from("/v")),
            on_disk: 0,
            unreadable: 0,
            indexed: 0,
            active: 0,
            superseded: 0,
            projects: Vec::new(),
            receipt: Receipt::default(),
            phases: PhaseReceipts::default(),
            searches: Searches::default(),
            window: None,
        }
    }

    /// **Every conditional line in the receipt was guarded in one direction only, and this is the
    /// other one** (M27).
    ///
    /// Thirty-one of `status.rs`'s forty surviving mutants are the same edit: `x > 0` relaxed to
    /// `x >= 0` on a line that decides whether something is *printed*. Nothing was red, because
    /// the only render fixture has numbers in it and every assertion is a `contains`. A `contains`
    /// cannot see a line that should not be there.
    ///
    /// The direction matters more here than in most modules. This page is read by a person
    /// deciding whether to withdraw a feature (D59), and each of these lines reports a *loss* or
    /// an *event*: `! 0 note(s) will not parse … that content is gone` on a healthy vault, a
    /// phase-2 block for a phase that has never run, `nothing injected has ever been cited …
    /// should be switched off` on a board where nothing was ever injected at all. Correct
    /// arithmetic, wrong page — and indistinguishable from real signal to the reader it is for.
    ///
    /// This is the omission rule the project stated after M23 and it is stated there against a
    /// single filter. Here it is the dominant defect of a whole module.
    #[test]
    fn an_empty_board_prints_no_line_that_implies_something_happened() {
        let out = render_status(&empty(), "all time", &HookState::Installed, 0);

        for (rule, needle) in [
            (
                "drift, when the index and the vault agree",
                "the index disagrees",
            ),
            ("D62's loss line, when nothing was lost", "will not parse"),
            ("the lane split, when neither lane fired", "by recency"),
            ("the lane split, when neither lane fired", "by path"),
            ("phase 2, when no candidate has ever existed", "phase 2:"),
            // Vacuous *here* and kept as documentation of why: on an empty board `candidates`
            // is 0, so the enclosing block returns before this line is reached and the
            // assertion proves only what the row above already proved. The real guard is
            // `the_suppression_line_needs_a_suppression_and_not_merely_a_candidate`.
            ("D64's suppression count, at zero", "held back by a decline"),
            ("phase 3, when --check has never run", "phase 3:"),
            (
                "the unprompted detail line, at zero",
                // Not `"never shown"` — that is also inside the line above, which IS printed at
                // zero (D47). A needle short enough to match two lines cannot assert either.
                "cite(s) of notes this session was never shown",
            ),
            (
                "the stopping rule, when nothing was injected",
                "should be switched off",
            ),
        ] {
            assert!(
                !out.contains(needle),
                "{rule}: {needle:?} was printed over an empty board:\n{out}"
            );
        }

        // **And the same rule as a property, which closes the hole the list above has** (M27).
        //
        // A needle list can only assert the lines someone thought to name, and one of them turned
        // out to be vacuous. Every warning `render_status` prints carries the same `  ! ` prefix —
        // there are eight — and on a healthy, empty, installed board not one of them is true. So a
        // ninth added without a guard, or with a guard that admits zero, fails here without anyone
        // having to remember it. This is what catches `failures > 0`, which the list above never
        // named: its own guard is asserted end-to-end by
        // `status_reports_whether_the_hook_is_actually_capturing`, at the outer layer M20 says to
        // suspect first.
        //
        // Stated with `Installed`, because `! verdict: none` is a *correct* alarm when the hooks
        // are missing — the property is about a healthy board, not about silence.
        for line in out.lines() {
            assert!(
                !line.trim_start().starts_with("! "),
                "an alarm fired on an empty, healthy board: {line:?}\n{out}"
            );
        }

        // And the other half, or this test would pass on a function that prints nothing at all.
        // These zeros ARE answers and are printed on purpose (D47) — the test says which is which.
        for needle in [
            "receipt:",
            "unprompted (never shown, used anyway): 0",
            "phase 4b:",
            "verdict:",
        ] {
            assert!(
                out.contains(needle),
                "{needle:?} is a measured zero and must still be printed:\n{out}"
            );
        }
    }

    /// The presence side of the same rule, which a different mutant needs.
    ///
    /// `x > 0` narrowed to `x < 0` never prints, and no fixture had a candidate, a suppression, an
    /// export check or an unprompted cite — so those blocks were unasserted in *both* directions.
    /// M17's shape: the branch was never reached, so nothing about it could be evidence.
    #[test]
    fn each_conditional_block_appears_once_it_has_something_to_report() {
        let st = Status {
            phases: PhaseReceipts {
                candidates: 4,
                reached_threshold: 2,
                promoted: 1,
                declined: 1,
                suppressed: 3,
                export_checks: 7,
                ..PhaseReceipts::default()
            },
            receipt: Receipt {
                unprompted: 2,
                ..Receipt::default()
            },
            unreadable: 1,
            indexed: 1,
            on_disk: 1,
            ..empty()
        };
        let out = render_status(&st, "all time", &HookState::Installed, 0);

        for needle in [
            "will not parse",
            "phase 2: 4 candidate(s)",
            "3 candidate(s) held back by a decline",
            "phase 3: export --check run 7 time(s)",
            "2 cite(s) of notes this session was never shown",
        ] {
            assert!(out.contains(needle), "{needle:?} missing from:\n{out}");
        }
    }
    /// **The lane split is an `||`, so either lane firing alone must still print it.**
    ///
    /// Lifted out of `each_conditional_block_appears_once_it_has_something_to_report`, where it was
    /// a second rule with its own fixture and its own rationale buried inside another test's needle
    /// list — a failure there reported a name that did not describe what broke.
    ///
    /// `&&` in place of `||` would silently drop the split on exactly the board D74's caveat is
    /// about. Both lanes are tried alone because `filled()` sets both, and a fixture with both
    /// cannot tell `||` from `&&`.
    #[test]
    fn the_lane_split_is_printed_when_either_lane_fired_alone() {
        for (lane, r) in [
            (
                "recency only",
                Receipt {
                    injected: 9,
                    ..Receipt::default()
                },
            ),
            (
                "path only",
                Receipt {
                    injected_file: 9,
                    ..Receipt::default()
                },
            ),
        ] {
            let out = render_status(
                &Status {
                    receipt: r,
                    ..empty()
                },
                "all time",
                &HookState::Installed,
                0,
            );
            assert!(
                out.contains("by recency") && out.contains("by path"),
                "{lane}: one lane with exposure must still print the split:\n{out}"
            );
        }
    }

    /// **A nested guard needs its enclosing block to render, or asserting absence proves nothing**
    /// (M27).
    ///
    /// `an_empty_board_prints_no_line_that_implies_something_happened` asserts this line is absent
    /// — and on an empty board `p.candidates` is 0, so the guard at the top of the phase-2 block
    /// returns first and `p.suppressed > 0` is **never evaluated**. The assertion passed, and
    /// `> 0` -> `>= 0` survived it: a board with candidates and no suppression would print
    /// `0 candidate(s) held back by a decline`, which is D64's tombstone-ROI number reporting a
    /// cost that was never paid.
    ///
    /// M17's shape — a filter upstream of the thing under test — reproduced inside a test written
    /// to catch omissions. The first assertion below is the premise, stated so this cannot happen
    /// again silently: if the enclosing block stops rendering, this test says so instead of
    /// quietly passing.
    #[test]
    fn the_suppression_line_needs_a_suppression_and_not_merely_a_candidate() {
        for (suppressed, expected) in [(0, false), (3, true)] {
            let st = Status {
                phases: PhaseReceipts {
                    candidates: 4,
                    suppressed,
                    ..PhaseReceipts::default()
                },
                ..empty()
            };
            let out = render_status(&st, "all time", &HookState::Installed, 0);
            assert!(
                out.contains("phase 2:"),
                "the enclosing block must render, or the assertion below is vacuous:\n{out}"
            );
            assert_eq!(
                out.contains("held back by a decline"),
                expected,
                "suppressed {suppressed}:\n{out}"
            );
        }
    }

    /// **The plan's own stopping rule, which fires on the loudest sentence this command prints.**
    ///
    /// `injected + injected_file > 0 && cited + cited_after_file == 0` had nine surviving mutants
    /// — every operator in it — because `filled()` has citations, so the block never rendered in
    /// any test. Told as a truth table rather than one case: `&&` -> `||` and `==` -> `!=` each
    /// need a *different* row to redden, and the two sums need a row where a lane is zero, or
    /// `+`, `-` and `*` all agree.
    #[test]
    fn the_stopping_rule_needs_both_something_injected_and_nothing_cited() {
        // (injected, injected_file, cited, cited_after_file, should the sentence appear)
        for (injected, injected_file, cited, cited_after, expected) in [
            (0, 0, 0, 0, false),  // nothing injected: not a verdict, no data
            (0, 5, 0, 0, true),   // one lane injected, nothing cited anywhere: the real case
            (0, 5, 0, 3, false),  // something was cited — through the other lane
            (40, 0, 5, 0, false), // and the ordinary healthy board
        ] {
            let st = Status {
                receipt: Receipt {
                    injected,
                    injected_file,
                    cited,
                    cited_after_file: cited_after,
                    ..Receipt::default()
                },
                ..empty()
            };
            let out = render_status(&st, "all time", &HookState::Installed, 0);
            assert_eq!(
                out.contains("should be switched off"),
                expected,
                "injected {injected}+{injected_file}, cited {cited}+{cited_after}:\n{out}"
            );
        }
    }

    /// **D49's reflex-approval warning needs both halves, and each half needs its own row.**
    ///
    /// `rate == 0.0 && offers() >= 3` is the condition that tells a reader to withdraw the
    /// promotion phase. All three of its mutants survived. `&&` -> `||` fires the warning after a
    /// single approval; `>= ` -> `<` fires it on a board with no offers to speak of. Both say
    /// "approval has become reflex" about boards where it demonstrably has not.
    #[test]
    fn the_reflex_approval_warning_needs_both_halves_of_its_condition() {
        // (promoted, declined, should the warning appear) — offers() is their sum.
        for (promoted, declined, expected) in [
            (3, 0, true),  // three approvals, nothing declined: exactly what D49 watches for
            (2, 1, false), // a decline happened, so approval is not reflex
            (2, 0, false), // never declined, but two offers is not yet a pattern
        ] {
            let st = Status {
                phases: PhaseReceipts {
                    candidates: 1,
                    promoted,
                    declined,
                    ..PhaseReceipts::default()
                },
                ..empty()
            };
            let out = render_status(&st, "all time", &HookState::Installed, 0);
            assert_eq!(
                out.contains("approval has become reflex"),
                expected,
                "{promoted} promoted, {declined} declined:\n{out}"
            );
        }
    }

    /// **D64's per-force number was printed and never read.**
    ///
    /// `cited / injected` had three surviving mutants — `/` -> `%`, `/` -> `*`, and the
    /// zero-guard inverted — because no test asserted the rendered value. Under them the same
    /// board reports `5.00`, `285.00` and `0.00` for a force that converted at `0.09`, and each
    /// is a plausible-looking number beside the ratio D59's floor is read off.
    ///
    /// The zero-injection row is deliberately not fixtured: `receipt()` filters it out, which
    /// `a_force_with_no_events_is_absent_from_the_split_rather_than_present_as_a_zero` is what
    /// makes true. That pairing is the guard — remove the filter and this branch becomes
    /// reachable, with a NaN in it.
    #[test]
    fn a_force_ratio_is_the_quotient_and_is_asserted_as_a_number() {
        let st = Status {
            receipt: Receipt {
                by_force: vec![("advice".into(), 57, 5)],
                ..Receipt::default()
            },
            ..empty()
        };
        let out = render_status(&st, "all time", &HookState::Installed, 0);
        assert!(
            out.contains("as advice  : 5/57 · 0.09"),
            "the per-force ratio is 5/57, and every mutant of it renders a different number:\n{out}"
        );
    }

    /// A list that exactly fits its bound has no remainder to announce.
    ///
    /// `a_truncated_uncovered_list_says_how_many_it_dropped` asserts the sentence appears when it
    /// should; `rest > 0` -> `>= 0` survived it, and prints `… and 0 more` under a complete list.
    /// The same omission shape as the receipt above, in the other renderer in this file.
    #[test]
    fn an_uncovered_list_that_fits_announces_no_remainder() {
        let text = render_coverage(&cov(UNCOVERED_SHOWN, 0, 0, UNCOVERED_SHOWN));
        assert_eq!(text.matches("  · src/f").count(), UNCOVERED_SHOWN, "{text}");
        assert!(
            !text.contains("… and"),
            "a complete list claimed a remainder:\n{text}"
        );
    }
}

//! The ledger: what was injected, what was cited, and the receipt over both.
//!
//! `note_events` carries no foreign key to `notes` on purpose — it records
//! what a *session* was shown, and that stays true after the note is deleted.

use super::*;
use rusqlite::OptionalExtension;

// ── The citation ledger ─────────────────────────────────────────────────────

/// Where an injection came from — `SessionStart` or the `PreToolUse` file lookup.
///
/// **Kept apart for a better reason than the one it was introduced with.** The first version
/// split them because `PreToolUse` `additionalContext` was believed unverified: a partial reading
/// of the hooks reference had returned only `permissionDecision` for that event. Re-checked
/// against the full *Decision control* table on **2026-08-28**, `PreToolUse` lists
/// `additionalContext`, and the page states it is injected "into the system context before the
/// next model call". **The doubt was manufactured by a bad reading, not by evidence** (D42,
/// corrected).
///
/// The split stays because the two measure genuinely different things. `SessionStart` retrieves by
/// *recency* and guesses at relevance; `PreToolUse` retrieves by *path* against a file the agent
/// has just named. Their cite rates are the only evidence available for the plan's open question —
/// *is lexical, path-anchored recall enough for observations?* — and a single merged number
/// answers it for neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `SessionStart` — retrieval by recency, relevance guessed.
    Session,
    /// `PreToolUse` — retrieval by path, against a file the agent has just named.
    File,
}

impl Source {
    fn event(self) -> &'static str {
        match self {
            Source::Session => "injected",
            Source::File => "injected_file",
        }
    }
}

/// Record that these notes were put in front of this session.
///
/// **Written by the read path, which is the whole point.** claude-mem's `observations` table
/// carries a `relevance_count` column added for exactly this purpose; across 80,264 rows every
/// value is zero, because a counter the read path must remember to bump is a counter the read
/// path forgets. Writing the ledger *is* how injection happens here, so it cannot quietly become
/// decorative (D39).
///
/// The primary key makes a re-injection in the same session a no-op, so the denominator counts
/// notes shown to sessions rather than hook invocations.
pub fn record_injected(
    conn: &Connection,
    session: &str,
    ids: &[NoteId],
    at: f64,
    source: Source,
) -> Result<()> {
    record(conn, session, ids, source.event(), at)
}

/// Record that this session says a note changed what it did.
pub fn record_cited(conn: &Connection, session: &str, ids: &[NoteId], at: f64) -> Result<()> {
    record(conn, session, ids, "cited", at)
}

fn record(conn: &Connection, session: &str, ids: &[NoteId], event: &str, at: f64) -> Result<()> {
    for id in ids {
        // **Force is copied onto the event, not left to be joined at read time.** A note's force
        // can change; a join would re-attribute every past injection to its current level, and
        // "are rules cited more than advice" would be answered about a history that never
        // happened. Denormalised deliberately (D64). A note missing from the index — possible if
        // the file was removed between injection and citation — records `advice` rather than
        // failing the event, since losing the measurement is worse than mis-filing one row.
        let force: String = conn
            .query_row(
                "SELECT force FROM notes WHERE kind = ?1 AND scope = ?2 AND slug = ?3",
                params![id.kind, id.scope, id.slug],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| ADVICE.to_string());
        conn.execute(
            "INSERT OR IGNORE INTO note_events (session, kind, scope, slug, event, ts, force)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![session, id.kind, id.scope, id.slug, event, at, force],
        )
        .map_err(sql("recording a note event"))?;
    }
    Ok(())
}

/// Phase 1's receipt. **Arithmetic, not impression.**
///
/// The question this whole design turns on is *did anything injected change what a session did?*
/// — and asking an agent about itself is exactly the shape devt's rule forbids ("a completion
/// claim without the id ledger is indistinguishable from a stale state"). So the answer is a
/// division.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Receipt {
    /// Distinct (session, note) pairs shown at `SessionStart` — retrieval by recency.
    pub injected: usize,
    /// Cites for a note this session was shown at `SessionStart`.
    pub cited: usize,
    /// Shown by the `PreToolUse` file lookup — retrieval by path.
    pub injected_file: usize,
    /// Cites for a note this session saw only through the file lookup.
    ///
    /// **Against `injected_file` this is evidence on the plan's open question** — whether
    /// path-anchored recall beats recency for observations, which no borrowed receipt covers.
    /// An earlier version of this comment called it *the first real evidence* and stopped there,
    /// which overstated it in a way that matters: the two lanes do not have the same exposure, so
    /// their ratios are not a like-for-like comparison. See [`Receipt::path_sessions`] and D74.
    pub cited_after_file: usize,
    /// Cites for a note this session was never shown — found through `amb memory recall`, or
    /// remembered from an earlier session. Counted apart so it cannot inflate a ratio it is not
    /// evidence for.
    pub unprompted: usize,
    pub sessions: usize,
    /// Sessions in which the recency lane fired at all.
    pub recency_sessions: usize,
    /// Sessions in which the path lane fired at all.
    ///
    /// **The two lanes have structurally different exposure, and without this the ratios printed
    /// beside them read as a fair comparison.** `SessionStart` fires once per session,
    /// unconditionally. `PreToolUse` fires only on `Read|Edit|Write|NotebookEdit`, so a session
    /// that reads its files through `Bash` raises the recency denominator and contributes nothing
    /// at all to the path one — not a failure of path anchoring, an absence of exposure.
    ///
    /// Measured on the real board, 2026-08-28: 29 recency events across 3 sessions, 8 path events
    /// across **1**. Reading `0/8 · 0.00` beside `4/29 · 0.14` as "path anchoring is losing" was
    /// the mistake this field exists to prevent (D74).
    pub path_sessions: usize,
    /// `(force, injected, cited)`, strongest first. **Read off the events rather than the notes**,
    /// so it reports the force each note actually carried when it was shown (D64).
    pub by_force: Vec<(String, usize, usize)>,
}

impl Receipt {
    /// The sentence that stops the two lane ratios being read as a comparison.
    ///
    /// **`None` when they genuinely are comparable**, so this is silent in the case it has
    /// nothing to add rather than decorating every receipt. A caveat printed unconditionally is
    /// one nobody reads by the third time, and D69 already had to move one *above* its ratio
    /// because a caveat underneath is read after the ratio has been believed.
    pub fn lane_caveat(&self) -> Option<String> {
        if self.injected == 0 && self.injected_file == 0 {
            return None;
        }
        if self.path_sessions >= self.recency_sessions {
            return None;
        }
        Some(format!(
            "the lanes are not directly comparable — recency fired in {} session(s), path in {}. \
             `PreToolUse` fires only on a Read/Edit/Write tool call, so a session that reads files \
             through Bash raises the first denominator and not the second (D74)",
            self.recency_sessions, self.path_sessions
        ))
    }

    /// **Whether the window is filling — the state `TooEarly` cannot express** (D95).
    ///
    /// `verdict: too early — needs 30 more session(s)` reads as progress, and prints identically
    /// whether twenty-nine sessions are on their way or none can ever arrive. On this machine the
    /// second is the live case, and D89's rule is exactly this shape one level up: an instrument
    /// that writes nothing on its unhappy path reports a mechanism that *cannot* arrive as one
    /// that is merely early.
    ///
    /// **What makes arrival the right word.** `note_events` is keyed
    /// `(session, kind, scope, slug, event)`, so a session injected before the window opened
    /// writes no row when it is re-injected. Sessions here are resumed rather than started — zero
    /// new transcripts in two days against sixteen active — so the roster counts activity while
    /// the floor can only consume arrivals, and the two had diverged to sixteen against zero
    /// (M24).
    ///
    /// Silent once the floor is met, and silent with no window open, where "arrival" means
    /// nothing. Elapsed time is deliberately not restated: `counting over the window opened 10h
    /// ago` is already printed directly above, and a second duration renderer is the drift
    /// [`counting_window`] documents.
    pub fn arrival_note(&self, window: Option<f64>) -> Option<String> {
        window?;
        if self.sessions >= VERDICT_MIN_SESSIONS {
            return None;
        }
        Some(if self.sessions == 0 {
            format!(
                "! no session has entered this window at all. A session injected before it \
                 opened cannot enter one — re-injection writes no row — so D59's floor of {} is \
                 unreachable here rather than unreached, until a session starts fresh",
                VERDICT_MIN_SESSIONS
            )
        } else {
            format!(
                "{} of {} session(s) have entered this window — the floor counts arrivals, \
                 and a resumed session is not one",
                self.sessions, VERDICT_MIN_SESSIONS
            )
        })
    }
}

/// How many sessions and injections the injection verdict needs before it means anything (D59).
pub const VERDICT_MIN_SESSIONS: usize = 30;
pub const VERDICT_MIN_INJECTED: usize = 50;
/// Which retrieval lane a search used.
///
/// Kept apart because they answer different questions and have different exposure — the same
/// reason `recency_sessions` and `path_sessions` are separate fields (D42, D74). `TEXT` is a
/// query an agent chose to write; `PATH` is `--file`, which the tool suggests; `ACROSS` is the
/// cross-repo differentiator Q10 turns on.
pub const LANE_TEXT: &str = "text";
pub const LANE_PATH: &str = "path";
pub const LANE_ACROSS: &str = "across";

/// Record that recall ran, and whether it answered.
///
/// **One row per search, never per note.** A search that matched nothing has no note to key on,
/// and that is the reason this is not an `event` in `note_events`: that table's primary key
/// deduplicates per `(session, note, event)`, so a session searching five times would record
/// once. The cost is paid per search, so the denominator rises per search.
pub fn record_search(
    conn: &Connection,
    session: &str,
    lane: &str,
    found: &[IndexedNote],
    home: &str,
    at: f64,
) -> Result<()> {
    // Counted here rather than at the call site, so every lane answers the cross-repo question
    // the same way and none can forget to.
    let foreign = found.iter().filter(|n| n.id.scope != home).count();
    conn.execute(
        "INSERT INTO searches (session, ts, lane, hits, foreign_hits)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![session, at, lane, found.len() as i64, foreign as i64],
    )
    .map_err(sql("recording a search"))?;
    Ok(())
}

/// How often recall was reached for, and how often it answered, over one window.
#[derive(Debug, Clone, Default)]
pub struct Searches {
    /// Every search in the window. **Times the cost was paid**, not distinct queries.
    pub ran: usize,
    /// Searches that returned at least one note.
    pub answered: usize,
    /// Sessions that searched at all — the exposure behind `ran`.
    pub sessions: usize,
    /// Searches that returned at least one note from another repository.
    ///
    /// **Q10's differentiator, counted where it fires.** Not `cross_repo_query`, which counts one
    /// undocumented flag while `concerning` — the documented `--file` path — returns foreign
    /// notes without touching it.
    pub crossed: usize,
}

impl Searches {
    /// The sentence that stops `unprompted: 0` being read as a verdict on its own.
    ///
    /// **`0 · 0` and `0 · 12` are opposite findings and used to print identically.** D59 retires
    /// the injection layer partly on "nothing ever reached for unprompted"; a session that never
    /// searched and a session whose every search missed are the same zero without this line.
    pub fn note(&self, unprompted: usize) -> String {
        match (self.ran, self.answered, unprompted) {
            (0, _, _) => "  recall: never run in this window — an unprompted zero says nothing yet"
                .to_string(),
            (ran, 0, _) => format!(
                "  recall: run {ran} time(s) across {} session(s), answered none — \
                 retrieval is failing, not unwanted",
                self.sessions
            ),
            (ran, ans, 0) => format!(
                "  recall: run {ran} time(s) across {} session(s), {ans} answered — \
                 notes were found and none was cited",
                self.sessions
            ),
            (ran, ans, _) => format!(
                "  recall: run {ran} time(s) across {} session(s), {ans} answered",
                self.sessions
            ),
        }
    }
}

impl Searches {
    /// What Q10 actually asks, from the number that answers it.
    ///
    /// **The verdict moved off the flag and onto the event.** `status` used to print
    /// "cross-repo query run 0 time(s) — if that holds, the differentiator is dead weight" from
    /// `cross_repo_query`, which only `recall --file --across-repos` bumps. That flag is in no
    /// README, no primer and no banner, and `across_repos` merely re-sorts `concerning` — so the
    /// capability fired through the documented path and the counter never saw it. A zero from a
    /// mechanism nobody can reach is D58's shape, and reading it as a verdict is the mistake
    /// `OPEN-QUESTIONS.md` has already recorded twice on this exact question.
    pub fn crossed_note(&self) -> String {
        match (self.ran, self.crossed) {
            (0, _) => "  cross-repo: no search to observe yet".to_string(),
            (ran, 0) => format!(
                "  cross-repo: 0 of {ran} search(es) returned a note from another repository"
            ),
            (ran, c) => format!(
                "  cross-repo: {c} of {ran} search(es) returned a note from another repository"
            ),
        }
    }
}

/// Count searches over the same window the receipt uses.
pub fn searches(conn: &Connection, since: Option<f64>) -> Result<Searches> {
    let floor = since.unwrap_or(0.0);
    let one = |q: &str| -> Result<usize> {
        conn.query_row(q, params![floor], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .map_err(sql("counting searches"))
    };
    Ok(Searches {
        ran: one("SELECT count(*) FROM searches WHERE ts >= ?1")?,
        answered: one("SELECT count(*) FROM searches WHERE hits > 0 AND ts >= ?1")?,
        sessions: one("SELECT count(DISTINCT session) FROM searches WHERE ts >= ?1")?,
        crossed: one("SELECT count(*) FROM searches WHERE foreign_hits > 0 AND ts >= ?1")?,
    })
}

/// Below this cited ratio, with nothing ever reached for unprompted, the layer is withdrawn (D59).
pub const VERDICT_FLOOR: f64 = 0.10;

/// D59's verdict on whether injection is earning its permanent tax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough sessions or injections yet. Carries what is still needed, so the condition is
    /// visible *before* it can fire rather than only when it does.
    TooEarly { sessions: usize, injected: usize },
    /// The sample is there and injection is converting.
    Earning,
    /// The sample is there, almost nothing is cited, and nothing is ever reached for unprompted.
    Withdraw,
    /// Poor conversion, but notes *are* reached for unprompted — a retrieval problem, not a
    /// worthless corpus. Explicitly not a withdrawal, because the two have different fixes.
    RetrievalSuspect,
    /// **The memory hooks are not installed, so there is no verdict to give.** Every other arm
    /// reads a ratio as evidence about the corpus; this one refuses to, because the ratio is not
    /// about the corpus at all. Carries the missing events so the reader is told what to fix
    /// rather than only that something is wrong.
    NotRunning { missing: Vec<String> },
}

impl Verdict {
    /// A stable token for `--json`. **The machine surface must be able to reach this**: `--json`
    /// is what an agent is told to use, and a consumer that gets counts without a verdict computes
    /// its own ratio and makes D59's mistake unaided. The text surface was corrected first and
    /// this one was left behind for a commit, which is the same asymmetry in miniature.
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::TooEarly { .. } => "too_early",
            Verdict::Earning => "earning",
            Verdict::Withdraw => "withdraw",
            Verdict::RetrievalSuspect => "retrieval_suspect",
            Verdict::NotRunning { .. } => "not_running",
        }
    }
}

impl Receipt {
    /// D59's standing verdict on the injection layer itself.
    ///
    /// **Evaluated rather than merely written down.** D54 recorded a withdrawal condition that
    /// nothing could check, and D58 named that shape; a condition stated in prose and computed
    /// nowhere would be the next instance of it.
    pub fn verdict(&self, hooks: &crate::hooks::HookState) -> Verdict {
        // **Asked before the numbers, because the numbers cannot answer it.** A flat zero means
        // "the corpus is not worth injecting" only if injection ran. When it did not, the same
        // zero means nothing, and the arms below would read it as the strongest possible
        // evidence for withdrawal. `Unknown` is deliberately allowed through to the numeric arms
        // rather than blocking them: an unreadable settings file is not proof the layer is off,
        // and refusing every verdict on that basis would be its own kind of wrong.
        if let crate::hooks::HookState::Incomplete { missing, .. } = hooks {
            return Verdict::NotRunning {
                missing: missing.clone(),
            };
        }
        let injected = self.injected + self.injected_file;
        if self.sessions < VERDICT_MIN_SESSIONS || injected < VERDICT_MIN_INJECTED {
            return Verdict::TooEarly {
                sessions: VERDICT_MIN_SESSIONS.saturating_sub(self.sessions),
                injected: VERDICT_MIN_INJECTED.saturating_sub(injected),
            };
        }
        if self.ratio() >= VERDICT_FLOOR {
            return Verdict::Earning;
        }
        // The distinction that keeps this from firing on the wrong fault: a note reached for
        // without being shown is wanted, and a low ratio beside it means retrieval is putting the
        // wrong ones forward — which is fixed, not withdrawn.
        if self.unprompted > 0 {
            Verdict::RetrievalSuspect
        } else {
            Verdict::Withdraw
        }
    }

    /// **Phase 1's receipt**: of everything put in front of a session, how much was used.
    ///
    /// Both paths, because both are injected — the earlier version divided only by the
    /// `SessionStart` count while `PreToolUse` was thought unverified, which understated the
    /// denominator and flattered the result.
    pub fn ratio(&self) -> f64 {
        Self::div(
            self.cited + self.cited_after_file,
            self.injected + self.injected_file,
        )
    }

    /// Recency-retrieved notes only.
    pub fn session_ratio(&self) -> f64 {
        Self::div(self.cited, self.injected)
    }

    /// Path-retrieved notes only.
    ///
    /// **Not comparable with [`session_ratio`](Self::session_ratio) without also reading
    /// [`path_sessions`](Self::path_sessions).** An earlier comment here called the two together
    /// "the retrieval comparison"; they are the two halves of it, and the third thing needed to
    /// make it a comparison is how often each lane was exposed at all (D74).
    pub fn file_ratio(&self) -> f64 {
        Self::div(self.cited_after_file, self.injected_file)
    }

    fn div(num: usize, den: usize) -> f64 {
        if den == 0 {
            0.0
        } else {
            num as f64 / den as f64
        }
    }

    /// **Takes the window because the caveats do**, so a surface cannot emit one and omit the
    /// other by construction. `arrival_note` was added to the human path and not to this one —
    /// D87's exact defect, on the other half of the same command, committed by the session that
    /// had just read D87 (M26).
    pub fn to_json(&self, window: Option<f64>) -> serde_json::Value {
        serde_json::json!({
            "ratio": self.ratio(),
            "injected": self.injected,
            "cited": self.cited,
            "session_ratio": self.session_ratio(),
            "injected_file": self.injected_file,
            "cited_after_file": self.cited_after_file,
            "file_ratio": self.file_ratio(),
            "unprompted_cites": self.unprompted,
            "by_force": self.by_force.iter()
                .map(|(f, i, c)| serde_json::json!({"force": f, "injected": i, "cited": c}))
                .collect::<Vec<_>>(),
            "sessions": self.sessions,
            // Both surfaces answer the same question or neither does — D69's correction, applied
            // here in the same commit rather than a follow-up.
            "recency_sessions": self.recency_sessions,
            "path_sessions": self.path_sessions,
            "lane_caveat": self.lane_caveat(),
            "arrival_note": self.arrival_note(window),
        })
    }
}

/// D59's window: the injection layer's measurement, the one whose floor retires a feature.
pub const INJECTION_WINDOW: &str = "injection";

/// What opening a window did. **Three outcomes, not a bool**, because "already open" and "opened"
/// must not print the same sentence — a measurement silently restarted is a measurement lost, and
/// this is the surface where that would happen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowChange {
    Opened,
    AlreadyOpen(f64),
    /// **Only `from` travels.** `to` is the `at` the caller just passed in, and nothing read it —
    /// the value a caller cannot already compute is the start it is discarding.
    Reopened {
        from: f64,
    },
}

/// When a named window opened, or `None` if it never has.
///
/// `None` means *all time* to [`receipt`], which is the honest reading: before anyone starts a
/// measurement there is no window, and pretending otherwise would date one from the board's
/// creation.
pub fn window_start(conn: &Connection, name: &str) -> Result<Option<f64>> {
    conn.query_row(
        "SELECT opened_at FROM measurement_window WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )
    .optional()
    .map_err(sql("reading the measurement window"))
}

/// Start a window, or say why it did not start.
///
/// **Reopening is possible and never accidental.** A window that could be silently reset would
/// let a bad run be retried until it read well, which is the failure the receipt exists to
/// prevent; a window that could never be reset would strand the measurement the first time the
/// instrument needed fixing — which is exactly the position D86 left this one in.
pub fn window_open(conn: &Connection, name: &str, at: f64, reopen: bool) -> Result<WindowChange> {
    if let Some(from) = window_start(conn, name)? {
        if !reopen {
            return Ok(WindowChange::AlreadyOpen(from));
        }
        conn.execute(
            "UPDATE measurement_window SET opened_at = ?2 WHERE name = ?1",
            params![name, at],
        )
        .map_err(sql("reopening the measurement window"))?;
        return Ok(WindowChange::Reopened { from });
    }
    conn.execute(
        "INSERT INTO measurement_window (name, opened_at) VALUES (?1, ?2)",
        params![name, at],
    )
    .map_err(sql("opening the measurement window"))?;
    Ok(WindowChange::Opened)
}

/// Which corpus `amb memory status` counts over, and the phrase that says so.
///
/// **Pure, and in the library, because this is the decision D87 turns on** — and because a
/// four-line `match` on a hook-adjacent argument is precisely the thing D78 found had drifted
/// into `main.rs` three times and gone untested each time. If the last arm silently became
/// `None`, every printed ratio would quietly revert to all-time and no test would notice; that
/// is the failure D87 exists to end, so it is guarded rather than merely written correctly.
///
/// Precedence is explicit-beats-default: `--days` is someone asking a different question and
/// wins, `--all-time` says so outright, and otherwise the open window decides. No window means
/// all time, which is the honest reading rather than a date invented from the board's creation.
pub fn counting_window(
    days: Option<u32>,
    all_time: bool,
    open: Option<f64>,
    now: f64,
) -> (Option<f64>, String) {
    match (days, all_time) {
        (Some(d), _) => (
            Some(now - f64::from(d) * 86_400.0),
            format!("the last {d} day(s)"),
        ),
        (None, true) => (None, "all time — every event on the board".to_string()),
        (None, false) => match open {
            Some(w) => (
                Some(w),
                // `age`, not `duration::humanise`: every other memory-surface timestamp —
                // `amb memory recall`, `IndexedNote::to_json` — renders through `age`, and the
                // two disagree at both ends ("just now" vs "0s ago", "1y ago" vs "400d ago").
                format!("the window opened {}", age(w, now)),
            ),
            None => (
                None,
                "all time — no measurement window is open; `amb memory window --open` starts one"
                    .to_string(),
            ),
        },
    }
}

/// The receipt over a window, or over all time when `since` is `None`.
pub fn receipt(conn: &Connection, since: Option<f64>) -> Result<Receipt> {
    let floor = since.unwrap_or(0.0);
    let one = |q: &str| -> Result<usize> {
        let n: i64 = conn
            .query_row(q, params![floor], |r| r.get(0))
            .map_err(sql("computing the receipt"))?;
        Ok(n as usize)
    };
    // One shape, four times: "a cite whose session also has an <event> row for the same note".
    // Written out rather than parameterised because the event name is the only thing that varies
    // and a reader should be able to see which count is which without following a substitution.
    let cited_with = |event: &str, exclude: Option<&str>| -> Result<usize> {
        let extra = exclude.map_or(String::new(), |e| {
            format!(
                " AND NOT EXISTS (SELECT 1 FROM note_events x WHERE x.event = '{e}'
                    AND x.session = c.session AND x.kind = c.kind
                    AND x.scope = c.scope AND x.slug = c.slug)"
            )
        });
        let q = format!(
            "SELECT count(*) FROM note_events c
              WHERE c.event = 'cited' AND c.ts >= ?1
                AND EXISTS (SELECT 1 FROM note_events i WHERE i.event = '{event}'
                     AND i.session = c.session AND i.kind = c.kind
                     AND i.scope = c.scope AND i.slug = c.slug){extra}"
        );
        one(&q)
    };

    Ok(Receipt {
        injected: one("SELECT count(*) FROM note_events WHERE event = 'injected' AND ts >= ?1")?,
        // A cite only counts toward the ratio when the same session was shown the note. Anything
        // else answers a different question, and mixing them would let the numerator exceed the
        // denominator.
        cited: cited_with("injected", None)?,
        injected_file: one(
            "SELECT count(*) FROM note_events WHERE event = 'injected_file' AND ts >= ?1",
        )?,
        cited_after_file: cited_with("injected_file", Some("injected"))?,
        unprompted: one("SELECT count(*) FROM note_events c
              WHERE c.event = 'cited' AND c.ts >= ?1
                AND NOT EXISTS (SELECT 1 FROM note_events i
                                 WHERE i.event IN ('injected', 'injected_file')
                                   AND i.session = c.session AND i.kind = c.kind
                                   AND i.scope = c.scope AND i.slug = c.slug)")?,
        sessions: one("SELECT count(DISTINCT session) FROM note_events WHERE ts >= ?1")?,
        recency_sessions: one("SELECT count(DISTINCT session) FROM note_events
              WHERE event = 'injected' AND ts >= ?1")?,
        path_sessions: one("SELECT count(DISTINCT session) FROM note_events
              WHERE event = 'injected_file' AND ts >= ?1")?,
        by_force: {
            let mut v = Vec::new();
            for f in FORCES {
                let injected = one(&format!(
                    "SELECT count(*) FROM note_events WHERE ts >= ?1 AND force = '{f}'
                       AND event IN ('injected', 'injected_file')"
                ))?;
                let cited = one(&format!(
                    "SELECT count(*) FROM note_events c WHERE c.ts >= ?1 AND c.force = '{f}'
                       AND c.event = 'cited'
                       AND EXISTS (SELECT 1 FROM note_events i
                                    WHERE i.event IN ('injected', 'injected_file')
                                      AND i.session = c.session AND i.kind = c.kind
                                      AND i.scope = c.scope AND i.slug = c.slug)"
                ))?;
                if injected > 0 || cited > 0 {
                    v.push((f.to_string(), injected, cited));
                }
            }
            v
        },
    })
}

/// `amb memory window`'s report, with no flags.
///
/// **The absent-window sentence is long on purpose.** "No window is open" and "the window opened
/// 3d ago" have opposite consequences for every ratio `status` prints, and the reader who most
/// needs telling is the one who never opened one — so the no-window case says what `status` is
/// actually counting, not merely that a row is missing (D87).
pub fn render_window_report(opened: Option<f64>, at: f64) -> String {
    match opened {
        Some(w) => format!(
            "the injection window opened {} — `amb memory status` counts from there\n",
            age(w, at)
        ),
        None => "no injection window is open. `amb memory status` counts every event on the \
                 board, which includes anything recorded before the layer was in the shape you \
                 mean to measure. `--open` starts one from now\n"
            .into(),
    }
}

/// The report as JSON — the same two facts as [`render_window_report`], beside it because one
/// command's two formats drift when they live in two files (M26; `WindowChange::to_json` below
/// is the same rule for the `--open` arm).
pub fn window_report_json(opened: Option<f64>) -> serde_json::Value {
    serde_json::json!({ "open": opened.is_some(), "since": opened })
}

/// What [`window_open`] did, as text.
///
/// **`AlreadyOpen` must not read like `Opened`, and that is the whole reason the enum has three
/// variants.** A window that resets by re-running the command is one that can be retried until it
/// reads well; the refusal is the feature, and `--reopen` is the deliberate spelling (D87).
pub fn render_window_change(change: &WindowChange, at: f64) -> String {
    match change {
        WindowChange::Opened => {
            "injection window opened. D59's floor now reads only events from now on\n".into()
        }
        WindowChange::AlreadyOpen(w) => format!(
            "already open, {} — nothing changed. `--reopen` restarts it and discards what it \
             has measured\n",
            age(*w, at)
        ),
        WindowChange::Reopened { from } => format!(
            "injection window restarted. Its previous start was {}, and everything it had \
             measured is discarded\n",
            age(*from, at)
        ),
    }
}

impl WindowChange {
    /// The change as JSON. `changed` is D87's distinction surviving the format — `AlreadyOpen`
    /// must not read like `Opened` here any more than it may in prose. Beside the prose
    /// renderer, because `Counts::to_json` above carries M26: the last time one command's two
    /// formats were maintained in two places, the human path was updated and this one was not.
    pub fn to_json(&self) -> serde_json::Value {
        let (changed, previous) = match self {
            WindowChange::Opened => (true, None),
            WindowChange::AlreadyOpen(w) => (false, Some(*w)),
            WindowChange::Reopened { from } => (true, Some(*from)),
        };
        serde_json::json!({ "open": true, "changed": changed, "previous_start": previous })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All three window outcomes stay distinct in JSON — D87's rule surviving the format.
    ///
    /// A truth table with presence rows (M27): `changed` is what separates `Opened` from
    /// `AlreadyOpen`, and `previous_start` is what makes `Reopened` accountable for what it
    /// discarded.
    #[test]
    fn every_window_change_is_distinct_in_json() {
        let opened = WindowChange::Opened.to_json();
        assert_eq!(opened["changed"], serde_json::Value::Bool(true), "{opened}");
        assert!(opened["previous_start"].is_null(), "{opened}");

        let already = WindowChange::AlreadyOpen(7.0).to_json();
        assert_eq!(
            already["changed"],
            serde_json::Value::Bool(false),
            "{already}"
        );
        assert_eq!(already["previous_start"], 7.0, "{already}");

        let reopened = WindowChange::Reopened { from: 3.0 }.to_json();
        assert_eq!(
            reopened["changed"],
            serde_json::Value::Bool(true),
            "{reopened}"
        );
        assert_eq!(
            reopened["previous_start"], 3.0,
            "what a reopen discarded must be visible: {reopened}"
        );
    }

    /// Open and absent stay distinct in the report's JSON, with a presence row each (M27).
    #[test]
    fn the_report_json_says_open_and_since_in_both_directions() {
        let closed = window_report_json(None);
        assert_eq!(closed["open"], serde_json::Value::Bool(false), "{closed}");
        assert!(closed["since"].is_null(), "{closed}");

        let open = window_report_json(Some(12.5));
        assert_eq!(open["open"], serde_json::Value::Bool(true), "{open}");
        assert_eq!(open["since"], 12.5, "{open}");
    }

    /// **"No window is open" must say what `status` is therefore counting.**
    ///
    /// D87's whole finding is that the default counted everything — including a hand-run probe
    /// that was 14% of the denominator. The reader who most needs telling is the one who never
    /// opened a window, and to them a bare "none" is not an answer.
    #[test]
    fn no_open_window_says_what_status_is_counting_instead() {
        let none = render_window_report(None, 1000.0);
        crate::assert_rendered_shape("render_window_report", &none);
        assert!(none.contains("no injection window is open"), "{none}");
        assert!(
            none.contains("counts every event on the board"),
            "the consequence, not just the state: {none}"
        );
        assert!(
            none.contains("--open"),
            "and the command that fixes it: {none}"
        );

        let open = render_window_report(Some(0.0), 1000.0);
        assert!(open.contains("the injection window opened"), "{open}");
        assert!(!open.contains("--open"), "nothing to fix: {open}");
    }

    /// **`AlreadyOpen` must not read like `Opened`.**
    ///
    /// This is the reason the enum has three variants rather than a bool. A window that resets by
    /// re-running the command is one that can be retried until it reads well; collapsing these
    /// two sentences would make the refusal invisible while leaving it in force, which is worse
    /// than removing it.
    #[test]
    fn opening_a_window_that_is_already_open_refuses_in_words_that_cannot_be_misread() {
        let opened = render_window_change(&WindowChange::Opened, 1000.0);
        crate::assert_rendered_shape("render_window_change", &opened);
        let already = render_window_change(&WindowChange::AlreadyOpen(0.0), 1000.0);
        assert_ne!(opened, already);
        assert!(opened.contains("injection window opened"), "{opened}");
        assert!(already.contains("nothing changed"), "{already}");
        assert!(
            !already.contains("injection window opened"),
            "it must not claim to have done the thing it refused: {already}"
        );
        assert!(
            already.contains("--reopen"),
            "and it names the deliberate spelling: {already}"
        );
    }

    /// A restart says what it destroyed. The previous start is the one thing the caller cannot
    /// recover afterwards, so it travels in the variant and is printed.
    #[test]
    fn a_restart_names_the_start_it_discarded() {
        let out = render_window_change(&WindowChange::Reopened { from: 0.0 }, 1000.0);
        assert!(out.contains("restarted"), "{out}");
        assert!(out.contains("discarded"), "{out}");
        assert!(
            out.contains(&age(0.0, 1000.0)),
            "and dates what it discarded: {out}"
        );
    }

    /// A migrated board on disk. `open_at` runs the ladder, which is what puts
    /// `measurement_window` there — an in-memory `Connection` would have no schema at all.
    fn board() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        (dir, conn)
    }

    /// Every search is a row, because every search is a cost that was paid.
    ///
    /// **This is CLAUDE.md's second question, asserted rather than assumed.** The cheap
    /// implementation was a sentinel row in `note_events`, whose key is
    /// `(session, kind, scope, slug, event)` — five searches in one session would have been one
    /// row, understating the cost while the numerator was untouched, which improves the ratio for
    /// free and is invisible because nothing is broken.
    ///
    /// Deleting the row for the repeated search, or keying this table on anything a repeat would
    /// collide with, reddens here.
    #[test]
    fn a_repeated_search_is_a_second_row_not_the_same_one() {
        let (_d, conn) = board();
        let note = |scope: &str| IndexedNote {
            id: NoteId::observation(scope, "a-slug"),
            title: "t".into(),
            status: ACTIVE.into(),
            created: 0.0,
            vault_path: "p.md".into(),
            excerpt: None,
            paths: Vec::new(),
            force: ADVICE.into(),
        };
        let none: &[IndexedNote] = &[];
        let local = [note("nest")];
        let mixed = [note("nest"), note("elsewhere")];

        let rec = |sess, lane, found: &[IndexedNote], at| {
            super::record_search(&conn, sess, lane, found, "nest", at).expect("recorded")
        };
        rec("sess-a", LANE_TEXT, none, 100.0);
        rec("sess-a", LANE_TEXT, none, 101.0); // the same query again
        rec("sess-a", LANE_PATH, &mixed, 102.0);
        rec("sess-b", LANE_TEXT, &local, 103.0);

        let all = super::searches(&conn, None).expect("counts");
        assert_eq!(all.ran, 4, "one row per search, not per distinct search");
        assert_eq!(all.answered, 2, "two of the four found something");
        assert_eq!(all.sessions, 2);
        assert_eq!(
            all.crossed, 1,
            "only the search that returned another project's note crossed a repository"
        );

        // And the window scopes it, so `status` counts what D87 says it counts.
        let windowed = super::searches(&conn, Some(102.0)).expect("counts");
        assert_eq!(windowed.ran, 2, "searches before the window are outside it");
        assert_eq!(windowed.sessions, 2);
    }

    /// A zero from a lane that never ran and a zero from a lane that ran and missed differ.
    ///
    /// D59 retires the injection layer partly on "nothing ever reached for unprompted". These
    /// two states printed identically before D89, and they are opposite findings.
    #[test]
    fn never_searching_and_always_missing_do_not_read_the_same() {
        let never = Searches::default();
        let missed = Searches {
            ran: 6,
            answered: 0,
            sessions: 2,
            crossed: 0,
        };
        let found = Searches {
            ran: 6,
            answered: 5,
            sessions: 2,
            crossed: 0,
        };
        let (a, b, c) = (never.note(0), missed.note(0), found.note(0));
        assert_ne!(a, b, "never run must not read like run-and-missed");
        assert_ne!(b, c, "missed must not read like answered");
        assert!(a.contains("never run"), "{a}");
        assert!(
            b.contains("retrieval is failing, not unwanted"),
            "a lane that ran and never answered is a retrieval finding: {b}"
        );
        assert!(c.contains("5 answered"), "{c}");
    }

    /// The window is what makes D59's floor mean the thing D79 defined.
    ///
    /// **Opening twice must not restart it.** A measurement that resets by repeating a command is
    /// one that can be retried until it reads well, and this receipt's floor retires a feature.
    #[test]
    fn a_window_opens_once_and_reopens_only_when_told() {
        let (_dir, c) = board();
        assert_eq!(
            window_start(&c, INJECTION_WINDOW).expect("read"),
            None,
            "no window until someone opens one — never dated from the board's creation"
        );

        assert_eq!(
            window_open(&c, INJECTION_WINDOW, 100.0, false).expect("open"),
            WindowChange::Opened
        );
        assert_eq!(
            window_start(&c, INJECTION_WINDOW).expect("read"),
            Some(100.0)
        );

        assert_eq!(
            window_open(&c, INJECTION_WINDOW, 200.0, false).expect("second open"),
            WindowChange::AlreadyOpen(100.0),
            "an accidental second open must not move the start"
        );
        assert_eq!(
            window_start(&c, INJECTION_WINDOW).expect("read"),
            Some(100.0),
            "and must not have moved it as a side effect either"
        );

        assert_eq!(
            window_open(&c, INJECTION_WINDOW, 300.0, true).expect("reopen"),
            WindowChange::Reopened { from: 100.0 }
        );
        assert_eq!(
            window_start(&c, INJECTION_WINDOW).expect("read"),
            Some(300.0)
        );
    }

    /// The default is the open window, and that is the arm with no other guard.
    ///
    /// **Each arm asserted separately, because the bug this prevents is one arm changing.** The
    /// dangerous mutation is the last one silently returning `None`: every ratio would revert to
    /// all-time, print a plausible number, and read as though D79's window were being honoured.
    #[test]
    fn status_counts_over_the_open_window_unless_told_otherwise() {
        let now = 1_000_000.0;
        let opened = now - 3_600.0;

        let (since, said) = counting_window(None, false, Some(opened), now);
        assert_eq!(since, Some(opened), "the open window is the default");
        assert!(said.contains("window opened"), "{said}");

        let (since, said) = counting_window(None, false, None, now);
        assert_eq!(since, None, "no window means all time, not a made-up date");
        assert!(said.contains("no measurement window is open"), "{said}");

        let (since, said) = counting_window(None, true, Some(opened), now);
        assert_eq!(since, None, "--all-time overrides an open window");
        assert!(said.contains("every event"), "{said}");

        let (since, said) = counting_window(Some(2), false, Some(opened), now);
        assert_eq!(
            since,
            Some(now - 2.0 * 86_400.0),
            "--days is a different question and wins"
        );
        assert!(said.contains("2 day(s)"), "{said}");
    }

    /// The window is the difference between two verdicts, not a cosmetic filter.
    ///
    /// Built from the shape that actually occurred: events recorded before the window opened —
    /// a hand-run probe among them — plus events after it. All-time counts both and reads as a
    /// worse ratio than the corpus D79 asked about.
    #[test]
    fn the_window_excludes_what_was_recorded_before_it_opened() {
        let (_dir, c) = board();
        c.execute_batch(
            "INSERT INTO note_events VALUES ('probe','observation','nest','a','injected',10.0,'advice');
             INSERT INTO note_events VALUES ('probe','observation','nest','b','injected',10.0,'advice');
             INSERT INTO note_events VALUES ('real','observation','nest','c','injected',100.0,'advice');
             INSERT INTO note_events VALUES ('real','observation','nest','c','cited',101.0,'advice');",
        )
        .expect("stage a ledger straddling the window");

        let all = super::receipt(&c, None).expect("all time");
        assert_eq!((all.injected, all.cited, all.sessions), (3, 1, 2));

        let windowed = super::receipt(&c, Some(50.0)).expect("windowed");
        assert_eq!(
            (windowed.injected, windowed.cited, windowed.sessions),
            (1, 1, 1),
            "the probe's injections are outside the window and must not be in the denominator"
        );
        assert!(
            windowed.ratio() > all.ratio(),
            "the window is load-bearing: all-time {:.2} vs windowed {:.2}",
            all.ratio(),
            windowed.ratio()
        );
    }

    fn receipt(sessions: usize, injected: usize, cited: usize, unprompted: usize) -> Receipt {
        Receipt {
            injected,
            cited,
            injected_file: 0,
            cited_after_file: 0,
            unprompted,
            sessions,
            // Equal by default so the D74 lane caveat stays silent in tests that are about
            // something else. The asymmetry has its own tests below.
            recency_sessions: sessions,
            path_sessions: sessions,
            by_force: Vec::new(),
        }
    }
    /// The caveat fires exactly when the two lanes had different exposure.
    ///
    /// **This is the finding it exists for, as data.** On the real board it read `0/8 · 0.00` by
    /// path against `4/29 · 0.14` by recency, which invites the conclusion that path anchoring is
    /// losing. All eight path events came from one session and all 29 recency events from three:
    /// `PreToolUse` fires only on a Read/Edit/Write tool call, so a session reading files through
    /// `Bash` contributes to one denominator and not the other.
    #[test]
    fn the_lane_caveat_fires_only_when_exposure_actually_differs() {
        let mut r = receipt(3, 29, 4, 0);
        r.injected_file = 8;
        r.recency_sessions = 3;
        r.path_sessions = 1;
        let caveat = r
            .lane_caveat()
            .expect("differing exposure must be declared");
        assert!(caveat.contains("recency fired in 3 session(s), path in 1"));

        // Equal exposure is a real comparison and must say nothing.
        r.path_sessions = 3;
        assert_eq!(r.lane_caveat(), None);

        // More path exposure than recency is not a warning either — the caveat is about the
        // denominator being understated, not about the lanes differing in any direction.
        r.path_sessions = 5;
        assert_eq!(r.lane_caveat(), None);
    }
    /// Nothing injected at all is not an exposure problem; it is an empty receipt.
    #[test]
    fn an_empty_receipt_carries_no_lane_caveat() {
        let mut r = receipt(0, 0, 0, 0);
        r.recency_sessions = 0;
        r.path_sessions = 0;
        assert_eq!(r.lane_caveat(), None);

        // A receipt this shape cannot arise from the production query — a lane with no
        // injections has no sessions either, and the equivalence test below owns that invariant
        // — so this row is not a behavior claim. It pins the all-zero gate as the *first*
        // decision: a receipt that somehow violates the invariant fails safe to silence rather
        // than printing a caveat about lanes that never fired. M52's one survivor lived here:
        // flipping the gate's second `==` was equivalent everywhere the invariant holds, and
        // only an impossible receipt can see the gate at all (the kept-vacuous-needle rule,
        // with the vacancy spelled out).
        r.recency_sessions = 2;
        r.path_sessions = 1;
        assert_eq!(
            r.lane_caveat(),
            None,
            "an inconsistent receipt stays silent, never alarming"
        );
    }
    /// D59's floor must not fire on a small sample — the failure that would retire the layer for
    /// a run of quiet afternoons rather than for a real absence of value.
    #[test]
    fn the_injection_verdict_waits_for_a_sample() {
        assert!(matches!(
            receipt(2, 7, 0, 0).verdict(&crate::hooks::HookState::Installed),
            Verdict::TooEarly { .. }
        ));
        // Sessions alone are not enough, and neither are injections alone.
        assert!(matches!(
            receipt(VERDICT_MIN_SESSIONS, 3, 0, 0).verdict(&crate::hooks::HookState::Installed),
            Verdict::TooEarly { .. }
        ));
        assert!(matches!(
            receipt(3, VERDICT_MIN_INJECTED, 0, 0).verdict(&crate::hooks::HookState::Installed),
            Verdict::TooEarly { .. }
        ));
    }
    /// **The exact receipt that would have withdrawn the layer must refuse to, when the layer
    /// was never running.**
    ///
    /// This is the defect written down as a test. `install --memory` describes the complete
    /// desired hook state, so a later `amb install` for a mode change removed all three memory
    /// entries — correctly, and printing every removal. Nobody was reading. The ratio then sat at
    /// zero for weeks, and D59 reads a sustained zero with nothing reached for unprompted as its
    /// strongest evidence to withdraw. It is the weakest: a negative result from an uninstalled
    /// feature is indistinguishable from a negative result.
    ///
    /// D54's own argument, turned back on the mechanism D54 produced — a condition that cannot
    /// tell "not working" from "not running" is not a condition.
    #[test]
    fn a_layer_that_never_ran_gets_no_verdict_however_bad_the_numbers_look() {
        let n = VERDICT_MIN_SESSIONS;
        let i = VERDICT_MIN_INJECTED;
        let damning = receipt(n, i, 1, 0);

        // Same receipt, both ways. Installed, this is D59 firing.
        assert_eq!(
            damning.verdict(&crate::hooks::HookState::Installed),
            Verdict::Withdraw,
            "the numbers really are bad — that is what makes the other half matter"
        );

        let missing = vec!["SessionStart".to_string(), "PreToolUse".to_string()];
        assert_eq!(
            damning.verdict(&crate::hooks::HookState::Incomplete {
                missing: missing.clone(),
                total: 3,
            }),
            Verdict::NotRunning {
                missing: missing.clone()
            },
            "identical numbers must not withdraw a layer that was switched off"
        );

        // `Unknown` is not `Absent`. An unreadable settings file is not proof the layer is off,
        // and suppressing every verdict on that basis would be its own confidently wrong reading.
        assert_eq!(
            damning.verdict(&crate::hooks::HookState::Unknown),
            Verdict::Withdraw,
            "an unverifiable hook state must not silently suspend D59"
        );
    }
    /// The distinction the condition exists to draw: a corpus nobody wants, versus one whose
    /// retrieval is putting the wrong notes forward. Same low ratio, different fix.
    #[test]
    fn a_low_ratio_only_withdraws_when_nothing_is_ever_reached_for() {
        let n = VERDICT_MIN_SESSIONS;
        let i = VERDICT_MIN_INJECTED;
        assert_eq!(
            receipt(n, i, 1, 0).verdict(&crate::hooks::HookState::Installed),
            Verdict::Withdraw
        );
        assert_eq!(
            receipt(n, i, 1, 1).verdict(&crate::hooks::HookState::Installed),
            Verdict::RetrievalSuspect,
            "a note reached for unprompted is a note that is wanted"
        );
        assert_eq!(
            receipt(n, i, i / 2, 0).verdict(&crate::hooks::HookState::Installed),
            Verdict::Earning
        );
    }

    /// A receipt with **both lanes actually populated**, which no other test builds.
    ///
    /// `receipt()` zeroes `injected_file` and `cited_after_file` — the obvious skeleton value, and
    /// the reason four mutants survived (M23). Zero is the additive identity, so every
    /// `x + <file field>` in the receipt is indistinguishable from `x - <file field>` in every
    /// existing fixture; `+` -> `*` dies in the same expressions because `x * 0` is not `x`.
    ///
    /// **The zeroed lane is `PreToolUse`** — the same lane D42 had to correct for being left out
    /// of the denominator, and the one D74's caveat protects from misreading. The fixture
    /// reproduced as a default the omission the design has twice had to fix in production.
    fn both_lanes() -> Receipt {
        Receipt {
            injected: 20,
            cited: 5,
            injected_file: 10,
            cited_after_file: 3,
            unprompted: 0,
            sessions: 4,
            recency_sessions: 4,
            path_sessions: 4,
            by_force: Vec::new(),
        }
    }

    /// **Every ratio counts what it says it counts, and the three are not each other.**
    ///
    /// Kills `cited + cited_after_file` -> `-` (369), `session_ratio -> 1.0` (376) and
    /// `file_ratio -> 0.0` (386). Each is a number `amb memory status` prints beside a verdict
    /// that can retire the layer, and none was pinned to a value.
    #[test]
    fn the_three_ratios_each_count_their_own_lane_and_the_whole_counts_both() {
        let r = both_lanes();
        // 5 of 20 by recency, 3 of 10 by path, 8 of 30 overall. Chosen so no two are equal and
        // none is 0 or 1 — a constant cannot satisfy any of them, let alone all three.
        assert!(
            (r.session_ratio() - 0.25).abs() < 1e-9,
            "{}",
            r.session_ratio()
        );
        assert!((r.file_ratio() - 0.30).abs() < 1e-9, "{}", r.file_ratio());
        assert!(
            (r.ratio() - 8.0 / 30.0).abs() < 1e-9,
            "the whole receipt is both numerators over both denominators (D42): {}",
            r.ratio()
        );
        assert!(
            r.session_ratio() < r.ratio() && r.ratio() < r.file_ratio(),
            "and the three are distinct numbers, not one number printed three times"
        );
    }

    /// **D42's rule at its second site.**
    ///
    /// `ratio()` sums both lanes for the denominator and is guarded. `verdict()` sums them again
    /// for the sample-size floor and was not — the shared fixture pins `injected_file` to 0, so
    /// `+` and `-` agree in every existing test (M23). D90's shape: one rule, two sites, one
    /// assertion.
    #[test]
    fn the_sample_floor_counts_both_lanes_and_neither_alone_would_reach_it() {
        let mut r = receipt(VERDICT_MIN_SESSIONS, 30, 0, 0);
        r.injected_file = 25;
        // 30 and 25 are each below the floor; 55 is above it. Were the lanes not summed, this
        // receipt would still be waiting for a sample it already has.
        // A compile-time check, per clippy: if either constant moves past these the
        // fixture stops demonstrating what its name claims, and that should not wait
        // for someone to run the test.
        const { assert!(30 < VERDICT_MIN_INJECTED && 25 < VERDICT_MIN_INJECTED) };
        assert!(
            !matches!(
                r.verdict(&crate::hooks::HookState::Installed),
                Verdict::TooEarly { .. }
            ),
            "both lanes are injections and both count toward the floor (D42)"
        );
    }

    /// **The purest form of D74, and the fixture could not express it.**
    ///
    /// `the_lane_caveat_fires_only_when_exposure_actually_differs` sets *both* lanes non-zero, so
    /// both halves of the first guard are false and no mutation of it changes the answer. The
    /// discriminating case is the path lane never firing at all: `by path 0/0` printed beside
    /// `by recency 4/29` is D74's misreading with nothing whatever on one side, which is when the
    /// caveat matters most. Kills `&&` -> `||` and `injected == 0` -> `!= 0` (147).
    #[test]
    fn a_lane_that_never_fired_is_the_case_the_caveat_most_needs_to_cover() {
        let mut r = receipt(3, 29, 4, 0);
        r.injected_file = 0;
        r.recency_sessions = 3;
        r.path_sessions = 0;
        let caveat = r
            .lane_caveat()
            .expect("a lane with no exposure at all must be declared, not left to read as 0/0");
        assert!(
            caveat.contains("recency fired in 3 session(s), path in 0"),
            "{caveat}"
        );
    }

    /// **Q10's line, and the same rule `never_searching_and_always_missing_do_not_read_the_same`
    /// asserts one method over.**
    ///
    /// D91 moved the cross-repo verdict off `--across-repos` — a flag in no README, no primer and
    /// no banner — and onto the event. That fix had no assertion: replacing the whole of
    /// `crossed_note` with an empty string survived the suite (M23). A number nothing writes when
    /// the mechanism fails is D89's defect; a sentence that vanishes silently is the same defect
    /// one layer out.
    #[test]
    fn never_searching_and_never_crossing_do_not_read_the_same_either() {
        let none = Searches {
            ran: 0,
            answered: 0,
            sessions: 0,
            crossed: 0,
        };
        let missed = Searches {
            ran: 5,
            answered: 3,
            sessions: 2,
            crossed: 0,
        };
        let hit = Searches {
            ran: 5,
            answered: 3,
            sessions: 2,
            crossed: 2,
        };

        assert!(none.crossed_note().contains("no search to observe yet"));
        assert!(
            missed.crossed_note().contains("0 of 5"),
            "{}",
            missed.crossed_note()
        );
        assert!(
            hit.crossed_note().contains("2 of 5"),
            "{}",
            hit.crossed_note()
        );
        // Stated as inequalities rather than three strings: the zero that means "nobody asked"
        // must not render as the zero that means "asked and missed".
        assert_ne!(none.crossed_note(), missed.crossed_note());
        assert_ne!(missed.crossed_note(), hit.crossed_note());
    }

    /// **A force with nothing to report is left out, and only an absence can assert that.**
    ///
    /// `by_force` exists so "are rules cited more than advice" is answerable, and `FORCES` has
    /// three members while most boards exercise one. The filter keeping 0/0 rows out is
    /// `injected > 0 || cited > 0`, and all four of its mutants survived (M23). The only
    /// assertion on `by_force` anywhere was `forces.contains("rule")` in `memory_e2e.rs`, which
    /// is true whichever way the filter goes — a positive assertion cannot guard a filter whose
    /// job is an omission.
    ///
    /// **Both clauses have to be load-bearing in the fixture, and the second one is the subtle
    /// half.** A force with citations but no injections is reachable: the `cited` query filters
    /// on `force` while its `EXISTS` does not, so a note injected as `advice` and cited after its
    /// force became `rule` counts `rule` 0/1. Without such a row, `cited > 0` never decides
    /// anything and `cited < 0` — always false on a `usize` — is indistinguishable from it.
    #[test]
    fn a_force_with_no_events_is_absent_from_the_split_rather_than_present_as_a_zero() {
        assert!(
            FORCES.len() > 2,
            "the absence below is vacuous unless a force is left out"
        );
        let (_dir, c) = board();
        c.execute_batch(
            "INSERT INTO note_events VALUES ('s','observation','nest','a','injected',10.0,'advice');
             INSERT INTO note_events VALUES ('s','observation','nest','a','cited',11.0,'rule');",
        )
        .expect("stage one injection and a citation under a different force");

        let r = super::receipt(&c, None).expect("receipt");
        assert_eq!(
            r.by_force,
            vec![("rule".to_string(), 0, 1), ("advice".to_string(), 1, 0)],
            "`rule` is carried by its citation alone and `decision` has nothing at all"
        );
        assert!(
            !r.by_force.iter().any(|(f, _, _)| f == FORCE_DECISION),
            "a force the board never used must not appear as a row of zeroes: {:?}",
            r.by_force
        );
    }

    /// **The premise of an equivalent-mutant claim, pinned so the claim cannot rot silently.**
    ///
    /// `lane_caveat`'s first guard has three mutants. Two change the answer on a receipt the
    /// database can actually produce and are killed above. The third — `injected == 0 &&
    /// injected_file != 0` — is equivalent, but only *relative to an invariant*: `injected` is
    /// `count(*)` and `recency_sessions` is `count(DISTINCT session)` over the same rows under
    /// the same predicate, so one is zero exactly when the other is, and the mutant always falls
    /// through to a second guard that returns `None` for the same inputs. Killing it needs a
    /// receipt with `injected == 0` and `recency_sessions > 0`, which no query can return.
    ///
    /// **That invariant lives in two SQL strings and nowhere else.** Change either `WHERE` and
    /// the word "equivalent" in `missed.txt` becomes false with nothing going red — a mutation
    /// report is a claim about the code, and a claim needs its premise asserted like any other.
    #[test]
    fn a_lane_with_no_injections_has_no_sessions_either_and_that_is_the_equivalence() {
        // Only the path lane fires: the recency lane is the empty one.
        let (_dir, c) = board();
        c.execute_batch(
            "INSERT INTO note_events VALUES ('s1','observation','nest','a','injected_file',10.0,'advice');
             INSERT INTO note_events VALUES ('s2','observation','nest','b','injected_file',11.0,'advice');",
        )
        .expect("stage one lane");
        let r = super::receipt(&c, None).expect("receipt");
        assert_eq!((r.injected, r.recency_sessions), (0, 0));
        assert_eq!((r.injected_file, r.path_sessions), (2, 2));

        // And with the lanes swapped, where the two counts differ in value while still agreeing
        // on zero — which is the half that makes this an invariant rather than a coincidence.
        let (_dir2, c2) = board();
        c2.execute_batch(
            "INSERT INTO note_events VALUES ('s1','observation','nest','a','injected',10.0,'advice');
             INSERT INTO note_events VALUES ('s1','observation','nest','b','injected',11.0,'advice');",
        )
        .expect("stage the other lane");
        let r2 = super::receipt(&c2, None).expect("receipt");
        assert_eq!(
            (r2.injected, r2.recency_sessions),
            (2, 1),
            "count(*) and count(DISTINCT session) differ in value and agree on zero"
        );
        assert_eq!((r2.injected_file, r2.path_sessions), (0, 0));
    }

    /// **The fourth state, and the two zeroes that used to read the same** (D95).
    ///
    /// `TooEarly` says "needs 30 more session(s)" whether twenty-nine are coming or none can. The
    /// distinction is not cosmetic here: `note_events` is keyed
    /// `(session, kind, scope, slug, event)`, so a session injected before the window opened
    /// writes no row when re-injected, and this machine starts no new sessions for days at a time
    /// (M24). Same shape as D89 one level up — an instrument silent on its unhappy path reports
    /// "cannot arrive" as "not yet".
    #[test]
    fn a_window_nothing_can_enter_does_not_read_like_one_that_is_filling() {
        let empty = receipt(0, 0, 0, 0);
        let filling = receipt(3, 40, 5, 0);

        let stalled = empty
            .arrival_note(Some(1.0))
            .expect("zero arrivals is the case this exists for");
        let moving = filling
            .arrival_note(Some(1.0))
            .expect("a floor not yet met still needs its rate shown");
        assert_ne!(
            stalled, moving,
            "'nobody arrived' and 'three arrived' must not print the same sentence"
        );
        assert!(
            stalled.contains("unreachable here rather than unreached"),
            "{stalled}"
        );
        // **Both sentences wrap in the source, and a continuation that keeps its indentation
        // renders a run of spaces mid-sentence.** That shipped once here and every `contains`
        // above still passed, because each needle happened to sit on one side of the damage.
        // Asserting the class rather than the instance: a rendered line has no double space.
        for s in [&stalled, &moving] {
            assert!(
                !s.contains("  "),
                "a wrapped literal leaked its indentation: {s:?}"
            );
        }
        assert!(moving.contains("3 of 30 session(s)"), "{moving}");
    }

    /// Silent in both cases where it would be noise, because a caveat on every receipt is one
    /// nobody reads by the third time — `lane_caveat`'s rule, applied to its neighbour.
    #[test]
    fn the_arrival_note_is_silent_over_all_time_and_once_the_floor_is_met() {
        assert_eq!(
            receipt(0, 0, 0, 0).arrival_note(None),
            None,
            "over all time there is no window to enter, so arrival means nothing"
        );
        assert_eq!(
            receipt(VERDICT_MIN_SESSIONS, 60, 9, 0).arrival_note(Some(1.0)),
            None,
            "the floor is met; the rate it was reached at is no longer the reader's problem"
        );
        assert!(
            receipt(VERDICT_MIN_SESSIONS - 1, 60, 9, 0)
                .arrival_note(Some(1.0))
                .is_some(),
            "and one short of it still is"
        );
    }
}

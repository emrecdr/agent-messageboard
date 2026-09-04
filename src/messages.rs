//! Sending and reading messages.
//!
//! Direct, broadcast and cross-project addressing are **one query**, not three code paths. That
//! falls out of two schema choices: `to_agent IS NULL` means "everyone in `to_proj`", and read
//! state lives in its own table rather than as a flag on the message — so one broadcast row is
//! consumed independently by each recipient without the sender knowing who they are.

use crate::address::Address;
use crate::db::now;
use crate::error::{Error, Result, sql};
use crate::identity::Identity;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// Where a message is actually going, after the roster has been consulted.
///
/// This is the "decide what to do" value in the functional-core sense: [`resolve_recipient`]
/// turns a human-written [`Address`] into the two nullable columns the schema stores, and
/// [`send`] does nothing but write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    /// A resolved agent **id**, never a display name. `None` means "not addressed to one agent".
    pub agent_id: Option<String>,
    /// `None` means every project.
    pub project: Option<String>,
}

/// Turn an [`Address`] into a [`Recipient`] by consulting the roster.
///
/// Resolution accepts a display name, a short ref (`c0a251`) or a full session id, so an agent
/// can address a peer with whatever `amb agents` showed it. An unknown name is an **error**:
/// accepting a message nobody can receive is the failure mode this whole function exists to
/// prevent.
pub fn resolve_recipient(conn: &Connection, to: &Address, me: &Identity) -> Result<Recipient> {
    let project = to.project(&me.project).map(str::to_string);
    let Some(name) = to.name() else {
        return Ok(Recipient {
            agent_id: None,
            project,
        });
    };
    let scope = project.clone().unwrap_or_else(|| me.project.clone());

    // Exact id first, then short ref, then display name within the project. Ordered most
    // specific to least so an id is never shadowed by someone who took it as a name.
    let found: Option<String> = conn
        .query_row(
            "SELECT id FROM agents
             WHERE id = ?1
                OR (substr(id, 1, 6) = ?1 AND project = ?2)
                OR (name = ?1 AND project = ?2)
             ORDER BY CASE WHEN id = ?1 THEN 0 WHEN substr(id, 1, 6) = ?1 THEN 1 ELSE 2 END
             LIMIT 1",
            params![name, scope],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql("resolving the recipient"))?;

    match found {
        Some(id) => Ok(Recipient {
            agent_id: Some(id),
            project,
        }),
        None => {
            // **The error knows the answer and used to withhold it** (U8). The same name is
            // usually registered one project over — that is what makes it worth typing — and the
            // row that proves it is one query away. D26's `nearest` covers a *typo*; this is the
            // exact name in the wrong scope, which no edit distance can reach.
            let elsewhere: Option<String> = conn
                .query_row(
                    "SELECT project FROM agents WHERE name = ?1 AND project <> ?2
                      ORDER BY last_seen DESC LIMIT 1",
                    params![name, scope],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql("looking for the agent in another project"))?;
            Err(match elsewhere {
                Some(other) => Error::AgentInAnotherProject {
                    name: name.to_string(),
                    project: scope,
                    elsewhere: other,
                },
                None => Error::NoSuchAgent {
                    name: name.to_string(),
                    project: scope,
                },
            })
        }
    }
}

/// A message about to be sent.
#[derive(Debug)]
pub struct Outgoing<'a> {
    pub to: &'a Recipient,
    pub subject: &'a str,
    pub body: &'a str,
    /// A lowercase tag: `note`, `question`, `proposal`, whatever the sender means. Not an enum
    /// — a closed set would need a release to add a kind, and the bus has no opinion about what
    /// it carries — but it *is* a charset (`[a-z0-9_-]`, at most [`MAX_KIND`] chars, D107),
    /// because anything other than `note` is rendered inside the header's brackets and a free
    /// string there would be grammar the sender controls.
    pub kind: &'a str,
    pub thread: Option<&'a str>,
    /// Caller-supplied stable id. Makes a resend idempotent (D6): the same `ext_id` twice
    /// yields one row and the original id back.
    pub ext_id: Option<&'a str>,
}

/// A message as stored.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: i64,
    pub ts: f64,
    pub from_agent: String,
    /// The sender's display name at read time, or `None` if they have since deregistered.
    /// Resolved in the inbox query rather than by a second lookup per row.
    pub from_name: Option<String>,
    pub from_proj: String,
    pub to_agent: Option<String>,
    pub to_proj: Option<String>,
    pub kind: String,
    pub subject: String,
    pub body: String,
    pub thread_id: Option<String>,
    /// Whether *this reader* has acknowledged the message (`amb read`), computed by the inbox
    /// query. `None` from paths with no reader in scope (`get`), where the question has no
    /// answer — U1's finding was that the primary surface hid a distinction the design makes
    /// first-class, and `Option` keeps the paths that cannot know from pretending they do.
    pub read: Option<bool>,
}

impl Message {
    /// The sender as a human should see them: their display name, falling back to the id.
    pub fn sender(&self) -> &str {
        self.from_name.as_deref().unwrap_or(&self.from_agent)
    }

    /// True when this message went to a whole project rather than a named agent.
    pub fn is_broadcast(&self) -> bool {
        self.to_agent.is_none()
    }

    /// True when this message went to every project (`@@`).
    pub fn is_global(&self) -> bool {
        self.to_agent.is_none() && self.to_proj.is_none()
    }

    /// How this message was addressed, for display.
    pub fn scope(&self) -> &'static str {
        match (&self.to_agent, &self.to_proj) {
            (Some(_), _) => "direct",
            (None, Some(_)) => "broadcast",
            (None, None) => "global",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "id": self.id,
            "ts": self.ts,
            "from": self.sender(),
            // **The field a reply is addressed with, because `from` is not one** (U8). A session
            // read `"from":"nestwatch-f04621"`, passed it to `amb send`, and got a refusal: the
            // display name only resolves inside its own project, and the reader is usually
            // somewhere else. Both halves were already in this document, one key apart, and every
            // caller had to know to join them. Always qualified, so it works from anywhere.
            "address": format!("{}@{}", self.sender(), self.from_proj),
            "from_id": self.from_agent,
            "from_project": self.from_proj,
            "to": self.to_agent,
            "to_project": self.to_proj,
            "broadcast": self.is_broadcast(),
            "global": self.is_global(),
            "scope": self.scope(),
            "kind": self.kind,
            "subject": self.subject,
            "body": self.body,
            "thread": self.thread_id,
        });
        // Only when the query could answer it — a `null` here would read as "unread-ish" to a
        // script, and `get()` genuinely does not know.
        if let Some(read) = self.read {
            v["read"] = serde_json::json!(read);
        }
        v
    }
}

/// The column list every `Message` query selects, in the order [`row_to_message`] expects.
///
/// Kept beside the mapping so the two cannot drift: adding a column here without adding it
/// there is a compile error rather than a silently shifted index.
const MESSAGE_COLUMNS: &str = "m.id, m.ts, m.from_agent, a.name, m.from_proj, m.to_agent, \
                               m.to_proj, m.kind, m.subject, m.body, m.thread_id";

/// Build a [`Message`] from a row selecting [`MESSAGE_COLUMNS`].
fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        ts: r.get(1)?,
        from_agent: r.get(2)?,
        from_name: r.get(3)?,
        from_proj: r.get(4)?,
        to_agent: r.get(5)?,
        to_proj: r.get(6)?,
        kind: r.get(7)?,
        subject: r.get(8)?,
        body: r.get(9)?,
        thread_id: r.get(10)?,
        read: None,
    })
}

/// Separator between a sender id and its caller-supplied key in the stored `ext_id`.
///
/// ASCII unit separator: it cannot occur inside a session UUID, and a caller typing one into
/// `--id` is not a case worth designing around.
const EXT_ID_SEP: char = '\u{1f}';

/// Scope a caller's idempotency key to its sender.
///
/// **The column carries a global UNIQUE index, but `--id` advertises per-sender semantics** —
/// *"sending twice with the same one delivers once"*. Two agents independently choosing `task-1`
/// (and natural keys are task-shaped, not agent-shaped) meant the second send returned the
/// *first* agent's message id, wrote nothing, and reported `{"sent":1}`. An accepted message
/// that is never delivered, with no error: D18's failure reached by another route. See D22.
///
/// **Why the key is scoped rather than the index changed.** `ext_id TEXT UNIQUE` is a column
/// constraint, so SQLite implements it as an implicit autoindex that cannot be dropped —
/// replacing it needs a full table rebuild. Verified: dropping `messages` with `foreign_keys=ON`
/// cascade-deletes every row in `reads`, which would mark the whole board unread for every agent
/// and re-inject its entire history into every session. Composing the key costs nothing and
/// risks nothing, and the stored form is never surfaced: no query reads `ext_id` except the
/// duplicate lookup below.
fn scoped_ext_id(from_agent: &str, ext_id: Option<&str>) -> Option<String> {
    ext_id.map(|key| format!("{from_agent}{EXT_ID_SEP}{key}"))
}

/// The longest body `send` accepts, in characters.
///
/// **Refused at the sender, before anything is written**, which is the only place that can tell
/// whoever wrote it what happened — the same shape Claude Code's own cross-session channel uses,
/// at roughly a million characters, for the same reason.
///
/// `MAX_RENDERED` and `QUOTED_MAX` bound what an *injection* spends (D24); nothing bounded what a
/// body could be. A 300,000-character body was accepted, stored, and produced 300,145 bytes from
/// `amb inbox` against 749 from the hook — so the containment was on the renderer the hook uses
/// and not on the field, and the unbounded path was the one an agent is told to run.
///
/// An order of magnitude below the platform's cap, because a board is read with up to
/// [`crate::delivery::MAX_RENDERED`] senders' mail at once and a message here is a coordination
/// note. Something genuinely large belongs in a file the recipient can choose to open.
pub const MAX_BODY: usize = 100_000;

/// A kind is a tag, not a sentence. The charset (`[a-z0-9_-]`) and this cap are what make it
/// safe to render inside the header's brackets (D107); `delivery::scope_kind` enforces the same
/// rule at render time for rows this validation never saw.
pub const MAX_KIND: usize = 20;

/// The subject's cap, two orders below the body's, because a subject is a header line rendered
/// inline on every surface. `QUOTED_MAX` bounds what an injection *renders*; this bounds what
/// the board *stores* — containment on the renderer alone is the exact defect `MAX_BODY`'s doc
/// records for bodies, and a 300 KB subject was accepted until this existed (D106).
pub const MAX_SUBJECT: usize = 500;

/// Send a message. Returns its id.
///
/// Wrapped in `BEGIN IMMEDIATE` so the writer takes the lock up front rather than discovering a
/// conflict at commit time — the configuration under which 17 concurrent processes sent 1,700
/// messages with zero `SQLITE_BUSY` and zero lost (`MEASUREMENTS.md` M1, corrected by M16).
///
/// **A body is stored exactly as written. Redaction is deliberately absent from this path, and
/// that is a decision rather than an oversight (D98).** `redact` runs on the vault because a note
/// is prose *about* work (D37); a message is frequently prose *containing an artefact* — a failing
/// command, a config fragment, a path — and mangling that destroys the only reason it was sent.
/// Measured before deciding, over the 53 bodies (98.3 KB) then on the board: **zero secrets, one
/// removal, and that removal was a false positive** — a scratchpad path, the whole payload of its
/// sentence, deleted while the same path with a filename appended survived twice in the same
/// message (M30). `a_body_is_stored_verbatim_because_the_send_path_does_not_redact` is the guard,
/// and it fails if anyone adds the call.
pub fn send(conn: &mut Connection, me: &Identity, out: &Outgoing<'_>) -> Result<i64> {
    // Before the transaction: a body that will be refused must not have opened one.
    let chars = out.body.chars().count();
    if chars > MAX_BODY {
        return Err(Error::BodyTooLarge {
            chars,
            max: MAX_BODY,
        });
    }
    let subject_chars = out.subject.chars().count();
    if subject_chars > MAX_SUBJECT {
        return Err(Error::FieldTooLarge {
            field: "subject",
            chars: subject_chars,
            max: MAX_SUBJECT,
        });
    }
    let kind_ok = !out.kind.is_empty()
        && out.kind.len() <= MAX_KIND
        && out
            .kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !kind_ok {
        return Err(Error::BadKind {
            input: out.kind.into(),
        });
    }

    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql("opening a write transaction"))?;

    // Scoped to this sender, so one agent's key cannot swallow another's message (D22).
    let key = scoped_ext_id(&me.id, out.ext_id);

    // ON CONFLICT DO NOTHING rather than an upsert: a resend must not overwrite the original,
    // only be recognised as the same message (D6). SQLite treats NULLs as distinct in a UNIQUE
    // index, so messages without an ext_id never collide with each other.
    let changed = tx
        .execute(
            "INSERT INTO messages
               (ext_id, ts, from_agent, from_proj, to_agent, to_proj, kind, subject, body, thread_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(ext_id) DO NOTHING",
            params![
                key,
                now()?,
                me.id,
                me.project,
                out.to.agent_id,
                out.to.project,
                out.kind,
                out.subject,
                out.body,
                out.thread,
            ],
        )
        .map_err(sql("inserting the message"))?;

    let id = if changed == 1 {
        tx.last_insert_rowid()
    } else {
        // A duplicate ext_id. Return the id of the message already there, so the caller sees
        // the same answer it got the first time.
        tx.query_row(
            "SELECT id FROM messages WHERE ext_id = ?1",
            params![key],
            |r| r.get(0),
        )
        .map_err(sql("looking up the existing message for this ext_id"))?
    };

    tx.commit().map_err(sql("committing the send"))?;
    Ok(id)
}

/// How many times one message is placed in front of one agent before it stops being offered.
///
/// The dead-letter threshold D6 asks for. Counted **per recipient** in `reads`, so a broadcast
/// nobody acknowledges backs off for each reader independently rather than vanishing for
/// everyone (D23).
///
/// Ten is chosen against turn-boundary delivery: ten turns is ample opportunity to act, and past
/// it the message is not being missed, it is being declined. Backing off is not deletion —
/// `amb inbox` still shows it, because a log you cannot re-read is not a log.
pub const MAX_OFFERS: i64 = 10;

/// Everything addressed to this agent, or broadcast to its project.
///
/// One query serves all three addressing modes. This is the **explicit** read: it never hides a
/// message, whatever its offer count, because an agent that runs `amb inbox` has asked.
pub fn inbox(conn: &Connection, me: &Identity, unread_only: bool) -> Result<Vec<Message>> {
    select(conn, me, unread_only, None)
}

/// Unread mail this agent has not already been offered [`MAX_OFFERS`] times.
///
/// What a *hook* injects, as distinct from what [`inbox`] shows. The split matters: automatic
/// injection spends context the agent did not ask to spend, so it must back off; an explicit
/// `amb inbox` must not, or a message the agent ignored for a while becomes unrecoverable.
pub fn deliverable(conn: &Connection, me: &Identity) -> Result<Vec<Message>> {
    select(conn, me, true, Some(MAX_OFFERS))
}

/// Mail this agent has never been offered at all.
///
/// What the `PostToolUse` hook injects, and the whole reason mid-turn delivery is affordable.
/// That hook fires after *every* tool call, so offering the same three messages again after each
/// of forty edits would be D24 at forty times the rate. Restricting it to genuinely new mail
/// means each message is delivered mid-turn at most once; `Stop` stays the catch-up sweep (D25).
pub fn undelivered(conn: &Connection, me: &Identity) -> Result<Vec<Message>> {
    select(conn, me, true, Some(1))
}

fn select(
    conn: &Connection,
    me: &Identity,
    unread_only: bool,
    max_offers: Option<i64>,
) -> Result<Vec<Message>> {
    // The 2x2 from schema.sql, as one predicate. Four addressing modes, one query:
    //   to_agent = me                      -> direct, from any project
    //   to_agent IS NULL AND to_proj = mine -> broadcast to my project
    //   to_agent IS NULL AND to_proj IS NULL -> global broadcast (`@@`)
    // The 12th column answers "has this reader acknowledged it" per row, so the inbox can say
    // which part of a list is new (U1). Delivery-side selects carry it too; it costs one index
    // probe against `reads` per row that the unread filter was already paying.
    let sql_text = format!(
        "SELECT {MESSAGE_COLUMNS},
                EXISTS(SELECT 1 FROM reads r
                        WHERE r.msg_id = m.id AND r.agent = ?2 AND r.read_at IS NOT NULL)
        FROM messages m
        LEFT JOIN agents a ON a.id = m.from_agent
        WHERE
          -- A sender does not need telling what it just said. Without this, every broadcast
          -- echoes back to its author as unread mail, which is pure noise in a channel whose
          -- whole value is that unread means 'something you have not seen'.
          m.from_agent <> ?2
          AND (m.to_agent = ?2
               OR (m.to_agent IS NULL AND (m.to_proj IS NULL OR m.to_proj = ?1)))
          AND (?3 = 0 OR NOT EXISTS (
                SELECT 1 FROM reads r
                WHERE r.msg_id = m.id AND r.agent = ?2 AND r.read_at IS NOT NULL))
          -- The back-off, applied only on the delivery path (?4 IS NULL for an explicit read).
          AND (?4 IS NULL OR NOT EXISTS (
                SELECT 1 FROM reads r
                WHERE r.msg_id = m.id AND r.agent = ?2 AND r.attempts >= ?4))
          -- D96's horizon, on the same delivery-only condition and for the same reason. A
          -- **broadcast** past it stops being injected; a message addressed to this agent never
          -- expires, because a question asked of you personally does not stop mattering because
          -- you were away. `?5` is the cutoff instant, not a duration.
          AND (?4 IS NULL OR m.to_agent IS NOT NULL OR m.ts >= ?5)
        ORDER BY m.id"
    );
    let cutoff = crate::db::now()? - broadcast_horizon().as_secs_f64();
    let mut stmt = conn
        .prepare(&sql_text)
        .map_err(sql("preparing the inbox query"))?;
    let rows = stmt
        .query_map(
            params![
                me.project,
                me.id,
                i32::from(unread_only),
                max_offers,
                cutoff
            ],
            |r| {
                let mut m = row_to_message(r)?;
                m.read = Some(r.get(11)?);
                Ok(m)
            },
        )
        .map_err(sql("running the inbox query"))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql("reading an inbox row"))
}

/// Levenshtein distance, for "did you mean" on a mistyped project name.
///
/// Pure, and small enough to keep rather than take a dependency for. Two rows, because only the
/// previous one is ever read.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(cur[j] + 1).min(prev[j + 1] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The closest known name to `typed`, if one is close enough to be worth suggesting.
///
/// Pure so the threshold can be argued about in tests rather than in production. A suggestion
/// that is usually wrong is worse than none: it invites an agent to "correct" a project name
/// that was right all along.
/// **A tie yields no suggestion.** With `api-v1` and `api-v2` both on the board, a typo of
/// `api-v3` is one edit from each, and naming whichever the roster happened to return first is a
/// coin flip presented as help. Silence is the honest answer when the evidence does not choose (D26).
pub fn nearest<'a>(typed: &str, known: &[&'a str]) -> Option<&'a str> {
    let budget = 2.max(typed.chars().count() / 4);
    let mut scored: Vec<(usize, &'a str)> = known
        .iter()
        .map(|k| (distance(typed, k), *k))
        .filter(|(d, _)| *d <= budget)
        .collect();
    scored.sort_by_key(|(d, _)| *d);
    match scored.as_slice() {
        [(_, only)] => Some(only),
        [(best, winner), (runner_up, _), ..] if best < runner_up => Some(winner),
        _ => None,
    }
}

/// Warn when a broadcast names a project no agent has ever registered in.
///
/// **Not an error, deliberately.** D17 makes `@project` address a *place*, and a place may be
/// occupied tomorrow — so the message is kept and will reach whoever works there next. But that
/// argument protects a project that does not exist *yet*; it does nothing for a transposed
/// letter in one that already does, which used to be accepted in total silence (D26).
/// The sentence a sender sees when the session they addressed is no longer running.
///
/// Pure, so the wording is testable without a roster: the database half is
/// [`departed_recipient`], and every judgement lives here.
///
/// **Silence on the happy path is the whole design.** A note on every send is a note nobody reads
/// by the third one (D24's rule for injected context, which this is a cousin of), so `None` is the
/// answer whenever the recipient is alive or the answer is unknown. "Unknown" resolving to silence
/// rather than to a warning is deliberate and matches `is_alive`: a session with no pid we can ask
/// about degrades to recency, and warning on a maybe would train the reader to ignore the sentence
/// that matters.
pub fn departed_note(name: &str, alive: bool, quiet_secs: f64) -> Option<String> {
    if alive {
        return None;
    }
    let hours = quiet_secs / 3600.0;
    let ago = if hours < 1.0 {
        format!("{:.0} minute(s)", quiet_secs / 60.0)
    } else if hours < 48.0 {
        format!("{hours:.0} hour(s)")
    } else {
        format!("{:.0} day(s)", hours / 24.0)
    };
    // Says what is still true as well as what is wrong. The message is not lost — it is a log
    // (D17), and `amb inbox` returns it whenever that agent runs again. What it will *not* do is
    // arrive in a running session, which is the expectation `sent #N` sets on its own.
    Some(format!(
        "{name} last showed up {ago} ago and its session appears to be over, so nothing will \
         deliver this until they return. It is kept: the board is a log, and `amb inbox` will \
         still show it."
    ))
}

/// Warn when a direct message is addressed to a session that has stopped running.
///
/// **The sibling of [`unknown_project`], and the arm that was missing.** That function warns when
/// a *broadcast* names a place nobody has registered in (D26); the direct-message arm beside it
/// produced no warning at all, so `sent #383` read identically whether the recipient was mid-turn
/// or had exited days earlier. Measured on the real board before this was written: of 286 direct
/// messages, 282 were acknowledged and the 4 that were not were all addressed to sessions that had
/// already ended — the oldest waiting a week. The sender was told the same thing every time.
///
/// Advisory, never blocking, in keeping with the rest of the tool (D5): the message is written
/// either way and this only decides whether a sentence follows it.
pub fn departed_recipient(conn: &Connection, agent_id: &str, at: f64) -> Result<Option<String>> {
    let row: Option<(String, Option<i64>, f64)> = conn
        .query_row(
            "SELECT name, pid, last_seen FROM agents WHERE id = ?1",
            params![agent_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(sql("reading the recipient's roster row"))?;
    // No row is not a warning: `resolve_recipient` already refused an unknown name, so this can
    // only be a race with a roster that is being written, and inventing a death from it would be
    // a warning about nothing.
    let Some((name, pid, last_seen)) = row else {
        return Ok(None);
    };
    Ok(departed_note(
        &name,
        crate::identity::is_alive(pid, last_seen, at),
        (at - last_seen).max(0.0),
    ))
}

pub fn unknown_project(conn: &Connection, project: Option<&str>) -> Result<Option<String>> {
    // `@@` names no project and reaches everyone, so there is nothing to be wrong about.
    let Some(project) = project else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare("SELECT DISTINCT project FROM agents")
        .map_err(sql("preparing the project list"))?;
    let known: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .map_err(sql("listing projects"))?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql("reading a project row"))?;

    if known.iter().any(|k| k == project) {
        return Ok(None);
    }
    let refs: Vec<&str> = known.iter().map(String::as_str).collect();
    // Two whole sentences either way. Interpolating an optional clause mid-sentence produced
    // "...in \"api-v3\" the message is kept", which reads as a run-on — and this text is read by
    // a model, which has to infer intent from prose it cannot ask about.
    let hint = match nearest(project, &refs) {
        Some(s) => format!(" Did you mean {s:?}?"),
        None => String::new(),
    };
    Ok(Some(format!(
        "no agent has ever registered in {project:?}.{hint} The message is kept, and \
         `@project` addresses a place, so it will reach whoever works there next."
    )))
}

/// Mark a message read by this agent.
///
/// The **only** thing that sets `read_at`. Delivery is observable; comprehension is not, so a
/// read is always declared and never inferred (D9).
pub fn mark_read(conn: &Connection, me: &Identity, msg_id: i64) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1)",
            params![msg_id],
            |r| r.get(0),
        )
        .map_err(sql("checking the message exists"))?;
    if !exists {
        return Err(Error::NoSuchMessage(msg_id));
    }
    conn.execute(
        "INSERT INTO reads (msg_id, agent, read_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(msg_id, agent) DO UPDATE SET read_at = COALESCE(reads.read_at, ?3)",
        params![msg_id, me.id, now()?],
    )
    .map_err(sql("recording the read"))?;
    Ok(())
}

/// Record that a batch of messages was placed in front of an agent, in one transaction.
///
/// **Separate from [`mark_read`] on purpose: we can observe that we injected something; we cannot
/// observe that it was understood.** `attempts` counts offers **to this agent**, so an ignored
/// message eventually stops competing for context instead of repeating forever (D6, D23).
///
/// **This was written twice, and the tests exercised the copy that did not ship** (D84). A
/// single-id `mark_delivered` held the same `INSERT … ON CONFLICT` and had no production caller —
/// `tests/delivery.rs` used it to set up the two offer-counting assertions, so the back-off
/// semantics D44's claim-notice depends on were being asserted against an implementation nothing
/// ran. Deleted; the tests call this.
///
/// The delivery paths offer a batch at once, and doing it a row at a time cost one transaction,
/// one clock read and one statement preparation *per message* — on the `PostToolUse` path, which
/// runs after every tool call. One timestamp is also the truer record: they were injected
/// together, in one block of context, not at ten separate instants.
///
/// Lives here rather than as a loop in the binary because "record the offer, not a read" is the
/// D9 invariant, and it was being restated at two call sites in `main.rs` that had to stay in
/// step by hand.
/// Takes ids rather than messages **so the caller must name what it actually showed.** Handed a
/// `&[Message]`, the obvious argument is the set that was selected, which is not the set that was
/// rendered once [`crate::delivery::MAX_RENDERED`] applies (D33).
pub fn mark_delivered_all(conn: &mut Connection, me: &Identity, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let at = now()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql("opening the delivery transaction"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO reads (msg_id, agent, delivered_at, attempts) VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT(msg_id, agent) DO UPDATE SET
                   delivered_at = COALESCE(reads.delivered_at, ?3),
                   attempts     = reads.attempts + 1",
            )
            .map_err(sql("preparing the delivery record"))?;
        for id in ids {
            stmt.execute(params![id, me.id, at])
                .map_err(sql("recording the delivery"))?;
        }
    }
    tx.commit().map_err(sql("committing the deliveries"))
}

/// Acknowledge a batch of messages named explicitly.
///
/// Deliberately *not* one transaction: [`mark_read`] rejects an id that does not exist, and
/// `amb read 1 99` has always acknowledged `1` before failing on `99`. Wrapping the batch would
/// silently turn that into "acknowledged nothing", which is a different contract than the one
/// the exit code documents.
pub fn mark_read_many(conn: &Connection, me: &Identity, ids: &[i64]) -> Result<()> {
    for id in ids {
        mark_read(conn, me, *id)?;
    }
    Ok(())
}

/// Acknowledge everything currently unread, returning the ids acknowledged.
///
/// **What `--all` covers is a question about the board, not about the command line**, so it is
/// answered here: exactly what `amb inbox --unread` would show. Resolving it in the binary put a
/// rule that needs a database in the one place `CLAUDE.md` says holds no logic, where no test
/// could reach it without spawning a process.
///
/// One transaction is safe here in a way it is not for [`mark_read_many`]: every id came from the
/// query two lines above, so none of them can fail the existence check.
pub fn mark_read_all(conn: &mut Connection, me: &Identity) -> Result<Vec<i64>> {
    let ids: Vec<i64> = select(conn, me, true, None)?.iter().map(|m| m.id).collect();
    if ids.is_empty() {
        return Ok(ids);
    }
    let at = now()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql("opening the acknowledgement transaction"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO reads (msg_id, agent, read_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(msg_id, agent) DO UPDATE SET read_at = COALESCE(reads.read_at, ?3)",
            )
            .map_err(sql("preparing the acknowledgement"))?;
        for id in &ids {
            stmt.execute(params![id, me.id, at])
                .map_err(sql("recording the read"))?;
        }
    }
    tx.commit()
        .map_err(sql("committing the acknowledgements"))?;
    Ok(ids)
}

/// How many times this agent has already been offered a message.
///
/// **No production caller.** It was documented as existing "so the delivery path can say why
/// something stopped appearing", and that path never called it — the doc described an intention,
/// not the code. Kept rather than deleted because reading the offer count is the natural query
/// behind the back-off, and the alternative is raw SQL in the test that asserts D23 holds; it is
/// honest about being test-facing instead of claiming a caller it does not have.
pub fn offers(conn: &Connection, me: &Identity, msg_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE((SELECT attempts FROM reads WHERE msg_id = ?1 AND agent = ?2), 0)",
        params![msg_id, me.id],
        |r| r.get(0),
    )
    .map_err(sql("reading the offer count"))
}

/// Fetch one message by id.
pub fn get(conn: &Connection, msg_id: i64) -> Result<Message> {
    conn.query_row(
        &format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m LEFT JOIN agents a ON a.id = m.from_agent
             WHERE m.id = ?1"
        ),
        params![msg_id],
        row_to_message,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::NoSuchMessage(msg_id),
        other => Error::Sqlite {
            context: "fetching the message".into(),
            source: other,
        },
    })
}

/// Reply to a message, addressed back to its sender and keeping its thread.
///
/// A reply to a *broadcast* goes to the sender, not back to the whole project. Broadcasting a
/// reply to everyone is how a coordination channel turns into noise.
pub fn reply(conn: &mut Connection, me: &Identity, msg_id: i64, body: &str) -> Result<i64> {
    let original = get(conn, msg_id)?;
    let thread = original
        .thread_id
        .clone()
        .unwrap_or_else(|| original.id.to_string());
    // Addressed by the sender's already-resolved id, so no roster lookup is needed and a reply
    // still reaches a sender that has since renamed itself.
    let to = Recipient {
        agent_id: Some(original.from_agent.clone()),
        project: original
            .to_proj
            .clone()
            .or_else(|| Some(original.from_proj.clone())),
    };
    let subject = if original.subject.starts_with("Re: ") {
        original.subject.clone()
    } else {
        format!("Re: {}", original.subject)
    };
    send(
        conn,
        me,
        &Outgoing {
            to: &to,
            subject: &subject,
            body,
            kind: &original.kind,
            thread: Some(&thread),
            ext_id: None,
        },
    )
}

/// How long a broadcast stays on the delivery path (D96).
///
/// **Delivery only. `inbox` is unaffected and still shows everything**, which is the split D23 and
/// D24 already argue for: automatic injection spends context the agent did not ask to spend, while
/// an explicit read was asked for.
///
/// Twenty-four hours because four (the claim lease, D13) loses the overnight case, and a week is
/// long enough to have produced the fifteen superseded broadcasts M29 measured. It is the shortest
/// horizon that preserves "a session starting tomorrow morning still hears about it", which is the
/// realistic form of D17's claim on one machine.
///
/// A variable for `AMB_MEMORY_THRESHOLD`'s reason — a guess that needs a rebuild to change is a
/// decision wearing a parameter's clothes — and it takes a default, unlike `AMB_VAULT` (D35),
/// because a duration creates no state.
pub const BROADCAST_HORIZON: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// [`BROADCAST_HORIZON`], or whatever `AMB_BROADCAST_HORIZON` says.
///
/// Accepts the same spellings as every other duration here (`30m`, `4h`, `2d`), so there is one
/// grammar rather than a second one for this variable alone. An unparseable value falls back to
/// the default rather than failing: this is read on the delivery path, and D9 puts refusing to
/// deliver mail well above honouring a typo in an environment variable.
pub fn broadcast_horizon() -> std::time::Duration {
    horizon_from(std::env::var("AMB_BROADCAST_HORIZON").ok().as_deref())
}

/// The env shell's decision, injected — M51's seam pattern, and the seam audit's second finding.
///
/// **`broadcast_horizon` had one caller and no test of any kind** (M60). Everything above is a
/// stated rule: the fallback on a typo is deliberate and argued from D9, and the default is
/// D96's. None of it was asserted, and `unwrap_or(BROADCAST_HORIZON)` relaxed to a zero duration
/// stops every broadcast from being delivered at all — the delivery path this comment is about.
fn horizon_from(raw: Option<&str>) -> std::time::Duration {
    raw.and_then(|s| crate::duration::parse(s).ok())
        .unwrap_or(BROADCAST_HORIZON)
}

/// The shortest re-check interval [`watch`] will accept, in milliseconds.
///
/// **A floor rather than a clamp, and the difference is the point.** Clamping silently would give
/// a caller who asked for 0 ms a 50 ms loop and no way to tell — a wrong answer delivered quietly,
/// which is the failure class this project keeps finding. The binary hands this to clap as a
/// `value_parser` range, so `--poll 0` is refused at parse time with exit 64 and the caller is
/// told the bound.
///
/// Fifty milliseconds is well under the perceptible delay for `monitor` mode and far above the
/// cost of one `deliverable()` — a plain `SELECT` measured in microseconds — so the loop is
/// bounded by the sleep rather than by the query at any accepted value.
pub const MIN_POLL_MS: u64 = 50;

/// Block until this agent has unread mail, or the timeout elapses.
///
/// Backs `monitor` delivery mode (D9). SQLite has no cross-process change notification, so the
/// wait is an internal poll loop — **one** process startup amortised across the whole wait
/// rather than one per poll, which is exactly what makes it affordable and why it is not the
/// notification subsystem D7 rejected.
pub fn watch(
    conn: &mut Connection,
    me: &Identity,
    timeout: std::time::Duration,
    poll: std::time::Duration,
) -> Result<Vec<Message>> {
    // Monotonic here, unlike every stored timestamp: this is a duration measured inside one
    // process, so it must not be affected by a wall-clock adjustment mid-wait (D13).
    let start = std::time::Instant::now();
    loop {
        let found = deliverable(conn, me)?;
        if !found.is_empty() {
            // A blocking read puts these in front of the agent exactly as a hook does, so it
            // owes the same record. Without it `monitor` mode delivered mail that the next
            // `Stop` hook then offered again, and no offer was ever counted.
            //
            // Every id, not a capped subset: `watch` hands its whole result to the caller, so
            // here the offered set and the returned set really are the same one.
            let ids: Vec<i64> = found.iter().map(|m| m.id).collect();
            mark_delivered_all(conn, me, &ids)?;
            return Ok(found);
        }
        if start.elapsed() >= timeout {
            return Ok(Vec::new());
        }
        std::thread::sleep(poll.min(timeout.saturating_sub(start.elapsed())));
    }
}

#[cfg(test)]
mod tests {

    /// A truth table, and the `alive` row is what proves the others' premise: assert only that a
    /// live recipient produces no sentence and a renderer that had stopped producing sentences
    /// entirely would still pass. That is the absence-only trap this project keeps finding, so the
    /// dead rows assert the text as well as its presence.
    #[test]
    fn only_a_departed_recipient_earns_a_sentence_and_it_says_the_message_is_kept() {
        use super::departed_note;
        assert_eq!(
            departed_note("bob", true, 999_999.0),
            None,
            "a live session earns no note however long it has been quiet"
        );
        let note = departed_note("bob", false, 3600.0 * 50.0).expect("a departed session warns");
        assert!(note.contains("bob"), "it names the recipient: {note}");
        assert!(note.contains("2 day(s)"), "and how long ago: {note}");
        assert!(
            note.contains("kept") && note.contains("amb inbox"),
            "and that the message survives — it is a log, not a dropped write: {note}"
        );
        let fresh = departed_note("bob", false, 120.0).expect("still a warning");
        assert!(
            fresh.contains("2 minute(s)"),
            "minutes below the hour: {fresh}"
        );
        let hours = departed_note("bob", false, 3600.0 * 5.0).expect("still a warning");
        assert!(hours.contains("5 hour(s)"), "hours below two days: {hours}");
    }

    use super::*;

    /// **`broadcast_horizon` had one caller and no test of any kind** (M60, the seam audit).
    /// Everything its docstring argues is a real decision: the default is D96's, and falling back
    /// on an unparseable value rather than failing is argued from D9 — refusing to deliver mail
    /// is worse than ignoring a typo in an environment variable.
    ///
    /// The non-zero row is the one that matters most. `unwrap_or(BROADCAST_HORIZON)` relaxed to a
    /// zero duration makes the cutoff `now`, so **no broadcast is ever delivered** — a silence on
    /// the delivery path, which is this project's signature failure and was unguarded here.
    #[test]
    fn the_broadcast_horizon_reads_env_and_falls_back_rather_than_failing() {
        assert_eq!(
            horizon_from(Some("30m")),
            std::time::Duration::from_secs(1800)
        );
        assert_eq!(
            horizon_from(Some("2d")),
            std::time::Duration::from_secs(172_800)
        );
        for bad in [
            None,
            Some(""),
            Some("   "),
            Some("later"),
            Some("30x"),
            Some("-1h"),
        ] {
            assert_eq!(
                horizon_from(bad),
                BROADCAST_HORIZON,
                "{bad:?} falls back to the default rather than failing delivery"
            );
        }
        assert!(
            !BROADCAST_HORIZON.is_zero(),
            "a zero horizon delivers no broadcast at all — the default cannot be it"
        );
    }

    /// A board with two agents in one project, and a third somewhere else.
    fn board() -> (tempfile::TempDir, Connection, Identity, Identity, Identity) {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::db::open_at(&dir.path().join("board.db")).expect("open");
        let who = |id: &str, name: &str, project: &str| Identity {
            id: id.to_string(),
            name: name.to_string(),
            project: project.to_string(),
            root: dir.path().to_string_lossy().into_owned(),
        };
        let alice = who("uuid-alice", "alice", "nest");
        let bob = who("uuid-bob", "bob", "nest");
        let carol = who("uuid-carol", "carol", "other");
        for me in [&alice, &bob, &carol] {
            crate::identity::touch(&conn, me, Some(&me.name)).expect("register");
        }
        (dir, conn, alice, bob, carol)
    }

    /// An oversized body is refused *and* leaves nothing behind.
    ///
    /// The second half is the point. Refusing after the insert would be a message the recipient
    /// can neither read comfortably nor decline, and `send` is the only write path (D10) — so
    /// the check sits above the transaction rather than inside it.
    #[test]
    fn a_body_past_the_cap_is_refused_before_anything_is_written() {
        let (_d, mut conn, alice, bob, _c) = board();
        let addr = Address::Agent {
            name: "alice".into(),
            project: None,
        };
        let to = resolve_recipient(&conn, &addr, &bob).expect("resolve");
        let huge = "x".repeat(MAX_BODY + 1);
        let out = Outgoing {
            to: &to,
            kind: "note",
            subject: "s",
            body: &huge,
            thread: None,
            ext_id: None,
        };

        let err = send(&mut conn, &bob, &out).expect_err("a body past the cap must be refused");
        assert!(
            matches!(err, Error::BodyTooLarge { .. }),
            "refused for the wrong reason: {err}"
        );
        assert_eq!(
            0,
            inbox(&conn, &alice, false).expect("inbox").len(),
            "the refusal stored a message anyway"
        );

        // And the cap is a ceiling, not a fence one character below it.
        let ok = "x".repeat(MAX_BODY);
        send(&mut conn, &bob, &Outgoing { body: &ok, ..out }).expect("exactly at the cap is fine");
        assert_eq!(1, inbox(&conn, &alice, false).expect("inbox").len());
    }

    /// D106: the body's cap had a sibling gap — a 300 KB *subject* was accepted and stored,
    /// because containment lived on the renderer (`QUOTED_MAX`) and not on the field, which is
    /// the exact defect `MAX_BODY`'s own doc records for bodies.
    #[test]
    fn a_subject_past_the_cap_is_refused_at_the_sender() {
        let (_d, mut conn, alice, bob, _c) = board();
        let addr = Address::Agent {
            name: "alice".into(),
            project: None,
        };
        let to = resolve_recipient(&conn, &addr, &bob).expect("resolve");
        let long = "s".repeat(MAX_SUBJECT + 1);
        let out = Outgoing {
            to: &to,
            kind: "note",
            subject: &long,
            body: "b",
            thread: None,
            ext_id: None,
        };
        let err = send(&mut conn, &bob, &out).expect_err("past the cap");
        assert!(
            matches!(
                err,
                Error::FieldTooLarge {
                    field: "subject",
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(
            0,
            inbox(&conn, &alice, false).expect("inbox").len(),
            "the refusal stored a message anyway"
        );
        let exact = "s".repeat(MAX_SUBJECT);
        send(
            &mut conn,
            &bob,
            &Outgoing {
                subject: &exact,
                ..out
            },
        )
        .expect("exactly at the cap is fine");
        assert_eq!(1, inbox(&conn, &alice, false).expect("inbox").len());
    }

    /// U1: the inbox query answers "has this reader acknowledged it" per row, and the one path
    /// with no reader in scope answers `None` rather than guessing. Both directions asserted —
    /// a flag that is always `Some(false)` would pass a presence-only check.
    #[test]
    fn the_inbox_says_which_rows_this_reader_has_acknowledged() {
        let (_d, mut conn, alice, bob, _c) = board();
        let addr = Address::Agent {
            name: "alice".into(),
            project: None,
        };
        let to = resolve_recipient(&conn, &addr, &bob).expect("resolve");
        let out = Outgoing {
            to: &to,
            kind: "note",
            subject: "s",
            body: "b",
            thread: None,
            ext_id: None,
        };
        let first = send(&mut conn, &bob, &out).expect("send");
        let _second = send(
            &mut conn,
            &bob,
            &Outgoing {
                ext_id: Some("k2"),
                ..out
            },
        )
        .expect("send");
        mark_read(&conn, &alice, first).expect("read");

        let rows = inbox(&conn, &alice, false).expect("inbox");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].read, Some(true), "the acknowledged row says so");
        assert_eq!(rows[1].read, Some(false), "the new row says so");
        assert_eq!(
            rows[0].to_json()["read"],
            serde_json::json!(true),
            "the flag reaches --json"
        );

        let fetched = get(&conn, first).expect("get");
        assert_eq!(fetched.read, None, "get() has no reader in scope");
        assert!(
            fetched.to_json().get("read").is_none(),
            "an unanswerable question is absent from JSON, not null"
        );
    }

    /// **`from` is a display name, and a display name is not an address** (U8).
    ///
    /// A session read `"from":"nestwatch-f04621"` out of `--json`, passed it to `amb send`, and
    /// was refused — the name resolves only inside its own project, and the reader of a global
    /// broadcast is usually somewhere else. Both halves of the answer were already in the
    /// document, one key apart, and every caller had to know to join them.
    ///
    /// Asserted by *using* it rather than by matching a string: the field is fed straight back
    /// into `resolve_recipient`, which is the layer a copy-paste actually hits. A test that only
    /// checked the key was present would pass on an address nobody can send to.
    #[test]
    fn the_address_beside_the_name_is_one_a_reply_can_actually_be_sent_to() {
        let (_d, mut conn, alice, _bob, carol) = board();
        // Carol is in another project — exactly the case where the bare name fails.
        let everywhere = Recipient {
            agent_id: None,
            project: None,
        };
        send(
            &mut conn,
            &carol,
            &Outgoing {
                to: &everywhere,
                kind: "note",
                subject: "s",
                body: "b",
                thread: None,
                ext_id: None,
            },
        )
        .expect("carol broadcasts");

        let got = inbox(&conn, &alice, false).expect("alice reads");
        let doc = got[0].to_json();
        let address = doc["address"].as_str().expect("an address field");
        assert_eq!(address, "carol@other");
        assert_ne!(
            address,
            doc["from"].as_str().expect("a name"),
            "the name alone is what failed; if they are equal this field adds nothing"
        );

        let parsed = crate::address::parse(address).expect("the address parses");
        let back =
            resolve_recipient(&conn, &parsed, &alice).expect("and resolves, from another project");
        assert_eq!(back.agent_id.as_deref(), Some("uuid-carol"));
    }

    /// **The refusal knew the answer and withheld it** (U8). The same name one project over is
    /// the ordinary case — that is what makes it worth typing — and the row proving it is one
    /// query away. D26's `nearest` covers a typo; this is the exact name in the wrong scope,
    /// which no edit distance reaches. Both rows: the suggestion when it exists, and the plain
    /// refusal when the name is nowhere, so the hint cannot be invented.
    #[test]
    fn a_name_registered_elsewhere_is_refused_with_the_address_that_would_work() {
        let (_d, conn, _alice, bob, _carol) = board();
        let addr = Address::Agent {
            name: "carol".into(),
            project: None,
        };
        let err = resolve_recipient(&conn, &addr, &bob).expect_err("carol is not in nest");
        assert!(
            matches!(err, Error::AgentInAnotherProject { .. }),
            "{err:?}"
        );
        assert!(
            err.to_string().contains("carol@other"),
            "the refusal must carry the address it already knows: {err}"
        );
        assert_eq!(
            err.exit_code(),
            65,
            "still a data error, not a new contract"
        );

        let nowhere = Address::Agent {
            name: "nobody".into(),
            project: None,
        };
        let err = resolve_recipient(&conn, &nowhere, &bob).expect_err("nobody is nowhere");
        assert!(matches!(err, Error::NoSuchAgent { .. }), "{err:?}");
        assert!(
            !err.to_string().contains("did you mean"),
            "a suggestion with nothing behind it is worse than none: {err}"
        );
    }

    /// D107's write half: the kind charset is refused where the author can still fix it. The
    /// render half (`delivery::scope_kind`) has its own table for rows this check never saw.
    #[test]
    fn a_kind_outside_the_charset_is_refused_at_the_sender() {
        let (_d, mut conn, _alice, bob, _c) = board();
        let addr = Address::Agent {
            name: "alice".into(),
            project: None,
        };
        let to = resolve_recipient(&conn, &addr, &bob).expect("resolve");
        let base = Outgoing {
            to: &to,
            kind: "note",
            subject: "s",
            body: "b",
            thread: None,
            ext_id: None,
        };
        // Sized off the constant, so the cap can move without this row silently becoming a
        // valid kind (the reached-assertion audit: a fixture that drifts under a grown cap
        // stops testing refusal, and only loudly if it asserts refusal — this one does).
        let over_cap = "x".repeat(MAX_KIND + 1);
        for bad in ["Bad Kind", "", "question!", over_cap.as_str()] {
            let err = send(&mut conn, &bob, &Outgoing { kind: bad, ..base })
                .expect_err("outside the charset");
            assert!(matches!(err, Error::BadKind { .. }), "{bad:?}: {err:?}");
        }
        send(
            &mut conn,
            &bob,
            &Outgoing {
                kind: "question",
                ..base
            },
        )
        .expect("a tame kind");
    }

    /// **Redaction is deliberately absent from the send path (D98), and this is the assertion of
    /// that absence.** `redact` guards the vault because a note is prose *about* work (D37); a
    /// message routinely *contains* the artefact — a command, a config fragment, a path — and the
    /// literal string is the payload. Nothing else in the tree would go red if the call were added.
    ///
    /// The first assertion is the premise, and M23 is why it is written down rather than assumed:
    /// an absence-only test proves nothing unless its fixture reaches the thing being excluded. If
    /// `redact` ever stopped matching this token, everything below would still pass and this test
    /// would guard nothing while staying green.
    #[test]
    fn a_body_is_stored_verbatim_because_the_send_path_does_not_redact() {
        let secret = concat!("ghp_", "0123456789abcdefghijABCDEFGHIJ0123456789");
        assert_eq!(
            1,
            crate::memory::redact(secret).removed,
            "the fixture no longer reaches the redactor, so the assertions below prove nothing"
        );

        let (_d, mut conn, alice, bob, _c) = board();
        let addr = Address::Agent {
            name: "alice".into(),
            project: None,
        };
        let to = resolve_recipient(&conn, &addr, &bob).expect("resolve");
        let subject = format!("deploy failed with {secret}");
        let body = format!("the command was `deploy --token {secret}`, reproduced verbatim");
        send(
            &mut conn,
            &bob,
            &Outgoing {
                to: &to,
                kind: "note",
                subject: &subject,
                body: &body,
                thread: None,
                ext_id: None,
            },
        )
        .expect("send");

        // Both fields, because `send` writes both and a secret in a subject is just as durable.
        let got = inbox(&conn, &alice, false).expect("inbox");
        assert_eq!(1, got.len());
        assert_eq!(
            body, got[0].body,
            "the send path redacted a body; D98 says it must not"
        );
        assert_eq!(
            subject, got[0].subject,
            "the send path redacted a subject; D98 says it must not"
        );
    }

    fn send_to(conn: &mut Connection, from: &Identity, to: &Address, subject: &str) -> i64 {
        let r = resolve_recipient(conn, to, from).expect("resolve");
        send(
            conn,
            from,
            &Outgoing {
                to: &r,
                kind: "note",
                subject,
                body: "b",
                thread: None,
                ext_id: None,
            },
        )
        .expect("send")
    }

    /// **The first unit test `select` has ever had**, and the reason it exists is a mutation that
    /// survived: reversing `ORDER BY m.id` broke nothing in 358 tests.
    ///
    /// It survived because `delivery::render_all` re-sorts by `(urgency, id)` before rendering, so
    /// the hook path is correct *whatever* this query returns. What has no such protection is
    /// `amb inbox`, which prints this order straight to a person — so the ordering was load-bearing
    /// on exactly the path nothing covered. D51's shape: correct by accident on one path, unguarded
    /// on the one that depends on it.
    #[test]
    fn the_inbox_arrives_in_the_order_it_was_sent() {
        let (_d, mut conn, alice, bob, _carol) = board();
        let to_bob = Address::Agent {
            name: "bob".into(),
            project: None,
        };
        let first = send_to(&mut conn, &alice, &to_bob, "first");
        let second = send_to(&mut conn, &alice, &to_bob, "second");
        let third = send_to(&mut conn, &alice, &to_bob, "third");

        let got: Vec<i64> = inbox(&conn, &bob, false)
            .expect("inbox")
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(
            got,
            vec![first, second, third],
            "oldest first, or a person reading `amb inbox` sees a conversation backwards"
        );
    }

    /// D17's central claim, as one assertion: **four addressing modes, one query.**
    ///
    /// Covered end-to-end by `tests/delivery.rs`, and never here — so a change to the predicate
    /// could only be caught by spawning processes. Each arm is pinned separately, because a single
    /// "bob sees three messages" assertion passes when two arms are broken in opposite directions.
    #[test]
    fn one_predicate_covers_every_addressing_mode_and_excludes_the_sender() {
        let (_d, mut conn, alice, bob, carol) = board();
        let direct = send_to(
            &mut conn,
            &alice,
            &Address::Agent {
                name: "bob".into(),
                project: None,
            },
            "direct",
        );
        let to_project = send_to(
            &mut conn,
            &alice,
            &Address::Broadcast { project: None },
            "proj",
        );
        let to_everyone = send_to(&mut conn, &alice, &Address::Everyone, "all");

        let seen = |me: &Identity| -> Vec<i64> {
            inbox(&conn, me, false)
                .expect("inbox")
                .iter()
                .map(|m| m.id)
                .collect()
        };

        // Bob is in the project and is the named recipient: all three reach him.
        assert_eq!(seen(&bob), vec![direct, to_project, to_everyone]);
        // Carol is elsewhere: the global broadcast reaches her, the project one does not, and
        // neither does mail addressed to Bob.
        assert_eq!(
            seen(&carol),
            vec![to_everyone],
            "a project broadcast must not leak across projects, and direct mail must not either"
        );
        // **The sender is excluded, asserted explicitly.** Without the `from_agent <> ?2` guard
        // every broadcast echoes back to its author as unread mail — noise in a channel whose
        // whole value is that unread means "something you have not seen".
        assert!(
            seen(&alice).is_empty(),
            "a sender does not need telling what it just said: {:?}",
            seen(&alice)
        );
    }

    /// The same 2×2, read back through the surface a machine consumes.
    ///
    /// **Found by mutation, and it is the read side of the claim the test above pins on the write
    /// side.** `is_broadcast`, `is_global` and `scope` are how `--json` reports which of the four
    /// modes a message used; nothing asserted any of them. Every one of these survived:
    /// `is_broadcast` forced to `true` *and* to `false`, `is_global` likewise, and `&&` swapped
    /// for `||` in `is_global` — five mutants, one unguarded field of the machine surface.
    ///
    /// All three modes are needed and none is redundant. `direct` kills the `true` forcings,
    /// `to_project` kills the `false` forcings and the `||`, `to_everyone` kills the rest. Drop a
    /// row and a mutant comes back.
    /// Mid-turn delivery offers a message once, and an explicit read is not rationed.
    ///
    /// **Found by mutation: `undelivered` could return `Ok(vec![])` unconditionally and nothing
    /// went red.** That is D25 deleted — the `PostToolUse` lane silently stops delivering, mail
    /// still arrives at the next `Stop`, and the only symptom is that it arrives later. A silence,
    /// which is this project's whole failure mode.
    ///
    /// The second half is the other side of the same rule: `undelivered` is rationed because the
    /// hook spends context the agent did not ask to spend; `inbox` must not be, or a message
    /// ignored for a while becomes unrecoverable.
    #[test]
    fn mid_turn_delivery_offers_a_message_once_and_an_explicit_read_is_not_rationed() {
        let (_d, mut conn, alice, bob, _carol) = board();
        let id = send_to(
            &mut conn,
            &alice,
            &Address::Agent {
                name: "bob".into(),
                project: None,
            },
            "hi",
        );
        let ids = |ms: Vec<Message>| ms.iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(undelivered(&conn, &bob).expect("first pass")),
            vec![id],
            "new mail must reach the mid-turn lane at least once"
        );
        mark_delivered_all(&mut conn, &bob, &[id]).expect("record the offer");
        assert!(
            undelivered(&conn, &bob).expect("second pass").is_empty(),
            "offered once is offered enough: the hook fires after every tool call"
        );
        assert_eq!(
            ids(inbox(&conn, &bob, true).expect("inbox")),
            vec![id],
            "an explicit read is not rationed, or an ignored message becomes unrecoverable"
        );
    }

    /// `watch` hands back waiting mail at once, and an empty hand at the deadline.
    ///
    /// **Found by mutation: `watch` had no test at all, and three separate mutations survived** —
    /// returning `Ok(vec![])`, deleting the `!` in `if !found.is_empty()`, and flipping `>=` to
    /// `<` on the deadline. All three collapse to the same behaviour: `watch` returns nothing,
    /// immediately, forever. The `SessionStart` banner tells every agent on this machine to run
    /// `amb watch --timeout 300 --json` under its Monitor tool for immediate delivery, so the
    /// symptom would have been mail that merely arrives at the next `Stop` instead — late rather
    /// than lost, and therefore invisible.
    ///
    /// The elapsed-time assertion is the part that kills two of the three: an early return still
    /// yields an empty vector, so *what* came back cannot distinguish them and *when* can.
    #[test]
    fn watch_returns_waiting_mail_at_once_and_an_empty_hand_at_the_deadline() {
        use std::time::{Duration, Instant};
        let (_d, mut conn, alice, bob, _carol) = board();

        let started = Instant::now();
        let nothing = watch(
            &mut conn,
            &bob,
            Duration::from_millis(60),
            Duration::from_millis(10),
        )
        .expect("an empty wait is not an error");
        assert!(nothing.is_empty());
        assert!(
            started.elapsed() >= Duration::from_millis(50),
            "it must wait for the deadline rather than return an empty hand at once"
        );

        let id = send_to(
            &mut conn,
            &alice,
            &Address::Agent {
                name: "bob".into(),
                project: None,
            },
            "hi",
        );
        let found = watch(
            &mut conn,
            &bob,
            Duration::from_secs(5),
            Duration::from_millis(5),
        )
        .expect("mail was waiting");
        assert_eq!(found.iter().map(|m| m.id).collect::<Vec<_>>(), vec![id]);
        assert_eq!(
            offers(&conn, &bob, id).expect("the offer count"),
            1,
            "a blocking read puts mail in front of an agent exactly as a hook does, so it owes \
             the same record — without it the next Stop offers the same message again"
        );
    }

    /// The hand-rolled edit distance is the metric it claims to be.
    ///
    /// **Found by mutation: `cur[0] = i + 1` could become `cur[0] = i` and every test stayed
    /// green.** That column is the cost of deleting a prefix, so the mutant under-counts by one
    /// whenever the first argument is the longer — and `nearest` compares against a budget, so an
    /// off-by-one there widens what counts as a near miss. The existing tests exercise `nearest`
    /// on a handful of names and none of them straddled the boundary.
    ///
    /// This is the case for keeping the implementation rather than taking a dependency: the
    /// function is four lines and its correctness is a table.
    #[test]
    fn the_edit_distance_is_the_metric_it_claims_to_be() {
        assert_eq!(distance("", ""), 0);
        assert_eq!(distance("nest", "nest"), 0);
        // Deleting a whole string, which is exactly the column the mutation lived in.
        assert_eq!(distance("api", ""), 3);
        assert_eq!(distance("", "api"), 3);
        assert_eq!(distance("api-v1", "api-v2"), 1);
        // Knuth's example, so the table is checkable against something outside this file.
        assert_eq!(distance("kitten", "sitting"), 3);
        // A metric is symmetric. Cheap to assert and it pins both triangle terms at once.
        assert_eq!(distance("nestwatch", "nest"), distance("nest", "nestwatch"));
    }

    #[test]
    fn every_addressing_mode_reads_back_the_same_on_the_machine_surface() {
        let (_d, mut conn, alice, _bob, _carol) = board();
        let direct = send_to(
            &mut conn,
            &alice,
            &Address::Agent {
                name: "bob".into(),
                project: None,
            },
            "direct",
        );
        let to_project = send_to(
            &mut conn,
            &alice,
            &Address::Broadcast { project: None },
            "proj",
        );
        let to_everyone = send_to(&mut conn, &alice, &Address::Everyone, "all");

        for (id, scope, broadcast, global) in [
            (direct, "direct", false, false),
            (to_project, "broadcast", true, false),
            (to_everyone, "global", true, true),
        ] {
            let doc = get(&conn, id).expect("the message").to_json();
            assert_eq!(doc["scope"], scope, "{doc}");
            assert_eq!(doc["broadcast"], broadcast, "{doc}");
            assert_eq!(doc["global"], global, "{doc}");
        }
    }

    #[test]
    fn a_scoped_key_is_unique_per_sender_and_stable_per_message() {
        let a = scoped_ext_id("uuid-alice", Some("task-1"));
        let b = scoped_ext_id("uuid-bob", Some("task-1"));
        assert_ne!(a, b, "the same key from two senders must not collide");
        assert_eq!(
            a,
            scoped_ext_id("uuid-alice", Some("task-1")),
            "and must be stable, or a resend stops being idempotent"
        );
        assert_eq!(
            scoped_ext_id("uuid-alice", None),
            None,
            "no key means no row in the unique index at all"
        );
    }

    #[test]
    fn a_near_miss_is_suggested_and_a_wild_guess_is_not() {
        let known = ["nestwatch", "agent-messageboard", "greenfield-api"];
        assert_eq!(
            nearest("nestwtach", &known),
            Some("nestwatch"),
            "a transposition is exactly the typo worth catching"
        );
        assert_eq!(nearest("nestwatc", &known), Some("nestwatch"));
        assert_eq!(
            nearest("totally-different", &known),
            None,
            "a suggestion that is usually wrong invites correcting a name that was right"
        );
        assert_eq!(nearest("nestwatch", &known), Some("nestwatch"));
    }

    #[test]
    fn a_tie_produces_no_suggestion_at_all() {
        // D26. `api-v3` is one edit from both, so naming either is a coin flip dressed as help.
        assert_eq!(nearest("api-v3", &["api-v1", "api-v2"]), None);
        // **A clear winner among several candidates is still suggested — and this line used to
        // assert nothing.** It read `&["api-v1", "totally-elsewhere"]`, and the second name is
        // outside the budget, so the filter dropped it and `scored` reached the *one-candidate*
        // arm. The `best < runner_up` guard was never evaluated: replacing it with `false`
        // survived mutation, and so did flipping `<` to `>`. Both mean "never suggest when two
        // names are close", which is the whole rule this arm exists for.
        //
        // `spi-v1` is two edits from `api-v1x` against `api-v1`'s one, so both survive the
        // budget and the guard actually decides.
        assert_eq!(
            nearest("api-v1x", &["api-v1", "spi-v1"]),
            Some("api-v1"),
            "two candidates within budget and one strictly better: the better one wins"
        );
    }

    #[test]
    fn the_suggestion_budget_scales_with_the_name() {
        // Two edits in a short name is a typo; two edits in a three-letter name is another word.
        assert_eq!(nearest("api", &["ftp"]), None);
        assert_eq!(nearest("api", &["apo"]), Some("apo"));
    }
}

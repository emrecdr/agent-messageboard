//! What happened to the mail *this session sent* — the sender's half of the receipt.
//!
//! [`crate::status`] answers *is the board healthy*, board-wide. It cannot answer *did anyone get
//! the thing I sent*, and the gap was reported from outside: a session evaluating `amb` across a
//! two-repo seam observed that `unoffered: 3` is board-wide and unattributable, so a sender who
//! wants to know whether their message landed has nothing to read.
//!
//! **This is a `SELECT` over rows that already exist, so D10 is not engaged.** No outbox, no relay,
//! no new write path — `reads` has carried `delivered_at` and `read_at` since schema 2, and the
//! only thing missing was a query keyed on `from_agent`.
//!
//! # Direct mail is three states, not a funnel
//!
//! The two-tick model every messenger trains you to expect — sent, then delivered, then read, each
//! a subset of the last — **is false here**, and rendering it would print what looks like an
//! arithmetic bug. Measured on the real board the day this landed: one session showed 49 delivered
//! against 50 read, and 118 rows board-wide carry a `read_at` with no `delivered_at` at all.
//!
//! The cause is [`crate::messages::mark_read`], which inserts `read_at` alone. A recipient who runs
//! `amb inbox` and then `amb read <id>` acknowledges mail no hook ever injected. That is not a
//! defect; it is the log-not-queue design working — mail can be *fetched* as well as *delivered* —
//! and chat protocols have no equivalent because their client is always connected.
//!
//! So a direct message this session sent is in exactly one of:
//!
//! | state | `reads` row | meaning |
//! |---|---|---|
//! | handed | `delivered_at` set | a hook put it in front of them |
//! | fetched | `read_at` set, `delivered_at` null | they went and got it themselves |
//! | unoffered | no row at all | it has never reached them |
//!
//! **The three partition the total, and that is asserted rather than assumed** — verified against
//! every sender on the real board before the module was written, and pinned by
//! [`tests::the_three_states_partition_the_direct_total`].
//!
//! **This is D127 seen from the sender's side.** That decision fixed `read 752 of 716 offer(s) ·
//! 105%` on the board-wide receipt, where the numerator counted rows the denominator excluded. The
//! same two populations exist here; the difference is that a *direct* message has exactly one
//! intended recipient, so the honest denominator is "messages I sent" and every state divides into
//! it cleanly.
//!
//! # Broadcast gets a numerator and no rate, deliberately
//!
//! **There is no denominator, and inventing one would commit D74 inside the instrument built to
//! answer a complaint about measurement.** `reads` rows are written *by the recipient*, so an agent
//! who never came back has no row — [`crate::status`]'s own `unoffered` query says as much in a
//! comment: *"A broadcast has no one recipient it can be said to have missed."*
//!
//! The only roster available is `agents`, which held 48 rows across 14 projects when this shipped.
//! Dividing by it would put *sessions that returned inside D96's 24-hour horizon* over *every
//! session that ever registered* — two different sentences, which is question 1 of the ratio rule.
//! Most of those 48 are dead sessions that will never return.
//!
//! **So the roster is not a field on [`Sent`], and that is deliberate.** An earlier cut carried it
//! to name the rejected denominator on the page. It is a fact about the board rather than about
//! what this session sent — `amb agents` and `amb status` both already report it — and putting it
//! in the `--json` object beside `broadcast_reached` would hand a parser the exact division this
//! module refuses to perform. The argument for the refusal belongs here and in D139; the receipt
//! only has to state that a refusal happened.
//!
//! That is not a limitation to route around. D17 makes `@project` address a **place**, not a set of
//! connected processes, and the field is unanimous that per-subscriber acknowledgement requires
//! *durable subscriptions* — a registered subscriber list that survives disconnection. amb has none
//! by design, and acquiring one to compute this number would destroy the property that makes a
//! broadcast reach an agent who registers tomorrow.
//!
//! So the reach count is printed and the refusal is spelled out in words on the line beneath it —
//! **not** as a `—` in a rate column, which would read as *nothing to divide yet* rather than
//! *this must not be divided*. [`crate::status::rate`] does render `—` and is used on the direct
//! line, where an empty denominator really does mean "you have sent nothing".
//!
//! # A mutation score on this module certifies nothing, and it is high
//!
//! Measured the day it landed: `cargo mutants --file src/sent.rs` finds **4 mutants and catches
//! 4** — a clean 100%. The four are *"make `gather` return `Default::default()`"*, two on `render`
//! returning an empty or junk string, and one on `render_json`. **Not one of them touches a state
//! boundary**, because every decision this module makes lives inside a SQL string:
//! `r.delivered_at IS NULL AND r.read_at IS NOT NULL` is the whole three-state rule and it is an
//! opaque `&str` to the mutation engine.
//!
//! That is M71's finding arriving in a module built after it was written, which is why it is
//! recorded here rather than left for the score to imply otherwise. The predicates are asserted by
//! hand instead — [`tests::the_three_states_partition_the_direct_total`] as an identity over
//! `gather`'s own output, and `cli_e2e`'s
//! `a_sent_message_is_handed_fetched_or_never_delivered_and_the_three_partition` through the
//! shipped binary, where the two arriving states are produced by the two commands that really
//! write those columns. Neither would exist if the score had been read as coverage.

use crate::error::{Result, sql};
use crate::status::rate;
use rusqlite::Connection;

/// Counts over one sender's outgoing mail, each field's unit named where it is not obvious.
///
/// No arithmetic here, for the same reason [`crate::status::Board`] holds none: the division
/// happens in [`render`], where the two numbers are adjacent and a reader can see which is which.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sent {
    /// Direct messages this session sent — `to_agent IS NOT NULL`, so exactly one intended
    /// recipient each. This is the denominator every direct rate below is taken over.
    pub direct: i64,
    /// A hook put it in front of the recipient: `delivered_at` is set.
    pub handed: i64,
    /// **The state a funnel cannot express.** `read_at` set with `delivered_at` still null — the
    /// recipient found it with `amb inbox` and acknowledged it before any hook offered it. 118 such
    /// rows existed board-wide when this shipped, so it is a standing condition and not an edge
    /// case (D127 reports the same population board-wide as `acknowledged_unoffered`).
    pub fetched: i64,
    /// **The unhappy path.** No `reads` row at all: that session has not come back since it was
    /// sent. Distinct from a message that was offered and ignored — nothing was spent here.
    pub unoffered: i64,
    /// Direct messages carrying a `read_at`, whichever way they arrived. Counted over the same rows
    /// [`Sent::direct`] counts, because a numerator and denominator that describe different
    /// populations is the defect D127 exists to record.
    pub acknowledged: i64,

    /// Broadcasts this session sent — `to_agent IS NULL`, covering both `@project` and `@@`.
    pub broadcasts: i64,
    /// **Distinct agents that have a `reads` row for any of them — a numerator with no denominator.**
    /// Counted over rows in either arriving state, matching the direct side's treatment of
    /// `fetched`; consistency *inside* one receipt matters more than matching a different
    /// receipt asking a different question.
    pub reached: i64,
    /// Broadcast rows carrying a `read_at`.
    pub broadcast_acknowledged: i64,
    /// **What the broadcasts cost everyone else** — `sum(reads.attempts)`, the unit that rises every
    /// time an injection is actually paid for. The row count understates it, which is question 2 of
    /// the ratio rule and the reason both numbers are printed.
    pub injections: i64,
}

/// Read every count for one sender in one pass.
pub fn gather(conn: &Connection, me: &str) -> Result<Sent> {
    let one = |q: &str| -> Result<i64> {
        conn.query_row(q, [me], |r| r.get::<_, Option<i64>>(0))
            .map(|v| v.unwrap_or(0))
            .map_err(sql("counting what this session sent"))
    };

    // Joined on `r.agent = m.to_agent` rather than on `msg_id` alone: a direct message has one
    // intended recipient, and a row for anybody else would be a different question.
    let direct_state = |clause: &str| -> Result<i64> {
        one(&format!(
            "SELECT count(*) FROM messages m
               JOIN reads r ON r.msg_id = m.id AND r.agent = m.to_agent
              WHERE m.from_agent = ?1 AND m.to_agent IS NOT NULL AND {clause}"
        ))
    };
    let broadcast_agg = |expr: &str| -> Result<i64> {
        one(&format!(
            "SELECT {expr} FROM messages m JOIN reads r ON r.msg_id = m.id
              WHERE m.from_agent = ?1 AND m.to_agent IS NULL"
        ))
    };

    Ok(Sent {
        direct: one(
            "SELECT count(*) FROM messages WHERE from_agent = ?1 AND to_agent IS NOT NULL",
        )?,
        handed: direct_state("r.delivered_at IS NOT NULL")?,
        fetched: direct_state("r.delivered_at IS NULL AND r.read_at IS NOT NULL")?,
        // `NOT EXISTS` rather than a `LEFT JOIN … IS NULL`, matching `status`'s `unoffered`.
        unoffered: one("SELECT count(*) FROM messages m
              WHERE m.from_agent = ?1 AND m.to_agent IS NOT NULL
                AND NOT EXISTS (SELECT 1 FROM reads r
                                 WHERE r.msg_id = m.id AND r.agent = m.to_agent)")?,
        acknowledged: direct_state("r.read_at IS NOT NULL")?,

        broadcasts: one(
            "SELECT count(*) FROM messages WHERE from_agent = ?1 AND to_agent IS NULL",
        )?,
        reached: broadcast_agg("count(DISTINCT r.agent)")?,
        broadcast_acknowledged: broadcast_agg("count(*) FILTER (WHERE r.read_at IS NOT NULL)")?,
        injections: broadcast_agg("sum(r.attempts)")?,
    })
}

/// The receipt, as a person reads it.
///
/// **Every line renders unconditionally, including at zero.** M27 measured a sibling renderer at
/// 52/92 with thirty-seven of forty survivors sitting on the `if` deciding whether a line appears
/// at all — ten of them the literal `x > 0` -> `x >= 0`. A count guard carries that relaxation and
/// an unguarded line carries none, so the cheapest defence is not to write the guard. The zero rows
/// are also the informative ones: `0 never reached them` is the thing a sender wants to see.
pub fn render(s: &Sent) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    let _ = writeln!(
        out,
        "sent      {} direct · {} broadcast",
        s.direct, s.broadcasts
    );

    // The three states, in the order a sender cares about: the good one, the surprising one, then
    // the failure. Each is a plain count of messages, so they sum to `direct` and a reader can
    // check the arithmetic on the page.
    let _ = writeln!(out, "direct    {} handed over by a hook", s.handed);
    let _ = writeln!(
        out,
        "          {} fetched by the recipient — read, never injected",
        s.fetched
    );
    let _ = writeln!(
        out,
        "          {} never reached them — that session has not come back",
        s.unoffered
    );
    let _ = writeln!(
        out,
        "          {} of {} acknowledged · {}",
        s.acknowledged,
        s.direct,
        rate(s.acknowledged, s.direct)
    );

    let _ = writeln!(
        out,
        "broadcast {} send(s) · {} agent(s) reached · {} acknowledged · {} injection(s)",
        s.broadcasts, s.reached, s.broadcast_acknowledged, s.injections
    );
    // **Why there is no rate, said on the page rather than left as an inference** (D91's shape: a
    // reader cannot tell a refused denominator from a forgotten one). Written as three whole lines
    // rather than one wrapped literal — M24 is the case where a wrapped string kept its indentation
    // and rendered a run of spaces that every `contains` assertion straddled.
    let _ = writeln!(
        out,
        "          reach has no rate — `@project` addresses a place, not a subscriber"
    );
    let _ = writeln!(
        out,
        "          list (D17), so there is nothing this can honestly be divided by."
    );

    out
}

/// The receipt, as a program reads it.
///
/// **The `let Sent { … }` below has no `..`, so adding a field is a compile error here.** That is
/// D127's seam guard, and it is strictly stronger than a test: [`crate::status`] grew two renderers
/// of one struct and the JSON fell four fields behind, because two sessions each updated the
/// renderer they happened to be looking at. A field deliberately withheld from the contract is
/// spelled `field: _` — greppable, and a decision somebody made rather than one nobody noticed.
///
/// **Adding a command adds no key to any existing object, so `JSON_CONTRACT` does not move** (D117:
/// it moves when a field a reader could be relying on changes meaning or leaves).
pub fn render_json(s: &Sent) -> serde_json::Value {
    let Sent {
        direct,
        handed,
        fetched,
        unoffered,
        acknowledged,
        broadcasts,
        reached,
        broadcast_acknowledged,
        injections,
    } = *s;
    serde_json::json!({
        "direct_total": direct,
        "direct_handed": handed,
        "direct_fetched": fetched,
        "direct_unoffered": unoffered,
        "direct_acknowledged": acknowledged,
        "broadcast_total": broadcasts,
        "broadcast_reached": reached,
        "broadcast_acknowledged": broadcast_acknowledged,
        "broadcast_injections": injections,
        // **No `broadcast_rate` key, and no roster to compute one from.** Its absence is the
        // contract: the human page refuses the division, and shipping the denominator here would
        // be the same mistake made one layer down, where nothing renders a sentence explaining it.
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn sent() -> Sent {
        Sent {
            direct: 51,
            handed: 49,
            fetched: 1,
            unoffered: 1,
            acknowledged: 50,
            broadcasts: 38,
            reached: 14,
            broadcast_acknowledged: 102,
            injections: 214,
        }
    }

    /// A board with one sender, one recipient, and one message in each of the three states.
    fn board() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory board");
        crate::db::migrate(&mut conn, std::path::Path::new(":memory:")).expect("schema");
        conn.execute_batch(
            // `cwd`, not `root`: the column predates D20's rename and `identity::Identity` is the
            // only place the field is called `root`. Getting this wrong is what the fixture caught.
            "INSERT INTO agents (id, name, project, cwd, first_seen, last_seen) VALUES
               ('me','me','p','/p',1,1), ('you','you','p','/p',1,1);
             INSERT INTO messages (ext_id, ts, from_agent, from_proj, to_agent, to_proj, kind,
                                   subject, body, thread_id)
               VALUES ('a', 1, 'me', 'p', 'you', 'p', 'note', 's', 'b', 't1'),
                      ('b', 2, 'me', 'p', 'you', 'p', 'note', 's', 'b', 't2'),
                      ('c', 3, 'me', 'p', 'you', 'p', 'note', 's', 'b', 't3'),
                      ('d', 4, 'me', 'p', NULL,  'p', 'note', 's', 'b', 't4');
             -- handed: a hook delivered it. fetched: read with no delivery. third: no row at all.
             INSERT INTO reads (msg_id, agent, delivered_at, read_at, attempts) VALUES
               ((SELECT id FROM messages WHERE ext_id='a'), 'you', 10, 11, 3),
               ((SELECT id FROM messages WHERE ext_id='b'), 'you', NULL, 12, 0),
               ((SELECT id FROM messages WHERE ext_id='d'), 'you', 13, 14, 5);",
        )
        .expect("seed");
        conn
    }

    /// **The claim the whole three-state model rests on**, and the one thing a fixture built to
    /// match the code could still get right while the SQL is wrong — so it is checked as an
    /// identity over `gather`'s own output rather than against hand-written expectations.
    #[test]
    fn the_three_states_partition_the_direct_total() {
        let s = gather(&board(), "me").expect("gather");
        assert_eq!(
            s.handed + s.fetched + s.unoffered,
            s.direct,
            "the three states must sum to the direct total, or one message is in two of them \
             (or none): {s:?}"
        );
        assert_eq!((s.handed, s.fetched, s.unoffered), (1, 1, 1), "{s:?}");
    }

    /// D127 from the sender's side: a message read without ever being delivered must not be counted
    /// as handed over, and must still be counted as acknowledged.
    #[test]
    fn a_message_read_without_delivery_is_fetched_and_not_handed() {
        let s = gather(&board(), "me").expect("gather");
        assert_eq!(s.fetched, 1, "the read-without-delivery row: {s:?}");
        assert_eq!(
            s.acknowledged, 2,
            "both the handed and the fetched message carry a read_at: {s:?}"
        );
        assert!(
            s.acknowledged > s.handed,
            "acknowledged may legitimately exceed handed — that is the whole finding: {s:?}"
        );
    }

    /// A broadcast contributes to the broadcast counts and to none of the direct ones.
    #[test]
    fn a_broadcast_is_not_counted_as_direct_mail() {
        let s = gather(&board(), "me").expect("gather");
        assert_eq!(s.direct, 3, "the fourth message is a broadcast: {s:?}");
        assert_eq!((s.broadcasts, s.reached, s.injections), (1, 1, 5), "{s:?}");
    }

    /// **A truth table, not a needle list** (M27). Asserting only that no rate appears would have an
    /// unproven premise — if the broadcast block stopped rendering entirely the absence would still
    /// hold. The presence row is what proves the renderer got that far.
    #[test]
    fn the_broadcast_block_reports_reach_and_refuses_a_rate() {
        let out = render(&sent());
        assert!(
            out.contains("14 agent(s) reached"),
            "the numerator must reach the page, or the absence below proves nothing:\n{out}"
        );
        assert!(
            out.contains("no rate"),
            "the refusal must be stated, not inferred:\n{out}"
        );
        assert!(
            out.contains("not a subscriber"),
            "the refusal must give its reason, or it reads as an omission (D91):\n{out}"
        );
        // **The absence that matters, and the presence rows above are what license it** (M27). A
        // `·` after the reach count is how every rate in this file is spelled, so its absence on
        // that line is the check — asserting on one arbitrary quotient would miss every other.
        let reach_line = out
            .lines()
            .find(|l| l.starts_with("broadcast "))
            .expect("the broadcast line rendered");
        assert!(
            !reach_line.contains('%'),
            "a percentage reached the broadcast line — the exact division this module \
             refuses:\n{reach_line}"
        );
    }

    /// Every line renders on an all-zero receipt. A count guard would make a healthy quiet session
    /// and an unwired counter print identically (D89), which is what the unconditional writes above
    /// are for — this is the assertion that reddens if somebody adds an `if`.
    #[test]
    fn an_empty_receipt_still_renders_every_line() {
        let out = render(&Sent::default());
        for needle in [
            "sent      0 direct",
            "0 handed over by a hook",
            "0 fetched by the recipient",
            "0 never reached them",
            "broadcast 0 send(s)",
            "no rate",
        ] {
            assert!(out.contains(needle), "missing {needle:?} at zero:\n{out}");
        }
        // `rate` renders an empty denominator as `—`, never as `0%`: a sender who has sent nothing
        // and one whose mail nobody read must not print the same thing (D74).
        assert!(
            out.contains("0 of 0 acknowledged · —"),
            "an empty denominator must render as — rather than 0%:\n{out}"
        );
    }

    #[test]
    fn a_full_receipt_holds_the_rendered_shape() {
        crate::assert_rendered_shape("sent", &render(&sent()));
        crate::assert_rendered_shape("sent empty", &render(&Sent::default()));
    }

    /// The two renderers must not drift: every field on the struct reaches the JSON. The exhaustive
    /// destructure makes that a compile error, and this pins the key names a parser binds to.
    #[test]
    fn the_json_carries_every_count_and_no_derived_rate() {
        let j = render_json(&sent());
        assert_eq!(j["direct_total"], 51);
        assert_eq!(j["direct_handed"], 49);
        assert_eq!(j["direct_fetched"], 1);
        assert_eq!(j["direct_unoffered"], 1);
        assert_eq!(j["direct_acknowledged"], 50);
        assert_eq!(j["broadcast_total"], 38);
        assert_eq!(j["broadcast_reached"], 14);
        assert_eq!(j["broadcast_acknowledged"], 102);
        assert_eq!(j["broadcast_injections"], 214);
        for absent in ["broadcast_rate", "roster_ever", "roster"] {
            assert!(
                j.get(absent).is_none(),
                "{absent:?} reached the contract — a parser can now compute the division the \
                 human page refuses: {j}"
            );
        }
    }
}

//! What the board is actually doing — the receipt for the three surfaces that had none.
//!
//! `amb memory status` has existed since D42 and carries a withdrawal verdict, a measurement
//! window (D87), a search ledger (D91) and a per-force split. Messaging, claims and delivery — the
//! three things this tool exists to do — had **no ledger of any kind**. `doctor`'s `deliver` row
//! reads `max(delivered_at)` and answers *is delivery firing*; nothing answered *is it working*.
//!
//! **The cost of that was concrete and is why this module exists.** Two interventions shipped on
//! measured grounds within a week: the primer taught `--kind` (U9) and taught `amb claim` (D58,
//! D91). Reading the first receipt took copying `board.db` and hand-writing SQL — it worked, 1 of
//! 12 senders before against 5 of 9 after. The second could not be read at all: 25 declared claims
//! before and 0 after, with the agent population falling 15 to 6 over the same window, and nothing
//! anywhere recording whether a session ever saw the line. "Nobody wanted to declare a claim" and
//! "nobody read that far" print the same zero.
//!
//! # The four questions this had to answer before it could be written
//!
//! CLAUDE.md's ratio rule is not decoration here; it is the specification for this file.
//!
//! 1. **What is one unit of the denominator, on each side?** Two different units are reported and
//!    they are never divided into each other. An *offer* is one `(message, agent)` pair — a row in
//!    `reads`. A *delivery* is one injection into one session's context — `reads.attempts`. A
//!    broadcast to five agents offered three times each is 5 offers and 15 deliveries, and calling
//!    either "messages delivered" would be wrong in a different direction.
//!
//! 2. **Does the denominator rise every time the cost is paid?** For rows, no — `reads` is
//!    `PRIMARY KEY (msg_id, agent)`, so a message injected ten times into one session records
//!    **one** row. That key is right for the question the table was built for (*was this put in
//!    front of them*) and wrong for *what did it cost*, exactly as D77 predicts. Measured on this
//!    board the day the module landed: 599 rows against 1,025 attempts, so the row count
//!    understates the real delivery cost by 71%. Both numbers are printed side by side and the
//!    gap between them is the point.
//!
//! 3. **What is recorded on the unhappy path?** [`Board::dead`] and [`Board::unoffered`] exist for
//!    this and for no other reason. A message that was offered [`MAX_OFFERS`] times and never
//!    acknowledged, and a message addressed to somebody who never came back, are the two ways
//!    delivery fails — and both were previously indistinguishable from a quiet board. D89's rule:
//!    a ledger that only writes on success reports a broken mechanism as an idle one.
//!
//! 4. **What can move this number, and can anyone reach it?** [`Board::declared`] moves only when
//!    somebody runs `amb claim`. The render says so in as many words, because D91 is the case
//!    where a counter watched a flag nobody had been told about and the resulting zero was a
//!    verdict by construction. Here the surface *is* reachable — the primer teaches it — so the
//!    zero would be real. Saying which is which is the whole difference.
//!
//! # What this deliberately does not do
//!
//! **No verdict, and no threshold.** D59 retires the memory injection layer on a number, and that
//! is a decision someone made about a feature that was explicitly experimental. Messaging and
//! claims are the product. A withdrawal condition on them would be theatre, and D95 records what a
//! stated threshold that cannot fire does to the next reader.

use crate::error::{Result, sql};
use crate::messages::MAX_OFFERS;
use rusqlite::Connection;

/// Counts over the whole board, with each field's unit named where it is not obvious.
///
/// Every field is a plain count read from one query. There is no arithmetic here on purpose: the
/// division is done in [`render`], where the two units are next to each other and a reader can see
/// which is which.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Board {
    /// Rows in `messages`.
    pub messages: i64,
    /// Distinct senders — the population any per-agent rate is really over.
    pub senders: i64,
    /// Messages whose sender set an explicit `--kind`. U9's intervention, still readable.
    pub explicit_kind: i64,
    /// Senders who have *ever* set one. The per-agent form, which is the one that survived U9's
    /// population confound: message counts are dominated by whoever talks most.
    pub kind_senders: i64,

    /// **One `(message, agent)` pair that has been offered at least once.** Rows in `reads`.
    pub offers: i64,
    /// **One injection into one session's context.** `sum(reads.attempts)`, and the number that
    /// rises every time the cost is actually paid (question 2).
    pub deliveries: i64,
    /// Offers the recipient acknowledged with `amb read`.
    pub acknowledged: i64,
    /// **The unhappy path.** Offered [`MAX_OFFERS`] times and never acknowledged — D6's
    /// dead-letter condition, which nothing has ever reported.
    pub dead: i64,
    /// **The other unhappy path.** Addressed to one agent and never offered to them at all,
    /// because that session never came back. Distinct from `dead`: nothing was spent here.
    pub unoffered: i64,

    /// **`@@` sends, and what they cost** (D126). Global broadcasts are the one scope whose reader
    /// may be working on something unrelated, so the interesting number is not how many were sent
    /// but how many injections they bought across how many projects.
    pub globals: i64,
    /// Injections spent delivering those globals — `sum(attempts)`, the unit that rises every time
    /// the cost is paid, never the row count.
    pub global_cost: i64,
    /// Projects other than the sender's that a global has actually been injected into. **The
    /// number D126's withdrawal condition is read off**, and the reason this exists: that
    /// condition shipped naming a query to run by hand, which is a stated condition nothing can
    /// evaluate — D95's defect, and it was written into the decision that documents it.
    pub global_reach: i64,

    /// Claims taken by `amb claim` — the proactive half of D5.
    pub declared: i64,
    /// Claims recorded by the `PostToolUse` hook as files were edited.
    pub observed: i64,
    /// Distinct `(agent, path, holder)` conflicts that have ever been surfaced.
    pub conflicts: i64,
    /// Times a conflict notice was actually shown. Same rows-versus-attempts split as delivery,
    /// and it is `claim_notices.count` that carries the cost.
    pub conflict_tells: i64,
}

/// Read every count in one pass per table.
pub fn gather(conn: &Connection) -> Result<Board> {
    let one = |q: &str| -> Result<i64> {
        conn.query_row(q, [], |r| r.get::<_, Option<i64>>(0))
            .map(|v| v.unwrap_or(0))
            .map_err(sql("counting the board"))
    };
    Ok(Board {
        messages: one("SELECT count(*) FROM messages")?,
        senders: one("SELECT count(DISTINCT from_agent) FROM messages")?,
        explicit_kind: one("SELECT count(*) FROM messages WHERE kind <> 'note'")?,
        kind_senders: one("SELECT count(DISTINCT from_agent) FROM messages WHERE kind <> 'note'")?,
        offers: one("SELECT count(*) FROM reads WHERE delivered_at IS NOT NULL")?,
        deliveries: one("SELECT sum(attempts) FROM reads")?,
        acknowledged: one("SELECT count(*) FROM reads WHERE read_at IS NOT NULL")?,
        dead: one(&format!(
            "SELECT count(*) FROM reads WHERE read_at IS NULL AND attempts >= {MAX_OFFERS}"
        ))?,
        // Direct mail only. A broadcast has no one recipient it can be said to have missed, so
        // counting it here would inflate this with messages nobody was ever owed.
        unoffered: one("SELECT count(*) FROM messages m
             WHERE m.to_agent IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM reads r WHERE r.msg_id = m.id)")?,
        globals: one("SELECT count(*) FROM messages WHERE to_agent IS NULL AND to_proj IS NULL")?,
        global_cost: one(
            "SELECT sum(r.attempts) FROM reads r JOIN messages m ON m.id = r.msg_id
              WHERE m.to_agent IS NULL AND m.to_proj IS NULL",
        )?,
        // **Joined to `agents` rather than counted off `messages`, because the question is where a
        // global LANDED and not where it was aimed.** `from_proj <> a.project` compares the
        // sender's project at send time against the reader's now; that is the honest available
        // comparison, and it is why this counts distinct projects rather than deliveries — a
        // reader's `agents.project` is overwritten on every registration, so per-row attribution
        // to a project is not a thing this schema can support.
        global_reach: one("SELECT count(DISTINCT a.project) FROM reads r
               JOIN messages m ON m.id = r.msg_id
               JOIN agents a ON a.id = r.agent
              WHERE m.to_agent IS NULL AND m.to_proj IS NULL
                AND r.delivered_at IS NOT NULL
                AND a.project <> m.from_proj")?,
        declared: one("SELECT count(*) FROM claims WHERE source = 'declared'")?,
        observed: one("SELECT count(*) FROM claims WHERE source = 'observed'")?,
        conflicts: one("SELECT count(*) FROM claim_notices")?,
        conflict_tells: one("SELECT sum(count) FROM claim_notices")?,
    })
}

/// A percentage that says `—` rather than `0.0` when there is nothing to divide.
///
/// **`0/0` is not zero, and printing it as zero is this project's catalogued failure.** D74's
/// whole subject is a ratio read as a verdict when its denominator described nothing; a lane that
/// has never run and a lane that ran and never succeeded must not render identically.
fn rate(numerator: i64, denominator: i64) -> String {
    if denominator <= 0 {
        "—".to_string()
    } else {
        format!("{:.0}%", 100.0 * numerator as f64 / denominator as f64)
    }
}

/// The receipt, as a person reads it.
///
/// Pure, so the exact bytes are testable without a board — the same split `delivery::render` uses
/// and for the same reason.
pub fn render(b: &Board) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();

    let _ = writeln!(s, "messages  {} from {} sender(s)", b.messages, b.senders);
    let _ = writeln!(
        s,
        "  kind      {} of {} message(s) set one · {} of {} sender(s) ever have",
        b.explicit_kind, b.messages, b.kind_senders, b.senders
    );

    // **`@@`'s reach, reported as three numbers that are never divided into each other** (D126).
    // The sends are what a person chooses; the injections are what everyone else pays; the project
    // count is who paid it. A single ratio over these would be exactly D74's mistake — "injections
    // per send" reads as a cost per message and is really a fact about how many sessions happened
    // to be open.
    //
    // **Rendered unconditionally, and that is deliberate.** M27 measured this module at 52/92 with
    // *thirty-seven of forty survivors* sitting on the `if` deciding whether a line renders at all,
    // ten of them the literal `x > 0` -> `x >= 0`. A count guard has that relaxation and a
    // rendered-always line has none, so the cheapest defence here is not to write the guard. The
    // zero row is also the informative one: `0 global(s)` on a board that used to send them is the
    // signal D126's withdrawal condition is looking for.
    let _ = writeln!(
        s,
        "global    {} `@@` send(s) · {} injection(s) · reached {} other project(s)",
        b.globals, b.global_cost, b.global_reach
    );

    // **The two units, adjacent and never divided into each other** (question 1). An offer is a
    // `(message, agent)` pair; a delivery is one injection. The gap between them is what a row
    // count hides, so it is stated rather than left to be noticed.
    let _ = writeln!(s, "delivery  {} offer(s) to a recipient", b.offers);
    let _ = writeln!(
        s,
        "  cost      {} injection(s) — the offers above were each made {} time(s) on average",
        b.deliveries,
        if b.offers > 0 {
            format!("{:.1}", b.deliveries as f64 / b.offers as f64)
        } else {
            "0".to_string()
        }
    );
    let _ = writeln!(
        s,
        "  read      {} of {} offer(s) acknowledged · {}",
        b.acknowledged,
        b.offers,
        rate(b.acknowledged, b.offers)
    );

    // The unhappy path, and it prints even at zero. A dead-letter count that appears only when
    // non-zero is one a reader cannot distinguish from a metric that was never wired up (D89).
    let _ = writeln!(
        s,
        "  dead      {} offered {MAX_OFFERS} time(s) and never acknowledged",
        b.dead
    );
    let _ = writeln!(
        s,
        "  unoffered {} direct message(s) whose recipient never came back",
        b.unoffered
    );

    let total_claims = b.declared + b.observed;
    let _ = writeln!(
        s,
        "claims    {} declared · {} observed · {} of {} taken deliberately",
        b.declared,
        b.observed,
        rate(b.declared, total_claims),
        total_claims
    );
    let _ = writeln!(
        s,
        "  conflict  {} surfaced, told {} time(s)",
        b.conflicts, b.conflict_tells
    );

    // **What this cannot answer, said plainly rather than left as an inference.** D91 is the case
    // where a number was read as a verdict on a capability when it only ever watched an
    // unreachable flag. `declared` is not that — `amb claim` is in the primer every session
    // reads — but the reader cannot know which situation they are in unless the instrument says.
    if b.declared == 0 && b.observed > 0 {
        let _ = writeln!(
            s,
            "  ! nothing has been declared, and the primer does teach `amb claim` — so this is a \
             real zero rather than an unreachable one. Auto-claiming already covers the ground; \
             declaring buys the intent string and the warning before the edit, not the claim"
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        Board {
            messages: 425,
            senders: 13,
            explicit_kind: 16,
            kind_senders: 5,
            offers: 599,
            deliveries: 1025,
            acknowledged: 589,
            dead: 0,
            unoffered: 2,
            globals: 15,
            global_cost: 198,
            global_reach: 12,
            declared: 25,
            observed: 442,
            conflicts: 4,
            conflict_tells: 9,
        }
    }

    /// The two units must both reach the page, because the whole reason this module exists is that
    /// one of them was silently standing in for the other (D77, CLAUDE.md's question 2).
    ///
    /// Measured on the real board the day this landed: 599 rows against 1,025 attempts. A render
    /// that printed only the row count would understate delivery cost by 71% and read as precise.
    #[test]
    fn both_delivery_units_are_printed_and_neither_is_divided_into_the_other() {
        let out = render(&board());
        assert!(out.contains("599 offer(s)"), "the pair count: {out}");
        assert!(out.contains("1025 injection(s)"), "the cost: {out}");
        assert!(
            out.contains("1.7 time(s) on average"),
            "and the gap between them, stated rather than left to be noticed: {out}"
        );
        crate::assert_rendered_shape("status", &out);
    }

    /// `@@`'s three numbers reach the page, and none of them is divided into another (D126).
    ///
    /// **The zero row is the point of the test, not a corner case.** D126's withdrawal condition
    /// is read off this line: if `@@` traffic does not fall after both ends are told, the
    /// awareness route failed and the flag argument reopens. A line that rendered only when
    /// `globals > 0` could not express "this board has stopped sending them", which is the exact
    /// signal being watched for — and it is the `x > 0` -> `x >= 0` relaxation M27 found thirty-
    /// seven of forty survivors sitting on, in this file.
    #[test]
    fn the_global_reach_line_is_printed_even_when_it_is_zero() {
        let busy = render(&board());
        assert!(busy.contains("15 `@@` send(s)"), "the sends: {busy}");
        assert!(
            busy.contains("198 injection(s)"),
            "what everyone else paid: {busy}"
        );
        assert!(
            busy.contains("reached 12 other project(s)"),
            "and who paid it: {busy}"
        );
        // No ratio over these three. "injections per send" would read as a cost per message and is
        // really a fact about how many sessions happened to be open — D74's mistake exactly.
        assert!(
            !busy.contains("13.2") && !busy.contains("per send"),
            "the three numbers must not be divided into each other: {busy}"
        );

        // The row that proves the line is unconditional. A board that used to broadcast and has
        // stopped must say so rather than fall silent.
        let quiet = Board {
            globals: 0,
            global_cost: 0,
            global_reach: 0,
            ..board()
        };
        let out = render(&quiet);
        assert!(
            out.contains("0 `@@` send(s)"),
            "a board that stopped broadcasting must say so, not go quiet: {out}"
        );
        assert!(
            out.contains("reached 0 other project(s)"),
            "and the reach must render at zero too: {out}"
        );
        crate::assert_rendered_shape("status quiet", &out);
    }

    /// **A truth table over the unhappy path, and the zero row is the one that matters.**
    ///
    /// D89: a ledger that only writes on success reports a broken mechanism as an idle one. If
    /// `dead` rendered only when non-zero, a reader could not tell a healthy board from a counter
    /// nobody wired up — which is the exact confusion `unprompted: 0` caused before D91.
    #[test]
    fn the_dead_letter_count_is_printed_even_when_it_is_zero() {
        let healthy = render(&board());
        assert!(
            healthy.contains("dead      0 offered"),
            "zero is stated, not omitted: {healthy}"
        );

        let mut broken = board();
        broken.dead = 7;
        broken.unoffered = 3;
        let out = render(&broken);
        assert!(out.contains("dead      7 offered"), "{out}");
        assert!(out.contains("3 direct message(s)"), "{out}");
        crate::assert_rendered_shape("status dead", &out);
    }

    /// `0/0` renders as `—` and never as `0%`.
    ///
    /// A lane that has never run and a lane that ran and always failed are different facts, and
    /// D74 is the entry about what happens when a ratio with a meaningless denominator is read as
    /// a verdict. The `0%` row is the presence assertion that keeps the `—` row honest: without
    /// it, a `rate` that returned `—` unconditionally would pass.
    #[test]
    fn an_empty_denominator_renders_as_no_answer_rather_than_as_zero_percent() {
        assert_eq!(rate(0, 0), "—", "nothing offered is not nought percent");
        assert_eq!(rate(0, 10), "0%", "but nothing read out of ten really is");
        assert_eq!(rate(5, 10), "50%");

        let empty = render(&Board::default());
        assert!(
            empty.contains("0 of 0 offer(s) acknowledged · —"),
            "an untouched board says it has no answer: {empty}"
        );
        crate::assert_rendered_shape("status empty", &empty);
    }

    /// The caveat fires only when it is true, and says which of D91's two situations this is.
    /// **The whole claims line, not needles inside it** (M24, M27).
    ///
    /// Found by mutation, and it is the failure this module's own header warns about: `total_claims
    /// = declared + observed` mutated to `-` and **survived**. Every assertion here passed, because
    /// they all checked the words around the numbers — `25 declared` is still true when the total
    /// beside it is `-417` and the percentage has degraded to `—` because [`rate`] saw a negative
    /// denominator. A count that reaches a person has to be asserted as a count.
    ///
    /// One `assert_eq!` on the rendered line rather than four `contains` calls, because
    /// `contains` describes points and this defect lived between them: it kills `+` -> `-`
    /// (467 becomes -417) and `+` -> `*` (467 becomes 11,050) in one row, and any future
    /// arithmetic edit on that line as well.
    #[test]
    fn the_claims_line_states_its_own_arithmetic() {
        let line = render(&board())
            .lines()
            .find(|l| l.starts_with("claims"))
            .expect("the claims line renders")
            .to_string();
        assert_eq!(
            line, "claims    25 declared · 442 observed · 5% of 467 taken deliberately",
            "declared + observed is the denominator, and 25/467 is 5%"
        );
    }

    #[test]
    fn the_declared_caveat_appears_only_on_a_board_that_observes_and_never_declares() {
        let mixed = render(&board());
        assert!(
            !mixed.contains("real zero"),
            "a board with declared claims needs no explanation: {mixed}"
        );
        assert!(
            mixed.contains("25 declared"),
            "and states them positively, so the absence above has a proven premise: {mixed}"
        );

        let mut never = board();
        never.declared = 0;
        let out = render(&never);
        assert!(
            out.contains("real zero rather than an unreachable one"),
            "it names which kind of zero this is (D91): {out}"
        );

        // Neither surface used at all: no claim of either sort. Saying "this is a real zero"
        // there would assert something about a mechanism that has had no opportunity to run.
        let quiet = Board {
            messages: 3,
            ..Default::default()
        };
        assert!(
            !render(&quiet).contains("real zero"),
            "an untouched board is not evidence about the declare surface"
        );
    }
}

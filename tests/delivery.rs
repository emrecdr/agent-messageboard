//! End-to-end delivery tests.
//!
//! These exist because of a real defect: `send` wrote a display *name* into `to_agent` while
//! `inbox` matched a session *id*, so every direct message was accepted and silently never
//! delivered. Twenty unit tests passed throughout, because all of them tested pure functions
//! and none tested delivery. Each test below would have failed against that build.

use amb::address;
use amb::db;
use amb::identity::Identity;
use amb::messages::{self, Outgoing, Recipient};
use rusqlite::Connection;

fn board() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().expect("temp dir");
    let conn = db::open_at(&dir.path().join("board.db")).expect("open board");
    (dir, conn)
}

fn agent(conn: &Connection, id: &str, name: &str, project: &str) -> Identity {
    let who = Identity {
        id: id.into(),
        name: name.into(),
        project: project.into(),
        root: format!("/tmp/{project}"),
    };
    amb::identity::touch(conn, &who, Some(name)).expect("register");
    who
}

/// Send addressed by a human-written string, exactly as the CLI does.
fn send_to(conn: &mut Connection, from: &Identity, to: &str, subject: &str) -> i64 {
    let addr = address::parse(to).expect("address parses");
    let rcpt = messages::resolve_recipient(conn, &addr, from).expect("recipient resolves");
    messages::send(
        conn,
        from,
        &Outgoing {
            to: &rcpt,
            subject,
            body: "b",
            kind: "note",
            thread: None,
            ext_id: None,
        },
    )
    .expect("send")
}

fn subjects(conn: &Connection, who: &Identity) -> Vec<String> {
    messages::inbox(conn, who, true)
        .expect("inbox")
        .into_iter()
        .map(|m| m.subject)
        .collect()
}

#[test]
fn a_direct_message_reaches_the_named_recipient() {
    // The regression test. Against the defective build this returned an empty inbox.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");

    send_to(&mut c, &alice, "bob", "direct");
    assert_eq!(
        subjects(&c, &bob),
        vec!["direct"],
        "bob must receive a message addressed to him"
    );
}

#[test]
fn a_direct_message_reaches_nobody_else() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let _bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");

    send_to(&mut c, &alice, "bob", "for bob only");
    assert!(
        subjects(&c, &carol).is_empty(),
        "carol must not see a message addressed to bob"
    );
}

#[test]
fn an_agent_can_be_addressed_by_short_ref_as_well_as_name() {
    let (_d, mut c) = board();
    let alice = agent(&c, "c0a251aa-1111", "alice", "nest");
    let bob = agent(&c, "d39e63bb-2222", "bob", "nest");

    send_to(&mut c, &alice, "d39e63", "by ref");
    assert_eq!(
        subjects(&c, &bob),
        vec!["by ref"],
        "a short ref must resolve like a name"
    );
}

#[test]
fn a_project_broadcast_reaches_everyone_in_that_project() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");

    send_to(&mut c, &alice, "@", "all hands");
    assert_eq!(subjects(&c, &bob), vec!["all hands"]);
    assert_eq!(subjects(&c, &carol), vec!["all hands"]);
}

#[test]
fn a_project_broadcast_reaches_an_agent_that_registered_afterwards() {
    // This is the property that makes `@project` address a *place* rather than a set of
    // currently-connected processes. A queue bound at publish time could not do it.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    send_to(&mut c, &alice, "@", "sent before bob existed");

    let bob = agent(&c, "uuid-bob", "bob", "nest");
    assert_eq!(
        subjects(&c, &bob),
        vec!["sent before bob existed"],
        "a late joiner must still receive an earlier broadcast"
    );
}

#[test]
fn a_project_broadcast_stays_inside_its_project() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let outsider = agent(&c, "uuid-dan", "dan", "mobile");

    send_to(&mut c, &alice, "@", "nest only");
    assert!(
        subjects(&c, &outsider).is_empty(),
        "a project broadcast must not leak to another"
    );
}

#[test]
fn a_global_broadcast_reaches_every_project() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let dan = agent(&c, "uuid-dan", "dan", "mobile");
    let eve = agent(&c, "uuid-eve", "eve", "thirdproj");

    send_to(&mut c, &alice, "@@", "everyone everywhere");
    for who in [&bob, &dan, &eve] {
        assert_eq!(
            subjects(&c, who),
            vec!["everyone everywhere"],
            "{} must receive a global broadcast",
            who.name
        );
    }
}

#[test]
fn a_direct_message_crosses_projects() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let dan = agent(&c, "uuid-dan", "dan", "mobile");

    send_to(&mut c, &alice, "dan@mobile", "cross project");
    assert_eq!(subjects(&c, &dan), vec!["cross project"]);
}

#[test]
fn an_unknown_recipient_is_an_error_not_a_stored_message() {
    let (_d, c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");

    let addr = address::parse("nobody").expect("parses");
    let err = messages::resolve_recipient(&c, &addr, &alice).expect_err("must not resolve");
    assert!(matches!(err, amb::Error::NoSuchAgent { .. }), "got {err:?}");

    let stored: i64 = c
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        stored, 0,
        "nothing may be stored for an unresolvable recipient"
    );
}

#[test]
fn a_name_cannot_be_taken_twice_in_one_project() {
    let (_d, c) = board();
    let _alice = agent(&c, "uuid-alice", "alice", "nest");

    let impostor = Identity {
        id: "uuid-other".into(),
        name: "alice".into(),
        project: "nest".into(),
        root: "/tmp/nest".into(),
    };
    let err = amb::identity::touch(&c, &impostor, Some("alice")).expect_err("must be refused");
    assert!(matches!(err, amb::Error::NameTaken { .. }), "got {err:?}");
}

#[test]
fn an_auto_generated_name_widens_rather_than_locking_a_session_out() {
    // D12 promises registration is optional and that forgetting it costs "only a less readable
    // name". It cost more than that: `default_name` uses a six-character ref, so two sessions
    // sharing that prefix collided on `UNIQUE(project, name)` and the second was refused by
    // *every* command — it could not read its own inbox. D32.
    let (_d, c) = board();
    let twin = |id: &str| Identity {
        id: id.into(),
        name: amb::identity::default_name("nest", id),
        project: "nest".into(),
        root: "/tmp/nest".into(),
    };
    let first = twin("abc123-1111-1111");
    let second = twin("abc123-2222-2222");
    assert_eq!(
        first.name, second.name,
        "the premise: both default to the same name"
    );

    let a = amb::identity::touch(&c, &first, None).expect("first registers");
    let b = amb::identity::touch(&c, &second, None)
        .expect("the second must not be locked out of the board");
    assert_ne!(a, b, "it settles for a longer, less readable name");
    assert!(
        b.starts_with("nest-abc123"),
        "still recognisably itself: {b}"
    );

    // And it is a real, addressable agent rather than a half-registered one.
    let mut c = c;
    let carol = agent(&c, "uuid-carol", "carol", "nest");
    let addr = address::parse(&b).expect("its name parses");
    let rcpt = messages::resolve_recipient(&c, &addr, &carol).expect("and resolves");
    assert_eq!(rcpt.agent_id.as_deref(), Some("abc123-2222-2222"));
    messages::send(
        &mut c,
        &carol,
        &Outgoing {
            to: &rcpt,
            subject: "reachable",
            body: "b",
            kind: "note",
            thread: None,
            ext_id: None,
        },
    )
    .expect("send");
    assert_eq!(subjects(&c, &second), vec!["reachable"]);
}

#[test]
fn an_explicit_name_clash_is_still_an_error() {
    // The widening must not soften D18: a name a human chose must clash loudly, to the agent
    // that can still choose another.
    let (_d, c) = board();
    let _first = agent(&c, "uuid-alice", "alice", "nest");
    let impostor = Identity {
        id: "uuid-other".into(),
        name: "alice".into(),
        project: "nest".into(),
        root: "/tmp/nest".into(),
    };
    assert!(matches!(
        amb::identity::touch(&c, &impostor, Some("alice")),
        Err(amb::Error::NameTaken { .. })
    ));
}

#[test]
fn the_same_name_is_free_in_a_different_project() {
    let (_d, c) = board();
    let _a = agent(&c, "uuid-alice", "alice", "nest");
    let _b = agent(&c, "uuid-alice2", "alice", "mobile");
    // Reaching here without a panic is the assertion: uniqueness is per project, not global.
}

#[test]
fn each_recipient_consumes_a_broadcast_independently() {
    // One row, per-agent read state. Bob acknowledging must not consume it for carol.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");

    let id = send_to(&mut c, &alice, "@", "shared");
    messages::mark_read(&c, &bob, id).expect("bob reads");

    assert!(subjects(&c, &bob).is_empty(), "bob has acknowledged it");
    assert_eq!(subjects(&c, &carol), vec!["shared"], "carol has not");
}

#[test]
fn resending_the_same_ext_id_delivers_once() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let to = Recipient {
        agent_id: Some(bob.id.clone()),
        project: Some("nest".into()),
    };

    let out = Outgoing {
        to: &to,
        subject: "once",
        body: "b",
        kind: "note",
        thread: None,
        ext_id: Some("stable-id-1"),
    };
    let first = messages::send(&mut c, &alice, &out).expect("first send");
    let second = messages::send(&mut c, &alice, &out).expect("resend");

    assert_eq!(first, second, "a resend must return the original id (D6)");
    assert_eq!(subjects(&c, &bob).len(), 1, "and must deliver exactly once");
}

#[test]
fn two_senders_may_choose_the_same_ext_id_without_one_swallowing_the_other() {
    // D22, and it is D18's failure wearing a different hat: the second send reported
    // `{"sent":1}` — the *first* sender's message id — wrote nothing, and left an inbox that
    // looked empty because it was. Natural idempotency keys are task-shaped (`task-1`,
    // `handoff`), not agent-shaped, so the collision is likely rather than exotic.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");
    let to_carol = Recipient {
        agent_id: Some(carol.id.clone()),
        project: Some("nest".into()),
    };

    let from_alice = messages::send(
        &mut c,
        &alice,
        &Outgoing {
            to: &to_carol,
            subject: "alice's message",
            body: "b",
            kind: "note",
            thread: None,
            ext_id: Some("task-1"),
        },
    )
    .expect("alice sends");
    let from_bob = messages::send(
        &mut c,
        &bob,
        &Outgoing {
            to: &to_carol,
            subject: "bob's message",
            body: "b",
            kind: "note",
            thread: None,
            ext_id: Some("task-1"),
        },
    )
    .expect("bob sends");

    assert_ne!(
        from_alice, from_bob,
        "the same key from a different sender is a different message"
    );
    let mut got = subjects(&c, &carol);
    got.sort();
    assert_eq!(
        got,
        ["alice's message", "bob's message"],
        "both must arrive; one used to be swallowed with no error"
    );
}

#[test]
fn one_sender_reusing_its_own_ext_id_still_delivers_once() {
    // The other half of D22: scoping must not weaken D6's idempotency for the sender itself.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let to = Recipient {
        agent_id: Some(bob.id.clone()),
        project: Some("nest".into()),
    };
    let out = Outgoing {
        to: &to,
        subject: "once",
        body: "b",
        kind: "note",
        thread: None,
        ext_id: Some("task-1"),
    };
    assert_eq!(
        messages::send(&mut c, &alice, &out).expect("first"),
        messages::send(&mut c, &alice, &out).expect("resend"),
        "a resend by the same sender still returns the original id"
    );
    assert_eq!(subjects(&c, &bob).len(), 1);
}

#[test]
fn a_message_stops_being_offered_after_enough_unacknowledged_attempts() {
    // D6's dead-letter path, which existed only as a column: `failed_at` had no writer at all,
    // so an unacknowledged message was re-injected at every turn boundary forever.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let id = send_to(&mut c, &alice, "bob", "ignored");

    for _ in 0..messages::MAX_OFFERS {
        assert_eq!(
            messages::deliverable(&c, &bob).expect("deliverable").len(),
            1,
            "still worth offering"
        );
        messages::mark_delivered_all(&mut c, &bob, &[id]).expect("offer it");
    }

    assert!(
        messages::deliverable(&c, &bob)
            .expect("deliverable")
            .is_empty(),
        "past the threshold a hook must stop spending context on it"
    );
    assert_eq!(
        subjects(&c, &bob),
        vec!["ignored"],
        "but an explicit `amb inbox` must still show it — backing off is not deletion"
    );
    assert_eq!(
        messages::offers(&c, &bob, id).expect("offers"),
        messages::MAX_OFFERS
    );
}

#[test]
fn one_agent_ignoring_a_broadcast_does_not_silence_it_for_everyone() {
    // D23, and the reason the counter had to move off `messages`. A per-message counter would
    // have been advanced by *bob's* every turn and then hidden the broadcast from carol, who
    // had never been offered it once. That is the D17 log property being quietly destroyed.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");
    let id = send_to(&mut c, &alice, "@", "for the room");

    for _ in 0..messages::MAX_OFFERS * 3 {
        messages::mark_delivered_all(&mut c, &bob, &[id]).expect("bob is offered it repeatedly");
    }

    assert!(
        messages::deliverable(&c, &bob).expect("bob").is_empty(),
        "bob has had his chances"
    );
    assert_eq!(
        messages::deliverable(&c, &carol).expect("carol").len(),
        1,
        "carol has been offered it zero times and must still receive it"
    );
}

#[test]
fn a_sender_does_not_receive_its_own_broadcast() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");

    send_to(&mut c, &alice, "@", "my own words");
    assert!(
        subjects(&c, &alice).is_empty(),
        "alice must not be told what she just said"
    );
    assert_eq!(
        subjects(&c, &bob),
        vec!["my own words"],
        "but bob must still receive it"
    );
}

#[test]
fn a_reply_to_a_broadcast_goes_only_to_its_sender() {
    // Replying to everyone is how a coordination channel turns into noise.
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");
    let carol = agent(&c, "uuid-carol", "carol", "nest");

    let bcast = send_to(&mut c, &alice, "@", "question for the room");
    messages::mark_read(&c, &bob, bcast).expect("bob reads");
    messages::mark_read(&c, &carol, bcast).expect("carol reads");

    messages::reply(&mut c, &bob, bcast, "my answer").expect("reply");

    assert_eq!(
        subjects(&c, &alice),
        vec!["Re: question for the room"],
        "alice gets the reply"
    );
    assert!(
        subjects(&c, &carol).is_empty(),
        "carol must not receive bob's reply"
    );
}

/// A name held by a session that has ended is reclaimable, and the roster shows both.
///
/// `ux_agents_name` is right — D18 needs a name to resolve to exactly one agent — but nothing
/// ever reaped the roster, so every name a session had ever used was consumed permanently. The
/// displaced session keeps a row under its auto-name rather than being deleted: `messages` stores
/// a sender id and joins the display name at read time, so its old mail relabels itself and two
/// sessions never read as one continuous identity (D75).
#[test]
fn a_name_held_by_a_session_that_has_ended_is_reclaimed_and_the_roster_shows_both() {
    let (_d, c) = board();
    let _alice = agent(&c, "uuid-alice", "alice", "nest");
    // **`pid = NULL` is not decoration.** These tests run in-process, so `session_pid()` reads the
    // environment of the *test runner* — inside a Claude session that is a real, live pid, and
    // `is_alive` checks the pid before it ever looks at `last_seen`. Ageing the row alone left the
    // holder looking alive and this test failed on its first run. Same family as D71: an
    // in-process test inherits an ambient environment unless it says otherwise. With no pid,
    // liveness degrades to recency, and ageing past the window is what "has ended" then means.
    c.execute(
        "UPDATE agents SET pid = NULL, last_seen = last_seen - ?1 WHERE id = 'uuid-alice'",
        [amb::identity::ASSUMED_ALIVE_FOR_SECS * 2.0],
    )
    .expect("age the holder");

    let newcomer = Identity {
        id: "uuid-other".into(),
        name: "alice".into(),
        project: "nest".into(),
        root: "/tmp/nest".into(),
    };
    let reg = amb::identity::register(&c, &newcomer, Some("alice")).expect("a dead holder yields");
    assert_eq!(reg.name, "alice");
    assert_eq!(
        reg.reclaimed_from.as_deref(),
        Some(amb::identity::default_name("nest", "uuid-alice").as_str()),
        "the reclamation must be reported, not silent"
    );

    // Both rows survive, under different names.
    let names: Vec<(String, String)> = c
        .prepare("SELECT id, name FROM agents WHERE project = 'nest' ORDER BY id")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(
        names,
        vec![
            ("uuid-alice".to_string(), "nest-uuid-a".to_string()),
            ("uuid-other".to_string(), "alice".to_string()),
        ],
        "the ended session keeps a row under its auto-name"
    );
}

/// A holder that is still around keeps its name. This is the direction that must never be wrong.
#[test]
fn a_live_holder_is_never_displaced() {
    let (_d, c) = board();
    let _alice = agent(&c, "uuid-alice", "alice", "nest");
    // Pin the premise rather than inheriting it: this process's own pid is alive by definition,
    // so the holder is unambiguously live whatever the runner's environment happens to be.
    c.execute(
        "UPDATE agents SET pid = ?1 WHERE id = 'uuid-alice'",
        [std::process::id() as i64],
    )
    .expect("make the holder live");
    let impostor = Identity {
        id: "uuid-other".into(),
        name: "alice".into(),
        project: "nest".into(),
        root: "/tmp/nest".into(),
    };
    let err = amb::identity::register(&c, &impostor, Some("alice"))
        .expect_err("a live holder must keep its name");
    assert!(matches!(err, amb::Error::NameTaken { .. }), "got {err:?}");
    let held: String = c
        .query_row("SELECT name FROM agents WHERE id = 'uuid-alice'", [], |r| {
            r.get(0)
        })
        .expect("alice still there");
    assert_eq!(held, "alice", "the live session kept its name");
}

/// A broadcast past D96's horizon leaves the delivery path and stays in the inbox.
///
/// **The case the D17 guard above cannot reach**, and that is the finding this test exists for.
/// `a_project_broadcast_reaches_an_agent_that_registered_afterwards` builds its fixture on a fresh
/// board, so every message in it is seconds old, and it reads through `inbox` — which the horizon
/// does not touch. Adding the horizon leaves it green. A guard that stays green when you change
/// the rule it names is D51's shape, and it was sitting on the project's central claim.
///
/// So this asserts the *split* rather than the horizon alone: gone from `deliverable`, present in
/// `inbox`. Either half alone is satisfiable by a wrong implementation — deleting the clause
/// passes the second, and dropping the row passes neither but for the wrong reason.
#[test]
fn a_broadcast_past_the_horizon_leaves_the_delivery_path_but_not_the_inbox() {
    let (_d, mut c) = board();
    let alice = agent(&c, "uuid-alice", "alice", "nest");
    let bob = agent(&c, "uuid-bob", "bob", "nest");

    let stale = send_to(&mut c, &alice, "@", "schema 9 is live");
    let _fresh = send_to(&mut c, &alice, "@", "schema 12 is live");
    let direct = send_to(&mut c, &alice, "bob", "a question for you");

    // Backdate the broadcast and the direct message by two days, past the 24h default.
    let two_days_ago = db::now().expect("clock") - 2.0 * 24.0 * 60.0 * 60.0;
    for id in [stale, direct] {
        c.execute(
            "UPDATE messages SET ts = ?1 WHERE id = ?2",
            rusqlite::params![two_days_ago, id],
        )
        .expect("backdate");
    }

    let delivered: Vec<String> = messages::deliverable(&c, &bob)
        .expect("deliverable")
        .into_iter()
        .map(|m| m.subject)
        .collect();
    assert!(
        !delivered.contains(&"schema 9 is live".to_string()),
        "a two-day-old broadcast must not be injected again: {delivered:?}"
    );
    assert!(
        delivered.contains(&"schema 12 is live".to_string()),
        "a fresh broadcast still delivers — without this row the test passes against a build \
         that delivers nothing at all: {delivered:?}"
    );
    assert!(
        delivered.contains(&"a question for you".to_string()),
        "direct mail never expires: a question asked of you personally does not stop mattering \
         because you were away: {delivered:?}"
    );

    // The other half of the split, and the reason this is not simply message expiry.
    assert_eq!(
        subjects(&c, &bob),
        vec![
            "schema 9 is live",
            "schema 12 is live",
            "a question for you"
        ],
        "`amb inbox` was asked for; it hides nothing, whatever the horizon says"
    );
}

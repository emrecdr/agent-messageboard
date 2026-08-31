//! Concurrency tests using real, unrelated OS processes.
//!
//! Threads would exercise a case that does not occur. The entire premise of this design is N
//! Claude sessions that are unrelated processes with no common parent (`BRIEF.md`), so a
//! threaded test would share a `Connection`, a process-wide SQLite lock table and an address
//! space that the real workload never shares.
//!
//! `MEASUREMENTS.md` M1 measured this shape in Python against the proposed schema. These tests
//! assert the same properties against the actual binary.

mod common;
use common::Board;

const SENDERS: usize = 8;
const PER_SENDER: usize = 10;

fn inbox_count(b: &Board, agent: &str) -> usize {
    b.json(agent, &["inbox", "--unread"])["count"]
        .as_u64()
        .expect("count is a number") as usize
}

/// The same count, read *as a member of* `project`.
///
/// `Board::json` cannot express this: it goes through `Board::cmd`, which pins
/// `AMB_PROJECT=nest` for every command in every suite built on the harness. That pin is right
/// for a suite about contention and wrong for one about scope, and it is the reason the 2x2's
/// project axis had never been exercised through the binary.
fn inbox_count_in(b: &Board, agent: &str, project: &str) -> usize {
    let mut c = b.cmd(agent);
    c.env("AMB_PROJECT", project);
    common::json_from(c, &["inbox", "--unread"])["count"]
        .as_u64()
        .expect("count is a number") as usize
}

/// Register `agent` under `name` into `project`.
fn register_in(b: &Board, agent: &str, name: &str, project: &str) {
    let out = b
        .cmd(agent)
        .env("AMB_PROJECT", project)
        .args(["register", "--name", name])
        .output()
        .expect("amb runs");
    assert!(
        out.status.success(),
        "register {name} into {project}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn concurrent_processes_lose_no_messages() {
    let b = Board::new();

    // Register every participant first, so resolution cannot fail for a race we are not testing.
    for i in 0..SENDERS {
        b.run(
            &format!("uuid-sender-{i}"),
            &["register", "--name", &format!("s{i}")],
        );
    }
    b.run("uuid-reader", &["register", "--name", "reader"]);

    // Spawn every sender before waiting on any of them: this is the contention the test exists
    // to create. Collecting the children first is load-bearing — waiting inside the loop would
    // serialise them and assert nothing.
    let mut children = Vec::new();
    for i in 0..SENDERS {
        for m in 0..PER_SENDER {
            let child = b
                .cmd(&format!("uuid-sender-{i}"))
                .args([
                    "send",
                    "reader",
                    "--subject",
                    &format!("s{i}-m{m}"),
                    "--body",
                    "concurrent",
                    "--json",
                ])
                .spawn()
                .expect("spawn amb");
            children.push(child);
        }
    }

    let mut failures = Vec::new();
    for c in children {
        let out = c.wait_with_output().expect("child finishes");
        if !out.status.success() {
            failures.push(String::from_utf8_lossy(&out.stderr).into_owned());
        }
    }
    assert!(
        failures.is_empty(),
        "{} sends failed: {:?}",
        failures.len(),
        failures
    );

    let expected = SENDERS * PER_SENDER;
    assert_eq!(
        inbox_count(&b, "uuid-reader"),
        expected,
        "every message from {SENDERS} concurrent processes must arrive exactly once"
    );
}

/// Exactly-once under contention. **Not scope** — see the test below.
///
/// Every reader here is in `nest`, so `@` and `@@` are the same message to this fixture, and
/// replacing the address below with `@@` leaves it green. That was checked rather than assumed.
/// The property this test owns is the `reads` ledger surviving N concurrent writers; the
/// addressing mode is incidental to it and the name should not be read as covering one.
#[test]
fn concurrent_broadcasts_reach_every_recipient_exactly_once() {
    let b = Board::new();
    let readers = ["uuid-r1", "uuid-r2", "uuid-r3"];
    for (i, r) in readers.iter().enumerate() {
        b.run(r, &["register", "--name", &format!("r{i}")]);
    }
    for i in 0..SENDERS {
        b.run(
            &format!("uuid-b{i}"),
            &["register", "--name", &format!("b{i}")],
        );
    }

    let mut children = Vec::new();
    for i in 0..SENDERS {
        children.push(
            b.cmd(&format!("uuid-b{i}"))
                .args([
                    "send",
                    "@",
                    "--subject",
                    &format!("bcast-{i}"),
                    "--body",
                    "x",
                    "--json",
                ])
                .spawn()
                .expect("spawn amb"),
        );
    }
    for mut c in children {
        assert!(
            c.wait().expect("child finishes").success(),
            "a concurrent broadcast failed"
        );
    }

    for r in readers {
        assert_eq!(
            inbox_count(&b, r),
            SENDERS,
            "{r} must receive each of the {SENDERS} broadcasts exactly once"
        );
    }
}

/// The two broadcast modes racing each other, which is the only arrangement that tells them apart.
///
/// **Same class as `bench_queue.py`'s defect (M16), and different in the way that decides the
/// fix.** There, `to_proj TEXT NOT NULL` made a global broadcast *unrepresentable*: no fixture
/// could have reached the cell, and the repair was to the schema. Here the DDL is whatever the
/// shipped binary creates, so all four of D17's cells are reachable — two of them were simply
/// never entered, because `Board::cmd` pins one project and nothing in this suite overrode it.
/// A schema that excludes a case and a fixture that skips one look identical from the test
/// report, and only the second can be repaired in the test file.
///
/// **Three expected counts, deliberately unequal.** A predicate that ignored `to_proj` would
/// hand every reader 12; one that scoped `@@` to its sender's project would hand the outsiders
/// 3 and 0. Equal expectations would let one wrong answer satisfy two assertions, which is how
/// a fixture stops discriminating without anything going red.
#[test]
fn a_global_broadcast_crosses_projects_under_contention_and_a_project_one_does_not() {
    let b = Board::new();

    // Unequal on purpose; see the note above.
    const GLOBAL: usize = 4;
    const NEST_LOCAL: usize = 5;
    const MOBILE_LOCAL: usize = 3;

    // One reader per project. `thirdproj` has no local sender at all, so its count is a witness
    // for `@@` alone rather than a sum that a scoping bug could arrive at another way.
    let readers = [
        ("uuid-rn", "rn", "nest", GLOBAL + NEST_LOCAL),
        ("uuid-rm", "rm", "mobile", GLOBAL + MOBILE_LOCAL),
        ("uuid-rt", "rt", "thirdproj", GLOBAL),
    ];
    for (id, name, project, _) in readers {
        register_in(&b, id, name, project);
    }

    // (agent, project it sends from, address it sends to)
    let mut senders = Vec::new();
    for i in 0..GLOBAL {
        senders.push((format!("uuid-g{i}"), "nest", "@@"));
    }
    for i in 0..NEST_LOCAL {
        senders.push((format!("uuid-n{i}"), "nest", "@"));
    }
    for i in 0..MOBILE_LOCAL {
        senders.push((format!("uuid-m{i}"), "mobile", "@"));
    }
    for (i, (id, project, _)) in senders.iter().enumerate() {
        register_in(&b, id, &format!("s{i}"), project);
    }

    // Spawn every sender before waiting on any: the contention is the point, and the two
    // addressing modes have to be in flight at the same time for this to be a race between them.
    let mut children = Vec::new();
    for (i, (id, project, to)) in senders.iter().enumerate() {
        children.push(
            b.cmd(id)
                .env("AMB_PROJECT", project)
                .args([
                    "send",
                    to,
                    "--subject",
                    &format!("mixed-{i}"),
                    "--body",
                    "x",
                    "--json",
                ])
                .spawn()
                .expect("spawn amb"),
        );
    }
    for mut c in children {
        assert!(
            c.wait().expect("child finishes").success(),
            "a concurrent broadcast failed"
        );
    }

    for (id, name, project, expected) in readers {
        assert_eq!(
            inbox_count_in(&b, id, project),
            expected,
            "{name} in {project} must receive exactly the broadcasts addressed to it"
        );
    }
}

#[test]
fn a_second_process_sees_a_message_the_first_committed() {
    // The cross-process visibility WAL is configured for. A threaded test sharing one
    // Connection would pass even if this were broken.
    let b = Board::new();
    b.run("uuid-a", &["register", "--name", "a"]);
    b.run("uuid-b", &["register", "--name", "b"]);

    b.run(
        "uuid-a",
        &["send", "b", "--subject", "hello", "--body", "x"],
    );
    assert_eq!(
        inbox_count(&b, "uuid-b"),
        1,
        "a separate process must see the commit"
    );
}

#[test]
fn an_unknown_recipient_exits_with_the_data_error_code() {
    // Exit codes are the contract a hook reads without parsing stderr, so they are asserted.
    let b = Board::new();
    b.run("uuid-a", &["register", "--name", "a"]);
    let out = b.try_run(
        "uuid-a",
        &["send", "ghost", "--subject", "s", "--body", "b"],
    );
    assert!(
        !out.status.success(),
        "sending to an unknown agent must fail"
    );
    assert_eq!(
        out.status.code(),
        Some(65),
        "EX_DATAERR, so a hook can tell it from a busy board"
    );
}

/// How many unrelated processes to race. Higher than the four-ish that would fail by luck.
const RACERS: usize = 12;

/// Put a board in the state an *older* binary would have left it in, ready to be raced.
///
/// Both race tests need the baseline schema plus a pragma or two saying which older state this
/// is — not-yet-WAL, or stamped a version back. Only that second part differs, so only that part
/// is a parameter; a third race test should add a call here rather than a third copy of the
/// `execute_batch` pair.
fn seed(b: &Board, as_left_by_an_older_binary: &str) {
    let conn = b.sqlite();
    conn.execute_batch(include_str!("../src/schema.sql"))
        .expect("baseline schema");
    conn.execute_batch(as_left_by_an_older_binary)
        .expect("seeding the older state");
}

/// Race `RACERS` processes and return every one that failed, with its stderr.
fn race(b: &Board, args: &[&str]) -> Vec<String> {
    let children: Vec<_> = (0..RACERS)
        .map(|i| {
            b.cmd(&format!("{i:02}-racer-uuid"))
                .args(args)
                .spawn()
                .expect("spawn amb")
        })
        .collect();
    children
        .into_iter()
        .filter_map(|c| {
            let out = c.wait_with_output().expect("child finishes");
            (!out.status.success()).then(|| String::from_utf8_lossy(&out.stderr).trim().to_string())
        })
        .collect()
}

#[test]
fn concurrent_conversions_to_wal_all_succeed() {
    // D30. Switching journal mode needs a brief *exclusive* lock, and SQLite will not invoke the
    // busy handler for it — so `busy_timeout` alone does not save you, whenever it is set.
    //
    // **The board is seeded not-yet-WAL on purpose**, so every racer meets the contended state
    // rather than only whoever arrives first.
    //
    // **Stated limitation: this guard is probabilistic.** With the retry removed it goes red in
    // roughly three runs out of six — measured, not estimated. Racing an absent file was no
    // better, and adding racers made it *worse*, because spawning them serialises their arrival
    // and the first converts the file before the rest turn up. An in-process version was tried
    // and deleted: a thread holding a read lock does not reproduce the contention at all, so that
    // test passed without ever exercising the retry, which is worse than a weak guard because it
    // looks like a strong one.
    //
    // It is kept because half-detection is real detection across repeated runs, and because the
    // fix it guards is measured elsewhere (`MEASUREMENTS.md` M8: 10 of 12 failing before, 0 of 12
    // after, five rounds). If this ever needs to be airtight, the missing piece is a start gate
    // the children block on so they arrive together.
    let b = Board::new();
    seed(
        &b,
        &format!(
            "PRAGMA journal_mode = DELETE; PRAGMA user_version = {};",
            amb::db::SCHEMA_VERSION
        ),
    );
    let failures = race(&b, &["register"]);
    assert!(
        failures.is_empty(),
        "{} of {RACERS} conversions failed: {:?}",
        failures.len(),
        failures
    );
}

#[test]
fn concurrent_processes_racing_a_schema_upgrade_all_succeed() {
    // D30, the other half. A schema bump reaches every live session at once — their next hook —
    // so this is the *normal* way an upgrade arrives, not an edge case. Under a deferred
    // transaction all of them read the old version, all apply the same migration, one wins and
    // the rest fail on a column that is already gone. Measured before the fix: 8 of 10.
    let b = Board::new();

    // A board as an older binary left it: schema present, stamped one version back.
    seed(&b, "PRAGMA journal_mode = WAL; PRAGMA user_version = 1;");

    let failures = race(&b, &["register"]);
    assert!(
        failures.is_empty(),
        "{} of {RACERS} upgrades failed: {:?}",
        failures.len(),
        failures
    );

    let conn = b.sqlite();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, amb::db::SCHEMA_VERSION, "and the board is current");
    let agents: i64 = conn
        .query_row("SELECT count(*) FROM agents", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        agents, RACERS as i64,
        "every racer must also have completed the work it came to do"
    );
}

/// Concurrent derivation must not lose a strike.
///
/// **This is the test that found D55, and it only found it on the second run.** The first pass —
/// eight processes, one round — lost nothing at all. The critical section is a file read followed
/// by a file write, microseconds wide, so a clean result is the *default* outcome rather than
/// evidence. Repeating it at higher contention lost between 1 and 17 of 24 strikes in every round.
///
/// Why it matters more than most races: the derivation count is the entire basis of promotion. A
/// lost strike is a candidate that was genuinely rediscovered three times, shows fewer, and is
/// never offered — with nothing anywhere reporting the loss.
///
/// Threads would not do. These are N unrelated OS processes with no common parent, which is the
/// premise the whole design rests on and the only arrangement where the board is the sole thing
/// they share (`CLAUDE.md`).
#[test]
fn concurrent_derivations_do_not_lose_strikes() {
    let b = Board::new();
    const N: usize = 16;

    // The board must exist before the children race for it, or they race to create it instead.
    b.mem("uuid-seed", &["memory", "status"]);

    let mut kids = Vec::new();
    for i in 0..N {
        let mut c = b.cmd_mem(&format!("uuid-{i}"));
        c.env("AMB_PROJECT", format!("proj{i}"));
        kids.push(
            c.args([
                "memory",
                "derive",
                "shared",
                "--title",
                "a shared thing",
                "--files",
                &format!("src/f{i}.rs"),
                "--note",
                "strike",
            ])
            .spawn()
            .expect("spawn"),
        );
    }
    for k in &mut kids {
        assert!(
            k.wait().expect("wait").success(),
            "every derive must succeed"
        );
    }

    let out = b.mem_json("uuid-check", &["memory", "candidates"]);
    assert_eq!(
        out["candidates"][0]["derived_count"], N as i64,
        "every independent derivation must be counted: {out}"
    );

    // And the file — the authority — must agree with the index (D34).
    let text = std::fs::read_to_string(b.vault.join("candidates/shared.md")).expect("the file");
    assert_eq!(
        text.matches("\"project\":").count(),
        N,
        "the ledger in the file must hold them all, not just the index"
    );
}

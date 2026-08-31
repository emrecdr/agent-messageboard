//! Properties of the pure core, over generated input rather than chosen input.
//!
//! **Why this exists, and why it is not `proptest`.** M17 records a test whose fixture never
//! reached the branch its own comment named: `nearest("api-v1x", &["api-v1", "totally-elsewhere"])`
//! drops the second candidate as out-of-budget, so the tie guard was never evaluated and two
//! mutations survived. Chosen inputs test the cases someone thought of. Generated inputs reach the
//! ones they did not.
//!
//! **What a property buys that an example cannot** is totality: *"`overlaps(a, b)` equals
//! `overlaps(b, a)`, for every pair"* is a claim no list of pairs can make.
//!
//! # Why no dependency
//!
//! Measured before deciding. Eight properties over 200,000 generated inputs found **zero
//! violations**, so the case for a crate rests on future value, not a present defect — and the
//! defects that motivated it (M17's tie guard, M24's rendered shape) are already closed by
//! fixtures and `assert_rendered_shape`.
//!
//! More decisive: **the hard part is the generator, not the framework.** The first version of this
//! file used a uniform character generator and left two of its eight properties *vacuous* —
//! `redact` fired zero times in 200,000 runs, and not one string parsed as a duration. A default
//! `proptest` strategy has exactly that problem: `any::<String>()` does not produce `ghp_…` or
//! `30m` either, so the custom strategies would be the same work in a different notation. What the
//! crate would add over this file is shrinking, which matters when a failure is hard to read; with
//! zero failures in 200,000 cases there is nothing to shrink.
//!
//! Revisit if a property here ever fails and the counter-example is unreadable. Until then this
//! costs one file and no supply chain.
//!
//! # The coverage assertions are the point
//!
//! `probe_reaches_every_branch_it_claims_to` is not decoration. A generator that never produces a
//! redactable string proves nothing about redaction while reporting green — the same failure M17
//! records, one level up, in the thing meant to catch it. Every counter below has a floor, so this
//! file cannot go quietly vacuous when a generator or a filter changes underneath it.

use amb::{address, claims, delivery, duration, memory, messages};

/// Iterations per property. 20,000 costs ~1.1 s against a ~3.3 s suite and reaches every branch
/// with margin; 200,000 was measured at 6.7 s, which is twice the suite for no new information.
const CASES: usize = 20_000;

/// xorshift64* — deterministic, seeded, no dependency.
///
/// Deterministic on purpose: a failure here is reproducible from the seed alone, with no
/// regressions file to keep in step. This project's recorded failure mode is an artefact drifting
/// from what it claims, and a corpus of saved counter-examples is another such artefact.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// Path-ish and prose-ish text: the alphabet the pure functions actually see.
    fn word(&mut self, maxlen: usize) -> String {
        // **Control characters are in here deliberately.** Without them `quoted`'s containment
        // property is vacuous — mutating `quoted` to pass them straight through left this file
        // green until they were added, which is the defect the whole file is about, in the file
        // itself. `\n` and `\r` are the ones that break the one-field-per-line grammar (D90);
        // `\t` and `\x07` are there so the rule is tested as "control", not as "newline".
        const ALPHA: [char; 16] = [
            'a', 'b', 'c', '-', '/', '.', '1', ' ', '@', '_', 'A', 'z', '\n', '\r', '\t', '\u{7}',
        ];
        let n = self.below(maxlen) + 1;
        (0..n).map(|_| ALPHA[self.below(ALPHA.len())]).collect()
    }

    /// A path with a real segment boundary, so `overlaps`' prefix arm is reached rather than
    /// stumbled upon. A uniform generator produced a `true` from `overlaps` 154 times in 200,000.
    fn path(&mut self, depth: usize) -> String {
        let segs = ["src", "a", "auth", "b", "memory", "c"];
        (0..self.below(depth) + 1)
            .map(|_| segs[self.below(segs.len())])
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Sometimes a duration, sometimes a credential shape. Without this, two properties below are
    /// vacuous — measured, not supposed.
    fn interesting(&mut self, maxlen: usize) -> String {
        match self.below(6) {
            0 => format!(
                "{}{}",
                self.below(90) + 1,
                ["s", "m", "h", "d"][self.below(4)]
            ),
            // Split so the literal never appears whole in tracked source (D100).
            1 => format!(
                "{}{}",
                concat!("ghp", "_"),
                self.word(24).replace(['-', '/', '.', ' ', '@'], "x")
            ),
            2 => format!("password={}", self.word(20).replace(' ', "x")),
            3 => self.path(4),
            _ => self.word(maxlen),
        }
    }
}

/// How often each interesting branch was reached. Asserted, never merely printed.
#[derive(Default)]
struct Reached {
    overlaps_true: u32,
    nearest_some: u32,
    redact_fired: u32,
    quoted_changed: u32,
    duration_ok: u32,
    address_ok: u32,
    /// Inputs that actually contained a control character — the only ones that test containment.
    control_in: u32,
}

#[test]
fn the_pure_core_holds_its_properties_over_generated_input() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut hit = Reached::default();
    let mut fails: Vec<String> = Vec::new();

    for _ in 0..CASES {
        let a = r.interesting(12);
        let b = if r.below(2) == 0 {
            r.path(4)
        } else {
            r.interesting(12)
        };

        // **Symmetry.** `overlaps` decides whether two agents collide; an asymmetric answer means
        // one session is warned and the other is not, which is a silence in one direction only.
        let (ab, ba) = (claims::overlaps(&a, &b), claims::overlaps(&b, &a));
        if ab {
            hit.overlaps_true += 1;
        }
        if ab != ba {
            fails.push(format!("overlaps asymmetric: {a:?} {b:?} -> {ab} / {ba}"));
        }

        // **Reflexivity**, for anything that normalises to a non-empty path. A claim that does not
        // overlap itself cannot conflict with a re-take of the same path.
        let norm = a.trim().trim_start_matches("./").trim_end_matches('/');
        if !norm.is_empty() && !claims::overlaps(&a, &a) {
            fails.push(format!("overlaps not reflexive: {a:?}"));
        }

        // **Containment.** `quoted` renders sender-written text into a one-field-per-line grammar
        // (D90). A control character escaping it is the forgery that constant exists to prevent.
        let q = delivery::quoted(&a);
        if q != a {
            hit.quoted_changed += 1;
        }
        if a.chars().any(char::is_control) {
            hit.control_in += 1;
        }
        if let Some(c) = q.chars().find(|c| c.is_control()) {
            fails.push(format!(
                "quoted leaked a control char {c:?}: {a:?} -> {q:?}"
            ));
        }
        // The cap plus the ellipsis it appends when it truncates.
        if q.chars().count() > delivery::QUOTED_MAX + 1 {
            fails.push(format!(
                "quoted exceeded its cap: {a:?} -> {} chars",
                q.chars().count()
            ));
        }
        // **Idempotent**, because a field can be rendered through more than one path.
        if delivery::quoted(&q) != q {
            fails.push(format!(
                "quoted not idempotent: {q:?} -> {:?}",
                delivery::quoted(&q)
            ));
        }

        // **Redaction is idempotent.** Re-redacting redacted text must not find new secrets in the
        // placeholder it just wrote, or the count printed to the author (D37) drifts from reality.
        let first = memory::redact(&a);
        if first.removed > 0 {
            hit.redact_fired += 1;
        }
        let second = memory::redact(&first.text);
        if second.text != first.text {
            fails.push(format!(
                "redact not idempotent: {a:?} -> {:?} -> {:?}",
                first.text, second.text
            ));
        }

        // **`nearest` only ever suggests a name it was given.** D26 makes this a suggestion shown
        // to a human; inventing one would be worse than staying silent.
        let (k1, k2, k3) = (r.word(8), r.word(8), r.word(8));
        let known = [k1.as_str(), k2.as_str(), k3.as_str()];
        if let Some(pick) = messages::nearest(&a, &known) {
            hit.nearest_some += 1;
            if !known.contains(&pick) {
                fails.push(format!("nearest invented {pick:?} from {known:?}"));
            }
        }

        // **Total, not panicking.** Both parse strings a person or a model typed.
        if duration::parse(&a).is_ok() {
            hit.duration_ok += 1;
        }
        if address::parse(&a).is_ok() {
            hit.address_ok += 1;
        }

        if fails.len() > 5 {
            break;
        }
    }

    assert!(
        fails.is_empty(),
        "property violations:\n  {}",
        fails.join("\n  ")
    );

    // **The premise, asserted rather than assumed.** Floors are an order of magnitude below what
    // was measured, so they fail on a generator that stopped reaching a branch and not on noise.
    let f = |name: &str, got: u32, floor: u32| {
        assert!(
            got >= floor,
            "{name} was reached {got} times in {CASES} cases, under a floor of {floor} — the \
             property above it is vacuous, which is M17's defect inside the test meant to catch it"
        );
    };
    f("overlaps returning true", hit.overlaps_true, 50);
    f("nearest returning Some", hit.nearest_some, 200);
    f("redact removing something", hit.redact_fired, 200);
    f("quoted changing its input", hit.quoted_changed, 200);
    f("a string parsing as a duration", hit.duration_ok, 200);
    f("a string parsing as an address", hit.address_ok, 200);
    // Without this floor the containment property above is satisfied by inputs that never had a
    // control character in them, which is how a mutation removing the strip survived once.
    f(
        "an input containing a control character",
        hit.control_in,
        200,
    );
}

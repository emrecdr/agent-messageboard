//! Pure text and time helpers: slugs, ages, hashes, dates.
//!
//! No filesystem, no database, no environment. The civil-calendar arithmetic
//! is Howard Hinnant's, thirty lines against a dependency.

/// A URL-safe, filesystem-safe stem for a title.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true; // leading dashes are trimmed by construction
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    // Truncated at a word boundary so the stem stays readable in Obsidian's file list.
    if out.len() > 48 {
        let cut = out[..48].rfind('-').unwrap_or(48);
        out.truncate(cut);
        while out.ends_with('-') {
            out.pop();
        }
    }
    if out.is_empty() {
        "note".to_string()
    } else {
        out
    }
}

/// One path component, with traversal made impossible rather than merely unlikely.
///
/// A project name comes from a repository directory's basename (D20), which is user-controlled
/// enough that `..` is worth refusing outright: this value is joined onto the vault root.
pub fn safe_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

/// How long ago, in the coarsest unit that is still informative.
///
/// **Every injected note renders this.** Staleness is the most-cited failure of memory systems —
/// a fact that is accurate until it isn't, and then confidently wrong — and rendering the age is
/// the cheapest possible defence: it lets a reader discount a note without the system having to
/// decide anything (D38).
pub fn age(then: f64, now: f64) -> String {
    let d = now - then;
    if d < 0.0 {
        return "just now".to_string();
    }
    let mins = d / 60.0;
    if mins < 1.0 {
        "just now".to_string()
    } else if mins < 60.0 {
        format!("{}m ago", mins as i64)
    } else if mins < 60.0 * 24.0 {
        format!("{}h ago", (mins / 60.0) as i64)
    } else if mins < 60.0 * 24.0 * 365.0 {
        format!("{}d ago", (mins / (60.0 * 24.0)) as i64)
    } else {
        format!("{}y ago", (mins / (60.0 * 24.0 * 365.0)) as i64)
    }
}

/// A 64-bit FNV-1a digest, hex-encoded.
///
/// **Change detection, not security.** It answers "is the file on disk still the one I indexed",
/// which is a question about accident rather than about an adversary, and it saves a dependency
/// on the hook path.
pub fn content_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ── Calendar ────────────────────────────────────────────────────────────────
//
// Howard Hinnant's public-domain civil-calendar algorithms. Thirty lines against a dependency
// that would otherwise appear on the hook path for two functions. UTC throughout, so a note's
// filename does not change meaning when its author travels.

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-08-27` — the date part of a note's stem.
pub fn format_date(secs: f64) -> String {
    let days = (secs / 86_400.0).floor() as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `2026-08-27T21:14:03Z`.
pub fn format_ts(secs: f64) -> String {
    let days = (secs / 86_400.0).floor() as i64;
    let rem = (secs - (days as f64) * 86_400.0).max(0.0) as i64;
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Read back what [`format_ts`] wrote. `None` on anything else, so a hand-edited note falls back
/// to its file mtime rather than to a wrong date.
pub fn parse_ts(s: &str) -> Option<f64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<i64> { s.get(a..b)?.parse::<i64>().ok() };
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = (days_from_civil(y, m, d) as f64) * 86_400.0;
    if bytes.len() >= 19 && bytes[10] == b'T' {
        let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
        secs += (h * 3600 + mi * 60 + sec) as f64;
    }
    Some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// M55's text.rs survivors, cluster by cluster: every boundary in `age`, the slug cap's
    /// own edge, the two `safe_component` flips that mangle letters and underscores, FNV-1a
    /// pinned to its published vectors, and the calendar round-tripped across eras. Each row
    /// names the mutant it kills.
    #[test]
    fn every_age_boundary_renders_the_coarser_unit_exactly_at_the_line() {
        // Exactly-at-boundary rows: `<` relaxing to `<=` shows the finer unit one step too long.
        assert_eq!(age(0.0, 60.0), "1m ago", "60s is a minute, not 'just now'");
        assert_eq!(age(0.0, 3_600.0), "1h ago", "60m is an hour");
        assert_eq!(age(0.0, 86_400.0), "1d ago", "24h is a day");
        assert_eq!(age(0.0, 365.0 * 86_400.0), "1y ago", "365d is a year");
        // The year threshold is a product; `60*24 + 365` minutes is about 21 days, so a
        // 30-day age would leap to years under the `* -> +` mutant.
        assert_eq!(age(0.0, 30.0 * 86_400.0), "30d ago");
        // Just-under rows: the finer unit holds its last value.
        assert_eq!(age(0.0, 59.0), "just now");
        assert_eq!(age(0.0, 3_540.0), "59m ago");
    }

    /// The negative-delta guard is unreachable in effect: every negative delta also satisfies
    /// `mins < 1.0` and renders "just now" through that arm, so its two `<` mutants are
    /// equivalent (M55 residue, reasoned and hand-checked, kept because a fail-safe first
    /// decision on a render path is worth one redundant branch). This row pins the *behavior*
    /// either way.
    #[test]
    fn a_clock_running_backwards_reads_as_just_now_not_as_negative_minutes() {
        assert_eq!(age(1_000.0, 0.0), "just now");
    }

    #[test]
    fn the_slug_cap_keeps_exactly_forty_eight_and_cuts_only_past_it() {
        // 48 chars exactly: 8 x "abcde-" minus the trailing dash = 47... build one precisely.
        let stem = "abcde-".repeat(8); // 48 chars, ends with '-'
        let input = stem.trim_end_matches('-'); // 47 chars — under the cap, untouched
        assert_eq!(slugify(input), input);
        let exact = format!("{}x", input); // 48 chars exactly
        assert_eq!(
            slugify(&exact),
            exact,
            "at the cap is kept whole; `>` not `>=`"
        );
        let over = format!("{}-yz", exact); // 51 chars: cut at the last dash inside 48
        assert_eq!(
            slugify(&over),
            &input[..41],
            "past the cap the cut lands at the last dash inside the window — dropping the \
             whole final word, because the boundary is the dash, not character 48"
        );
    }

    #[test]
    fn safe_component_keeps_the_named_charset_and_dashes_everything_else() {
        assert_eq!(
            safe_component("ab1"),
            "ab1",
            "letters pass — the first || flip dashes them"
        );
        assert_eq!(
            safe_component("a_b"),
            "a_b",
            "underscores pass — the second || flip dashes them"
        );
        assert_eq!(safe_component("a.b-c"), "a.b-c");
        assert_eq!(safe_component("a b"), "a-b");
    }

    /// FNV-1a pinned to its published vectors: any operator change in the fold — the `^=`
    /// becoming `|=` or `&=` that survived M55 — lands on a different digest. `drifted` compares
    /// these hashes to decide export staleness, where a degraded hash reads stale as current.
    #[test]
    fn the_content_hash_is_fnv1a_by_its_published_vectors() {
        assert_eq!(content_hash(""), "cbf29ce484222325");
        assert_eq!(content_hash("a"), "af63dc4c8601ec8c");
        assert_eq!(content_hash("foobar"), "85944171f73967e8");
    }

    /// Hinnant's algorithms round-tripped across eras — any single `+`/`-`/`/` flip in either
    /// direction breaks the identity somewhere in a sweep this wide — plus the epoch anchor.
    #[test]
    fn the_calendar_round_trips_across_eras_and_anchors_at_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0, "the epoch is day zero");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(format_date(0.0), "1970-01-01");
        let mut z = -1_000_000;
        while z < 1_000_000 {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(
                days_from_civil(y, m, d),
                z,
                "round-trip broke at day {z}: {y}-{m}-{d}"
            );
            assert!(
                (1..=12).contains(&m) && (1..=31).contains(&d),
                "day {z} gave {y}-{m}-{d}"
            );
            z += 337;
        }
    }

    /// `parse_ts` at its own length gates: a bare date of exactly ten bytes parses (the `<`
    /// mutants return None precisely there), and nineteen bytes without the 'T' stays date-only
    /// (the `&& -> ||` mutant would read hours out of a string that never promised them).
    #[test]
    fn a_ten_byte_date_parses_and_a_t_less_long_string_stays_date_only() {
        let date_only = parse_ts("2026-09-02").expect("ten bytes is a date");
        assert_eq!(
            parse_ts("2026-09-02 21:14:03"),
            Some(date_only),
            "no 'T' at byte ten means the time is not ours to read"
        );
        assert_eq!(
            parse_ts("2026-09-02T21:14:03Z").expect("with the T it counts"),
            date_only + 21.0 * 3600.0 + 14.0 * 60.0 + 3.0
        );
    }

    #[test]
    fn a_slug_is_lowercase_hyphenated_and_bounded() {
        assert_eq!(slugify("Flaky Fixture Race!"), "flaky-fixture-race");
        assert_eq!(slugify("  ...  "), "note");
        assert_eq!(slugify("render_all caps at ten"), "render-all-caps-at-ten");
        assert!(slugify(&"very long title ".repeat(20)).len() <= 48);
    }
    #[test]
    fn a_long_slug_is_cut_at_a_word_boundary_not_mid_word() {
        let s = slugify("alpha bravo charlie delta echo foxtrot golf hotel india juliet");
        assert!(!s.ends_with('-'), "{s}");
        assert!(
            s.split('-').all(|w| !w.is_empty()),
            "no empty segments: {s}"
        );
    }
    #[test]
    fn a_project_name_can_never_walk_out_of_the_vault() {
        // The value reaching this is a repository basename (D20), which is user-controlled
        // enough that traversal is worth making impossible rather than unlikely.
        // The invariant is containment, so it is asserted as containment: a component with no
        // separator and no bare-dots spelling is always a child of what it is joined to. Matching
        // on substrings instead would fail a perfectly safe name like `a..b`.
        let vault = Path::new("/tmp/vault/projects");
        for hostile in [
            "../../etc",
            "..",
            "/etc/passwd",
            "a/../../b",
            ".",
            "...",
            "",
        ] {
            let c = safe_component(hostile);
            assert!(!c.contains('/'), "{hostile} -> {c}");
            assert!(c != "." && c != "..", "{hostile} -> {c}");
            assert!(!c.is_empty(), "{hostile} -> {c}");
            let joined = vault.join(&c);
            assert_eq!(
                joined.components().count(),
                vault.components().count() + 1,
                "{hostile} -> {c} added more than one component"
            );
        }
        assert_eq!(safe_component("agent-messageboard"), "agent-messageboard");
    }
    #[test]
    fn the_calendar_round_trips_and_agrees_with_known_dates() {
        // Epoch itself, a leap day, and a date after 2038 — the three places a hand-rolled
        // calendar goes wrong.
        assert_eq!(format_ts(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(format_date(1_582_934_400.0), "2020-02-29");
        assert_eq!(format_date(2_240_006_400.0), "2040-12-25");
        for ts in [0.0, 1_582_934_400.0, 1_787_000_000.0, 2_240_006_400.0] {
            let s = format_ts(ts);
            assert_eq!(parse_ts(&s), Some(ts), "round trip failed for {s}");
        }
    }
    #[test]
    fn an_unparseable_timestamp_is_none_rather_than_a_wrong_date() {
        for bad in ["", "yesterday", "2026-13-01T00:00:00Z", "26-08-27"] {
            assert_eq!(parse_ts(bad), None, "{bad:?} should not parse");
        }
    }
    #[test]
    fn age_reads_in_the_coarsest_useful_unit() {
        let now = 1_000_000.0;
        assert_eq!(age(now, now), "just now");
        assert_eq!(age(now - 300.0, now), "5m ago");
        assert_eq!(age(now - 7200.0, now), "2h ago");
        assert_eq!(age(now - 86_400.0 * 3.0, now), "3d ago");
        assert_eq!(age(now - 86_400.0 * 800.0, now), "2y ago");
        // A note from the future is a clock problem, not a panic.
        assert_eq!(age(now + 500.0, now), "just now");
    }
    #[test]
    fn the_content_hash_changes_with_the_content() {
        assert_eq!(content_hash("a"), content_hash("a"));
        assert_ne!(content_hash("a"), content_hash("b"));
        assert_eq!(content_hash("").len(), 16);
    }
}

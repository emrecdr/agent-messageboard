//! Duration parsing for `--ttl`.
//!
//! Pure, like [`crate::address`], and for the same reason: it is a decision, not an action.

use crate::error::{Error, Result};
use std::time::Duration;

/// Parse `30s`, `10m`, `4h`, `2d`. A bare number is rejected rather than assumed to be seconds —
/// `--ttl 4` meaning four seconds when the writer meant four hours is a silent, expensive typo.
pub fn parse(input: &str) -> Result<Duration> {
    let s = input.trim();
    let bad = || Error::BadDuration {
        input: input.to_string(),
    };
    let (digits, unit) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = digits.parse().map_err(|_| bad())?;
    if n == 0 {
        return Err(bad());
    }
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3_600,
        "d" => n * 86_400,
        _ => return Err(bad()),
    };
    Ok(Duration::from_secs(secs))
}

/// The default claim lease: four hours, matching observed session length (`DECISIONS.md` D13).
pub const DEFAULT_TTL: Duration = Duration::from_secs(4 * 3_600);

/// Render a signed second count as a short human string: `40m ago`, `in 3h`.
pub fn humanise(delta_secs: f64) -> String {
    let past = delta_secs < 0.0;
    let s = delta_secs.abs();
    let body = if s < 90.0 {
        format!("{}s", s.round() as i64)
    } else if s < 5_400.0 {
        format!("{}m", (s / 60.0).round() as i64)
    } else if s < 172_800.0 {
        format!("{}h", (s / 3_600.0).round() as i64)
    } else {
        format!("{}d", (s / 86_400.0).round() as i64)
    };
    if past {
        format!("{body} ago")
    } else {
        format!("in {body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_unit() {
        assert_eq!(parse("30s").expect("s"), Duration::from_secs(30));
        assert_eq!(parse("10m").expect("m"), Duration::from_secs(600));
        assert_eq!(parse("4h").expect("h"), Duration::from_secs(14_400));
        assert_eq!(parse("2d").expect("d"), Duration::from_secs(172_800));
    }

    #[test]
    fn a_bare_number_is_rejected_not_assumed_to_be_seconds() {
        assert!(matches!(parse("4"), Err(Error::BadDuration { .. })));
    }

    #[test]
    fn zero_and_nonsense_are_rejected() {
        assert!(matches!(parse("0h"), Err(Error::BadDuration { .. })));
        assert!(matches!(parse("banana"), Err(Error::BadDuration { .. })));
        assert!(matches!(parse(""), Err(Error::BadDuration { .. })));
        assert!(matches!(parse("4y"), Err(Error::BadDuration { .. })));
    }

    #[test]
    fn default_ttl_is_four_hours() {
        // D13. If this changes, the reasoning in DECISIONS.md must change with it.
        assert_eq!(DEFAULT_TTL.as_secs(), 14_400);
    }

    #[test]
    fn humanise_distinguishes_past_from_future() {
        assert_eq!(humanise(-2_400.0), "40m ago");
        assert_eq!(humanise(10_800.0), "in 3h");
    }
}

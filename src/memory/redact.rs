//! Redaction — named shapes, never a global entropy threshold (D46).
//!
//! Entropy was measured over this project's own vocabulary and does not
//! separate: a crate version scores 4.06 bits/char and four real secrets score
//! below it. Do not "improve" this with a threshold.

// ── Redaction ───────────────────────────────────────────────────────────────

/// Text with secrets removed, and how many were removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    pub text: String,
    pub removed: usize,
}

/// Key fragments whose value is never worth keeping.
///
/// Bare `auth` and bare `token` are deliberately absent: this repository's own prose is full of
/// "token cost" and "auth lock ordering", and a filter that mangles ordinary sentences is one
/// people switch off. The length-and-shape test below is what catches the real ones.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "access-key",
    "credential",
    "private_key",
    "authorization",
    "auth_token",
    "access_token",
    "refresh_token",
    "session_token",
];

/// Words that sit between a sensitive key and the value it introduces.
///
/// `Authorization: Bearer <token>` puts three whitespace-separated tokens where the naive reading
/// expects two, and the secret is the third. Without this the header form — one of the commonest
/// ways a credential ends up pasted into prose — passed straight through.
const SCHEME_WORDS: &[&str] = &["bearer", "basic", "token", "digest", "apikey", "key"];

/// Prefixes that are secrets by construction, whatever they are attached to.
const SECRET_PREFIXES: &[&str] = &[
    "sk-",
    "sk_live_",
    "sk_test_",
    "pk_live_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxs-",
    "AKIA",
    "ASIA",
    "AIza",
    "ya29.",
    "hf_",
    "npm_",
    "dop_v1_",
    "sq0csp-",
    "shpat_",
    "sk-ant-",
    "sk-proj-",
    "SG.",
    "xapp-",
    "pypi-",
    "rubygems_",
    "glrt-",
    "doo_v1_",
    "atlassian_",
];

/// Strip `<private>` blocks and anything secret-shaped.
///
/// **Not deferrable, which is why it is on the write path rather than the read path.** Everything
/// captured here eventually reaches a model, and a note is durable: a secret redacted at
/// injection time would still be sitting in the vault in plain text (D37).
///
/// Deliberately biased toward over-redacting. The cost of a false positive is a `[redacted]` in
/// a note whose author is still in the session and can see the count [`observe`] reports; the
/// cost of a false negative is a credential in a markdown file forever.
pub fn redact(input: &str) -> Redacted {
    let mut removed = 0usize;
    let text = strip_private(input, &mut removed);
    let text = strip_pem(&text, &mut removed);

    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    // Set when the previous token was a sensitive key with nothing after its separator, so the
    // value it introduces arrives as the *next* token. `password: hunter2hunter2` and
    // `Authorization: Bearer abc…` both take this path.
    let mut armed = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            push_token(&mut out, &mut token, &mut removed, &mut armed);
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    push_token(&mut out, &mut token, &mut removed, &mut armed);
    Redacted { text: out, removed }
}

fn push_token(out: &mut String, token: &mut String, removed: &mut usize, armed: &mut bool) {
    if token.is_empty() {
        return;
    }
    if *armed {
        let bare = token.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';');
        // A scheme word is not the secret; stay armed for the token after it.
        if !SCHEME_WORDS.contains(&bare.to_ascii_lowercase().as_str()) {
            *armed = false;
            if substantial(bare) {
                out.push_str("[redacted]");
                *removed += 1;
                token.clear();
                return;
            }
        }
    } else {
        *armed = arms_next(token);
    }
    match redact_token(token) {
        Some(replacement) => {
            out.push_str(&replacement);
            *removed += 1;
        }
        None => out.push_str(token),
    }
    token.clear();
}

/// Whether this token is a sensitive key whose value has not arrived yet.
fn arms_next(token: &str) -> bool {
    let Some(key) = token.strip_suffix(':').or_else(|| token.strip_suffix('=')) else {
        return false;
    };
    let k = key
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .to_ascii_lowercase();
    SENSITIVE_KEYS.iter().any(|s| k.contains(s))
}

/// Long enough and varied enough to be a credential rather than a measurement.
fn substantial(v: &str) -> bool {
    v.len() >= 8 && !v.chars().all(|c| c.is_ascii_digit())
}

/// Remove `<private>…</private>`. An unclosed tag strips to the end of the text, which is the
/// direction that fails safe.
fn strip_private(input: &str, removed: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("<private>") {
        out.push_str(&rest[..start]);
        *removed += 1;
        let after = &rest[start + "<private>".len()..];
        match after.find("</private>") {
            Some(end) => rest = &after[end + "</private>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Remove PEM blocks whole. A key is the one secret that spans lines, so the token scan below
/// cannot see it.
fn strip_pem(input: &str, removed: &mut usize) -> String {
    if !input.contains("-----BEGIN") {
        return input.to_string();
    }
    let mut out = Vec::new();
    let mut inside = false;
    for line in input.lines() {
        if !inside && line.contains("-----BEGIN") {
            inside = true;
            *removed += 1;
            out.push("[redacted key block]");
            continue;
        }
        if inside {
            if line.contains("-----END") {
                inside = false;
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// Strip the credentials out of `scheme://user:secret@host`, keeping the rest legible.
///
/// **A structural shape rather than a guessed one**, which is why it is reliable where an entropy
/// test is not: the password sits at a fixed position between `://` and `@`. Connection strings
/// and basic-auth URLs are among the commonest ways a credential reaches prose, and every one of
/// them passed through the earlier filter untouched — `postgres` is not a sensitive key, and the
/// password is too short and too path-like for any length rule.
fn redact_url_credentials(token: &str) -> Option<String> {
    let (scheme, rest) = token.split_once("://")?;
    // `@` may legitimately appear later in a path or query; only the authority counts.
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let at = rest[..authority_end].rfind('@')?;
    let (userinfo, host) = (&rest[..at], &rest[at + 1..]);
    let user = userinfo.split_once(':')?.0;
    Some(format!("{scheme}://{user}:[redacted]@{host}"))
}

fn redact_token(token: &str) -> Option<String> {
    if let Some(cleaned) = redact_url_credentials(token) {
        return Some(cleaned);
    }
    // `key=value` and `key: value`, where the value is substantial enough to be a secret rather
    // than a measurement. "5,200 tokens" survives; `api_key=AKIA...` does not.
    for sep in ['=', ':'] {
        if let Some((key, value)) = token.split_once(sep) {
            let k = key.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
            let k = k.to_ascii_lowercase();
            if SENSITIVE_KEYS.iter().any(|s| k.contains(s)) {
                let v = value.trim_matches(|c: char| c == '"' || c == '\'' || c == ',');
                if v.len() >= 8 && !v.chars().all(|c| c.is_ascii_digit()) {
                    return Some(format!("{key}{sep}[redacted]"));
                }
            }
        }
    }
    let core = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if core.len() < 8 {
        return None;
    }
    if SECRET_PREFIXES.iter().any(|p| core.starts_with(p)) {
        return Some("[redacted]".to_string());
    }
    if is_jwt(core) || is_high_entropy(core) {
        return Some("[redacted]".to_string());
    }
    None
}

fn is_jwt(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts[0].starts_with("eyJ") && parts.iter().all(|p| p.len() >= 4)
}

/// A long opaque run with mixed case and digits.
///
/// **Shannon entropy was tried and rejected on measurement, not on principle.** It is the standard
/// secondary signal — gitleaks uses it — but gitleaks applies it *inside* a named rule's capture
/// group, and the reason shows up immediately when it is applied globally here: measured over this
/// project's own vocabulary, the lowest real secret scored **3.18 bits/char** and the highest
/// benign string — a crate version, `rusqlite-0.40.2-bundled-sqlite3-static` — scored **4.06**.
/// The bands overlap, so no threshold separates them. Named shapes are the primary signal and this
/// is a backstop.
///
/// `/` and `+` are allowed because an AWS secret access key is forty characters of base64 and was
/// otherwise missed. `.` still is not: a path with an extension is what prose is full of.
///
/// **The clause that used to close that sentence — "and a path without one is lowercase and fails
/// the mixed-case test anyway" — is false, and only measurement showed it** (M30). An agent's
/// scratchpad path on this machine carries capitals in `-Users-…-Projects-…` and digits in a
/// session UUID, so it satisfies every clause above and is redacted. Run over 53 real message
/// bodies it was the *only* thing this function removed, and the same path with a filename
/// appended survived twice in the same message, because the dot excluded it. The discriminator
/// between kept and destroyed is therefore a file extension, which has no relationship to
/// secrecy. `a_deep_path_is_redacted_which_is_a_known_false_positive` pins the behaviour so this
/// paragraph cannot rot back; the vault (D37) runs this filter today, and fixing it is a
/// calibration change deliberately not made while settling a documentation question (D98).
///
/// **The honest limit: a long lowercase-only token is indistinguishable from a git SHA here**, and
/// this rule does not try. gitleaks answers that case with named rules alone, and so does the
/// prefix list above (D46).
fn is_high_entropy(s: &str) -> bool {
    if s.len() < 40 || s.contains('.') {
        return false;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '=' | '_' | '-' | '/'))
    {
        return false;
    }
    s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars().any(|c| c.is_ascii_lowercase())
        && s.chars().any(|c| c.is_ascii_digit())
}

#[cfg(test)]
// **The credential fixtures below are built with `concat!`, and rejoining them breaks the push.**
//
// GitHub push protection blocked this repository's first push on five commits, flagging a Slack
// token and a Stripe key here. Every one was a fixture and none was real — the AWS one is
// Amazon's own published `AKIA…EXAMPLE` placeholder, and another spells the alphabet. But
// the condition is permanent rather than accidental: **a module that catches credential shapes has
// to be tested with credential shapes.**
//
// Splitting the prefix from the body leaves no contiguous match in the file while `concat!`
// rejoins it at compile time, so every value asserted below is byte-identical to a plain literal.
// It looks pointless, which is exactly why this comment is here — a negative decision leaves no
// trace in the code and gets helpfully fixed later. `tools/check_secret_literals.py` fails the
// gate if one is rejoined, and was verified by rejoining one.
mod tests {
    use super::*;

    #[test]
    fn private_blocks_are_removed_whole() {
        let r = redact("keep this <private>but not this</private> and this");
        assert!(!r.text.contains("but not this"), "{}", r.text);
        assert!(r.text.contains("keep this"));
        assert!(r.text.contains("and this"));
        assert_eq!(r.removed, 1);
    }
    #[test]
    fn an_unclosed_private_tag_strips_to_the_end_rather_than_leaking() {
        let r = redact("safe <private>secret and everything after it");
        assert!(!r.text.contains("secret"), "{}", r.text);
        assert!(r.text.contains("safe"));
    }
    #[test]
    fn secret_shaped_strings_are_replaced() {
        for secret in [
            "sk-abc123def456ghi789",
            concat!("ghp_", "16CharsOrMoreOfTokenHere00"),
            concat!("AKIA", "IOSFODNN7EXAMPLE"),
            concat!("glpat-", "abc123def456ghi"),
            "eyJhbGciOi.eyJzdWIiOiIx.SflKxwRJSMeKKF2QT4",
        ] {
            let r = redact(&format!("the key is {secret} ok"));
            assert!(
                !r.text.contains(secret),
                "{secret} survived redaction: {}",
                r.text
            );
            assert_eq!(r.removed, 1, "{secret}");
        }
    }
    #[test]
    fn a_key_block_is_removed_across_its_lines() {
        let r = redact(
            "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nAAAA\n-----END RSA PRIVATE KEY-----\nafter",
        );
        assert!(!r.text.contains("MIIEow"), "{}", r.text);
        assert!(r.text.contains("before") && r.text.contains("after"));
    }
    #[test]
    fn a_sensitive_key_loses_its_value_but_keeps_its_name() {
        let r = redact("password=hunter2hunter2 next");
        assert!(r.text.starts_with("password=[redacted]"), "{}", r.text);
        assert!(r.text.contains("next"));
    }
    #[test]
    fn ordinary_prose_and_identifiers_survive() {
        // These are the false positives that would make the filter something people switch off,
        // and every one of them is a phrase from this repository's own documents.
        let prose = "\
            injection costs 5200 tokens per turn boundary, measured against src/delivery.rs; \
            the auth lock ordering note cites 8fd3787abc1234567890abcdef1234567890abcd and \
            https://code.claude.com/docs/en/hooks says token: 5200";
        let r = redact(prose);
        assert_eq!(r.removed, 0, "redacted ordinary prose: {}", r.text);
        assert_eq!(r.text, prose);
    }
    #[test]
    fn a_git_sha_is_not_mistaken_for_a_secret() {
        // Forty lowercase hex characters: exactly the length the entropy rule triggers on, and
        // the reason that rule also requires mixed case.
        let sha = "8fd3787abc1234567890abcdef1234567890abcd";
        assert_eq!(sha.len(), 40);
        assert_eq!(redact(sha).removed, 0);

        // Reach proof: the same forty bytes with one case flipped clear the mixed-case bar, so
        // the row above tests the tri-class rule and not some earlier gate. If the length or
        // charset gate ever drifts, this row reddens — without it, the assertion above would
        // quietly become a statement about the gate instead (M17's fixture rule; the length
        // check on `sha` covers only the gate as it is spelled today).
        let flipped = "8fd3787Abc1234567890abcdef1234567890abcd";
        assert_eq!(
            redact(flipped).removed,
            1,
            "one case flip must cross the entropy bar"
        );
    }
    #[test]
    fn a_long_mixed_case_opaque_run_is_redacted() {
        let secret = "Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MFFXRVJUWXVpb3A";
        assert!(secret.len() >= 40);
        assert_eq!(redact(secret).removed, 1);
    }
    #[test]
    fn the_leak_shapes_that_actually_occur_are_caught() {
        for (name, input) in [
            (
                "openai",
                concat!("the key is sk-proj-", "abc123def456ghi789jkl012mno345pqr"),
            ),
            (
                "anthropic",
                concat!(
                    "ANTHROPIC_API_KEY=sk-ant-",
                    "api03-AbCdEf123456GhIjKl789MnOpQr"
                ),
            ),
            ("github pat", concat!("ghp_", "16CharsOrMoreOfTokenHere00")),
            ("aws access key id", concat!("AKIA", "IOSFODNN7EXAMPLE")),
            // Forty characters of base64 — contains '/', which the first version excluded.
            ("aws secret key", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            (
                "postgres url",
                "DATABASE_URL=postgres://admin:s3cr3tP4ssw0rd@db.internal:5432/app",
            ),
            ("mysql url", "mysql://root:hunter2hunter2@localhost/db"),
            (
                "basic auth url",
                "https://user:9f8e7d6c5b4a3210@api.example.com/v1",
            ),
            // Three tokens where the naive reading expects two; the secret is the third.
            (
                "bearer header",
                "Authorization: Bearer abc123def456ghi789jkl012mno345",
            ),
            (
                "env line",
                "SECRET_KEY=django-insecure-8f3a9c2b1d4e5f6a7b8c9d0e1f2a3b4c",
            ),
            (
                "slack bot token",
                concat!(
                    "xoxb-",
                    "123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
                ),
            ),
            (
                "stripe live key",
                concat!("sk_live_", "51AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"),
            ),
            (
                "jwt",
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.SflKxwRJSMeKKF2QT4fwpM",
            ),
        ] {
            let r = redact(input);
            assert!(r.removed > 0, "{name} survived redaction: {}", r.text);
        }
    }
    #[test]
    fn a_connection_string_keeps_everything_except_the_password() {
        // Redacting the whole token would destroy the note's usefulness. The host and the user
        // are what makes an observation about a database worth reading.
        let r = redact("DATABASE_URL=postgres://admin:s3cr3tP4ssw0rd@db.internal:5432/app");
        assert!(
            r.text
                .contains("postgres://admin:[redacted]@db.internal:5432/app"),
            "{}",
            r.text
        );
        assert!(!r.text.contains("s3cr3t"), "{}", r.text);
    }
    #[test]
    fn a_scheme_word_does_not_absorb_the_redaction_meant_for_the_token_after_it() {
        let r = redact("Authorization: Bearer abc123def456ghi789jkl012mno345");
        assert!(
            r.text.contains("Bearer"),
            "the scheme word is not the secret: {}",
            r.text
        );
        assert!(!r.text.contains("abc123def456"), "{}", r.text);
    }
    #[test]
    fn the_vocabulary_this_project_actually_uses_is_left_alone() {
        // Every one of these is a real string from this repository's own documents or source.
        // A filter that mangles them is one people switch off, which costs more than it saves.
        for (name, input) in [
            ("git sha", "8fd3787abc1234567890abcdef1234567890abcd"),
            ("uuid", "14e7b964-f5ac-4cb9-9191-9780a01cd1a4"),
            ("paths", "src/delivery.rs and tests/delivery.rs"),
            ("url", "https://code.claude.com/docs/en/hooks says so"),
            ("crate version", "rusqlite-0.40.2-bundled-sqlite3-static"),
            (
                "note slug",
                "2026-08-27-render-all-caps-at-ten-but-the-caller",
            ),
            (
                "measurement",
                "injection costs 5200 tokens per turn boundary",
            ),
            ("prose", "the auth lock ordering note, token: 5200"),
        ] {
            let r = redact(input);
            assert_eq!(r.removed, 0, "{name} was redacted: {}", r.text);
            assert_eq!(r.text, input, "{name}");
        }
    }
    #[test]
    fn a_long_lowercase_token_is_a_known_and_stated_miss() {
        // Not a bug to be fixed later — a limit. It is indistinguishable from a git SHA by any
        // rule available here, and entropy does not separate them either (3.94 against 3.93).
        // gitleaks answers this case with named prefixes alone, and so does the list above.
        let r = redact("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2");
        assert_eq!(
            r.removed, 0,
            "if this ever starts passing, delete this test"
        );
    }

    /// **A deep path is redacted, and that is a false positive rather than a limit** (M30).
    ///
    /// Its sibling above is a *miss* nothing can fix; this is a *hit* nothing should want. The
    /// docstring on [`is_high_entropy`] argued such a path would be lowercase and fail the
    /// mixed-case test; a real one taken off the board is neither, and it was the single removal
    /// this filter made across 53 message bodies.
    ///
    /// The two assertions are the finding, not two examples of it: the *shorter* form is
    /// destroyed and the *longer* form containing it survives, so the discriminator is a file
    /// extension. Pinned so the corrected paragraph cannot rot back to the claim it replaced —
    /// if this ever starts failing, the filter was fixed and that docstring must change with it.
    #[test]
    fn a_deep_path_is_redacted_which_is_a_known_false_positive() {
        let prefix = "/private/tmp/claude-501/-Users-e-Projects-playground-nest/4dd46026-10f3-4750-9a30-ce11c0f38633/scratchpad/";
        assert_eq!(
            redact(prefix).removed,
            1,
            "the false positive is gone; update is_high_entropy's docstring and delete this test"
        );

        let with_file = format!("{prefix}peer-latency-edit.patch");
        assert_eq!(
            redact(&with_file).removed,
            0,
            "a dot is the only thing keeping the longer, strictly more revealing form"
        );
    }

    /// **A wrapped secret is the shape a secret has in prose, and the prefix list stopped seeing
    /// it.**
    ///
    /// `core` exists so `SECRET_PREFIXES` — D46's named shapes, the primary signal — matches a
    /// token that arrived inside quotes, brackets or a trailing comma. Every existing test feeds
    /// the *bare* token, which is the shape a secret has in a code sample and not the shape it
    /// has in the paste that produced the note. So both mutants of that trim predicate survived:
    /// stop trimming quotes and `starts_with("sk-")` fails on every quoted credential in the
    /// list, silently, in the leaking direction (M27).
    #[test]
    fn a_secret_wrapped_in_punctuation_is_still_a_secret() {
        for (shape, input, core) in [
            (
                "json value",
                concat!("\"sk-ant-", "abc12345", "\""),
                concat!("sk-ant-", "abc12345"),
            ),
            (
                "trailing comma",
                concat!("'AKIA", "IOSFODNN7EXAMPLE", "',"),
                concat!("AKIA", "IOSFODNN7EXAMPLE"),
            ),
            (
                "parenthesised",
                concat!("(ghp_", "abcdefgh1234", ")"),
                concat!("ghp_", "abcdefgh1234"),
            ),
            (
                "shell quoted",
                concat!("'glpat-", "abc123def456ghi", "'"),
                concat!("glpat-", "abc123def456ghi"),
            ),
        ] {
            let r = redact(&format!("the key is {input} ok"));
            assert!(
                !r.text.contains(core),
                "{shape}: {core} survived redaction: {}",
                r.text
            );
            assert_eq!(r.removed, 1, "{shape}");
        }
    }

    /// **The length floor belongs to entropy, not to the prefix list, and the boundary is where
    /// that stops being true.**
    ///
    /// `core.len() < 8` runs *before* the prefix check, so it gates a rule that has no length
    /// premise of its own: `sk-` names a shape and the shape is the evidence. Both mutants of
    /// that comparison drop an exactly-eight-character credential and return `None` — the leaking
    /// direction, unreachable from any bare-token fixture.
    ///
    /// Asserted from both sides, because `< 8` -> `== 8` leaks at eight *and* starts redacting at
    /// seven; one assertion sees only half of it.
    #[test]
    fn the_prefix_list_carries_no_length_minimum_of_its_own() {
        let eight = "sk-abcde";
        assert_eq!(eight.len(), 8, "the boundary this test exists for");
        assert_eq!(
            redact(&format!("key {eight} end")).removed,
            1,
            "a named shape is evidence at any length above the floor"
        );

        let seven = "sk-abcd";
        assert_eq!(seven.len(), 7);
        assert_eq!(
            redact(&format!("key {seven} end")).removed,
            0,
            "and the floor still holds below it — otherwise `sk-a` is a credential"
        );
    }

    /// **Every path that removes something increments the same counter, because that counter is
    /// the only thing the author sees.**
    ///
    /// `Redacted::removed` reaches a person as `write.rs`'s `"N value(s) redacted before
    /// writing"`, printed under `if w.redacted > 0` — so a removal that fails to count is not a
    /// wrong number, it is a **silent redaction**, which is the one thing that comment forbids.
    /// `strip_pem`'s increment was unguarded: the block was still replaced and the note still
    /// said nothing had been. The mutation is in this file and the harm is only legible in the
    /// other one, which is why neither module's own tests could see it (M27).
    ///
    /// Enumerated rather than asserted once, for M23's reason: this is a property of the *field*.
    /// The residual hole an enumeration always has — a fifth removal path added without an
    /// increment — is closed separately by `changing_the_text_always_costs_a_count`, which does
    /// not need to know which path ran.
    /// **A measurement after a sensitive key is kept, whatever wraps it** — and the wrapping
    /// never counts toward the length that convicts. Nine M53 survivors sat on this boundary:
    /// the armed-path and inline trims could stop stripping quotes and commas (`|| -> &&`, one
    /// mutant per character), so a seven-character value read as nine and was redacted, and
    /// `substantial`'s conjunction could relax so an all-digit value — a measurement, the thing
    /// the rule exists to keep — was convicted by length alone. Every row asserts both halves
    /// of the seam M27 named: the text is unchanged *and* the count the author reads is zero.
    #[test]
    fn a_measurement_after_a_sensitive_key_is_kept_whatever_wraps_it() {
        for text in [
            // Armed path: key, separator, then the value as its own token.
            concat!("pass", "word: \"hunter7\""),
            concat!("pass", "word: 'hunter7'"),
            concat!("pass", "word: hunter7,"),
            concat!("pass", "word: 1234567890"),
            // Inline path: one key=value token.
            concat!("api", "_key=\"short12\""),
            concat!("api", "_key='short12'"),
            concat!("api", "_key=12345678901"),
        ] {
            let r = redact(text);
            assert_eq!(r.text, text, "kept verbatim: {text:?}");
            assert_eq!(
                r.removed, 0,
                "and the author is told nothing happened: {text:?}"
            );
        }
    }

    /// **The length floor holds at its boundary, and the dot exemption is charset-independent.**
    /// M53's `len < 40 || contains('.')` -> `&&` flip was first "killed" with a dotted long
    /// token — wrongly: the dot also fails the charset gate below, so that row is rejected under
    /// both codes and sees nothing (M17's fixture-never-reaches lesson, caught here because the
    /// hand-applied mutant stayed green). The flip's real difference is the floor collapsing: a
    /// 39-character opaque run gets entropy-checked where the real gate stops it. So the killer
    /// is the boundary row; the dotted row stays as the documented filename exemption, and the
    /// 40-character dotless twin proves the fixture family crosses every other gate.
    #[test]
    fn the_entropy_length_floor_holds_at_its_own_boundary() {
        let just_under = format!("Ab9{}", "xY7w".repeat(9)); // 39 chars, opaque, mixed case
        let r = redact(&just_under);
        assert_eq!(
            r.text, just_under,
            "under the floor is a name, not a secret"
        );
        assert_eq!(r.removed, 0);

        let dotted = format!("Ab9{}.rs", "xY7w".repeat(10));
        assert_eq!(
            redact(&dotted).removed,
            0,
            "the dot exempts a filename at any length"
        );

        let dotless = format!("Ab9{}", "xY7w".repeat(10)); // 43 chars
        assert_eq!(
            redact(&dotless).removed,
            1,
            "the dotless twin over the floor is caught — the family reaches every other gate"
        );
    }

    #[test]
    fn every_removal_path_is_counted_where_the_author_will_read_the_count() {
        for (path, input) in [
            ("private block", "keep <private>drop this</private> keep"),
            (
                "key block",
                "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----\nafter",
            ),
            (
                "armed key, value next token",
                "password: hunter2hunter2 next",
            ),
            ("secret-shaped token", "the key is sk-abc123def456ghi789 ok"),
        ] {
            assert_eq!(
                redact(input).removed,
                1,
                "{path}: removed something and told the author nothing"
            );
        }
    }

    /// **The counting rule as a property, which has no residual hole** (M27).
    ///
    /// The enumeration above lists the four paths that remove something, and every enumeration
    /// carries the same hole: a fifth path added without an increment stays silent until someone
    /// adds its row — the hole `delivery::UNTRUSTED` names in its own comment and cannot close.
    /// Here it can be closed, because the rule is not *"these four paths count"* but **"changing
    /// the text costs a count"**, and that is checkable over any input without knowing which path
    /// ran.
    ///
    /// Stated as an implication rather than an equivalence. The converse is false, and asserted
    /// below so this comment cannot rot into the kind D67 warns about.
    #[test]
    fn changing_the_text_always_costs_a_count() {
        for input in [
            "keep <private>drop this</private> keep",
            "safe <private>unclosed to the end",
            "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----\nafter",
            "password: hunter2hunter2 next",
            concat!("api_key=AKIA", "IOSFODNN7EXAMPLE", " done"),
            "the key is sk-abc123def456ghi789 ok",
            "token eyJhbGciOi.eyJzdWIiOiIx.SflKxwRJSMeKKF2QT4 end",
            "Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MFFXRVJUWXVpb3A",
            concat!("\"sk-ant-", "abc12345", "\" wrapped"),
            // The benign half, without which the implication is satisfied by a filter that
            // redacts everything. The corpus deliberately excludes the one input where the
            // converse fails; that one is asserted on its own below.
            "injection costs 5200 tokens per turn boundary",
            "8fd3787abc1234567890abcdef1234567890abcd",
            "src/delivery.rs and tests/delivery.rs",
        ] {
            let r = redact(input);
            assert!(
                r.text == input || r.removed > 0,
                "the text changed and the author was told nothing: {input:?} -> {:?}",
                r.text
            );
            assert!(
                r.text != input || r.removed == 0,
                "counted a removal that left the text alone: {input:?}"
            );
        }

        // **The one exception, asserted rather than described.** Re-redacting an already-redacted
        // value produces an identical string and still counts one: `[redacted]` is ten characters
        // and not all digits, so it satisfies the value rule the same way a secret does.
        // Redaction is idempotent in its text and not in its count. Nothing redacts twice today —
        // and if something starts to, this is where that shows up.
        let already = "password=[redacted]";
        let r = redact(already);
        assert_eq!(r.text, already, "re-redaction is text-idempotent");
        assert_eq!(r.removed, 1, "and is not count-idempotent");
    }
}

//! `--attach <path>`: a verifiable reference to a file, so a message cites bytes rather than
//! transcribing them (D142).
//!
//! **The bytes never touch the board.** D15 keeps the board disposable and `DECISIONS.md` already
//! rejected putting a new class of content on it (the query-text rejection at D131's site names the
//! same hazard: user content on a path `redact.rs` does not cover, in a database that is meant to be
//! deletable). What is recorded is a **sha256** the reader checks against their own copy — with
//! `sha256sum` from coreutils, needing no `amb` to verify it. That interoperability is the whole
//! reason the hash is sha256 rather than something faster: the verifier is a tool present on every
//! machine, and a blake3 digest would need one the reader may not have.
//!
//! **Not a trust boundary.** `amb` computes the hash from a real file at send time, but the block is
//! then ordinary body text (D98), so a hostile sender can hand-type a false one — which is exactly
//! what the untrusted-content banner at the point of consumption already governs. The value is for
//! *cooperating* senders: an honest `--attach` lets the reader detect a citation that has gone
//! stale, which prose cannot — the failure a session evaluating the board reported three times, each
//! a hand-written claim false in a way its writer could not see.
//!
//! Functional core: [`sha256_hex`] and [`render_block`] are pure and exhaustively testable; [`read`]
//! is the one function that touches the filesystem.

use crate::error::Error;
use sha2::{Digest, Sha256};

/// A file's provenance: the path as the sender named it, its exact size, and the digest that
/// identifies its bytes.
pub struct Attachment {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Lowercase hex of the sha256 of `data`.
///
/// Hand-rolled hex rather than a `hex` crate: one dependency for `{:02x}` in a loop is the
/// unused-surface the gate's `check_unused_deps` exists to refuse. Validated against the NIST
/// vectors in the tests, because a wrong digest would corrupt every provenance record silently —
/// the exact class of failure this project treats as its worst.
pub fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write;
    let digest = Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Read a file and record its provenance.
///
/// An unreadable path is an **error**, never a silent omission: the sender named it on purpose, and
/// a dropped attachment would be precisely the silence this project's failures keep taking — a
/// message that looks sent while citing nothing. Reuses [`Error::Io`] as `--body-file` does for the
/// sibling case of a path that will not read.
pub fn read(path: &str) -> Result<Attachment, Error> {
    let data = std::fs::read(path).map_err(|source| Error::Io {
        context: format!("reading the attachment {path}"),
        source,
    })?;
    Ok(Attachment {
        path: path.to_string(),
        bytes: data.len() as u64,
        sha256: sha256_hex(&data),
    })
}

/// Render attachments as a block appended to a message body, or `""` for none.
///
/// Exact bytes rather than a human-rounded size: a provenance record is precise, and the reader
/// compares it with `wc -c`. The `sha256:` prefix names the algorithm the reader runs, matching the
/// `algo:hex` convention (OCI, git) so it reads as verifiable rather than decorative.
///
/// **The path is contained, because this block has a grammar of its own** (M23). One attachment is
/// one line — the property [`each_attachment_is_its_own_line`] names and a reader counts on. A path
/// is sender-written and a filename may legally contain a newline on Unix, so `--attach $'ok.rs\n
/// secrets.env · 4096 bytes · sha256:00…'` rendered *two* entries from one file, the second wholly
/// fabricated and identical in shape to a real one. `quoted_block` downstream keeps it inside the
/// `> ` region so it never reaches column zero — this is not D90's voice forgery — but by then the
/// forged line is indistinguishable from a true one, which is the half containment at the caller
/// cannot fix.
///
/// **[`crate::delivery::breaks_grammar`] rather than [`crate::delivery::quoted`]**, deliberately.
/// `quoted` is built for an injection field: it also collapses space runs and truncates at
/// `QUOTED_MAX`, and both are wrong here — a reader runs `sha256sum <path>`, so a path silently
/// re-spaced or cut at 240 characters is a citation they cannot follow. Reusing the *predicate*
/// keeps one definition of "what breaks a line" (M28) while leaving the path otherwise exact.
pub fn render_block(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n── attached ──");
    for a in attachments {
        s.push_str(&format!(
            "\n{} · {} bytes · sha256:{}",
            contained(&a.path),
            a.bytes,
            a.sha256
        ));
    }
    s
}

/// Flatten anything that would break this block's one-line-per-attachment grammar.
///
/// Everything else is passed through byte-for-byte: the path's job is to be pasted into
/// `sha256sum`, so it is mangled as little as the grammar allows.
fn contained(path: &str) -> String {
    path.chars()
        .map(|c| {
            if crate::delivery::breaks_grammar(c) {
                ' '
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The digest is correct, checked against the published NIST vectors.
    ///
    /// **This is the load-bearing test of the whole feature.** A wrong sha256 would not fail — it
    /// would produce a plausible 64-hex string that no reader's `sha256sum` matches, turning every
    /// attachment into a false staleness alarm. A hand-rolled hex loop is exactly where an
    /// off-by-one nibble hides, so the assertion is byte-for-byte against known answers.
    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56 bytes, the vector that crosses a padding block boundary.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// The digest is over the exact bytes, so any change to the content changes the hash — the one
    /// property staleness detection rests on.
    #[test]
    fn a_single_byte_change_changes_the_digest() {
        assert_ne!(sha256_hex(b"abc"), sha256_hex(b"abd"));
    }

    /// No attachments render nothing — the block is absent, not an empty heading, so an ordinary
    /// message carries no residue.
    #[test]
    fn no_attachments_render_an_empty_string() {
        assert_eq!(render_block(&[]), "");
    }

    /// A rendered block names the path, the exact byte count, and the full digest — the three
    /// things a reader needs to verify, none abbreviated.
    #[test]
    fn a_block_carries_path_size_and_the_whole_digest() {
        let a = Attachment {
            path: "src/parse.rs".into(),
            bytes: 4321,
            sha256: sha256_hex(b"abc"),
        };
        let block = render_block(std::slice::from_ref(&a));
        assert!(block.contains("── attached ──"), "{block}");
        assert!(block.contains("src/parse.rs"), "{block}");
        assert!(block.contains("4321 bytes"), "{block}");
        assert!(
            block.contains(
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            ),
            "the whole digest must render, not a prefix a reader cannot compare: {block}"
        );
        // Whole-shape, not only presence: the block opens with the separator on its own lines, so
        // a future edit that inlined it into prose would be caught (M24's rule).
        assert!(block.starts_with("\n\n── attached ──\n"), "{block:?}");
    }

    /// A newline in a path cannot forge a second attachment entry (M23).
    ///
    /// **The sibling test below asserts the same invariant and cannot reach this case**, because
    /// its fixture uses clean paths — M17's shape, a guard whose input never arrives at it. A
    /// filename may legally contain a newline on Unix, and `--attach` accepts whatever reads, so
    /// this is the input that decides whether "one attachment, one line" is a rule or a comment.
    /// Confirmed red against the shipped binary before containment: `amb read` showed two entries,
    /// the fabricated one carrying a plausible size and digest.
    ///
    /// Counted as **lines**, and the first draft of this test counted something else and proved
    /// nothing. Filtering for lines containing `" bytes · sha256:"` stays green with the guard
    /// deleted: the newline puts `ok.rs` on its own line but leaves the forged text and the real
    /// `· 5 bytes · sha256:…` sharing the next one, so that filter finds exactly one line either
    /// way. Verified by removing the containment and watching it pass — the rule this project
    /// keeps for guards, applied to the guard's own test.
    #[test]
    fn a_newline_in_a_path_cannot_forge_a_second_attachment_entry() {
        let a = Attachment {
            path: "ok.rs\nsecrets.env · 4096 bytes · sha256:0000".into(),
            bytes: 5,
            sha256: sha256_hex(b"hello"),
        };
        let block = render_block(std::slice::from_ref(&a));
        // The presence row: the block rendered at all and carries the true digest, so the count
        // below is not vacuously satisfied by an empty string (M27 — an absence-only assertion has
        // an unproven premise).
        assert!(block.contains("── attached ──"), "{block:?}");
        assert!(block.contains(&sha256_hex(b"hello")), "{block:?}");
        assert_eq!(
            entry_lines(&block),
            1,
            "one attachment must occupy exactly one line: {block:?}"
        );
    }

    /// Lines of a rendered block that are neither the leading blanks nor the heading — one per
    /// attachment, which is the whole grammar.
    fn entry_lines(block: &str) -> usize {
        block
            .lines()
            .filter(|l| !l.is_empty() && !l.contains("── attached ──"))
            .count()
    }

    /// Two attachments render one line each, so a message citing several files stays legible.
    #[test]
    fn each_attachment_is_its_own_line() {
        let mk = |p: &str| Attachment {
            path: p.into(),
            bytes: 1,
            sha256: sha256_hex(b"x"),
        };
        let block = render_block(&[mk("a.rs"), mk("b.rs")]);
        assert_eq!(block.matches("sha256:").count(), 2, "{block}");
        assert!(
            block.contains("\na.rs · ") && block.contains("\nb.rs · "),
            "{block}"
        );
    }
}

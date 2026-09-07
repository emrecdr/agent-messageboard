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
pub fn render_block(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n── attached ──");
    for a in attachments {
        s.push_str(&format!(
            "\n{} · {} bytes · sha256:{}",
            a.path, a.bytes, a.sha256
        ));
    }
    s
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

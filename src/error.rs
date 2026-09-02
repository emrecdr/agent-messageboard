//! Typed domain errors.
//!
//! `thiserror` here rather than `anyhow`, because these are the failures a *caller* may want to
//! match on — a hook deciding whether a non-zero exit means "misconfigured" or "try again".
//! `anyhow` lives at the binary boundary in `main.rs`, where the only remaining job is to print
//! and pick an exit code.

/// Everything this library can fail with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "no agent identity: set AMB_AGENT, or run inside a Claude Code session where \
         CLAUDE_CODE_SESSION_ID is set"
    )]
    NoIdentity,

    #[error(
        "refusing to open {path}: it is inside a synced volume ({marker}), where SQLite's file \
         locking is not reliably honoured. Set AMB_DB to a local path."
    )]
    SyncedVolume { path: String, marker: String },

    #[error(
        "refusing to open {path}: it is on a {fstype} volume, and SQLite in WAL mode requires \
         every process using a database to be on the same host — \"processes on separate host \
         machines obviously cannot share memory with each other\". Set AMB_DB to a local path."
    )]
    RemoteVolume { path: String, fstype: String },

    #[error("invalid address {input:?}: {reason}")]
    BadAddress { input: String, reason: String },

    #[error("no message with id {0}")]
    NoSuchMessage(i64),

    #[error("no agent named {name:?} is registered in project {project:?}")]
    NoSuchAgent { name: String, project: String },

    #[error("the name {name:?} is already taken in project {project:?} — choose another")]
    NameTaken { name: String, project: String },

    #[error(
        "the database at {path} was created by a different schema version ({found}, expected \
         {expected}). It holds only ephemeral coordination state, so deleting it is safe."
    )]
    SchemaVersion {
        path: String,
        found: i64,
        expected: i64,
    },

    #[error("this agent holds no claim on {0}")]
    NoSuchClaim(String),

    /// No vault is configured, so there is nowhere to put a note.
    ///
    /// **This is the memory layer's kill switch, and it is deliberately the only one.** `amb` has
    /// no config file; a vault is somewhere the user already keeps notes and points Obsidian at,
    /// so guessing a default would create a directory nobody asked for. Unset means off, and the
    /// hook is silent rather than absent — see `memory::vault_path`.
    #[error(
        "no vault configured: set AMB_VAULT to a directory for your notes, e.g. \
         AMB_VAULT=~/vault"
    )]
    NoVault,

    #[error("no note with id {0:?} — ids look like `project/2026-08-27-some-slug`")]
    NoSuchNote(String),

    /// A repository's exported decisions disagree with the vault.
    ///
    /// **Its own variant rather than a reused one**, because the exit code is a contract a CI job
    /// or a pre-commit hook branches on, and the message is the thing a human reads when the hook
    /// fails. Borrowing `NoSuchNote` produced "no note with id \"1 exported decision(s) are
    /// stale\"", which is worse than no message.
    #[error(
        "{count} exported decision(s) in {repo} disagree with the vault \u{2014} run \
         `amb memory export` to refresh"
    )]
    ExportStale { count: usize, repo: String },

    #[error(
        "the id {slug:?} matches notes in more than one project: {projects} \u{2014} qualify it \
         as `project/{slug}`"
    )]
    AmbiguousNote { slug: String, projects: String },

    /// D11 is structural here rather than a convention a caller is trusted to honour.
    #[error(
        "{path} is inside the repository at {repo}, and `amb` never writes inside one (D11). \
         Choose a path outside every repository \u{2014} a parent directory works."
    )]
    InsideRepository { path: String, repo: String },

    #[error("invalid duration {input:?}: expected a form like 30m, 4h or 2d")]
    BadDuration { input: String },

    #[error(
        "body is {chars} characters and the limit is {max} \u{2014} refused here rather than \
         stored, because the recipient cannot decline it later"
    )]
    BodyTooLarge { chars: usize, max: usize },

    /// `subject`, claim `intent`, or a display `name` past its cap — `BodyTooLarge`'s reasoning
    /// at header scale. One variant for the three siblings because the message is the same
    /// sentence with a different noun, and three copies would drift (M28's rule for constants
    /// holds for wordings too).
    #[error(
        "{field} is {chars} characters and the limit is {max} \u{2014} something that long \
         belongs in the body, or in a file the recipient can choose to open"
    )]
    FieldTooLarge {
        field: &'static str,
        chars: usize,
        max: usize,
    },

    /// The board file is unreadable as a database — the one failure where "delete it" is the
    /// documented remedy, so the error says so at the moment it is needed (D15, U9) rather
    /// than in a doc the person in trouble has not read.
    #[error(
        "the board at {path} is not readable as a database. The board is disposable (D15): \
         move the file aside and it is recreated empty on the next command. Notes live in the \
         vault and are unaffected."
    )]
    CorruptBoard { path: String },

    /// A kind is rendered inside the message header's brackets, so it is a tag with a charset,
    /// not free text (D107).
    #[error(
        "invalid kind {input:?}: a kind is a lowercase tag \u{2014} letters, digits, `_` or \
         `-`, at most 20 characters"
    )]
    BadKind { input: String },

    #[error("a message needs a body: pass --body, or --body-file (use - for stdin)")]
    MissingBody,

    #[error("{context}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("database error while {context}")]
    Sqlite {
        context: String,
        #[source]
        source: rusqlite::Error,
    },

    #[error("could not parse {context} as JSON")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("PRAGMA {pragma} was requested but the database reports {got:?}")]
    PragmaRefused { pragma: String, got: String },

    #[error("the system clock is before the Unix epoch")]
    ClockBeforeEpoch,
}

/// This crate's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Attach context to an [`std::io::Error`] without a helper trait at every call site.
pub(crate) fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    move |source| Error::Io {
        context: context.into(),
        source,
    }
}

/// Attach context to a [`rusqlite::Error`].
pub(crate) fn sql(context: impl Into<String>) -> impl FnOnce(rusqlite::Error) -> Error {
    move |source| Error::Sqlite {
        context: context.into(),
        source,
    }
}

/// Exit codes, following the `sysexits.h` convention.
///
/// Distinct codes exist so a *hook* can react without parsing stderr: a misconfiguration is
/// worth surfacing to the user once, while a busy database is worth ignoring until next turn.
pub mod exit {
    /// `EX_USAGE` — the caller passed something malformed.
    pub const USAGE: u8 = 64;
    /// `EX_DATAERR` — the request was well-formed but names something that does not exist.
    pub const DATA: u8 = 65;
    /// `EX_UNAVAILABLE` — the board could not be reached or was busy. Transient; retry later.
    pub const UNAVAILABLE: u8 = 69;
    /// `EX_SOFTWARE` — an internal error. A bug.
    pub const SOFTWARE: u8 = 70;
    /// `EX_CANTCREAT` — a file or directory could not be created.
    pub const CANTCREAT: u8 = 73;
    /// `EX_CONFIG` — the environment is wrong: no identity, or a database in a bad place.
    pub const CONFIG: u8 = 78;
}

impl Error {
    /// A stable machine-readable slug, for `--json` output.
    ///
    /// A caller that asked for JSON gets JSON on the failure path too. Before this it got prose
    /// on stderr and an empty stdout, so an agent parsing output saw nothing at all and had to
    /// fall back to reading English — which is the one thing `--json` exists to avoid.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::NoIdentity => "no_identity",
            Error::SyncedVolume { .. } => "synced_volume",
            Error::RemoteVolume { .. } => "remote_volume",
            Error::BadAddress { .. } => "bad_address",
            Error::NoSuchMessage(_) => "no_such_message",
            Error::NoSuchAgent { .. } => "no_such_agent",
            Error::NameTaken { .. } => "name_taken",
            Error::SchemaVersion { .. } => "schema_version",
            Error::NoSuchClaim(_) => "no_such_claim",
            Error::NoVault => "no_vault",
            Error::NoSuchNote(_) => "no_such_note",
            Error::ExportStale { .. } => "export_stale",
            Error::AmbiguousNote { .. } => "ambiguous_note",
            Error::InsideRepository { .. } => "inside_repository",
            Error::BadDuration { .. } => "bad_duration",
            Error::BodyTooLarge { .. } => "body_too_large",
            Error::FieldTooLarge { .. } => "field_too_large",
            Error::BadKind { .. } => "bad_kind",
            Error::CorruptBoard { .. } => "corrupt_board",
            Error::MissingBody => "missing_body",
            Error::Io { .. } => "io",
            Error::Sqlite { .. } => "database",
            Error::Json { .. } => "json",
            Error::PragmaRefused { .. } => "pragma_refused",
            Error::ClockBeforeEpoch => "clock",
        }
    }

    /// The chain of `source` messages beneath this error, outermost first.
    pub fn causes(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut src = std::error::Error::source(self);
        while let Some(s) = src {
            out.push(s.to_string());
            src = std::error::Error::source(s);
        }
        out
    }

    /// Map an error to the process exit code it should produce.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::NoIdentity
            | Error::SyncedVolume { .. }
            | Error::RemoteVolume { .. }
            | Error::NoVault => exit::CONFIG,
            Error::BadAddress { .. }
            | Error::BadDuration { .. }
            | Error::BodyTooLarge { .. }
            | Error::FieldTooLarge { .. }
            | Error::BadKind { .. }
            | Error::MissingBody => exit::USAGE,
            Error::NoSuchMessage(_)
            | Error::NoSuchClaim(_)
            | Error::NoSuchAgent { .. }
            | Error::NoSuchNote(_)
            | Error::ExportStale { .. } => exit::DATA,
            Error::AmbiguousNote { .. } | Error::InsideRepository { .. } => exit::USAGE,
            Error::NameTaken { .. } => exit::USAGE,
            Error::SchemaVersion { .. } => exit::CONFIG,
            Error::Sqlite { .. } | Error::CorruptBoard { .. } => exit::UNAVAILABLE,
            Error::Io { .. } => exit::CANTCREAT,
            Error::Json { .. } | Error::ClockBeforeEpoch => exit::SOFTWARE,
            Error::PragmaRefused { .. } => exit::UNAVAILABLE,
        }
    }
}

#[cfg(test)]
mod tests {
    /// `causes` walks the real source chain — the constant-replacement mutants returned an
    /// empty or fabricated chain and nothing noticed, because the binary prints these lines
    /// ("  caused by: …") and no test read them (M55).
    #[test]
    fn the_cause_chain_carries_the_inner_error_outermost_first() {
        let inner = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "inner-detail");
        let err = crate::error::io("opening the board".to_string())(inner);
        let causes = err.causes();
        assert_eq!(causes.len(), 1, "one wrapped source, one cause");
        assert!(causes[0].contains("inner-detail"), "{causes:?}");
    }
}

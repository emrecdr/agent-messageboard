//! Topics: the middle rung between one project and everywhere (D82).
//!
//! A topic is a named thing a repository *is* — Rust, Python, Terraform — and it is the scope a
//! principle belongs to when it was rediscovered in three Rust repositories rather than in three
//! unrelated ones. Without it the promotion router has two rungs and over-generalises: it calls
//! that evidence universal, which is a stronger claim than the ledger made.

use super::*;

/// A topic, and the files whose presence at a repository root mean a repository is in it.
///
/// **Definition and detection are the same data**, which is the property that keeps this from
/// becoming two tables that disagree: the markers that say what `#rust` *is* are the markers that
/// decide whether you are standing in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    pub name: &'static str,
    pub markers: &'static [&'static str],
}

/// Every topic `amb` knows, in the order the router prefers them.
///
/// **Built in, because a configuration file is a surface this project has refused by name.**
/// `src/memory.rs` says so at the top: `AMB_VAULT` is the whole configuration surface, and memory
/// is not the layer that introduces a config file by accident. The companion plan proposed `.amb`
/// as TOML — which would also be this repository's **first new dependency in either direction**,
/// since nothing here parses TOML and the pitch is one static binary.
///
/// The cost is that the list is fixed until someone edits it and rebuilds. That is a real
/// limitation and it is the right trade at two projects: a wrong default here costs a mis-scoped
/// note that a person declines at the offer, while a config file costs a setup step in a tool
/// whose value is that it has none.
///
/// **Order is preference, not alphabet.** When several topics are shared by every deriving
/// project, the router takes the first one listed here and the offer names the others — so the
/// choice is deterministic and visible rather than an artefact of how a `Vec` happened to sort.
pub const TOPICS: &[Topic] = &[
    Topic {
        name: "rust",
        markers: &["Cargo.toml"],
    },
    Topic {
        name: "python",
        markers: &["pyproject.toml", "setup.py", "requirements.txt", "Pipfile"],
    },
    Topic {
        name: "typescript",
        markers: &["tsconfig.json"],
    },
    Topic {
        name: "javascript",
        markers: &["package.json"],
    },
    Topic {
        name: "go",
        markers: &["go.mod"],
    },
    Topic {
        name: "terraform",
        markers: &["main.tf", "terraform.tf"],
    },
    Topic {
        name: "docker",
        markers: &["Dockerfile", "docker-compose.yml", "compose.yml"],
    },
];

/// **The limit, in code rather than in a comment nobody reads.**
///
/// A topic is detectable only when a file at the repository root implies it. `security`,
/// `performance` and `api-design` are real topics and are **not detectable this way, ever** — no
/// cleverer heuristic closes that gap, because there is nothing on disk that means "this
/// repository is about security".
///
/// They stay reachable: a note can be written at `#security` by hand, `amb memory recall` searches
/// every scope, and the promotion router can be told where to land with `--scope`. What they
/// cannot do is be *detected*, so a session standing in a repository is never shown them
/// automatically. Writing that down is the point — a heuristic that guessed would be worse than
/// the gap, because it would be wrong silently.
pub const UNDETECTABLE: &[&str] = &["security", "performance", "api-design", "accessibility"];

/// Which topics a repository at `root` is in.
///
/// **Root markers, not a directory walk.** This runs inside `PreToolUse`, which fires before every
/// file tool call, so the budget is D9's. Checking a handful of names at one level is a dozen
/// `stat` calls; globbing `**/*.rs` across a large repository is not something to do on that path,
/// and it would answer a slightly different question anyway — a repository with one vendored `.rs`
/// file is not a Rust project.
pub fn detect(root: &Path) -> Vec<String> {
    TOPICS
        .iter()
        .filter(|t| t.markers.iter().any(|m| root.join(m).exists()))
        .map(|t| t.name.to_string())
        .collect()
}

/// The topics every one of these projects shares, in [`TOPICS`] order.
///
/// Empty when the projects have nothing in common, which is what routes a promotion to `@@`.
pub fn shared(per_project: &[Vec<String>]) -> Vec<String> {
    if per_project.is_empty() {
        return Vec::new();
    }
    TOPICS
        .iter()
        .map(|t| t.name.to_string())
        .filter(|name| per_project.iter().all(|ts| ts.contains(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_in_the_topics_its_root_files_imply() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("Cargo.toml"), "").expect("write");
        std::fs::write(dir.path().join("Dockerfile"), "").expect("write");
        assert_eq!(detect(dir.path()), vec!["rust", "docker"]);
    }

    #[test]
    fn a_repository_with_no_markers_is_in_no_topics() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("README.md"), "").expect("write");
        assert!(detect(dir.path()).is_empty());
    }

    /// Any one marker is enough — a Python project has `pyproject.toml` *or* `setup.py`, and
    /// requiring all of them would detect almost nothing.
    #[test]
    fn one_marker_out_of_several_is_enough() {
        for marker in ["pyproject.toml", "setup.py", "requirements.txt", "Pipfile"] {
            let dir = tempfile::tempdir().expect("temp dir");
            std::fs::write(dir.path().join(marker), "").expect("write");
            assert_eq!(
                detect(dir.path()),
                vec!["python"],
                "{marker} should imply python"
            );
        }
    }

    #[test]
    fn shared_is_the_intersection_and_is_empty_when_there_is_none() {
        let rust_docker = vec!["rust".to_string(), "docker".to_string()];
        let rust_only = vec!["rust".to_string()];
        let python = vec!["python".to_string()];

        assert_eq!(
            shared(&[rust_docker.clone(), rust_only.clone()]),
            vec!["rust"]
        );
        assert!(shared(&[rust_only.clone(), python.clone()]).is_empty());
        assert!(shared(&[rust_only.clone(), Vec::new()]).is_empty());
        // One project shares everything with itself, which is why the router checks the project
        // count *before* it asks this.
        assert_eq!(
            shared(std::slice::from_ref(&rust_docker)),
            vec!["rust", "docker"]
        );
    }

    /// The order is `TOPICS` order, not the order the projects happened to report them in.
    ///
    /// This is what makes the router deterministic when several topics qualify: it takes the
    /// first, and "first" has to mean something stable that a reader can look up.
    #[test]
    fn shared_returns_topics_in_the_declared_order_whatever_order_they_arrive_in() {
        let a = vec!["docker".to_string(), "rust".to_string()];
        let b = vec!["rust".to_string(), "docker".to_string()];
        assert_eq!(shared(&[a, b]), vec!["rust", "docker"]);
        let rust_first = TOPICS.iter().position(|t| t.name == "rust");
        let docker_first = TOPICS.iter().position(|t| t.name == "docker");
        assert!(
            rust_first < docker_first,
            "the assertion above rests on this"
        );
    }

    /// Every topic name is a legal scope, or the router can produce a scope that will not parse.
    #[test]
    fn every_topic_name_round_trips_as_a_scope() {
        use crate::address::{Scope, parse_scope};
        for t in TOPICS {
            let written = format!("#{}", t.name);
            assert_eq!(
                parse_scope(&written).expect("a topic name must be a legal scope"),
                Scope::Topic(t.name.to_string()),
                "{written} did not parse back"
            );
        }
    }

    /// The stated limit is stated about things that are genuinely not detectable.
    ///
    /// A name appearing in both lists would mean the documentation contradicts the code — the
    /// exact drift `check_docs.py` cannot see, because it is prose about a mechanism.
    #[test]
    fn nothing_declared_undetectable_is_secretly_detectable() {
        for name in UNDETECTABLE {
            assert!(
                !TOPICS.iter().any(|t| t.name == *name),
                "{name} is listed as undetectable and also has markers"
            );
        }
    }
}

//! The activity log.
//!
//! One append-only file of JSON lines. It records what the app did on a
//! person's behalf, which is the interesting half: the app starting a shell is
//! not worth writing down, and the app starting an agent, creating a branch or
//! refusing a request is.
//!
//! If the log cannot be written, the caller hears about it. An action that
//! cannot be recorded is an action the app should not take.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuditError {
    #[error("could not write the activity log: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not encode an activity entry: {0}")]
    Encode(#[from] serde_json::Error),
}

/// One recorded action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Milliseconds since the Unix epoch. A number and not a formatted date, so
    /// the log does not depend on a locale or a clock library.
    pub at_ms: u128,
    /// What happened: `agent.open`, `agent.refused`, `worktree.create`.
    pub kind: String,
    /// The pane it concerns, when there is one.
    pub pane: Option<String>,
    /// The agent it concerns, when there is one.
    pub agent: Option<String>,
    /// One line a person can read.
    pub detail: String,
}

impl Entry {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or_default(),
            kind: kind.into(),
            pane: None,
            agent: None,
            detail: detail.into(),
        }
    }

    pub fn for_pane(mut self, pane: impl Into<String>) -> Self {
        self.pane = Some(pane.into());
        self
    }

    pub fn for_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }
}

/// An append-only activity log at a fixed path.
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry, creating the file if it is not there yet.
    pub fn append(&self, entry: &Entry) -> Result<(), AuditError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // One line per entry, appended. A crash can lose the last line but
        // never an earlier one, which is the right trade for a log.
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&line)?;
        file.flush()?;
        Ok(())
    }

    /// Every entry recorded so far. Used by tests and by a future log viewer.
    pub fn read(&self) -> Result<Vec<Entry>, AuditError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(AuditError::Io(error)),
        };

        let mut entries = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            entries.push(serde_json::from_str(line)?);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hyperpane-audit-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch");
            Self { dir }
        }

        fn log(&self) -> AuditLog {
            AuditLog::new(self.dir.join("nested").join("audit.log"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn reading_a_log_that_is_not_there_is_not_an_error() {
        let scratch = Scratch::new("missing");
        assert!(scratch.log().read().expect("read").is_empty());
    }

    #[test]
    fn entries_survive_a_round_trip_and_keep_their_order() {
        let scratch = Scratch::new("roundtrip");
        let log = scratch.log();

        log.append(&Entry::new("agent.open", "first"))
            .expect("append");
        log.append(
            &Entry::new("agent.open", "second")
                .for_pane("p2")
                .for_agent("opencode"),
        )
        .expect("append");

        let entries = log.read().expect("read");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].detail, "first");
        assert_eq!(entries[1].detail, "second");
        assert_eq!(entries[1].pane.as_deref(), Some("p2"));
        assert_eq!(entries[1].agent.as_deref(), Some("opencode"));
        assert!(entries[1].at_ms > 0);
    }

    #[test]
    fn appending_creates_the_parent_directory() {
        let scratch = Scratch::new("mkdir");
        let log = scratch.log();
        log.append(&Entry::new("worktree.create", "agent/1234"))
            .expect("append");
        assert!(log.path().exists());
    }

    #[test]
    fn the_log_is_json_lines() {
        let scratch = Scratch::new("lines");
        let log = scratch.log();
        log.append(&Entry::new("a", "one")).expect("append");
        log.append(&Entry::new("b", "two")).expect("append");

        let text = std::fs::read_to_string(log.path()).expect("read");
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        }
    }

    #[test]
    fn a_corrupt_line_is_reported_rather_than_skipped() {
        let scratch = Scratch::new("corrupt");
        let log = scratch.log();
        log.append(&Entry::new("a", "one")).expect("append");
        std::fs::write(log.path(), "{not json}\n").expect("write");

        assert!(matches!(log.read(), Err(AuditError::Encode(_))));
    }
}

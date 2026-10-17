//! Session persistence.
//!
//! One JSON file, written atomically. A corrupt or future-version file is
//! reported rather than silently replaced, because the alternative is deleting
//! a person's layout without telling them.

use std::path::{Path, PathBuf};

use hp_core::{Session, SESSION_VERSION};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("could not read or write the session file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the session file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the session file is version {found}; this build understands version {expected}")]
    Version { found: u32, expected: u32 },
}

/// Reads and writes the session file at a fixed path.
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the stored session.
    ///
    /// A missing file is not an error: it is what a first run looks like, and
    /// the caller gets an empty session to start from.
    pub fn load(&self) -> Result<Session, StoreError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Session::empty())
            }
            Err(error) => return Err(StoreError::Io(error)),
        };

        // Notepad and PowerShell both write a UTF-8 BOM, and serde_json
        // rejects one. A hand-edited session should not fail to load.
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);

        let session: Session = serde_json::from_slice(bytes)?;
        if session.version != SESSION_VERSION {
            return Err(StoreError::Version {
                found: session.version,
                expected: SESSION_VERSION,
            });
        }
        Ok(session.normalise())
    }

    /// Write the session, replacing whatever was there.
    pub fn save(&self, session: &Session) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(session)?;
        self.write_atomically(&bytes)
    }

    /// Write to a sibling file and rename over the target.
    ///
    /// A rename is atomic on both Windows and Unix, so a crash partway through
    /// leaves either the old session or the new one, never a truncated file.
    fn write_atomically(&self, bytes: &[u8]) -> Result<(), StoreError> {
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp_core::{Profile, SplitDirection, Workspace};

    /// A directory that cleans itself up, named per test so tests can run in
    /// parallel without colliding.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hyperpane-store-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch dir");
            Self { dir }
        }

        fn store(&self) -> SessionStore {
            SessionStore::new(self.dir.join("session.json"))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_session() {
        let scratch = Scratch::new("missing");
        let session = scratch.store().load().expect("load");
        assert!(session.workspace.is_none());
        assert_eq!(session.profiles.len(), 1);
    }

    #[test]
    fn a_saved_session_comes_back_identical() {
        let scratch = Scratch::new("roundtrip");
        let store = scratch.store();

        let mut session = Session::empty();
        let mut workspace = Workspace::single("main", "Main", "p1");
        workspace.add_pane("p1", SplitDirection::Row, "p2");
        session.workspace = Some(workspace);
        session.profiles.push(Profile {
            id: "pwsh".into(),
            name: "PowerShell".into(),
            shell: Some("pwsh.exe".into()),
            args: vec!["-NoLogo".into()],
            cwd: None,
            env: Default::default(),
            integration: true,
        });
        session.default_profile = Some("pwsh".into());

        store.save(&session).expect("save");
        let loaded = store.load().expect("load");
        assert_eq!(loaded, session.normalise());
    }

    #[test]
    fn saving_creates_the_parent_directory() {
        let scratch = Scratch::new("mkdir");
        let store = SessionStore::new(scratch.dir.join("nested").join("deeper").join("s.json"));
        store.save(&Session::empty()).expect("save");
        assert!(store.path().exists());
    }

    #[test]
    fn saving_replaces_an_existing_file_and_leaves_no_temporary() {
        let scratch = Scratch::new("replace");
        let store = scratch.store();

        let mut first = Session::empty();
        first.profiles[0].name = "first".into();
        store.save(&first).expect("save first");

        let mut second = Session::empty();
        second.profiles[0].name = "second".into();
        store.save(&second).expect("save second");

        assert_eq!(store.load().expect("load").profiles[0].name, "second");
        assert!(
            !store.path().with_extension("json.tmp").exists(),
            "the temporary file should have been renamed away"
        );
    }

    #[test]
    fn corrupt_json_is_reported_not_replaced() {
        let scratch = Scratch::new("corrupt");
        let store = scratch.store();
        std::fs::write(store.path(), b"{ this is not json").expect("write");

        let error = store.load().expect_err("should not parse");
        assert!(matches!(error, StoreError::Parse(_)));
        // The file is still there: the caller decides what to do about it.
        assert!(store.path().exists());
    }

    #[test]
    fn a_future_version_is_refused_rather_than_half_read() {
        let scratch = Scratch::new("version");
        let store = scratch.store();
        std::fs::write(
            store.path(),
            br#"{"version":99,"workspace":null,"profiles":[],"defaultProfile":null}"#,
        )
        .expect("write");

        let error = store.load().expect_err("should refuse");
        assert!(matches!(
            error,
            StoreError::Version {
                found: 99,
                expected: 1
            }
        ));
    }

    #[test]
    fn a_utf8_bom_does_not_break_loading() {
        let scratch = Scratch::new("bom");
        let store = scratch.store();

        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(
            br#"{"version":1,"workspace":null,"profiles":[{"id":"default","name":"Default shell","shell":null,"args":[],"cwd":null,"env":{}}],"defaultProfile":"default"}"#,
        );
        std::fs::write(store.path(), &bytes).expect("write");

        let session = store.load().expect("a BOM is not a parse error");
        assert_eq!(session.default_profile.as_deref(), Some("default"));
    }

    #[test]
    fn an_empty_workspace_is_dropped_on_load() {
        let scratch = Scratch::new("emptyworkspace");
        let store = scratch.store();

        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: hp_core::LayoutNode::leaf(""),
            focused: None,
            active: true,
        });
        store.save(&session).expect("save");

        assert!(store.load().expect("load").workspace.is_none());
    }
}

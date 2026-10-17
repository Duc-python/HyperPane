//! Git worktrees for agent panes.
//!
//! Two agents editing the same checkout collide, so a pane that writes files
//! gets its own worktree on its own branch. Worktrees are created under a
//! directory the caller owns — the app's data directory — rather than inside
//! the repository, so nothing about a person's checkout changes because they
//! opened a pane.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("{0} is not inside a git repository")]
    NotARepository(PathBuf),
    #[error("git is not available: {0}")]
    GitMissing(#[from] std::io::Error),
    #[error("git {command} failed: {message}")]
    Git { command: String, message: String },
    #[error("the branch name {0} is not usable")]
    BadBranch(String),
}

/// A worktree that was created, or already existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

/// A branch name for an agent run, derived from `seed`.
///
/// The shape is `agent/<eight hex>`: short enough to type, and derived rather
/// than random so the same seed gives the same name in a log and in a branch
/// listing.
pub fn branch_name(seed: &str) -> String {
    // FNV-1a. A hash is enough here: the name only has to be distinct and
    // stable, and pulling in a cryptographic digest for that would be silly.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("agent/{:08x}", (hash >> 32) as u32)
}

/// Whether `path` is inside a git work tree.
pub fn is_repository(path: &Path) -> bool {
    run_git(path, &["rev-parse", "--is-inside-work-tree"])
        .map(|output| output.trim() == "true")
        .unwrap_or(false)
}

/// The repository root that contains `path`.
pub fn repository_root(path: &Path) -> Option<PathBuf> {
    let output = run_git(path, &["rev-parse", "--show-toplevel"]).ok()?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Create a worktree for `branch` at `base/branch-name`, or reuse it.
///
/// Reusing rather than failing makes the call idempotent: a pane restored from
/// a saved session finds the worktree its branch already has.
pub fn create(repo: &Path, base: &Path, branch: &str) -> Result<Worktree, WorktreeError> {
    if !is_repository(repo) {
        return Err(WorktreeError::NotARepository(repo.to_path_buf()));
    }
    if branch.trim().is_empty() || branch.contains(char::is_whitespace) {
        return Err(WorktreeError::BadBranch(branch.to_string()));
    }

    let directory = base.join(branch.replace('/', "-"));
    if directory.join(".git").exists() {
        return Ok(Worktree {
            path: directory,
            branch: branch.to_string(),
        });
    }

    std::fs::create_dir_all(base)?;

    let directory_text = directory.to_string_lossy().into_owned();
    run_git(
        repo,
        &["worktree", "add", "-b", branch, &directory_text, "HEAD"],
    )
    .map_err(|error| match error {
        WorktreeError::Git { command, message } if message.contains("already exists") => {
            // The branch is there but the directory is not: attach to it.
            let _ = run_git(repo, &["worktree", "add", &directory_text, branch]);
            WorktreeError::Git { command, message }
        }
        other => other,
    })?;

    Ok(Worktree {
        path: directory,
        branch: branch.to_string(),
    })
}

/// Remove a worktree. Leaves the branch alone: only a person deletes work.
pub fn remove(repo: &Path, path: &Path) -> Result<(), WorktreeError> {
    let path_text = path.to_string_lossy().into_owned();
    run_git(repo, &["worktree", "remove", "--force", &path_text])?;
    Ok(())
}

/// Run git in `directory` and return stdout, or a typed error.
fn run_git(directory: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(WorktreeError::Git {
        command: args.join(" "),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway repository, or `None` when git is not installed.
    struct Scratch {
        dir: PathBuf,
        available: bool,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hyperpane-worktree-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch");

            let available = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["init", "--quiet"])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false);

            if available {
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["config", "user.email", "test@example.com"])
                    .output();
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["config", "user.name", "Test"])
                    .output();
                std::fs::write(dir.join("README.md"), "hello\n").expect("write");
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["add", "."])
                    .output();
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["commit", "--quiet", "-m", "first"])
                    .output();
            }

            Self { dir, available }
        }

        fn base(&self) -> PathBuf {
            self.dir.join("worktrees")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_branch_name_is_short_stable_and_safe() {
        let name = branch_name("p1:fix the build");
        assert!(name.starts_with("agent/"), "{name}");
        assert_eq!(name.len(), "agent/".len() + 8, "{name}");
        assert_eq!(
            name,
            branch_name("p1:fix the build"),
            "same seed, same name"
        );
        assert_ne!(name, branch_name("p2:fix the build"), "a different seed");
        assert!(!name.contains(char::is_whitespace));
    }

    #[test]
    fn a_plain_directory_is_not_a_repository() {
        // Deliberately outside the scratch repository: a directory inside one
        // *is* inside a work tree, and that is not what this test is about.
        let plain = std::env::temp_dir().join("hyperpane-worktree-not-a-repo");
        let _ = std::fs::remove_dir_all(&plain);
        std::fs::create_dir_all(&plain).expect("create");

        let base = std::env::temp_dir().join("hyperpane-worktree-not-a-repo-base");
        let outcome = create(&plain, &base, "agent/12345678");
        assert!(
            matches!(outcome, Err(WorktreeError::NotARepository(_))),
            "expected NotARepository, got {outcome:?}"
        );

        let _ = std::fs::remove_dir_all(&plain);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn an_unusable_branch_name_is_refused() {
        let scratch = Scratch::new("badbranch");
        let error = create(&scratch.dir, &scratch.base(), "agent/not allowed");
        assert!(matches!(error, Err(WorktreeError::BadBranch(_))));
    }

    #[test]
    fn creates_a_worktree_on_its_own_branch() {
        let scratch = Scratch::new("create");
        if !scratch.available {
            return;
        }

        let worktree = create(&scratch.dir, &scratch.base(), "agent/aaaaaaaa").expect("created");
        assert!(
            worktree.path.join("README.md").is_file(),
            "files are checked out"
        );
        assert!(is_repository(&worktree.path));

        let head = run_git(&worktree.path, &["rev-parse", "--abbrev-ref", "HEAD"]).expect("git");
        assert_eq!(head.trim(), "agent/aaaaaaaa");
    }

    #[test]
    fn creating_twice_reuses_the_same_worktree() {
        let scratch = Scratch::new("reuse");
        if !scratch.available {
            return;
        }

        let first = create(&scratch.dir, &scratch.base(), "agent/bbbbbbbb").expect("created");
        let second = create(&scratch.dir, &scratch.base(), "agent/bbbbbbbb").expect("reused");
        assert_eq!(first, second);
    }

    #[test]
    fn remove_takes_the_worktree_but_not_the_branch() {
        let scratch = Scratch::new("remove");
        if !scratch.available {
            return;
        }

        let worktree = create(&scratch.dir, &scratch.base(), "agent/cccccccc").expect("created");
        remove(&scratch.dir, &worktree.path).expect("removed");
        assert!(!worktree.path.exists());

        let branches = run_git(&scratch.dir, &["branch", "--list", "agent/cccccccc"]).expect("git");
        assert!(
            branches.contains("agent/cccccccc"),
            "the branch is left for a person to delete: {branches}"
        );
    }

    #[test]
    fn the_repository_root_is_found_from_a_subdirectory() {
        let scratch = Scratch::new("root");
        if !scratch.available {
            return;
        }

        let nested = scratch.dir.join("src").join("deep");
        std::fs::create_dir_all(&nested).expect("create");
        let root = repository_root(&nested).expect("a root");
        assert_eq!(
            root.canonicalize().expect("canonical"),
            scratch.dir.canonicalize().expect("canonical"),
        );
    }
}

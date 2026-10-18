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
    #[error("{0} is not one of the app's worktrees")]
    OutsideBase(PathBuf),
    #[error("{0} is a worktree of a repository that cannot be found, so it is left alone")]
    UnknownRepository(PathBuf),
    #[error("{0} is no longer a worktree and still has files in it")]
    NotEmpty(PathBuf),
    #[error("{0} belongs to another repository")]
    TakenByAnotherRepository(PathBuf),
    #[error("could not remove {path}: {source}")]
    Remove {
        path: PathBuf,
        source: std::io::Error,
    },
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
        // Reusing a directory that belongs to a *different* repository would hand
        // one checkout to two repositories: the branch name comes from the seed
        // alone, so two runs in two repositories can derive the same name.
        return match repository_of(&directory) {
            Some(existing) if same_path(&existing, repo) => Ok(Worktree {
                path: directory,
                branch: branch.to_string(),
            }),
            Some(_) => Err(WorktreeError::TakenByAnotherRepository(directory)),
            None => Err(WorktreeError::TakenByAnotherRepository(directory)),
        };
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
///
/// Without `force` git refuses a worktree that has uncommitted changes, and its
/// own refusal is the error: nothing here parses a message to decide that.
pub fn remove(repo: &Path, path: &Path, force: bool) -> Result<(), WorktreeError> {
    let path_text = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_text);
    run_git(repo, &args)?;
    Ok(())
}

/// A worktree, or a directory where one used to be, described well enough to
/// decide what to do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeState {
    pub path: PathBuf,
    /// The branch it has checked out. `None` when it is no longer a worktree.
    pub branch: Option<String>,
    /// Files git reports as changed or untracked.
    pub changes: usize,
    /// Commits this branch has that the repository's main worktree does not.
    pub commits: usize,
    /// The branch `commits` is measured against, when one could be read.
    pub base: Option<String>,
    /// Entries in the directory, so an empty leftover is told from a full one.
    pub entries: usize,
}

/// Survey the directory the app creates worktrees in.
///
/// Each entry is described from the directory itself: git is asked about it, not
/// about a repository the caller has to name, so a worktree of a repository this
/// app instance never opened is still described correctly. A missing directory is
/// not an error - it is what a first run looks like.
pub fn survey(base: &Path) -> Vec<WorktreeState> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };

    let mut out: Vec<WorktreeState> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| describe(&path))
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// One directory, described by asking git about it.
fn describe(path: &Path) -> WorktreeState {
    let mut state = WorktreeState {
        path: path.to_path_buf(),
        branch: None,
        changes: 0,
        commits: 0,
        base: None,
        entries: count_entries(path),
    };

    // A directory with no `.git` of its own is not a worktree, whatever happens to
    // be above it: it is where a worktree used to be. Asking git here would find
    // the repository that contains the app's data directory, which is not an
    // answer about this directory at all.
    if !path.join(".git").exists() {
        return state;
    }

    // Not a worktree any more: a directory whose registration went away, which is
    // what a removal that died half way leaves behind. Nothing else is asked.
    let Ok(head) = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"]) else {
        return state;
    };
    let branch = head.trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return state;
    }

    state.changes = run_git(path, &["status", "--porcelain"])
        .map(|out| out.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0);

    // Measured against the main worktree's branch: what this branch has that the
    // checkout a person looks at does not.
    if let Some(base) = base_branch(path) {
        state.commits = run_git(path, &["rev-list", "--count", &format!("{base}..HEAD")])
            .ok()
            .and_then(|out| out.trim().parse().ok())
            .unwrap_or(0);
        state.base = Some(base);
    }

    state.branch = Some(branch);
    state
}

/// The branch the repository's main worktree has checked out.
///
/// `git worktree list` always lists the main worktree first, so the first branch
/// it names is the one a person would call the repository's own.
fn base_branch(path: &Path) -> Option<String> {
    run_git(path, &["worktree", "list", "--porcelain"])
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("branch refs/heads/"))
        .map(|name| name.trim().to_string())
}

/// Remove one of the app's worktrees, or a directory left where one was.
///
/// `force` is the caller's answer to "has a person seen what this costs?". Without
/// it, git refuses a worktree with uncommitted changes, and a leftover keeps its
/// files. The branch is never deleted, so commits stay reachable by name even when
/// a worktree goes.
pub fn discard(base: &Path, path: &Path, force: bool) -> Result<(), WorktreeError> {
    if !is_inside(base, path) {
        return Err(WorktreeError::OutsideBase(path.to_path_buf()));
    }

    match repository_of(path) {
        Some(repo) => remove(&repo, path, force),
        // A live worktree whose repository cannot be named is left alone: it is
        // registered somewhere, and deleting its files would leave that
        // registration pointing at nothing.
        None if is_live_worktree(path) => Err(WorktreeError::UnknownRepository(path.to_path_buf())),
        None => {
            if !force && count_entries(path) > 0 {
                return Err(WorktreeError::NotEmpty(path.to_path_buf()));
            }
            std::fs::remove_dir_all(path).map_err(|source| WorktreeError::Remove {
                path: path.to_path_buf(),
                source,
            })
        }
    }
}

/// The repository a worktree belongs to, read from its own `.git`.
///
/// In a worktree, `.git` is a file naming the git directory it is registered in, so
/// the repository is that path with `.git/worktrees/<name>` taken off. Reading it
/// means nothing has to be assumed about where the app was started or which
/// checkout a person happens to have open. A plain checkout - where `.git` is a
/// directory - is its own repository.
pub fn repository_of(path: &Path) -> Option<PathBuf> {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return Some(path.to_path_buf());
    }

    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    if target.is_empty() {
        return None;
    }
    let target = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        path.join(target)
    };

    let git_dir = target.parent()?.parent()?;
    if git_dir.file_name()? != ".git" {
        // A git directory somewhere else entirely: the repository root is not
        // recoverable from the link alone, and a guess would be worse than saying
        // so.
        return None;
    }
    git_dir.parent().map(Path::to_path_buf)
}

/// Whether `path` is a worktree right now: its link points at a git directory that
/// exists.
pub fn is_live_worktree(path: &Path) -> bool {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return true;
    }
    let Ok(text) = std::fs::read_to_string(&dot_git) else {
        return false;
    };
    let Some(target) = text.trim().strip_prefix("gitdir:") else {
        return false;
    };
    let target = Path::new(target.trim());
    if target.is_absolute() {
        target.exists()
    } else {
        path.join(target).exists()
    }
}

/// Whether `path` is `base` itself or inside it.
///
/// Compared as text, case-insensitively, because that is how Windows and macOS
/// filesystems behave and a worktree path is written by a person or by git rather
/// than by this process.
fn is_inside(base: &Path, path: &Path) -> bool {
    let base = normalised(base);
    let path = normalised(path);
    path == base || path.starts_with(&format!("{base}/"))
}

/// Two paths that mean the same place.
///
/// Canonicalised when both exist, which is what settles the short-name and
/// case forms Windows can report the same directory under.
fn same_path(first: &Path, second: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (first.canonicalize(), second.canonicalize()) {
        return normalised(&a) == normalised(&b);
    }
    normalised(first) == normalised(second)
}

fn normalised(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed
    }
}

fn count_entries(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
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
        remove(&scratch.dir, &worktree.path, false).expect("removed");
        assert!(!worktree.path.exists());

        let branches = run_git(&scratch.dir, &["branch", "--list", "agent/cccccccc"]).expect("git");
        assert!(
            branches.contains("agent/cccccccc"),
            "the branch is left for a person to delete: {branches}"
        );
    }

    #[test]
    fn a_survey_describes_a_worktree_and_a_leftover() {
        let scratch = Scratch::new("survey");
        if !scratch.available {
            return;
        }

        let worktree = create(&scratch.dir, &scratch.base(), "agent/aaaaaaaa").expect("created");
        std::fs::write(worktree.path.join("changed.txt"), "work\n").expect("write");

        // Two leftovers: what a removal that died half way leaves behind, empty
        // and not.
        let empty = scratch.base().join("agent-deadbeef");
        std::fs::create_dir_all(&empty).expect("create");
        let occupied = scratch.base().join("agent-feedface");
        std::fs::create_dir_all(&occupied).expect("create");
        std::fs::write(occupied.join("left.txt"), "left\n").expect("write");

        let states = survey(&scratch.base());
        assert_eq!(states.len(), 3);

        let live = states
            .iter()
            .find(|state| state.path == worktree.path)
            .expect("the worktree");
        assert_eq!(live.branch.as_deref(), Some("agent/aaaaaaaa"));
        assert_eq!(live.changes, 1, "one changed file");
        assert_eq!(live.commits, 0, "nothing committed on it yet");
        assert!(
            live.base.is_some(),
            "the branch is counted against the main worktree's"
        );

        let leftover = states
            .iter()
            .find(|state| state.path == empty)
            .expect("the empty leftover");
        assert_eq!(leftover.branch, None);
        assert_eq!(leftover.entries, 0);

        let full = states
            .iter()
            .find(|state| state.path == occupied)
            .expect("the full leftover");
        assert_eq!(full.entries, 1);
    }

    #[test]
    fn a_leftover_with_files_is_kept_unless_a_person_says_so() {
        let scratch = Scratch::new("leftover");
        if !scratch.available {
            return;
        }

        let full = scratch.base().join("agent-feedface");
        std::fs::create_dir_all(&full).expect("create");
        std::fs::write(full.join("left.txt"), "left\n").expect("write");

        let refused = discard(&scratch.base(), &full, false);
        assert!(
            matches!(refused, Err(WorktreeError::NotEmpty(_))),
            "expected NotEmpty, got {refused:?}"
        );
        assert!(full.join("left.txt").is_file(), "nothing was deleted");

        discard(&scratch.base(), &full, true).expect("removed when asked twice");
        assert!(!full.exists());
    }

    #[test]
    fn an_empty_leftover_goes_without_being_forced() {
        let scratch = Scratch::new("emptyset");
        if !scratch.available {
            return;
        }

        let empty = scratch.base().join("agent-deadbeef");
        std::fs::create_dir_all(&empty).expect("create");

        discard(&scratch.base(), &empty, false).expect("removed");
        assert!(!empty.exists());
    }

    #[test]
    fn a_worktree_is_removed_through_git_and_not_just_from_disk() {
        let scratch = Scratch::new("discard");
        if !scratch.available {
            return;
        }

        let worktree = create(&scratch.dir, &scratch.base(), "agent/cccccccc").expect("created");
        discard(&scratch.base(), &worktree.path, false).expect("removed");
        assert!(!worktree.path.exists());

        let listing = run_git(&scratch.dir, &["worktree", "list"]).expect("git");
        assert!(
            !listing.contains("agent-cccccccc"),
            "git's own bookkeeping goes too: {listing}"
        );
    }

    #[test]
    fn a_directory_outside_the_apps_worktrees_is_refused() {
        let scratch = Scratch::new("outside");
        if !scratch.available {
            return;
        }

        let elsewhere = scratch.dir.join("somewhere-else");
        std::fs::create_dir_all(&elsewhere).expect("create");

        let refused = discard(&scratch.base(), &elsewhere, true);
        assert!(
            matches!(refused, Err(WorktreeError::OutsideBase(_))),
            "expected OutsideBase, got {refused:?}"
        );
        assert!(elsewhere.is_dir(), "and it is still there");
    }

    #[test]
    fn a_worktree_of_another_repository_is_not_reused() {
        let first = Scratch::new("repo-one");
        let second = Scratch::new("repo-two");
        if !first.available || !second.available {
            return;
        }

        // The branch name comes from the seed alone, so two repositories can
        // derive the same name; the directory must not be handed over.
        let mine = create(&first.dir, &first.base(), "agent/aaaaaaaa").expect("created");
        let taken = create(&second.dir, &first.base(), "agent/aaaaaaaa");
        assert!(
            matches!(taken, Err(WorktreeError::TakenByAnotherRepository(_))),
            "expected TakenByAnotherRepository, got {taken:?}"
        );
        assert!(mine.path.is_dir(), "the first repository keeps it");
    }

    #[test]
    fn the_repository_is_read_from_the_worktrees_own_git_file() {
        let scratch = Scratch::new("readlink");
        if !scratch.available {
            return;
        }

        let worktree = create(&scratch.dir, &scratch.base(), "agent/dddddddd").expect("created");

        let found = repository_of(&worktree.path).expect("a repository");
        assert!(
            same_path(&found, &scratch.dir),
            "{found:?} is not {:?}",
            scratch.dir
        );
        assert!(is_live_worktree(&worktree.path));
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

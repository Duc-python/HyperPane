//! The agent catalogue.
//!
//! HyperPane does not implement agents — it starts the ones you already have
//! installed and gives each one a pane. The catalogue describes *how* to start
//! a given CLI and what the app can learn about it, so a new agent is a data
//! change rather than a code change.
//!
//! The built-in list below is a starting point. A `catalogue.json` beside the
//! app's data directory overrides and extends it (later in P2).

use std::path::PathBuf;

pub mod guard;
pub mod result;
pub mod worktree;

pub use result::{ResultScanner, RunResult, RunStatus, TestOutcome};

use serde::{Deserialize, Serialize};

/// How a task sentence reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Delivery {
    /// Passed as an argument at startup.
    StartArgument,
    /// The app starts the agent, then types the task into its prompt once the
    /// agent is up. Used for CLIs that take no prompt argument.
    PastedAfterStart,
}

/// What the app can tell you about installing a missing agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallHint {
    /// A one-line install command is available.
    Command,
    /// Documentation link only.
    Docs,
    /// Depends on how you obtained it.
    Varies,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSpec {
    /// Stable id used by `pane.spawn`. Never changes once published.
    pub id: &'static str,
    /// Human label for the UI.
    pub label: &'static str,
    /// Executable looked up on `PATH`.
    pub program: &'static str,
    pub delivery: Delivery,
    /// Flag the task is passed behind, when the CLI expects one. `None` means
    /// the task is the first argument.
    pub prompt_flag: Option<&'static str>,
    /// The app can tell `working` / `blocked` / `done` apart for this agent.
    pub status_detection: bool,
    /// The app's usage readout covers this agent.
    pub token_meter: bool,
    pub install: InstallHint,
}

impl AgentSpec {
    /// Program and arguments for starting this agent with `task`.
    ///
    /// Agents that take no prompt argument are started bare; the app types the
    /// task afterwards.
    pub fn launch(&self, task: &str) -> (&'static str, Vec<String>) {
        let trimmed = task.trim();
        match self.delivery {
            Delivery::StartArgument if !trimmed.is_empty() => match self.prompt_flag {
                Some(flag) => (self.program, vec![flag.to_string(), trimmed.to_string()]),
                None => (self.program, vec![trimmed.to_string()]),
            },
            _ => (self.program, Vec::new()),
        }
    }

    /// Whether the task has to be typed in after the agent starts.
    pub fn needs_paste(&self) -> bool {
        matches!(self.delivery, Delivery::PastedAfterStart)
    }

    /// Whether this agent's program is on `PATH`.
    pub fn is_installed(&self) -> bool {
        resolve_program(self.program).is_some()
    }
}

/// Built-in catalogue, ordered by how commonly they are used as pane agents.
pub const BUILTIN: &[AgentSpec] = &[
    AgentSpec {
        id: "claude-code",
        label: "Claude Code",
        program: "claude",
        delivery: Delivery::StartArgument,
        prompt_flag: None,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "codex",
        label: "Codex",
        program: "codex",
        delivery: Delivery::StartArgument,
        prompt_flag: None,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "opencode",
        label: "opencode",
        program: "opencode",
        // Verified against the installed CLI: `opencode --prompt "<task>"`
        // starts it with the task already submitted.
        delivery: Delivery::StartArgument,
        prompt_flag: Some("--prompt"),
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "gemini",
        label: "Gemini CLI",
        program: "gemini",
        delivery: Delivery::StartArgument,
        prompt_flag: None,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "qwen-code",
        label: "Qwen Code",
        program: "qwen",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "cursor",
        label: "Cursor CLI",
        program: "cursor-agent",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "grok",
        label: "Grok CLI",
        program: "grok",
        delivery: Delivery::StartArgument,
        prompt_flag: None,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "goose",
        label: "Goose",
        program: "goose",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "aider",
        label: "Aider",
        program: "aider",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "crush",
        label: "Crush",
        program: "crush",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "copilot",
        label: "GitHub Copilot CLI",
        program: "copilot",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "amp",
        label: "Amp",
        program: "amp",
        delivery: Delivery::PastedAfterStart,
        prompt_flag: None,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Docs,
    },
];

/// The whole catalogue: built-ins, then anything loaded from disk.
pub fn catalogue() -> &'static [AgentSpec] {
    BUILTIN
}

/// Look up one agent by the exact id used in `pane.spawn`.
pub fn by_id(id: &str) -> Option<&'static AgentSpec> {
    catalogue().iter().find(|agent| agent.id == id)
}

/// Ids of every known agent, for error messages and the palette.
pub fn known_ids() -> Vec<&'static str> {
    catalogue().iter().map(|agent| agent.id).collect()
}

/// File names to try for a program, in order.
fn executable_names(program: &str) -> Vec<String> {
    if program.contains('.') {
        return vec![program.to_string()];
    }

    #[cfg(windows)]
    {
        // An npm-installed CLI is a `.cmd` shim, not a `.exe`, so a bare name
        // does not name a file that can be started.
        let mut names = vec![format!("{program}.cmd")];
        let extensions =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        for extension in extensions.split(';') {
            let extension = extension.trim().to_ascii_lowercase();
            if !extension.is_empty() {
                names.push(format!("{program}{extension}"));
            }
        }
        names
    }
    #[cfg(not(windows))]
    {
        vec![program.to_string()]
    }
}

/// The file that `program` names, searched on `PATH`.
pub fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        for name in executable_names(program) {
            let candidate = directory.join(&name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Program and arguments that will actually start.
///
/// A resolved `.cmd` or `.bat` has to go through `cmd.exe`: the Windows process
/// API starts executables, and a batch file is not one.
pub fn spawn_argv(program: &str, args: &[String]) -> (String, Vec<String>) {
    let Some(path) = resolve_program(program) else {
        // Leave it alone so the pane reports the failure it would report in a
        // terminal, rather than failing in a way that looks like an app bug.
        return (program.to_string(), args.to_vec());
    };

    command_for(&path, args)
}

/// The command line that starts a resolved program.
fn command_for(path: &std::path::Path, args: &[String]) -> (String, Vec<String>) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "cmd" || extension == "bat" {
        let mut wrapped = vec!["/c".to_string(), path.to_string_lossy().into_owned()];
        wrapped.extend(args.iter().map(|arg| single_line(arg)));
        ("cmd.exe".to_string(), wrapped)
    } else {
        (path.to_string_lossy().into_owned(), args.to_vec())
    }
}

/// An argument that survives the trip through a batch shim.
///
/// A command line cannot carry a line break through `cmd.exe`: the break ends
/// the command there, and everything after it never reaches the program. A team
/// brief - five sections, each on its own line - arrived at the agent as the
/// single word "GOAL", so the agent asked what the goal was. Breaks become
/// spaces, which is how one paragraph reads anyway.
fn single_line(argument: &str) -> String {
    let mut out = String::with_capacity(argument.len());
    let mut after_space = false;
    for character in argument.chars() {
        let character = match character {
            '\r' | '\n' | '\t' => ' ',
            other => other,
        };
        if character == ' ' {
            if after_space {
                continue;
            }
            after_space = true;
        } else {
            after_space = false;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = known_ids();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate agent id in the catalogue");
    }

    #[test]
    fn start_argument_agents_receive_the_task() {
        let agent = by_id("claude-code").expect("claude-code is built in");
        let (program, args) = agent.launch("fix the build");
        assert_eq!(program, "claude");
        assert_eq!(args, vec!["fix the build".to_string()]);
        assert!(!agent.needs_paste());
    }

    #[test]
    fn a_prompt_flag_is_placed_before_the_task() {
        let agent = by_id("opencode").expect("opencode is built in");
        let (_, args) = agent.launch("fix the build");
        assert_eq!(
            args,
            vec!["--prompt".to_string(), "fix the build".to_string()]
        );
    }

    #[test]
    fn pasted_agents_start_bare() {
        let agent = by_id("aider").expect("aider is built in");
        let (_, args) = agent.launch("fix the build");
        assert!(args.is_empty());
        assert!(agent.needs_paste());
    }

    #[test]
    fn an_empty_task_never_becomes_a_bare_argument() {
        let agent = by_id("codex").expect("codex is built in");
        let (_, args) = agent.launch("   ");
        assert!(args.is_empty());
    }

    #[test]
    fn unknown_ids_are_not_found() {
        assert!(by_id("definitely-not-an-agent").is_none());
    }

    #[test]
    fn a_program_that_cannot_exist_does_not_resolve() {
        assert!(resolve_program("hyperpane-not-a-real-program-xyz").is_none());
    }

    #[test]
    fn an_unresolved_program_is_left_for_the_shell_to_report() {
        let (program, args) = spawn_argv("hyperpane-not-a-real-program-xyz", &[]);
        assert_eq!(program, "hyperpane-not-a-real-program-xyz");
        assert!(args.is_empty());
    }
}

#[cfg(test)]
mod command_tests {
    use super::{command_for, single_line};
    use std::path::Path;

    #[test]
    fn a_brief_survives_the_batch_shim() {
        // cmd.exe ends the command at the first line break; a brief used to
        // arrive as its first word, and the agent asked what the goal was.
        let brief = "GOAL\nShould we ship it?\n\nSCOPE\nRead only".to_string();
        let (program, args) = command_for(
            Path::new("C:\\tools\\opencode.cmd"),
            &["--prompt".to_string(), brief],
        );

        assert_eq!(program, "cmd.exe");
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], "C:\\tools\\opencode.cmd");
        assert_eq!(args[2], "--prompt");
        assert!(!args[3].contains('\n'), "{:?}", args[3]);
        assert!(
            args[3].starts_with("GOAL Should we ship it? SCOPE Read only"),
            "{:?}",
            args[3]
        );
    }

    #[test]
    fn a_native_program_keeps_its_argument_exactly() {
        let brief = "GOAL\nShould we ship it?".to_string();
        let (program, args) = command_for(Path::new("C:\\tools\\opencode.exe"), &[brief.clone()]);
        assert_eq!(program, "C:\\tools\\opencode.exe");
        assert_eq!(args, vec![brief]);
    }

    #[test]
    fn runs_of_breaks_become_one_space() {
        assert_eq!(single_line("a\r\n\r\nb\tc  d"), "a b c d");
    }
}

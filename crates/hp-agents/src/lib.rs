//! The agent catalogue.
//!
//! HyperPane does not implement agents — it starts the ones you already have
//! installed and gives each one a pane. The catalogue describes *how* to start
//! a given CLI and what the app can learn about it, so a new agent is a data
//! change rather than a code change.
//!
//! The built-in list below is a starting point. A `catalogue.json` beside the
//! app's data directory overrides and extends it at runtime (P2).

use serde::{Deserialize, Serialize};

/// How a task sentence reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Delivery {
    /// Passed as the agent's own start prompt.
    StartArgument,
    /// The app starts the agent, then pastes the task into its prompt once the
    /// agent is up. Used for CLIs that do not accept a prompt argument.
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
    /// The app can tell `working` / `blocked` / `done` apart for this agent.
    pub status_detection: bool,
    /// The app's usage readout covers this agent.
    pub token_meter: bool,
    pub install: InstallHint,
}

impl AgentSpec {
    /// Program and arguments for starting this agent with `task`.
    ///
    /// Agents that take no prompt argument are started bare; the app pastes the
    /// task afterwards.
    pub fn launch(&self, task: &str) -> (&'static str, Vec<String>) {
        match self.delivery {
            Delivery::StartArgument if !task.is_empty() => (self.program, vec![task.to_string()]),
            _ => (self.program, Vec::new()),
        }
    }

    /// Whether the task has to be pasted after the agent starts.
    pub fn needs_paste(&self) -> bool {
        matches!(self.delivery, Delivery::PastedAfterStart)
    }
}

/// Built-in catalogue. Ordered by how commonly they are used as pane agents.
pub const BUILTIN: &[AgentSpec] = &[
    AgentSpec {
        id: "claude-code",
        label: "Claude Code",
        program: "claude",
        delivery: Delivery::StartArgument,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "codex",
        label: "Codex",
        program: "codex",
        delivery: Delivery::StartArgument,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "opencode",
        label: "opencode",
        program: "opencode",
        delivery: Delivery::PastedAfterStart,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "gemini",
        label: "Gemini CLI",
        program: "gemini",
        delivery: Delivery::StartArgument,
        status_detection: true,
        token_meter: true,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "qwen-code",
        label: "Qwen Code",
        program: "qwen",
        delivery: Delivery::PastedAfterStart,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "cursor",
        label: "Cursor CLI",
        program: "cursor-agent",
        delivery: Delivery::PastedAfterStart,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "grok",
        label: "Grok CLI",
        program: "grok",
        delivery: Delivery::StartArgument,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "goose",
        label: "Goose",
        program: "goose",
        delivery: Delivery::PastedAfterStart,
        status_detection: true,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "aider",
        label: "Aider",
        program: "aider",
        delivery: Delivery::PastedAfterStart,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "crush",
        label: "Crush",
        program: "crush",
        delivery: Delivery::PastedAfterStart,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Command,
    },
    AgentSpec {
        id: "copilot",
        label: "GitHub Copilot CLI",
        program: "copilot",
        delivery: Delivery::PastedAfterStart,
        status_detection: false,
        token_meter: false,
        install: InstallHint::Docs,
    },
    AgentSpec {
        id: "amp",
        label: "Amp",
        program: "amp",
        delivery: Delivery::PastedAfterStart,
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
    fn pasted_agents_start_bare() {
        let agent = by_id("opencode").expect("opencode is built in");
        let (_, args) = agent.launch("fix the build");
        assert!(args.is_empty());
        assert!(agent.needs_paste());
    }

    #[test]
    fn an_empty_task_never_becomes_a_bare_argument() {
        let agent = by_id("codex").expect("codex is built in");
        let (_, args) = agent.launch("");
        assert!(args.is_empty());
    }

    #[test]
    fn unknown_ids_are_not_found() {
        assert!(by_id("definitely-not-an-agent").is_none());
    }
}

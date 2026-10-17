//! The servers a person declared, and the ones currently running.
//!
//! Nothing here is started until it is asked for: a plugin is a process, and
//! starting every server in the file at launch would make the app's own startup
//! depend on programs it does not own. So a call starts its server on first use
//! and keeps it afterwards.
//!
//! A failure is remembered rather than retried. A server that cannot start -
//! `npx` fetching a package that does not exist, a command that is not there -
//! would otherwise cost a startup timeout on every click, and the person would
//! be watching a spinner instead of reading the reason. The reason is kept and
//! shown; reloading the file is what clears it.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::trust::{Approval, ToolChange, Trust};
use crate::{Called, McpError, Options, Peer, Server, ServerSpec, Tool};

/// One server as a person wrote it down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// Stable id this file and the activity log refer to it by.
    pub id: String,
    /// Shown instead of the id when it is there.
    #[serde(default)]
    pub title: Option<String>,
    /// Program to run, as a person would type it.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// A server that is switched off is still kept, so turning it back on is
    /// one word in the file rather than retyping the whole entry.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Plugin {
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.id)
    }
}

/// The whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFile {
    pub version: u32,
    #[serde(default)]
    pub servers: Vec<Plugin>,
}

/// The shipped starting point: a file with nothing in it, which is a valid file.
pub const EMPTY_FILE: &str = r#"{
  "version": 1,
  "servers": []
}
"#;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("the plugin file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the plugin file is version {found}; this build understands version {expected}")]
    Version { found: u32, expected: u32 },
    #[error("no plugin called {0}")]
    Unknown(String),
    #[error("plugin {0} is switched off in the plugin file")]
    Disabled(String),
    #[error("plugin {id} has not been approved: {why}")]
    Unapproved { id: String, why: String },
    #[error("plugin {id} did not start: {problem}")]
    Broken { id: String, problem: String },
    #[error(transparent)]
    Server(#[from] McpError),
}

/// One plugin as the UI needs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
    /// `declared`, `running` or `broken`.
    pub state: &'static str,
    /// Why it is broken, when it is.
    pub problem: Option<String>,
    /// What the server called itself, once it has been asked.
    pub peer: Option<String>,
    /// `modern` or `legacy`, once it has been asked.
    pub era: Option<&'static str>,
    pub protocol_version: Option<String>,
    /// The last thing the server said on its own error stream. This is where a
    /// server explains itself, so it is kept beside whatever went wrong.
    pub last_words: Vec<String>,
    /// Whether this declaration has been read and approved as it stands.
    pub approved: bool,
    /// Why it needs approving, when it does: never approved, or changed since.
    pub approval_note: Option<String>,
    /// How the tools differ from the ones approved, when they do.
    pub tool_change_note: Option<String>,
    pub tools: Vec<ToolView>,
}

/// One tool, as the UI needs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The tool's own schema for its arguments, so the UI can show what it
    /// takes rather than guessing.
    pub input_schema: Value,
}

#[derive(Debug, Default)]
struct Entry {
    server: Option<Server>,
    tools: Vec<Tool>,
    peer: Option<Peer>,
    /// Why this plugin could not be started. Kept so that asking again is a
    /// refusal rather than another wait.
    problem: Option<String>,
}

/// The declared plugins and the servers among them that are running.
#[derive(Debug, Default)]
pub struct Host {
    plugins: Vec<Plugin>,
    specs: HashMap<String, ServerSpec>,
    entries: HashMap<String, Entry>,
    /// What is wrong with the file, in a person's words.
    problems: Vec<String>,
    options: Options,
}

impl Host {
    /// Read a plugin file.
    ///
    /// `resolve` turns a declared command into one that can actually be started.
    /// On Windows an npm-installed server is a `.cmd` shim, which the process API
    /// will not start by name, and the caller already knows how to handle that
    /// for agents.
    pub fn load(
        text: &str,
        resolve: impl Fn(&str, &[String]) -> (String, Vec<String>),
    ) -> Result<Self, PluginError> {
        Self::with_options(text, resolve, Options::default())
    }

    pub fn with_options(
        text: &str,
        resolve: impl Fn(&str, &[String]) -> (String, Vec<String>),
        options: Options,
    ) -> Result<Self, PluginError> {
        let file: PluginFile = serde_json::from_str(text)?;
        if file.version != 1 {
            return Err(PluginError::Version {
                found: file.version,
                expected: 1,
            });
        }

        let mut problems = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for plugin in &file.servers {
            if !seen.contains(&plugin.id.as_str()) {
                seen.push(&plugin.id);
            } else {
                problems.push(format!(
                    "two plugins share the id {}: the first one is the one that runs",
                    plugin.id
                ));
            }
            if plugin.id.trim().is_empty() {
                problems.push("a plugin has no id, so nothing can refer to it".to_string());
            }
            if plugin.command.trim().is_empty() {
                problems.push(format!("{}: no command to run", plugin.id));
            }
        }

        let mut specs = HashMap::new();
        for plugin in &file.servers {
            if specs.contains_key(&plugin.id) {
                continue;
            }
            let (command, args) = resolve(&plugin.command, &plugin.args);
            specs.insert(
                plugin.id.clone(),
                ServerSpec {
                    id: plugin.id.clone(),
                    command,
                    args,
                    env: plugin.env.clone(),
                },
            );
        }

        // A duplicated id keeps the first entry only, which is what the problem
        // above says will happen.
        let mut plugins: Vec<Plugin> = Vec::new();
        for plugin in file.servers {
            if plugins.iter().any(|kept| kept.id == plugin.id) {
                continue;
            }
            plugins.push(plugin);
        }

        Ok(Self {
            plugins,
            specs,
            entries: HashMap::new(),
            problems,
            options,
        })
    }

    /// What is wrong with the file, which is not the same as what is wrong with
    /// a server: a plugin that cannot start says so when it is asked to.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn views(&self, trust: &Trust) -> Vec<PluginView> {
        self.plugins
            .iter()
            .map(|plugin| self.view(plugin, trust))
            .collect()
    }

    /// What the file states about a plugin, in the shape an approval takes.
    pub fn declaration(&self, id: &str) -> Result<Approval, PluginError> {
        let plugin = self.plugin(id)?;
        Ok(Approval::of(&plugin.command, &plugin.args, &plugin.env))
    }

    /// Start a plugin if it is not running, and say what it offers.
    ///
    /// A declaration nobody has read does not start. That is the consent step
    /// the specification requires of a client that runs a local server, and it
    /// is why the panel shows the exact command before offering the button.
    pub fn open(&mut self, id: &str, trust: &Trust) -> Result<PluginView, PluginError> {
        // Switched off is about the file and is the more useful thing to say
        // first; approval is about the person.
        let plugin = self.plugin(id)?.clone();
        if !plugin.enabled {
            return Err(PluginError::Disabled(id.to_string()));
        }

        let declaration = self.declaration(id)?;
        let verdict = trust.verdict(id, &declaration);
        if !verdict.is_approved() {
            return Err(PluginError::Unapproved {
                id: id.to_string(),
                why: verdict
                    .message()
                    .unwrap_or_else(|| "it has not been approved".to_string()),
            });
        }

        self.run(&plugin)?;
        Ok(self.view(&plugin, trust))
    }

    /// Start a plugin the person has just read and approved.
    ///
    /// Approving and starting are one action on purpose: the thing that shows
    /// the command is the thing that runs it. What the server offers is handed
    /// back so the caller can record it beside the declaration, which is what
    /// makes a later rewrite of a tool visible.
    pub fn start_approved(&mut self, id: &str) -> Result<(PluginView, Vec<Tool>), PluginError> {
        let plugin = self.plugin(id)?.clone();
        self.run(&plugin)?;

        let tools = self
            .entries
            .get(id)
            .map(|entry| entry.tools.clone())
            .unwrap_or_default();
        Ok((self.view(&plugin, &Trust::default()), tools))
    }

    /// Run it, whatever anyone has approved: the entry points above have
    /// already decided whether that is allowed.
    fn run(&mut self, plugin: &Plugin) -> Result<(), PluginError> {
        let id = plugin.id.as_str();
        if !plugin.enabled {
            return Err(PluginError::Disabled(id.to_string()));
        }

        if let Some(problem) = self.entries.get(id).and_then(|entry| entry.problem.clone()) {
            return Err(PluginError::Broken {
                id: id.to_string(),
                problem,
            });
        }

        if self
            .entries
            .get(id)
            .is_none_or(|entry| entry.server.is_none())
        {
            match self.start(plugin) {
                Ok(entry) => {
                    self.entries.insert(id.to_string(), entry);
                    Ok(())
                }
                Err(error) => {
                    // Remembered rather than retried: see the note at the top of
                    // this file. A server that dies before it answers has no
                    // words left to keep, so the message names the command that
                    // was tried and a person can run it by hand to see why.
                    let tried = self
                        .specs
                        .get(id)
                        .map(|spec| format!("{} {}", spec.command, spec.args.join(" ")))
                        .unwrap_or_else(|| plugin.command.clone());
                    let problem = format!("{error} (tried: {tried})");
                    self.entries.insert(
                        id.to_string(),
                        Entry {
                            problem: Some(problem.clone()),
                            ..Entry::default()
                        },
                    );
                    Err(PluginError::Broken {
                        id: id.to_string(),
                        problem,
                    })
                }
            }
        } else {
            Ok(())
        }
    }

    /// Call one tool, starting the plugin first if it is not running.
    pub fn call(
        &mut self,
        id: &str,
        tool: &str,
        arguments: Value,
        trust: &Trust,
    ) -> Result<Called, PluginError> {
        self.open(id, trust)?;

        let (outcome, last_words) = {
            let entry = self
                .entries
                .get_mut(id)
                .ok_or_else(|| PluginError::Broken {
                    id: id.to_string(),
                    problem: "it was not running and could not be started".to_string(),
                })?;
            let Some(server) = entry.server.as_mut() else {
                return Err(PluginError::Broken {
                    id: id.to_string(),
                    problem: "it is not running".to_string(),
                });
            };

            match server.call(tool, arguments) {
                Ok(called) => (Ok(called), Vec::new()),
                Err(error) => {
                    let words = server.stderr_tail();
                    (Err(error), words)
                }
            }
        };

        match outcome {
            Ok(called) => Ok(called),
            Err(error) => {
                // A refusal means the server answered and did not like the
                // request, so it is still working and stays usable. Anything
                // else - junk, silence, a closed pipe - means this process is
                // finished with, and what it said on the way down is the only
                // explanation there will be.
                if !matches!(error, McpError::Refused { .. }) {
                    if let Some(entry) = self.entries.get_mut(id) {
                        entry.problem = Some(match last_words.is_empty() {
                            true => error.to_string(),
                            false => {
                                format!("{error} (the server said: {})", last_words.join(" / "))
                            }
                        });
                        entry.server = None;
                    }
                }
                Err(PluginError::Server(error))
            }
        }
    }

    /// End one plugin's process, keeping what the file says about it.
    ///
    /// What the process offered goes with it: a tool list is that server's
    /// answer about itself, and there is no longer a server to have answered.
    pub fn close(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            if let Some(server) = entry.server.take() {
                server.shutdown();
            }
            entry.tools.clear();
            entry.peer = None;
        }
    }

    /// End every running server. Called when the app is on its way out.
    pub fn shutdown(&mut self) {
        for entry in self.entries.values_mut() {
            if let Some(server) = entry.server.take() {
                server.shutdown();
            }
        }
    }

    fn plugin(&self, id: &str) -> Result<&Plugin, PluginError> {
        self.plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| PluginError::Unknown(id.to_string()))
    }

    fn start(&mut self, plugin: &Plugin) -> Result<Entry, McpError> {
        let spec = self
            .specs
            .get(&plugin.id)
            .cloned()
            .unwrap_or_else(|| ServerSpec {
                id: plugin.id.clone(),
                command: plugin.command.clone(),
                args: plugin.args.clone(),
                env: plugin.env.clone(),
            });

        let mut server = Server::start_with(spec, self.options)?;
        let tools = server.tools()?;
        let peer = server.peer().clone();

        Ok(Entry {
            server: Some(server),
            tools,
            peer: Some(peer),
            problem: None,
        })
    }

    fn view(&self, plugin: &Plugin, trust: &Trust) -> PluginView {
        let entry = self.entries.get(&plugin.id);
        let running = entry.is_some_and(|entry| entry.server.is_some());

        let declaration = Approval::of(&plugin.command, &plugin.args, &plugin.env);
        let verdict = trust.verdict(&plugin.id, &declaration);
        // Only a server that has run can have described its tools, so an entry
        // that has never started has nothing to compare.
        let tool_change = entry
            .filter(|entry| entry.server.is_some())
            .map(|entry| trust.tool_change(&plugin.id, &entry.tools))
            .unwrap_or(ToolChange::Unknown);

        PluginView {
            id: plugin.id.clone(),
            title: plugin.label().to_string(),
            command: plugin.command.clone(),
            args: plugin.args.clone(),
            enabled: plugin.enabled,
            state: match (running, entry.and_then(|entry| entry.problem.as_ref())) {
                (true, _) => "running",
                (false, Some(_)) => "broken",
                (false, None) => "declared",
            },
            approved: verdict.is_approved(),
            approval_note: verdict.message(),
            tool_change_note: tool_change.message(),
            problem: entry.and_then(|entry| entry.problem.clone()),
            peer: entry
                .and_then(|entry| entry.peer.as_ref())
                .map(|peer| peer.label(&plugin.id)),
            era: entry
                .and_then(|entry| entry.peer.as_ref())
                .map(|peer| match peer.era {
                    crate::Era::Modern => "modern",
                    crate::Era::Legacy => "legacy",
                }),
            protocol_version: entry
                .and_then(|entry| entry.peer.as_ref())
                .map(|peer| peer.protocol_version.clone()),
            last_words: entry
                .and_then(|entry| entry.server.as_ref())
                .map(|server| server.stderr_tail())
                .unwrap_or_default(),
            tools: entry
                .map(|entry| {
                    entry
                        .tools
                        .iter()
                        .map(|tool| ToolView {
                            name: tool.name.clone(),
                            title: tool.title.clone(),
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_empty_file_is_a_valid_file() {
        let host = Host::load(EMPTY_FILE, |command, args| {
            (command.to_string(), args.to_vec())
        })
        .expect("the shipped file parses");

        assert!(host.views(&Trust::default()).is_empty());
        assert!(host.problems().is_empty());
    }

    #[test]
    fn a_file_from_a_later_version_is_refused() {
        let file = r#"{"version": 2, "servers": []}"#;
        let error = Host::load(file, |command, args| (command.to_string(), args.to_vec()))
            .expect_err("a version this build does not know");

        assert!(matches!(
            error,
            PluginError::Version {
                found: 2,
                expected: 1
            }
        ));
    }

    #[test]
    fn a_plugin_with_two_entries_keeps_the_first_and_says_so() {
        let file = r#"{
            "version": 1,
            "servers": [
                { "id": "weather", "command": "first" },
                { "id": "weather", "command": "second" }
            ]
        }"#;
        let host = Host::load(file, |command, args| (command.to_string(), args.to_vec()))
            .expect("it parses");

        assert_eq!(host.views(&Trust::default()).len(), 1);
        assert_eq!(host.views(&Trust::default())[0].command, "first");
        assert!(
            host.problems()
                .iter()
                .any(|problem| problem.contains("share the id weather")),
            "problems were {:?}",
            host.problems()
        );
    }

    #[test]
    fn a_plugin_with_nothing_to_run_is_a_problem() {
        let file = r#"{"version": 1, "servers": [{"id": "empty", "command": "  "}]}"#;
        let host = Host::load(file, |command, args| (command.to_string(), args.to_vec()))
            .expect("it parses");

        assert!(
            host.problems()
                .iter()
                .any(|problem| problem.contains("no command")),
            "problems were {:?}",
            host.problems()
        );
    }

    #[test]
    fn a_plugin_is_resolved_into_what_will_actually_be_started() {
        let file = r#"{
            "version": 1,
            "servers": [{ "id": "weather", "command": "npx", "args": ["-y", "server-weather"] }]
        }"#;
        let host = Host::load(file, |command, args| {
            (format!("{command}.cmd"), args.to_vec())
        })
        .expect("it parses");

        // The view shows what the person wrote, not what the resolver made of it.
        assert_eq!(host.views(&Trust::default())[0].command, "npx");
        assert_eq!(
            host.views(&Trust::default())[0].args,
            ["-y", "server-weather"]
        );
    }

    #[test]
    fn nothing_is_started_by_reading_the_file() {
        let file = r#"{"version": 1, "servers": [{"id": "weather", "command": "npx"}]}"#;
        let host = Host::load(file, |command, args| (command.to_string(), args.to_vec()))
            .expect("it parses");

        assert_eq!(host.views(&Trust::default())[0].state, "declared");
        assert!(host.views(&Trust::default())[0].tools.is_empty());
    }

    #[test]
    fn an_id_nothing_declares_names_the_id_it_could_not_find() {
        let mut host = Host::load(EMPTY_FILE, |command, args| {
            (command.to_string(), args.to_vec())
        })
        .expect("it parses");

        match host.open("weather", &Trust::default()) {
            Err(PluginError::Unknown(id)) => assert_eq!(id, "weather"),
            other => panic!("expected an unknown plugin, got {other:?}"),
        }
    }
}

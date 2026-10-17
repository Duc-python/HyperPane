//! HyperPane desktop shell: Tauri commands over the PTY and layout layers.
//!
//! The frontend owns pane identity (it generates ids), the backend owns pane
//! *state*. Commands return the whole workspace after a layout change so the UI
//! never has to guess what the tree looks like.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hp_agents::result::{ResultScanner, RunResult};
use hp_core::{Session, SplitDirection, Workspace};
use hp_pty::{EventSink, PaneManager, PaneSpec, PtyEvent};
use hp_store::SessionStore;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};

/// What the app knows about an agent pane beyond its process.
#[derive(Default)]
struct RunState {
    agent: Option<String>,
    result: Option<RunResult>,
    /// Watches the pane's output for the child's own `RESULT` line.
    scanner: ResultScanner,
}

/// Shared application state. The PTY manager is behind an `Arc` because
/// channel callbacks outlive the command that created them.
struct AppState {
    panes: Arc<PaneManager>,
    store: SessionStore,
    /// Where generated shell integration scripts are written.
    integration_dir: PathBuf,
    /// Where agent worktrees are created. Outside any repository, so opening a
    /// pane never changes a person's checkout.
    worktree_dir: PathBuf,
    /// Per-pane agent state, keyed by pane id.
    runs: Arc<Mutex<HashMap<String, RunState>>>,
    /// Where each pane's shell last said it was, from its own OSC 7 reports.
    /// Opening a worktree for an agent needs a directory to fork from, and the
    /// pane that has one is the pane the person is looking at.
    directories: Arc<Mutex<HashMap<String, String>>>,
    /// `None` until the frontend opens its first pane.
    workspace: Mutex<Option<Workspace>>,
}

impl AppState {
    fn new(
        store: SessionStore,
        integration_dir: PathBuf,
        worktree_dir: PathBuf,
        workspace: Option<Workspace>,
    ) -> Self {
        Self {
            panes: Arc::new(PaneManager::new()),
            store,
            integration_dir,
            worktree_dir,
            runs: Arc::new(Mutex::new(HashMap::new())),
            directories: Arc::new(Mutex::new(HashMap::new())),
            workspace: Mutex::new(workspace),
        }
    }

    fn update<F>(&self, change: F) -> Result<Workspace, String>
    where
        F: FnOnce(&mut Workspace),
    {
        let mut guard = self.workspace.lock();
        let workspace = guard
            .as_mut()
            .ok_or_else(|| "workspace is not initialised; call workspace_init first".to_string())?;
        change(workspace);
        Ok(workspace.clone())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
    default_shell: String,
    /// Used by the UI to shorten a path to `~/…`.
    home_dir: Option<String>,
}

/// The workspace currently on screen, or `null` before the first pane exists.
#[tauri::command]
fn workspace_get(state: State<'_, AppState>) -> Option<Workspace> {
    state.workspace.lock().clone()
}

/// Create the workspace around its first pane.
#[tauri::command]
fn workspace_init(
    pane: String,
    name: Option<String>,
    state: State<'_, AppState>,
) -> Result<Workspace, String> {
    if pane.trim().is_empty() {
        return Err("pane id must not be empty".into());
    }
    // Re-initialising the same pane is a no-op, so a reloaded webview does not
    // lose the layout it already had.
    if let Some(existing) = state.workspace.lock().as_ref() {
        if existing.layout.contains(&pane) && existing.layout.pane_ids().len() == 1 {
            return Ok(existing.clone());
        }
    }
    let workspace = Workspace::single("main", name.unwrap_or_else(|| "Main".into()), pane);
    *state.workspace.lock() = Some(workspace.clone());
    Ok(workspace)
}

/// Split `target`, putting `new_pane` beside it.
#[tauri::command]
fn workspace_split(
    target: String,
    direction: SplitDirection,
    new_pane: String,
    state: State<'_, AppState>,
) -> Result<Workspace, String> {
    state.update(|workspace| {
        workspace.add_pane(&target, direction, new_pane);
    })
}

#[tauri::command]
fn workspace_close_pane(pane: String, state: State<'_, AppState>) -> Result<Workspace, String> {
    state.update(|workspace| {
        workspace.close_pane(&pane);
    })
}

#[tauri::command]
fn workspace_set_focus(pane: String, state: State<'_, AppState>) -> Result<Workspace, String> {
    state.update(|workspace| {
        if workspace.layout.contains(&pane) {
            workspace.focused = Some(pane);
        }
    })
}

#[tauri::command]
fn workspace_cycle_focus(delta: isize, state: State<'_, AppState>) -> Result<Workspace, String> {
    state.update(|workspace| workspace.cycle_focus(delta))
}

/// Move a divider. `path` walks the tree; each step says whether to descend
/// into the first child.
#[tauri::command]
fn workspace_set_ratio(
    path: Vec<bool>,
    ratio: f32,
    state: State<'_, AppState>,
) -> Result<Workspace, String> {
    state.update(|workspace| {
        workspace.layout.set_ratio_at(&path, ratio);
    })
}

/// Open a pane and stream its output to `on_event`.
#[tauri::command]
fn pane_spawn(
    mut spec: PaneSpec,
    on_event: Channel<PtyEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if spec.integration {
        let program = spec.shell.clone().unwrap_or_else(hp_pty::resolve_shell);
        let (program, args) = hp_agents::spawn_argv(&program, &spec.args);
        // Fail soft: a shell integration problem must never stop a person from
        // opening a terminal.
        match hp_shell::integrate(&program, &args, &state.integration_dir) {
            Ok(prepared) => {
                spec.shell = Some(program);
                spec.args = prepared;
            }
            Err(error) => eprintln!("hyperpane: shell integration unavailable: {error}"),
        }
    } else if !spec.args.is_empty() {
        // An agent started with a task needs the same treatment: an npm CLI is
        // a batch shim, which the Windows process API will not start directly.
        let program = spec.shell.clone().unwrap_or_else(hp_pty::resolve_shell);
        let (program, args) = hp_agents::spawn_argv(&program, &spec.args);
        spec.shell = Some(program);
        spec.args = args;
    }

    let pane_id = spec.id.clone();
    if spec.agent.is_some() {
        state.runs.lock().insert(
            pane_id.clone(),
            RunState {
                agent: spec.agent.clone(),
                ..RunState::default()
            },
        );
    }

    let runs = Arc::clone(&state.runs);
    let directories = Arc::clone(&state.directories);
    let sink: EventSink = Arc::new(move |event| {
        // A child's report is read from the stream it already writes to; no
        // extra channel is needed, and the terminal still receives every byte.
        match &event {
            PtyEvent::Data { chunk } => {
                if let Some(run) = runs.lock().get_mut(&pane_id) {
                    if let Some(result) = run.scanner.push(chunk) {
                        run.result = Some(result);
                    }
                }
            }
            PtyEvent::Cwd { path } => {
                directories.lock().insert(pane_id.clone(), path.clone());
            }
            _ => {}
        }
        // A closed window means the webview is gone; dropping the event is fine.
        let _ = on_event.send(event);
    });
    state.panes.spawn(spec, sink).map_err(|e| e.to_string())
}

#[tauri::command]
fn pane_write(id: String, data: String, state: State<'_, AppState>) -> Result<(), String> {
    state.panes.write(&id, &data).map_err(|e| e.to_string())
}

#[tauri::command]
fn pane_resize(id: String, cols: u16, rows: u16, state: State<'_, AppState>) -> Result<(), String> {
    state
        .panes
        .resize(&id, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn pane_close(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.runs.lock().remove(&id);
    state.directories.lock().remove(&id);
    state.panes.kill(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn pane_list(state: State<'_, AppState>) -> Vec<String> {
    state.panes.list()
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "HyperPane",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        default_shell: hp_pty::resolve_shell(),
        home_dir: home_directory(),
    }
}

/// The user's home directory, for displaying paths as `~/…`.
fn home_directory() -> Option<String> {
    #[cfg(windows)]
    let keys = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let keys = ["HOME", "USERPROFILE"];

    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

/// The agent catalogue, for the UI and for `pane.spawn` validation.
#[tauri::command]
fn agent_catalogue() -> Vec<hp_agents::AgentSpec> {
    hp_agents::catalogue().to_vec()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentAvailability {
    id: &'static str,
    label: &'static str,
    program: &'static str,
    installed: bool,
}

/// Which catalogue agents are actually on this machine.
#[tauri::command]
fn agents_available() -> Vec<AgentAvailability> {
    hp_agents::catalogue()
        .iter()
        .map(|agent| AgentAvailability {
            id: agent.id,
            label: agent.label,
            program: agent.program,
            installed: agent.is_installed(),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentLaunch {
    program: String,
    args: Vec<String>,
    /// The task has to be typed in once the agent is up.
    needs_paste: bool,
    label: &'static str,
}

/// How to start one agent, already wrapped for the platform.
#[tauri::command]
fn agent_launch(agent: String, task: String) -> Result<AgentLaunch, String> {
    let spec = hp_agents::by_id(&agent).ok_or_else(|| {
        format!(
            "unknown agent {agent}; known ids are {}",
            hp_agents::known_ids().join(", ")
        )
    })?;

    let (program, args) = spec.launch(&task);
    let (program, args) = hp_agents::spawn_argv(program, &args);

    Ok(AgentLaunch {
        program,
        args,
        needs_paste: spec.needs_paste(),
        label: spec.label,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeInfo {
    path: String,
    branch: String,
}

/// Create a worktree for an agent run and report where it is.
#[tauri::command]
fn agent_worktree(
    repo: String,
    seed: String,
    state: State<'_, AppState>,
) -> Result<WorktreeInfo, String> {
    let branch = hp_agents::worktree::branch_name(&seed);
    let worktree = hp_agents::worktree::create(Path::new(&repo), &state.worktree_dir, &branch)
        .map_err(|error| error.to_string())?;

    Ok(WorktreeInfo {
        path: worktree.path.display().to_string(),
        branch: worktree.branch,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaneRunState {
    agent: Option<String>,
    /// The child's own last `RESULT` line. A claim, not a fact.
    result: Option<RunResult>,
}

/// What an agent pane has reported so far.
#[tauri::command]
fn pane_run_state(id: String, state: State<'_, AppState>) -> Option<PaneRunState> {
    state.runs.lock().get(&id).map(|run| PaneRunState {
        agent: run.agent.clone(),
        result: run.result.clone(),
    })
}

/// Where a pane's shell last said it was, if it reports that at all.
#[tauri::command]
fn pane_cwd(id: String, state: State<'_, AppState>) -> Option<String> {
    state.directories.lock().get(&id).cloned()
}

/// Read the stored session, including the layout to restore.
#[tauri::command]
fn session_load(state: State<'_, AppState>) -> Result<Session, String> {
    state.store.load().map_err(|error| error.to_string())
}

/// Persist the session. The caller decides how often to call this.
#[tauri::command]
fn session_save(session: Session, state: State<'_, AppState>) -> Result<(), String> {
    state
        .store
        .save(&session)
        .map_err(|error| error.to_string())
}

/// Where the session file lives, so the UI can show it.
#[tauri::command]
fn session_path(state: State<'_, AppState>) -> String {
    state.store.path().display().to_string()
}

/// Build and run the desktop app.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let store = SessionStore::new(directory.join("session.json"));

            // A session that cannot be read is not fatal: the app starts with
            // nothing open. The file is left where it is rather than
            // overwritten, so a person can look at it before losing it.
            let session = store.load().unwrap_or_else(|error| {
                eprintln!("hyperpane: could not read the session file: {error}");
                Session::empty()
            });

            app.manage(AppState::new(
                store,
                directory.join("integration"),
                directory.join("worktrees"),
                session.workspace,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workspace_get,
            workspace_init,
            workspace_split,
            workspace_close_pane,
            workspace_set_focus,
            workspace_cycle_focus,
            workspace_set_ratio,
            pane_spawn,
            pane_write,
            pane_resize,
            pane_close,
            pane_list,
            app_info,
            agent_catalogue,
            agents_available,
            agent_launch,
            agent_worktree,
            pane_run_state,
            pane_cwd,
            session_load,
            session_save,
            session_path,
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(state) = window.try_state::<AppState>() {
                    state.panes.shutdown();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start HyperPane");
}

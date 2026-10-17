//! HyperPane desktop shell: Tauri commands over the PTY and layout layers.
//!
//! The frontend owns pane identity (it generates ids), the backend owns pane
//! *state*. Commands return the whole workspace after a layout change so the UI
//! never has to guess what the tree looks like.

use std::sync::Arc;

use hp_core::{Session, SplitDirection, Workspace};
use hp_pty::{EventSink, PaneManager, PaneSpec, PtyEvent};
use hp_store::SessionStore;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};

/// Shared application state. The PTY manager is behind an `Arc` because
/// channel callbacks outlive the command that created them.
struct AppState {
    panes: Arc<PaneManager>,
    store: SessionStore,
    /// `None` until the frontend opens its first pane.
    workspace: Mutex<Option<Workspace>>,
}

impl AppState {
    fn new(store: SessionStore, workspace: Option<Workspace>) -> Self {
        Self {
            panes: Arc::new(PaneManager::new()),
            store,
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
    spec: PaneSpec,
    on_event: Channel<PtyEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sink: EventSink = Arc::new(move |event| {
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

            app.manage(AppState::new(store, session.workspace));
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

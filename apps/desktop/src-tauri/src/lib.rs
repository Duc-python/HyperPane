//! HyperPane desktop shell: Tauri commands over the PTY and layout layers.
//!
//! The frontend owns pane identity (it generates ids), the backend owns pane
//! *state*. Commands return the whole workspace after a layout change so the UI
//! never has to guess what the tree looks like.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hp_agents::guard::{Limits, SpawnGuard};
use hp_agents::result::{ResultScanner, RunResult};
use hp_core::{Session, SplitDirection, Workspace};
use hp_pty::{EventSink, PaneManager, PaneSpec, PtyEvent};
use hp_store::audit::{AuditLog, Entry as AuditEntry};
use hp_store::SessionStore;
use hp_teams::{Advance, Hold, PlannedMember};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{Manager, State};

/// How long a pane may be silent before it stops counting as working. It is a
/// display threshold, not a judgement: an agent thinking hard and an agent
/// waiting for an answer look identical from here.
const QUIET_AFTER: Duration = Duration::from_secs(5);

/// What an agent pane looks like from outside.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum RunPhase {
    /// No output yet.
    Starting,
    /// Output in the last few seconds.
    Working,
    /// Silent for a while. Deliberately not called "idle" and never "blocked":
    /// the app cannot tell those apart, and will not invent a distinction.
    Quiet,
    /// The child printed its own `RESULT` line.
    Reported,
    Exited,
    Failed,
}

/// What the app knows about an agent pane beyond its process.
#[derive(Default)]
struct RunState {
    agent: Option<String>,
    result: Option<RunResult>,
    /// Watches the pane's output for the child's own `RESULT` line.
    scanner: ResultScanner,
    started_at: Option<Instant>,
    last_output_at: Option<Instant>,
    exit: Option<i32>,
}

impl RunState {
    fn phase(&self) -> RunPhase {
        // A finished process is the clearest fact available, and a report the
        // child wrote before exiting is a claim about work, not a state.
        if let Some(code) = self.exit {
            return if code == 0 {
                RunPhase::Exited
            } else {
                RunPhase::Failed
            };
        }
        if self.result.is_some() {
            return RunPhase::Reported;
        }
        match self.last_output_at {
            None => RunPhase::Starting,
            Some(last) if last.elapsed() < QUIET_AFTER => RunPhase::Working,
            Some(_) => RunPhase::Quiet,
        }
    }
}

/// What a member reported about its own work.
///
/// Recorded while the pane is alive and kept after it closes, because a person
/// reading a hold wants to know what the checkers said, and the checkers' panes
/// are usually gone by then.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemberClaim {
    wave: u32,
    pane: String,
    role: String,
    label: String,
    agent: String,
    /// The child's own last `RESULT` line, if it printed one. A claim about its
    /// own work, not a fact the app checked.
    result: Option<RunResult>,
    /// The process had already ended when this was recorded.
    finished: bool,
}

/// A team run the app is keeping track of.
///
/// One at a time, and only while it is going: a run is a sequence of waves and
/// it makes no sense to have two of them competing for the same panes.
struct TeamRun {
    key: String,
    title: String,
    /// What the person asked the run to do, kept for the next wave's brief.
    task: String,
    /// The repository a member's worktree is forked from.
    repo: Option<String>,
    plan: Vec<PlannedMember>,
    progress: hp_teams::RunState,
    /// Panes opened for the wave in progress.
    panes: Vec<String>,
    /// What the members of each wave have reported so far.
    claims: Vec<MemberClaim>,
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
    /// Refuses a run that would exceed the app's own limits.
    guard: Mutex<SpawnGuard>,
    /// Append-only record of what the app did on a person's behalf.
    audit: AuditLog,
    /// The team run in progress, if there is one.
    team: Mutex<Option<TeamRun>>,
    /// `None` until the frontend opens its first pane.
    workspace: Mutex<Option<Workspace>>,
}

impl AppState {
    fn new(
        store: SessionStore,
        integration_dir: PathBuf,
        worktree_dir: PathBuf,
        audit: AuditLog,
        workspace: Option<Workspace>,
    ) -> Self {
        Self {
            panes: Arc::new(PaneManager::new()),
            store,
            integration_dir,
            worktree_dir,
            runs: Arc::new(Mutex::new(HashMap::new())),
            directories: Arc::new(Mutex::new(HashMap::new())),
            guard: Mutex::new(SpawnGuard::new(Limits::default())),
            audit,
            team: Mutex::new(None),
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
        {
            let mut runs = runs.lock();
            match &event {
                PtyEvent::Data { chunk } => {
                    if let Some(run) = runs.get_mut(&pane_id) {
                        run.last_output_at = Some(Instant::now());
                        if let Some(result) = run.scanner.push(chunk) {
                            run.result = Some(result);
                        }
                    }
                }
                PtyEvent::Exit { code } => {
                    if let Some(run) = runs.get_mut(&pane_id) {
                        run.exit = *code;
                    }
                }
                _ => {}
            }
        }
        if let PtyEvent::Cwd { path } = &event {
            directories.lock().insert(pane_id.clone(), path.clone());
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
    /// What the pane looks like from outside.
    phase: RunPhase,
    /// Seconds since this run started.
    elapsed_seconds: u64,
}

/// What an agent pane has reported so far.
#[tauri::command]
fn pane_run_state(id: String, state: State<'_, AppState>) -> Option<PaneRunState> {
    state.runs.lock().get(&id).map(|run| PaneRunState {
        agent: run.agent.clone(),
        result: run.result.clone(),
        phase: run.phase(),
        elapsed_seconds: run
            .started_at
            .map(|started| started.elapsed().as_secs())
            .unwrap_or_default(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentOpenRequest {
    /// The pane that will host the run.
    pane: String,
    agent: String,
    #[serde(default)]
    task: String,
    /// Confine the run to its own worktree.
    #[serde(default)]
    worktree: bool,
    /// The repository to fork from, when a worktree was asked for.
    #[serde(default)]
    repo: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentOpen {
    label: String,
    program: String,
    args: Vec<String>,
    needs_paste: bool,
    cwd: Option<String>,
    branch: Option<String>,
}

/// Open an agent run: check the limits, prepare the worktree, record it.
///
/// This is one call rather than three the UI could interleave or skip. The
/// limits live here, behind the command, so no amount of UI change can get
/// around them.
/// Reserve `count` runs against the app's limits, all or nothing.
fn reserve_runs(state: &AppState, count: usize) -> Result<(), String> {
    let live = state.runs.lock().len();
    state
        .guard
        .lock()
        .reserve(live, count, Instant::now())
        .map_err(|refusal| refusal.message())
}

/// Create a worktree for one run and record it.
fn prepare_worktree(
    state: &AppState,
    pane: &str,
    agent_id: &str,
    repo: &str,
    seed: &str,
) -> Result<hp_agents::worktree::Worktree, String> {
    let branch = hp_agents::worktree::branch_name(seed);
    let created = hp_agents::worktree::create(Path::new(repo), &state.worktree_dir, &branch)
        .map_err(|error| error.to_string())?;

    state
        .audit
        .append(
            &AuditEntry::new(
                "worktree.create",
                format!("{} at {}", created.branch, created.path.display()),
            )
            .for_agent(agent_id.to_string())
            .for_pane(pane.to_string()),
        )
        .map_err(|error| error.to_string())?;

    Ok(created)
}

/// Everything after the slot is held and the worktree exists.
///
/// Split out so a team can prepare a member's worktree first and then name it
/// in the member's brief: a brief that tells an agent where it may write has to
/// know the answer before it is written.
fn open_reserved(
    state: &AppState,
    pane: &str,
    agent_id: &str,
    task: &str,
    worktree: Option<&hp_agents::worktree::Worktree>,
) -> Result<AgentOpen, String> {
    let spec = hp_agents::by_id(agent_id).ok_or_else(|| {
        format!(
            "unknown agent {agent_id}; known ids are {}",
            hp_agents::known_ids().join(", ")
        )
    })?;

    let (program, args) = spec.launch(task);
    let (program, args) = hp_agents::spawn_argv(program, &args);

    state
        .audit
        .append(
            &AuditEntry::new(
                "agent.open",
                if task.trim().is_empty() {
                    format!("opened {}", spec.label)
                } else {
                    format!("opened {} with a task", spec.label)
                },
            )
            .for_agent(spec.id.to_string())
            .for_pane(pane.to_string()),
        )
        .map_err(|error| error.to_string())?;

    state.runs.lock().insert(
        pane.to_string(),
        RunState {
            agent: Some(spec.id.to_string()),
            started_at: Some(Instant::now()),
            ..RunState::default()
        },
    );

    Ok(AgentOpen {
        label: spec.label.to_string(),
        program,
        args,
        needs_paste: spec.needs_paste(),
        cwd: worktree.map(|worktree| worktree.path.display().to_string()),
        branch: worktree.map(|worktree| worktree.branch.clone()),
    })
}

/// Open one run: reserve its slot, prepare its worktree, then start it.
fn open_run(
    state: &AppState,
    pane: &str,
    agent_id: &str,
    task: &str,
    repo: Option<&str>,
) -> Result<AgentOpen, String> {
    // The guard runs before anything exists, so a refusal leaves nothing
    // behind to clean up.
    if let Err(message) = reserve_runs(state, 1) {
        let _ = state.audit.append(
            &AuditEntry::new("agent.refused", message.clone())
                .for_agent(agent_id.to_string())
                .for_pane(pane.to_string()),
        );
        return Err(message);
    }

    let worktree = match repo {
        Some(repo) => Some(prepare_worktree(
            state,
            pane,
            agent_id,
            repo,
            &format!("{pane}:{task}"),
        )?),
        None => None,
    };

    open_reserved(state, pane, agent_id, task, worktree.as_ref())
}

#[tauri::command]
fn agent_open(request: AgentOpenRequest, state: State<'_, AppState>) -> Result<AgentOpen, String> {
    let repo = if request.worktree {
        request.repo.as_deref()
    } else {
        None
    };

    if request.worktree && repo.is_none() {
        return Err(
            "a worktree needs a repository, and that pane has not reported a \
             directory yet: run a command in it first"
                .to_string(),
        );
    }

    open_run(&state, &request.pane, &request.agent, &request.task, repo)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamStartRequest {
    key: String,
    #[serde(default)]
    task: String,
    /// Pane ids the frontend has allocated, one per member of the wave.
    panes: Vec<String>,
    /// The repository a member's worktree is forked from, when one is wanted.
    #[serde(default)]
    repo: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartedMember {
    pane: String,
    role: String,
    label: String,
    agent: String,
    read_only: bool,
    branch: Option<String>,
    program: String,
    args: Vec<String>,
    needs_paste: bool,
    cwd: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamStart {
    key: String,
    title: String,
    wave: u32,
    members: Vec<StartedMember>,
    /// Waves after this one, so the UI can say what is still to come.
    waves_remaining: usize,
}

/// Open every member of one wave.
///
/// The whole wave is reserved before any of it starts, so a refusal costs
/// nothing rather than leaving half a wave running.
fn open_wave(
    state: &AppState,
    preset: &hp_teams::Preset,
    plan: &[PlannedMember],
    wave: u32,
    task: &str,
    repo: Option<&str>,
    panes: &[String],
) -> Result<Vec<StartedMember>, String> {
    let members: Vec<&PlannedMember> = plan
        .iter()
        .filter(|member| member.wave == wave && member.kind != hp_teams::RoleKind::Leader)
        .collect();

    if panes.len() != members.len() {
        return Err(format!(
            "wave {wave} opens {} panes and {} were allocated",
            members.len(),
            panes.len()
        ));
    }

    if let Err(message) = reserve_runs(state, members.len()) {
        let _ = state.audit.append(
            &AuditEntry::new("team.refused", message.clone()).for_agent(preset.key.clone()),
        );
        return Err(message);
    }

    let mut started = Vec::new();
    for (member, pane) in members.iter().zip(panes.iter()) {
        let worktree = if member.worktree {
            let repo = repo.ok_or_else(|| {
                format!(
                    "role {} wants its own worktree and no repository was given",
                    member.label
                )
            })?;
            Some(prepare_worktree(
                state,
                pane,
                &member.agent,
                repo,
                &format!("{pane}:{}", member.role),
            )?)
        } else {
            None
        };

        let worktree_path = worktree
            .as_ref()
            .map(|worktree| worktree.path.display().to_string());

        let brief = hp_teams::write_brief(&hp_teams::BriefInput {
            team: &preset.title_en(),
            purpose: &preset.purpose_en(),
            role_label: &member.label,
            focus: &member.focus,
            task,
            read_only: member.read_only,
            worktree: worktree_path.as_deref(),
            wave: member.wave,
        });

        let opened = open_reserved(state, pane, &member.agent, &brief, worktree.as_ref())?;

        started.push(StartedMember {
            pane: pane.clone(),
            role: member.role.clone(),
            label: member.label.clone(),
            agent: member.agent.clone(),
            read_only: member.read_only,
            branch: opened.branch,
            program: opened.program,
            args: opened.args,
            needs_paste: opened.needs_paste,
            cwd: opened.cwd,
        });
    }

    Ok(started)
}

/// What each pane of a wave has reported, and whether it is still going.
type WaveReport = HashMap<String, (Option<RunResult>, bool)>;

/// Read a wave's panes once, so the two locks are never held at the same time.
fn wave_report(runs: &HashMap<String, RunState>, panes: &[String]) -> WaveReport {
    panes
        .iter()
        .map(|pane| {
            let state = match runs.get(pane) {
                Some(run) => (run.result.clone(), run.exit.is_some()),
                // A pane that is gone cannot still be working.
                None => (None, true),
            };
            (pane.clone(), state)
        })
        .collect()
}

/// Record what a wave's members have reported so far.
///
/// Rebuilt from the panes each time rather than appended to, so a member that
/// reports twice ends up with its latest claim instead of two, and a pane that
/// closed keeps the claim it made before it went.
fn snapshot_claims(run: &mut TeamRun, report: &WaveReport) {
    let wave = run.progress.wave;
    let members: Vec<&PlannedMember> = run
        .plan
        .iter()
        .filter(|member| member.wave == wave)
        .collect();

    run.claims.retain(|claim| claim.wave != wave);

    for (member, pane) in members.iter().zip(run.panes.iter()) {
        let (result, finished) = report.get(pane).cloned().unwrap_or((None, true));

        run.claims.push(MemberClaim {
            wave,
            pane: pane.clone(),
            role: member.role.clone(),
            label: member.label.clone(),
            agent: member.agent.clone(),
            result,
            finished,
        });
    }
}

/// How many of a wave's panes are still alive.
///
/// A pane that is gone counts as not running: a person closing a pane has to
/// move the run on, or the wave would wait forever for something that no longer
/// exists.
fn running_panes(runs: &HashMap<String, RunState>, panes: &[String]) -> usize {
    panes
        .iter()
        .filter(|pane| {
            runs.get(*pane)
                .map(|run| run.exit.is_none())
                .unwrap_or(false)
        })
        .count()
}

/// Start the first wave of a team that has anything to spawn.
///
/// The leader is the pane the person is already in, so a run begins at the
/// first wave holding a member that is not one. Every member goes through the
/// same reservation and the same worktree preparation as a palette run, which
/// is what stops a team from being a way around the app's own limits.
#[tauri::command]
fn team_start(request: TeamStartRequest, state: State<'_, AppState>) -> Result<TeamStart, String> {
    let file = hp_teams::PresetFile::parse(hp_teams::SHIPPED).map_err(|e| e.to_string())?;
    let preset = file
        .team(&request.key)
        .ok_or_else(|| format!("no team called {}", request.key))?;

    let planned = hp_teams::plan(preset, &installed_agents()).map_err(|e| e.to_string())?;

    let Some(progress) = hp_teams::RunState::start(&planned) else {
        return Err(format!(
            "{} has nothing to spawn: every role is a leader",
            preset.title_en()
        ));
    };

    let wave = progress.wave;
    let members = open_wave(
        &state,
        preset,
        &planned,
        wave,
        &request.task,
        request.repo.as_deref(),
        &request.panes,
    )?;

    let waves_remaining = progress.pending.len();
    let _ = state.audit.append(
        &AuditEntry::new(
            "team.start",
            format!(
                "wave {wave}: {} member(s), {waves_remaining} wave(s) left",
                members.len()
            ),
        )
        .for_agent(preset.key.clone()),
    );

    *state.team.lock() = Some(TeamRun {
        key: preset.key.clone(),
        title: preset.title_en(),
        task: request.task,
        repo: request.repo,
        plan: planned,
        progress,
        panes: request.panes.clone(),
        claims: Vec::new(),
    });

    Ok(TeamStart {
        key: preset.key.clone(),
        title: preset.title_en(),
        wave,
        members,
        waves_remaining,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamStatus {
    key: String,
    title: String,
    wave: u32,
    /// Panes of the current wave that are still alive.
    running: usize,
    /// What the run is waiting for, when it is waiting on a person.
    holding: Option<Hold>,
    holding_message: Option<String>,
    /// The wave the run would open next, once the current one is done.
    next_wave: Option<u32>,
    /// How many panes that wave needs.
    next_wave_panes: usize,
    waves_remaining: usize,
    /// What each member has reported, wave by wave. A claim, not a fact.
    claims: Vec<MemberClaim>,
}

/// Where the team run is, without moving it.
#[tauri::command]
fn team_status(state: State<'_, AppState>) -> Option<TeamStatus> {
    // Which panes matter is read first, so the two locks are never held at once.
    let panes = {
        let team = state.team.lock();
        team.as_ref()?.panes.clone()
    };

    let report = wave_report(&state.runs.lock(), &panes);

    let mut team = state.team.lock();
    let run = team.as_mut()?;
    snapshot_claims(run, &report);

    let running = running_panes(&state.runs.lock(), &run.panes);
    let next_wave = run.progress.pending.first().copied();

    Some(TeamStatus {
        key: run.key.clone(),
        title: run.title.clone(),
        wave: run.progress.wave,
        running,
        holding: run.progress.holding(),
        holding_message: run
            .progress
            .holding()
            .map(|hold| hold.message().to_string()),
        next_wave,
        next_wave_panes: run
            .plan
            .iter()
            .filter(|member| {
                Some(member.wave) == next_wave && member.kind != hp_teams::RoleKind::Leader
            })
            .count(),
        waves_remaining: run.progress.pending.len(),
        claims: run.claims.clone(),
    })
}

/// What moving the run on produced.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum TeamAdvance {
    Waiting {
        running: usize,
    },
    Ready {
        wave: u32,
        members: Vec<StartedMember>,
        waves_remaining: usize,
    },
    Held {
        reason: String,
    },
    Finished,
}

/// Move the run on if it can be moved on.
///
/// The frontend calls this when it sees the wave's panes finish. What decides
/// whether that is allowed lives here, so a UI that calls it early gets
/// `Waiting` rather than a run that skips a dependency.
#[tauri::command]
fn team_advance(panes: Vec<String>, state: State<'_, AppState>) -> Result<TeamAdvance, String> {
    let file = hp_teams::PresetFile::parse(hp_teams::SHIPPED).map_err(|e| e.to_string())?;

    // Taken out so the decision can hold it mutably, and put back below
    // whatever the answer is.
    let mut run = state
        .team
        .lock()
        .take()
        .ok_or_else(|| "no team run is going".to_string())?;

    let preset = file
        .team(&run.key)
        .ok_or_else(|| format!("no team called {}", run.key))?
        .clone();

    let running = running_panes(&state.runs.lock(), &run.panes);
    // Recorded here as well as on every status read, so the wave being left
    // behind keeps its claims even if nobody polled before it finished.
    let report = wave_report(&state.runs.lock(), &run.panes);
    snapshot_claims(&mut run, &report);
    let before = run.progress.clone();

    let outcome = match run.progress.advance(&preset, &run.plan, running) {
        Advance::Waiting { running } => TeamAdvance::Waiting { running },
        Advance::Held(hold) => TeamAdvance::Held {
            reason: hold.message().to_string(),
        },
        Advance::Finished => TeamAdvance::Finished,
        Advance::Ready { wave } => {
            match open_wave(
                &state,
                &preset,
                &run.plan,
                wave,
                &run.task,
                run.repo.as_deref(),
                &panes,
            ) {
                Ok(members) => {
                    run.panes = panes;
                    let _ = state.audit.append(
                        &AuditEntry::new(
                            "team.wave",
                            format!("wave {wave}: {} member(s)", members.len()),
                        )
                        .for_agent(run.key.clone()),
                    );
                    TeamAdvance::Ready {
                        wave,
                        members,
                        waves_remaining: run.progress.pending.len(),
                    }
                }
                Err(message) => {
                    // Nothing opened, so the run goes back where it was rather
                    // than claiming a wave it never started.
                    run.progress = before;
                    *state.team.lock() = Some(run);
                    return Err(message);
                }
            }
        }
    };

    match outcome {
        TeamAdvance::Finished => {
            let _ = state.audit.append(
                &AuditEntry::new("team.finished", "the last wave finished")
                    .for_agent(run.key.clone()),
            );
        }
        _ => *state.team.lock() = Some(run),
    }

    Ok(outcome)
}

/// A person cleared the hold the run was sitting on.
#[tauri::command]
fn team_confirm(state: State<'_, AppState>) -> Option<Hold> {
    let mut team = state.team.lock();
    let run = team.as_mut()?;
    let was = run.progress.holding();
    run.progress.confirm();
    was
}

/// Where the activity log lives, so the UI can show it.
#[tauri::command]
fn audit_path(state: State<'_, AppState>) -> String {
    state.audit.path().display().to_string()
}

/// Agent ids that are actually installed on this machine.
fn installed_agents() -> std::collections::BTreeSet<String> {
    hp_agents::catalogue()
        .iter()
        .filter(|agent| agent.is_installed())
        .map(|agent| agent.id.to_string())
        .collect()
}

/// The shipped team presets, as a picker needs them.
#[tauri::command]
fn teams_list() -> Result<Vec<hp_teams::Preset>, String> {
    let file = hp_teams::PresetFile::parse(hp_teams::SHIPPED).map_err(|e| e.to_string())?;
    let mut teams = file.teams.clone();
    teams.sort_by(|left, right| {
        left.group
            .as_deref()
            .unwrap_or("")
            .cmp(right.group.as_deref().unwrap_or(""))
            .then_with(|| left.key.cmp(&right.key))
    });
    Ok(teams)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanWave {
    wave: u32,
    members: Vec<hp_teams::PlannedMember>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamPlan {
    key: String,
    title: String,
    purpose: String,
    gates: Vec<hp_teams::Gate>,
    stop: hp_teams::Stop,
    /// The waves a run would open, when it can be planned at all.
    waves: Vec<PlanWave>,
    /// Why it cannot be planned, when it cannot.
    refusal: Option<String>,
    /// What is wrong with the preset itself, which does not stop a run but is
    /// worth saying out loud.
    problems: Vec<String>,
}

/// Work out what a team would do, without doing any of it.
///
/// Nothing here opens a pane or starts a process. Seeing the plan before it
/// costs anything is the point.
#[tauri::command]
fn team_plan(key: String) -> Result<TeamPlan, String> {
    let file = hp_teams::PresetFile::parse(hp_teams::SHIPPED).map_err(|e| e.to_string())?;
    let preset = file
        .team(&key)
        .ok_or_else(|| format!("no team called {key}"))?;

    let installed = installed_agents();
    let (waves, refusal) = match hp_teams::plan(preset, &installed) {
        Ok(members) => {
            let mut ordered: Vec<u32> = members.iter().map(|member| member.wave).collect();
            ordered.sort_unstable();
            ordered.dedup();
            let waves = ordered
                .into_iter()
                .map(|wave| PlanWave {
                    wave,
                    members: members
                        .iter()
                        .filter(|member| member.wave == wave)
                        .cloned()
                        .collect(),
                })
                .collect();
            (waves, None)
        }
        Err(error) => (Vec::new(), Some(error.to_string())),
    };

    Ok(TeamPlan {
        key: preset.key.clone(),
        title: preset.title_en(),
        purpose: preset.purpose_en(),
        gates: preset.gates.clone(),
        stop: preset.stop.clone(),
        waves,
        refusal,
        problems: file.problems(),
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
                AuditLog::new(directory.join("audit.log")),
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
            agent_open,
            audit_path,
            teams_list,
            team_plan,
            team_start,
            team_status,
            team_advance,
            team_confirm,
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

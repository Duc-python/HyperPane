import { Channel, invoke } from "@tauri-apps/api/core";

export type SplitDirection = "row" | "column";

/** Mirrors `hp_core::LayoutNode`. */
export type LayoutNode =
  | { type: "leaf"; pane: string }
  | {
      type: "split";
      direction: SplitDirection;
      ratio: number;
      first: LayoutNode;
      second: LayoutNode;
    };

/** Mirrors `hp_core::Workspace`. */
export interface Workspace {
  id: string;
  name: string;
  layout: LayoutNode;
  focused: string | null;
  active: boolean;
}

/** Mirrors `hp_pty::PtyEvent`. */
export type PtyEvent =
  | { kind: "data"; chunk: string }
  | { kind: "exit"; code: number | null }
  | { kind: "error"; message: string }
  /** The shell reported its working directory (OSC 7). */
  | { kind: "cwd"; path: string }
  /** The shell reported a prompt boundary (OSC 133). */
  | { kind: "prompt"; mark: PromptKind; exitCode: number | null };

export type PromptKind =
  | "promptStart"
  | "promptEnd"
  | "commandStart"
  | "commandEnd";

/** Mirrors `hp_pty::PaneSpec`. */
export interface PaneSpec {
  id: string;
  cwd?: string | null;
  shell?: string | null;
  args?: string[];
  env?: Record<string, string>;
  cols?: number;
  rows?: number;
  /** Load HyperPane's prompt hooks into the shell. */
  integration?: boolean;
  /** Catalogue id when this pane runs an agent. */
  agent?: string | null;
}

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  defaultShell: string;
  homeDir: string | null;
}

/** Mirrors `hp_agents::AgentSpec`. */
export interface AgentSpec {
  id: string;
  label: string;
  program: string;
  delivery: "startArgument" | "pastedAfterStart";
  statusDetection: boolean;
  tokenMeter: boolean;
  install: "command" | "docs" | "varies";
}

export const workspaceGet = () => invoke<Workspace | null>("workspace_get");

export const workspaceInit = (pane: string, name?: string) =>
  invoke<Workspace>("workspace_init", { pane, name: name ?? null });

export const workspaceSplit = (
  target: string,
  direction: SplitDirection,
  newPane: string,
) => invoke<Workspace>("workspace_split", { target, direction, newPane });

export const workspaceClosePane = (pane: string) =>
  invoke<Workspace>("workspace_close_pane", { pane });

export const workspaceSetFocus = (pane: string) =>
  invoke<Workspace>("workspace_set_focus", { pane });

export const workspaceCycleFocus = (delta: number) =>
  invoke<Workspace>("workspace_cycle_focus", { delta });

export const workspaceSetRatio = (path: boolean[], ratio: number) =>
  invoke<Workspace>("workspace_set_ratio", { path, ratio });

export const paneWrite = (id: string, data: string) =>
  invoke<void>("pane_write", { id, data });

export const paneResize = (id: string, cols: number, rows: number) =>
  invoke<void>("pane_resize", { id, cols, rows });

export const paneClose = (id: string) => invoke<void>("pane_close", { id });

export const paneList = () => invoke<string[]>("pane_list");

export const appInfo = () => invoke<AppInfo>("app_info");

export const agentCatalogue = () => invoke<AgentSpec[]>("agent_catalogue");

/**
 * Opens a PTY pane and streams its output.
 *
 * The channel is returned rather than kept private so the caller can close over
 * it; dropping the terminal pane must not leave a reader pumping events into a
 * disposed term.
 */
export async function paneSpawn(
  spec: PaneSpec,
  onEvent: (event: PtyEvent) => void,
): Promise<Channel<PtyEvent>> {
  const channel = new Channel<PtyEvent>();
  channel.onmessage = onEvent;
  await invoke<void>("pane_spawn", { spec, onEvent: channel });
  return channel;
}

/** Pane ids of a layout, in visual order. */
export function paneIds(node: LayoutNode): string[] {
  if (node.type === "leaf") return [node.pane];
  return [...paneIds(node.first), ...paneIds(node.second)];
}

export function containsPane(node: LayoutNode, pane: string): boolean {
  if (node.type === "leaf") return node.pane === pane;
  return containsPane(node.first, pane) || containsPane(node.second, pane);
}

/** Mirrors `hp_core::Profile`. */
export interface Profile {
  id: string;
  name: string;
  shell: string | null;
  args: string[];
  cwd: string | null;
  env: Record<string, string>;
  /** Whether the shell loads HyperPane's prompt hooks. */
  integration: boolean;
}

/** Mirrors `hp_core::Session`. */
export interface Session {
  version: number;
  workspace: Workspace | null;
  profiles: Profile[];
  defaultProfile: string | null;
}

/** Mirrors `hp_agents::result::RunResult`. */
export interface RunResult {
  status: "done" | "failed" | "blocked";
  files: string[];
  tests: "pass" | "fail" | "none";
}

/** Mirrors the app's `RunPhase`. */
export type RunPhase =
  | "starting"
  | "working"
  | "quiet"
  | "reported"
  | "exited"
  | "failed";

/** Mirrors the app's `PaneRunState`. */
export interface PaneRunState {
  agent: string | null;
  /** The child's own last RESULT line. A claim, not a fact. */
  result: RunResult | null;
  phase: RunPhase;
  elapsedSeconds: number;
}

/** Which catalogue agents are installed on this machine. */
export interface AgentInfo {
  id: string;
  label: string;
  program: string;
  installed: boolean;
}

export interface AgentLaunch {
  program: string;
  args: string[];
  needsPaste: boolean;
  label: string;
}

export interface WorktreeInfo {
  path: string;
  branch: string;
}

/** What the app needs to open one agent run. */
export interface AgentOpenRequest {
  /** The pane that will host the run. */
  pane: string;
  agent: string;
  task?: string;
  worktree?: boolean;
  /** The repository to fork from, when a worktree was asked for. */
  repo?: string | null;
}

/** Mirrors the app's `AgentOpen`. */
export interface AgentOpen {
  label: string;
  program: string;
  args: string[];
  needsPaste: boolean;
  cwd: string | null;
  branch: string | null;
}

/**
 * Open an agent run.
 *
 * One call, not three the UI could interleave: the app's limits and the
 * activity log live behind this, so no UI change can get around them.
 */
export const agentOpen = (request: AgentOpenRequest) =>
  invoke<AgentOpen>("agent_open", { request });

/** Where the activity log lives. */
export const auditPath = () => invoke<string>("audit_path");

export type Archetype =
  | "reviewer"
  | "pipeline"
  | "researcher"
  | "drafter"
  | "operator"
  | "control";

export type Gate = "humanPlan" | "checkerPass" | "humanCommit";
export type RoleKind = "leader" | "checker" | "worker";
export type Tier = "fast" | "mid" | "strong";
export type OnStall = "escalate" | "stop";

/** Text in one or more languages, keyed by language tag. */
export type Localised = Record<string, string>;

export interface TeamRole {
  key: string;
  label: Localised | null;
  kind: RoleKind;
  /** Agent ids, most preferred first. */
  agents: string[];
  count: number;
  focus: string;
  readOnly: boolean;
  worktree: boolean;
  tier: Tier;
}

export interface TeamStop {
  maxRounds: number;
  maxSpawns: number;
  wallMinutes: number;
  onStall: OnStall;
}

export interface TeamPreset {
  key: string;
  title: Localised;
  purpose: Localised;
  group: string | null;
  archetype: Archetype;
  gates: Gate[];
  stop: TeamStop;
  waves: Array<{ wave: number; roles: TeamRole[] }>;
  starterPrompt: Localised | null;
}

/** One member a run would open. */
export interface PlannedMember {
  role: string;
  label: string;
  wave: number;
  agent: string;
  /** Why the router gave this member that agent, in one line. */
  reason: string;
  kind: RoleKind;
  tier: Tier;
  readOnly: boolean;
  worktree: boolean;
  focus: string;
}

export interface TeamPlan {
  key: string;
  title: string;
  purpose: string;
  gates: Gate[];
  stop: TeamStop;
  waves: Array<{ wave: number; members: PlannedMember[] }>;
  /** Why it cannot be planned, when it cannot. */
  refusal: string | null;
  problems: string[];
}

export const teamsList = () => invoke<TeamPreset[]>("teams_list");

export const teamPlan = (key: string) => invoke<TeamPlan>("team_plan", { key });

export interface TeamStartRequest {
  key: string;
  task?: string;
  /** Pane ids the frontend has allocated, one per member of the wave. */
  panes: string[];
  /** The repository a member's worktree is forked from. */
  repo?: string | null;
}

export interface StartedMember {
  pane: string;
  role: string;
  label: string;
  agent: string;
  readOnly: boolean;
  branch: string | null;
  program: string;
  args: string[];
  needsPaste: boolean;
  cwd: string | null;
}

export interface TeamStart {
  key: string;
  title: string;
  wave: number;
  members: StartedMember[];
  /** Waves after this one. */
  wavesRemaining: number;
}

/**
 * Start the first wave of a team that has anything to spawn.
 *
 * Every member goes through the same reservation and worktree preparation as a
 * palette run, so a team is not a way around the app's own limits.
 */
export const teamStart = (request: TeamStartRequest) =>
  invoke<TeamStart>("team_start", { request });

export type Hold = "checksReported" | "resultReady";

/** What a member reported about its own work. A claim, not a fact. */
export interface MemberClaim {
  wave: number;
  pane: string;
  role: string;
  label: string;
  agent: string;
  result: RunResult | null;
  /** What the member wrote to its findings file, when it wrote one. */
  findings: string | null;
  /** The process had already ended when this was recorded. */
  finished: boolean;
}

export interface TeamStatus {
  key: string;
  title: string;
  wave: number;
  /** Panes of the current wave that are still alive. */
  running: number;
  holding: Hold | null;
  holdingMessage: string | null;
  nextWave: number | null;
  nextWavePanes: number;
  wavesRemaining: number;
  /** What each member has reported, wave by wave. */
  claims: MemberClaim[];
}

export type TeamAdvance =
  | { kind: "waiting"; running: number }
  | {
      kind: "ready";
      wave: number;
      members: StartedMember[];
      wavesRemaining: number;
    }
  | { kind: "held"; reason: string }
  | { kind: "finished" };

/** Where the team run is, without moving it. */
export const teamStatus = () => invoke<TeamStatus | null>("team_status");

/**
 * Move the run on if it can be moved on.
 *
 * The frontend calls this when it sees the wave's panes finish. The decision
 * lives in the app, so calling it early gets `waiting` rather than a run that
 * skipped a dependency.
 */
export const teamAdvance = (panes: string[]) =>
  invoke<TeamAdvance>("team_advance", { panes });

/** Clear the hold the run is sitting on. Returns the hold that was cleared. */
export const teamConfirm = () => invoke<Hold | null>("team_confirm");

export const agentsAvailable = () => invoke<AgentInfo[]>("agents_available");

export const agentLaunch = (agent: string, task: string) =>
  invoke<AgentLaunch>("agent_launch", { agent, task });

export const agentWorktree = (repo: string, seed: string) =>
  invoke<WorktreeInfo>("agent_worktree", { repo, seed });

export const paneRunState = (id: string) =>
  invoke<PaneRunState | null>("pane_run_state", { id });

/** Where a pane's shell last said it was. */
export const paneCwd = (id: string) => invoke<string | null>("pane_cwd", { id });

export const sessionLoad = () => invoke<Session>("session_load");

export const sessionSave = (session: Session) =>
  invoke<void>("session_save", { session });

export const sessionPath = () => invoke<string>("session_path");

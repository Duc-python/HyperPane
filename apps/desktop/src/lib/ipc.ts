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

export const sessionLoad = () => invoke<Session>("session_load");

export const sessionSave = (session: Session) =>
  invoke<void>("session_save", { session });

export const sessionPath = () => invoke<string>("session_path");

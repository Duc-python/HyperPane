import { agentLaunch, type AgentInfo, type RunResult } from "./ipc";

/** Everything a pane needs to start an agent. */
export interface PaneAgent {
  id: string;
  label: string;
  program: string;
  args: string[];
  /** The task has to be typed in once the agent is up. */
  needsPaste: boolean;
  /** The directory this run is confined to, when it has one. */
  cwd: string | null;
  /** The branch of that directory, when it is a worktree. */
  branch: string | null;
}

/** Resolve one catalogue agent into pane arguments. */
export async function preparePaneAgent(
  agent: AgentInfo,
  task: string,
  worktree: { path: string; branch: string } | null,
): Promise<PaneAgent> {
  const launch = await agentLaunch(agent.id, task);
  return {
    id: agent.id,
    label: launch.label,
    program: launch.program,
    args: launch.args,
    needsPaste: launch.needsPaste,
    cwd: worktree?.path ?? null,
    branch: worktree?.branch ?? null,
  };
}

/**
 * A child's report as one short phrase.
 *
 * The word "claim" is doing real work here: this is what the child said about
 * its own work, and the header presents it as such rather than as a verdict.
 */
export function describeResult(result: RunResult): string {
  if (result.status === "blocked") return "blocked";

  const files =
    result.files.length === 1 ? "1 file" : `${result.files.length} files`;
  const tests = result.tests === "none" ? "" : ` · tests ${result.tests}`;
  return `${result.status} · ${files}${tests}`;
}

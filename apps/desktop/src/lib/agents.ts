import { agentOpen, type AgentInfo, type RunResult } from "./ipc";

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

/** Ask the app to open a run, and turn its answer into pane arguments. */
export async function openPaneAgent(
  paneId: string,
  agent: AgentInfo,
  task: string,
  repo: string | null,
  worktree: boolean,
): Promise<PaneAgent> {
  const opened = await agentOpen({
    pane: paneId,
    agent: agent.id,
    task,
    worktree,
    repo,
  });

  return {
    id: agent.id,
    label: opened.label,
    program: opened.program,
    args: opened.args,
    needsPaste: opened.needsPaste,
    cwd: opened.cwd,
    branch: opened.branch,
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

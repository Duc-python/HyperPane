import { agentOpen, type AgentInfo, type PaneAgent, type RunResult } from "./ipc";

/**
 * Everything a pane needs to start an agent: the stored launch itself, not a
 * second shape that has to be kept in step with the file it is written to.
 */
export type { PaneAgent };

/**
 * The agents to bring back, for the panes that are still there.
 *
 * The session file applies the same rule when it loads, and it is applied again
 * here because the layout is the authority on which panes exist right now: a
 * launch kept for a pane id that is gone could come back on a later split that
 * happened to reuse the id, which would put someone else's run in a new pane.
 */
export function restoredAgents(
  panes: readonly string[],
  stored: Readonly<Record<string, PaneAgent>>,
): Record<string, PaneAgent> {
  const out: Record<string, PaneAgent> = {};
  for (const pane of panes) {
    const launch = stored[pane];
    if (launch) out[pane] = launch;
  }
  return out;
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

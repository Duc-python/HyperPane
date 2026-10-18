import type { WorktreeStateView } from "./ipc";

/**
 * What removing one of the app's worktrees would cost.
 *
 * The app makes a worktree per agent run, and they accumulate. Deciding which can
 * go is a question about what is inside them, so the answer is phrased as what
 * would be lost rather than as a status: a person reads it and knows whether they
 * still care about that run.
 *
 * A branch is never deleted - only a person deletes work - so commits stay
 * reachable by name, and the sentence says so. What is actually lost is
 * uncommitted work, and the files of a directory that is not a worktree any more.
 */
export function removalCost(entry: WorktreeStateView): string | null {
  if (!entry.branch) {
    if (entry.entries === 0) return null;
    const noun = entry.entries === 1 ? "entry is" : "entries are";
    return `not a worktree any more, and ${entry.entries} ${noun} still in it`;
  }

  const losses: string[] = [];
  if (entry.changes > 0) {
    const noun = entry.changes === 1 ? "file" : "files";
    losses.push(`${entry.changes} uncommitted ${noun} would be lost`);
  }
  if (entry.commits > 0) {
    const noun = entry.commits === 1 ? "commit" : "commits";
    const verb = entry.commits === 1 ? "is" : "are";
    // Measured against the main worktree's branch, which is what a person means
    // by "has this run done anything?".
    losses.push(`${entry.commits} ${noun} of its own ${verb} only in it`);
  }
  if (losses.length === 0) return null;

  return `${losses.join(" · ")}; ${entry.branch} stays`;
}

/** Whether removing this needs a person to answer twice. */
export function needsConfirming(entry: WorktreeStateView): boolean {
  return removalCost(entry) !== null;
}

/** One line describing what is there, for the row that has nothing to lose. */
export function describeWorktree(entry: WorktreeStateView): string {
  if (!entry.branch) {
    return entry.entries === 0 ? "empty, and no longer a worktree" : "no longer a worktree";
  }
  if (entry.changes === 0 && entry.commits === 0) return "clean";
  return removalCost(entry) ?? "clean";
}

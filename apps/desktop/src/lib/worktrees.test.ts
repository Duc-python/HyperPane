import { describe, expect, it } from "vitest";
import type { WorktreeStateView } from "./ipc";
import { describeWorktree, needsConfirming, removalCost } from "./worktrees";

/**
 * The sentence a person reads before deleting something of their own. It has to
 * say what is *lost*, not how the directory is classified, because the point of
 * the panel is to decide, not to report.
 */
function entry(patch: Partial<WorktreeStateView> = {}): WorktreeStateView {
  return {
    path: "C:\\app\\worktrees\\agent-78d3aa19",
    branch: "agent/78d3aa19",
    changes: 0,
    commits: 0,
    base: "main",
    entries: 12,
    ...patch,
  };
}

describe("what removing a worktree would cost", () => {
  it("costs nothing when the run left nothing behind", () => {
    expect(removalCost(entry())).toBeNull();
    expect(needsConfirming(entry())).toBe(false);
    expect(describeWorktree(entry())).toBe("clean");
  });

  it("names uncommitted files, and says the branch stays", () => {
    const cost = removalCost(entry({ changes: 3 }));

    expect(cost).toContain("3 uncommitted files would be lost");
    expect(cost).toContain("agent/78d3aa19 stays");
    expect(needsConfirming(entry({ changes: 3 }))).toBe(true);
  });

  it("counts one file as a file", () => {
    expect(removalCost(entry({ changes: 1 }))).toContain("1 uncommitted file");
  });

  it("reports commits of its own, and does not call them lost", () => {
    const cost = removalCost(entry({ commits: 2 }));

    expect(cost).toContain("2 commits of its own are only in it");
    // They stay reachable by branch name, which is the whole reason removal is
    // allowed with a second click rather than refused.
    expect(cost).toContain("agent/78d3aa19 stays");
  });

  it("joins both kinds of work into one sentence", () => {
    const cost = removalCost(entry({ changes: 1, commits: 1 }));

    expect(cost).toContain("1 uncommitted file would be lost");
    expect(cost).toContain("1 commit of its own is only in it");
  });

  it("treats a leftover with files as something to look at", () => {
    const leftover = entry({ branch: null, entries: 4 });

    expect(removalCost(leftover)).toContain("not a worktree any more");
    expect(removalCost(leftover)).toContain("4 entries are still in it");
    expect(needsConfirming(leftover)).toBe(true);
    expect(describeWorktree(leftover)).toBe("no longer a worktree");
  });

  it("lets an empty leftover go in one click", () => {
    const empty = entry({ branch: null, entries: 0 });

    expect(removalCost(empty)).toBeNull();
    expect(describeWorktree(empty)).toBe("empty, and no longer a worktree");
  });
});

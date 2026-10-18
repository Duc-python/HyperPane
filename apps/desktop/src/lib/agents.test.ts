import { describe, expect, it } from "vitest";
import type { PaneAgent } from "./ipc";
import { restoredAgents } from "./agents";

/**
 * A restored run is a claim about what a pane is: the same CLI, in the same
 * worktree, or no pane at all. These tests pin the second half down - a launch is
 * dropped as soon as its pane is gone, rather than waiting for a later split that
 * happens to reuse the id.
 */
const launch: PaneAgent = {
  id: "opencode",
  label: "opencode",
  program: "opencode",
  args: ["--model", "x"],
  needsPaste: true,
  cwd: "C:\\repo\\.worktrees\\run",
  branch: "hp/run",
};

describe("bringing agents back", () => {
  it("keeps the launch of a pane that is still there", () => {
    expect(restoredAgents(["p1"], { p1: launch })).toEqual({ p1: launch });
  });

  it("drops a launch whose pane is gone", () => {
    expect(restoredAgents(["p1"], { p1: launch, p2: launch })).toEqual({
      p1: launch,
    });
  });

  it("brings nothing back when nothing was stored", () => {
    expect(restoredAgents(["p1", "p2"], {})).toEqual({});
  });
});

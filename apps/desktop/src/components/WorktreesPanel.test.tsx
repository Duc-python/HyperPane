import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

/**
 * The one place in the app where something of a person's is deleted, so the tests
 * are about which answer is needed rather than about rendering: a run that left
 * nothing goes in one click, and anything else needs a second one that says
 * `force`.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { WorktreesPanel } from "./WorktreesPanel";

const CLEAN = {
  path: "C:\\app\\worktrees\\agent-clean",
  branch: "agent/clean0001",
  changes: 0,
  commits: 0,
  base: "main",
  entries: 12,
};

const DIRTY = {
  path: "C:\\app\\worktrees\\agent-dirty",
  branch: "agent/dirty001",
  changes: 2,
  commits: 0,
  base: "main",
  entries: 13,
};

const LEFTOVER = {
  path: "C:\\app\\worktrees\\agent-gone",
  branch: null,
  changes: 0,
  commits: 0,
  base: null,
  entries: 3,
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((command: string) => {
    if (command === "worktrees_list") {
      return Promise.resolve({ dir: "C:\\app\\worktrees", items: [CLEAN, DIRTY, LEFTOVER] });
    }
    return Promise.resolve(null);
  });
});

const formatPath = (path: string) => path.replace("C:\\app", "…");

describe("the worktrees panel", () => {
  it("describes what each one would cost", async () => {
    render(<WorktreesPanel formatPath={formatPath} inUse={[]} onClose={() => undefined} />);

    expect(await screen.findByText("agent/clean0001")).toBeTruthy();
    expect(screen.getByText("clean")).toBeTruthy();
    expect(
      screen.getByText(/removing it: 2 uncommitted files would be lost/),
    ).toBeTruthy();
    expect(screen.getByText(/removing it: not a worktree any more/)).toBeTruthy();
  });

  it("removes a run that left nothing behind in one click", async () => {
    render(<WorktreesPanel formatPath={formatPath} inUse={[]} onClose={() => undefined} />);
    await screen.findByText("agent/clean0001");

    const rows = screen.getAllByRole("button", { name: "remove" });
    fireEvent.click(rows[0] as HTMLElement);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("worktree_remove", {
        path: CLEAN.path,
        force: false,
      });
    });
  });

  it("asks twice before discarding uncommitted work, and says so", async () => {
    render(<WorktreesPanel formatPath={formatPath} inUse={[]} onClose={() => undefined} />);
    await screen.findByText("agent/dirty001");

    const dirtyRow = screen.getByText("agent/dirty001").closest("li");
    const button = dirtyRow?.querySelector("button") as HTMLElement;
    fireEvent.click(button);

    // The first click only arms it: nothing was removed.
    expect(invoke).not.toHaveBeenCalledWith("worktree_remove", expect.anything());
    expect(button.textContent).toBe("remove anyway");

    fireEvent.click(button);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("worktree_remove", {
        path: DIRTY.path,
        force: true,
      });
    });
  });

  it("re-reads the directory after a removal, so the list is what is there", async () => {
    render(<WorktreesPanel formatPath={formatPath} inUse={[]} onClose={() => undefined} />);
    await screen.findByText("agent/clean0001");

    invoke.mockImplementation((command: string) => {
      if (command === "worktrees_list") {
        return Promise.resolve({ dir: "C:\\app\\worktrees", items: [DIRTY, LEFTOVER] });
      }
      return Promise.resolve(null);
    });

    const cleanRow = screen.getByText("agent/clean0001").closest("li");
    fireEvent.click(cleanRow?.querySelector("button") as HTMLElement);

    await waitFor(() => {
      expect(screen.queryByText("agent/clean0001")).toBeNull();
    });
  });

  it("will not remove the worktree a run is living in", async () => {
    // Removing it would take the directory out from under a live process; the
    // app knows which runs are open, so the row says so instead of letting the
    // operating system explain it later.
    render(
      <WorktreesPanel
        formatPath={formatPath}
        inUse={["c:/app/worktrees/agent-clean/"]}
        onClose={() => undefined}
      />,
    );

    expect(await screen.findByText("a run is using this")).toBeTruthy();
    expect(screen.getByText("close that pane before removing it")).toBeTruthy();

    const cleanRow = screen.getByText("agent/clean0001").closest("li");
    const button = cleanRow?.querySelector("button") as HTMLButtonElement;
    expect(button.disabled).toBe(true);

    fireEvent.click(button);
    expect(invoke).not.toHaveBeenCalledWith("worktree_remove", expect.anything());
  });
});

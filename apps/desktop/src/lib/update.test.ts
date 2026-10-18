import { describe, expect, it, vi, beforeEach } from "vitest";

/**
 * The update line is a claim about how two shells behave, so the tests pin the
 * shell idioms down: `&&` where it exists, `$?` where it does not, and a guard in
 * front of every step that must not run after a failure. The typing helper is the
 * other half - it must wait for a shell that is not there yet, and give up rather
 * than wait forever.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { guardedChain, typeIntoPane, updateCommand, updateHint } from "./update";

beforeEach(() => {
  invoke.mockReset();
});

describe("guarding a chain of commands", () => {
  it("uses the older spelling of success on Windows", () => {
    expect(guardedChain(["a", "b", "c"], "windows")).toBe(
      "a; if ($?) { b }; if ($?) { c }",
    );
  });

  it("uses && where the shell has it", () => {
    expect(guardedChain(["a", "b", "c"], "linux")).toBe("a && b && c");
    expect(guardedChain(["a", "b"], "macos")).toBe("a && b");
  });

  it("leaves no guard dangling when there is nothing to guard", () => {
    expect(guardedChain([], "windows")).toBe("");
  });
});

describe("the update command", () => {
  it("pulls, installs, and only checks the build while the app runs on Windows", () => {
    const command = updateCommand("windows");
    // Split on the guard itself: each piece is one step, in order, and the last
    // one is the note rather than a command.
    const steps = command
      .split("; if ($?) { ")
      .map((step) => step.replace(/ \}$/, ""));

    expect(steps[0]).toBe("git pull --ff-only");
    expect(steps[1]).toBe("pnpm install");
    expect(steps[2]).toContain("with-msvc.ps1 -- cargo check -p hyperpane");

    // The build itself is not run here: the executable is locked while this app
    // is open, so it is named in the note instead, for after the window closes.
    expect(steps[3]).toContain("Write-Host");
    expect(steps[3]).toContain("Close this window");
    expect(steps[3]).toContain("cargo build -p hyperpane");
    expect(steps).toHaveLength(4);
  });

  it("pulls, installs and builds where a running program can be replaced", () => {
    const command = updateCommand("macos");

    expect(command).toBe(
      "git pull --ff-only && pnpm install && cargo build -p hyperpane",
    );
    // No MSVC environment to find, no note about a lock, and no second step to
    // leave to a person.
    expect(command).not.toContain("with-msvc");
    expect(command).not.toContain("Write-Host");
  });
  it("says in the palette which platform finishes the job later", () => {
    // The hint is the only warning a person gets before a build starts, so it
    // has to differ where the behaviour does.
    expect(updateHint("windows")).toContain("after this window closes");
    expect(updateHint("linux")).toBe("pull, install and build here");
    expect(updateHint("macos")).not.toContain("after this window closes");
  });
});

describe("typing a line into a pane", () => {
  it("waits for the shell to exist, then types the line and a carriage return", async () => {
    let calls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "pane_list") {
        calls += 1;
        return Promise.resolve(calls < 2 ? [] : ["p3"]);
      }
      return Promise.resolve(null);
    });

    const typed = await typeIntoPane("p3", "echo hi", {
      polls: 5,
      delayMs: 0,
    });

    expect(typed).toBe(true);
    expect(invoke).toHaveBeenCalledWith("pane_write", {
      id: "p3",
      data: "echo hi\r",
    });
    expect(calls).toBe(2);
  });

  it("gives up when the pane never comes up, and writes nothing", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "pane_list" ? [] : null),
    );

    const typed = await typeIntoPane("p9", "echo hi", {
      polls: 2,
      delayMs: 0,
    });

    expect(typed).toBe(false);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).not.toHaveBeenCalledWith("pane_write", expect.anything());
  });

  it("reports a failed write instead of claiming the pane ran it", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "pane_list") return Promise.resolve(["p1"]);
      return Promise.reject(new Error("no such pane"));
    });

    const typed = await typeIntoPane("p1", "echo hi", {
      polls: 2,
      delayMs: 0,
    });

    expect(typed).toBe(false);
  });
});

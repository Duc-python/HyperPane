import { describe, expect, it } from "vitest";
import {
  displayPaneTitle,
  paneRows,
  paneTone,
  titleLooksLikeProgramPath,
  type PaneLive,
} from "./panes";
import type { PaneSnapshot } from "./paneRuntime";

function snapshot(overrides: Partial<PaneSnapshot> = {}): PaneSnapshot {
  return {
    status: "live",
    title: "",
    cwd: null,
    exitCode: null,
    durationMs: null,
    running: false,
    ...overrides,
  };
}

function live(
  overrides: Partial<PaneSnapshot> = {},
  phase: PaneLive["phase"] = null,
): PaneLive {
  return { snapshot: snapshot(overrides), phase };
}

describe("a pane's name", () => {
  it("uses the shell's own title when it sets one", () => {
    expect(displayPaneTitle("npm run dev", "PowerShell")).toBe("npm run dev");
  });

  it("falls back when the title is really a program path", () => {
    // ConPTY reports the program as the window title until the shell names the
    // window, and `...\pwsh.exe` as a pane's name tells nobody anything.
    expect(
      titleLooksLikeProgramPath("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
    ).toBe(true);
    expect(
      displayPaneTitle(
        "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
        "PowerShell",
      ),
    ).toBe("PowerShell");
    expect(displayPaneTitle("/usr/bin/fish", "shell")).toBe("shell");
    expect(displayPaneTitle("wsl.exe", "shell")).toBe("shell");
  });

  it("falls back for a title that is only whitespace", () => {
    expect(displayPaneTitle("   ", "shell")).toBe("shell");
  });

  it("keeps a title that merely contains a slash", () => {
    // A shell can name its window after a directory, and that is a name.
    expect(displayPaneTitle("~/work/hyperpane", "shell")).toBe(
      "~/work/hyperpane",
    );
  });
});

describe("a pane's tone", () => {
  it("is starting until a view has reported anything", () => {
    expect(paneTone(undefined)).toBe("starting");
    expect(paneTone(live({ status: "starting" }))).toBe("starting");
  });

  it("is live for an idle shell, and busy while a command runs", () => {
    expect(paneTone(live())).toBe("live");
    expect(paneTone(live({ running: true }))).toBe("busy");
  });

  it("is busy for an agent that is working", () => {
    expect(paneTone(live({}, "working"))).toBe("busy");
    expect(paneTone(live({}, "quiet"))).toBe("live");
  });

  it("does not report a finished shell as live", () => {
    // A pane whose process is gone is not a pane somebody can type into, even
    // if its last command left it looking busy.
    expect(paneTone(live({ status: "exited", running: true }))).toBe("exited");
    expect(paneTone(live({ status: "failed" }))).toBe("failed");
  });
});

describe("a layout as rows", () => {
  it("keeps the layout's order, its focus, and each pane's own name", () => {
    const rows = paneRows({
      ids: ["p1", "p2", "p3"],
      live: { p1: live({ title: "dev server" }), p3: live({}, "working") },
      focused: "p3",
      fallback: (id) => `fallback ${id}`,
    });

    expect(rows.map((row) => row.id)).toEqual(["p1", "p2", "p3"]);
    expect(rows[0]?.label).toBe("dev server");
    // A pane nothing has drawn yet keeps its place in the list.
    expect(rows[1]?.label).toBe("fallback p2");
    expect(rows[1]?.tone).toBe("starting");
    expect(rows[2]?.tone).toBe("busy");
    expect(rows.map((row) => row.focused)).toEqual([false, false, true]);
  });

  it("carries the directory a shell reported, so a row can say where it is", () => {
    const rows = paneRows({
      ids: ["p1"],
      live: { p1: live({ cwd: "C:\\work" }) },
      focused: null,
      fallback: () => "shell",
    });
    expect(rows[0]?.cwd).toBe("C:\\work");
  });
});

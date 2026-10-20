import { describe, expect, it } from "vitest";
import {
  displayPaneTitle,
  orphanPanes,
  paneRows,
  paneTone,
  tilingRatios,
  titleLooksLikeProgramPath,
  widestPane,
  type PaneLive,
} from "./panes";
import type { LayoutNode } from "./ipc";
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

describe("sessions left over by a rebuilt interface", () => {
  const split: LayoutNode = {
    type: "split",
    direction: "row",
    ratio: 0.5,
    first: { type: "leaf", pane: "p1" },
    second: { type: "leaf", pane: "p2" },
  };

  it("keeps the sessions its panes are still using", () => {
    // The whole point of the reconnect: these shells are alive and the panes on
    // screen are about to attach to them, so closing them would restart work
    // that never stopped.
    expect(orphanPanes(["p1", "p2"], split)).toEqual([]);
  });

  it("closes a session no pane is using any more", () => {
    expect(orphanPanes(["p1", "p2", "p3"], split)).toEqual(["p3"]);
  });

  it("closes a session whose pane the layout has forgotten", () => {
    const narrower: LayoutNode = { type: "leaf", pane: "p2" };
    expect(orphanPanes(["p1", "p2"], narrower)).toEqual(["p1"]);
  });

  it("has nothing to reconnect to without a workspace", () => {
    expect(orphanPanes(["p1", "p2"], null)).toEqual(["p1", "p2"]);
  });
});

describe("where a team wave lands", () => {
  it("picks the pane that owns the most of the window", () => {
    // p1 is half the window; p2 and p3 are quarters. A wave goes into p1.
    const layout: LayoutNode = {
      type: "split",
      direction: "row",
      ratio: 0.5,
      first: { type: "leaf", pane: "p1" },
      second: {
        type: "split",
        direction: "column",
        ratio: 0.5,
        first: { type: "leaf", pane: "p2" },
        second: { type: "leaf", pane: "p3" },
      },
    };

    const widest = widestPane(layout);
    // The path names where that leaf sits, which is also where the split that
    // replaces it will sit.
    expect(widest?.pane).toBe("p1");
    expect(widest?.path).toEqual([false]);
    expect(widest?.share).toBeCloseTo(0.5);
  });

  it("names the path of a nested pane the way a resize does", () => {
    const layout: LayoutNode = {
      type: "split",
      direction: "row",
      ratio: 0.25,
      first: { type: "leaf", pane: "p1" },
      second: {
        type: "split",
        direction: "row",
        ratio: 0.25,
        first: { type: "leaf", pane: "p2" },
        second: { type: "leaf", pane: "p3" },
      },
    };

    expect(widestPane(layout)).toEqual({
      pane: "p3",
      path: [true, true],
      share: 0.5625,
    });
  });

  it("splits a row into equal columns, not one wide pane and slivers", () => {
    expect(tilingRatios(1)).toEqual([0.5]);
    expect(tilingRatios(2)).toEqual([1 / 3, 1 / 2]);
    expect(tilingRatios(3)).toEqual([1 / 4, 1 / 3, 1 / 2]);
  });
});
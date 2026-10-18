import { describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { PaneRow } from "../lib/panes";
import type { RoomView, TeamStatus, Workspace } from "../lib/ipc";

import { TopBar } from "./TopBar";

/**
 * The tab strip is the layout read sideways: one chip per pane, and a click
 * focuses rather than moves. What is worth guarding is that every chip acts on
 * the pane it names, and that a chip only appears when there is something
 * behind it.
 */

const WORKSPACE: Workspace = {
  id: "main",
  name: "Main",
  layout: { type: "leaf", pane: "p1" },
  focused: "p1",
  active: true,
};

const PANES: PaneRow[] = [
  { id: "p1", label: "dev server", cwd: "C:\\work", tone: "live", focused: true },
  { id: "p2", label: "Codex", cwd: null, tone: "busy", focused: false },
];

const VISITING: RoomView = {
  mode: "visiting",
  port: null,
  me: "guest",
  room: {
    id: "room",
    participants: [
      { id: "host", name: "Duc", focus: "p1", following: null, host: true },
      { id: "guest", name: "Mai", focus: null, following: null, host: false },
    ],
    panes: [{ pane: "p1", access: "control" }],
    grants: [],
  },
  transcript: [],
  refused: null,
};

function renderTopBar(overrides: Partial<Parameters<typeof TopBar>[0]> = {}) {
  const props = {
    workspace: WORKSPACE,
    panes: PANES,
    info: null,
    profileName: "PowerShell",
    canClose: true,
    teamRun: null as TeamStatus | null,
    room: null as RoomView | null,
    roomOpen: false,
    onFocusPane: vi.fn(),
    onClosePane: vi.fn(),
    onSplit: vi.fn(),
    onCloseFocused: vi.fn(),
    onOpenProfiles: vi.fn(),
    onOpenRun: vi.fn(),
    onToggleRoom: vi.fn(),
    onOpenPalette: vi.fn(),
    ...overrides,
  };
  render(<TopBar {...props} />);
  return props;
}

describe("the tab strip", () => {
  it("names every pane, and focuses the one that was clicked", () => {
    const props = renderTopBar();

    expect(screen.getByText("dev server")).toBeTruthy();
    expect(screen.getByText("Codex")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Codex" }));
    expect(props.onFocusPane).toHaveBeenCalledWith("p2");
  });

  it("closes the pane its × belongs to", () => {
    const props = renderTopBar();
    fireEvent.click(screen.getByRole("button", { name: "Close pane p2" }));
    expect(props.onClosePane).toHaveBeenCalledWith("p2");
  });

  it("splits right from the +, and offers no × when the last pane must stay", () => {
    const props = renderTopBar({ canClose: false });

    fireEvent.click(screen.getByRole("button", { name: "Split the focused pane right" }));
    expect(props.onSplit).toHaveBeenCalledWith("row");
    expect(screen.queryByRole("button", { name: "Close pane p1" })).toBeNull();
  });
});

describe("what the bar chips say", () => {
  it("counts the panes and names the workspace", () => {
    renderTopBar();
    expect(screen.getByText("Main")).toBeTruthy();
    expect(screen.getByText("02")).toBeTruthy();
  });

  it("says there is no room rather than showing a zero", () => {
    const props = renderTopBar();
    fireEvent.click(screen.getByRole("button", { name: "no room" }));
    expect(props.onToggleRoom).toHaveBeenCalled();
  });

  it("names the room's mode and how many people are in it", () => {
    renderTopBar({ room: VISITING });
    expect(screen.getByRole("button", { name: "visiting · 2" })).toBeTruthy();
  });

  it("shows a team chip only while a run is going", () => {
    renderTopBar();
    expect(screen.queryByRole("button", { name: /^team/ })).toBeNull();

    cleanup();
    renderTopBar({
      teamRun: {
        key: "review",
        title: "Review",
        wave: 2,
        running: 1,
        holding: null,
        holdingMessage: null,
        nextWave: null,
        nextWavePanes: 0,
        wavesRemaining: 1,
        claims: [],
      },
    });
    expect(screen.getByRole("button", { name: /^team Review/ })).toBeTruthy();
  });
});

import { describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { PaneRow } from "../lib/panes";
import type { RoomView, Workspace } from "../lib/ipc";

import { Sidebar } from "./Sidebar";

/**
 * The rail is a reading of state the app already has, so the tests are about
 * which row asks for what: a pane row focuses or closes one pane, and a room
 * row follows a person only where following means something.
 */

const WORKSPACE: Workspace = {
  id: "main",
  name: "Main",
  layout: { type: "leaf", pane: "p1" },
  focused: "p1",
  active: true,
};

function row(id: string, label: string, tone: PaneRow["tone"], focused: boolean): PaneRow {
  return { id, label, cwd: null, tone, focused };
}

const PANES: PaneRow[] = [
  row("p1", "dev server", "live", true),
  row("p2", "Codex", "busy", false),
  row("p3", "tests", "exited", false),
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

function renderSidebar(overrides: Partial<Parameters<typeof Sidebar>[0]> = {}) {
  const props = {
    workspace: WORKSPACE,
    panes: PANES,
    canClose: true,
    room: null as RoomView | null,
    paneLabel: (id: string) => id,
    onFocusPane: vi.fn(),
    onClosePane: vi.fn(),
    onSplit: vi.fn(),
    onFollow: vi.fn(),
    onOpenRoom: vi.fn(),
    onOpenProfiles: vi.fn(),
    onOpenSettings: vi.fn(),
    onHide: vi.fn(),
    ...overrides,
  };
  render(<Sidebar {...props} />);
  return props;
}

describe("the terminals list", () => {
  it("is one row per pane, and clicking a row focuses that pane", () => {
    const props = renderSidebar();

    for (const pane of PANES) {
      expect(screen.getByRole("button", { name: pane.label })).toBeTruthy();
    }
    // A pane that is working says so rather than looking idle.
    expect(screen.getByRole("img", { name: "working" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "tests" }));
    expect(props.onFocusPane).toHaveBeenCalledWith("p3");
  });

  it("closes the pane the × belongs to, and has no × for the last pane", () => {
    const props = renderSidebar();
    fireEvent.click(screen.getByRole("button", { name: "Close pane p2" }));
    expect(props.onClosePane).toHaveBeenCalledWith("p2");

    // A session always has one pane, so the last one cannot be closed.
    cleanup();
    render(
      <Sidebar
        {...props}
        canClose={false}
        panes={[row("p1", "dev server", "live", true)]}
      />,
    );
    expect(screen.queryByRole("button", { name: "Close pane p1" })).toBeNull();
  });

  it("offers to open a pane, which is a split of the focused one", () => {
    const props = renderSidebar();
    fireEvent.click(screen.getByRole("button", { name: "New pane" }));
    expect(props.onSplit).toHaveBeenCalledWith("row");
  });
});

describe("the workspace list", () => {
  it("says there is one workspace rather than offering to add another", () => {
    renderSidebar();

    expect(screen.getByText("Main")).toBeTruthy();
    expect(screen.getByText(/one workspace per window/)).toBeTruthy();
    // A `+` here would be a button with no command behind it.
    expect(screen.queryByRole("button", { name: /workspace/i })).toBeNull();
  });
});

describe("the room list", () => {
  it("says nothing is going on when nothing is, and offers a way in", () => {
    const props = renderSidebar();
    expect(screen.getByText(/not in a room/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "host or join one" }));
    expect(props.onOpenRoom).toHaveBeenCalled();
  });

  it("lets a visitor follow someone else, and never itself", () => {
    const props = renderSidebar({ room: VISITING });

    expect(screen.getAllByText("Duc").length).toBeGreaterThan(0);
    const follow = screen.getByTitle(
      "Follow Duc: watch the pane they are looking at",
    );
    fireEvent.click(follow);
    expect(props.onFollow).toHaveBeenCalledWith("host");
    // Following yourself would be a request the room answers with nothing.
    expect(screen.queryByTitle(/Follow Mai/)).toBeNull();
  });

  it("offers no follow from the host's own rail, which has no stream to ask", () => {
    renderSidebar({ room: { ...VISITING, mode: "hosting", me: "host" } });
    expect(screen.queryByTitle(/Follow Duc/)).toBeNull();
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { RoomView } from "../lib/ipc";

/**
 * The panel talks to the app through `invoke`, which only exists inside Tauri.
 * Mocking that one module is enough to render the real component and check what
 * it shows and what it asks for.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { RoomPanel } from "./RoomPanel";

function hosting(): RoomView {
  return {
    mode: "hosting",
    port: 4123,
    me: "host",
    room: {
      id: "room",
      participants: [
        { id: "host", name: "Duc", focus: "p1", following: null, host: true },
        { id: "p1", name: "Mai", focus: null, following: null, host: false },
      ],
      panes: [{ pane: "p1", access: "control" }],
      grants: [{ pane: "p1", who: "p1", access: "view" }],
    },
    transcript: [],
    refused: null,
  };
}

function visiting(grantsForHost: "view" | "control" = "control"): RoomView {
  return {
    mode: "visiting",
    port: null,
    me: "p1",
    room: {
      id: "room",
      participants: [
        { id: "host", name: "Duc", focus: "p1", following: null, host: true },
        { id: "p1", name: "Mai", focus: "p2", following: "host", host: false },
      ],
      panes: [
        { pane: "p1", access: grantsForHost },
        { pane: "p2", access: "view" },
      ],
      grants: [],
    },
    transcript: [
      { pane: "p1", text: "one\ntwo\n" },
      { pane: "p2", text: "elsewhere\n" },
    ],
    refused: null,
  };
}

beforeEach(() => {
  invoke.mockReset();
});

describe("the host's side", () => {
  it("says what each person may do, rather than what the host may do", async () => {
    invoke.mockResolvedValue(hosting());
    render(<RoomPanel focusedPane="p1" onClose={() => undefined} />);

    expect(await screen.findByText("Mai")).toBeTruthy();
    // Mai was granted view. The host's own access to its pane is control, and
    // showing that beside her name would be the wrong person's answer.
    await waitFor(() => {
      expect(screen.getByText("view")).toBeTruthy();
    });
    expect(screen.getByRole("button", { name: "can type" })).toBeTruthy();
  });

  it("asks the app to grant and to take away", async () => {
    invoke.mockResolvedValue(hosting());
    render(<RoomPanel focusedPane="p1" onClose={() => undefined} />);

    fireEvent.click(await screen.findByRole("button", { name: "can type" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("room_grant", {
        pane: "p1",
        who: "p1",
        access: "control",
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "revoke" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("room_revoke", { pane: "p1", who: "p1" });
    });
  });

  it("offers to share the focused pane, and not to share nothing", async () => {
    invoke.mockResolvedValue(hosting());
    const { unmount } = render(<RoomPanel focusedPane={null} onClose={() => undefined} />);

    const button = await screen.findByRole("button", { name: "share the focused pane" });
    expect(button.hasAttribute("disabled")).toBe(true);

    unmount();
    invoke.mockResolvedValue(hosting());
    render(<RoomPanel focusedPane="p1" onClose={() => undefined} />);
    const ready = await screen.findByRole("button", { name: "share the focused pane" });
    expect(ready.hasAttribute("disabled")).toBe(false);
  });
});

describe("the visitor's side", () => {
  it("shows one block per shared pane", async () => {
    invoke.mockResolvedValue(visiting());
    render(<RoomPanel focusedPane={null} onClose={() => undefined} />);

    expect(await screen.findByText(/one/)).toBeTruthy();
    expect(screen.getByText(/elsewhere/)).toBeTruthy();
    // The pane id is on each block, so two panes are two things to watch rather
    // than one mixture.
    expect(screen.getAllByText("p1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("p2").length).toBeGreaterThan(0);
  });

  it("marks the pane the person it follows is looking at", async () => {
    invoke.mockResolvedValue(visiting());
    render(<RoomPanel focusedPane={null} onClose={() => undefined} />);

    expect(await screen.findByText(/the one you are following/)).toBeTruthy();
    // Duc is looking at p1, which is the pane being followed; Mai is looking at
    // p2, which is where her own view is.
    expect(screen.getByText(/looking at p1/)).toBeTruthy();
    expect(screen.getByText(/looking at p2/)).toBeTruthy();
  });

  it("offers to follow someone, and to stop", async () => {
    invoke.mockResolvedValue(visiting());
    render(<RoomPanel focusedPane={null} onClose={() => undefined} />);

    fireEvent.click(await screen.findByRole("button", { name: "stop following" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("room_follow", { who: null });
    });
  });

  it("lets a controller type and a viewer only watch", async () => {
    invoke.mockResolvedValue(visiting("control"));
    const { unmount } = render(<RoomPanel focusedPane={null} onClose={() => undefined} />);
    const box = await screen.findByLabelText("Type into the pane");
    expect(box.hasAttribute("disabled")).toBe(false);

    unmount();
    invoke.mockResolvedValue(visiting("view"));
    render(<RoomPanel focusedPane={null} onClose={() => undefined} />);
    const watching = await screen.findByLabelText("Type into the pane");
    expect(watching.hasAttribute("disabled")).toBe(true);
  });
});

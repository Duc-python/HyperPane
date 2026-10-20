import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { RoomPaneView, RoomView } from "../lib/ipc";

/**
 * The dock talks to the app through `invoke`, which only exists inside Tauri,
 * and reads the room from the snapshot the app polls for it. Mocking the one
 * module is enough to render the real component and check what it shows and
 * what it asks for.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { ControlRoom } from "./ControlRoom";

const label = (id: string) => id;
const noop = () => undefined;

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

function visiting(
  grantsForHost: "view" | "control" = "control",
  panes: RoomPaneView[] = [
    { pane: "p1", access: grantsForHost },
    { pane: "p2", access: "view" },
  ],
): RoomView {
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
      panes,
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
    render(
      <ControlRoom
        view={hosting()}
        focusedPane="p1"
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    expect(screen.getByText("Mai")).toBeTruthy();
    // Mai was granted view. The host's own access to its pane is control, and
    // showing that beside her name would be the wrong person's answer.
    expect(screen.getByText("view")).toBeTruthy();
    expect(screen.getByRole("button", { name: "can type" })).toBeTruthy();
  });

  it("asks the app to grant and to take away", async () => {
    invoke.mockResolvedValue(hosting());
    render(
      <ControlRoom
        view={hosting()}
        focusedPane="p1"
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "can type" }));
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

  it("offers to share the focused pane, and not to share nothing", () => {
    const { unmount } = render(
      <ControlRoom
        view={hosting()}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    expect(
      screen.getByRole("button", { name: "share the focused pane" }).hasAttribute(
        "disabled",
      ),
    ).toBe(true);

    unmount();
    render(
      <ControlRoom
        view={hosting()}
        focusedPane="p1"
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    expect(
      screen.getByRole("button", { name: "share the focused pane" }).hasAttribute(
        "disabled",
      ),
    ).toBe(false);
  });

  it("hands what it learns back to the app that polls the room", async () => {
    const learned = hosting();
    invoke.mockResolvedValue(learned);
    const onView = vi.fn();
    render(
      <ControlRoom
        view={hosting()}
        focusedPane="p1"
        paneLabel={label}
        onView={onView}
        onHide={noop}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "stop hosting" }));
    await waitFor(() => {
      expect(onView).toHaveBeenCalledWith(learned);
    });
  });
});

describe("the visitor's side", () => {
  it("shows one block per shared pane", () => {
    render(
      <ControlRoom
        view={visiting()}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    // One line per line of output, so what the pane printed is what is shown.
    expect(screen.getByText("one")).toBeTruthy();
    expect(screen.getByText("two")).toBeTruthy();
    expect(screen.getByText("elsewhere")).toBeTruthy();
    // The pane is named on each block, so two panes are two things to watch
    // rather than one mixture.
    expect(screen.getAllByText("p1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("p2").length).toBeGreaterThan(0);
  });

  it("marks the pane the person it follows is looking at", () => {
    render(
      <ControlRoom
        view={visiting()}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    expect(screen.getByText(/the one you are following/)).toBeTruthy();
    // Duc is looking at p1, which is the pane being followed; Mai is looking at
    // p2, which is where her own view is.
    expect(screen.getByText(/looking at p1/)).toBeTruthy();
    expect(screen.getByText(/looking at p2/)).toBeTruthy();
  });

  it("offers to follow someone, and to stop", async () => {
    invoke.mockResolvedValue(visiting());
    render(
      <ControlRoom
        view={visiting()}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "stop following" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("room_follow", { who: null });
    });
  });

  it("lets a controller type and a viewer only watch", () => {
    const { unmount } = render(
      <ControlRoom
        view={visiting("control")}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    expect(
      screen.getByLabelText("Type into the pane").hasAttribute("disabled"),
    ).toBe(false);
    // The composer is the way into a pane, so it says what a line does.
    expect(screen.getByText(/activity log/)).toBeTruthy();

    unmount();
    render(
      <ControlRoom
        view={visiting("view")}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    const watching = screen.getByLabelText("Type into the pane");
    expect(watching.hasAttribute("disabled")).toBe(true);
    // Nothing may be typed, so there is nothing to choose between.
    expect(screen.queryByLabelText("Recipient")).toBeNull();
  });

  it("writes into the pane that was chosen, and clears the box", async () => {
    invoke.mockResolvedValue(visiting("control"));
    render(
      <ControlRoom
        view={visiting("control", [
          { pane: "p1", access: "control" },
          { pane: "p2", access: "control" },
        ])}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );

    fireEvent.change(screen.getByLabelText("Recipient"), {
      target: { value: "p2" },
    });
    const box = screen.getByLabelText("Type into the pane");
    fireEvent.change(box, { target: { value: "npm test" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("room_input", {
        pane: "p2",
        data: "npm test\r",
      });
    });
    expect((box as HTMLInputElement).value).toBe("");
  });

  it("says when the room refused something, rather than looking empty", () => {
    render(
      <ControlRoom
        view={{ ...visiting(), refused: "p2 is not shared with you" }}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    expect(screen.getByText(/p2 is not shared with you/)).toBeTruthy();
  });
});

describe("the dock itself", () => {
  it("says what it is, and can be hidden", () => {
    const onHide = vi.fn();
    render(
      <ControlRoom
        view={null}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={onHide}
      />,
    );

    expect(screen.getByRole("heading", { name: "Control Room" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Hide the control room" }));
    expect(onHide).toHaveBeenCalled();
  });

  it("offers to host or to join when nobody is in a room", () => {
    render(
      <ControlRoom
        view={null}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
    expect(screen.getByRole("button", { name: "host on this machine" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "join" })).toBeTruthy();
    // The loopback limit is a sentence, not something to discover later.
    expect(screen.getByText(/loopback only/)).toBeTruthy();
  });
});

describe("pairing from the dock", () => {
  /**
   * `invoke` answers by command name here.
   *
   * The dock asks more than one question, and a bare `mockResolvedValue` would
   * hand the peer list to the pairing card; this keeps each answer its own.
   */
  function answering(answers: Record<string, unknown>) {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command in answers ? answers[command] : undefined),
    );
  }

  function renderDock() {
    render(
      <ControlRoom
        view={null}
        focusedPane={null}
        paneLabel={label}
        onView={noop}
        onHide={noop}
      />,
    );
  }

  it("shows the code and the address the other machine needs", async () => {
    answering({
      pairing_status: { state: "idle" },
      peers_list: { path: "peers.json", peers: [] },
      pairing_host: {
        state: "waiting",
        code: "ABCDEFGHJKMN",
        address: "192.168.1.4:51234",
        hint: "on the same network as this machine",
        hosting: true,
      },
    });
    renderDock();

    fireEvent.click(screen.getByRole("button", { name: "show a code" }));

    expect(await screen.findByText("ABCD EFGH JKMN")).toBeTruthy();
    expect(screen.getByText(/listening on 192\.168\.1\.4:51234/)).toBeTruthy();
    expect(screen.getByText(/same network/)).toBeTruthy();
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("pairing_host");
    });
  });

  it("fills both answers from the line the other side sent", () => {
    answering({
      pairing_status: { state: "idle" },
      peers_list: { path: "peers.json", peers: [] },
    });
    renderDock();

    fireEvent.click(screen.getByRole("button", { name: "use a code" }));
    fireEvent.change(screen.getByLabelText("Their address"), {
      target: { value: "10.0.0.7:4123 ABCD-EFGH-JKMN" },
    });

    expect(
      (screen.getByLabelText("Their address") as HTMLInputElement).value,
    ).toBe("10.0.0.7:4123");
    expect(
      (screen.getByLabelText("Pairing code") as HTMLInputElement).value,
    ).toBe("ABCD EFGH JKMN");
  });

  it("says what it is waiting for, and can stop waiting", async () => {
    answering({
      pairing_status: {
        state: "waiting",
        code: null,
        address: "10.0.0.7:4123",
        hint: "asking the other machine",
        hosting: false,
      },
      peers_list: { path: "peers.json", peers: [] },
      pairing_cancel: { state: "idle" },
    });
    renderDock();

    expect(await screen.findByText(/pairing with 10\.0\.0\.7:4123/)).toBeTruthy();
    // A joining side has no code to show: the host has it.
    expect(screen.queryByText(/^[A-Z0-9]{4} [A-Z0-9]{4}/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "stop" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("pairing_cancel");
    });
  });

  it("names the machines this one has already paired with", async () => {
    const peer = {
      id: "b".repeat(64),
      short: "bbbbbbbb",
      name: "laptop",
      pairedAtMs: 1,
      lastSeenMs: 2,
    };
    answering({
      pairing_status: { state: "paired", peer },
      peers_list: { path: "peers.json", peers: [peer] },
    });
    renderDock();

    // The panel says it twice: once as what just happened, once in the list of
    // machines this one trusts, with the short id beside each.
    expect(await screen.findAllByText("laptop")).not.toHaveLength(0);
    // The short id is what tells two machines with the same name apart.
    expect(screen.getAllByText("bbbbbbbb").length).toBeGreaterThan(0);
    expect(screen.getByText(/paired with/)).toBeTruthy();
  });
});


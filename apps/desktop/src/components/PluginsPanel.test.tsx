import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PluginView, PluginsView } from "../lib/ipc";

/**
 * The panel talks to the app through `invoke`, which only exists inside Tauri.
 * Mocking the one module it reaches for is enough to render the real component
 * and check what it asks the app to do.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { PluginsPanel } from "./PluginsPanel";

function server(overrides: Partial<PluginView> = {}): PluginView {
  return {
    id: "scripted",
    title: "Scripted server",
    command: "C:\\tools\\fake-mcp-server.exe",
    args: ["modern", "--read-only"],
    enabled: true,
    state: "declared",
    problem: null,
    peer: null,
    era: null,
    protocolVersion: null,
    lastWords: [],
    approved: false,
    approvalNote: "never run from here before: read the command and say yes",
    toolChangeNote: null,
    tools: [],
    ...overrides,
  };
}

function view(overrides: Partial<PluginsView> = {}): PluginsView {
  return {
    path: "C:\\app\\plugins.json",
    trustPath: "C:\\app\\trust.json",
    problems: [],
    servers: [server()],
    ...overrides,
  };
}

const running = view({
  servers: [
    server({
      state: "running",
      approved: true,
      approvalNote: null,
      peer: "fake-mcp-server 0.1.0",
      era: "modern",
      protocolVersion: "2026-07-28",
      tools: [
        {
          name: "echo",
          title: "Says what it was given",
          description: "Says what it was given",
          inputSchema: { type: "object" },
        },
      ],
    }),
  ],
});

beforeEach(() => {
  invoke.mockReset();
});

describe("a plugin nobody has approved", () => {
  it("offers to run it rather than to start it, and shows the command whole", async () => {
    invoke.mockResolvedValue(view());
    render(<PluginsPanel onClose={() => undefined} />);

    expect(await screen.findByText("Scripted server")).toBeTruthy();

    // The whole command line is on screen: a truncated one is exactly what a
    // consent step must not show.
    expect(screen.getByText(/fake-mcp-server\.exe modern --read-only/)).toBeTruthy();
    expect(screen.getByText(/never run from here before/)).toBeTruthy();

    const button = screen.getByRole("button", { name: "run it" });
    expect(button).toBeTruthy();
    expect(screen.queryByRole("button", { name: "start" })).toBeNull();
  });

  it("asks the app to approve (and so start) it, and shows what came back", async () => {
    invoke.mockResolvedValueOnce(view()).mockResolvedValueOnce(running);
    render(<PluginsPanel onClose={() => undefined} />);

    fireEvent.click(await screen.findByRole("button", { name: "run it" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("plugins_approve", { id: "scripted" });
    });

    // The view the app returned is what is drawn: running, with its tools.
    expect(await screen.findByText(/fake-mcp-server 0.1.0/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "echo" })).toBeTruthy();
    expect(screen.queryByText(/never run from here before/)).toBeNull();
  });
});

describe("a plugin that is already approved", () => {
  it("offers to start it", async () => {
    invoke.mockResolvedValue(view({ servers: [server({ approved: true, approvalNote: null })] }));
    render(<PluginsPanel onClose={() => undefined} />);

    expect(await screen.findByRole("button", { name: "start" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "run it" })).toBeNull();
  });
});

describe("a tool list that moved", () => {
  it("says what moved and offers to accept it", async () => {
    invoke
      .mockResolvedValueOnce(
        view({
          servers: [
            server({
              state: "running",
              approved: true,
              approvalNote: null,
              toolChangeNote: "describes itself differently now: echo",
              tools: [
                {
                  name: "echo",
                  title: "Says what it was given",
                  description: "Says what it was given",
                  inputSchema: { type: "object" },
                },
              ],
            }),
          ],
        }),
      )
      .mockResolvedValueOnce(running);
    render(<PluginsPanel onClose={() => undefined} />);

    expect(await screen.findByText(/describes itself differently now: echo/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "accept it" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("plugins_approve", { id: "scripted" });
    });
  });
});

describe("when the app cannot answer", () => {
  it("shows the reason instead of an empty panel", async () => {
    invoke.mockRejectedValue("the plugin file is not valid JSON: expected value at line 1");
    render(<PluginsPanel onClose={() => undefined} />);

    expect(await screen.findByText(/not valid JSON/)).toBeTruthy();
  });
});

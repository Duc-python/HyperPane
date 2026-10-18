import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import type { AppInfo } from "../lib/ipc";

/**
 * The panel reads the plugin file's own paths from the app, so `invoke` is the
 * one module to mock: what is worth checking is that the paths a person cannot
 * otherwise find are on screen, and that an unreadable file says so.
 */
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: unknown = null;
  },
}));

import { SettingsPanel } from "./SettingsPanel";

const INFO: AppInfo = {
  name: "HyperPane",
  version: "0.1.0",
  platform: "windows",
  defaultShell: "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
  homeDir: "C:\\Users\\ducar",
};

const noop = () => undefined;

beforeEach(() => {
  invoke.mockReset();
});

describe("where things live", () => {
  it("names every file HyperPane keeps, and the machine it is on", async () => {
    invoke.mockResolvedValue({
      path: "C:\\app\\plugins.json",
      trustPath: "C:\\app\\trust.json",
      problems: [],
      servers: [],
    });
    render(
      <SettingsPanel
        info={INFO}
        sessionFile={"C:\\app\\session.json"}
        auditFile={"C:\\app\\activity.jsonl"}
        onClose={noop}
      />,
    );

    expect(await screen.findByText("C:\\app\\plugins.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\trust.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\session.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\activity.jsonl")).toBeTruthy();
    expect(screen.getByText("HyperPane 0.1.0")).toBeTruthy();
    expect(screen.getByText("C:\\Users\\ducar")).toBeTruthy();
  });

  it("says the plugin file could not be read, and still shows the rest", async () => {
    invoke.mockRejectedValue("the plugin file is not valid JSON");
    render(
      <SettingsPanel
        info={INFO}
        sessionFile={"C:\\app\\session.json"}
        auditFile={null}
        onClose={noop}
      />,
    );

    expect(await screen.findByText(/not valid JSON/)).toBeTruthy();
    // A file that cannot be read does not take the other paths with it.
    expect(screen.getByText("C:\\app\\session.json")).toBeTruthy();
    expect(screen.getAllByText("could not be read").length).toBeGreaterThan(0);
  });
});

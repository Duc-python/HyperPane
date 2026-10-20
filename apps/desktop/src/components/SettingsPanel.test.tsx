import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AppInfo, IdentityInfo, PeersView } from "../lib/ipc";

/**
 * The panel asks the app three questions: where the plugin files are, who this
 * machine is, and who it trusts. `invoke` is the one module to mock; what is
 * worth checking is that all three answers reach the page, and that each failure
 * is its own sentence rather than a blank.
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
  source: "C:\\Users\\ducar\\orca\\HyperPane",
};

const PLUGINS = {
  path: "C:\\app\\plugins.json",
  trustPath: "C:\\app\\trust.json",
  problems: [],
  servers: [],
};

const IDENTITY: IdentityInfo = {
  id: "ab".repeat(32),
  short: "abababab",
  fingerprint: "abab abab abab abab abab abab abab abab",
  path: "C:\\app\\identity.json",
};

const NOBODY: PeersView = { path: "C:\\app\\peers.json", peers: [] };

const noop = () => undefined;

function answers({
  plugins = () => Promise.resolve(PLUGINS),
  identity = () => Promise.resolve(IDENTITY),
  peers = () => Promise.resolve(NOBODY),
  reveal = () => Promise.resolve(undefined),
}: {
  plugins?: () => Promise<unknown>;
  identity?: () => Promise<unknown>;
  peers?: () => Promise<unknown>;
  reveal?: () => Promise<unknown>;
} = {}) {
  invoke.mockImplementation((command: string) => {
    if (command === "plugins_list") return plugins();
    if (command === "identity_info") return identity();
    if (command === "peers_list") return peers();
    if (command === "reveal_path") return reveal();
    return Promise.reject(`unexpected command ${command}`);
  });
}

function renderPanel() {
  return render(
    <SettingsPanel
      info={INFO}
      sessionFile={"C:\\app\\session.json"}
      auditFile={"C:\\app\\activity.jsonl"}
      onClose={noop}
    />,
  );
}

beforeEach(() => {
  invoke.mockReset();
});

describe("where things live", () => {
  it("names every file HyperPane keeps, and the machine it is on", async () => {
    answers();
    renderPanel();

    expect(await screen.findByText("C:\\app\\plugins.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\trust.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\session.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\activity.jsonl")).toBeTruthy();
    expect(screen.getByText("C:\\app\\identity.json")).toBeTruthy();
    expect(screen.getByText("C:\\app\\peers.json")).toBeTruthy();
    expect(screen.getByText("HyperPane 0.1.0")).toBeTruthy();
    expect(screen.getByText("C:\\Users\\ducar")).toBeTruthy();
  });

  it("shows this machine's name in the form it can be read out", async () => {
    answers();
    renderPanel();

    // Groups of four: the point is that two people can compare it by eye and
    // out loud, which a single 64-character line does not allow.
    expect(await screen.findByText(IDENTITY.fingerprint)).toBeTruthy();
  });

  it("shows a record where it lives, when asked", async () => {
    answers();
    renderPanel();

    await screen.findByText("C:\\app\\identity.json");
    fireEvent.click(screen.getByRole("button", { name: "Reveal identity file" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("reveal_path", {
        path: "C:\\app\\identity.json",
      });
    });
  });

  it("says the plugin file could not be read, and still shows the rest", async () => {
    answers({ plugins: () => Promise.reject("the plugin file is not valid JSON") });
    renderPanel();

    expect(await screen.findByText(/not valid JSON/)).toBeTruthy();
    expect(screen.getByText("C:\\app\\session.json")).toBeTruthy();
    expect(screen.getAllByText("could not be read").length).toBeGreaterThan(0);
  });
});

describe("who is trusted", () => {
  it("has no list yet, and says what pairing would do about it", async () => {
    answers();
    renderPanel();

    expect(await screen.findByText(/None yet\. Open the control room/)).toBeTruthy();
  });

  it("lists a pinned machine by name and the start of its id", async () => {
    answers({
      peers: () =>
        Promise.resolve({
          path: "C:\\app\\peers.json",
          peers: [
            {
              id: "cd".repeat(32),
              short: "cdcdcdcd",
              name: "laptop",
              pairedAtMs: 1_000,
              lastSeenMs: 2_000,
            },
          ],
        }),
    });
    renderPanel();

    expect(await screen.findByText("laptop")).toBeTruthy();
    expect(screen.getByText("cdcdcdcd")).toBeTruthy();
    expect(screen.queryByText(/None yet/)).toBeNull();
  });

  it("keeps an identity failure apart from the files it can still show", async () => {
    answers({ identity: () => Promise.reject("the key file does not hold a usable key") });
    renderPanel();

    expect(await screen.findByText(/does not hold a usable key/)).toBeTruthy();
    // The plugin paths arrived on their own and are not taken down with it.
    expect(screen.getByText("C:\\app\\plugins.json")).toBeTruthy();
  });
});

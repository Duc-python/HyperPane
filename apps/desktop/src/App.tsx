import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  agentsAvailable,
  appInfo,
  paneClose,
  paneIds,
  paneList,
  sessionLoad,
  sessionPath,
  sessionSave,
  workspaceClosePane,
  workspaceCycleFocus,
  workspaceInit,
  workspaceSetFocus,
  workspaceSetRatio,
  workspaceSplit,
  type AgentInfo,
  type AppInfo,
  type Profile,
  type Session,
  type SplitDirection,
  type Workspace,
} from "./lib/ipc";
import { chordForCommand, commandForEvent, formatChord } from "./lib/keybindings";
import { shortenPath } from "./lib/paths";
import { preparePaneAgent, type PaneAgent } from "./lib/agents";
import type { Command } from "./lib/commands";
import { LayoutView } from "./components/LayoutView";
import { CommandPalette } from "./components/CommandPalette";
import type { PaneController } from "./components/TerminalPane";

/** Must match `hp_core::SESSION_VERSION`. */
const SESSION_VERSION = 1;
const SAVE_DEBOUNCE_MS = 400;

/** The next free `p<N>` id, so a restored layout cannot collide with a new pane. */
function nextPaneNumber(workspace: Workspace): number {
  let highest = 0;
  for (const id of paneIds(workspace.layout)) {
    const digits = /^p(\d+)$/.exec(id)?.[1];
    if (digits) highest = Math.max(highest, Number(digits));
  }
  return highest + 1;
}

export default function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [defaultProfile, setDefaultProfile] = useState<string | null>(null);
  const [paneProfiles, setPaneProfiles] = useState<Record<string, string>>({});
  const [paneAgents, setPaneAgents] = useState<Record<string, PaneAgent>>({});
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** Set while the palette is asking for a task. */
  const [pendingAgent, setPendingAgent] = useState<AgentInfo | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [sessionFile, setSessionFile] = useState<string | null>(null);

  const nextPane = useRef(1);
  const workspaceRef = useRef<Workspace | null>(null);
  const defaultProfileRef = useRef<string | null>(null);
  const commandsRef = useRef<Command[]>([]);

  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  useEffect(() => {
    defaultProfileRef.current = defaultProfile;
  }, [defaultProfile]);

  // Boot: restore the stored layout, or open the first pane.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const details = await appInfo().catch(() => null);
      if (!cancelled && details) setInfo(details);

      const available = await agentsAvailable().catch(() => []);
      if (!cancelled) setAgents(available);

      try {
        const session = await sessionLoad();
        if (cancelled) return;
        setProfiles(session.profiles);
        setDefaultProfile(session.defaultProfile);
        setSessionFile(await sessionPath().catch(() => null));
        if (cancelled) return;

        // A webview reload leaves the previous PTYs alive in Rust. Every pane
        // on screen is about to be re-created from the layout, so close the
        // leftovers first and let each pane open its own shell.
        const stale = await paneList();
        await Promise.all(stale.map((id) => paneClose(id).catch(() => undefined)));
        if (cancelled) return;

        const fallbackProfile = session.defaultProfile ?? "default";

        if (session.workspace) {
          setWorkspace(session.workspace);
          nextPane.current = nextPaneNumber(session.workspace);
          setPaneProfiles(
            Object.fromEntries(
              paneIds(session.workspace.layout).map((id) => [id, fallbackProfile]),
            ),
          );
        } else {
          nextPane.current = 2;
          setWorkspace(await workspaceInit("p1"));
          setPaneProfiles({ p1: fallbackProfile });
        }
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Persist. Debounced so a drag does not write a file per pointer move.
  useEffect(() => {
    if (!workspace) return;
    const session: Session = {
      version: SESSION_VERSION,
      workspace,
      profiles,
      defaultProfile,
    };
    const timer = window.setTimeout(() => {
      void sessionSave(session).catch((cause) => setError(String(cause)));
    }, SAVE_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [workspace, profiles, defaultProfile]);

  const splitWith = useCallback(
    async (direction: SplitDirection, profileId: string | null) => {
      const current = workspaceRef.current;
      if (!current) return;
      const target = current.focused ?? paneIds(current.layout)[0];
      if (!target) return;

      const id = `p${nextPane.current++}`;
      const chosen = profileId ?? defaultProfileRef.current ?? "default";
      setPaneProfiles((map) => ({ ...map, [id]: chosen }));
      try {
        setWorkspace(await workspaceSplit(target, direction, id));
      } catch (cause) {
        setError(String(cause));
      }
    },
    [],
  );

  const closePane = useCallback(async (id: string) => {
    const current = workspaceRef.current;
    if (!current || paneIds(current.layout).length <= 1) return;
    await paneClose(id).catch(() => undefined);
    setPaneProfiles((map) => {
      const remaining = { ...map };
      delete remaining[id];
      return remaining;
    });
    setPaneAgents((map) => {
      const remaining = { ...map };
      delete remaining[id];
      return remaining;
    });
    try {
      setWorkspace(await workspaceClosePane(id));
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  // Focus is optimistic: a click must not wait for a round trip to look right.
  const focus = useCallback((id: string) => {
    setWorkspace((current) =>
      current && current.focused !== id ? { ...current, focused: id } : current,
    );
    void workspaceSetFocus(id).catch(() => undefined);
  }, []);

  const cycle = useCallback(async (delta: number) => {
    try {
      setWorkspace(await workspaceCycleFocus(delta));
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const commitRatio = useCallback(async (path: boolean[], ratio: number) => {
    try {
      setWorkspace(await workspaceSetRatio(path, ratio));
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const profileFor = useCallback(
    (paneId: string): Profile | null => {
      const wanted = paneProfiles[paneId];
      if (!wanted) return null;
      return profiles.find((profile) => profile.id === wanted) ?? null;
    },
    [paneProfiles, profiles],
  );

  const agentFor = useCallback(
    (paneId: string): PaneAgent | null => paneAgents[paneId] ?? null,
    [paneAgents],
  );

  /** Open an agent in a new pane beside the focused one. */
  const spawnAgent = useCallback(
    async (agent: AgentInfo, task: string) => {
      const current = workspaceRef.current;
      if (!current) return;
      const target = current.focused ?? paneIds(current.layout)[0];
      if (!target) return;

      const id = `p${nextPane.current++}`;
      try {
        const prepared = await preparePaneAgent(agent, task);
        // Both updates land in one render, so the new pane mounts with its
        // agent already known rather than starting a shell first.
        setPaneAgents((map) => ({ ...map, [id]: prepared }));
        setWorkspace(await workspaceSplit(target, "row", id));
      } catch (cause) {
        setError(String(cause));
      }
    },
    [],
  );

  const formatPath = useCallback(
    (path: string) => shortenPath(path, info?.homeDir ?? null),
    [info],
  );

  // Live panes, by id, so a command can act on the focused one. Panes register
  // themselves when their terminal exists and clear on the way out.
  const controllers = useRef(new Map<string, PaneController>());
  const onReady = useCallback(
    (id: string, controller: PaneController | null) => {
      if (controller) controllers.current.set(id, controller);
      else controllers.current.delete(id);
    },
    [],
  );

  const withFocusedPane = useCallback(
    (action: (controller: PaneController) => void) => {
      const focused = workspaceRef.current?.focused;
      if (!focused) return;
      const controller = controllers.current.get(focused);
      if (controller) action(controller);
    },
    [],
  );

  const commands = useMemo<Command[]>(() => {
    const hint = (id: string) => {
      const chord = chordForCommand(id);
      return chord ? formatChord(chord) : null;
    };
    const closeFocused = () => {
      const focused = workspaceRef.current?.focused;
      if (focused) void closePane(focused);
    };

    const list: Command[] = [
      {
        id: "app.palette",
        title: "Command palette",
        group: "App",
        hint: hint("app.palette"),
        run: () => setPaletteOpen((open) => !open),
      },
      {
        id: "pane.splitRight",
        title: "Split pane right",
        group: "Pane",
        hint: hint("pane.splitRight"),
        run: () => splitWith("row", null),
      },
      {
        id: "pane.splitDown",
        title: "Split pane down",
        group: "Pane",
        hint: hint("pane.splitDown"),
        run: () => splitWith("column", null),
      },
      {
        id: "pane.close",
        title: "Close focused pane",
        group: "Pane",
        hint: hint("pane.close"),
        run: closeFocused,
      },
      {
        id: "pane.next",
        title: "Focus next pane",
        group: "Pane",
        hint: hint("pane.next"),
        run: () => cycle(1),
      },
      {
        id: "pane.previous",
        title: "Focus previous pane",
        group: "Pane",
        hint: hint("pane.previous"),
        run: () => cycle(-1),
      },
      {
        id: "pane.previousPrompt",
        title: "Jump to the previous prompt",
        group: "Scrollback",
        hint: hint("pane.previousPrompt"),
        run: () => withFocusedPane((pane) => pane.jumpToPreviousPrompt()),
      },
      {
        id: "pane.nextPrompt",
        title: "Jump to the next prompt",
        group: "Scrollback",
        hint: hint("pane.nextPrompt"),
        run: () => withFocusedPane((pane) => pane.jumpToNextPrompt()),
      },
    ];

    for (const profile of profiles) {
      list.push({
        id: `profile.open.${profile.id}`,
        title: `New pane: ${profile.name}`,
        group: "Profile",
        hint: profile.shell ?? "platform default",
        run: () => splitWith("row", profile.id),
      });
    }

    for (const agent of agents.filter((entry) => entry.installed)) {
      list.push({
        id: `agent.run.${agent.id}`,
        title: `Run ${agent.label}…`,
        group: "Agent",
        hint: agent.program,
        // The palette asks for the task next, in the same input.
        keepOpen: true,
        run: () => setPendingAgent(agent),
      });
    }

    return list;
  }, [profiles, agents, splitWith, closePane, cycle, withFocusedPane]);

  useEffect(() => {
    commandsRef.current = commands;
  }, [commands]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const id = commandForEvent(event);
      if (!id) return;
      // Capture phase plus stopPropagation: without this the terminal would
      // also receive the chord and send it to the shell.
      event.preventDefault();
      event.stopPropagation();
      const command = commandsRef.current.find((entry) => entry.id === id);
      void command?.run();
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, []);

  const paneCount = workspace ? paneIds(workspace.layout).length : 0;
  const canClose = paneCount > 1;
  const activeProfile = profiles.find(
    (profile) => profile.id === (defaultProfile ?? ""),
  );

  return (
    <div className="flex h-full w-full flex-col bg-base">
      <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line/70 bg-surface/50 px-3 backdrop-blur">
        <div className="flex items-center gap-2">
          <span
            aria-hidden
            className="size-2 rounded-[3px] bg-accent shadow-[0_0_12px_-2px_var(--color-accent)]"
          />
          <span className="text-[13px] font-semibold tracking-tight text-ink">
            HyperPane
          </span>
          {info ? (
            <span className="text-[10px] text-ink-faint">v{info.version}</span>
          ) : null}
        </div>

        <div className="h-4 w-px bg-line" aria-hidden />

        <div className="flex items-center gap-1">
          <ToolButton
            label="Split right"
            shortcut={formatChord("mod+shift+e")}
            onClick={() => void splitWith("row", null)}
            disabled={!workspace}
          >
            <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
            <path d="M6.5 1.8v8.4" />
          </ToolButton>
          <ToolButton
            label="Split down"
            shortcut={formatChord("mod+shift+o")}
            onClick={() => void splitWith("column", null)}
            disabled={!workspace}
          >
            <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
            <path d="M1.2 6h10.6" />
          </ToolButton>
          <ToolButton
            label="Close pane"
            shortcut={formatChord("mod+shift+w")}
            onClick={() => {
              const focused = workspace?.focused;
              if (focused) void closePane(focused);
            }}
            disabled={!canClose}
          >
            <path d="M3.4 3.4l6.2 6.2M9.6 3.4l-6.2 6.2" />
          </ToolButton>
        </div>

        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          className="flex items-center gap-2 rounded-md border border-line bg-raised/60 px-2.5 py-1 text-[11px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink"
        >
          <span>Commands</span>
          <kbd className="rounded border border-line bg-surface px-1.5 py-px text-[9px]">
            {formatChord("mod+shift+p")}
          </kbd>
        </button>

        <div className="ml-auto flex items-center gap-3 text-[10px] text-ink-faint">
          {activeProfile ? (
            <span title={activeProfile.shell ?? "platform default shell"}>
              {activeProfile.name}
            </span>
          ) : null}
          {workspace ? (
            <span>
              {paneCount} pane{paneCount === 1 ? "" : "s"}
            </span>
          ) : null}
          {info ? <span className="uppercase">{info.platform}</span> : null}
        </div>
      </header>

      {error ? (
        <div className="flex shrink-0 items-center gap-2 border-b border-danger/40 bg-danger/10 px-3 py-1.5 text-[11px] text-danger">
          <span className="font-semibold">error</span>
          <span className="truncate">{error}</span>
          <button
            type="button"
            onClick={() => setError(null)}
            className="ml-auto rounded px-1.5 py-0.5 hover:bg-danger/20"
          >
            dismiss
          </button>
        </div>
      ) : null}

      <main className="min-h-0 flex-1 p-1.5">
        {workspace ? (
          <LayoutView
            node={workspace.layout}
            path={[]}
            focused={workspace.focused}
            canClose={canClose}
            profileFor={profileFor}
            agentFor={agentFor}
            formatPath={formatPath}
            onReady={onReady}
            onFocus={focus}
            onClose={(id) => void closePane(id)}
            onRatioCommit={(path, ratio) => void commitRatio(path, ratio)}
          />
        ) : (
          <div className="grid h-full place-items-center text-[12px] text-ink-faint">
            opening a shell…
          </div>
        )}
      </main>

      <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-line/70 bg-surface/40 px-3 text-[10px] text-ink-faint">
        <Hint chord="mod+shift+p" action="commands" />
        <Hint chord="mod+shift+e" action="split right" />
        <Hint chord="mod+shift+o" action="split down" />
        <Hint chord="mod+shift+w" action="close pane" />
        <Hint chord="mod+tab" action="next pane" />
        <Hint chord="mod+shift+up" action="previous prompt" />
        <Hint chord="mod+shift+down" action="next prompt" />
        {sessionFile ? (
          <span
            className="ml-auto truncate"
            title={`Layout and profiles are stored in ${sessionFile}`}
          >
            session saved
          </span>
        ) : (
          <span className="ml-auto">local-first · no account · no server</span>
        )}
      </footer>

      <CommandPalette
        open={paletteOpen}
        commands={commands}
        taskFor={pendingAgent}
        onSubmitTask={(task) => {
          const agent = pendingAgent;
          setPendingAgent(null);
          setPaletteOpen(false);
          if (agent) void spawnAgent(agent, task);
        }}
        onClose={() => {
          setPaletteOpen(false);
          setPendingAgent(null);
        }}
      />
    </div>
  );
}

interface ToolButtonProps {
  label: string;
  shortcut: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}

function ToolButton({
  label,
  shortcut,
  disabled,
  onClick,
  children,
}: ToolButtonProps) {
  return (
    <button
      type="button"
      title={`${label} (${shortcut})`}
      aria-label={label}
      disabled={disabled ?? false}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-md text-ink-muted transition-colors hover:bg-overlay hover:text-ink disabled:pointer-events-none disabled:opacity-35"
    >
      <svg
        viewBox="0 0 13 12"
        className="size-3.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinecap="round"
        aria-hidden
      >
        {children}
      </svg>
    </button>
  );
}

function Hint({ chord, action }: { chord: string; action: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="rounded border border-line bg-raised px-1.5 py-px text-[9px] text-ink-muted">
        {formatChord(chord)}
      </kbd>
      <span>{action}</span>
    </span>
  );
}

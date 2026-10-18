import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  agentsAvailable,
  appInfo,
  auditPath,
  paneClose,
  paneCwd,
  paneIds,
  paneList,
  roomFollow,
  roomState,
  sessionLoad,
  sessionPath,
  sessionSave,
  teamAdvance,
  teamConfirm,
  teamPlan,
  teamsList,
  teamStart,
  teamStatus,
  workspaceClosePane,
  workspaceCycleFocus,
  workspaceInit,
  workspaceSetFocus,
  workspaceSetRatio,
  workspaceSplit,
  type AgentInfo,
  type AppInfo,
  type Localised,
  type Profile,
  type RoomView,
  type Session,
  type SplitDirection,
  type StartedMember,
  type TeamPreset,
  type TeamStatus,
  type Workspace,
} from "./lib/ipc";
import { chordForCommand, commandForEvent, formatChord } from "./lib/keybindings";
import { shortenPath } from "./lib/paths";
import { describeResult, openPaneAgent, type PaneAgent } from "./lib/agents";
import { displayPaneTitle, paneRows, type PaneLive } from "./lib/panes";
import { sameRoomView } from "./lib/room";
import type { Command } from "./lib/commands";
import { LayoutView } from "./components/LayoutView";
import { CommandPalette } from "./components/CommandPalette";
import { ProfileEditor } from "./components/ProfileEditor";
import { PluginsPanel } from "./components/PluginsPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { ControlRoom } from "./components/ControlRoom";
import { TeamPlanPanel } from "./components/TeamPlanPanel";
import { TeamRunPanel } from "./components/TeamRunPanel";
import { Hint } from "./components/Chrome";
import type { PaneController } from "./components/TerminalPane";

/** Must match `hp_core::SESSION_VERSION`. */
const SESSION_VERSION = 1;
const SAVE_DEBOUNCE_MS = 400;

/** English first, then whatever language the preset has. */
function localisedTitle(text: Localised): string {
  return text["en"] ?? Object.values(text)[0] ?? "";
}

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
  /**
   * What each pane's view last reported.
   *
   * The rail and the tab strip describe the same panes the grid draws, and the
   * grid is the only thing that knows what a pane's shell said. This is that
   * answer, lifted rather than asked for twice.
   */
  const [paneLive, setPaneLive] = useState<Record<string, PaneLive>>({});
  /**
   * The room, as the app last saw it.
   *
   * Polled here rather than inside the dock: the top bar chip and the rail show
   * the room whether or not the dock is on screen, and three pollers would be
   * three answers to one question.
   */
  const [room, setRoom] = useState<RoomView | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** Set while the profile editor is on screen. */
  const [profilesOpen, setProfilesOpen] = useState(false);
  /** Set while the plugin panel is on screen. */
  const [pluginsOpen, setPluginsOpen] = useState(false);
  /** Set while the settings panel is on screen. */
  const [settingsOpen, setSettingsOpen] = useState(false);
  /** Whether the left rail is on screen. */
  const [sidebarOpen, setSidebarOpen] = useState(true);
  /**
   * Whether the control room dock is on screen. Closed by default: a column
   * that says "not in a room" all day is space taken from the terminal.
   */
  const [roomOpen, setRoomOpen] = useState(false);
  /**
   * Set while the palette is asking for a task. The worktree flag is decided
   * when the agent is picked, because it is part of which entry was picked.
   */
  const [pendingAgent, setPendingAgent] = useState<{
    agent: AgentInfo;
    worktree: boolean;
  } | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [teams, setTeams] = useState<TeamPreset[]>([]);
  /** Set while the plan for a team is on screen. */
  const [teamKey, setTeamKey] = useState<string | null>(null);
  /** Where the team run in progress is, refreshed while it goes. */
  const [teamRun, setTeamRun] = useState<TeamStatus | null>(null);
  /** Set while the timeline of the run is on screen. */
  const [showRun, setShowRun] = useState(false);
  const [sessionFile, setSessionFile] = useState<string | null>(null);
  const [auditFile, setAuditFile] = useState<string | null>(null);

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

      const log = await auditPath().catch(() => null);
      if (!cancelled) setAuditFile(log);

      const loadedTeams = await teamsList().catch(() => []);
      if (!cancelled) setTeams(loadedTeams);

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

  // The room changes without this instance asking: someone joins, types, or the
  // transcript grows. One second is faster than a person reads and slower than
  // a poll that costs anything. An unchanged answer is dropped here so the
  // whole shell is not redrawn once a second for nothing.
  useEffect(() => {
    let cancelled = false;
    const read = () => {
      void roomState()
        .then((next) => {
          if (!cancelled) {
            setRoom((previous) => (sameRoomView(previous, next) ? previous : next));
          }
        })
        .catch(() => undefined);
    };
    read();
    const timer = window.setInterval(read, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

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
    setPaneLive((map) => {
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
    async (agent: AgentInfo, task: string, useWorktree: boolean) => {
      const current = workspaceRef.current;
      if (!current) return;
      const target = current.focused ?? paneIds(current.layout)[0];
      if (!target) return;

      const id = `p${nextPane.current++}`;
      try {
        // The pane the person is looking at is the one with a repository in it,
        // and its shell has already told us where that is. A worktree with no
        // repository is refused by the app, with a sentence that says so.
        const repo = useWorktree ? await paneCwd(target) : null;
        const prepared = await openPaneAgent(id, agent, task, repo, useWorktree);
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

  /** Register the panes a wave opened and lay them out beside the person. */
  const placeWave = useCallback(
    async (members: StartedMember[]) => {
      const current = workspaceRef.current;
      if (!current) return;
      const target = current.focused ?? paneIds(current.layout)[0];
      if (!target) return;

      setPaneAgents((map) => {
        const next = { ...map };
        for (const member of members) {
          next[member.pane] = {
            id: member.agent,
            label: member.label,
            program: member.program,
            args: member.args,
            needsPaste: member.needsPaste,
            cwd: member.cwd,
            branch: member.branch,
          };
        }
        return next;
      });

      // Every member splits from the pane the person is in, so the wave lands
      // beside it rather than nesting into the last member opened.
      let workspace = current;
      for (const member of members) {
        workspace = await workspaceSplit(target, "row", member.pane);
      }
      setWorkspace(workspace);
    },
    [],
  );

  /**
   * Open the first wave of a team.
   *
   * The frontend works out how many panes the wave needs from the same plan
   * the person just read, allocates the ids, and hands them over. The app
   * checks the count and refuses a mismatch rather than opening part of a wave.
   */
  const startTeam = useCallback(async (key: string, task: string) => {
    const current = workspaceRef.current;
    if (!current) return;
    const target = current.focused ?? paneIds(current.layout)[0];
    if (!target) return;

    try {
      const plan = await teamPlan(key);
      const spawnable = plan.waves
        .flatMap((wave) => wave.members)
        .filter((member) => member.kind !== "leader");
      if (spawnable.length === 0) {
        throw new Error("this team has nothing to spawn: every role is a leader");
      }

      const firstWave = Math.min(...spawnable.map((member) => member.wave));
      const waveMembers = spawnable.filter((member) => member.wave === firstWave);
      const ids = waveMembers.map(() => `p${nextPane.current++}`);

      const started = await teamStart({
        key,
        task,
        panes: ids,
        repo: await paneCwd(target),
      });

      await placeWave(started.members);
      setTeamRun(await teamStatus());
      setTeamKey(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, [placeWave]);

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

  /**
   * Take a pane's report of itself.
   *
   * Identical reports are dropped rather than stored: a pane that polls its own
   * run state once a second would otherwise redraw the entire shell once a
   * second to say the same thing.
   */
  const onLive = useCallback((id: string, live: PaneLive | null) => {
    setPaneLive((map) => {
      if (live === null) {
        if (!(id in map)) return map;
        const remaining = { ...map };
        delete remaining[id];
        return remaining;
      }
      const previous = map[id];
      if (
        previous &&
        previous.snapshot === live.snapshot &&
        previous.phase === live.phase
      ) {
        return map;
      }
      return { ...map, [id]: live };
    });
  }, []);

  const withFocusedPane = useCallback(
    (action: (controller: PaneController) => void) => {
      const focused = workspaceRef.current?.focused;
      if (!focused) return;
      const controller = controllers.current.get(focused);
      if (controller) action(controller);
    },
    [],
  );

  /** One poll of the run, kept out of the effect so it can be awaited. */
  const advancing = useRef(false);
  const pollTeam = useCallback(async () => {
    // Advancing takes a round trip, so a poll could overlap the previous one
    // and ask to open the same wave twice. The app refuses the second, but
    // not asking is simpler than handling a refusal that means nothing.
    if (advancing.current) return;
    advancing.current = true;
    try {
      const status = await teamStatus();
      setTeamRun(status);
      // A finished run takes its view with it. Leaving the flag up would open
      // a timeline nobody asked for the moment the next run starts.
      if (!status) setShowRun(false);
      if (!status || status.holding || status.running > 0) return;

      // The last advance is the one that reports `finished` or raises a hold,
      // so it is asked for like any other. `nextWavePanes` is zero when there
      // is nothing left, and the app never answers that with a wave to open.
      const ids = Array.from(
        { length: status.nextWavePanes },
        () => `p${nextPane.current++}`,
      );
      const outcome = await teamAdvance(ids);
      if (outcome.kind === "ready") {
        await placeWave(outcome.members);
      }
      setTeamRun(await teamStatus());
    } catch (cause) {
      setError(String(cause));
    } finally {
      advancing.current = false;
    }
  }, [placeWave]);

  useEffect(() => {
    const timer = window.setInterval(() => void pollTeam(), 2000);
    return () => window.clearInterval(timer);
  }, [pollTeam]);

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
        id: "app.profiles",
        title: "Profiles…",
        group: "App",
        hint: hint("app.profiles"),
        run: () => setProfilesOpen(true),
      },
      {
        id: "app.plugins",
        title: "Plugins…",
        group: "App",
        hint: null,
        run: () => setPluginsOpen(true),
      },
      {
        id: "app.settings",
        title: "Settings…",
        group: "App",
        hint: null,
        run: () => setSettingsOpen(true),
      },
      {
        id: "view.controlRoom",
        title: "Toggle the control room",
        group: "View",
        hint: null,
        run: () => setRoomOpen((open) => !open),
      },
      {
        id: "view.sidebar",
        title: "Toggle the sidebar",
        group: "View",
        hint: null,
        run: () => setSidebarOpen((open) => !open),
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
        run: () => setPendingAgent({ agent, worktree: false }),
      });
      list.push({
        id: `agent.worktree.${agent.id}`,
        title: `Run ${agent.label} in a worktree…`,
        group: "Agent",
        hint: "git",
        keepOpen: true,
        run: () => setPendingAgent({ agent, worktree: true }),
      });
    }

    for (const team of teams) {
      list.push({
        id: `team.plan.${team.key}`,
        title: `Team: ${localisedTitle(team.title)}`,
        group: "Team",
        hint: team.group ?? null,
        run: () => setTeamKey(team.key),
      });
    }

    if (teamRun) {
      list.push({
        id: "team.showRun",
        // Verb first, like every other command, and a prefix no other command
        // shares: "team" alone still finds the presets, not this.
        title: `Open team run: ${teamRun.title}`,
        group: "Team",
        hint:
          teamRun.running > 0
            ? `${teamRun.running} running`
            : teamRun.holding
              ? "waiting on you"
              : "idle",
        run: () => setShowRun(true),
      });
    }

    return list;
  }, [profiles, agents, teams, teamRun, splitWith, closePane, cycle, withFocusedPane]);

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

  const confirmTeam = useCallback(async () => {
    try {
      await teamConfirm();
      setTeamRun(await teamStatus());
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  /** Follow, or stop following, whoever the rail was clicked on. */
  const follow = useCallback((who: string | null) => {
    void roomFollow(who)
      .then((next) => setRoom(next))
      .catch((cause: unknown) => setError(String(cause)));
  }, []);

  /** What to call a pane, for the rail, the tabs and the control room. */
  const labelFor = useCallback(
    (id: string) => {
      const fallback = paneAgents[id]?.label ?? profileFor(id)?.name ?? "shell";
      return displayPaneTitle(paneLive[id]?.snapshot.title ?? "", fallback);
    },
    [paneAgents, paneLive, profileFor],
  );

  const rows = useMemo(() => {
    const ids = workspace ? paneIds(workspace.layout) : [];
    return paneRows({
      ids,
      live: paneLive,
      focused: workspace?.focused ?? null,
      fallback: labelFor,
    });
  }, [workspace, paneLive, labelFor]);

  const paneCount = rows.length;
  const canClose = paneCount > 1;
  const holdingClaims =
    teamRun?.claims.filter((claim) => claim.wave === teamRun.wave) ?? [];
  const activeProfile = profiles.find(
    (profile) => profile.id === (defaultProfile ?? ""),
  );

  return (
    <div className="flex h-full w-full flex-col bg-canvas">
      <TopBar
        workspace={workspace}
        panes={rows}
        info={info}
        profileName={activeProfile?.name ?? null}
        canClose={canClose}
        teamRun={teamRun}
        room={room}
        roomOpen={roomOpen}
        onFocusPane={focus}
        onClosePane={(id) => void closePane(id)}
        onSplit={(direction) => void splitWith(direction, null)}
        onCloseFocused={() => {
          if (workspace?.focused) void closePane(workspace.focused);
        }}
        onOpenProfiles={() => setProfilesOpen(true)}
        onOpenRun={() => setShowRun(true)}
        onToggleRoom={() => setRoomOpen((open) => !open)}
        onOpenPalette={() => setPaletteOpen(true)}
      />

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

      {teamRun?.holding ? (
        <div className="flex shrink-0 items-start gap-3 border-b border-warn/40 bg-warn/10 px-3 py-1.5 text-[11px] text-warn">
          <span className="shrink-0 font-semibold">{teamRun.title}</span>
          <div className="min-w-0 flex-1">
            <div className="truncate">{teamRun.holdingMessage}</div>
            {holdingClaims.length > 0 ? (
              <ul className="mt-1 flex flex-col gap-0.5">
                {holdingClaims.map((claim) => (
                  <li key={claim.pane} className="text-[10px] text-ink-muted">
                    <span
                      title={
                        claim.result
                          ? `${claim.agent} reported${claim.result.files.length > 0 ? `: ${claim.result.files.join(", ")}` : " no files"}`
                          : claim.finished
                            ? `${claim.agent} exited without printing a RESULT line`
                            : `${claim.agent} is still going`
                      }
                    >
                      <span className="text-accent">{claim.label}</span>
                      {" · "}
                      {claim.result
                        ? describeResult(claim.result)
                        : claim.finished
                          ? "no report"
                          : "still going"}
                    </span>
                    {claim.findings ? (
                      <pre className="mt-0.5 max-h-28 overflow-auto rounded border border-line/60 bg-base/60 px-2 py-1 font-mono text-[10px] whitespace-pre-wrap text-ink-faint">
                        {claim.findings}
                      </pre>
                    ) : null}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
          <button
            type="button"
            onClick={() => setShowRun(true)}
            className="shrink-0 rounded border border-warn/50 px-2 py-0.5 transition-colors hover:bg-warn/20"
          >
            timeline
          </button>
          <button
            type="button"
            onClick={() => void confirmTeam()}
            className="shrink-0 rounded border border-warn/50 px-2 py-0.5 transition-colors hover:bg-warn/20"
          >
            continue
          </button>
        </div>
      ) : null}

      {/*
        A column that appears and disappears stays in the same child slot, so
        the grid in the middle is never unmounted by a toggle. Unmounting it
        would take every terminal with it and open the shells again.
      */}
      <div className="flex min-h-0 flex-1">
        {sidebarOpen ? (
          <Sidebar
            workspace={workspace}
            panes={rows}
            canClose={canClose}
            room={room}
            paneLabel={labelFor}
            onFocusPane={focus}
            onClosePane={(id) => void closePane(id)}
            onSplit={(direction) => void splitWith(direction, null)}
            onFollow={follow}
            onOpenRoom={() => setRoomOpen(true)}
            onOpenProfiles={() => setProfilesOpen(true)}
            onOpenSettings={() => setSettingsOpen(true)}
            onHide={() => setSidebarOpen(false)}
          />
        ) : null}

        <main className="min-h-0 min-w-0 flex-1 p-1">
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
              onLive={onLive}
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

        {roomOpen ? (
          <ControlRoom
            view={room}
            focusedPane={workspace?.focused ?? null}
            paneLabel={labelFor}
            onView={setRoom}
            onHide={() => setRoomOpen(false)}
          />
        ) : null}
      </div>

      <footer className="flex h-7 shrink-0 items-center gap-3 border-t border-line/60 bg-surface/40 px-3 text-[10px] text-ink-faint">
        {/*
          The hints give way before the toggles do: a window too narrow for both
          drops the least urgent chords rather than pushing the status bar wider
          than the window. Every chord is still in the palette.
        */}
        <div className="flex min-w-0 items-center gap-3 overflow-hidden">
          <Hint chord="mod+shift+p" action="commands" />
          <Hint chord="mod+shift+e" action="split right" />
          <Hint chord="mod+shift+w" action="close pane" />
          <Hint chord="mod+tab" action="next pane" className="max-lg:hidden" />
          <Hint
            chord="mod+shift+up"
            action="previous prompt"
            className="max-xl:hidden"
          />
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {info ? (
            <span title={`This machine reports itself as ${info.platform}`}>
              <span className="uppercase">{info.platform}</span>
            </span>
          ) : null}
          <PanelToggle
            label="sidebar"
            open={sidebarOpen}
            onClick={() => setSidebarOpen((open) => !open)}
          />
          <PanelToggle
            label="control room"
            open={roomOpen}
            onClick={() => setRoomOpen((open) => !open)}
          />
          {sessionFile ? (
            <span
              className="max-w-64 truncate"
              title={[
                `Layout and profiles: ${sessionFile}`,
                auditFile ? `Activity log: ${auditFile}` : null,
              ]
                .filter(Boolean)
                .join("\n")}
            >
              session saved
            </span>
          ) : (
            <span>local-first · no account · no server</span>
          )}
        </div>
      </footer>

      {teamKey ? (
        <TeamPlanPanel
          teamKey={teamKey}
          onStart={(task) => void startTeam(teamKey, task)}
          onClose={() => setTeamKey(null)}
        />
      ) : null}

      {showRun && teamRun ? (
        <TeamRunPanel
          teamKey={teamRun.key}
          status={teamRun}
          onClose={() => setShowRun(false)}
        />
      ) : null}

      {pluginsOpen ? (
        <PluginsPanel onClose={() => setPluginsOpen(false)} />
      ) : null}

      {settingsOpen ? (
        <SettingsPanel
          info={info}
          sessionFile={sessionFile}
          auditFile={auditFile}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}

      {profilesOpen ? (
        <ProfileEditor
          profiles={profiles}
          defaultProfile={defaultProfile}
          platformDefaultShell={info?.defaultShell ?? null}
          onChange={setProfiles}
          onDefaultChange={setDefaultProfile}
          onOpen={(id) => {
            setProfilesOpen(false);
            void splitWith("row", id);
          }}
          onClose={() => setProfilesOpen(false)}
        />
      ) : null}

      <CommandPalette
        open={paletteOpen}
        commands={commands}
        taskFor={pendingAgent ? pendingAgent.agent : null}
        onSubmitTask={(task) => {
          const pending = pendingAgent;
          setPendingAgent(null);
          setPaletteOpen(false);
          if (pending) void spawnAgent(pending.agent, task, pending.worktree);
        }}
        onClose={() => {
          setPaletteOpen(false);
          setPendingAgent(null);
        }}
      />
    </div>
  );
}

/**
 * The status bar's way back to a column that was hidden.
 *
 * A toggle rather than a one-way hide: a hidden rail whose only way back is the
 * palette is a rail most people would not find again.
 */
function PanelToggle({
  label,
  open,
  onClick,
}: {
  label: string;
  open: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={open}
      title={`${open ? "Hide" : "Show"} the ${label}`}
      onClick={onClick}
      className={`flex shrink-0 items-center gap-1.5 rounded px-1 py-0.5 transition-colors hover:bg-overlay hover:text-ink ${
        open ? "text-ink-muted" : "text-ink-faint"
      }`}
    >
      <span
        aria-hidden
        className={`size-1.5 rounded-full ${open ? "bg-accent" : "bg-line-strong"}`}
      />
      {label}
    </button>
  );
}

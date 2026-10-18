import { useEffect, useRef, useState } from "react";
import { formatDuration } from "../lib/format";
import { describeResult, type PaneAgent } from "../lib/agents";
import {
  detachPaneRuntime,
  openPaneRuntime,
  type PaneRuntime,
  type PaneSnapshot,
} from "../lib/paneRuntime";
import {
  displayPaneTitle,
  paneTone,
  TONE_LABEL,
  type PaneLive,
} from "../lib/panes";
import { Chip, CloseButton, PaneGlyph } from "./Chrome";
import {
  paneRunState,
  type PaneRunState,
  type Profile,
  type RunPhase,
} from "../lib/ipc";

/**
 * What the rest of the app may ask of one pane.
 *
 * The terminal instance outlives this component, so anything that needs to act
 * on it is handed back through here rather than reaching into the DOM.
 */
export interface PaneController {
  focus: () => void;
  /** Scrolls to the previous prompt. Returns false when there is none. */
  jumpToPreviousPrompt: () => boolean;
  /** Scrolls to the next prompt. Returns false when there is none. */
  jumpToNextPrompt: () => boolean;
}

interface TerminalPaneProps {
  id: string;
  focused: boolean;
  canClose: boolean;
  /** Profile this pane opens with, or `null` for the platform default shell. */
  profile: Profile | null;
  /** The agent this pane runs, or `null` for a plain shell. */
  agent: PaneAgent | null;
  /** Shortens a filesystem path for the header. */
  formatPath: (path: string) => string;
  /** Publishes this pane's controller, or `null` when it goes away. */
  onReady: (id: string, controller: PaneController | null) => void;
  /**
   * Publishes what this pane is doing, for the rail and the tab strip, or
   * `null` when the view goes away. The header draws the same state; this is
   * what lets the shell outside the pane say it too.
   */
  onLive: (id: string, live: PaneLive | null) => void;
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
}

/**
 * What an agent pane is doing, from the app's own observation.
 *
 * `quiet` is not `idle` and never `blocked`: a thinking agent, a waiting one
 * and a finished one that forgot to report all look the same from outside a
 * terminal. The header says what is known and stops there.
 */
const PHASE_LABEL: Record<RunPhase, string> = {
  starting: "starting",
  working: "working",
  quiet: "quiet",
  reported: "reported",
  exited: "exited",
  failed: "failed",
};

export function TerminalPane({
  id,
  focused,
  canClose,
  profile,
  agent,
  formatPath,
  onReady,
  onLive,
  onFocus,
  onClose,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const runtimeRef = useRef<PaneRuntime | null>(null);
  const [view, setView] = useState<PaneSnapshot>({
    status: "starting",
    title: "",
    cwd: null,
    exitCode: null,
    durationMs: null,
    running: false,
  });
  /** What an agent pane has reported about itself, when it runs one. */
  const [runState, setRunState] = useState<PaneRunState | null>(null);

  // The profile is read once, when the pane opens. Editing a profile must not
  // kill a shell that is already running, so this is captured at mount rather
  // than tracked.
  const openProfile = useRef(profile);
  const openAgent = useRef(agent);

  // Held in a ref so that publishing a pane's state never re-runs the effect
  // that owns the terminal: a new callback identity from the app would
  // otherwise detach and re-attach a live shell.
  const report = useRef(onLive);
  useEffect(() => {
    report.current = onLive;
  }, [onLive]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const chosen = openProfile.current;
    const agentPane = openAgent.current;

    // A second mount is not a second shell: this returns the terminal and the
    // PTY that already exist when the pane merely moved inside the layout.
    const runtime = openPaneRuntime({
      id,
      host,
      spec: (cols, rows) =>
        agentPane
          ? {
              id,
              cols,
              rows,
              shell: agentPane.program,
              args: agentPane.args,
              // A worktree run is confined to its own checkout; without one it
              // starts where the profile says.
              cwd: agentPane.cwd ?? chosen?.cwd ?? null,
              env: chosen?.env ?? {},
              // An agent CLI is not a shell. Prompt hooks typed into it would
              // be read as part of the conversation and confuse it.
              integration: false,
              agent: agentPane.id,
            }
          : {
              id,
              cols,
              rows,
              shell: chosen?.shell ?? null,
              args: chosen?.args ?? [],
              cwd: chosen?.cwd ?? null,
              env: chosen?.env ?? {},
              integration: chosen?.integration ?? true,
            },
    });
    runtimeRef.current = runtime;

    const unsubscribe = runtime.subscribe(setView);
    onReady(id, {
      focus: () => runtime.focus(),
      jumpToPreviousPrompt: () => runtime.jumpToPreviousPrompt(),
      jumpToNextPrompt: () => runtime.jumpToNextPrompt(),
    });

    return () => {
      unsubscribe();
      onReady(id, null);
      runtimeRef.current = null;
      // Detach, and let the layout decide whether the shell is finished: an
      // unmount is a split as often as it is a close.
      void detachPaneRuntime(id);
    };
  }, [id, onReady]);

  useEffect(() => {
    if (focused) runtimeRef.current?.focus();
  }, [focused]);

  // An agent pane reports on itself, and the app reads that report out of the
  // pane's own output. A second between reads is enough for a header and costs
  // nothing next to the terminal stream itself.
  useEffect(() => {
    if (!agent) return;
    let cancelled = false;

    const read = () => {
      void paneRunState(id)
        .then((state) => {
          if (!cancelled) setRunState(state);
        })
        .catch(() => undefined);
    };

    read();
    const timer = window.setInterval(read, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [id, agent]);

  // Tell the shell what this pane is doing, and stop telling it once the view
  // is gone. The rail and the tab strip read this rather than polling again:
  // they are describing the same pane, not a second opinion about it.
  const phase = runState?.phase ?? null;
  useEffect(() => {
    report.current(id, { snapshot: view, phase });
  }, [id, view, phase]);

  useEffect(() => {
    return () => {
      report.current(id, null);
    };
  }, [id]);

  // ConPTY reports the program path as the window title until the shell sets
  // one of its own, so a title that is really a path tells a person nothing.
  // The profile name is more useful in that case.
  const displayTitle = displayPaneTitle(
    view.title,
    agent?.label ?? profile?.name ?? "shell",
  );
  const tone = paneTone({ snapshot: view, phase });
  const result = runState?.result ?? null;

  return (
    <section
      data-focused={focused}
      onMouseDownCapture={() => {
        if (!focused) onFocus(id);
      }}
      className="pane-shell pane-ring flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-[var(--radius-pane)] bg-base"
    >
      <header className="flex h-7 shrink-0 items-center gap-2 border-b border-line/60 bg-surface px-2">
        <PaneGlyph tone={tone} label={TONE_LABEL[tone]} />
        <span
          className="shrink-0 truncate text-[11px] text-ink"
          title={displayTitle}
        >
          {displayTitle}
        </span>
        {view.cwd ? (
          <span
            className="min-w-0 flex-1 truncate text-[10px] text-ink-faint"
            title={view.cwd}
          >
            {formatPath(view.cwd)}
          </span>
        ) : (
          <span className="flex-1" />
        )}
        {agent?.branch ? (
          <Chip
            tone="accent"
            title={`This run is confined to the worktree on ${agent.branch}`}
          >
            {"\u2387"} {agent.branch}
          </Chip>
        ) : null}
        {result ? (
          <Chip
            tone={
              result.status === "failed"
                ? "danger"
                : result.status === "blocked"
                  ? "warn"
                  : "ok"
            }
            title={
              result.files.length > 0
                ? `The child reports: ${result.files.join(", ")}`
                : "The child reported no files"
            }
          >
            {describeResult(result)}
          </Chip>
        ) : null}
        <span className="ml-1 flex shrink-0 items-center gap-2">
          {view.durationMs !== null && view.durationMs > 0 ? (
            <span
              className="text-[9px] text-ink-faint tabular-nums"
              title="How long the last command took"
            >
              {formatDuration(view.durationMs)}
            </span>
          ) : null}
          {view.exitCode !== null && view.exitCode !== 0 ? (
            <Chip
              tone="danger"
              title={`The last command exited with status ${view.exitCode}`}
            >
              exit {view.exitCode}
            </Chip>
          ) : null}
          <span
            className="text-[9px] tracking-wide text-ink-faint uppercase"
            title={
              agent && runState
                ? `What this run reported about itself; it has been going for ${runState.elapsedSeconds}s`
                : "What this pane's process is doing"
            }
          >
            {agent && runState ? PHASE_LABEL[runState.phase] : TONE_LABEL[tone]}
          </span>
        </span>
        {canClose ? (
          <span className="pane-chrome">
            <CloseButton
              label={`Close pane ${id}`}
              title="Close pane"
              onClick={() => onClose(id)}
            />
          </span>
        ) : null}
      </header>
      <div ref={hostRef} className="terminal-host min-h-0 flex-1" />
    </section>
  );
}

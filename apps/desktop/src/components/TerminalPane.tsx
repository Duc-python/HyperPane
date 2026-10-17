import { useEffect, useRef, useState } from "react";
import { formatDuration } from "../lib/format";
import { describeResult, type PaneAgent } from "../lib/agents";
import {
  detachPaneRuntime,
  openPaneRuntime,
  type PaneRuntime,
  type PaneSnapshot,
  type PaneStatus,
} from "../lib/paneRuntime";
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
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
}

const STATUS_DOT: Record<PaneStatus, string> = {
  starting: "bg-warn",
  live: "bg-ok",
  exited: "bg-ink-faint",
  failed: "bg-danger",
};

const STATUS_LABEL: Record<PaneStatus, string> = {
  starting: "starting",
  live: "live",
  exited: "exited",
  failed: "failed",
};

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
  });
  /** What an agent pane has reported about itself, when it runs one. */
  const [runState, setRunState] = useState<PaneRunState | null>(null);

  // The profile is read once, when the pane opens. Editing a profile must not
  // kill a shell that is already running, so this is captured at mount rather
  // than tracked.
  const openProfile = useRef(profile);
  const openAgent = useRef(agent);

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

  // ConPTY reports the program path as the window title until the shell sets
  // one of its own, so a title that is really a path tells a person nothing.
  // The profile name is more useful in that case.
  const rawTitle = view.title.trim();
  const titleIsProgramPath =
    rawTitle.includes("\\") ||
    /\.(exe|cmd|bat|com|ps1)$/i.test(rawTitle) ||
    /^\/[^\s]*$/.test(rawTitle);
  const displayTitle =
    rawTitle === "" || titleIsProgramPath
      ? (agent?.label ?? profile?.name ?? "shell")
      : rawTitle;

  const result = runState?.result ?? null;

  return (
    <section
      data-focused={focused}
      onMouseDownCapture={() => {
        if (!focused) onFocus(id);
      }}
      className="pane-shell pane-ring flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-[var(--radius-pane)] bg-base"
    >
      <header className="flex h-7 shrink-0 items-center gap-2 border-b border-line/60 bg-surface/70 px-2.5 backdrop-blur-sm">
        <span
          aria-hidden
          className={`size-1.5 shrink-0 rounded-full ${STATUS_DOT[view.status]}`}
        />
        <span className="shrink-0 truncate text-[11px] text-ink-muted" title={displayTitle}>
          {displayTitle}
        </span>
        {view.cwd ? (
          <span
            className="min-w-0 truncate text-[10px] text-ink-faint"
            title={view.cwd}
          >
            {formatPath(view.cwd)}
          </span>
        ) : null}
        {agent?.branch ? (
          <span
            className="shrink-0 rounded border border-line bg-raised px-1 text-[9px] text-ink-muted"
            title={`This run is confined to the worktree on ${agent.branch}`}
          >
            {agent.branch}
          </span>
        ) : null}
        {result ? (
          <span
            className={`shrink-0 rounded border px-1 text-[9px] ${
              result.status === "failed"
                ? "border-danger/50 bg-danger/10 text-danger"
                : result.status === "blocked"
                  ? "border-warn/50 bg-warn/10 text-warn"
                  : "border-ok/50 bg-ok/10 text-ok"
            }`}
            title={
              result.files.length > 0
                ? `The child reports: ${result.files.join(", ")}`
                : "The child reported no files"
            }
          >
            {describeResult(result)}
          </span>
        ) : null}
        <span className="ml-auto flex shrink-0 items-center gap-2">
          {view.durationMs !== null && view.durationMs > 0 ? (
            <span
              className="text-[9px] text-ink-faint tabular-nums"
              title="How long the last command took"
            >
              {formatDuration(view.durationMs)}
            </span>
          ) : null}
          {view.exitCode !== null && view.exitCode !== 0 ? (
            <span
              className="rounded border border-danger/50 bg-danger/10 px-1 text-[9px] text-danger"
              title={`The last command exited with status ${view.exitCode}`}
            >
              exit {view.exitCode}
            </span>
          ) : null}
          <span
            className="text-[10px] tracking-wide text-ink-faint uppercase"
            title={
              agent && runState
                ? `This run has been going for ${runState.elapsedSeconds}s`
                : undefined
            }
          >
            {agent && runState ? PHASE_LABEL[runState.phase] : STATUS_LABEL[view.status]}
          </span>
        </span>
        {canClose ? (
          <button
            type="button"
            title="Close pane"
            aria-label={`Close pane ${id}`}
            onClick={(event) => {
              event.stopPropagation();
              onClose(id);
            }}
            className="pane-chrome grid size-4 shrink-0 place-items-center rounded-[4px] text-ink-faint transition-colors hover:bg-overlay hover:text-ink focus-visible:opacity-100"
          >
            <svg viewBox="0 0 10 10" className="size-2.5" aria-hidden>
              <path
                d="M1 1l8 8M9 1l-8 8"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
            </svg>
          </button>
        ) : null}
      </header>
      <div ref={hostRef} className="terminal-host min-h-0 flex-1" />
    </section>
  );
}

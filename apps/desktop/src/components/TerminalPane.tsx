import { useEffect, useRef, useState } from "react";
import { Terminal, type IMarker, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { formatDuration } from "../lib/format";
import {
  paneClose,
  paneResize,
  paneSpawn,
  paneWrite,
  type PaneSpec,
  type Profile,
  type PtyEvent,
} from "../lib/ipc";

type Status = "starting" | "live" | "exited" | "failed";

/**
 * What the rest of the app may ask of one pane.
 *
 * The terminal instance lives inside this component, so anything that needs to
 * act on it is handed back through here rather than reaching into the DOM.
 */
export interface PaneController {
  focus: () => void;
  /** Scrolls to the previous prompt. Returns false when there is none. */
  jumpToPreviousPrompt: () => boolean;
  /** Scrolls to the next prompt. Returns false when there is none. */
  jumpToNextPrompt: () => boolean;
}

/** Prompt marks kept for jumping. Older ones are dropped. */
const MAX_PROMPT_MARKS = 500;

interface TerminalPaneProps {
  id: string;
  focused: boolean;
  canClose: boolean;
  /** Profile this pane opens with, or `null` for the platform default shell. */
  profile: Profile | null;
  /** Shortens a filesystem path for the header. */
  formatPath: (path: string) => string;
  /** Publishes this pane's controller, or `null` when it goes away. */
  onReady: (id: string, controller: PaneController | null) => void;
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
}

/**
 * xterm needs concrete colours, so this mirrors the OKLCH tokens in
 * `styles.css`. Keep the two in step when the palette changes.
 */
const THEME: ITheme = {
  background: "#0e1016",
  foreground: "#e6e9f0",
  cursor: "#56d4dd",
  cursorAccent: "#0e1016",
  selectionBackground: "#263043",
  black: "#1b1e27",
  red: "#ff6b81",
  green: "#7ee787",
  yellow: "#f2cc60",
  blue: "#79b8ff",
  magenta: "#d2a8ff",
  cyan: "#56d4dd",
  white: "#c9d1d9",
  brightBlack: "#4b5263",
  brightRed: "#ff8fa3",
  brightGreen: "#a5f3a0",
  brightYellow: "#ffd97d",
  brightBlue: "#a5d6ff",
  brightMagenta: "#e2c5ff",
  brightCyan: "#8ce8f0",
  brightWhite: "#f0f3f8",
};

const STATUS_DOT: Record<Status, string> = {
  starting: "bg-warn",
  live: "bg-ok",
  exited: "bg-ink-faint",
  failed: "bg-danger",
};

const STATUS_LABEL: Record<Status, string> = {
  starting: "starting",
  live: "live",
  exited: "exited",
  failed: "failed",
};

export function TerminalPane({
  id,
  focused,
  canClose,
  profile,
  formatPath,
  onReady,
  onFocus,
  onClose,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const [status, setStatus] = useState<Status>("starting");
  const [title, setTitle] = useState<string>("");
  const [cwd, setCwd] = useState<string | null>(null);
  /** Status of the last finished command, from OSC 133. */
  const [exitCode, setExitCode] = useState<number | null>(null);
  /**
   * How long the last command took.
   *
   * Only shells that mark the start of a command report this. PowerShell does
   * not, so this stays empty there rather than showing the time a person spent
   * typing as if it were the command's own.
   */
  const [durationMs, setDurationMs] = useState<number | null>(null);

  // The profile is read once, when the pane opens. Editing a profile must not
  // kill a shell that is already running, so this is captured at mount rather
  // than tracked.
  const openProfile = useRef(profile);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      fontFamily:
        '"JetBrains Mono", "Cascadia Mono", "SF Mono", ui-monospace, Menlo, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.25,
      cursorBlink: true,
      cursorStyle: "bar",
      scrollback: 10_000,
      macOptionIsMeta: true,
      allowProposedApi: true,
      theme: THEME,
    });
    termRef.current = term;

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);

    // WebGL is a large win but is not available on every driver; the canvas
    // renderer is a correct fallback, so a failure here is not an error.
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      /* keep the canvas renderer */
    }

    const safeFit = () => {
      try {
        fit.fit();
      } catch {
        /* the host has no size yet */
      }
    };
    safeFit();

    let disposed = false;

    // Prompt positions for jumping. Markers move with the buffer, so the line
    // numbers are read at the time of a jump rather than stored.
    const promptMarks: IMarker[] = [];
    let commandStartedAt: number | null = null;

    const liveMarks = () => {
      for (let index = promptMarks.length - 1; index >= 0; index -= 1) {
        if (promptMarks[index]?.isDisposed) promptMarks.splice(index, 1);
      }
      return promptMarks;
    };

    const chosen = openProfile.current;
    const spec: PaneSpec = {
      id,
      cols: term.cols,
      rows: term.rows,
      shell: chosen?.shell ?? null,
      args: chosen?.args ?? [],
      cwd: chosen?.cwd ?? null,
      env: chosen?.env ?? {},
      integration: chosen?.integration ?? true,
    };
    void paneSpawn(spec, (event: PtyEvent) => {
      if (disposed) return;
      switch (event.kind) {
        case "data":
          term.write(event.chunk);
          break;
        case "exit": {
          setStatus("exited");
          const suffix =
            event.code === null ? "" : ` with code ${event.code}`;
          term.write(
            `\r\n\x1b[38;5;245m[process exited${suffix}]\x1b[0m\r\n`,
          );
          break;
        }
        case "error":
          setStatus("failed");
          term.write(`\r\n\x1b[38;5;203m[${event.message}]\x1b[0m\r\n`);
          break;
        case "cwd":
          setCwd(event.path);
          break;
        case "prompt":
          switch (event.mark) {
            case "promptStart": {
              const marker = term.registerMarker(0);
              if (marker) {
                promptMarks.push(marker);
                if (promptMarks.length > MAX_PROMPT_MARKS) {
                  promptMarks.shift()?.dispose();
                }
              }
              break;
            }
            case "commandStart":
              commandStartedAt = performance.now();
              setDurationMs(null);
              break;
            case "commandEnd": {
              const started = commandStartedAt;
              commandStartedAt = null;
              setExitCode(event.exitCode ?? null);
              setDurationMs(started === null ? null : performance.now() - started);
              break;
            }
            default:
              break;
          }
          break;
      }
    })
      .then(() => setStatus((current) => (current === "starting" ? "live" : current)))
      .catch((error: unknown) => {
        if (disposed) return;
        setStatus("failed");
        term.write(
          `\r\n\x1b[38;5;203m[could not open a pane: ${String(error)}]\x1b[0m\r\n`,
        );
      });

    const dataSub = term.onData((data) => {
      void paneWrite(id, data).catch(() => undefined);
    });
    const titleSub = term.onTitleChange((value) => {
      if (value.trim()) setTitle(value.trim());
    });

    // The PTY has to hear about geometry changes or full-screen programs wrap
    // their output to the old width.
    const observer = new ResizeObserver(() => {
      safeFit();
      void paneResize(id, term.cols, term.rows).catch(() => undefined);
    });
    observer.observe(host);

    onReady(id, {
      focus: () => term.focus(),
      jumpToPreviousPrompt: () => {
        const here = term.buffer.active.viewportY;
        const above = liveMarks()
          .map((marker) => marker.line)
          .filter((line) => line < here);
        if (above.length === 0) return false;
        term.scrollToLine(Math.max(...above));
        return true;
      },
      jumpToNextPrompt: () => {
        const here = term.buffer.active.viewportY;
        const below = liveMarks()
          .map((marker) => marker.line)
          .filter((line) => line > here);
        if (below.length === 0) return false;
        term.scrollToLine(Math.min(...below));
        return true;
      },
    });

    return () => {
      disposed = true;
      onReady(id, null);
      observer.disconnect();
      dataSub.dispose();
      titleSub.dispose();
      void paneClose(id).catch(() => undefined);
      term.dispose();
      termRef.current = null;
    };
  }, [id, onReady]);

  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused]);

  // ConPTY reports the program path as the window title until the shell sets
  // one of its own, so a title that is really a path tells a person nothing.
  // The profile name is more useful in that case.
  const rawTitle = title.trim();
  const titleIsProgramPath =
    rawTitle.includes("\\") ||
    /\.(exe|cmd|bat|com|ps1)$/i.test(rawTitle) ||
    /^\/[^\s]*$/.test(rawTitle);
  const displayTitle =
    rawTitle === "" || titleIsProgramPath ? (profile?.name ?? "shell") : rawTitle;

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
          className={`size-1.5 shrink-0 rounded-full ${STATUS_DOT[status]}`}
        />
        <span className="shrink-0 truncate text-[11px] text-ink-muted" title={displayTitle}>
          {displayTitle}
        </span>
        {cwd ? (
          <span
            className="min-w-0 truncate text-[10px] text-ink-faint"
            title={cwd}
          >
            {formatPath(cwd)}
          </span>
        ) : null}
        <span className="ml-auto flex shrink-0 items-center gap-2">
          {durationMs !== null && durationMs > 0 ? (
            <span
              className="text-[9px] text-ink-faint tabular-nums"
              title="How long the last command took"
            >
              {formatDuration(durationMs)}
            </span>
          ) : null}
          {exitCode !== null && exitCode !== 0 ? (
            <span
              className="rounded border border-danger/50 bg-danger/10 px-1 text-[9px] text-danger"
              title={`The last command exited with status ${exitCode}`}
            >
              exit {exitCode}
            </span>
          ) : null}
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            {STATUS_LABEL[status]}
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

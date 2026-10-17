import { useEffect, useRef, useState } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  paneClose,
  paneResize,
  paneSpawn,
  paneWrite,
  type PaneSpec,
  type PtyEvent,
} from "../lib/ipc";

type Status = "starting" | "live" | "exited" | "failed";

interface TerminalPaneProps {
  id: string;
  focused: boolean;
  canClose: boolean;
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
  onFocus,
  onClose,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const [status, setStatus] = useState<Status>("starting");
  const [title, setTitle] = useState<string>("shell");

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

    const spec: PaneSpec = { id, cols: term.cols, rows: term.rows };
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

    return () => {
      disposed = true;
      observer.disconnect();
      dataSub.dispose();
      titleSub.dispose();
      void paneClose(id).catch(() => undefined);
      term.dispose();
      termRef.current = null;
    };
  }, [id]);

  useEffect(() => {
    if (focused) termRef.current?.focus();
  }, [focused]);

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
        <span className="truncate text-[11px] text-ink-muted">{title}</span>
        <span className="ml-auto shrink-0 text-[10px] tracking-wide text-ink-faint uppercase">
          {STATUS_LABEL[status]}
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

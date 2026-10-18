import { Terminal, type IMarker, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  containsPane,
  paneClose,
  paneList,
  paneResize,
  paneSpawn,
  paneWrite,
  workspaceGet,
  type PaneSpec,
  type PtyEvent,
} from "./ipc";

/**
 * A pane's terminal, kept alive between mounts.
 *
 * React identifies a component by its position in the element tree, and the
 * element tree here mirrors the layout tree. Splitting a pane therefore moves
 * the pane that was already on screen one level deeper, which React sees as an
 * unmount followed by a mount of a different component. Anything owned by that
 * component - the terminal, the PTY, the scrollback - would be destroyed and
 * rebuilt, and splitting one pane would kill the shell beside it.
 *
 * So a pane's session no longer belongs to its view. It is created on the first
 * mount and survives every later one; the view borrows it, and only the app's
 * own layout can say when the pane is really gone.
 */

export type PaneStatus = "starting" | "live" | "exited" | "failed";

/** Everything a pane's view needs to draw its header and its terminal. */
export interface PaneSnapshot {
  status: PaneStatus;
  title: string;
  cwd: string | null;
  exitCode: number | null;
  durationMs: number | null;
  /**
   * Whether a command is running right now, from the shell's own prompt marks.
   *
   * Only a shell with the generated hooks reports this, so `false` means "no
   * command is known to be running" rather than "nothing is happening". It is
   * what lets a chip be amber for a build instead of green while one runs.
   */
  running: boolean;
}

const EMPTY: PaneSnapshot = {
  status: "starting",
  title: "",
  cwd: null,
  exitCode: null,
  durationMs: null,
  running: false,
};

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

/** Prompt marks kept for jumping. Older ones are dropped. */
const MAX_PROMPT_MARKS = 500;

export interface PaneRuntimeOptions {
  id: string;
  /** The element the terminal draws into on its first mount. */
  host: HTMLDivElement;
  /** Builds the PTY spec once the terminal knows how big it is. */
  spec: (cols: number, rows: number) => PaneSpec;
}

export class PaneRuntime {
  readonly id: string;
  readonly term: Terminal;

  private readonly fit = new FitAddon();
  private readonly listeners = new Set<(snapshot: PaneSnapshot) => void>();
  private readonly promptMarks: IMarker[] = [];
  private snapshot: PaneSnapshot = EMPTY;
  private observer: ResizeObserver | null = null;
  private commandStartedAt: number | null = null;
  private detached = false;
  private disposed = false;

  constructor(options: PaneRuntimeOptions) {
    this.id = options.id;
    this.term = new Terminal({
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
    this.term.loadAddon(this.fit);
    this.term.open(options.host);

    // WebGL is a large win but is not available on every driver; the canvas
    // renderer is a correct fallback, so a failure here is not an error.
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      this.term.loadAddon(webgl);
    } catch {
      /* keep the canvas renderer */
    }

    this.watch(options.host);
    // The spec carries the size the PTY opens at, so the terminal has to know
    // how big it is first. A host without a size yet falls back to xterm's
    // default, and the first resize event corrects it.
    this.fitQuietly();

    this.term.onData((data) => {
      void paneWrite(this.id, data).catch(() => undefined);
    });
    this.term.onTitleChange((value) => {
      if (value.trim()) this.update(() => ({ title: value.trim() }));
    });

    void paneSpawn(options.spec(this.term.cols, this.term.rows), (event) =>
      this.receive(event),
    )
      .then(() =>
        this.update((current) =>
          current.status === "starting" ? { status: "live" } : {},
        ),
      )
      .catch((error: unknown) => {
        if (this.disposed) return;
        this.term.write(
          `\r\n\x1b[38;5;203m[could not open a pane: ${String(error)}]\x1b[0m\r\n`,
        );
        this.update(() => ({ status: "failed" }));
      });
  }

  /** Draws the terminal in `host`. Safe to call again after a detach. */
  attach(host: HTMLDivElement): void {
    if (this.disposed) return;
    this.detached = false;
    const element = this.term.element;
    if (element && element.parentElement !== host) host.appendChild(element);
    this.watch(host);
    // The pane almost certainly changed size while it was away, and the shell
    // has to hear about it or a full-screen program wraps to the old width.
    this.fitAndResize();
    this.term.refresh(0, this.term.rows - 1);
    this.emit();
  }

  /**
   * Lets go of the view without touching the shell.
   *
   * Called when a view unmounts, which happens both when a pane moves inside
   * the layout and when it is closed; the layout is what tells the two apart,
   * so the caller decides whether this is followed by `dispose`.
   */
  detach(): void {
    this.detached = true;
    this.observer?.disconnect();
    this.observer = null;
  }

  /**
   * The terminal is finished with, whether or not its shell still runs.
   *
   * `killShell` is the caller's answer to "is this shell still there?", which
   * only the pane registry can give: ending a shell that a close route has
   * already ended would be a second, empty-handed request for the same thing.
   */
  dispose(killShell: boolean): void {
    if (this.disposed) return;
    this.disposed = true;
    this.detach();
    this.listeners.clear();
    this.promptMarks.length = 0;
    if (killShell) void paneClose(this.id).catch(() => undefined);
    this.term.dispose();
  }

  get isDetached(): boolean {
    return this.detached;
  }

  /** Mirrors the pane's state into a view. Fires immediately with what is known. */
  subscribe(listener: (snapshot: PaneSnapshot) => void): () => void {
    this.listeners.add(listener);
    listener(this.snapshot);
    return () => {
      this.listeners.delete(listener);
    };
  }

  focus(): void {
    this.term.focus();
  }

  jumpToPreviousPrompt(): boolean {
    const here = this.term.buffer.active.viewportY;
    const above = this.liveMarks()
      .map((marker) => marker.line)
      .filter((line) => line < here);
    if (above.length === 0) return false;
    this.term.scrollToLine(Math.max(...above));
    return true;
  }

  jumpToNextPrompt(): boolean {
    const here = this.term.buffer.active.viewportY;
    const below = this.liveMarks()
      .map((marker) => marker.line)
      .filter((line) => line > here);
    if (below.length === 0) return false;
    this.term.scrollToLine(Math.min(...below));
    return true;
  }

  private receive(event: PtyEvent): void {
    if (this.disposed) return;
    switch (event.kind) {
      case "data":
        this.term.write(event.chunk);
        break;
      case "exit": {
        const suffix = event.code === null ? "" : ` with code ${event.code}`;
        this.term.write(
          `\r\n\x1b[38;5;245m[process exited${suffix}]\x1b[0m\r\n`,
        );
        this.update(() => ({ status: "exited", running: false }));
        break;
      }
      case "error":
        this.term.write(`\r\n\x1b[38;5;203m[${event.message}]\x1b[0m\r\n`);
        this.update(() => ({ status: "failed", running: false }));
        break;
      case "cwd":
        this.update(() => ({ cwd: event.path }));
        break;
      case "prompt":
        this.receivePrompt(event.mark, event.exitCode ?? null);
        break;
    }
  }

  private receivePrompt(mark: string, exitCode: number | null): void {
    switch (mark) {
      case "promptStart": {
        const marker = this.term.registerMarker(0);
        if (!marker) break;
        this.promptMarks.push(marker);
        if (this.promptMarks.length > MAX_PROMPT_MARKS) {
          this.promptMarks.shift()?.dispose();
        }
        break;
      }
      case "commandStart":
        this.commandStartedAt = performance.now();
        this.update(() => ({ durationMs: null, running: true }));
        break;
      case "commandEnd": {
        const started = this.commandStartedAt;
        this.commandStartedAt = null;
        this.update(() => ({
          exitCode,
          durationMs:
            started === null ? null : performance.now() - started,
          running: false,
        }));
        break;
      }
      default:
        break;
    }
  }

  private watch(host: HTMLDivElement): void {
    this.observer?.disconnect();
    this.observer = new ResizeObserver(() => this.fitAndResize());
    this.observer.observe(host);
  }

  private fitAndResize(): void {
    if (!this.fitQuietly()) return;
    void paneResize(this.id, this.term.cols, this.term.rows).catch(
      () => undefined,
    );
  }

  /** Fits, and reports whether the host had a size to fit to. */
  private fitQuietly(): boolean {
    try {
      this.fit.fit();
      return true;
    } catch {
      return false; // the host has no size yet
    }
  }

  /** Markers move with the buffer, so line numbers are read at jump time. */
  private liveMarks(): IMarker[] {
    for (let index = this.promptMarks.length - 1; index >= 0; index -= 1) {
      if (this.promptMarks[index]?.isDisposed) this.promptMarks.splice(index, 1);
    }
    return this.promptMarks;
  }

  private update(patch: (current: PaneSnapshot) => Partial<PaneSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch(this.snapshot) };
    this.emit();
  }

  private emit(): void {
    for (const listener of this.listeners) listener(this.snapshot);
  }
}

/** Every pane whose shell is running, whether or not it is on screen. */
const runtimes = new Map<string, PaneRuntime>();

/**
 * The runtime for a pane: created on its first mount, reused after that.
 *
 * A second mount is not a second shell. It draws the terminal that already
 * exists, which is why splitting a pane no longer costs the neighbour its
 * scrollback or whatever it was running.
 */
export function openPaneRuntime(options: PaneRuntimeOptions): PaneRuntime {
  const existing = runtimes.get(options.id);
  if (existing) {
    existing.attach(options.host);
    return existing;
  }
  const runtime = new PaneRuntime(options);
  runtimes.set(options.id, runtime);
  return runtime;
}

/**
 * Called when a pane's view unmounts.
 *
 * Unmounting is not closing: a split moves a pane inside the layout without
 * removing it. Only the app's layout knows which happened, and it is the
 * authority on what still exists, so the terminal is freed only once the pane
 * has left it. A pane that leaves by any route - the close button, the toolbar,
 * the palette - passes through here, so no route leaks a terminal or a shell.
 */
export async function detachPaneRuntime(id: string): Promise<void> {
  const runtime = runtimes.get(id);
  if (!runtime) return;
  runtime.detach();

  const workspace = await workspaceGet().catch(() => null);
  if (!workspace || !runtime.isDetached) return;
  if (containsPane(workspace.layout, id)) return;

  // The pane has left the layout, so it is finished. The close routes end the
  // shell themselves; this ends one that is somehow still running, which is the
  // only way a shell could outlive its pane.
  const live = await paneList().catch((): string[] => []);
  runtimes.delete(id);
  runtime.dispose(live.includes(id));
}

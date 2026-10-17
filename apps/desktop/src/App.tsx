import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  appInfo,
  paneClose,
  paneIds,
  paneList,
  workspaceClosePane,
  workspaceCycleFocus,
  workspaceInit,
  workspaceSetFocus,
  workspaceSetRatio,
  workspaceSplit,
  type AppInfo,
  type SplitDirection,
  type Workspace,
} from "./lib/ipc";
import { LayoutView } from "./components/LayoutView";

export default function App() {
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const nextPane = useRef(1);
  const workspaceRef = useRef<Workspace | null>(null);

  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  // Boot: agree with the backend about what is open before drawing anything.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const details = await appInfo();
        if (!cancelled) setInfo(details);
      } catch {
        /* informational only */
      }
      try {
        // A webview reload leaves the previous PTYs alive in Rust. Close them
        // so the panes on screen and the panes on disk always agree.
        const stale = await paneList();
        await Promise.all(stale.map((id) => paneClose(id).catch(() => undefined)));
        nextPane.current = 2;
        const fresh = await workspaceInit("p1");
        if (!cancelled) setWorkspace(fresh);
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const split = useCallback(async (direction: SplitDirection) => {
    const current = workspaceRef.current;
    if (!current) return;
    const target = current.focused ?? paneIds(current.layout)[0];
    if (!target) return;
    const id = `p${nextPane.current++}`;
    try {
      setWorkspace(await workspaceSplit(target, direction, id));
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const closePane = useCallback(async (id: string) => {
    const current = workspaceRef.current;
    if (!current || paneIds(current.layout).length <= 1) return;
    await paneClose(id).catch(() => undefined);
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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const accelerator = event.ctrlKey || event.metaKey;
      if (!accelerator) return;

      if (event.key === "Tab") {
        event.preventDefault();
        void cycle(event.shiftKey ? -1 : 1);
        return;
      }
      if (!event.shiftKey) return;

      switch (event.key.toLowerCase()) {
        case "e":
          event.preventDefault();
          void split("row");
          break;
        case "o":
          event.preventDefault();
          void split("column");
          break;
        case "w": {
          const focused = workspaceRef.current?.focused;
          if (!focused) return;
          event.preventDefault();
          void closePane(focused);
          break;
        }
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [cycle, split, closePane]);

  const canClose = workspace !== null && paneIds(workspace.layout).length > 1;

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
            shortcut="Ctrl Shift E"
            onClick={() => void split("row")}
            disabled={!workspace}
          >
            <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
            <path d="M6.5 1.8v8.4" />
          </ToolButton>
          <ToolButton
            label="Split down"
            shortcut="Ctrl Shift O"
            onClick={() => void split("column")}
            disabled={!workspace}
          >
            <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
            <path d="M1.2 6h10.6" />
          </ToolButton>
          <ToolButton
            label="Close pane"
            shortcut="Ctrl Shift W"
            onClick={() => {
              const focused = workspace?.focused;
              if (focused) void closePane(focused);
            }}
            disabled={!canClose}
          >
            <path d="M3.4 3.4l6.2 6.2M9.6 3.4l-6.2 6.2" />
          </ToolButton>
        </div>

        <div className="ml-auto flex items-center gap-3 text-[10px] text-ink-faint">
          {workspace ? (
            <span>
              {paneIds(workspace.layout).length} pane
              {paneIds(workspace.layout).length === 1 ? "" : "s"}
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
        <Hint keys="Ctrl Shift E" action="split right" />
        <Hint keys="Ctrl Shift O" action="split down" />
        <Hint keys="Ctrl Shift W" action="close pane" />
        <Hint keys="Ctrl Tab" action="next pane" />
        <span className="ml-auto">local-first · no account · no server</span>
      </footer>
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

function Hint({ keys, action }: { keys: string; action: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="rounded border border-line bg-raised px-1.5 py-px text-[9px] text-ink-muted">
        {keys}
      </kbd>
      <span>{action}</span>
    </span>
  );
}

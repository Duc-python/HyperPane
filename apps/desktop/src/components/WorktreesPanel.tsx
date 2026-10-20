import { useEffect, useState } from "react";
import {
  worktreeRemove,
  worktreesList,
  type WorktreeStateView,
  type WorktreesView,
} from "../lib/ipc";
import { describeWorktree, needsConfirming, normalisePath, removalCost } from "../lib/worktrees";

/**
 * The worktrees the app has made, and what each one would cost to remove.
 *
 * The app forks a worktree per agent run rather than letting two agents write in
 * one checkout, so this directory grows on its own. Nothing here is removed
 * automatically - a run that has not been looked at yet is exactly the one worth
 * keeping - and a worktree that still has work needs a second answer, which is
 * what makes the second click mean something.
 */
export function WorktreesPanel({
  onClose,
  formatPath,
  inUse,
}: {
  onClose: () => void;
  /** Shortens a filesystem path for a row, as the pane headers do. */
  formatPath: (path: string) => string;
  /**
   * Worktrees a pane is running in right now.
   *
   * Removing one would pull the directory out from under a live process. The
   * app knows which runs are open, so it says so instead of letting a person
   * find out from the operating system's error.
   */
  inUse: readonly string[];
}) {
  const [view, setView] = useState<WorktreesView | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const live = new Set(inUse.map(normalisePath));

  const load = () => {
    setError(null);
    void worktreesList()
      .then(setView)
      .catch((cause: unknown) => setError(String(cause)));
  };

  useEffect(load, []);

  const remove = (entry: WorktreeStateView, force: boolean) => {
    setBusy(entry.path);
    setError(null);
    void worktreeRemove(entry.path, force)
      .then(() => {
        setArmed(null);
        load();
      })
      .catch((cause: unknown) => setError(String(cause)))
      .finally(() => setBusy(null));
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Worktrees"
        className="flex max-h-[80vh] w-[min(820px,94vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">Worktrees</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            one per agent run · branches are never deleted
          </span>
          <button
            type="button"
            onClick={load}
            className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            reload
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            close
          </button>
        </header>

        {view ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <div className="shrink-0 border-b border-line px-4 py-2">
              <span className="text-[10px] text-ink-faint">{view.dir}</span>
            </div>
            {view.items.length === 0 ? (
              <p className="px-4 py-6 text-[11px] text-ink-muted">
                Nothing here: a worktree is made when a run asks to be confined to
                one.
              </p>
            ) : (
              <ul className="min-h-0 flex-1 overflow-auto">
                {view.items.map((entry) => {
                  const cost = removalCost(entry);
                  const isArmed = armed === entry.path;
                  const running = live.has(normalisePath(entry.path));
                  return (
                    <li
                      key={entry.path}
                      className="flex items-start gap-3 border-b border-line/60 px-4 py-2.5"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="flex items-baseline gap-2">
                          <span className="min-w-0 truncate text-[11px] text-ink">
                            {entry.branch ?? "no branch"}
                          </span>
                          {running ? (
                            <span className="shrink-0 rounded border border-accent-strong/60 bg-accent-soft px-1 text-[9px] text-accent">
                              a run is using this
                            </span>
                          ) : null}
                        </span>
                        <span
                          className="block truncate text-[10px] text-ink-faint"
                          title={entry.path}
                        >
                          {formatPath(entry.path)}
                        </span>
                        <span className="block text-[10px] text-ink-muted">
                          {running
                            ? "close that pane before removing it"
                            : cost === null
                              ? describeWorktree(entry)
                              : `removing it: ${cost}`}
                        </span>
                      </span>
                      <button
                        type="button"
                        disabled={busy === entry.path || running}
                        title={
                          running
                            ? "a pane is running in this worktree; close it first"
                            : undefined
                        }
                        onClick={() => {
                          if (needsConfirming(entry) && !isArmed) {
                            setArmed(entry.path);
                            return;
                          }
                          remove(entry, isArmed);
                        }}
                        className="shrink-0 rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink disabled:opacity-50"
                      >
                        {isArmed ? "remove anyway" : "remove"}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        ) : (
          <p className="px-4 py-6 text-[11px] text-ink-muted">
            {error ? `could not read them: ${error}` : "reading…"}
          </p>
        )}

        {error && view ? (
          <p className="shrink-0 border-t border-line px-4 py-2 text-[10px] text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </div>
  );
}

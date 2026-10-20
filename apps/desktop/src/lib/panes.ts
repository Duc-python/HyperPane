/**
 * What a pane is, said the same way everywhere.
 *
 * The pane header, the tab strip and the sidebar all describe the same panes.
 * Working the words out once per component is three chances to disagree — the
 * ConPTY case below is exactly the one that would drift — so it is worked out
 * once, here, and the components only decide where to put it.
 */

import { containsPane, type LayoutNode, type RunPhase } from "./ipc";
import type { PaneSnapshot } from "./paneRuntime";

/** Everything a pane's view has reported about itself. */
export interface PaneLive {
  snapshot: PaneSnapshot;
  /** What an agent pane said about its own work, when it runs one. */
  phase: RunPhase | null;
}

/**
 * How a pane reads at a glance.
 *
 * `busy` is a command running inside a live shell — from the prompt marks the
 * shell integration emits — or an agent that is working. It is deliberately not
 * folded into `live`: a green dot on a pane halfway through a build would be a
 * claim the app cannot back.
 */
export type PaneTone = "starting" | "live" | "busy" | "exited" | "failed";

export const TONE_LABEL: Record<PaneTone, string> = {
  starting: "starting",
  live: "live",
  busy: "working",
  exited: "exited",
  failed: "failed",
};

export interface PaneRow {
  id: string;
  /** What to call this pane on screen. */
  label: string;
  /** The directory its shell last reported, when it reported one. */
  cwd: string | null;
  tone: PaneTone;
  focused: boolean;
}

/**
 * Whether a title is really a program path rather than a name.
 *
 * ConPTY reports the program path as the window title until the shell sets one
 * of its own. `C:\Program Files\PowerShell\7\pwsh.exe` as a pane's name tells a
 * person nothing, so it is treated as no title at all.
 */
export function titleLooksLikeProgramPath(raw: string): boolean {
  return (
    raw.includes("\\") ||
    /\.(exe|cmd|bat|com|ps1)$/i.test(raw) ||
    /^\/[^\s]*$/.test(raw)
  );
}

/**
 * A pane's name: its own title, or what it is running when that is all there is.
 *
 * The fallback is the profile's name for a shell and the agent's label for a
 * run, which is what tells two panes apart before either has printed anything.
 */
export function displayPaneTitle(raw: string, fallback: string): string {
  const title = raw.trim();
  return title === "" || titleLooksLikeProgramPath(title) ? fallback : title;
}

/** The tone of a pane, from the last thing its view reported. */
export function paneTone(live: PaneLive | undefined): PaneTone {
  if (!live) return "starting";
  const { snapshot, phase } = live;
  switch (snapshot.status) {
    case "starting":
      return "starting";
    case "exited":
      return "exited";
    case "failed":
      return "failed";
    case "live":
      // Both of these are things the pane said about itself: a shell that
      // reported the start of a command, or an agent whose own output says it
      // is working. Nothing here is inferred from a timer.
      if (snapshot.running) return "busy";
      if (phase === "starting" || phase === "working") return "busy";
      return "live";
  }
}

/**
 * One row per pane of a layout, in the layout's own visual order.
 *
 * `ids` comes from the layout tree and `live` from the views that have mounted,
 * so a pane the app knows about but nothing has drawn yet still gets a row:
 * dropping it would make the list disagree with the grid.
 */
export function paneRows(options: {
  ids: readonly string[];
  live: Record<string, PaneLive>;
  focused: string | null;
  /** What to call a pane that has not reported a title of its own. */
  fallback: (id: string) => string;
}): PaneRow[] {
  return options.ids.map((id) => {
    const live = options.live[id];
    return {
      id,
      label: displayPaneTitle(live?.snapshot.title ?? "", options.fallback(id)),
      cwd: live?.snapshot.cwd ?? null,
      tone: paneTone(live),
      focused: options.focused === id,
    };
  });
}

/**
 * Sessions the app still has that no pane on screen is using.
 *
 * A rebuilt interface brings its panes back, and each of them reconnects to the
 * shell it was drawing, so the only thing left over is a session with no pane:
 * one closed while the window was away, or a layout that no longer mentions it.
 * Those are the sessions to end - closing the others is what used to restart every
 * shell on a reload, taking the scrollback and whatever was running with it.
 *
 * With no layout at all there is nothing to reconnect to, and every session is
 * somebody's leftovers.
 */
export function orphanPanes(
  live: readonly string[],
  layout: LayoutNode | null,
): string[] {
  if (!layout) return [...live];
  return live.filter((id) => !containsPane(layout, id));
}

/**
 * The pane that owns the most of the window, and the path that names it.
 *
 * A team wave lands in the widest pane. Splitting the one a person happened to
 * have focused put two debaters into a quarter-width column, where their own
 * interface wrapped every word. The path is the list of decisions from the root
 * (false = first, true = second), which is how a split is named when it is
 * resized.
 */
export function widestPane(
  layout: LayoutNode,
): { pane: string; path: boolean[]; share: number } | null {
  let best: { pane: string; path: boolean[]; share: number } | null = null;

  const walk = (node: LayoutNode, path: boolean[], share: number) => {
    if (node.type === "leaf") {
      if (!best || share > best.share) best = { pane: node.pane, path, share };
      return;
    }
    // A ratio outside these bounds is a file someone edited by hand; the tree
    // is still readable, so clamp rather than refuse the whole layout.
    const ratio = Math.min(Math.max(node.ratio, 0.05), 0.95);
    walk(node.first, [...path, false], share * ratio);
    walk(node.second, [...path, true], share * (1 - ratio));
  };

  walk(layout, [], 1);
  return best;
}

/**
 * The ratios a row of fresh panes needs so every pane in it is the same width.
 *
 * The layout is binary, so equal shares are a chain: the pane being split keeps
 * `1/(n+1)` of the row, the next split takes `1/n` of what is left, and so on.
 * Two new panes therefore leave three equal columns, not one wide one and two
 * slivers.
 */
export function tilingRatios(tiles: number): number[] {
  return Array.from({ length: tiles }, (_, index) => 1 / (tiles - index + 1));
}

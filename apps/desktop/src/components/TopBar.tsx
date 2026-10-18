import type {
  AppInfo,
  RoomView,
  SplitDirection,
  TeamStatus,
  Workspace,
} from "../lib/ipc";
import { formatChord } from "../lib/keybindings";
import { TONE_LABEL, type PaneRow } from "../lib/panes";
import { roomChip, roomSummary } from "../lib/room";
import { Chip, ChipButton, CloseButton, IconButton, PaneGlyph } from "./Chrome";

interface TopBarProps {
  workspace: Workspace | null;
  /** Every pane of the layout, in visual order, as the grid draws them. */
  panes: readonly PaneRow[];
  info: AppInfo | null;
  /** The profile a new pane opens with, when one is chosen. */
  profileName: string | null;
  canClose: boolean;
  /** The run in progress, when there is one. */
  teamRun: TeamStatus | null;
  room: RoomView | null;
  /** Whether the control room dock is on screen. */
  roomOpen: boolean;
  onFocusPane: (id: string) => void;
  onClosePane: (id: string) => void;
  onSplit: (direction: SplitDirection) => void;
  onCloseFocused: () => void;
  onOpenProfiles: () => void;
  onOpenRun: () => void;
  onToggleRoom: () => void;
  onOpenPalette: () => void;
}

/**
 * The top bar: what is open, and the way to open or close something.
 *
 * The tab strip is the layout, read sideways: one chip per pane, in the order
 * the grid puts them, and clicking one focuses that pane rather than moving it.
 * The app's layout is the authority on what exists, so nothing here keeps its
 * own copy of the tabs.
 */
export function TopBar({
  workspace,
  panes,
  info,
  profileName,
  canClose,
  teamRun,
  room,
  roomOpen,
  onFocusPane,
  onClosePane,
  onSplit,
  onCloseFocused,
  onOpenProfiles,
  onOpenRun,
  onToggleRoom,
  onOpenPalette,
}: TopBarProps) {
  const count = panes.length;

  return (
    <header className="flex h-10 shrink-0 items-center gap-2 border-b border-line/60 bg-surface/40 px-2">
      <div className="flex shrink-0 items-center gap-2 pl-0.5">
        <span
          aria-hidden
          className="size-4 rounded-[5px] bg-accent shadow-[0_0_12px_-2px_var(--color-accent)]"
        />
        <span className="text-[12px] font-semibold tracking-tight text-ink">
          HyperPane
        </span>
        {info ? (
          <span className="text-[9px] text-ink-faint">v{info.version}</span>
        ) : null}
      </div>

      <div className="flex shrink-0 items-center gap-1">
        <Chip
          tone="accent"
          title={
            workspace
              ? `Workspace ${workspace.id}. This build holds one workspace per window.`
              : "no workspace yet"
          }
        >
          {workspace?.name ?? "…"}
        </Chip>
        <Chip
          title={`${count} pane${count === 1 ? "" : "s"} in this workspace`}
        >
          <span className="tabular-nums">{String(count).padStart(2, "0")}</span>
        </Chip>
      </div>

      <nav
        aria-label="Panes"
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
      >
        {panes.map((pane) => (
          <span
            key={pane.id}
            data-active={pane.focused}
            className="flex max-w-44 min-w-0 shrink-0 items-center gap-1 rounded-md border border-line/60 bg-base/70 px-1.5 py-1 data-[active=true]:border-accent-strong/70 data-[active=true]:bg-accent-soft/25"
          >
            <PaneGlyph tone={pane.tone} label={TONE_LABEL[pane.tone]} />
            <button
              type="button"
              data-active={pane.focused}
              onClick={() => onFocusPane(pane.id)}
              title={pane.cwd ? `${pane.id} · ${pane.cwd}` : pane.id}
              className="min-w-0 truncate text-[11px] text-ink-muted transition-colors hover:text-ink data-[active=true]:text-ink"
            >
              {pane.label}
            </button>
            {canClose ? (
              <CloseButton
                label={`Close pane ${pane.id}`}
                title={`Close ${pane.label}`}
                onClick={() => onClosePane(pane.id)}
              />
            ) : null}
          </span>
        ))}
        <button
          type="button"
          aria-label="Split the focused pane right"
          title={`New pane beside the focused one (${formatChord("mod+shift+e")})`}
          onClick={() => onSplit("row")}
          className="grid size-6 shrink-0 place-items-center rounded-md border border-dashed border-line/70 text-[13px] leading-none text-ink-faint transition-colors hover:border-line-strong hover:text-ink"
        >
          +
        </button>
      </nav>

      <div className="ml-1 flex shrink-0 items-center gap-1.5">
        {profileName ? (
          <ChipButton
            title={`${profileName} — click to edit profiles (${formatChord("mod+,")})`}
            onClick={onOpenProfiles}
          >
            {profileName}
          </ChipButton>
        ) : null}

        {teamRun ? (
          <ChipButton
            tone={teamRun.holding ? "warn" : "plain"}
            title={`${
              teamRun.holdingMessage ??
              `${teamRun.wavesRemaining} wave(s) after this one`
            } — open the run view`}
            onClick={onOpenRun}
          >
            team {teamRun.title} · wave {teamRun.wave}
            {teamRun.running > 0 ? ` · ${teamRun.running} running` : ""}
          </ChipButton>
        ) : null}

        <ChipButton
          tone={room && room.mode !== "off" ? "accent" : "plain"}
          active={roomOpen}
          title={`${roomSummary(room)} — ${roomOpen ? "hide" : "show"} the control room`}
          onClick={onToggleRoom}
        >
          {roomChip(room)}
        </ChipButton>

        <ChipButton
          title={`Command palette (${formatChord("mod+shift+p")})`}
          onClick={onOpenPalette}
        >
          <svg
            viewBox="0 0 13 12"
            className="size-3"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
            aria-hidden
          >
            <circle cx="5.4" cy="5.2" r="3.4" />
            <path d="M8 7.8l2.8 2.6" strokeLinecap="round" />
          </svg>
          <kbd className="text-[9px] text-ink-faint">
            {formatChord("mod+shift+p")}
          </kbd>
        </ChipButton>

        <span className="mx-0.5 h-4 w-px bg-line" aria-hidden />

        <IconButton
          label="Split right"
          shortcut={formatChord("mod+shift+e")}
          disabled={!workspace}
          onClick={() => onSplit("row")}
        >
          <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
          <path d="M6.5 1.8v8.4" />
        </IconButton>
        <IconButton
          label="Split down"
          shortcut={formatChord("mod+shift+o")}
          disabled={!workspace}
          onClick={() => onSplit("column")}
        >
          <rect x="1.2" y="1.8" width="10.6" height="8.4" rx="1.6" />
          <path d="M1.2 6h10.6" />
        </IconButton>
        <IconButton
          label="Close pane"
          shortcut={formatChord("mod+shift+w")}
          disabled={!canClose}
          onClick={onCloseFocused}
        >
          <path d="M3.4 3.4l6.2 6.2M9.6 3.4l-6.2 6.2" />
        </IconButton>
      </div>
    </header>
  );
}

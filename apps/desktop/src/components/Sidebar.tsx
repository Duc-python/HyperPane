import type { ReactNode } from "react";
import type { RoomParticipant, RoomView, SplitDirection, Workspace } from "../lib/ipc";
import { formatChord } from "../lib/keybindings";
import { TONE_LABEL, type PaneRow } from "../lib/panes";
import { CloseButton, IconButton, PaneGlyph } from "./Chrome";

interface SidebarProps {
  workspace: Workspace | null;
  /** Every pane of the layout, in visual order. */
  panes: readonly PaneRow[];
  canClose: boolean;
  room: RoomView | null;
  /** What to call a pane, so the room speaks in names rather than ids. */
  paneLabel: (id: string) => string;
  onFocusPane: (id: string) => void;
  onClosePane: (id: string) => void;
  onSplit: (direction: SplitDirection) => void;
  onFollow: (who: string | null) => void;
  onOpenRoom: () => void;
  onOpenProfiles: () => void;
  onOpenSettings: () => void;
  onHide: () => void;
}

/**
 * The left rail: what this window holds.
 *
 * Three lists and two ways in. Everything on it is a reading of state the app
 * already has — the workspace from the layout, one row per pane, and the room's
 * own snapshot of who is in it — so nothing here can disagree with the grid in
 * the middle.
 *
 * Two things from the design this is built from are deliberately absent: a `+`
 * beside the workspace list and an SSH/SFTP manager at the bottom. Neither has
 * anything behind it in this app — there is one workspace per window, and no
 * SSH client at all — so the workspace list says so in words rather than
 * offering a button that would do nothing.
 */
export function Sidebar({
  workspace,
  panes,
  canClose,
  room,
  paneLabel,
  onFocusPane,
  onClosePane,
  onSplit,
  onFollow,
  onOpenRoom,
  onOpenProfiles,
  onOpenSettings,
  onHide,
}: SidebarProps) {
  const people = room?.room?.participants ?? [];
  const inARoom = room !== null && room.mode !== "off";
  const me = people.find((entry) => entry.id === room?.me) ?? null;

  return (
    <aside
      aria-label="Workspace sidebar"
      className="flex w-[188px] shrink-0 flex-col border-r border-line/60 bg-surface/25"
    >
      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-2">
        <Section
          label="Workspace"
          actions={
            <IconButton label="Hide the sidebar" onClick={onHide}>
              <path d="M8 3l-3.5 3L8 9" />
            </IconButton>
          }
        >
          {/* A readout, not a row to click: there is nothing else to switch to. */}
          <div className="flex items-center gap-1.5 rounded-md border border-line/70 bg-raised/50 px-1.5 py-1">
            <span aria-hidden className="size-2 shrink-0 rounded-[3px] bg-accent" />
            <span className="min-w-0 flex-1 truncate text-[11px] text-ink">
              {workspace?.name ?? "…"}
            </span>
            <span className="shrink-0 rounded border border-line/70 px-1 text-[9px] leading-4 tabular-nums text-ink-faint">
              {panes.length}
            </span>
          </div>
          <p className="px-1.5 pt-1 text-[9px] leading-4 text-ink-faint">
            one workspace per window in this build
          </p>
        </Section>

        <Section
          label="Terminals"
          count={panes.length}
          actions={
            <IconButton
              label="New pane"
              shortcut={formatChord("mod+shift+e")}
              onClick={() => onSplit("row")}
            >
              <path d="M6.5 2.5v7M3 6h7" />
            </IconButton>
          }
        >
          {panes.length === 0 ? (
            <p className="px-1.5 text-[9px] text-ink-faint">nothing open</p>
          ) : (
            panes.map((pane) => (
              <div
                key={pane.id}
                data-active={pane.focused}
                className="flex items-center gap-1.5 rounded-md border border-transparent px-1.5 py-1 hover:bg-overlay/40 data-[active=true]:border-line/70 data-[active=true]:bg-raised/50"
              >
                <PaneGlyph tone={pane.tone} label={TONE_LABEL[pane.tone]} />
                <button
                  type="button"
                  onClick={() => onFocusPane(pane.id)}
                  title={pane.cwd ? `${pane.id} · ${pane.cwd}` : pane.id}
                  className="min-w-0 flex-1 truncate text-left text-[11px] text-ink-muted transition-colors hover:text-ink"
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
              </div>
            ))
          )}
        </Section>

        <Section
          label="Room"
          count={inARoom ? people.length : undefined}
          actions={
            <IconButton
              label="Open the control room"
              active={inARoom}
              onClick={onOpenRoom}
            >
              <path d="M5 3l3.5 3L5 9" />
            </IconButton>
          }
        >
          {!inARoom ? (
            <>
              <p className="px-1.5 text-[9px] leading-4 text-ink-faint">
                not in a room · nothing leaves this machine until a pane is
                shared
              </p>
              <button
                type="button"
                onClick={onOpenRoom}
                className="mt-1 w-full rounded-md border border-line/70 px-1.5 py-1 text-left text-[10px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink"
              >
                host or join one
              </button>
            </>
          ) : (
            people.map((participant) => (
              <PersonRow
                key={participant.id}
                participant={participant}
                isMe={participant.id === room.me}
                following={me?.following === participant.id}
                label={
                  participant.focus ? paneLabel(participant.focus) : null
                }
                // Only a visitor can follow: the host sends nothing to a room,
                // so offering it there would be a button that does nothing.
                onFollow={room.mode === "visiting" ? onFollow : null}
              />
            ))
          )}
        </Section>
      </div>

      <div className="flex shrink-0 flex-col gap-0.5 border-t border-line/60 p-2">
        <PinnedRow
          label="Profiles…"
          title="Edit the shells a pane opens with"
          onClick={onOpenProfiles}
        >
          <circle cx="6.5" cy="4" r="2" />
          <path d="M2.6 10.2c.7-2 2.2-3 3.9-3s3.2 1 3.9 3" />
        </PinnedRow>
        <PinnedRow
          label="Settings…"
          title="Where HyperPane keeps its files"
          onClick={onOpenSettings}
        >
          <path d="M2 4h9M2 8h9" />
          <circle cx="5" cy="4" r="1.3" />
          <circle cx="8.5" cy="8" r="1.3" />
        </PinnedRow>
      </div>
    </aside>
  );
}

/**
 * One person in the room, as the rail shows them.
 *
 * Green means they are looking at a pane, which is the only thing this instance
 * is told about them; fading means they have not said. A person with no pane is
 * not a problem, so it is not amber.
 */
function PersonRow({
  participant,
  isMe,
  following,
  label,
  onFollow,
}: {
  participant: RoomParticipant;
  isMe: boolean;
  following: boolean;
  label: string | null;
  onFollow: ((who: string | null) => void) | null;
}) {
  const body = (
    <>
      <span
        aria-hidden
        className={`size-1.5 shrink-0 rounded-full ${
          participant.focus ? "bg-ok" : "bg-ink-faint/50"
        }`}
      />
      <span className="min-w-0 flex-1 truncate text-[11px] text-ink-muted">
        {participant.name}
      </span>
      <span className="max-w-[62px] shrink-0 truncate text-[9px] text-ink-faint">
        {label ?? "no pane"}
      </span>
    </>
  );

  const tags = (
    <>
      {participant.host ? (
        <span className="shrink-0 text-[8px] tracking-wide text-ink-faint uppercase">
          host
        </span>
      ) : null}
      {isMe ? (
        <span className="shrink-0 text-[8px] tracking-wide text-accent uppercase">
          you
        </span>
      ) : null}
      {following ? (
        <span className="shrink-0 text-[8px] tracking-wide text-accent uppercase">
          following
        </span>
      ) : null}
    </>
  );

  // Somebody else's view can be followed from here, when this instance is
  // visiting: following the host is the main reason to. There is nothing to
  // follow in the host's own rail, so `onFollow` is null there.
  if (!onFollow || isMe) {
    return (
      <div className="flex items-center gap-1.5 rounded-md px-1.5 py-1">
        {body}
        {tags}
      </div>
    );
  }

  return (
    <button
      type="button"
      aria-pressed={following}
      onClick={() => onFollow(following ? null : participant.id)}
      title={
        following
          ? `Stop watching ${participant.name}`
          : `Follow ${participant.name}: watch the pane they are looking at`
      }
      className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left transition-colors hover:bg-overlay/40"
    >
      {body}
      {tags}
    </button>
  );
}

function Section({
  label,
  count,
  actions,
  children,
}: {
  label: string;
  count?: number | undefined;
  actions: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="mb-3">
      <header className="flex items-center gap-1.5 px-1.5 pb-1">
        <h2 className="text-[9px] font-medium tracking-[0.08em] text-ink-faint uppercase">
          {label}
        </h2>
        {count !== undefined ? (
          <span className="text-[9px] tabular-nums text-ink-faint">{count}</span>
        ) : null}
        <span className="ml-auto flex items-center gap-0.5">{actions}</span>
      </header>
      {children}
    </section>
  );
}

function PinnedRow({
  label,
  title,
  onClick,
  children,
}: {
  label: string;
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-md px-1.5 py-1 text-left text-[10px] text-ink-muted transition-colors hover:bg-raised/60 hover:text-ink"
    >
      <svg
        viewBox="0 0 13 12"
        className="size-3.5 shrink-0"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinecap="round"
        aria-hidden
      >
        {children}
      </svg>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}

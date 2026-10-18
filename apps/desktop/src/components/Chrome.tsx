import type { ReactNode } from "react";
import { formatChord } from "../lib/keybindings";
import type { PaneTone } from "../lib/panes";

/**
 * The shell's small parts.
 *
 * The top bar, the sidebar, the pane header and the control room all need the
 * same three shapes — a square icon button, a chip, and a key hint. Written out
 * per component they drift apart a little each time, and the drift is only
 * visible when two of them sit next to each other.
 */

interface IconButtonProps {
  label: string;
  /** Shown after the label in the tooltip: the chord, when there is one. */
  shortcut?: string | undefined;
  /** Set for a button that shows or hides something, so its state is readable. */
  active?: boolean | undefined;
  disabled?: boolean | undefined;
  onClick: () => void;
  children: ReactNode;
}

export function IconButton({
  label,
  shortcut,
  active,
  disabled,
  onClick,
  children,
}: IconButtonProps) {
  return (
    <button
      type="button"
      title={shortcut ? `${label} (${shortcut})` : label}
      aria-label={label}
      aria-pressed={active}
      disabled={disabled ?? false}
      onClick={onClick}
      data-active={active ?? false}
      className="grid size-6 shrink-0 place-items-center rounded-md border border-transparent text-ink-muted transition-colors hover:border-line/70 hover:bg-raised/60 hover:text-ink data-[active=true]:border-accent-strong/60 data-[active=true]:bg-accent-soft/30 data-[active=true]:text-accent disabled:pointer-events-none disabled:opacity-35"
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

export type ChipTone = "plain" | "accent" | "ok" | "warn" | "danger";

const CHIP_TONE: Record<ChipTone, string> = {
  plain: "border-line/70 bg-raised/50 text-ink-muted",
  accent: "border-accent-strong/60 bg-accent-soft/30 text-accent",
  ok: "border-ok/50 bg-ok/10 text-ok",
  warn: "border-warn/50 bg-warn/10 text-warn",
  danger: "border-danger/50 bg-danger/10 text-danger",
};

const CHIP =
  "inline-flex shrink-0 items-center gap-1 rounded border px-1 py-px text-[9px] leading-4";

interface ChipProps {
  tone?: ChipTone;
  title?: string | undefined;
  children: ReactNode;
}

/** A small piece of state: a count, a branch, a claim a child made. */
export function Chip({ tone = "plain", title, children }: ChipProps) {
  return (
    <span title={title} className={`${CHIP} ${CHIP_TONE[tone]}`}>
      {children}
    </span>
  );
}

interface ChipButtonProps {
  tone?: ChipTone;
  title?: string | undefined;
  /** Set for a chip that toggles something, so its state is readable. */
  active?: boolean | undefined;
  onClick: () => void;
  children: ReactNode;
}

/** A chip that is also the way in to whatever it is about. */
export function ChipButton({
  tone = "plain",
  title,
  active,
  onClick,
  children,
}: ChipButtonProps) {
  return (
    <button
      type="button"
      title={title}
      aria-pressed={active}
      data-active={active ?? false}
      onClick={onClick}
      className={`${CHIP} ${CHIP_TONE[tone]} transition-colors hover:border-line-strong hover:text-ink data-[active=true]:border-accent-strong/70 data-[active=true]:bg-accent-soft/30 data-[active=true]:text-accent`}
    >
      {children}
    </button>
  );
}

const DOT: Record<PaneTone, string> = {
  starting: "bg-warn",
  live: "bg-ok",
  busy: "bg-warn",
  exited: "bg-ink-faint",
  failed: "bg-danger",
};

/**
 * A pane's state as a mark.
 *
 * A live pane is a dot; a pane that is working spins, because a still dot
 * cannot say "this is busy" and a colour alone cannot say it either — amber
 * means both "starting" and "working" at 6 pixels wide. The box is a fixed
 * size so a row does not shift when its mark changes shape.
 *
 * The label is required rather than optional: a mark with no name is a shape
 * nobody can read out, and `TONE_LABEL` already has the words.
 */
export function PaneGlyph({ tone, label }: { tone: PaneTone; label: string }) {
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className="grid size-2.5 shrink-0 place-items-center"
    >
      {tone === "busy" || tone === "starting" ? (
        <span
          aria-hidden
          className={`size-2.5 rounded-full border ${
            tone === "busy"
              ? "animate-spin border-warn/80 border-t-transparent"
              : "animate-pulse border-warn/60"
          }`}
        />
      ) : (
        <span aria-hidden className={`size-1.5 rounded-full ${DOT[tone]}`} />
      )}
    </span>
  );
}

/** A chord and what it does, for the status bar. */
export function Hint({
  chord,
  action,
  className,
}: {
  chord: string;
  action: string;
  /** Set when a hint should drop out on a window too narrow for all of them. */
  className?: string | undefined;
}) {
  return (
    <span
      className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap ${className ?? ""}`}
    >
      <kbd className="rounded border border-line bg-raised px-1.5 py-px text-[9px] text-ink-muted">
        {formatChord(chord)}
      </kbd>
      <span>{action}</span>
    </span>
  );
}

/** The small × that closes a pane, from the tab strip or the rail. */
export function CloseButton({
  label,
  title,
  onClick,
}: {
  label: string;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={title}
      onClick={onClick}
      className="grid size-3.5 shrink-0 place-items-center rounded-[3px] text-ink-faint transition-colors hover:bg-overlay hover:text-danger"
    >
      <svg viewBox="0 0 10 10" className="size-2" aria-hidden>
        <path
          d="M1 1l8 8M9 1l-8 8"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
          fill="none"
        />
      </svg>
    </button>
  );
}

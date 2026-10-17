/**
 * Keybindings.
 *
 * One table. Nothing in the app compares key events to hard-coded chords, so a
 * binding can be changed (and later, remapped) in one place.
 *
 * A chord is written `mod+shift+e`, where `mod` is Command on macOS and Control
 * elsewhere. That is the only platform difference the rest of the app sees.
 */

export interface KeyBinding {
  chord: string;
  /** Command id, resolved by the app against its command list. */
  command: string;
}

export const DEFAULT_BINDINGS: readonly KeyBinding[] = [
  // Control+Shift+P, not Control+K: Control+K is a real terminal key
  // (kill-line in readline), and stealing it would break every shell.
  { chord: "mod+shift+p", command: "app.palette" },
  { chord: "mod+shift+e", command: "pane.splitRight" },
  { chord: "mod+shift+o", command: "pane.splitDown" },
  { chord: "mod+shift+w", command: "pane.close" },
  { chord: "mod+tab", command: "pane.next" },
  { chord: "mod+shift+tab", command: "pane.previous" },
  { chord: "mod+shift+up", command: "pane.previousPrompt" },
  { chord: "mod+shift+down", command: "pane.nextPrompt" },
];

/**
 * Names a person writes, mapped to what `KeyboardEvent.key` reports. A chord is
 * written the way it is spoken; the comparison is the browser's spelling.
 */
const KEY_ALIASES: Record<string, string> = {
  up: "arrowup",
  down: "arrowdown",
  left: "arrowleft",
  right: "arrowright",
  esc: "escape",
  space: " ",
};

const KEY_LABELS: Record<string, string> = {
  up: "\u2191",
  down: "\u2193",
  left: "\u2190",
  right: "\u2192",
};

function resolveKey(key: string): string {
  return KEY_ALIASES[key] ?? key;
}

export function isMac(): boolean {
  return navigator.userAgent.includes("Mac");
}

function isPrimaryModifier(event: KeyboardEvent): boolean {
  return isMac() ? event.metaKey : event.ctrlKey;
}

function isSecondaryModifier(event: KeyboardEvent): boolean {
  return isMac() ? event.ctrlKey : event.metaKey;
}

/** Whether an event is exactly this chord, with no extra modifiers held. */
export function matchesChord(event: KeyboardEvent, chord: string): boolean {
  const parts = chord.toLowerCase().split("+");
  const key = parts.at(-1);
  if (!key) return false;
  const modifiers = new Set(parts.slice(0, -1));

  if (isPrimaryModifier(event) !== modifiers.has("mod")) return false;
  if (event.shiftKey !== modifiers.has("shift")) return false;
  if (event.altKey !== modifiers.has("alt")) return false;
  // The other meta key must not be held, or Command+K would also fire on
  // Control+K on macOS.
  if (isSecondaryModifier(event)) return false;

  return event.key.toLowerCase() === resolveKey(key);
}

/** The command an event should run, or `null`. First match wins. */
export function commandForEvent(
  event: KeyboardEvent,
  bindings: readonly KeyBinding[] = DEFAULT_BINDINGS,
): string | null {
  for (const binding of bindings) {
    if (matchesChord(event, binding.chord)) return binding.command;
  }
  return null;
}

const MAC_LABELS: Record<string, string> = {
  mod: "\u2318",
  shift: "\u21e7",
  alt: "\u2325",
};

const OTHER_LABELS: Record<string, string> = {
  mod: "Ctrl",
  shift: "Shift",
  alt: "Alt",
};

/** A chord as it should appear on screen, for the current platform. */
export function formatChord(chord: string): string {
  const parts = chord.split("+");
  const key = parts.pop() ?? "";
  const labels = parts.map((part) =>
    isMac() ? (MAC_LABELS[part] ?? part) : (OTHER_LABELS[part] ?? part),
  );
  labels.push(
    key.length <= 1
      ? key.toUpperCase()
      : (KEY_LABELS[key] ?? key[0]!.toUpperCase() + key.slice(1)),
  );
  return labels.join(isMac() ? "" : " ");
}

/** The chord bound to a command, if any. Used to label palette entries. */
export function chordForCommand(
  command: string,
  bindings: readonly KeyBinding[] = DEFAULT_BINDINGS,
): string | null {
  return bindings.find((binding) => binding.command === command)?.chord ?? null;
}

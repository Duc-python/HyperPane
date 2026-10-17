/** A runnable action, shown in the command palette and bound to a chord. */
export interface Command {
  id: string;
  title: string;
  group: string;
  /** Right-aligned note: a chord, or why the command is unavailable. */
  hint: string | null;
  /**
   * Commands that gather more input leave the palette open rather than
   * closing it and asking somewhere else.
   */
  keepOpen?: boolean;
  run: () => void | Promise<void>;
}

/**
 * Rank a command against a query.
 *
 * A prefix match beats an interior match, which beats a subsequence. Lower is
 * never shown; the palette drops anything that scores below zero.
 */
export function scoreCommand(query: string, command: Command): number {
  if (!query) return 0;
  const haystack = command.title.toLowerCase();
  const needle = query.toLowerCase();

  const position = haystack.indexOf(needle);
  if (position === 0) return 1000;
  if (position > 0) return 800 - position;

  // Every character of the query appears in order.
  let matched = 0;
  for (const character of haystack) {
    if (character === needle[matched]) matched += 1;
    if (matched === needle.length) return 400 - haystack.length;
  }
  return -1;
}

/** Commands matching a query, best first. An empty query keeps the input order. */
export function filterCommands(query: string, commands: Command[]): Command[] {
  const trimmed = query.trim();
  if (!trimmed) return commands;

  return commands
    .map((command) => ({ command, score: scoreCommand(trimmed, command) }))
    .filter((entry) => entry.score >= 0)
    .sort((left, right) => right.score - left.score)
    .map((entry) => entry.command);
}

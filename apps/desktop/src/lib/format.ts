/**
 * Display formatting.
 *
 * A header has room for a few characters, so numbers are rounded to the
 * precision a person can actually use: milliseconds under a second, one decimal
 * up to ten seconds, then whole seconds.
 */

/** A duration as it should appear in a pane header. */
export function formatDuration(milliseconds: number): string {
  if (!Number.isFinite(milliseconds) || milliseconds < 0) return "";
  if (milliseconds < 1000) return `${Math.round(milliseconds)}ms`;

  const seconds = milliseconds / 1000;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  if (seconds < 60) return `${Math.round(seconds)}s`;

  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes}m ${rest.toString().padStart(2, "0")}s`;
}

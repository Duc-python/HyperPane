/**
 * Path display.
 *
 * A pane header has about 500 pixels. A full path does not fit and the useful
 * part is at the end, so a path is shown as `~/…/b/c`: home replaced, leading
 * segments dropped. The full path belongs in the tooltip.
 */

/** Separators are normalised so a Windows path and a POSIX path shorten alike. */
function toSlashes(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

/**
 * Shorten `path` for display, keeping at most `keep` trailing segments.
 *
 * `home` is the user's home directory; when it is not known the path is shown
 * relative to nothing rather than guessed at.
 */
export function shortenPath(path: string, home: string | null, keep = 2): string {
  const slashed = toSlashes(path);
  if (!slashed) return path;

  let display = slashed;
  if (home) {
    const homeSlashed = toSlashes(home);
    if (homeSlashed && (slashed === homeSlashed || slashed.startsWith(`${homeSlashed}/`))) {
      display = `~${slashed.slice(homeSlashed.length)}`;
    }
  }

  const parts = display.split("/").filter((part) => part.length > 0);
  if (parts.length <= keep) return display;

  const tail = parts.slice(-keep).join("/");
  return `${display.startsWith("/") ? "/\u2026/" : "\u2026/"}${tail}`;
}

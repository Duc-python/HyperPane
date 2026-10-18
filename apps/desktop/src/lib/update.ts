import { paneList, paneWrite } from "./ipc";

/**
 * Bringing this build's own source up to date.
 *
 * The app is running from a checkout, so "update" means the checkout rather than
 * a downloaded artifact: pull, install, build. Tauri's own updater cannot be used
 * here - it needs a signing key kept forever and a published endpoint of signed
 * artifacts - and TODO.md records what that would take.
 *
 * Windows locks a running executable, so while this window is open the binary
 * cannot be replaced: cargo links, then fails at the last step with "failed to
 * remove file ... Access is denied", which is what was measured on this machine.
 * So the compile is only *checked* while the app runs, and the note at the end
 * names the one command that finishes the job once the window is closed. macOS
 * and Linux have no such lock: there the build runs and the new binary is waiting
 * for the next start.
 */

/**
 * Joins steps so that the first failure stops the rest.
 *
 * This matters more than it looks: sent one per line, a pull that fails is
 * followed by an install and a build that succeed, and a person reads a green
 * pane and believes a revision arrived that never did. Each guard is the shell's
 * own idea of success - `$?` in PowerShell, `&&` in a POSIX shell - so nothing
 * here has to interpret an exit code by hand.
 */
export function guardedChain(steps: string[], platform: string): string {
  const [first, ...rest] = steps;
  if (first === undefined) return "";

  if (platform === "windows") {
    // Windows PowerShell 5.1 predates `&&`, so the guard is the older spelling.
    return [first, ...rest.map((step) => `if ($?) { ${step} }`)].join("; ");
  }

  return [first, ...rest].join(" && ");
}

/** The line an update pane runs, in the idiom of this machine's shell. */
export function updateCommand(platform: string): string {
  const steps = ["git pull --ff-only", "pnpm install"];

  if (platform === "windows") {
    return guardedChain(
      [
        ...steps,
        "& .\\scripts\\with-msvc.ps1 -- cargo check -p hyperpane",
        "Write-Host 'HyperPane: the source is current and it compiles. " +
          "Close this window, then finish with: " +
          "& .\\scripts\\with-msvc.ps1 -- cargo build -p hyperpane'",
      ],
      platform,
    );
  }

  return guardedChain([...steps, "cargo build -p hyperpane"], platform);
}

/** What the palette promises before anything runs. */
export function updateHint(platform: string): string {
  return platform === "windows"
    ? "pull, install and check here — the build finishes after this window closes"
    : "pull, install and build here";
}

/** How many polls, and how long apart, before a pane is called missing. */
const PANE_POLLS = 60;
const PANE_POLL_MS = 50;

/**
 * Types a line into a pane once its shell exists.
 *
 * A pane is spawned by the view that draws it, so a pane the app has just asked
 * for is not in the backend's list yet and a write would race the mount. The list
 * is the backend's own answer to "is this shell there", so that is what this waits
 * on - and it gives up rather than waiting forever, because a pane that never
 * comes up is something to report, not to hang on.
 *
 * The line arrives as input, so the shell runs it exactly as if it had been typed:
 * it is in the scrollback, and the exit status the pane shows afterwards is the
 * shell's own.
 */
export async function typeIntoPane(
  id: string,
  line: string,
  options: { polls?: number; delayMs?: number } = {},
): Promise<boolean> {
  const polls = options.polls ?? PANE_POLLS;
  const delayMs = options.delayMs ?? PANE_POLL_MS;

  for (let attempt = 0; attempt < polls; attempt += 1) {
    const live = await paneList().catch((): string[] => []);
    if (live.includes(id)) {
      try {
        // The carriage return is what makes the shell run it; a terminal has no
        // separate "execute".
        await paneWrite(id, `${line}\r`);
        return true;
      } catch {
        return false;
      }
    }
    await new Promise((resolve) => {
      window.setTimeout(resolve, delayMs);
    });
  }

  return false;
}

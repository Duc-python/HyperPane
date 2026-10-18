/**
 * Editing rules for profiles.
 *
 * Everything here is pure, so the rules a person relies on — an id nothing
 * else is using, a default that survives a deletion, a saved environment that
 * means what it said — can be read without rendering anything.
 */

import type { Profile } from "./ipc";

/**
 * A name turned into an id that nothing else has.
 *
 * An id is identity: palette commands, saved panes and the session file refer
 * to a profile by it. So it is derived once, when the profile is created, and
 * never rewritten when the profile is renamed.
 */
export function freeProfileId(name: string, taken: readonly string[]): string {
  const base = slug(name) || "profile";
  if (!taken.includes(base)) return base;
  let suffix = 2;
  while (taken.includes(`${base}-${suffix}`)) suffix += 1;
  return `${base}-${suffix}`;
}

function slug(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The profile id that points at this build's source, made on first use. */
export const SELF_PROFILE = "hyperpane-source";

/**
 * The profile list with a profile that points at this build's source.
 *
 * Made once and then kept, because a person may change the shell it uses. The
 * directory is the one part that is not theirs to keep: the source is where this
 * build came from, so a checkout that moved is followed rather than remembered
 * wrongly. Returns the list it was given when nothing needs to change, so a
 * caller can hand it to a state setter without an edit every time.
 */
export function ensureSelfProfile(profiles: Profile[], source: string): Profile[] {
  const existing = profiles.find((profile) => profile.id === SELF_PROFILE);

  if (!existing) {
    return [
      ...profiles,
      {
        id: SELF_PROFILE,
        name: "HyperPane source",
        shell: null,
        args: [],
        cwd: source,
        env: {},
        integration: true,
      },
    ];
  }

  if (existing.cwd === source) return profiles;

  return profiles.map((profile) =>
    profile.id === SELF_PROFILE ? { ...profile, cwd: source } : profile,
  );
}

/**
 * The profile each pane opens with, after a restart.
 *
 * The layout says where the panes were, not how they were opened, so the choice
 * is stored beside it. A stored choice wins; a pane the file does not mention
 * opens with the default profile, and so does one whose profile has since been
 * deleted. A pane has to open with something: a shell with the wrong profile
 * beats a pane that never comes back.
 */
export function panesWithProfiles(
  panes: readonly string[],
  stored: Readonly<Record<string, string>>,
  available: readonly Profile[],
  fallback: string,
): Record<string, string> {
  const known = new Set(available.map((profile) => profile.id));
  return Object.fromEntries(
    panes.map((pane) => {
      const chosen = stored[pane];
      return [pane, chosen && known.has(chosen) ? chosen : fallback];
    }),
  );
}

/** A new profile with a name and an id that nothing else is using. */export function createProfile(existing: readonly Profile[]): Profile {
  const taken = existing.map((profile) => profile.id);
  const names = new Set(existing.map((profile) => profile.name));
  let number = existing.length + 1;
  while (names.has(`Profile ${number}`)) number += 1;
  const name = `Profile ${number}`;

  return {
    id: freeProfileId(name, taken),
    name,
    shell: null,
    args: [],
    cwd: null,
    env: {},
    integration: true,
  };
}

/**
 * The list without one profile, with the default moved on when that was it.
 *
 * `null` when nothing can be removed: the last profile is what the palette
 * falls back to, and removing it would only make the session loader put a
 * default back, which would look like the deletion did not take.
 */
export function removeProfile(
  profiles: readonly Profile[],
  defaultProfile: string | null,
  id: string,
): { profiles: Profile[]; defaultProfile: string | null } | null {
  if (profiles.length <= 1) return null;
  const remaining = profiles.filter((profile) => profile.id !== id);
  if (remaining.length === profiles.length) return null;

  return {
    profiles: remaining,
    defaultProfile:
      defaultProfile === id ? (remaining[0]?.id ?? null) : defaultProfile,
  };
}

/**
 * Argument lines as arguments.
 *
 * One per line rather than split on spaces, because an argument is allowed to
 * contain a space and guessing where one ends is how `--name "two words"`
 * becomes three arguments.
 */
export function parseArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export function formatArgs(args: readonly string[]): string {
  return args.join("\n");
}

export interface EnvEdit {
  env: Record<string, string>;
  /** Lines that are not `KEY=value`, kept so the editor can say which. */
  ignored: string[];
}

/**
 * Environment lines as a map.
 *
 * A line without `=` is not an environment variable, and guessing what it
 * meant would set the wrong thing silently, so it is reported instead. A
 * `#` line is treated as a note and dropped without complaint, because that
 * is what it is.
 */
export function parseEnv(text: string): EnvEdit {
  const env: Record<string, string> = {};
  const ignored: string[] = [];

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;

    const at = line.indexOf("=");
    const key = at === -1 ? "" : line.slice(0, at).trim();
    if (at === -1 || key === "") {
      ignored.push(line);
    } else {
      env[key] = line.slice(at + 1).trim();
    }
  }

  return { env, ignored };
}

/** The map as lines, in one order, so the box reads the same every opening. */
export function formatEnv(env: Record<string, string>): string {
  return Object.keys(env)
    .sort()
    .map((key) => `${key}=${env[key] ?? ""}`)
    .join("\n");
}

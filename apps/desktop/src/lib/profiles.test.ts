import { describe, expect, it } from "vitest";
import type { Profile } from "./ipc";
import {
  SELF_PROFILE,
  createProfile,
  ensureSelfProfile,
  formatArgs,
  formatEnv,
  freeProfileId,
  parseArgs,
  parseEnv,
  removeProfile,
} from "./profiles";

function profile(id: string, name = id): Profile {
  return {
    id,
    name,
    shell: null,
    args: [],
    cwd: null,
    env: {},
    integration: true,
  };
}

describe("ids", () => {
  it("derives one from the name", () => {
    expect(freeProfileId("Profile 2", [])).toBe("profile-2");
    expect(freeProfileId("  My Shell  ", [])).toBe("my-shell");
  });

  it("never reuses one that is taken", () => {
    expect(freeProfileId("Profile 2", ["profile-2"])).toBe("profile-2-2");
    expect(freeProfileId("Profile 2", ["profile-2", "profile-2-2"])).toBe("profile-2-3");
  });

  it("still has an id when the name has no letters left", () => {
    expect(freeProfileId("***", [])).toBe("profile");
  });
});

describe("the profile that points at the app's own source", () => {
  it("is made on first use, with the source as its directory", () => {
    const next = ensureSelfProfile([], "C:\\src\\HyperPane");

    expect(next).toHaveLength(1);
    expect(next[0]?.id).toBe(SELF_PROFILE);
    expect(next[0]?.name).toBe("HyperPane source");
    expect(next[0]?.cwd).toBe("C:\\src\\HyperPane");
    // The platform's shell and the prompt hooks: it is an ordinary pane.
    expect(next[0]?.shell).toBeNull();
    expect(next[0]?.integration).toBe(true);
  });

  it("follows a checkout that moved, and keeps the shell someone chose", () => {
    const edited: Profile = {
      ...profile(SELF_PROFILE, "My HyperPane"),
      shell: "pwsh.exe",
      cwd: "C:\\old\\HyperPane",
    };

    const next = ensureSelfProfile([edited], "C:\\new\\HyperPane");

    expect(next[0]?.cwd).toBe("C:\\new\\HyperPane");
    expect(next[0]?.name).toBe("My HyperPane");
    expect(next[0]?.shell).toBe("pwsh.exe");
  });

  it("changes nothing when the directory is already right", () => {
    const existing: Profile = {
      ...profile(SELF_PROFILE, "HyperPane source"),
      cwd: "C:\\src\\HyperPane",
    };
    const list = [existing];

    // The same array back: a caller can hand this to a state setter and React
    // will not re-render for nothing.
    expect(ensureSelfProfile(list, "C:\\src\\HyperPane")).toBe(list);
  });
});

describe("adding a profile", () => {
  it("starts from Profile 1", () => {
    const created = createProfile([]);
    expect(created.name).toBe("Profile 1");
    expect(created.id).toBe("profile-1");
    expect(created.integration).toBe(true);
    expect(created.shell).toBeNull();
  });

  it("does not repeat a name that is already there", () => {
    const created = createProfile([profile("profile-1", "Profile 1")]);
    expect(created.name).toBe("Profile 2");
  });

  it("does not repeat a name another profile already has", () => {
    // A profile renamed to "Profile 2" keeps whatever id it had, so the name is
    // taken even though the default id for that name is not.
    const created = createProfile([profile("mine", "Profile 2")]);
    expect(created.name).toBe("Profile 3");
    expect(created.id).toBe("profile-3");
  });
});

describe("removing a profile", () => {
  const one = profile("one");
  const two = profile("two");

  it("refuses to remove the last one", () => {
    expect(removeProfile([one], "one", "one")).toBeNull();
  });

  it("refuses an id that is not there", () => {
    expect(removeProfile([one, two], "one", "three")).toBeNull();
  });

  it("moves the default on when the default is what went", () => {
    const next = removeProfile([one, two], "one", "one");
    expect(next?.profiles.map((entry) => entry.id)).toEqual(["two"]);
    expect(next?.defaultProfile).toBe("two");
  });

  it("leaves the default alone when something else went", () => {
    const next = removeProfile([one, two], "one", "two");
    expect(next?.defaultProfile).toBe("one");
  });
});

describe("arguments", () => {
  it("are one per line, so a space inside one is not a separator", () => {
    expect(parseArgs('--name "two words"\n--verbose\n\n')).toEqual([
      '--name "two words"',
      "--verbose",
    ]);
  });

  it("survive a round trip through the box", () => {
    const args = ["-lc", "echo hi"];
    expect(parseArgs(formatArgs(args))).toEqual(args);
  });
});

describe("environment lines", () => {
  it("reads KEY=value", () => {
    expect(parseEnv("FOO=1\nBAR=two words").env).toEqual({ FOO: "1", BAR: "two words" });
  });

  it("drops comments without complaining", () => {
    const edit = parseEnv("# a note\nFOO=1");
    expect(edit.env).toEqual({ FOO: "1" });
    expect(edit.ignored).toEqual([]);
  });

  it("reports a line that is not an assignment instead of guessing", () => {
    const edit = parseEnv("JUST_A_WORD\n=no-key\nFOO=1");
    expect(edit.env).toEqual({ FOO: "1" });
    expect(edit.ignored).toEqual(["JUST_A_WORD", "=no-key"]);
  });

  it("keeps an empty value, which is a real value", () => {
    expect(parseEnv("EMPTY=").env).toEqual({ EMPTY: "" });
  });

  it("reads back in one order, so the box does not move", () => {
    expect(formatEnv({ B: "2", A: "1" })).toBe("A=1\nB=2");
  });
});

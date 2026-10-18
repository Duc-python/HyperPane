import { describe, expect, it } from "vitest";
import { filterCommands, scoreCommand, type Command } from "./commands";

function command(title: string, id = title): Command {
  return { id, title, group: "test", hint: null, run: () => undefined };
}

describe("ranking a command against a query", () => {
  it("puts a prefix first, an interior match next, letters in order last", () => {
    expect(scoreCommand("split", command("Split pane right"))).toBe(1000);
    expect(scoreCommand("pane", command("Split pane right"))).toBeGreaterThan(0);
    expect(scoreCommand("pane", command("Split pane right"))).toBeLessThan(1000);
    expect(scoreCommand("spr", command("Split pane right"))).toBeGreaterThan(0);
    expect(scoreCommand("spr", command("Split pane right"))).toBeLessThan(
      scoreCommand("pane", command("Split pane right")),
    );
  });

  it("scores a command that does not match at all as a miss", () => {
    expect(scoreCommand("zzz", command("Split pane right"))).toBe(-1);
  });

  it("ignores case", () => {
    expect(scoreCommand("SPLIT", command("Split pane right"))).toBe(1000);
  });

  it("scores everything equally when there is no query", () => {
    expect(scoreCommand("", command("Split pane right"))).toBe(0);
  });
});

describe("filtering", () => {
  const commands = [
    command("Split pane right"),
    command("Split pane down"),
    command("Close focused pane"),
    command("Profiles\u2026"),
  ];

  it("keeps the list as it is when nothing is typed", () => {
    expect(filterCommands("", commands).map((entry) => entry.title)).toEqual(
      commands.map((entry) => entry.title),
    );
    expect(filterCommands("   ", commands)).toHaveLength(commands.length);
  });

  it("drops what does not match and keeps the rest in rank order", () => {
    const found = filterCommands("pane", commands).map((entry) => entry.title);
    expect(found).toContain("Split pane right");
    expect(found).toContain("Split pane down");
    expect(found).toContain("Close focused pane");
    expect(found).not.toContain("Profiles\u2026");
  });

  it("puts an exact prefix ahead of an interior match", () => {
    const found = filterCommands("close", [command("Re-close it"), command("Close focused pane")]);
    expect(found[0]?.title).toBe("Close focused pane");
  });
});

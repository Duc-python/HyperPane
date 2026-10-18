import { describe, expect, it } from "vitest";
import {
  DEFAULT_BINDINGS,
  chordForCommand,
  commandForEvent,
  formatChord,
  matchesChord,
} from "./keybindings";

function press(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", init);
}

describe("matching a chord", () => {
  it("matches Control where the chord says mod", () => {
    expect(
      matchesChord(press({ key: "p", ctrlKey: true, shiftKey: true }), "mod+shift+p"),
    ).toBe(true);
  });

  it("does not match when a modifier is missing", () => {
    expect(matchesChord(press({ key: "p", ctrlKey: true }), "mod+shift+p")).toBe(false);
  });

  it("does not match the wrong key", () => {
    expect(
      matchesChord(press({ key: "k", ctrlKey: true, shiftKey: true }), "mod+shift+p"),
    ).toBe(false);
  });

  it("does not match when an extra modifier is held", () => {
    // Command+K must not fire on Control+K, so the other meta key is excluded
    // rather than ignored.
    expect(
      matchesChord(
        press({ key: "p", ctrlKey: true, shiftKey: true, metaKey: true }),
        "mod+shift+p",
      ),
    ).toBe(false);
    expect(
      matchesChord(
        press({ key: "p", ctrlKey: true, shiftKey: true, altKey: true }),
        "mod+shift+p",
      ),
    ).toBe(false);
  });

  it("compares named keys the way the browser spells them", () => {
    expect(matchesChord(press({ key: "Tab", ctrlKey: true }), "mod+tab")).toBe(true);
    expect(
      matchesChord(press({ key: "ArrowUp", ctrlKey: true, shiftKey: true }), "mod+shift+up"),
    ).toBe(true);
  });

  it("matches a chord that is a punctuation key", () => {
    expect(matchesChord(press({ key: ",", ctrlKey: true }), "mod+,")).toBe(true);
  });
});

describe("the binding table", () => {
  it("binds every chord once", () => {
    const chords = DEFAULT_BINDINGS.map((binding) => binding.chord);
    expect(new Set(chords).size).toBe(chords.length);
  });

  it("leaves Control+K alone, because a shell uses it for kill-line", () => {
    expect(commandForEvent(press({ key: "k", ctrlKey: true }))).toBeNull();
  });

  it("answers with the command for a chord", () => {
    expect(commandForEvent(press({ key: "p", ctrlKey: true, shiftKey: true }))).toBe(
      "app.palette",
    );
    expect(commandForEvent(press({ key: ",", ctrlKey: true }))).toBe("app.profiles");
  });

  it("finds a command's chord so the palette can show it", () => {
    expect(chordForCommand("app.profiles")).toBe("mod+,");
    // A command with no chord is a legitimate answer, not a missing one.
    expect(chordForCommand("app.plugins")).toBeNull();
  });
});

describe("showing a chord", () => {
  it("spells it the way the platform spells it", () => {
    // This is not a Mac, so mod is Control.
    expect(formatChord("mod+shift+p")).toBe("Ctrl Shift P");
    expect(formatChord("mod+,")).toBe("Ctrl ,");
    expect(formatChord("mod+shift+up")).toBe("Ctrl Shift \u2191");
  });
});

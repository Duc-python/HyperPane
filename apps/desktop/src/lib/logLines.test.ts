import { describe, expect, it } from "vitest";
import { lineSeverity, transcriptLines } from "./logLines";

describe("reading a line's severity", () => {
  it("recognises the words loggers use, at the start of a line", () => {
    expect(lineSeverity("ERROR: could not bind port 4123")).toBe("error");
    expect(lineSeverity("  fatal: not a git repository")).toBe("error");
    expect(lineSeverity("failed to connect")).toBe("error");
    expect(lineSeverity("WARN  retrying in 2s")).toBe("warn");
    expect(lineSeverity("Warning: deprecated flag")).toBe("warn");
  });

  it("leaves everything else alone", () => {
    expect(lineSeverity("INFO  listening on :5173")).toBe("plain");
    expect(lineSeverity("added 12 packages")).toBe("plain");
    // A word that merely starts with one of these is not the word itself.
    expect(lineSeverity("ERRORS: 0")).toBe("plain");
    // And a severity in the middle of a sentence is prose, not a log level.
    expect(lineSeverity("the error was mine")).toBe("plain");
  });
});

describe("transcript lines", () => {
  it("drops escape sequences, which would otherwise read as corruption", () => {
    expect(transcriptLines("\u001b[32m+\u001b[0m added a line")).toEqual([
      "+ added a line",
    ]);
  });

  it("drops the blank lines that come with a terminal's line endings", () => {
    expect(transcriptLines("one\r\n\r\ntwo\r\n")).toEqual(["one", "two"]);
  });
});

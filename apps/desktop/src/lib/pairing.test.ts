import { describe, expect, it } from "vitest";
import {
  cleanCode,
  groupCode,
  looksLikeCode,
  splitPairingText,
} from "./pairing";

describe("a pairing code", () => {
  it("reads in groups of four", () => {
    // Twelve symbols is long enough that one run of them is hard to keep your
    // place in; four at a time is how it gets read out.
    expect(groupCode("ABCDEFGHJKMN")).toBe("ABCD EFGH JKMN");
  });

  it("is the same code with spaces, dashes or lower case", () => {
    expect(cleanCode("ab-cd ef gh jkmn")).toBe("ABCDEFGHJKMN");
    expect(looksLikeCode("abcd-efgh-jkmn")).toBe(true);
  });

  it("cannot contain the symbols the alphabet leaves out", () => {
    // 0, O, 1, I and L are out because they are read as each other.
    expect(looksLikeCode("ABCDEFGHJKM0")).toBe(false);
    expect(looksLikeCode("ABCDEFGHJKMO")).toBe(false);
  });

  it("is not eleven symbols or thirteen", () => {
    expect(looksLikeCode("ABCDEFGHJKM")).toBe(false);
    expect(looksLikeCode("ABCDEFGHJKMNM")).toBe(false);
  });
});

describe("a pasted pairing", () => {
  it("splits the address from the code", () => {
    expect(splitPairingText("192.168.1.4:51234 abcd-efgh-jkmn")).toEqual({
      address: "192.168.1.4:51234",
      code: "ABCDEFGHJKMN",
    });
  });

  it("takes the code in one group or three", () => {
    expect(splitPairingText("10.0.0.7:4123 ABCDEFGHJKMN")).toEqual({
      address: "10.0.0.7:4123",
      code: "ABCDEFGHJKMN",
    });
  });

  it("leaves an address alone while it is being typed", () => {
    // One word is never a pairing line, and a half-typed address must keep what
    // the person typed.
    expect(splitPairingText("192.168.1.4:51234")).toBeNull();
    expect(splitPairingText("192.168.1.4:51234 ABCD")).toBeNull();
  });

  it("refuses a line whose first word has no port", () => {
    // An address is host:port; "my-laptop" would leave the join with nothing to
    // connect to, so it is not split and not silently rewritten.
    expect(splitPairingText("my-laptop ABCDEFGHJKMN")).toBeNull();
  });

  it("says nothing at all about an empty box", () => {
    expect(splitPairingText("   ")).toBeNull();
  });
});

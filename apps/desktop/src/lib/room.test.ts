import { describe, expect, it } from "vitest";
import {
  controllablePanes,
  participantCount,
  roomChip,
  roomSummary,
  sameRoomView,
} from "./room";
import type { RoomView } from "./ipc";

function visiting(access: Array<"view" | "control"> = ["control"]): RoomView {
  return {
    mode: "visiting",
    port: null,
    me: "p1",
    room: {
      id: "room",
      participants: [
        { id: "host", name: "Duc", focus: "p1", following: null, host: true },
        { id: "p1", name: "Mai", focus: null, following: "host", host: false },
      ],
      panes: access.map((tier, index) => ({ pane: `p${index + 1}`, access: tier })),
      grants: [],
    },
    transcript: [],
    refused: null,
  };
}

describe("asking whether a poll changed anything", () => {
  it("says yes for two views that are the same", () => {
    expect(sameRoomView(visiting(), visiting())).toBe(true);
    expect(sameRoomView(null, null)).toBe(true);
  });

  it("says no when a field moved", () => {
    const before = visiting();
    const after = visiting();
    after.transcript = [{ pane: "p1", text: "output\n" }];
    expect(sameRoomView(before, after)).toBe(false);
    expect(sameRoomView(before, null)).toBe(false);
    expect(sameRoomView(null, before)).toBe(false);
  });
});

describe("what a room lets this instance do", () => {
  it("lists the panes it may type into, and only those", () => {
    expect(controllablePanes(visiting(["view", "control"])).map((p) => p.pane)).toEqual([
      "p2",
    ]);
    expect(controllablePanes(visiting(["view"]))).toEqual([]);
    expect(controllablePanes(null)).toEqual([]);
  });

  it("counts the people in the snapshot", () => {
    expect(participantCount(visiting())).toBe(2);
    expect(participantCount(null)).toBe(0);
  });
});

describe("the room as a chip", () => {
  it("says there is no room rather than showing a zero", () => {
    expect(roomChip(null)).toBe("no room");
    expect(roomChip({ ...visiting(), mode: "off", room: null })).toBe("no room");
  });

  it("names the mode and how many people are in it", () => {
    expect(roomChip(visiting())).toBe("visiting · 2");
  });

  it("explains where a hosted room lives, and what it is not", () => {
    const hosted: RoomView = { ...visiting(), mode: "hosting", port: 4123, me: "host" };
    expect(roomChip(hosted)).toBe("hosting · 2");
    expect(roomSummary(hosted)).toContain("127.0.0.1:4123");
    expect(roomSummary(hosted)).toContain("Loopback only");
    expect(roomSummary(null)).toContain("Nobody is in a room");
  });
});

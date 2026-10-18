/**
 * The room as the rest of the shell needs it.
 *
 * The control room dock is no longer the only thing that shows a room: the top
 * bar carries a chip and the sidebar lists the people. Those three read the
 * same snapshot, so what it means is decided here rather than in each of them.
 */

import type { RoomPaneView, RoomView } from "./ipc";

/**
 * Whether two polls said the same thing.
 *
 * The app polls `room_state` every second and the answer is plain JSON built in
 * one place, so comparing the serialized form is both exact and cheap. A deep
 * walk would be a second description of the type, to be kept in step with the
 * first every time the room grows a field.
 */
export function sameRoomView(a: RoomView | null, b: RoomView | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Panes this instance may type into.
 *
 * Empty unless the room itself says otherwise: the host decides who may type,
 * so this is a reading of its grants rather than a local opinion.
 */
export function controllablePanes(view: RoomView | null): RoomPaneView[] {
  return (view?.room?.panes ?? []).filter((pane) => pane.access === "control");
}

/** How many people the snapshot names, this instance included. */
export function participantCount(view: RoomView | null): number {
  return view?.room?.participants.length ?? 0;
}

/** The room in a couple of words, for a chip. */
export function roomChip(view: RoomView | null): string {
  if (!view || view.mode === "off") return "no room";
  return `${view.mode} · ${participantCount(view)}`;
}

/** The chip's tooltip: what this instance is doing in a room, and where it is. */
export function roomSummary(view: RoomView | null): string {
  if (!view || view.mode === "off") {
    return "Nobody is in a room. Open the control room to host one on this machine or to join one.";
  }
  const people = participantCount(view);
  const count = people === 1 ? "1 person is in it" : `${people} people are in it`;
  if (view.mode === "hosting") {
    return `This instance hosts a room on 127.0.0.1:${view.port}. ${count}. Loopback only: a network is the next step, not this one.`;
  }
  return `This instance visits a room as ${view.me}. ${count}.`;
}

/**
 * The words a pairing needs, worked out once.
 *
 * One side shows a code that was made on that machine; the other side reads one
 * that was typed or pasted. Both are the same twelve symbols from an alphabet
 * without 0/O or 1/I/L, so a code that is read out loud cannot become a
 * different code on the way across the room.
 */

/** Mirrors `CODE_ALPHABET` in `hp-identity`. */
const ALPHABET = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const LENGTH = 12;

/** The code without the spaces and dashes people add when reading it out. */
export function cleanCode(text: string): string {
  return text.replace(/[\s-]/g, "").toUpperCase();
}

/** A code in groups of four, which is how a person reads it out. */
export function groupCode(text: string): string {
  const cleaned = cleanCode(text);
  const groups: string[] = [];
  for (let at = 0; at < cleaned.length; at += 4) {
    groups.push(cleaned.slice(at, at + 4));
  }
  return groups.join(" ");
}

/**
 * Whether a string is a whole code.
 *
 * Spaces, dashes and lower case are forgiven: they are how a person writes a
 * code down, not part of it.
 */
export function looksLikeCode(text: string): boolean {
  const cleaned = cleanCode(text);
  return (
    cleaned.length === LENGTH &&
    [...cleaned].every((symbol) => ALPHABET.includes(symbol))
  );
}

/**
 * Split the line two machines end up passing around, if that is what this is.
 *
 * People copy "192.168.1.4:51234 ABCD-EFGH-IJKL" as one piece, so one field can
 * take both answers. `null` means "not that shape", and the caller keeps what
 * was typed instead of emptying a field somebody is still filling in.
 */
export function splitPairingText(
  text: string,
): { address: string; code: string } | null {
  const trimmed = text.trim();
  // A single word is an address being typed, never a pairing line.
  if (!trimmed.includes(" ")) return null;

  const parts = trimmed.split(/\s+/);
  // The code is the tail: one group of twelve, or three groups of four.
  for (let at = parts.length - 1; at >= 1; at -= 1) {
    const tail = parts.slice(at).join("");
    if (!looksLikeCode(tail)) continue;

    const head = parts.slice(0, at).join("");
    // An address is one word with a port in it; anything else is not a pairing
    // line and guessing at it would put a code where an address belongs.
    if (head === "" || !head.includes(":")) return null;
    return { address: head, code: cleanCode(tail) };
  }

  return null;
}

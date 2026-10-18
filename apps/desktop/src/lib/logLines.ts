/**
 * Reading someone else's output.
 *
 * A visitor's transcript is a pane's raw output with its control sequences
 * removed, so anything drawn from it is a guess about text. The guess here is
 * deliberately narrow: a severity counts only as a whole word at the start of a
 * line, which is the convention loggers follow and prose rarely does. Colouring
 * a paragraph amber because it happened to begin with "Warning" would be a
 * worse failure than colouring nothing at all.
 */

export type LogSeverity = "plain" | "warn" | "error";

const ERROR_WORDS = /^(error|fatal|failed|failure|panic|traceback)\b/i;
const WARN_WORDS = /^(warn|warning|deprecated)\b/i;

/** What a line of output looks like at a glance. */
export function lineSeverity(line: string): LogSeverity {
  const trimmed = line.trimStart();
  if (ERROR_WORDS.test(trimmed)) return "error";
  if (WARN_WORDS.test(trimmed)) return "warn";
  return "plain";
}

/**
 * Drop terminal escape sequences and blank lines.
 *
 * A pane's output is written for a terminal and this is a `<pre>`: a control
 * sequence would be shown as punctuation, which reads like corruption. Blank
 * lines go too — a block is read at a glance, and a run of them in a 200px card
 * is only scrollbar.
 */
export function transcriptLines(text: string): string[] {
  return stripEscapes(text)
    .split(/\r?\n/)
    .filter((line) => line.trim() !== "");
}

function stripEscapes(text: string): string {
  return text
    .replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "")
    .replace(/\x1b\][^\x07]*\x07/g, "");
}

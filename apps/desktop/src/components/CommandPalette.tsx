import { useEffect, useMemo, useRef, useState } from "react";
import { filterCommands, type Command } from "../lib/commands";

interface CommandPaletteProps {
  open: boolean;
  commands: Command[];
  onClose: () => void;
}

export function CommandPalette({ open, commands, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const results = useMemo(
    () => filterCommands(query, commands),
    [query, commands],
  );

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
    // Focus after the paint, or the caret lands before the input is mounted.
    const frame = requestAnimationFrame(() => inputRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [open]);

  useEffect(() => {
    if (index >= results.length) setIndex(0);
  }, [results.length, index]);

  if (!open) return null;

  const run = (command: Command | undefined) => {
    if (!command) return;
    onClose();
    void command.run();
  };

  const move = (delta: number) => {
    if (results.length === 0) return;
    setIndex((current) => (current + delta + results.length) % results.length);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[12vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="w-[min(580px,92vw)] overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Type a command…"
          aria-label="Command"
          className="w-full border-b border-line bg-transparent px-4 py-3 text-[13px] text-ink outline-none placeholder:text-ink-faint"
          onKeyDown={(event) => {
            switch (event.key) {
              case "Escape":
                event.preventDefault();
                onClose();
                break;
              case "ArrowDown":
                event.preventDefault();
                move(1);
                break;
              case "ArrowUp":
                event.preventDefault();
                move(-1);
                break;
              case "Enter":
                event.preventDefault();
                run(results[index]);
                break;
              default:
                break;
            }
          }}
        />

        <ul className="max-h-[46vh] overflow-y-auto py-1">
          {results.length === 0 ? (
            <li className="px-4 py-6 text-center text-[12px] text-ink-faint">
              no matching command
            </li>
          ) : (
            results.map((command, position) => (
              <li key={command.id}>
                <button
                  type="button"
                  data-active={position === index}
                  onMouseEnter={() => setIndex(position)}
                  onClick={() => run(command)}
                  className="flex w-full items-center gap-3 px-4 py-2 text-left text-[12px] transition-colors data-[active=true]:bg-overlay"
                >
                  <span className="w-20 shrink-0 text-[10px] tracking-wide text-ink-faint uppercase">
                    {command.group}
                  </span>
                  <span className="truncate text-ink">{command.title}</span>
                  {command.hint ? (
                    <kbd className="ml-auto shrink-0 rounded border border-line bg-raised px-1.5 py-px text-[9px] text-ink-muted">
                      {command.hint}
                    </kbd>
                  ) : null}
                </button>
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  );
}

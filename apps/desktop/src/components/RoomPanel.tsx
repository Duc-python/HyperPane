import { useEffect, useRef, useState } from "react";
import {
  roomFocus,
  roomFollow,
  roomGrant,
  roomHost,
  roomInput,
  roomJoin,
  roomRevoke,
  roomShare,
  roomState,
  roomStop,
  roomUnshare,
  type RoomView,
} from "../lib/ipc";

/**
 * A room, from either end.
 *
 * Hosting: this instance listens, shares one of its panes, and decides who may
 * watch and who may type. Visiting: it joins someone else's room, sees what it
 * is allowed to see, and types if it may.
 *
 * The transcript is text, not a terminal: escape sequences are dropped rather
 * than interpreted, because a viewer is reading someone else's screen and a
 * half-drawn full-screen program is worse than plain lines.
 */
export function RoomPanel({
  focusedPane,
  onClose,
}: {
  /** The pane this instance is looking at, if any. */
  focusedPane: string | null;
  onClose: () => void;
}) {
  const [view, setView] = useState<RoomView | null>(null);
  const [port, setPort] = useState("4123");
  const [address, setAddress] = useState("127.0.0.1:4123");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const tail = useRef<HTMLDivElement | null>(null);
  /**
   * What is in the command box.
   *
   * A ref, not state: this panel redraws every second from the poll, and a
   * controlled input would be reset by a redraw that happened between typing and
   * pressing send. Nothing here needs to render per keystroke.
   */
  const typed = useRef<HTMLInputElement | null>(null);

  const act = (call: () => Promise<RoomView>) => {
    setBusy(true);
    setError(null);
    void call()
      .then(setView)
      .catch((cause: unknown) => setError(String(cause)))
      .finally(() => setBusy(false));
  };

  // The room changes without this panel asking: someone joins, types, or the
  // transcript grows. One second is faster than a person reads and slower than a
  // poll that costs anything.
  useEffect(() => {
    let cancelled = false;
    const read = () => {
      void roomState()
        .then((next) => {
          if (!cancelled) setView(next);
        })
        .catch(() => undefined);
    };
    read();
    const timer = window.setInterval(read, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  // Keep the newest output in sight.
  useEffect(() => {
    const node = tail.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [view?.transcript.length]);

  const mine = view?.room?.panes ?? [];
  const controllable = mine.find((pane) => pane.access === "control");
  const shared = mine.map((pane) => pane.pane);
  const me = view?.room?.participants.find((entry) => entry.id === view.me) ?? null;
  const followed = me?.following ?? null;
  /** The pane the person I follow is looking at, when I may see it. */
  const watching =
    view?.room?.participants.find((entry) => entry.id === followed)?.focus ?? null;

  const send = () => {
    const box = typed.current;
    if (!controllable || !box) return;
    const line = box.value;
    if (line === "") return;
    // A terminal wants the return that ends a line, so it is added here rather
    // than expected from the person.
    act(() => roomInput(controllable.pane, `${line}\r`));
    box.value = "";
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Room"
        className="flex max-h-[80vh] w-[min(760px,94vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">Room</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            {view?.mode === "hosting"
              ? `hosting on 127.0.0.1:${view.port}`
              : view?.mode === "visiting"
                ? `visiting as ${view.me}`
                : "not in a room"}
          </span>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            close
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {error ? (
            <p className="mb-3 rounded border border-danger/40 bg-danger/10 px-2 py-1.5 text-[11px] text-danger">
              {error}
            </p>
          ) : null}

          {view?.refused ? (
            <p className="mb-3 rounded border border-warn/40 bg-warn/10 px-2 py-1.5 text-[11px] text-warn">
              the room refused that: {view.refused}
            </p>
          ) : null}

          {view?.mode === "off" || view === null ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-baseline gap-2">
                <span className="w-24 text-[10px] tracking-wide text-ink-faint uppercase">
                  host
                </span>
                <input
                  value={port}
                  onChange={(event) => setPort(event.target.value)}
                  aria-label="Port"
                  className="w-24 rounded border border-line bg-base px-2 py-1 font-mono text-[11px] text-ink outline-none focus:border-line-strong"
                />
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => act(() => roomHost(Number(port) || 4123))}
                  className="rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink disabled:opacity-40"
                >
                  host on this machine
                </button>
                <span className="text-[10px] text-ink-faint">
                  loopback only: a network is the next step, not this one
                </span>
              </div>

              <div className="flex items-baseline gap-2">
                <span className="w-24 text-[10px] tracking-wide text-ink-faint uppercase">
                  join
                </span>
                <input
                  value={address}
                  onChange={(event) => setAddress(event.target.value)}
                  aria-label="Address"
                  className="w-48 rounded border border-line bg-base px-2 py-1 font-mono text-[11px] text-ink outline-none focus:border-line-strong"
                />
                <input
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder="your name"
                  aria-label="Name"
                  className="w-32 rounded border border-line bg-base px-2 py-1 text-[11px] text-ink outline-none placeholder:text-ink-faint focus:border-line-strong"
                />
                <button
                  type="button"
                  disabled={busy || name.trim() === ""}
                  onClick={() => act(() => roomJoin(address.trim(), name.trim()))}
                  className="rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink disabled:pointer-events-none disabled:opacity-35"
                >
                  join
                </button>
              </div>
            </div>
          ) : null}

          {view?.mode === "hosting" ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-baseline gap-2">
                <button
                  type="button"
                  disabled={busy || focusedPane === null}
                  onClick={() => focusedPane && act(() => roomShare(focusedPane))}
                  className="rounded border border-accent-strong bg-accent-soft px-2.5 py-1 text-[11px] text-ink hover:border-accent disabled:pointer-events-none disabled:opacity-40"
                >
                  share the focused pane
                </button>
                <span className="text-[10px] text-ink-faint">
                  {focusedPane ? `that is ${focusedPane}` : "no pane has focus"}
                </span>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => act(roomStop)}
                  className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
                >
                  stop hosting
                </button>
              </div>

              <section>
                <h3 className="text-[10px] tracking-wide text-ink-faint uppercase">
                  shared panes
                </h3>
                {shared.length === 0 ? (
                  <p className="text-[11px] text-ink-muted">
                    None yet. Nothing leaves this machine until a pane is shared.
                  </p>
                ) : (
                  <ul className="mt-1 flex flex-wrap gap-1">
                    {shared.map((pane) => (
                      <li key={pane} className="flex items-center gap-1">
                        <span className="rounded border border-line px-1.5 py-0.5 font-mono text-[10px] text-accent">
                          {pane}
                        </span>
                        <button
                          type="button"
                          onClick={() => act(() => roomUnshare(pane))}
                          className="text-[9px] text-ink-faint hover:text-ink"
                        >
                          unshare
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section>
                <h3 className="text-[10px] tracking-wide text-ink-faint uppercase">
                  people
                </h3>
                <ul className="mt-1 flex flex-col gap-1">
                  {(view.room?.participants ?? []).map((participant) => (
                    <li key={participant.id} className="flex flex-wrap items-baseline gap-2">
                      <span className="text-[11px] text-ink">
                        {participant.name}
                        <span className="ml-1 font-mono text-[9px] text-ink-faint">
                          {participant.id}
                        </span>
                      </span>
                      {participant.host ? (
                        <span className="text-[9px] tracking-wide text-ink-faint uppercase">
                          host
                        </span>
                      ) : shared.length > 0 ? (
                        shared.map((pane) => {
                          // What this person may do here, from the host's own
                          // record of it rather than from what the host may do.
                          const held =
                            view.room?.grants.find(
                              (grant) =>
                                grant.pane === pane && grant.who === participant.id,
                            )?.access ?? null;
                          return (
                            <span key={pane} className="flex items-baseline gap-1">
                              <span className="font-mono text-[9px] text-ink-faint">
                                {pane}
                              </span>
                              <span
                                className={`rounded border px-1 text-[9px] ${
                                  held === "control"
                                    ? "border-ok/50 bg-ok/10 text-ok"
                                    : held === "view"
                                      ? "border-line text-ink-muted"
                                      : "border-line/60 text-ink-faint"
                                }`}
                              >
                                {held ?? "no access"}
                              </span>
                              <button
                                type="button"
                                onClick={() =>
                                  act(() => roomGrant(pane, participant.id, "view"))
                                }
                                className="rounded border border-line px-1 text-[9px] text-ink-muted hover:border-line-strong hover:text-ink"
                              >
                                can watch
                              </button>
                              <button
                                type="button"
                                onClick={() =>
                                  act(() => roomGrant(pane, participant.id, "control"))
                                }
                                className="rounded border border-line px-1 text-[9px] text-ink-muted hover:border-line-strong hover:text-ink"
                              >
                                can type
                              </button>
                              <button
                                type="button"
                                onClick={() => act(() => roomRevoke(pane, participant.id))}
                                disabled={held === null}
                                className="text-[9px] text-ink-faint hover:text-danger disabled:opacity-30"
                                title={`take away whatever they have on ${pane}`}
                              >
                                revoke
                              </button>
                            </span>
                          );
                        })
                      ) : null}
                    </li>
                  ))}
                </ul>
              </section>
            </div>
          ) : null}

          {view?.mode === "visiting" ? (
            <div className="flex flex-col gap-3">
              <div className="flex items-baseline gap-2">
                <span className="text-[10px] tracking-wide text-ink-faint uppercase">
                  you may see
                </span>
                <span className="font-mono text-[10px] text-accent">
                  {mine.length === 0
                    ? "nothing yet"
                    : mine.map((pane) => `${pane.pane} (${pane.access})`).join(", ")}
                </span>
                <button
                  type="button"
                  disabled={busy || mine.length === 0}
                  onClick={() => act(() => roomFocus(mine[0]?.pane ?? null))}
                  className="rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink disabled:pointer-events-none disabled:opacity-35"
                >
                  say I am looking at it
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => act(roomStop)}
                  className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
                >
                  leave
                </button>
              </div>

              <section>
                <h3 className="text-[10px] tracking-wide text-ink-faint uppercase">
                  people
                </h3>
                <ul className="mt-1 flex flex-col gap-1">
                  {(view.room?.participants ?? []).map((participant) => (
                    <li key={participant.id} className="flex flex-wrap items-baseline gap-2">
                      <span className="text-[11px] text-ink">
                        {participant.name}
                        <span className="ml-1 font-mono text-[9px] text-ink-faint">
                          {participant.id}
                        </span>
                      </span>
                      {participant.focus ? (
                        <span className="font-mono text-[9px] text-ink-faint">
                          looking at {participant.focus}
                        </span>
                      ) : null}
                      {participant.id === view.me ? (
                        <span className="text-[9px] tracking-wide text-ink-faint uppercase">
                          you
                        </span>
                      ) : (
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() =>
                            act(() =>
                              roomFollow(followed === participant.id ? null : participant.id),
                            )
                          }
                          className="rounded border border-line px-1 text-[9px] text-ink-muted hover:border-line-strong hover:text-ink disabled:opacity-40"
                        >
                          {followed === participant.id ? "stop following" : "follow"}
                        </button>
                      )}
                      {followed === participant.id ? (
                        <span className="text-[9px] tracking-wide text-accent uppercase">
                          following
                        </span>
                      ) : null}
                    </li>
                  ))}
                </ul>
              </section>

              <div
                ref={tail}
                aria-label="Transcript"
                className="max-h-56 min-h-24 overflow-auto rounded border border-line/60 bg-base px-2 py-1"
              >
                {view.transcript.length === 0 ? (
                  <span className="font-mono text-[11px] text-ink-faint">
                    nothing yet: the host shares a pane and output arrives here
                  </span>
                ) : (
                  view.transcript.map((line) => (
                    <section key={line.pane} className="mb-1 last:mb-0">
                      <div className="flex items-baseline gap-2">
                        <span className="font-mono text-[9px] text-ink-faint">
                          {line.pane}
                        </span>
                        {watching === line.pane ? (
                          <span className="text-[9px] tracking-wide text-accent uppercase">
                            the one you are following
                          </span>
                        ) : null}
                      </div>
                      <pre
                        className={`overflow-x-auto rounded border px-1.5 py-1 font-mono text-[11px] whitespace-pre-wrap text-ink ${
                          watching === line.pane
                            ? "border-accent-strong bg-accent-soft/20"
                            : "border-line/40"
                        }`}
                      >
                        {strip(line.text)}
                      </pre>
                    </section>
                  ))
                )}
              </div>

              <div className="flex items-baseline gap-2">
                <input
                  ref={typed}
                  defaultValue=""
                  aria-label="Type into the pane"
                  placeholder={
                    controllable
                      ? `a command for ${controllable.pane}`
                      : "you may watch, not type"
                  }
                  disabled={!controllable}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      send();
                    }
                  }}
                  className="min-w-0 flex-1 rounded border border-line bg-base px-2 py-1 font-mono text-[11px] text-ink outline-none placeholder:text-ink-faint focus:border-line-strong disabled:opacity-40"
                />
                <button
                  type="button"
                  disabled={!controllable || busy}
                  onClick={send}
                  className="rounded border border-accent-strong bg-accent-soft px-2.5 py-1 text-[11px] text-ink hover:border-accent disabled:pointer-events-none disabled:opacity-40"
                >
                  send
                </button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

/**
 * Drop terminal escape sequences.
 *
 * A pane's output is written for a terminal, and this is a `<pre>`: a control
 * sequence would be shown as punctuation, which reads like corruption.
 */
function strip(text: string): string {
  return text.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "").replace(/\x1b\][^\x07]*\x07/g, "");
}

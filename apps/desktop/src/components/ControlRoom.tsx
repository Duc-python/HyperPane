import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  roomFocus,
  roomFollow,
  roomGrant,
  roomHost,
  roomInput,
  roomJoin,
  roomRevoke,
  roomShare,
  roomStop,
  roomUnshare,
  type RoomAccess,
  type RoomView,
} from "../lib/ipc";
import { controllablePanes, participantCount } from "../lib/room";
import {
  lineSeverity,
  transcriptLines,
  type LogSeverity,
} from "../lib/logLines";
import { Chip, IconButton } from "./Chrome";

interface ControlRoomProps {
  /** The newest snapshot the app polled, or `null` before the first answer. */
  view: RoomView | null;
  /** The pane this instance is looking at, for the share button. */
  focusedPane: string | null;
  /** What to call a pane, so the room speaks in names rather than ids. */
  paneLabel: (id: string) => string;
  /** The app's copy of the room, kept in step with what this panel learns. */
  onView: (view: RoomView) => void;
  /** Hide the dock. The top bar and the palette can bring it back. */
  onHide: () => void;
}

/**
 * The control room, docked down the right-hand side.
 *
 * A room from either end. Hosting: this instance listens, shares one of its
 * panes, and decides who may watch and who may type. Visiting: it joins someone
 * else's room, sees what it is allowed to see, and types if it may.
 *
 * The transcript is text, not a terminal: escape sequences are dropped rather
 * than interpreted, because a viewer is reading someone else's screen and a
 * half-drawn full-screen program is worse than plain lines.
 *
 * The poll lives in the app rather than here. The chip in the top bar and the
 * rail both say what the room is doing, and three pollers would be three
 * answers to the same question.
 */
export function ControlRoom({
  view,
  focusedPane,
  paneLabel,
  onView,
  onHide,
}: ControlRoomProps) {
  const [port, setPort] = useState("4123");
  const [address, setAddress] = useState("127.0.0.1:4123");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Which pane the composer writes into, when there is a choice. */
  const [chosen, setChosen] = useState<string | null>(null);
  /**
   * What is in the composer.
   *
   * A ref, not state: the panel redraws every second from the app's poll, and a
   * controlled input would be reset by a redraw that happened between typing
   * and pressing send. Nothing here needs to render per keystroke.
   */
  const typed = useRef<HTMLInputElement | null>(null);
  const tail = useRef<HTMLDivElement | null>(null);

  const act = (call: () => Promise<RoomView>) => {
    setBusy(true);
    setError(null);
    void call()
      .then(onView)
      .catch((cause: unknown) => setError(String(cause)))
      .finally(() => setBusy(false));
  };

  const mine = view?.room?.panes ?? [];
  const controllables = controllablePanes(view);
  const recipient =
    controllables.find((pane) => pane.pane === chosen) ?? controllables[0] ?? null;
  const me = view?.room?.participants.find((entry) => entry.id === view.me) ?? null;
  const followed = me?.following ?? null;
  /** The pane the person followed is looking at, when this instance may see it. */
  const watching =
    view?.room?.participants.find((entry) => entry.id === followed)?.focus ?? null;
  const people = view?.room?.participants ?? [];
  const shared = mine.map((pane) => pane.pane);
  const mode = view?.mode ?? "off";
  /** How much of a transcript arrived: what tells the tail to follow it. */
  const transcriptSize =
    view?.transcript.reduce((total, line) => total + line.text.length, 0) ?? 0;

  // Keep the newest output in sight.
  useEffect(() => {
    const node = tail.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [transcriptSize]);

  const send = () => {
    const box = typed.current;
    if (!recipient || !box) return;
    const line = box.value;
    if (line === "") return;
    // A terminal wants the return that ends a line, so it is added here rather
    // than expected from the person.
    act(() => roomInput(recipient.pane, `${line}\r`));
    box.value = "";
  };

  return (
    <aside
      aria-label="Control room"
      className="flex w-[236px] shrink-0 flex-col border-l border-line/60 bg-surface/25"
    >
      <header className="flex h-8 shrink-0 items-center gap-1.5 border-b border-line/60 px-2">
        <h2 className="text-[11px] font-semibold text-ink">Control Room</h2>
        <Chip
          tone={mode === "off" ? "plain" : "accent"}
          title={
            mode === "off"
              ? "Nobody is in a room"
              : `${participantCount(view)} people in this room, including this instance`
          }
        >
          {mode === "off"
            ? "off"
            : view?.room
              ? participantCount(view)
              : "…"}
        </Chip>
        <span className="ml-auto">
          <IconButton label="Hide the control room" onClick={onHide}>
            <path d="M5 3l3.5 3L5 9" />
          </IconButton>
        </span>
      </header>

      <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-2">
        {error ? (
          <p className="mb-2 rounded border border-danger/40 bg-danger/10 px-2 py-1 text-[10px] text-danger">
            {error}
          </p>
        ) : null}

        {view?.refused ? (
          <p className="mb-2 rounded border border-warn/40 bg-warn/10 px-2 py-1 text-[10px] text-warn">
            the room refused that: {view.refused}
          </p>
        ) : null}

        {mode === "off" ? (
          <>
            <p className="text-[10px] leading-4 text-ink-faint">
              Nothing leaves this machine until a pane is shared. Hosting listens
              on loopback only: a network is the next step, not this one.
            </p>

            <Section title="Host">
              <input
                value={port}
                onChange={(event) => setPort(event.target.value)}
                aria-label="Port"
                className={INPUT}
              />
              {/*
                Below the field rather than beside it: this column is about 230px
                wide, and a button whose text does not fit is a button that lies
                about what it says.
              */}
              <div className="mt-1 flex">
                <Button
                  label="host on this machine"
                  disabled={busy}
                  onClick={() => act(() => roomHost(Number(port) || 4123))}
                />
              </div>
            </Section>

            <Section title="Join">
              <input
                value={address}
                onChange={(event) => setAddress(event.target.value)}
                aria-label="Address"
                className={INPUT}
              />
              <input
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="your name"
                aria-label="Name"
                className={`${INPUT} mt-1`}
              />
              <div className="mt-1 flex">
                <Button
                  label="join"
                  disabled={busy || name.trim() === ""}
                  onClick={() => act(() => roomJoin(address.trim(), name.trim()))}
                />
              </div>
            </Section>
          </>
        ) : null}

        {mode === "hosting" ? (
          <>
            <Section
              title="This instance"
              actions={
                <p className="font-mono text-[9px] text-ink-faint">
                  127.0.0.1:{view?.port}
                </p>
              }
            >
              <Button
                label="share the focused pane"
                primary
                disabled={busy || focusedPane === null}
                onClick={() => focusedPane && act(() => roomShare(focusedPane))}
              />
              <p className="mt-1 text-[9px] text-ink-faint">
                {focusedPane
                  ? `that is ${paneLabel(focusedPane)}`
                  : "no pane has focus"}
              </p>
              <Button
                label="stop hosting"
                disabled={busy}
                onClick={() => act(roomStop)}
              />
            </Section>

            <Section title="Shared panes" count={shared.length}>
              {shared.length === 0 ? (
                <p className="text-[10px] text-ink-faint">
                  None yet. Nothing leaves this machine until a pane is shared.
                </p>
              ) : (
                <ul className="flex flex-col gap-1">
                  {shared.map((pane) => (
                    <li
                      key={pane}
                      className="flex items-center gap-1 rounded-md border border-line/50 bg-base/40 px-1.5 py-1"
                    >
                      <span
                        className="min-w-0 flex-1 truncate font-mono text-[10px] text-accent"
                        title={pane}
                      >
                        {paneLabel(pane)}
                      </span>
                      <Button
                        label="unshare"
                        onClick={() => act(() => roomUnshare(pane))}
                      />
                    </li>
                  ))}
                </ul>
              )}
            </Section>

            <Section title="People" count={people.length}>
              <ul className="flex flex-col gap-1.5">
                {people.map((participant) => (
                  <li
                    key={participant.id}
                    className="rounded-md border border-line/50 bg-base/40 p-1.5"
                  >
                    <div className="flex items-baseline gap-1.5">
                      <span className="min-w-0 flex-1 truncate text-[11px] text-ink">
                        {participant.name}
                        <span className="ml-1 font-mono text-[9px] text-ink-faint">
                          {participant.id}
                        </span>
                      </span>
                      {participant.host ? (
                        <span className="shrink-0 text-[8px] tracking-wide text-ink-faint uppercase">
                          host
                        </span>
                      ) : null}
                    </div>

                    {participant.host
                      ? null
                      : shared.map((pane) => {
                          // What this person may do here, from the host's own
                          // record of it rather than from what the host may do.
                          const held =
                            view?.room?.grants.find(
                              (grant) =>
                                grant.pane === pane && grant.who === participant.id,
                            )?.access ?? null;
                          return (
                            <div
                              key={pane}
                              className="mt-1 flex items-center gap-1 border-t border-line/30 pt-1"
                            >
                              <span
                                className="min-w-0 flex-1 truncate font-mono text-[9px] text-ink-faint"
                                title={`${participant.name} in ${pane}`}
                              >
                                {paneLabel(pane)}
                              </span>
                              <Chip tone={held === "control" ? "ok" : "plain"}>
                                {held ?? "no access"}
                              </Chip>
                              <Button
                                label="can watch"
                                disabled={held === "view"}
                                onClick={() =>
                                  act(() =>
                                    roomGrant(pane, participant.id, "view"),
                                  )
                                }
                              />
                              <Button
                                label="can type"
                                disabled={held === "control"}
                                onClick={() =>
                                  act(() =>
                                    roomGrant(pane, participant.id, "control"),
                                  )
                                }
                              />
                              <Button
                                label="revoke"
                                tone="danger"
                                disabled={held === null}
                                title={`take away whatever they have on ${pane}`}
                                onClick={() =>
                                  act(() => roomRevoke(pane, participant.id))
                                }
                              />
                            </div>
                          );
                        })}
                  </li>
                ))}
              </ul>
            </Section>
          </>
        ) : null}

        {mode === "visiting" ? (
          <>
            <Section title="You may see">
              <p className="font-mono text-[10px] text-accent">
                {mine.length === 0
                  ? "nothing yet"
                  : mine.map((pane) => `${pane.pane} (${pane.access})`).join(", ")}
              </p>
              <div className="mt-1 flex flex-wrap items-center gap-1">
                <Button
                  label="say I am looking at it"
                  disabled={busy || mine.length === 0}
                  onClick={() => act(() => roomFocus(mine[0]?.pane ?? null))}
                />
                <Button label="leave" disabled={busy} onClick={() => act(roomStop)} />
              </div>
            </Section>

            <Section title="People" count={people.length}>
              <ul className="flex flex-col gap-0.5">
                {people.map((participant) => (
                  <li
                    key={participant.id}
                    className="flex flex-wrap items-baseline gap-1.5 rounded-md px-1 py-0.5"
                  >
                    <span className="min-w-0 truncate text-[11px] text-ink">
                      {participant.name}
                      <span className="ml-1 font-mono text-[9px] text-ink-faint">
                        {participant.id}
                      </span>
                    </span>
                    {participant.focus ? (
                      <span className="font-mono text-[9px] text-ink-faint">
                        looking at {paneLabel(participant.focus)}
                      </span>
                    ) : null}
                    {participant.id === view?.me ? (
                      <span className="text-[8px] tracking-wide text-ink-faint uppercase">
                        you
                      </span>
                    ) : (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() =>
                          act(() =>
                            roomFollow(
                              followed === participant.id ? null : participant.id,
                            ),
                          )
                        }
                        className="rounded border border-line px-1 text-[9px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink disabled:opacity-40"
                      >
                        {followed === participant.id
                          ? "stop following"
                          : "follow"}
                      </button>
                    )}
                    {followed === participant.id ? (
                      <span className="text-[8px] tracking-wide text-accent uppercase">
                        following
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            </Section>

            <Section title="Output" count={view?.transcript.length ?? 0}>
              <div
                ref={tail}
                className="flex max-h-72 flex-col gap-1.5 overflow-x-hidden overflow-y-auto"
              >
                {(view?.transcript ?? []).map((line) => (
                  <TranscriptCard
                    key={line.pane}
                    pane={line.pane}
                    label={paneLabel(line.pane)}
                    access={mine.find((pane) => pane.pane === line.pane)?.access ?? null}
                    text={line.text}
                    watching={watching === line.pane}
                  />
                ))}
                {(view?.transcript.length ?? 0) === 0 ? (
                  <p className="text-[10px] leading-4 text-ink-faint">
                    nothing yet: the host shares a pane and output arrives here
                  </p>
                ) : null}
              </div>
            </Section>
          </>
        ) : null}
      </div>

      {mode === "visiting" ? (
        <footer className="shrink-0 border-t border-line/60 p-2">
          {controllables.length > 1 ? (
            <label className="flex items-center gap-1 text-[9px] tracking-wide text-ink-faint uppercase">
              to
              <select
                aria-label="Recipient"
                value={recipient?.pane ?? ""}
                onChange={(event) => setChosen(event.target.value)}
                className="min-w-0 flex-1 rounded border border-line bg-base px-1 py-0.5 text-[10px] text-ink-muted outline-none focus:border-line-strong"
              >
                {controllables.map((pane) => (
                  <option key={pane.pane} value={pane.pane}>
                    {paneLabel(pane.pane)}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <p className="text-[9px] text-ink-faint">
              {recipient
                ? `typing into ${paneLabel(recipient.pane)}`
                : "the host has not shared a pane you may type into"}
            </p>
          )}

          <div className="mt-1 flex items-center gap-1">
            <input
              ref={typed}
              defaultValue=""
              aria-label="Type into the pane"
              placeholder={
                recipient ? "Message the pane…" : "you may watch, not type"
              }
              disabled={recipient === null}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  send();
                }
              }}
              className={`${INPUT} disabled:opacity-40`}
            />
            <Button
              label="send"
              primary
              disabled={recipient === null || busy}
              onClick={send}
            />
          </div>
          <p className="mt-1 text-[9px] leading-4 text-ink-faint">
            enter sends the line · every remote keystroke is a line in the
            activity log, the text is not
          </p>
        </footer>
      ) : null}
    </aside>
  );
}

const INPUT =
  "w-full min-w-0 flex-1 rounded border border-line bg-base px-1.5 py-1 font-mono text-[10px] text-ink outline-none placeholder:text-ink-faint focus:border-line-strong";

function Section({
  title,
  count,
  actions,
  children,
}: {
  title: string;
  count?: number | undefined;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="mt-2.5 first:mt-0">
      <header className="flex items-center gap-1.5 pb-1">
        <h3 className="text-[9px] font-medium tracking-[0.08em] text-ink-faint uppercase">
          {title}
        </h3>
        {count !== undefined ? (
          <span className="text-[9px] tabular-nums text-ink-faint">{count}</span>
        ) : null}
        <span className="ml-auto flex items-center gap-1">{actions}</span>
      </header>
      {children}
    </section>
  );
}

/**
 * The panel's one button, in the three weights it needs.
 *
 * `label` is the visible text and the accessible name at once: the tests find
 * these by what they say, and so does anybody reading the panel with a reader.
 */
function Button({
  label,
  title,
  primary,
  tone,
  disabled,
  onClick,
}: {
  label: string;
  title?: string | undefined;
  primary?: boolean | undefined;
  tone?: "danger" | undefined;
  disabled?: boolean | undefined;
  onClick: () => void;
}) {
  const look = primary
    ? "border-accent-strong bg-accent-soft text-ink hover:border-accent"
    : tone === "danger"
      ? "border-line text-ink-faint hover:border-danger/60 hover:text-danger"
      : "border-line text-ink-muted hover:border-line-strong hover:text-ink";
  return (
    <button
      type="button"
      title={title}
      disabled={disabled ?? false}
      onClick={onClick}
      className={`shrink-0 rounded border px-1.5 py-0.5 text-[9px] whitespace-nowrap transition-colors disabled:pointer-events-none disabled:opacity-35 ${look}`}
    >
      {label}
    </button>
  );
}

const SEVERITY: Record<LogSeverity, string> = {
  plain: "text-ink-muted",
  warn: "text-warn",
  error: "text-danger",
};

/**
 * One pane's output as a card.
 *
 * The arrow reads "this pane, to you": the transcript is what arrived here, and
 * the access chip is this instance's own grant, which is the only reason the
 * card exists at all.
 */
function TranscriptCard({
  pane,
  label,
  access,
  text,
  watching,
}: {
  pane: string;
  label: string;
  access: RoomAccess | null;
  text: string;
  watching: boolean;
}) {
  return (
    <article
      className={`rounded-md border bg-base/40 ${
        watching ? "border-accent-strong bg-accent-soft/15" : "border-line/50"
      }`}
    >
      <header className="flex items-center gap-1.5 border-b border-line/40 px-1.5 py-1">
        <span
          aria-hidden
          className="grid size-4 shrink-0 place-items-center rounded-full border border-line bg-raised text-[8px] text-ink-muted uppercase"
        >
          {label.slice(0, 1)}
        </span>
        <span className="min-w-0 flex-1 truncate text-[10px] text-ink" title={pane}>
          {label}
        </span>
        <span className="shrink-0 text-[9px] text-ink-faint">→ you</span>
        {access ? (
          <Chip tone={access === "control" ? "ok" : "plain"}>{access}</Chip>
        ) : null}
      </header>
      {watching ? (
        /*
          The marker is a line of its own rather than a word in the header: this
          is the pane to look at, and the sentence saying so is worth more than
          the six pixels a tag would have saved.
        */
        <p className="border-b border-accent-strong/40 bg-accent-soft/20 px-1.5 py-0.5 text-[8px] tracking-wide text-accent uppercase">
          the one you are following
        </p>
      ) : null}
      <pre className="max-h-44 overflow-x-hidden overflow-y-auto px-1.5 py-1 font-mono text-[10px] leading-4 whitespace-pre-wrap">
        {transcriptLines(text).map((line, index) => (
          <div key={index} className={SEVERITY[lineSeverity(line)]}>
            {line}
          </div>
        ))}
      </pre>
    </article>
  );
}

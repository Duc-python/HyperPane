import { useEffect, useState } from "react";
import {
  pluginsCall,
  pluginsList,
  pluginsOpen,
  pluginsReload,
  type PluginCalled,
  type PluginToolView,
  type PluginsView,
} from "../lib/ipc";

/**
 * The MCP servers a person has declared, and what they offer.
 *
 * Calling a tool needs arguments, and a tool's arguments are described only by
 * its own JSON Schema. Rather than guess at a form, this shows the schema and
 * takes JSON: an honest first cut that a form can replace once a schema is worth
 * rendering.
 */
export function PluginsPanel({ onClose }: { onClose: () => void }) {
  const [view, setView] = useState<PluginsView | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);
  const [tool, setTool] = useState<PluginToolView | null>(null);
  const [args, setArgs] = useState("{}");
  const [called, setCalled] = useState<PluginCalled | null>(null);

  const load = (read: () => Promise<PluginsView>) => {
    setError(null);
    void read()
      .then(setView)
      .catch((cause: unknown) => setError(String(cause)));
  };

  useEffect(() => {
    load(pluginsList);
  }, []);

  const start = (id: string) => {
    setBusy(id);
    setError(null);
    void pluginsOpen(id)
      .then((next) => {
        setView(next);
        setOpenId(id);
      })
      .catch((cause: unknown) => {
        setError(String(cause));
        // The list still has to move on: a plugin that would not start is now
        // broken, and that is worth seeing.
        load(pluginsList);
      })
      .finally(() => setBusy(null));
  };

  const call = () => {
    if (!openId || !tool) return;
    setBusy(`${openId}/${tool.name}`);
    setError(null);
    setCalled(null);
    let parsed: unknown;
    try {
      parsed = JSON.parse(args.trim() === "" ? "{}" : args);
    } catch (cause) {
      setError(`arguments are not JSON: ${String(cause)}`);
      setBusy(null);
      return;
    }

    void pluginsCall(openId, tool.name, parsed)
      .then(setCalled)
      .catch((cause: unknown) => setError(String(cause)))
      .finally(() => setBusy(null));
  };

  const open = view?.servers.find((server) => server.id === openId) ?? null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Plugins"
        className="flex max-h-[80vh] w-[min(820px,94vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">Plugins</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            MCP servers you declared · nothing starts until it is asked for
          </span>
          <button
            type="button"
            onClick={() => load(pluginsReload)}
            className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            reload
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            close
          </button>
        </header>

        <p
          className="shrink-0 truncate border-b border-line/60 px-4 py-2 font-mono text-[10px] text-ink-faint"
          title={view?.path}
        >
          {view?.path ?? "reading…"}
        </p>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {error ? (
            <p className="mb-3 rounded border border-danger/40 bg-danger/10 px-2 py-1.5 text-[11px] text-danger">
              {error}
            </p>
          ) : null}

          {view && view.problems.length > 0 ? (
            <ul className="mb-3 list-disc pl-4 text-[10px] text-warn">
              {view.problems.map((problem) => (
                <li key={problem}>{problem}</li>
              ))}
            </ul>
          ) : null}

          {view && view.servers.length === 0 ? (
            <p className="text-[12px] text-ink-muted">
              No servers yet. Add one to the file above — it is read on reload,
              and nothing in it runs until you open it.
            </p>
          ) : null}

          {view?.servers.map((server) => (
            <section
              key={server.id}
              className="mb-2 rounded border border-line/60 bg-base/40 px-3 py-2"
            >
              <div className="flex items-baseline gap-2">
                <span className="text-[12px] text-ink">{server.title}</span>
                <span
                  className={`rounded border px-1 text-[9px] uppercase ${
                    server.state === "running"
                      ? "border-ok/50 bg-ok/10 text-ok"
                      : server.state === "broken"
                        ? "border-danger/50 bg-danger/10 text-danger"
                        : "border-line text-ink-faint"
                  }`}
                >
                  {server.state}
                </span>
                {server.peer ? (
                  <span className="text-[10px] text-ink-faint">
                    {server.peer}
                    {server.era ? ` · ${server.era} ${server.protocolVersion}` : ""}
                  </span>
                ) : null}
                {!server.enabled ? (
                  <span className="text-[10px] text-ink-faint">switched off</span>
                ) : null}
                <button
                  type="button"
                  disabled={!server.enabled || busy !== null}
                  onClick={() => start(server.id)}
                  className="ml-auto shrink-0 rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink disabled:pointer-events-none disabled:opacity-35"
                >
                  {busy === server.id
                    ? "starting…"
                    : server.state === "running"
                      ? "open"
                      : "start"}
                </button>
              </div>

              <p className="mt-0.5 truncate font-mono text-[10px] text-ink-faint">
                {server.command} {server.args.join(" ")}
              </p>

              {server.problem ? (
                <p className="mt-1 rounded border border-danger/40 bg-danger/10 px-2 py-1 text-[10px] text-danger">
                  {server.problem}
                </p>
              ) : null}

              {server.lastWords.length > 0 && !server.problem ? (
                <p className="mt-1 truncate text-[10px] text-ink-faint" title={server.lastWords.join("\n")}>
                  it said: {server.lastWords[server.lastWords.length - 1]}
                </p>
              ) : null}

              {server.tools.length > 0 ? (
                <ul className="mt-1.5 flex flex-wrap gap-1">
                  {server.tools.map((entry) => (
                    <li key={entry.name}>
                      <button
                        type="button"
                        onClick={() => {
                          setOpenId(server.id);
                          setTool(entry);
                          setCalled(null);
                          setArgs("{}");
                        }}
                        title={entry.description ?? entry.name}
                        data-active={
                          openId === server.id && tool?.name === entry.name
                        }
                        className="rounded border border-line px-1.5 py-0.5 font-mono text-[10px] text-accent transition-colors hover:border-line-strong data-[active=true]:border-accent-strong data-[active=true]:bg-accent-soft/40"
                      >
                        {entry.name}
                      </button>
                    </li>
                  ))}
                </ul>
              ) : null}
            </section>
          ))}

          {open && tool ? (
            <section className="mt-3 rounded border border-line-strong bg-base/60 px-3 py-2">
              <h3 className="font-mono text-[11px] text-accent">
                {open.id} / {tool.name}
              </h3>
              {tool.description ? (
                <p className="mt-0.5 text-[11px] text-ink-muted">{tool.description}</p>
              ) : null}
              <p className="mt-1 text-[10px] tracking-wide text-ink-faint uppercase">
                arguments · the schema this tool declares
              </p>
              <pre className="mt-0.5 max-h-24 overflow-auto rounded border border-line/60 bg-base px-2 py-1 font-mono text-[10px] text-ink-muted">
                {JSON.stringify(tool.inputSchema, null, 2)}
              </pre>
              <textarea
                value={args}
                onChange={(event) => setArgs(event.target.value)}
                rows={3}
                spellCheck={false}
                aria-label="Arguments"
                className="mt-1 w-full rounded border border-line bg-base px-2 py-1 font-mono text-[11px] text-ink outline-none focus:border-line-strong"
              />
              <div className="mt-1 flex items-center gap-2">
                <button
                  type="button"
                  disabled={busy !== null}
                  onClick={call}
                  className="rounded border border-accent-strong bg-accent-soft px-2.5 py-1 text-[11px] text-ink transition-colors hover:border-accent disabled:pointer-events-none disabled:opacity-40"
                >
                  {busy === `${open.id}/${tool.name}` ? "calling…" : "call"}
                </button>
                <span className="text-[10px] text-ink-faint">
                  the call is recorded in the activity log, the arguments are not
                </span>
              </div>

              {called ? (
                <div className="mt-2">
                  {called.kind === "needsInput" ? (
                    <p className="rounded border border-warn/40 bg-warn/10 px-2 py-1 text-[11px] text-warn">
                      it wants input first: {called.asked.join(", ")} — this build
                      cannot answer that yet
                    </p>
                  ) : (
                    <div>
                      {called.isError ? (
                        <p className="text-[10px] tracking-wide text-danger uppercase">
                          the tool reported a failure
                        </p>
                      ) : null}
                      {called.text.map((part, index) => (
                        <pre
                          key={index}
                          className="mt-1 max-h-40 overflow-auto rounded border border-line/60 bg-base px-2 py-1 font-mono text-[11px] whitespace-pre-wrap text-ink"
                        >
                          {part}
                        </pre>
                      ))}
                      {called.other.length > 0 ? (
                        <p className="mt-1 text-[10px] text-ink-faint">
                          {called.other.length} part(s) this build does not
                          render: {called.other.join(", ")}
                        </p>
                      ) : null}
                      {called.structured !== null && called.structured !== undefined ? (
                        <pre className="mt-1 max-h-32 overflow-auto rounded border border-line/60 bg-base px-2 py-1 font-mono text-[10px] text-ink-muted">
                          {JSON.stringify(called.structured, null, 2)}
                        </pre>
                      ) : null}
                    </div>
                  )}
                </div>
              ) : null}
            </section>
          ) : null}
        </div>
      </div>
    </div>
  );
}

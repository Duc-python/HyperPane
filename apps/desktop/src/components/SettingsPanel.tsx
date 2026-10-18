import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  identityInfo,
  peersList,
  pluginsList,
  type AppInfo,
  type IdentityInfo,
  type PeersView,
} from "../lib/ipc";
import { formatChord } from "../lib/keybindings";

interface SettingsPanelProps {
  info: AppInfo | null;
  /** Where the layout and the profiles are stored. */
  sessionFile: string | null;
  /** Where the activity log is written. */
  auditFile: string | null;
  onClose: () => void;
}

/**
 * Settings: where HyperPane keeps what it keeps.
 *
 * Deliberately not a form. There is nothing here to configure that is not
 * already configured where it belongs — profiles in the profile editor, MCP
 * servers in `plugins.json` — so a page of toggles would either duplicate those
 * or invent settings with nothing behind them. What is genuinely hard to find
 * out is *where the files are*, and that is what this answers.
 *
 * The bootstrap does not hand the plugin paths to the shell, so they are read
 * here from the same command the plugins panel reads. Reading the file has no
 * side effects: nothing in it is started until it is opened.
 */
export function SettingsPanel({
  info,
  sessionFile,
  auditFile,
  onClose,
}: SettingsPanelProps) {
  const [plugins, setPlugins] = useState<{
    path: string;
    trustPath: string;
  } | null>(null);
  const [identity, setIdentity] = useState<IdentityInfo | null>(null);
  const [peers, setPeers] = useState<PeersView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [identityProblem, setIdentityProblem] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void pluginsList()
      .then((view) => {
        if (!cancelled) setPlugins({ path: view.path, trustPath: view.trustPath });
      })
      .catch((cause: unknown) => {
        if (!cancelled) setProblem(String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Who this machine is, and who it trusts: two answers that belong together,
  // because a fingerprint with nobody pinned to it is only half the story. They
  // fail apart, though: a key file that cannot be read must not hide the list of
  // machines, and the list must not hide the key.
  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([identityInfo(), peersList()]).then(([who, trusted]) => {
      if (cancelled) return;
      if (who.status === "fulfilled") {
        setIdentity(who.value);
      } else {
        setIdentityProblem(String(who.reason));
      }
      if (trusted.status === "fulfilled") {
        setPeers(trusted.value);
      } else {
        setIdentityProblem((current) => current ?? String(trusted.reason));
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Take the keyboard when this opens. Nothing inside is a field, so without
  // this Escape would land on whatever the person clicked to get here and the
  // dialog would ignore it.
  const dialog = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    dialog.current?.focus();
  }, []);

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        tabIndex={-1}
        className="flex max-h-[80vh] w-[min(620px,92vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl outline-none"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">Settings</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            where things live · there is nothing else to set here yet
          </span>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink"
          >
            close
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          <h3 className="text-[10px] tracking-wide text-ink-faint uppercase">
            This machine
          </h3>
          <dl className="mt-1">
            <Row label="Application">
              {info ? `${info.name} ${info.version}` : "…"}
            </Row>
            <Row label="Platform">{info?.platform ?? "…"}</Row>
            <Row label="Default shell" note="when a profile names none">
              {info?.defaultShell ?? "…"}
            </Row>
            <Row label="Home">{info?.homeDir ?? "not known"}</Row>
            <Row
              label="Identity"
              note="this machine's name: the key itself, in groups of four. Read it to somebody else and compare — a machine that shows a different one is a different machine."
            >
              {identity?.fingerprint ??
                (identityProblem ? "could not be read" : "…")}
            </Row>
          </dl>

          <h3 className="mt-4 text-[10px] tracking-wide text-ink-faint uppercase">
            Files
          </h3>
          <dl className="mt-1">
            <Row label="Session" note="layout, panes and profiles">
              {sessionFile ?? "not known yet"}
            </Row>
            <Row label="Activity log" note="append-only JSON lines, refusals included">
              {auditFile ?? "not known yet"}
            </Row>
            <Row label="MCP servers" note="declared in this file; nothing in it starts on its own">
              {plugins?.path ?? (problem ? "could not be read" : "reading…")}
            </Row>
            <Row label="Approvals" note="the command, arguments and tools that were read and accepted">
              {plugins?.trustPath ?? (problem ? "could not be read" : "reading…")}
            </Row>
            <Row label="Identity file" note="this machine's private key; the account is what protects it">
              {identity?.path ?? (identityProblem ? "could not be read" : "reading…")}
            </Row>
            <Row label="Trusted machines" note="who this machine has paired with">
              {peers?.path ?? (identityProblem ? "could not be read" : "reading…")}
            </Row>
          </dl>

          <h3 className="mt-4 text-[10px] tracking-wide text-ink-faint uppercase">
            Trusted machines
          </h3>
          {identityProblem ? (
            <p className="mt-2 rounded border border-warn/40 bg-warn/10 px-2 py-1 text-[11px] text-warn">
              this machine's own records could not be read: {identityProblem}
            </p>
          ) : null}
          {peers === null ? (
            <p className="mt-1 text-[11px] text-ink-faint">reading…</p>
          ) : peers.peers.length === 0 ? (
            <p className="mt-1 text-[11px] leading-5 text-ink-faint">
              None yet. Pairing is the next step: one machine shows a code, the
              other types it, and the name above is remembered here. Until then a
              room is loopback and unauthenticated, and nothing on that list is
              trusted.
            </p>
          ) : (
            <ul className="mt-1">
              {peers.peers.map((peer) => (
                <li
                  key={peer.id}
                  className="flex items-baseline gap-2 border-b border-line/40 py-1.5 last:border-b-0"
                >
                  <span className="text-[11px] text-ink">{peer.name}</span>
                  <span className="font-mono text-[10px] text-ink-faint">
                    {peer.short}
                  </span>
                  <span className="ml-auto text-[10px] text-ink-faint">
                    paired
                  </span>
                </li>
              ))}
            </ul>
          )}

          {problem ? (
            <p className="mt-2 rounded border border-warn/40 bg-warn/10 px-2 py-1 text-[11px] text-warn">
              the plugin file could not be read: {problem}
            </p>
          ) : null}

          <p className="mt-4 text-[11px] leading-5 text-ink-muted">
            Profiles are edited where a pane is opened — the profile chip in the
            top bar, the rail, or the{" "}
            <span className="text-ink">{formatChord("mod+,")}</span> chord — and
            they are saved into the session file above as they are typed.
            Everything else HyperPane stores is one of these files.
          </p>
        </div>
      </div>
    </div>
  );
}

function Row({
  label,
  note,
  children,
}: {
  label: string;
  note?: string | undefined;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[7rem_1fr] gap-x-2 border-b border-line/40 py-1.5 last:border-b-0">
      <dt className="text-[10px] tracking-wide text-ink-faint uppercase">
        {label}
      </dt>
      <dd className="min-w-0">
        {/* Selectable: a path is the one thing on this page somebody wants to
            copy somewhere else. */}
        <span className="block font-mono text-[11px] break-all select-text text-ink-muted">
          {children}
        </span>
        {note ? (
          <span className="mt-0.5 block text-[10px] text-ink-faint">{note}</span>
        ) : null}
      </dd>
    </div>
  );
}

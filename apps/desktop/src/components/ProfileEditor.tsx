import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Profile } from "../lib/ipc";
import {
  createProfile,
  formatArgs,
  formatEnv,
  parseArgs,
  parseEnv,
  removeProfile,
} from "../lib/profiles";

interface ProfileEditorProps {
  profiles: Profile[];
  /** The profile the palette opens when a pane names none. */
  defaultProfile: string | null;
  /** The shell the platform opens when a profile names none. */
  platformDefaultShell: string | null;
  onChange: (profiles: Profile[]) => void;
  onDefaultChange: (id: string | null) => void;
  /** Open a pane with this profile, so an edit can be tried straight away. */
  onOpen: (id: string) => void;
  onClose: () => void;
}

const INPUT =
  "rounded border border-line bg-base px-2 py-1 text-[12px] text-ink outline-none placeholder:text-ink-faint focus:border-line-strong";

const AREA = `${INPUT} font-mono text-[11px] leading-4`;

const BUTTON =
  "rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted transition-colors hover:border-line-strong hover:text-ink disabled:pointer-events-none disabled:opacity-35";

/**
 * Edit the profile list: one form for the selected profile, one button per
 * action that changes the list.
 *
 * An edit applies as it is typed. The app already saves the session on a
 * debounce, and a pane keeps the profile it was opened with, so nothing here
 * can pull a running shell out from under anyone.
 */
export function ProfileEditor({
  profiles,
  defaultProfile,
  platformDefaultShell,
  onChange,
  onDefaultChange,
  onOpen,
  onClose,
}: ProfileEditorProps) {
  const [selectedId, setSelectedId] = useState<string | null>(defaultProfile);
  const [argsDraft, setArgsDraft] = useState("");
  const [envDraft, setEnvDraft] = useState("");
  /** Lines the last commit could not read, so the box can say which. */
  const [ignored, setIgnored] = useState<string[]>([]);
  const nameRef = useRef<HTMLInputElement | null>(null);
  /** Set while the next selection should take the keyboard. */
  const focusName = useRef(true);

  const selected =
    profiles.find((profile) => profile.id === selectedId) ??
    profiles[0] ??
    null;

  // The textareas hold text until it is committed: parsing on every keystroke
  // would rewrite `FOO=` to `FOO` under the caret and `--flag ` to `--flag`.
  // Reset here, not on every render, so an edit in one box does not clear text
  // that belongs to the profile on screen.
  useEffect(() => {
    const profile =
      profiles.find((entry) => entry.id === selectedId) ?? profiles[0] ?? null;
    setArgsDraft(profile ? formatArgs(profile.args) : "");
    setEnvDraft(profile ? formatEnv(profile.env) : "");
    setIgnored([]);
    if (focusName.current) {
      focusName.current = false;
      nameRef.current?.focus();
    }
    // Keyed on the selection alone: the drafts are about the profile on
    // screen, not about whatever else the list happens to hold.
  }, [selectedId]);

  /** The list with the drafts applied, ready to store. */
  const withDrafts = (list: Profile[]): Profile[] => {
    if (!selected) return list;
    const args = parseArgs(argsDraft);
    const { env } = parseEnv(envDraft);
    return list.map((profile) =>
      profile.id === selected.id ? { ...profile, args, env } : profile,
    );
  };

  const commitDrafts = () => {
    setIgnored(parseEnv(envDraft).ignored);
    onChange(withDrafts(profiles));
  };

  const patch = (change: Partial<Profile>) => {
    if (!selected) return;
    onChange(
      profiles.map((profile) =>
        profile.id === selected.id ? { ...profile, ...change } : profile,
      ),
    );
  };

  const select = (id: string) => {
    if (id === selected?.id) return;
    commitDrafts();
    setSelectedId(id);
  };

  const addProfile = () => {
    const created = createProfile(profiles);
    focusName.current = true;
    onChange([...withDrafts(profiles), created]);
    setSelectedId(created.id);
  };

  const removeSelected = () => {
    if (!selected) return;
    const at = profiles.findIndex((profile) => profile.id === selected.id);
    const next = removeProfile(
      withDrafts(profiles),
      defaultProfile,
      selected.id,
    );
    if (!next) return;

    onChange(next.profiles);
    if (next.defaultProfile !== defaultProfile) {
      onDefaultChange(next.defaultProfile);
    }
    // Select whichever profile took its place, or its neighbour at the end.
    const neighbour =
      next.profiles[Math.min(Math.max(at, 0), next.profiles.length - 1)];
    if (neighbour) setSelectedId(neighbour.id);
  };

  /** A name is what the palette shows, so it is trimmed and never empty. */
  const commitName = () => {
    if (!selected) return;
    const trimmed = selected.name.trim();
    const next = trimmed === "" ? selected.id : trimmed;
    if (next !== selected.name) patch({ name: next });
  };

  const commitShell = () => {
    if (!selected) return;
    const trimmed = selected.shell?.trim() ?? "";
    const next = trimmed === "" ? null : trimmed;
    if (next !== selected.shell) patch({ shell: next });
  };

  const commitCwd = () => {
    if (!selected) return;
    const trimmed = selected.cwd?.trim() ?? "";
    const next = trimmed === "" ? null : trimmed;
    if (next !== selected.cwd) patch({ cwd: next });
  };

  const close = () => {
    commitDrafts();
    onClose();
  };

  const openSelected = () => {
    if (!selected) return;
    commitDrafts();
    onOpen(selected.id);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={close}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Profiles"
        className="flex max-h-[80vh] w-[min(760px,92vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            close();
          }
        }}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">Profiles</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            kept as you type · open panes keep the shell they started with
          </span>
          <button
            type="button"
            onClick={close}
            className="ml-auto rounded border border-line px-2 py-0.5 text-[10px] text-ink-muted hover:border-line-strong hover:text-ink"
          >
            close
          </button>
        </header>

        <div className="flex min-h-0 flex-1">
          <nav
            aria-label="Profiles"
            className="flex w-52 shrink-0 flex-col gap-1 overflow-y-auto border-r border-line p-2"
          >
            {profiles.map((profile) => (
              <button
                key={profile.id}
                type="button"
                title={profile.id}
                data-active={profile.id === selected?.id}
                onClick={() => select(profile.id)}
                className="flex flex-col items-start gap-0.5 rounded border border-transparent px-2 py-1.5 text-left transition-colors hover:border-line-strong hover:bg-overlay/60 data-[active=true]:border-accent-strong data-[active=true]:bg-accent-soft/40"
              >
                <span className="flex w-full items-baseline gap-2">
                  <span className="min-w-0 flex-1 truncate text-[12px] text-ink">
                    {profile.name}
                  </span>
                  {profile.id === defaultProfile ? (
                    <span className="shrink-0 rounded border border-line px-1 text-[8px] tracking-wide text-ink-faint uppercase">
                      default
                    </span>
                  ) : null}
                </span>
                <span className="w-full truncate text-[9px] text-ink-faint">
                  {profile.shell ?? "platform default"}
                  {profile.integration ? "" : " · no hooks"}
                </span>
              </button>
            ))}
          </nav>

          {selected ? (
            <form
              className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4"
              onSubmit={(event) => event.preventDefault()}
            >
              <Field label="Name">
                <input
                  ref={nameRef}
                  value={selected.name}
                  onChange={(event) => patch({ name: event.target.value })}
                  onBlur={commitName}
                  className={INPUT}
                />
              </Field>

              <Field label="Shell" note="empty opens the platform default">
                <input
                  value={selected.shell ?? ""}
                  placeholder={platformDefaultShell ?? "platform default"}
                  onChange={(event) =>
                    patch({
                      shell: event.target.value === "" ? null : event.target.value,
                    })
                  }
                  onBlur={commitShell}
                  spellCheck={false}
                  className={INPUT}
                />
              </Field>

              <Field label="Working directory" note="empty starts at the shell's own">
                <input
                  value={selected.cwd ?? ""}
                  onChange={(event) =>
                    patch({
                      cwd: event.target.value === "" ? null : event.target.value,
                    })
                  }
                  onBlur={commitCwd}
                  spellCheck={false}
                  className={INPUT}
                />
              </Field>

              <Field label="Arguments" note="one per line">
                <textarea
                  value={argsDraft}
                  onChange={(event) => setArgsDraft(event.target.value)}
                  onBlur={commitDrafts}
                  rows={3}
                  spellCheck={false}
                  className={AREA}
                />
              </Field>

              <Field label="Environment" note="KEY=value, one per line">
                <textarea
                  value={envDraft}
                  onChange={(event) => setEnvDraft(event.target.value)}
                  onBlur={commitDrafts}
                  rows={4}
                  spellCheck={false}
                  className={AREA}
                />
              </Field>

              {ignored.length > 0 ? (
                <p className="text-[10px] text-warn">
                  not KEY=value, so not saved: {ignored.join(" · ")}
                </p>
              ) : null}

              <label className="flex items-baseline gap-2 text-[11px] text-ink-muted">
                <input
                  type="checkbox"
                  checked={selected.integration}
                  onChange={(event) =>
                    patch({ integration: event.target.checked })
                  }
                />
                <span>shell integration</span>
                <span
                  className="text-[10px] text-ink-faint"
                  title="HyperPane's prompt hooks report the working directory and where each command began and ended. Only shells the app knows how to instrument — PowerShell and bash — are touched."
                >
                  prompt marks for PowerShell and bash
                </span>
              </label>
            </form>
          ) : null}
        </div>

        <footer className="flex shrink-0 items-center gap-2 border-t border-line px-4 py-3">
          <button type="button" onClick={addProfile} className={BUTTON}>
            new
          </button>
          <button
            type="button"
            onClick={removeSelected}
            disabled={profiles.length <= 1}
            title={
              profiles.length <= 1
                ? "A session needs at least one profile; the loader would put one back anyway."
                : undefined
            }
            className={BUTTON}
          >
            delete
          </button>
          <button
            type="button"
            onClick={() => {
              if (selected) onDefaultChange(selected.id);
            }}
            disabled={!selected || selected.id === defaultProfile}
            className={BUTTON}
          >
            set as default
          </button>
          <button
            type="button"
            onClick={openSelected}
            disabled={!selected}
            className="ml-auto shrink-0 rounded border border-accent-strong bg-accent-soft px-2.5 py-1 text-[11px] text-ink transition-colors hover:border-accent disabled:pointer-events-none disabled:opacity-40"
          >
            open a pane
          </button>
        </footer>
      </div>
    </div>
  );
}

interface FieldProps {
  label: string;
  /** Shown after the label, in the app's usual lower-case voice. */
  note?: string;
  children: ReactNode;
}

function Field({ label, note, children }: FieldProps) {
  return (
    <label className="flex flex-col gap-1">
      <span className="flex items-baseline gap-2 text-[10px] tracking-wide text-ink-faint uppercase">
        {label}
        {note ? (
          <span className="normal-case tracking-normal">{note}</span>
        ) : null}
      </span>
      {children}
    </label>
  );
}

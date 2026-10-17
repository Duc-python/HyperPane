import { useEffect, useState } from "react";
import { teamPlan, type Gate, type TeamPlan } from "../lib/ipc";

interface TeamPlanPanelProps {
  teamKey: string;
  onClose: () => void;
}

const GATE_LABEL: Record<Gate, string> = {
  humanPlan: "you approve the plan",
  checkerPass: "a check has to pass",
  humanCommit: "you keep the result",
};

const KIND_NOTE: Record<string, string> = {
  leader: "coordinates",
  checker: "checks",
  worker: "writes",
};

/**
 * What a team would do, before it does any of it.
 *
 * A preview and nothing more: no pane is opened and no process is started by
 * looking at this. That is the whole point of showing it.
 */
export function TeamPlanPanel({ teamKey, onClose }: TeamPlanPanelProps) {
  const [plan, setPlan] = useState<TeamPlan | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setPlan(null);
    setError(null);
    void teamPlan(teamKey)
      .then((loaded) => {
        if (!cancelled) setPlan(loaded);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(String(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [teamKey]);

  const childCount =
    plan?.waves
      .flatMap((wave) => wave.members)
      .filter((member) => member.kind !== "leader").length ?? 0;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Team plan"
        className="flex max-h-[80vh] w-[min(680px,92vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">
            {plan ? plan.title : teamKey}
          </h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            preview, nothing starts from here
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
            <p className="text-[12px] text-danger">{error}</p>
          ) : !plan ? (
            <p className="text-[12px] text-ink-faint">working out the plan…</p>
          ) : (
            <>
              <p className="text-[12px] text-ink-muted">{plan.purpose}</p>

              <p className="mt-3 text-[10px] text-ink-faint">
                {plan.waves.flatMap((wave) => wave.members).length} members ·
                {" "}
                {childCount} of them opened as panes · a run stops after{" "}
                {plan.stop.maxSpawns} panes, {plan.stop.maxRounds} round
                {plan.stop.maxRounds === 1 ? "" : "s"} or{" "}
                {plan.stop.wallMinutes} minutes
              </p>

              {plan.gates.length > 0 ? (
                <p className="mt-1 text-[10px] text-ink-faint">
                  gates: {plan.gates.map((gate) => GATE_LABEL[gate]).join(" · ")}
                </p>
              ) : null}

              {plan.refusal ? (
                <p className="mt-3 rounded border border-warn/40 bg-warn/10 px-2 py-1.5 text-[11px] text-warn">
                  {plan.refusal}
                </p>
              ) : null}

              {plan.waves.map((wave) => (
                <section key={wave.wave} className="mt-4">
                  <h3 className="text-[10px] tracking-wide text-ink-faint uppercase">
                    wave {wave.wave}
                  </h3>
                  <ul className="mt-1">
                    {wave.members.map((member, position) => (
                      <li
                        key={`${member.role}-${position}`}
                        className="flex items-baseline gap-2 border-b border-line/40 py-1.5 last:border-b-0"
                      >
                        <span className="w-32 shrink-0 truncate text-[11px] text-ink">
                          {member.label}
                        </span>
                        <span className="w-24 shrink-0 truncate text-[10px] text-accent">
                          {member.agent}
                        </span>
                        <span className="w-16 shrink-0 text-[10px] text-ink-faint">
                          {KIND_NOTE[member.kind] ?? member.kind}
                        </span>
                        <span className="min-w-0 flex-1 truncate text-[10px] text-ink-faint">
                          {member.readOnly ? "read-only" : "may write"}
                          {member.worktree ? " · own worktree" : ""}
                        </span>
                        <span className="shrink-0 text-[9px] text-ink-faint">
                          {member.tier}
                        </span>
                      </li>
                    ))}
                  </ul>
                </section>
              ))}

              {plan.problems.length > 0 ? (
                <section className="mt-4">
                  <h3 className="text-[10px] tracking-wide text-warn uppercase">
                    problems with this preset
                  </h3>
                  <ul className="mt-1 list-disc pl-4 text-[10px] text-ink-faint">
                    {plan.problems.map((problem) => (
                      <li key={problem}>{problem}</li>
                    ))}
                  </ul>
                </section>
              ) : null}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

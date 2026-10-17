import { useEffect, useMemo, useState } from "react";
import {
  teamPlan,
  type MemberClaim,
  type PlannedMember,
  type TeamPlan,
  type TeamStatus,
} from "../lib/ipc";
import { describeResult } from "../lib/agents";

interface TeamRunPanelProps {
  teamKey: string;
  /** The live status the app already polls. This panel only reads it. */
  status: TeamStatus;
  onClose: () => void;
}

const KIND_NOTE: Record<string, string> = {
  leader: "coordinates",
  checker: "checks",
  worker: "writes",
};

/** How a member of the plan is doing, as far as anything can tell. */
type MemberState = "pending" | "working" | "reported" | "no-report";

function stateOf(
  claim: { result: { status: string } | null; finished: boolean } | undefined,
): MemberState {
  if (!claim) return "pending";
  if (!claim.finished) return "working";
  if (claim.result) return "reported";
  return "no-report";
}

const STATE_LABEL: Record<MemberState, string> = {
  // A wave that has not been opened yet: the member is not "not opened"
  // forever, it is next.
  pending: "to come",
  working: "working",
  reported: "reported",
  "no-report": "no report",
};

const STATE_COLOUR: Record<MemberState, string> = {
  pending: "text-ink-faint",
  working: "text-accent",
  reported: "text-ok",
  "no-report": "text-warn",
};

/**
 * The waves of a run, in order, with what each member has said so far.
 *
 * A view and nothing else: the buttons that move a run live where the decision
 * is, not here. Reading a timeline should never be able to change one.
 */
export function TeamRunPanel({ teamKey, status, onClose }: TeamRunPanelProps) {
  const [plan, setPlan] = useState<TeamPlan | null>(null);

  useEffect(() => {
    let cancelled = false;
    void teamPlan(teamKey)
      .then((loaded) => {
        if (!cancelled) setPlan(loaded);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [teamKey]);

  const claimForMember = useMemo(() => {
    // A role can be asked for more than once (`count`), and those members are
    // told apart by order alone — in the plan and in the claims alike. Hand the
    // claims out one per slot in that order, rather than by role alone: with
    // two members of one role, matching on the role would show the first
    // member's claim beside both of them.
    const queues = new Map<string, MemberClaim[]>();
    for (const claim of status.claims) {
      const key = `${claim.wave}:${claim.role}`;
      const queue = queues.get(key);
      if (queue) queue.push(claim);
      else queues.set(key, [claim]);
    }

    const byMember = new Map<PlannedMember, MemberClaim>();
    for (const wave of plan?.waves ?? []) {
      for (const member of wave.members) {
        const claim = queues.get(`${wave.wave}:${member.role}`)?.shift();
        if (claim) byMember.set(member, claim);
      }
    }
    return byMember;
  }, [plan, status.claims]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[8vh] backdrop-blur-[2px]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Team run"
        className="flex max-h-[80vh] w-[min(720px,92vw)] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-2xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-baseline gap-3 border-b border-line px-4 py-3">
          <h2 className="text-[13px] font-semibold text-ink">{status.title}</h2>
          <span className="text-[10px] tracking-wide text-ink-faint uppercase">
            wave {status.wave}
            {status.wavesRemaining > 0
              ? ` · ${status.wavesRemaining} to come`
              : " · last wave"}
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
          {!plan ? (
            <p className="text-[12px] text-ink-faint">loading the plan…</p>
          ) : (
            plan.waves.map((wave) => {
              const isCurrent = wave.wave === status.wave;
              const isPast = wave.wave < status.wave;
              // A wave holding nothing but the leader is never opened: the
              // leader is the pane the person is already in, so calling it
              // "done" would claim something ran there.
              const opensNothing = !wave.members.some(
                (member) => member.kind !== "leader",
              );

              return (
                <section key={wave.wave} className="mt-3 first:mt-0">
                  <h3
                    className={`text-[10px] tracking-wide uppercase ${
                      isCurrent ? "text-accent" : "text-ink-faint"
                    }`}
                  >
                    wave {wave.wave}
                    {opensNothing
                      ? " · no panes"
                      : isPast
                        ? " · done"
                        : isCurrent
                          ? " · now"
                          : ""}
                  </h3>
                  <ul className="mt-1">
                    {wave.members.map((member, position) => {
                      const claim = claimForMember.get(member);
                      const state = stateOf(claim);
                      const leader = member.kind === "leader";

                      return (
                        <li
                          key={`${member.role}-${position}`}
                          className="border-b border-line/40 py-1.5 last:border-b-0"
                        >
                          <div className="flex items-baseline gap-2">
                            <span className="w-32 shrink-0 truncate text-[11px] text-ink">
                              {member.label}
                            </span>
                            <span className="w-24 shrink-0 truncate text-[10px] text-accent">
                              {member.agent}
                            </span>
                            <span className="w-16 shrink-0 text-[10px] text-ink-faint">
                              {KIND_NOTE[member.kind] ?? member.kind}
                            </span>
                            <span
                              className={`text-[10px] ${STATE_COLOUR[state]}`}
                              title={
                                leader
                                  ? "the leader is the pane the run is coordinated from, so it is never opened"
                                  : claim?.result
                                    ? `reported: ${describeResult(claim.result)}`
                                    : claim?.finished
                                      ? "exited without printing a RESULT line"
                                      : undefined
                              }
                            >
                              {leader ? "not opened" : STATE_LABEL[state]}
                            </span>
                            {claim?.result ? (
                              <span className="ml-auto shrink-0 text-[10px] text-ink-faint">
                                {describeResult(claim.result)}
                              </span>
                            ) : null}
                          </div>
                          {claim?.findings ? (
                            <pre className="mt-1 max-h-24 overflow-auto rounded border border-line/60 bg-base/60 px-2 py-1 font-mono text-[10px] whitespace-pre-wrap text-ink-faint">
                              {claim.findings}
                            </pre>
                          ) : null}
                        </li>
                      );
                    })}
                  </ul>
                </section>
              );
            })
          )}
        </div>

        <footer className="shrink-0 border-t border-line px-4 py-2 text-[10px] text-ink-faint">
          {status.holding
            ? status.holdingMessage
            : status.running > 0
              ? `${status.running} member(s) still going`
              : "nothing is running; the run moves on when the app is asked to"}
        </footer>
      </div>
    </div>
  );
}

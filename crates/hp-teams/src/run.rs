//! Where a run is, and what should happen next.
//!
//! A run moves wave by wave, and two things can stop it: the wave is still
//! working, or a person has to decide something. Everything here is a pure
//! function of the plan, the gates and how many panes are still alive, so the
//! part that is easy to get wrong can be tested without opening anything.

use serde::{Deserialize, Serialize};

use crate::{Gate, PlannedMember, Preset, RoleKind};

/// Why a run is waiting for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Hold {
    /// A wave of checkers finished. Whether that counts as passing is a
    /// judgement, and the app does not make it.
    ChecksReported,
    /// The last wave finished. The result is the person's to keep or drop.
    ResultReady,
}

impl Hold {
    pub fn message(self) -> &'static str {
        match self {
            Hold::ChecksReported => {
                "the checkers have finished. What each one claimed is beside it; \
                 whether that counts as passing is yours to decide."
            }
            Hold::ResultReady => {
                "the last wave has finished. The result is yours to keep: \
                 nothing is merged, pushed or deployed."
            }
        }
    }
}

/// Where a run is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    /// The wave in progress.
    pub wave: u32,
    /// Waves still to come, in order. The current one is not in here.
    pub pending: Vec<u32>,
    /// The hold the run is sitting on, if it is sitting on one.
    ///
    /// Remembered rather than inferred: which hold applies cannot be worked
    /// out from the wave alone, because a checker wave can also be the last
    /// wave, and then the hold that was raised is the result one.
    holding: Option<Hold>,
    /// Holds a person has already cleared, so neither is raised twice.
    checks_confirmed: bool,
    result_confirmed: bool,
}

impl RunState {
    /// Start at `wave`, with `pending` still to come.
    pub fn new(wave: u32, pending: Vec<u32>) -> Self {
        Self {
            wave,
            pending,
            holding: None,
            checks_confirmed: false,
            result_confirmed: false,
        }
    }

    /// The hold the run is currently sitting on, if any.
    pub fn holding(&self) -> Option<Hold> {
        self.holding
    }

    /// Every wave a plan asks for, in order, ignoring the leader's wave:
    /// the leader is the pane the person is already in, so there is nothing
    /// there to open.
    pub fn waves_of(plan: &[PlannedMember]) -> Vec<u32> {
        let mut waves: Vec<u32> = plan
            .iter()
            .filter(|member| member.kind != RoleKind::Leader)
            .map(|member| member.wave)
            .collect();
        waves.sort_unstable();
        waves.dedup();
        waves
    }

    /// The first wave with something to open.
    pub fn start(plan: &[PlannedMember]) -> Option<Self> {
        let mut waves = Self::waves_of(plan);
        if waves.is_empty() {
            return None;
        }
        let first = waves.remove(0);
        Some(Self::new(first, waves))
    }

    /// What should happen next.
    ///
    /// `running` is how many panes of the current wave are still alive. Nothing
    /// moves while one of them is: a wave is a dependency barrier, which is the
    /// only reason to have waves at all.
    pub fn advance(&mut self, preset: &Preset, plan: &[PlannedMember], running: usize) -> Advance {
        if running > 0 {
            return Advance::Waiting { running };
        }

        let wave_was_checkers = !plan.is_empty()
            && plan
                .iter()
                .filter(|member| member.wave == self.wave)
                .all(|member| member.kind == RoleKind::Checker)
            && plan.iter().any(|member| member.wave == self.wave);

        if wave_was_checkers && preset.gates.contains(&Gate::CheckerPass) && !self.checks_confirmed
        {
            self.holding = Some(Hold::ChecksReported);
            return Advance::Held(Hold::ChecksReported);
        }

        if self.pending.is_empty() {
            if preset.gates.contains(&Gate::HumanCommit) && !self.result_confirmed {
                self.holding = Some(Hold::ResultReady);
                return Advance::Held(Hold::ResultReady);
            }
            return Advance::Finished;
        }

        let next = self.pending.remove(0);
        self.wave = next;
        self.holding = None;
        Advance::Ready { wave: next }
    }

    /// A person cleared whatever the run was waiting on.
    ///
    /// Clears the hold the run actually raised, and nothing else. A call when
    /// no hold is up does nothing, rather than quietly confirming a check that
    /// never happened.
    pub fn confirm(&mut self) {
        match self.holding.take() {
            Some(Hold::ChecksReported) => self.checks_confirmed = true,
            Some(Hold::ResultReady) => self.result_confirmed = true,
            None => {}
        }
    }
}

/// What the app should do next with a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Advance {
    /// Members of the current wave are still alive.
    Waiting { running: usize },
    /// Open this wave.
    Ready { wave: u32 },
    /// A person has to decide before the run goes on.
    Held(Hold),
    /// Nothing left to open.
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Archetype, Stop, Wave};
    use std::collections::BTreeMap;

    fn role(kind: RoleKind, wave: u32, key: &str) -> PlannedMember {
        PlannedMember {
            role: key.to_string(),
            label: key.to_string(),
            wave,
            agent: "opencode".to_string(),
            reason: "the role's first installed choice".to_string(),
            kind,
            tier: crate::Tier::Mid,
            read_only: matches!(kind, RoleKind::Checker | RoleKind::Leader),
            worktree: matches!(kind, RoleKind::Worker),
            focus: "do the thing".to_string(),
        }
    }

    fn preset(gates: Vec<Gate>) -> Preset {
        let mut title = BTreeMap::new();
        title.insert("en".to_string(), "Test team".to_string());
        Preset {
            key: "test".to_string(),
            title,
            purpose: BTreeMap::new(),
            group: None,
            archetype: Archetype::Pipeline,
            gates,
            stop: Stop {
                max_rounds: 1,
                max_spawns: 8,
                wall_minutes: 60,
                on_stall: crate::OnStall::Escalate,
            },
            waves: Vec::<Wave>::new(),
            starter_prompt: None,
        }
    }

    /// A leader in wave 0, a builder in wave 1, a checker in wave 2.
    fn plan() -> Vec<PlannedMember> {
        vec![
            role(RoleKind::Leader, 0, "lead"),
            role(RoleKind::Worker, 1, "build"),
            role(RoleKind::Checker, 2, "check"),
        ]
    }

    #[test]
    fn a_run_starts_at_the_first_wave_with_something_to_open() {
        let state = RunState::start(&plan()).expect("a start");
        assert_eq!(state.wave, 1, "wave 0 is the leader, which is not a spawn");
        assert_eq!(state.pending, vec![2]);
    }

    #[test]
    fn a_plan_of_only_leaders_has_nowhere_to_start() {
        let leaders = vec![role(RoleKind::Leader, 0, "lead")];
        assert!(RunState::start(&leaders).is_none());
    }

    #[test]
    fn nothing_moves_while_a_member_is_alive() {
        let mut state = RunState::start(&plan()).expect("a start");
        assert_eq!(
            state.advance(&preset(vec![]), &plan(), 2),
            Advance::Waiting { running: 2 }
        );
        assert_eq!(state.wave, 1, "the wave did not move");
    }

    #[test]
    fn the_next_wave_opens_once_this_one_is_done() {
        let mut state = RunState::start(&plan()).expect("a start");
        assert_eq!(
            state.advance(&preset(vec![]), &plan(), 0),
            Advance::Ready { wave: 2 }
        );
        assert_eq!(state.wave, 2);
        assert!(state.pending.is_empty());
    }

    #[test]
    fn a_clean_run_finishes_without_a_gate() {
        let mut state = RunState::start(&plan()).expect("a start");
        state.advance(&preset(vec![]), &plan(), 0);
        assert_eq!(
            state.advance(&preset(vec![]), &plan(), 0),
            Advance::Finished
        );
    }

    #[test]
    fn the_last_wave_holds_when_the_result_is_the_persons() {
        let gates = vec![Gate::HumanCommit];
        let mut state = RunState::start(&plan()).expect("a start");
        state.advance(&preset(gates.clone()), &plan(), 0);

        assert_eq!(
            state.advance(&preset(gates.clone()), &plan(), 0),
            Advance::Held(Hold::ResultReady)
        );

        // Clearing it once is enough; asking again would be nagging.
        state.confirm();
        assert_eq!(state.advance(&preset(gates), &plan(), 0), Advance::Finished);
    }

    #[test]
    fn a_wave_of_checkers_holds_when_the_preset_asks_for_a_check() {
        let gates = vec![Gate::CheckerPass];
        let mut state = RunState::new(2, vec![]);
        assert_eq!(
            state.advance(&preset(gates.clone()), &plan(), 0),
            Advance::Held(Hold::ChecksReported)
        );

        state.confirm();
        assert_eq!(state.advance(&preset(gates), &plan(), 0), Advance::Finished);
    }

    #[test]
    fn a_wave_that_is_not_all_checkers_does_not_hold() {
        // Wave 1 holds a builder, so finishing it is not a check result.
        let gates = vec![Gate::CheckerPass];
        let mut state = RunState::new(1, vec![2]);
        assert_eq!(
            state.advance(&preset(gates), &plan(), 0),
            Advance::Ready { wave: 2 }
        );
    }

    #[test]
    fn a_preset_without_the_check_gate_does_not_hold_after_checkers() {
        let mut state = RunState::new(2, vec![]);
        assert_eq!(
            state.advance(&preset(vec![]), &plan(), 0),
            Advance::Finished
        );
    }

    #[test]
    fn the_plan_gate_is_not_a_mid_run_hold_it_is_the_start() {
        // `humanPlan` is answered by a person pressing Start, so a run that is
        // already going must never stop for it again.
        let gates = vec![Gate::HumanPlan];
        let mut state = RunState::start(&plan()).expect("a start");

        assert_eq!(
            state.advance(&preset(gates.clone()), &plan(), 0),
            Advance::Ready { wave: 2 }
        );
        assert_eq!(state.advance(&preset(gates), &plan(), 0), Advance::Finished);
    }

    #[test]
    fn confirming_when_nothing_is_held_does_nothing() {
        // The hold that is up has to be the hold that gets cleared. A stray
        // confirmation must not answer a check nobody has run yet.
        let mut state = RunState::new(2, vec![]);
        state.confirm();
        assert_eq!(state.holding(), None);

        assert_eq!(
            state.advance(&preset(vec![Gate::CheckerPass]), &plan(), 0),
            Advance::Held(Hold::ChecksReported),
            "a confirmation with no hold up answered the check anyway"
        );
    }

    #[test]
    fn a_hold_is_reported_while_it_is_up_and_cleared_afterwards() {
        let gates = vec![Gate::CheckerPass];
        let mut state = RunState::new(2, vec![]);

        assert_eq!(
            state.advance(&preset(gates.clone()), &plan(), 0),
            Advance::Held(Hold::ChecksReported)
        );
        assert_eq!(state.holding(), Some(Hold::ChecksReported));

        state.confirm();
        assert_eq!(state.holding(), None);
        assert_eq!(state.advance(&preset(gates), &plan(), 0), Advance::Finished);
    }

    #[test]
    fn a_wave_is_never_opened_twice() {
        let mut state = RunState::start(&plan()).expect("a start");
        let mut opened = vec![state.wave];

        while let Advance::Ready { wave } = state.advance(&preset(vec![]), &plan(), 0) {
            assert!(!opened.contains(&wave), "wave {wave} opened twice");
            opened.push(wave);
        }

        assert_eq!(opened, vec![1, 2]);
    }

    #[test]
    fn a_waiting_run_reports_the_number_that_is_holding_it() {
        let mut state = RunState::start(&plan()).expect("a start");
        match state.advance(&preset(vec![]), &plan(), 3) {
            Advance::Waiting { running } => assert_eq!(running, 3),
            other => panic!("expected Waiting, got {other:?}"),
        }
    }
}

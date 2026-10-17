//! Choosing which agent takes a role.
//!
//! A preset names agents in preference order, and on a machine with one agent
//! installed that list is the whole story. On a machine with several it is not
//! quite: a role that only reads is worth more when it is not the same agent
//! that read the last thing, because two readings by one agent share that
//! agent's blind spots. That is a fact about the shape of the run, not a claim
//! about which agent is better, which this crate has no way to know.
//!
//! So the preset's order decides, with one exception: reading roles are spread
//! over the installed agents. A role that writes keeps the preset's first
//! installed choice, because spreading writers would change who does the work
//! for no reason anyone could read off the plan.

use std::collections::BTreeSet;

use crate::Role;

/// One agent, and a line saying why it was picked.
///
/// The line is shown beside the choice in the plan preview, so a person can
/// disagree with the router rather than wonder what it was thinking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub agent: String,
    /// Short and lower-case, like the rest of the app's asides.
    pub reason: String,
}

/// Pick the agent for one member of a role.
///
/// `wave_readings` is what the other reading members of this wave have, which
/// is the independence that matters most: three reviewers of one diff sent to
/// one agent is one reading, not three. `run_readings` is the same over the
/// whole run, which breaks a tie in favour of an agent the run has used least.
/// Both are ignored for a role that writes.
pub fn route(
    role: &Role,
    installed: &BTreeSet<String>,
    wave_readings: &[String],
    run_readings: &[String],
) -> Option<Routed> {
    let candidates: Vec<&str> = role
        .agents
        .iter()
        .map(String::as_str)
        .filter(|agent| installed.contains(*agent))
        .collect();

    let first = *candidates.first()?;

    if !role.read_only {
        return Some(Routed {
            agent: first.to_string(),
            reason: "the role's first installed choice".to_string(),
        });
    }

    // Fewest readings in this wave, then fewest in the run, then the preset's
    // own order. Three reading roles that all accept three agents read with
    // three different ones; a fourth is the least used of them.
    let score = |agent: &str| {
        (
            readings_of(wave_readings, agent),
            readings_of(run_readings, agent),
        )
    };

    let mut chosen = first;
    let mut best = score(chosen);
    for candidate in &candidates[1..] {
        let candidate = *candidate;
        let candidate_score = score(candidate);
        if candidate_score < best {
            chosen = candidate;
            best = candidate_score;
        }
    }

    let reason = if chosen == first {
        if best.0 > 0 {
            "every installed choice is already reading this wave, so they share one".to_string()
        } else {
            "the role's first installed choice".to_string()
        }
    } else if best.0 > 0 {
        "every installed choice is already reading this wave, so they share one".to_string()
    } else if readings_of(wave_readings, first) > 0 {
        "moved down the list: another reader in this wave has the first choice".to_string()
    } else {
        "moved down the list: an earlier wave read with the first choice".to_string()
    };

    Some(Routed {
        agent: chosen.to_string(),
        reason,
    })
}

fn readings_of(readings: &[String], agent: &str) -> usize {
    readings
        .iter()
        .filter(|used| used.as_str() == agent)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RoleKind, Tier};

    fn installed(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn role(agents: &[&str], read_only: bool) -> Role {
        Role {
            key: "review".to_string(),
            label: None,
            kind: if read_only {
                RoleKind::Checker
            } else {
                RoleKind::Worker
            },
            agents: agents.iter().map(|agent| (*agent).to_string()).collect(),
            count: 1,
            focus: "Read it.".to_string(),
            read_only,
            worktree: !read_only,
            tier: Tier::Mid,
        }
    }

    fn names(agents: &[&str]) -> Vec<String> {
        agents.iter().map(|agent| (*agent).to_string()).collect()
    }

    #[test]
    fn a_writing_role_takes_the_first_installed_choice() {
        let routed = route(
            &role(&["claude-code", "codex"], false),
            &installed(&["codex", "opencode"]),
            &names(&["opencode"]),
            &names(&["opencode"]),
        )
        .expect("a choice");

        assert_eq!(routed.agent, "codex");
        assert_eq!(routed.reason, "the role's first installed choice");
    }

    #[test]
    fn the_first_reader_of_a_wave_takes_the_first_choice() {
        let routed = route(
            &role(&["claude-code", "codex"], true),
            &installed(&["claude-code", "codex"]),
            &[],
            &[],
        )
        .expect("a choice");

        assert_eq!(routed.agent, "claude-code");
        assert_eq!(routed.reason, "the role's first installed choice");
    }

    #[test]
    fn a_later_wave_moves_off_a_choice_an_earlier_wave_read_with() {
        let routed = route(
            &role(&["claude-code", "codex"], true),
            &installed(&["claude-code", "codex"]),
            &[],
            &names(&["claude-code"]),
        )
        .expect("a choice");

        // Nothing in this wave has read yet, but the run has read with the
        // first choice, so the run is what the spread is measured against.
        assert_eq!(routed.agent, "codex");
        assert_eq!(
            routed.reason,
            "moved down the list: an earlier wave read with the first choice"
        );
    }

    #[test]
    fn a_role_that_names_one_agent_uses_it_in_every_wave() {
        let routed = route(
            &role(&["opencode"], true),
            &installed(&["opencode", "claude-code"]),
            &[],
            &names(&["opencode"]),
        )
        .expect("a choice");

        assert_eq!(routed.agent, "opencode");
        assert_eq!(routed.reason, "the role's first installed choice");
    }

    #[test]
    fn readers_in_one_wave_are_spread_over_the_installed_agents() {
        let installed = installed(&["claude-code", "codex", "opencode"]);
        let reviewers = role(&["opencode", "claude-code", "codex"], true);

        let mut wave: Vec<String> = Vec::new();
        let mut chosen = Vec::new();
        for _ in 0..3 {
            let routed = route(&reviewers, &installed, &wave, &wave).expect("a choice");
            wave.push(routed.agent.clone());
            chosen.push(routed.agent);
        }

        assert_eq!(chosen, ["opencode", "claude-code", "codex"]);
    }

    #[test]
    fn a_reader_leaves_the_first_choice_to_another_reader_and_says_so() {
        let routed = route(
            &role(&["opencode", "claude-code"], true),
            &installed(&["opencode", "claude-code"]),
            &names(&["opencode"]),
            &names(&["opencode"]),
        )
        .expect("a choice");

        assert_eq!(routed.agent, "claude-code");
        assert_eq!(
            routed.reason,
            "moved down the list: another reader in this wave has the first choice"
        );
    }

    #[test]
    fn a_reading_role_with_one_agent_says_that_it_is_sharing() {
        let reviewers = role(&["opencode"], true);
        let installed = installed(&["opencode"]);

        let first = route(&reviewers, &installed, &[], &[]).expect("a choice");
        let second = route(
            &reviewers,
            &installed,
            &names(&["opencode"]),
            &names(&["opencode"]),
        )
        .expect("a choice");

        assert_eq!(first.reason, "the role's first installed choice");
        assert_eq!(second.agent, "opencode");
        assert_eq!(
            second.reason,
            "every installed choice is already reading this wave, so they share one"
        );
    }

    #[test]
    fn a_role_with_nothing_installed_gets_no_choice() {
        assert!(route(
            &role(&["claude-code"], true),
            &installed(&["opencode"]),
            &[],
            &[]
        )
        .is_none());
    }
}

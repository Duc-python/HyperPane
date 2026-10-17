//! Team presets: several agents, an order, and the rules that stop them.
//!
//! A preset is data, not code. `presets/team.schema.json` describes the shape
//! and `presets/teams.json` ships two worked examples; this crate reads them,
//! says what is wrong with them, and turns one into a plan.
//!
//! A plan is only a plan. Nothing here opens a pane, starts a process or spends
//! anything: it says which roles come in which wave, with which agent, and then
//! stops. A person decides whether to run it.

use std::collections::{BTreeMap, BTreeSet};

pub mod brief;

pub use brief::{write_brief, BriefInput};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Text in one or more languages, keyed by language tag.
pub type Localised = BTreeMap<String, String>;

/// The shipped examples, compiled in so the schema and the data cannot drift
/// apart without a test noticing.
pub const SHIPPED: &str = include_str!("../../../presets/teams.json");

#[derive(Debug, Error)]
pub enum PresetError {
    #[error("the preset file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the preset file is version {found}; this build understands version {expected}")]
    Version { found: u32, expected: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetFile {
    pub version: u32,
    pub teams: Vec<Preset>,
}

/// The shape of a run, which sets the expectations a person should have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Archetype {
    Reviewer,
    Pipeline,
    Researcher,
    Drafter,
    Operator,
    Control,
}

/// A checkpoint the run has to clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Gate {
    /// A person approves the plan before any work starts.
    HumanPlan,
    /// An independent check has to pass.
    CheckerPass,
    /// A person decides whether to keep the result.
    HumanCommit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoleKind {
    Leader,
    Checker,
    Worker,
}

/// How much model a role deserves, as a hint for whoever picks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Fast,
    #[default]
    Mid,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub max_rounds: u32,
    pub max_spawns: u32,
    pub wall_minutes: u32,
    #[serde(default)]
    pub on_stall: OnStall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnStall {
    #[default]
    Escalate,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub key: String,
    #[serde(default)]
    pub label: Option<Localised>,
    pub kind: RoleKind,
    /// Agent ids, most preferred first.
    pub agents: Vec<String>,
    #[serde(default = "one")]
    pub count: u32,
    /// The role's standing instruction, which becomes part of the brief.
    pub focus: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub tier: Tier,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wave {
    pub wave: u32,
    pub roles: Vec<Role>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub key: String,
    pub title: Localised,
    pub purpose: Localised,
    #[serde(default)]
    pub group: Option<String>,
    pub archetype: Archetype,
    #[serde(default)]
    pub gates: Vec<Gate>,
    pub stop: Stop,
    pub waves: Vec<Wave>,
    #[serde(default)]
    pub starter_prompt: Option<Localised>,
}

impl Preset {
    /// Every role, in wave order.
    pub fn roles(&self) -> impl Iterator<Item = &Role> {
        self.waves.iter().flat_map(|wave| wave.roles.iter())
    }

    /// Members the preset asks for, counting each role's `count`.
    pub fn requested_members(&self) -> u32 {
        self.roles().map(|role| role.count.max(1)).sum()
    }

    /// Children the preset asks to spawn.
    ///
    /// The leader is the pane the run is coordinated from, not a spawn: a
    /// person opens one pane and the run fills the panes around it. That is why
    /// a preset can describe five members and cap the run at four spawns.
    pub fn requested_children(&self) -> u32 {
        self.roles()
            .filter(|role| role.kind != RoleKind::Leader)
            .map(|role| role.count.max(1))
            .sum()
    }

    /// English text, or the first language available. Presets are written in
    /// English first, so a missing key is a missing translation and not a
    /// reason to show nothing.
    pub fn title_en(&self) -> String {
        pick(&self.title)
    }

    pub fn purpose_en(&self) -> String {
        pick(&self.purpose)
    }
}

/// English if it is there, otherwise the first entry, so a caller never has to
/// handle an empty string for text the schema says must exist.
fn pick(text: &Localised) -> String {
    text.get("en")
        .or_else(|| text.values().next())
        .cloned()
        .unwrap_or_default()
}

impl PresetFile {
    pub fn parse(text: &str) -> Result<Self, PresetError> {
        let file: Self = serde_json::from_str(text)?;
        if file.version != 1 {
            return Err(PresetError::Version {
                found: file.version,
                expected: 1,
            });
        }
        Ok(file)
    }

    pub fn team(&self, key: &str) -> Option<&Preset> {
        self.teams.iter().find(|team| team.key == key)
    }

    /// What is wrong with this file that serde cannot see.
    ///
    /// Nothing here refuses a file. A preset that names an agent this build
    /// does not know is still usable the moment that agent is installed, so the
    /// problem is reported and the caller decides.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let known: BTreeSet<&str> = hp_agents::known_ids().into_iter().collect();
        let mut seen: BTreeSet<&str> = BTreeSet::new();

        for team in &self.teams {
            if !seen.insert(&team.key) {
                problems.push(format!("duplicate team key: {}", team.key));
            }
            if team.waves.is_empty() {
                problems.push(format!("{}: no waves", team.key));
            }
            if team.requested_children() > team.stop.max_spawns {
                problems.push(format!(
                    "{}: asks to spawn {} children but caps the run at {}",
                    team.key,
                    team.requested_children(),
                    team.stop.max_spawns
                ));
            }

            let mut wave_numbers: Vec<u32> = team.waves.iter().map(|wave| wave.wave).collect();
            wave_numbers.sort_unstable();
            if wave_numbers.windows(2).any(|pair| pair[0] == pair[1]) {
                problems.push(format!("{}: two waves share a number", team.key));
            }

            for role in team.roles() {
                if role.agents.is_empty() {
                    problems.push(format!("{}: role {} names no agent", team.key, role.key));
                }
                for agent in &role.agents {
                    if !known.contains(agent.as_str()) {
                        problems.push(format!(
                            "{}: role {} names agent {agent}, which this build does not know",
                            team.key, role.key
                        ));
                    }
                }
            }
        }

        problems
    }

    /// Keys ordered for a picker: group, then key.
    pub fn ordered_keys(&self) -> Vec<&str> {
        let mut teams: Vec<&Preset> = self.teams.iter().collect();
        teams.sort_by(|left, right| {
            left.group
                .as_deref()
                .unwrap_or("")
                .cmp(right.group.as_deref().unwrap_or(""))
                .then_with(|| left.key.cmp(&right.key))
        });
        teams.into_iter().map(|team| team.key.as_str()).collect()
    }
}

/// One member a run would open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedMember {
    pub role: String,
    pub label: String,
    pub wave: u32,
    /// The agent chosen for this member.
    pub agent: String,
    pub kind: RoleKind,
    pub tier: Tier,
    pub read_only: bool,
    pub worktree: bool,
    pub focus: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("role {role} has no agent available: it names {named}")]
    NoAgentForRole { role: String, named: String },
    #[error("the plan spawns {needed} children but the preset caps the run at {cap}")]
    OverCap { needed: u32, cap: u32 },
}

/// Work out who does what, in what order, given which agents are installed.
///
/// A role's agent list is a preference order, so the first one installed wins.
/// Nothing else about the run is decided here: this is the part that can be
/// wrong on its own, so it is the part worth testing.
pub fn plan(
    preset: &Preset,
    installed: &BTreeSet<String>,
) -> Result<Vec<PlannedMember>, PlanError> {
    let mut ordered: Vec<&Wave> = preset.waves.iter().collect();
    ordered.sort_by_key(|wave| wave.wave);

    let mut members = Vec::new();

    for wave in ordered {
        for role in &wave.roles {
            let Some(agent) = role
                .agents
                .iter()
                .find(|candidate| installed.contains(candidate.as_str()))
            else {
                return Err(PlanError::NoAgentForRole {
                    role: role.key.clone(),
                    named: role.agents.join(", "),
                });
            };

            for _ in 0..role.count.max(1) {
                members.push(PlannedMember {
                    role: role.key.clone(),
                    label: role
                        .label
                        .as_ref()
                        .map(pick)
                        .unwrap_or_else(|| role.key.clone()),
                    wave: wave.wave,
                    agent: agent.clone(),
                    kind: role.kind,
                    tier: role.tier,
                    read_only: role.read_only,
                    worktree: role.worktree,
                    focus: role.focus.clone(),
                });
            }
        }
    }

    if members
        .iter()
        .filter(|member| member.kind != RoleKind::Leader)
        .count() as u32
        > preset.stop.max_spawns
    {
        return Err(PlanError::OverCap {
            needed: members
                .iter()
                .filter(|member| member.kind != RoleKind::Leader)
                .count() as u32,
            cap: preset.stop.max_spawns,
        });
    }

    Ok(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    #[test]
    fn the_shipped_presets_parse() {
        let file = PresetFile::parse(SHIPPED).expect("the shipped presets parse");
        assert_eq!(file.version, 1);
        assert!(!file.teams.is_empty());
    }

    #[test]
    fn the_shipped_presets_only_name_agents_this_build_knows() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        // `omp` appears in the original research notes but is not in this
        // catalogue, because its program name was never verified. A preset that
        // names it would be a preset nobody can run.
        let problems = file.problems();
        assert!(
            problems
                .iter()
                .all(|problem| !problem.contains("does not know")),
            "shipped presets name unknown agents: {problems:#?}"
        );
        assert!(
            problems.is_empty(),
            "shipped presets have problems: {problems:#?}"
        );
    }

    #[test]
    fn every_shipped_preset_can_be_planned_for_a_fully_equipped_machine() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        let all: BTreeSet<String> = hp_agents::known_ids()
            .into_iter()
            .map(str::to_string)
            .collect();

        for team in &file.teams {
            let members = plan(team, &all)
                .unwrap_or_else(|error| panic!("{} cannot be planned: {error}", team.key));
            assert_eq!(
                members.len() as u32,
                team.requested_members(),
                "{} planned the wrong number of members",
                team.key
            );
        }
    }

    #[test]
    fn a_plan_keeps_wave_order() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        let team = file.team("feature-build").expect("the build preset");
        let all: BTreeSet<String> = hp_agents::known_ids()
            .into_iter()
            .map(str::to_string)
            .collect();

        let members = plan(team, &all).expect("planned");
        let waves: Vec<u32> = members.iter().map(|member| member.wave).collect();
        let mut sorted = waves.clone();
        sorted.sort_unstable();
        assert_eq!(waves, sorted, "members are not in wave order");
        assert_eq!(waves.first(), Some(&0), "the planner goes first");
    }

    #[test]
    fn every_shipped_preset_can_be_planned_with_a_single_agent_installed() {
        // A preset that needs three specific agents is a preset most people
        // cannot run. `opencode` is the last choice in every role, so the
        // shipped examples work on a machine that has only that one.
        let file = PresetFile::parse(SHIPPED).expect("parse");
        for team in &file.teams {
            let members = plan(team, &installed(&["opencode"])).unwrap_or_else(|error| {
                panic!("{} cannot run with one agent installed: {error}", team.key)
            });
            assert_eq!(members.len() as u32, team.requested_members());
        }
    }

    #[test]
    fn a_preset_may_have_more_members_than_spawns_because_the_leader_is_not_one() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        let team = file.team("independent-review").expect("present");

        // Five members, a cap of four: the lead is the pane the run is
        // coordinated from, so only the four around it are spawns.
        assert_eq!(team.requested_members(), 5);
        assert_eq!(team.requested_children(), 4);
        assert_eq!(team.stop.max_spawns, 4);
        assert!(
            plan(
                team,
                &hp_agents::known_ids()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            )
            .is_ok(),
            "a preset with five members and a cap of four must still plan"
        );
    }

    #[test]
    fn a_role_picks_the_first_agent_it_actually_has() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        let team = file.team("feature-build").expect("present");

        // Only codex is installed, and every role prefers something else
        // first. The plan must fall through to each role's second choice.
        let members = plan(team, &installed(&["codex"])).expect("planned");
        assert_eq!(members.len(), 3);
        assert!(members.iter().all(|member| member.agent == "codex"));
        assert_eq!(members[0].role, "planner");
    }

    #[test]
    fn a_role_with_nothing_installed_refuses_with_the_names_it_tried() {
        let file = PresetFile::parse(SHIPPED).expect("parse");
        let team = file.team("independent-review").expect("present");

        let error = plan(team, &installed(&["aider"])).expect_err("refused");
        match error {
            PlanError::NoAgentForRole { role, named } => {
                assert_eq!(role, "lead");
                assert!(named.contains("claude-code"), "{named}");
            }
            other => panic!("expected NoAgentForRole, got {other}"),
        }
    }

    #[test]
    fn a_preset_that_asks_for_more_than_its_cap_is_reported() {
        let text = r#"{
          "version": 1,
          "teams": [{
            "key": "greedy",
            "title": { "en": "Greedy" },
            "purpose": { "en": "Asks for more than it allows." },
            "archetype": "pipeline",
            "stop": { "maxRounds": 1, "maxSpawns": 1, "wallMinutes": 5 },
            "waves": [{ "wave": 0, "roles": [
              { "key": "a", "kind": "worker", "agents": ["codex"], "count": 3, "focus": "work" }
            ]}]
          }]
        }"#;

        let file = PresetFile::parse(text).expect("parse");
        let problems = file.problems();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("caps the run")),
            "{problems:#?}"
        );

        let error = plan(&file.teams[0], &installed(&["codex"])).expect_err("refused");
        assert_eq!(error, PlanError::OverCap { needed: 3, cap: 1 });
    }

    #[test]
    fn a_duplicate_team_key_is_reported() {
        let text = r#"{
          "version": 1,
          "teams": [
            { "key": "same", "title": {"en":"A"}, "purpose": {"en":"a"}, "archetype": "reviewer",
              "stop": {"maxRounds":1,"maxSpawns":1,"wallMinutes":5},
              "waves": [{"wave":0,"roles":[{"key":"r","kind":"checker","agents":["codex"],"focus":"x"}]}] },
            { "key": "same", "title": {"en":"B"}, "purpose": {"en":"b"}, "archetype": "reviewer",
              "stop": {"maxRounds":1,"maxSpawns":1,"wallMinutes":5},
              "waves": [{"wave":0,"roles":[{"key":"r","kind":"checker","agents":["codex"],"focus":"x"}]}] }
          ]
        }"#;

        let file = PresetFile::parse(text).expect("parse");
        assert!(
            file.problems()
                .iter()
                .any(|problem| problem.contains("duplicate")),
            "{:#?}",
            file.problems()
        );
    }

    #[test]
    fn a_future_version_is_refused() {
        let error = PresetFile::parse(r#"{"version": 7, "teams": []}"#).expect_err("refused");
        assert!(matches!(
            error,
            PresetError::Version {
                found: 7,
                expected: 1
            }
        ));
    }

    #[test]
    fn text_falls_back_when_english_is_missing() {
        let mut text = Localised::new();
        text.insert("vi".to_string(), "Duyệt mã".to_string());
        assert_eq!(pick(&text), "Duyệt mã");
        assert_eq!(pick(&Localised::new()), "");
    }
}

//! Domain types for HyperPane: pane identity, the split layout tree and
//! workspaces.
//!
//! The layout is a binary tree so that any pane can be split in either
//! direction and the whole arrangement round-trips through JSON. Geometry is
//! relative (a ratio per split), so a saved layout reflows to any window size.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type PaneId = String;

/// Which way a split places its two children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitDirection {
    /// Children sit side by side, sharing the width.
    Row,
    /// Children stack, sharing the height.
    Column,
}

/// Ratio bounds keep a split from collapsing to nothing. The UI clamps to the
/// same range, so a drag can never produce an unclickable pane.
pub const MIN_RATIO: f32 = 0.1;
pub const MAX_RATIO: f32 = 0.9;

/// One node of the layout tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LayoutNode {
    Leaf {
        pane: PaneId,
    },
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(pane: impl Into<PaneId>) -> Self {
        LayoutNode::Leaf { pane: pane.into() }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(self, LayoutNode::Leaf { .. })
    }

    /// Pane ids in visual order: left to right, top to bottom.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            LayoutNode::Leaf { pane } => out.push(pane.clone()),
            LayoutNode::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    pub fn contains(&self, pane: &str) -> bool {
        match self {
            LayoutNode::Leaf { pane: id } => id == pane,
            LayoutNode::Split { first, second, .. } => {
                first.contains(pane) || second.contains(pane)
            }
        }
    }

    /// Replace `target` with a split of `target` and `new_pane`.
    ///
    /// `target` keeps its slot and `new_pane` follows it: to the right for
    /// `Row`, below for `Column`. That is what "split right" and "split down"
    /// mean in a terminal.
    ///
    /// Returns `false` when `target` is not in this tree, in which case the
    /// tree is unchanged.
    pub fn split_at(
        &mut self,
        target: &str,
        direction: SplitDirection,
        new_pane: impl Into<PaneId>,
    ) -> bool {
        match self {
            LayoutNode::Leaf { pane } if pane == target => {
                let existing = std::mem::replace(self, LayoutNode::leaf(""));
                *self = LayoutNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(existing),
                    second: Box::new(LayoutNode::leaf(new_pane)),
                };
                true
            }
            LayoutNode::Leaf { .. } => false,
            LayoutNode::Split { first, second, .. } => {
                if first.contains(target) {
                    first.split_at(target, direction, new_pane)
                } else if second.contains(target) {
                    second.split_at(target, direction, new_pane)
                } else {
                    false
                }
            }
        }
    }

    /// Remove a pane and collapse its parent split.
    ///
    /// Refuses to remove the last remaining pane, so a tree always has at least
    /// one leaf. Returns `false` when nothing changed.
    pub fn remove(&mut self, target: &str) -> bool {
        if self.is_leaf() {
            return false;
        }
        if let LayoutNode::Split { first, second, .. } = self {
            if matches!(first.as_ref(), LayoutNode::Leaf { pane } if pane == target) {
                let sibling = std::mem::replace(second.as_mut(), LayoutNode::leaf(""));
                *self = sibling;
                return true;
            }
            if matches!(second.as_ref(), LayoutNode::Leaf { pane } if pane == target) {
                let sibling = std::mem::replace(first.as_mut(), LayoutNode::leaf(""));
                *self = sibling;
                return true;
            }
            return first.remove(target) || second.remove(target);
        }
        false
    }

    /// Set the ratio of the split reached by walking `path`, where each step
    /// says whether to descend into the first child. This is how a dragged
    /// divider addresses its own split without needing split ids.
    pub fn set_ratio_at(&mut self, path: &[bool], ratio: f32) -> bool {
        match self {
            LayoutNode::Leaf { .. } => false,
            LayoutNode::Split {
                ratio: current,
                first,
                second,
                ..
            } => match path.split_first() {
                None => {
                    *current = ratio.clamp(MIN_RATIO, MAX_RATIO);
                    true
                }
                Some((true, rest)) => first.set_ratio_at(rest, ratio),
                Some((false, rest)) => second.set_ratio_at(rest, ratio),
            },
        }
    }

    /// Set the ratio of the split that directly holds `pane`, for keyboard
    /// resize. Returns `false` when no split directly holds it.
    pub fn set_ratio_for(&mut self, pane: &str, ratio: f32) -> bool {
        if let LayoutNode::Split {
            ratio: current,
            first,
            second,
            ..
        } = self
        {
            let holds =
                |node: &LayoutNode| matches!(node, LayoutNode::Leaf { pane: id } if id == pane);
            if holds(first) || holds(second) {
                *current = ratio.clamp(MIN_RATIO, MAX_RATIO);
                return true;
            }
            return first.set_ratio_for(pane, ratio) || second.set_ratio_for(pane, ratio);
        }
        false
    }
}

/// A named arrangement of panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub layout: LayoutNode,
    /// Pane that receives keyboard input.
    pub focused: Option<PaneId>,
    /// Workspaces with no panes left are dropped on load.
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

impl Workspace {
    pub fn single(id: impl Into<String>, name: impl Into<String>, pane: impl Into<PaneId>) -> Self {
        let pane = pane.into();
        Self {
            id: id.into(),
            name: name.into(),
            layout: LayoutNode::leaf(pane.clone()),
            focused: Some(pane),
            active: true,
        }
    }

    /// Add a pane by splitting `target`. Focus moves to the new pane, which is
    /// what a person expects after opening one.
    pub fn add_pane(
        &mut self,
        target: &str,
        direction: SplitDirection,
        new_pane: impl Into<PaneId>,
    ) -> bool {
        let new_pane = new_pane.into();
        if self.layout.split_at(target, direction, new_pane.clone()) {
            self.focused = Some(new_pane);
            true
        } else {
            false
        }
    }

    /// Close a pane; focus falls to the next pane in visual order.
    pub fn close_pane(&mut self, pane: &str) -> bool {
        let remaining = self.layout.pane_ids();
        if remaining.len() <= 1 {
            return false;
        }
        let index = remaining.iter().position(|id| id == pane);
        if !self.layout.remove(pane) {
            return false;
        }
        let after = self.layout.pane_ids();
        self.focused = index
            .and_then(|i| after.get(i).or_else(|| after.last()).cloned())
            .or_else(|| after.first().cloned());
        true
    }

    /// Move focus by `delta` steps in visual order (wraps).
    pub fn cycle_focus(&mut self, delta: isize) {
        let panes = self.layout.pane_ids();
        if panes.is_empty() {
            self.focused = None;
            return;
        }
        let current = self
            .focused
            .as_ref()
            .and_then(|id| panes.iter().position(|p| p == id))
            .unwrap_or(0) as isize;
        let len = panes.len() as isize;
        let next = ((current + delta) % len + len) % len;
        self.focused = Some(panes[next as usize].clone());
    }

    /// Whether this workspace has at least one pane with a real id.
    ///
    /// Every leaf carries a pane id, so "no panes" is not the same as an empty
    /// tree: a blank id is what a half-written file looks like, and a pane
    /// with no id cannot be spawned.
    pub fn is_usable(&self) -> bool {
        self.layout
            .pane_ids()
            .iter()
            .any(|pane| !pane.trim().is_empty())
    }
}

/// A per-pane launch configuration. `shell: None` means the platform default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub shell: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// Whether HyperPane's prompt hooks are loaded into this shell. Only a
    /// shell the app knows how to instrument is touched at all.
    #[serde(default = "default_true")]
    pub integration: bool,
}

impl Profile {
    /// The profile every install starts with.
    pub fn default_shell() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default shell".to_string(),
            shell: None,
            args: Vec::new(),
            cwd: None,
            env: Default::default(),
            integration: true,
        }
    }
}

/// What a pane runs, when it runs an agent rather than a shell.
///
/// Stored as the launch itself rather than as a catalogue id and a lookup: a pane
/// comes back running what it ran, even if the catalogue's idea of that CLI has
/// moved on since, and a CLI that is no longer installed fails in the pane and
/// says so instead of the pane quietly becoming something else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneAgent {
    /// Catalogue id, so a pane can say which agent this is.
    pub id: String,
    pub label: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// The task had to be typed in once the agent was up.
    #[serde(default)]
    pub needs_paste: bool,
    /// The directory this run is confined to, when it has one.
    pub cwd: Option<String>,
    /// The branch of that directory, when it is a worktree.
    pub branch: Option<String>,
}

/// Bumped when a field changes meaning, so an old file is refused rather than
/// silently half-read.
pub const SESSION_VERSION: u32 = 1;

/// Everything that survives a restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub version: u32,
    /// `None` on a first run, or when the last session had no panes left.
    pub workspace: Option<Workspace>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    pub default_profile: Option<String>,
    /// Which profile each pane opened with, by pane id.
    ///
    /// The layout alone does not say how a pane was opened, and the difference
    /// shows: a pane on a checkout comes back as a shell in whatever directory the
    /// app was started from, with the wrong shell and without the profile's
    /// environment. A pane this map does not mention - a new one, or one from a
    /// file written before panes were remembered - opens with the default profile.
    #[serde(default)]
    pub panes: BTreeMap<PaneId, String>,
    /// What each agent pane runs, by pane id. A pane that ran a CLI has to come
    /// back running it: a shell in its place would look like the same pane and
    /// quietly be a different thing. The task itself is deliberately not stored -
    /// sending an order again to an agent nobody is watching is a decision, not a
    /// default - so a restored run waits for one.
    #[serde(default)]
    pub agents: BTreeMap<PaneId, PaneAgent>,
}

impl Default for Session {
    fn default() -> Self {
        Self::empty()
    }
}

impl Session {
    pub fn empty() -> Self {
        Self {
            version: SESSION_VERSION,
            workspace: None,
            profiles: vec![Profile::default_shell()],
            default_profile: Some("default".to_string()),
            panes: BTreeMap::new(),
            agents: BTreeMap::new(),
        }
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    /// The profile used when none is named.
    pub fn active_profile(&self) -> Option<&Profile> {
        self.default_profile
            .as_deref()
            .and_then(|id| self.profile(id))
            .or_else(|| self.profiles.first())
    }

    /// Repair a session so the app can always start: drop a workspace with no
    /// usable pane, and make sure every profile can be picked and shown.
    ///
    /// The only way to get any of this wrong is to hand-edit the file — the
    /// editor derives ids from names and refuses to save an empty one — and
    /// the rest of the file is still worth keeping, so nothing here is an
    /// error.
    pub fn normalise(mut self) -> Self {
        if self
            .workspace
            .as_ref()
            .is_some_and(|workspace| !workspace.is_usable())
        {
            self.workspace = None;
        }
        if self.profiles.is_empty() {
            self.profiles.push(Profile::default_shell());
        }
        repair_profiles(&mut self.profiles);
        if self
            .default_profile
            .as_ref()
            .is_none_or(|id| !self.profiles.iter().any(|p| &p.id == id))
        {
            self.default_profile = self.profiles.first().map(|p| p.id.clone());
        }
        // A pane's profile is worth remembering only while the pane exists. A
        // workspace that has been dropped takes every pane with it, and an entry
        // left behind would attach a profile to a pane that will never come back.
        let live = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.layout.pane_ids())
            .unwrap_or_default();
        self.panes.retain(|pane, _| live.contains(pane));
        self.agents.retain(|pane, _| live.contains(pane));
        self
    }
}

/// Give every profile a name to show and an id nothing else holds.
///
/// Two profiles with one id would leave the second unreachable — a lookup finds
/// the first — and a profile with no name shows as a blank row in the palette.
fn repair_profiles(profiles: &mut [Profile]) {
    let mut taken: Vec<String> = Vec::with_capacity(profiles.len());
    for profile in profiles.iter_mut() {
        if profile.id.trim().is_empty() {
            profile.id = "profile".to_string();
        }
        profile.id = free_id(profile.id.trim(), &taken);
        taken.push(profile.id.clone());
        if profile.name.trim().is_empty() {
            profile.name = profile.id.clone();
        }
    }
}

/// `candidate`, or `candidate-2`, `candidate-3`… until nothing holds it.
fn free_id(candidate: &str, taken: &[String]) -> String {
    if !taken.iter().any(|id| id == candidate) {
        return candidate.to_string();
    }
    let mut suffix = 2;
    loop {
        let numbered = format!("{candidate}-{suffix}");
        if !taken.contains(&numbered) {
            return numbered;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        Workspace::single("w1", "main", "p1")
    }

    #[test]
    fn starts_with_one_focused_pane() {
        let w = ws();
        assert_eq!(w.layout.pane_ids(), vec!["p1"]);
        assert_eq!(w.focused.as_deref(), Some("p1"));
    }

    #[test]
    fn split_adds_a_pane_and_focuses_it() {
        let mut w = ws();
        assert!(w.add_pane("p1", SplitDirection::Row, "p2"));
        assert_eq!(w.layout.pane_ids(), vec!["p1", "p2"]);
        assert_eq!(w.focused.as_deref(), Some("p2"));
    }

    #[test]
    fn nested_splits_keep_visual_order() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        w.add_pane("p1", SplitDirection::Column, "p3");
        // p1 keeps its slot, p3 follows it below, and p2 stays to the right of
        // the whole column.
        assert_eq!(w.layout.pane_ids(), vec!["p1", "p3", "p2"]);
    }

    #[test]
    fn a_split_places_the_new_pane_after_the_existing_one() {
        let mut right = ws();
        right.add_pane("p1", SplitDirection::Row, "new");
        assert_eq!(right.layout.pane_ids(), vec!["p1", "new"]);

        let mut below = ws();
        below.add_pane("p1", SplitDirection::Column, "new");
        assert_eq!(below.layout.pane_ids(), vec!["p1", "new"]);
    }

    #[test]
    fn splitting_an_unknown_pane_changes_nothing() {
        let mut w = ws();
        assert!(!w.add_pane("nope", SplitDirection::Row, "p2"));
        assert_eq!(w.layout.pane_ids(), vec!["p1"]);
    }

    #[test]
    fn closing_a_pane_collapses_its_parent_split() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        assert!(w.close_pane("p2"));
        assert_eq!(w.layout.pane_ids(), vec!["p1"]);
        assert!(w.layout.is_leaf());
    }

    #[test]
    fn the_last_pane_cannot_be_closed() {
        let mut w = ws();
        assert!(!w.close_pane("p1"));
        assert_eq!(w.layout.pane_ids(), vec!["p1"]);
    }

    #[test]
    fn focus_falls_to_the_pane_that_took_the_slot() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        w.add_pane("p2", SplitDirection::Row, "p3");
        assert_eq!(w.layout.pane_ids(), vec!["p1", "p2", "p3"]);
        assert!(w.close_pane("p2"));
        assert_eq!(w.focused.as_deref(), Some("p3"));
    }

    #[test]
    fn focus_cycles_and_wraps() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        w.add_pane("p2", SplitDirection::Row, "p3");
        w.focused = Some("p3".into());
        w.cycle_focus(1);
        assert_eq!(w.focused.as_deref(), Some("p1"));
        w.cycle_focus(-1);
        assert_eq!(w.focused.as_deref(), Some("p3"));
    }

    #[test]
    fn ratio_is_clamped_to_a_clickable_range() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        assert!(w.layout.set_ratio_for("p1", 0.99));
        match &w.layout {
            LayoutNode::Split { ratio, .. } => assert_eq!(*ratio, MAX_RATIO),
            _ => panic!("expected a split"),
        }
    }

    #[test]
    fn layout_round_trips_through_json() {
        let mut w = ws();
        w.add_pane("p1", SplitDirection::Row, "p2");
        w.add_pane("p2", SplitDirection::Column, "p3");
        let json = serde_json::to_string(&w).expect("serialize");
        let back: Workspace = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(w, back);
    }

    #[test]
    fn a_fresh_session_has_a_usable_default_profile() {
        let session = Session::empty();
        assert_eq!(session.version, SESSION_VERSION);
        assert!(session.workspace.is_none());
        let profile = session.active_profile().expect("a default profile");
        assert_eq!(profile.id, "default");
        assert!(profile.shell.is_none(), "None means the platform default");
    }

    #[test]
    fn normalise_drops_a_workspace_with_no_panes() {
        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: LayoutNode::leaf("p1"),
            focused: Some("p1".into()),
            active: true,
        });
        assert!(session.clone().normalise().workspace.is_some());

        // A leaf with an empty id is what a half-written file would look like.
        if let Some(workspace) = session.workspace.as_mut() {
            workspace.layout = LayoutNode::leaf("");
        }
        assert!(session.normalise().workspace.is_none());
    }

    #[test]
    fn a_pane_remembers_the_profile_it_opened_with() {
        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: LayoutNode::leaf("p1"),
            focused: Some("p1".into()),
            active: true,
        });
        session.panes.insert("p1".into(), "source".into());

        let json = serde_json::to_string(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.panes.get("p1").map(String::as_str), Some("source"));
    }

    #[test]
    fn a_session_from_before_panes_were_remembered_still_loads() {
        // The field is absent rather than empty in a file written by an older
        // build, and a missing map must not stop the session from loading.
        let old = r#"{
            "version": 1,
            "workspace": null,
            "profiles": [],
            "defaultProfile": "default"
        }"#;

        let session: Session = serde_json::from_str(old).expect("deserialize");
        assert!(session.panes.is_empty());
        assert!(session.agents.is_empty());
    }

    fn pane_agent() -> PaneAgent {
        PaneAgent {
            id: "opencode".into(),
            label: "opencode".into(),
            program: "opencode".into(),
            args: vec!["--foo".into()],
            needs_paste: true,
            cwd: Some("/repo/.worktrees/run".into()),
            branch: Some("hp/run".into()),
        }
    }

    #[test]
    fn a_pane_remembers_the_agent_it_runs() {
        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: LayoutNode::leaf("p1"),
            focused: Some("p1".into()),
            active: true,
        });
        session.agents.insert("p1".into(), pane_agent());

        let json = serde_json::to_string(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.agents.get("p1"), Some(&pane_agent()));
    }

    #[test]
    fn normalise_forgets_agents_whose_panes_are_gone() {
        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: LayoutNode::leaf("p1"),
            focused: Some("p1".into()),
            active: true,
        });
        session.agents.insert("p1".into(), pane_agent());
        session.agents.insert("p2".into(), pane_agent());

        let repaired = session.normalise();

        assert!(repaired.agents.contains_key("p1"));
        assert!(!repaired.agents.contains_key("p2"));
    }

    #[test]
    fn normalise_forgets_panes_that_are_no_longer_in_the_layout() {
        let mut session = Session::empty();
        session.workspace = Some(Workspace {
            id: "w".into(),
            name: "w".into(),
            layout: LayoutNode::leaf("p1"),
            focused: Some("p1".into()),
            active: true,
        });
        session.panes.insert("p1".into(), "source".into());
        // A pane that was closed, whose entry a half-written file kept.
        session.panes.insert("p2".into(), "source".into());

        let repaired = session.normalise();

        assert_eq!(repaired.panes.get("p1").map(String::as_str), Some("source"));
        assert!(!repaired.panes.contains_key("p2"));
    }

    #[test]
    fn normalise_forgets_every_pane_when_the_workspace_is_gone() {
        let mut session = Session::empty();
        session.panes.insert("p1".into(), "source".into());

        assert!(session.normalise().panes.is_empty());
    }

    #[test]
    fn normalise_repairs_a_session_with_no_profiles() {
        let mut session = Session::empty();
        session.profiles.clear();
        session.default_profile = Some("gone".into());
        let repaired = session.normalise();
        assert_eq!(repaired.profiles.len(), 1);
        assert_eq!(repaired.default_profile.as_deref(), Some("default"));
    }

    #[test]
    fn normalise_repoints_a_default_profile_that_vanished() {
        let mut session = Session::empty();
        session.profiles.push(Profile {
            id: "zsh".into(),
            name: "zsh".into(),
            shell: Some("/bin/zsh".into()),
            args: vec![],
            cwd: None,
            env: Default::default(),
            integration: true,
        });
        session.default_profile = Some("does-not-exist".into());
        assert_eq!(
            session.normalise().default_profile.as_deref(),
            Some("default")
        );
    }

    #[test]
    fn normalise_gives_a_duplicate_profile_id_a_free_one() {
        let mut session = Session::empty();
        session.profiles.push(Profile {
            id: "default".into(),
            name: "Copy".into(),
            shell: Some("bash".into()),
            args: vec![],
            cwd: None,
            env: Default::default(),
            integration: true,
        });

        let repaired = session.normalise();
        assert_eq!(repaired.profiles[1].id, "default-2");
        // Both profiles are still there, and the default names a real one.
        assert_eq!(repaired.profiles.len(), 2);
        assert_eq!(repaired.default_profile.as_deref(), Some("default"));
        assert_eq!(
            repaired
                .profile("default-2")
                .map(|profile| profile.name.as_str()),
            Some("Copy")
        );
    }

    #[test]
    fn normalise_names_a_profile_that_has_none() {
        let mut session = Session::empty();
        session.profiles.push(Profile {
            id: "ci".into(),
            name: "   ".into(),
            shell: None,
            args: vec![],
            cwd: None,
            env: Default::default(),
            integration: true,
        });

        let repaired = session.normalise();
        assert_eq!(repaired.profile("ci").expect("profile").name, "ci");
    }

    #[test]
    fn normalise_repairs_a_profile_with_no_id_or_name() {
        let mut session = Session::empty();
        for _ in 0..2 {
            session.profiles.push(Profile {
                id: String::new(),
                name: String::new(),
                shell: None,
                args: vec![],
                cwd: None,
                env: Default::default(),
                integration: true,
            });
        }

        let repaired = session.normalise();
        // A fallback id is still an id, and two of them cannot collide.
        assert_eq!(repaired.profiles[1].id, "profile");
        assert_eq!(repaired.profiles[1].name, "profile");
        assert_eq!(repaired.profiles[2].id, "profile-2");
        assert_eq!(repaired.profiles[2].name, "profile-2");
    }

    #[test]
    fn session_round_trips_through_json() {
        let mut session = Session::empty();
        let mut workspace = ws();
        workspace.add_pane("p1", SplitDirection::Row, "p2");
        session.workspace = Some(workspace);
        session.profiles.push(Profile {
            id: "ssh-prod".into(),
            name: "prod".into(),
            shell: Some("ssh".into()),
            args: vec!["prod".into()],
            cwd: Some("/srv".into()),
            env: [("TERM".to_string(), "xterm-256color".to_string())]
                .into_iter()
                .collect(),
            integration: false,
        });

        let json = serde_json::to_string(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(session, back);
        assert_eq!(back.profile("ssh-prod").expect("profile").args, ["prod"]);
    }
}

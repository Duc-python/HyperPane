//! Domain types for HyperPane: pane identity, the split layout tree and
//! workspaces.
//!
//! The layout is a binary tree so that any pane can be split in either
//! direction and the whole arrangement round-trips through JSON. Geometry is
//! relative (a ratio per split), so a saved layout reflows to any window size.

use serde::{Deserialize, Serialize};

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
            let holds = |node: &LayoutNode| matches!(node, LayoutNode::Leaf { pane: id } if id == pane);
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
        // p3 sits above p1, so it comes first in visual order.
        assert_eq!(w.layout.pane_ids(), vec!["p3", "p1", "p2"]);
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
}

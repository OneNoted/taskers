use serde::{Deserialize, Serialize};

use crate::PaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutNode {
    Leaf {
        pane_id: PaneId,
    },
    Split {
        axis: SplitAxis,
        ratio: u16,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn leaf(pane_id: PaneId) -> Self {
        Self::Leaf { pane_id }
    }

    pub fn split_leaf(
        &mut self,
        target: PaneId,
        axis: SplitAxis,
        new_pane: PaneId,
        ratio: u16,
    ) -> bool {
        match self {
            Self::Leaf { pane_id } if *pane_id == target => {
                let existing = *pane_id;
                *self = Self::Split {
                    axis,
                    ratio,
                    first: Box::new(Self::leaf(existing)),
                    second: Box::new(Self::leaf(new_pane)),
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, axis, new_pane, ratio)
                    || second.split_leaf(target, axis, new_pane, ratio)
            }
        }
    }

    pub fn remove_leaf(&mut self, target: PaneId) -> bool {
        match self {
            Self::Leaf { pane_id } if *pane_id == target => false,
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                if let Self::Leaf { pane_id } = first.as_ref() {
                    if *pane_id == target {
                        *self = *second.clone();
                        return true;
                    }
                }
                if let Self::Leaf { pane_id } = second.as_ref() {
                    if *pane_id == target {
                        *self = *first.clone();
                        return true;
                    }
                }
                first.remove_leaf(target) || second.remove_leaf(target)
            }
        }
    }

    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf { pane_id } => vec![*pane_id],
            Self::Split { first, second, .. } => {
                let mut leaves = first.leaves();
                leaves.extend(second.leaves());
                leaves
            }
        }
    }
}

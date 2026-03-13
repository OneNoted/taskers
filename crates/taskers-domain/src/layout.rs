use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::PaneId;

const MIN_SPLIT_RATIO: u16 = 150;
const MAX_SPLIT_RATIO: u16 = 850;
const ROOT_LAYOUT_SIZE: f32 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
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
                    ratio: clamp_ratio(ratio),
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
                if let Self::Leaf { pane_id } = first.as_ref()
                    && *pane_id == target
                {
                    *self = *second.clone();
                    return true;
                }
                if let Self::Leaf { pane_id } = second.as_ref()
                    && *pane_id == target
                {
                    *self = *first.clone();
                    return true;
                }
                first.remove_leaf(target) || second.remove_leaf(target)
            }
        }
    }

    pub fn contains(&self, target: PaneId) -> bool {
        match self {
            Self::Leaf { pane_id } => *pane_id == target,
            Self::Split { first, second, .. } => first.contains(target) || second.contains(target),
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

    pub fn focus_neighbor(&self, target: PaneId, direction: Direction) -> Option<PaneId> {
        let leaves = self.collect_leaf_rects();
        let (_, target_rect) = leaves
            .iter()
            .find(|(pane_id, _)| *pane_id == target)
            .copied()?;

        leaves
            .into_iter()
            .filter(|(pane_id, _)| *pane_id != target)
            .filter_map(|(pane_id, rect)| {
                rect.directional_score(target_rect, direction)
                    .map(|score| (pane_id, score))
            })
            .min_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(Ordering::Equal))
            .map(|(pane_id, _)| pane_id)
    }

    pub fn resize_leaf(&mut self, target: PaneId, direction: Direction, amount: i32) -> bool {
        self.resize_leaf_inner(target, direction, amount.unsigned_abs() as u16)
            .is_some()
    }

    pub fn set_ratio_at_path(&mut self, path: &[bool], ratio: u16) -> bool {
        let ratio = clamp_ratio(ratio);
        if path.is_empty() {
            if let Self::Split {
                ratio: current_ratio,
                ..
            } = self
            {
                *current_ratio = ratio;
                return true;
            }
            return false;
        }

        match self {
            Self::Split { first, second, .. } => {
                let (head, tail) = path.split_first().expect("path is not empty");
                if *head {
                    second.set_ratio_at_path(tail, ratio)
                } else {
                    first.set_ratio_at_path(tail, ratio)
                }
            }
            Self::Leaf { .. } => false,
        }
    }

    fn resize_leaf_inner(
        &mut self,
        target: PaneId,
        direction: Direction,
        amount: u16,
    ) -> Option<bool> {
        match self {
            Self::Leaf { pane_id } => (*pane_id == target).then_some(false),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let found_in_first = first.contains(target);
                let child_result = if found_in_first {
                    first.resize_leaf_inner(target, direction, amount)
                } else {
                    second.resize_leaf_inner(target, direction, amount)
                };

                match child_result {
                    Some(true) => Some(true),
                    Some(false) => {
                        let delta = split_resize_delta(*axis, direction, found_in_first, amount)?;
                        *ratio = apply_ratio_delta(*ratio, delta);
                        Some(true)
                    }
                    None => None,
                }
            }
        }
    }

    fn collect_leaf_rects(&self) -> Vec<(PaneId, LayoutRect)> {
        let mut leaves = Vec::new();
        self.collect_leaf_rects_into(
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: ROOT_LAYOUT_SIZE,
                height: ROOT_LAYOUT_SIZE,
            },
            &mut leaves,
        );
        leaves
    }

    fn collect_leaf_rects_into(&self, rect: LayoutRect, out: &mut Vec<(PaneId, LayoutRect)>) {
        match self {
            Self::Leaf { pane_id } => out.push((*pane_id, rect)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = f32::from(*ratio) / 1000.0;
                match axis {
                    SplitAxis::Horizontal => {
                        let first_width = rect.width * ratio;
                        first.collect_leaf_rects_into(
                            LayoutRect {
                                width: first_width,
                                ..rect
                            },
                            out,
                        );
                        second.collect_leaf_rects_into(
                            LayoutRect {
                                x: rect.x + first_width,
                                width: rect.width - first_width,
                                ..rect
                            },
                            out,
                        );
                    }
                    SplitAxis::Vertical => {
                        let first_height = rect.height * ratio;
                        first.collect_leaf_rects_into(
                            LayoutRect {
                                height: first_height,
                                ..rect
                            },
                            out,
                        );
                        second.collect_leaf_rects_into(
                            LayoutRect {
                                y: rect.y + first_height,
                                height: rect.height - first_height,
                                ..rect
                            },
                            out,
                        );
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LayoutRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LayoutRect {
    fn center_x(self) -> f32 {
        self.x + (self.width / 2.0)
    }

    fn center_y(self) -> f32 {
        self.y + (self.height / 2.0)
    }

    fn directional_score(self, target: Self, direction: Direction) -> Option<f32> {
        let primary = match direction {
            Direction::Left => target.center_x() - self.center_x(),
            Direction::Right => self.center_x() - target.center_x(),
            Direction::Up => target.center_y() - self.center_y(),
            Direction::Down => self.center_y() - target.center_y(),
        };
        if primary <= 0.0 {
            return None;
        }

        let secondary = match direction {
            Direction::Left | Direction::Right => (self.center_y() - target.center_y()).abs(),
            Direction::Up | Direction::Down => (self.center_x() - target.center_x()).abs(),
        };

        Some((primary * 10.0) + secondary)
    }
}

fn clamp_ratio(ratio: u16) -> u16 {
    ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
}

fn split_resize_delta(
    axis: SplitAxis,
    direction: Direction,
    found_in_first: bool,
    amount: u16,
) -> Option<i32> {
    match axis {
        SplitAxis::Horizontal => match (found_in_first, direction) {
            (true, Direction::Right) => Some(i32::from(amount)),
            (true, Direction::Left) => Some(-i32::from(amount)),
            (false, Direction::Right) => Some(-i32::from(amount)),
            (false, Direction::Left) => Some(i32::from(amount)),
            _ => None,
        },
        SplitAxis::Vertical => match (found_in_first, direction) {
            (true, Direction::Down) => Some(i32::from(amount)),
            (true, Direction::Up) => Some(-i32::from(amount)),
            (false, Direction::Down) => Some(-i32::from(amount)),
            (false, Direction::Up) => Some(i32::from(amount)),
            _ => None,
        },
    }
}

fn apply_ratio_delta(current: u16, delta: i32) -> u16 {
    (i32::from(current) + delta).clamp(i32::from(MIN_SPLIT_RATIO), i32::from(MAX_SPLIT_RATIO))
        as u16
}

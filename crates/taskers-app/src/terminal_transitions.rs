use std::collections::{HashMap, HashSet};

use taskers_domain::{LayoutNode, PaneId, SplitAxis, WindowFrame, WorkspaceWindowId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionCurve {
    EaseInOutCubic,
}

impl MotionCurve {
    pub fn sample(self, t: f64) -> f64 {
        let clamped = t.clamp(0.0, 1.0);
        match self {
            Self::EaseInOutCubic => {
                if clamped < 0.5 {
                    4.0 * clamped * clamped * clamped
                } else {
                    1.0 - ((-2.0 * clamped + 2.0).powi(3) / 2.0)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTiming {
    pub duration_us: i64,
    pub curve: MotionCurve,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LifecycleMotionSpec {
    pub timing: MotionTiming,
    pub ghost_start_opacity: f64,
    pub minimum_extent_px: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabMotionSpec {
    pub structural: MotionTiming,
    pub drag_snap: MotionTiming,
    pub enter_offset_px: f64,
    pub exit_offset_px: f64,
    pub size_delta_px: i32,
    pub drag_threshold_px: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalMotionSpec {
    pub window: LifecycleMotionSpec,
    pub pane: LifecycleMotionSpec,
    pub tab: TabMotionSpec,
}

pub const TERMINAL_MOTION_SPEC: TerminalMotionSpec = TerminalMotionSpec {
    window: LifecycleMotionSpec {
        timing: MotionTiming {
            duration_us: 320_000,
            curve: MotionCurve::EaseInOutCubic,
        },
        ghost_start_opacity: 0.9,
        minimum_extent_px: 96,
    },
    pane: LifecycleMotionSpec {
        timing: MotionTiming {
            duration_us: 320_000,
            curve: MotionCurve::EaseInOutCubic,
        },
        ghost_start_opacity: 0.92,
        minimum_extent_px: 42,
    },
    tab: TabMotionSpec {
        structural: MotionTiming {
            duration_us: 320_000,
            curve: MotionCurve::EaseInOutCubic,
        },
        drag_snap: MotionTiming {
            duration_us: 200_000,
            curve: MotionCurve::EaseInOutCubic,
        },
        enter_offset_px: 28.0,
        exit_offset_px: 28.0,
        size_delta_px: 18,
        drag_threshold_px: 6.0,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSceneSnapshot {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub windows: Vec<WorkspaceWindowSnapshot>,
    pub panes: Vec<PaneSceneSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceWindowSnapshot {
    pub id: WorkspaceWindowId,
    pub rect: WindowFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneSceneSnapshot {
    pub id: PaneId,
    pub window_id: WorkspaceWindowId,
    pub rect: WindowFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionItemId {
    Window(WorkspaceWindowId),
    Pane(PaneId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionItemKind {
    Window,
    Pane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionPhase {
    Enter,
    Reflow,
    Exit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionItem {
    pub id: TransitionItemId,
    pub kind: TransitionItemKind,
    pub phase: TransitionPhase,
    pub start_rect: WindowFrame,
    pub end_rect: WindowFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionPlan {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub items: Vec<TransitionItem>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentedTransitionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RectEdge {
    Left,
    Right,
    Top,
    Bottom,
}

pub fn derive_pane_frames(
    window_rect: WindowFrame,
    layout: &LayoutNode,
) -> Vec<(PaneId, WindowFrame)> {
    let mut panes = Vec::new();
    derive_pane_frames_into(window_rect, layout, &mut panes);
    panes
}

pub fn plan_workspace_transition(
    previous: Option<&WorkspaceSceneSnapshot>,
    next: &WorkspaceSceneSnapshot,
    motion: TerminalMotionSpec,
) -> TransitionPlan {
    let Some(previous) = previous else {
        return TransitionPlan {
            canvas_width: next.canvas_width,
            canvas_height: next.canvas_height,
            items: Vec::new(),
        };
    };

    let canvas_width = previous.canvas_width.max(next.canvas_width);
    let canvas_height = previous.canvas_height.max(next.canvas_height);
    let previous_windows = previous
        .windows
        .iter()
        .map(|window| (window.id, window))
        .collect::<HashMap<_, _>>();
    let next_windows = next
        .windows
        .iter()
        .map(|window| (window.id, window))
        .collect::<HashMap<_, _>>();
    let persistent_window_ids = previous_windows
        .keys()
        .filter(|window_id| next_windows.contains_key(window_id))
        .copied()
        .collect::<HashSet<_>>();

    let mut items = Vec::new();

    for window_id in &persistent_window_ids {
        let previous_window = previous_windows
            .get(window_id)
            .expect("persistent window should exist in previous scene");
        let next_window = next_windows
            .get(window_id)
            .expect("persistent window should exist in next scene");
        if previous_window.rect != next_window.rect {
            items.push(TransitionItem {
                id: TransitionItemId::Window(*window_id),
                kind: TransitionItemKind::Window,
                phase: TransitionPhase::Reflow,
                start_rect: previous_window.rect,
                end_rect: next_window.rect,
            });
        }
    }

    for window in &next.windows {
        if persistent_window_ids.contains(&window.id) {
            continue;
        }
        let edge = best_shared_edge(
            window.rect,
            next.windows
                .iter()
                .filter(|candidate| candidate.id != window.id)
                .filter(|candidate| persistent_window_ids.contains(&candidate.id))
                .map(|candidate| candidate.rect),
        )
        .unwrap_or_else(|| nearest_canvas_edge(window.rect, next.canvas_width, next.canvas_height));
        items.push(TransitionItem {
            id: TransitionItemId::Window(window.id),
            kind: TransitionItemKind::Window,
            phase: TransitionPhase::Enter,
            start_rect: collapse_rect(window.rect, edge, motion.window.minimum_extent_px),
            end_rect: window.rect,
        });
    }

    for window in &previous.windows {
        if persistent_window_ids.contains(&window.id) {
            continue;
        }
        let edge = best_shared_edge(
            window.rect,
            previous
                .windows
                .iter()
                .filter(|candidate| candidate.id != window.id)
                .map(|candidate| candidate.rect),
        )
        .unwrap_or_else(|| {
            nearest_canvas_edge(window.rect, previous.canvas_width, previous.canvas_height)
        });
        items.push(TransitionItem {
            id: TransitionItemId::Window(window.id),
            kind: TransitionItemKind::Window,
            phase: TransitionPhase::Exit,
            start_rect: window.rect,
            end_rect: collapse_rect(window.rect, edge, motion.window.minimum_extent_px),
        });
    }

    let previous_panes = previous
        .panes
        .iter()
        .filter(|pane| persistent_window_ids.contains(&pane.window_id))
        .map(|pane| (pane.id, pane))
        .collect::<HashMap<_, _>>();
    let next_panes = next
        .panes
        .iter()
        .filter(|pane| persistent_window_ids.contains(&pane.window_id))
        .map(|pane| (pane.id, pane))
        .collect::<HashMap<_, _>>();
    let persistent_pane_ids = previous_panes
        .keys()
        .filter(|pane_id| next_panes.contains_key(pane_id))
        .copied()
        .collect::<HashSet<_>>();

    for pane_id in &persistent_pane_ids {
        let previous_pane = previous_panes
            .get(pane_id)
            .expect("persistent pane should exist in previous scene");
        let next_pane = next_panes
            .get(pane_id)
            .expect("persistent pane should exist in next scene");
        if previous_pane.rect != next_pane.rect {
            items.push(TransitionItem {
                id: TransitionItemId::Pane(*pane_id),
                kind: TransitionItemKind::Pane,
                phase: TransitionPhase::Reflow,
                start_rect: previous_pane.rect,
                end_rect: next_pane.rect,
            });
        }
    }

    for pane in next_panes.values() {
        if persistent_pane_ids.contains(&pane.id) {
            continue;
        }
        let edge = best_shared_edge(
            pane.rect,
            next.panes
                .iter()
                .filter(|candidate| {
                    candidate.window_id == pane.window_id && candidate.id != pane.id
                })
                .map(|candidate| candidate.rect),
        )
        .unwrap_or_else(|| nearest_canvas_edge(pane.rect, next.canvas_width, next.canvas_height));
        items.push(TransitionItem {
            id: TransitionItemId::Pane(pane.id),
            kind: TransitionItemKind::Pane,
            phase: TransitionPhase::Enter,
            start_rect: collapse_rect(pane.rect, edge, motion.pane.minimum_extent_px),
            end_rect: pane.rect,
        });
    }

    for pane in previous_panes.values() {
        if persistent_pane_ids.contains(&pane.id) {
            continue;
        }
        let edge = best_shared_edge(
            pane.rect,
            previous
                .panes
                .iter()
                .filter(|candidate| {
                    candidate.window_id == pane.window_id && candidate.id != pane.id
                })
                .map(|candidate| candidate.rect),
        )
        .unwrap_or_else(|| {
            nearest_canvas_edge(pane.rect, previous.canvas_width, previous.canvas_height)
        });
        items.push(TransitionItem {
            id: TransitionItemId::Pane(pane.id),
            kind: TransitionItemKind::Pane,
            phase: TransitionPhase::Exit,
            start_rect: pane.rect,
            end_rect: collapse_rect(pane.rect, edge, motion.pane.minimum_extent_px),
        });
    }

    items.sort_by_key(|item| match item.kind {
        TransitionItemKind::Window => 0_u8,
        TransitionItemKind::Pane => 1_u8,
    });

    TransitionPlan {
        canvas_width,
        canvas_height,
        items,
    }
}

pub fn retarget_transition_plan(
    plan: &mut TransitionPlan,
    presented: &HashMap<TransitionItemId, PresentedTransitionRect>,
) {
    for item in &mut plan.items {
        let Some(rect) = presented.get(&item.id) else {
            continue;
        };
        item.start_rect = WindowFrame {
            x: rect.x.round() as i32,
            y: rect.y.round() as i32,
            width: rect.width.round().max(1.0) as i32,
            height: rect.height.round().max(1.0) as i32,
        };
    }
}

fn derive_pane_frames_into(
    rect: WindowFrame,
    layout: &LayoutNode,
    panes: &mut Vec<(PaneId, WindowFrame)>,
) {
    match layout {
        LayoutNode::Leaf { pane_id } => panes.push((*pane_id, rect)),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => match axis {
            SplitAxis::Horizontal => {
                let first_width = split_extent(rect.width, *ratio);
                derive_pane_frames_into(
                    WindowFrame {
                        width: first_width,
                        ..rect
                    },
                    first,
                    panes,
                );
                derive_pane_frames_into(
                    WindowFrame {
                        x: rect.x + first_width,
                        width: rect.width - first_width,
                        ..rect
                    },
                    second,
                    panes,
                );
            }
            SplitAxis::Vertical => {
                let first_height = split_extent(rect.height, *ratio);
                derive_pane_frames_into(
                    WindowFrame {
                        height: first_height,
                        ..rect
                    },
                    first,
                    panes,
                );
                derive_pane_frames_into(
                    WindowFrame {
                        y: rect.y + first_height,
                        height: rect.height - first_height,
                        ..rect
                    },
                    second,
                    panes,
                );
            }
        },
    }
}

fn split_extent(total: i32, ratio: u16) -> i32 {
    if total <= 1 {
        return total.max(1);
    }

    let scaled = ((i64::from(total) * i64::from(ratio)) + 500) / 1000;
    (scaled as i32).clamp(1, total - 1)
}

fn best_shared_edge(
    rect: WindowFrame,
    candidates: impl Iterator<Item = WindowFrame>,
) -> Option<RectEdge> {
    let mut best = None;
    let mut best_overlap = 0;

    for candidate in candidates {
        for (edge, overlap) in shared_edges(rect, candidate) {
            if overlap > best_overlap {
                best = Some(edge);
                best_overlap = overlap;
            }
        }
    }

    best
}

fn shared_edges(rect: WindowFrame, candidate: WindowFrame) -> Vec<(RectEdge, i32)> {
    let overlap_x = axis_overlap(rect.x, rect.right(), candidate.x, candidate.right());
    let overlap_y = axis_overlap(rect.y, rect.bottom(), candidate.y, candidate.bottom());
    let mut edges = Vec::new();

    if rect.x == candidate.right() && overlap_y > 0 {
        edges.push((RectEdge::Left, overlap_y));
    }
    if rect.right() == candidate.x && overlap_y > 0 {
        edges.push((RectEdge::Right, overlap_y));
    }
    if rect.y == candidate.bottom() && overlap_x > 0 {
        edges.push((RectEdge::Top, overlap_x));
    }
    if rect.bottom() == candidate.y && overlap_x > 0 {
        edges.push((RectEdge::Bottom, overlap_x));
    }

    edges
}

fn axis_overlap(start: i32, end: i32, other_start: i32, other_end: i32) -> i32 {
    (end.min(other_end) - start.max(other_start)).max(0)
}

fn nearest_canvas_edge(rect: WindowFrame, canvas_width: i32, canvas_height: i32) -> RectEdge {
    let candidates = [
        (RectEdge::Left, rect.x),
        (RectEdge::Right, canvas_width - rect.right()),
        (RectEdge::Top, rect.y),
        (RectEdge::Bottom, canvas_height - rect.bottom()),
    ];

    candidates
        .into_iter()
        .min_by_key(|(_, distance)| *distance)
        .map(|(edge, _)| edge)
        .unwrap_or(RectEdge::Left)
}

fn collapse_rect(rect: WindowFrame, edge: RectEdge, minimum_extent_px: i32) -> WindowFrame {
    match edge {
        RectEdge::Left => WindowFrame {
            width: minimum_extent_px.clamp(1, rect.width.max(1)),
            ..rect
        },
        RectEdge::Right => {
            let width = minimum_extent_px.clamp(1, rect.width.max(1));
            WindowFrame {
                x: rect.right() - width,
                width,
                ..rect
            }
        }
        RectEdge::Top => WindowFrame {
            height: minimum_extent_px.clamp(1, rect.height.max(1)),
            ..rect
        },
        RectEdge::Bottom => {
            let height = minimum_extent_px.clamp(1, rect.height.max(1));
            WindowFrame {
                y: rect.bottom() - height,
                height,
                ..rect
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> WindowFrame {
        WindowFrame {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn derives_pane_frames_from_nested_split_layout() {
        let left = PaneId::new();
        let top_right = PaneId::new();
        let bottom_right = PaneId::new();
        let layout = LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 600,
            first: Box::new(LayoutNode::Leaf { pane_id: left }),
            second: Box::new(LayoutNode::Split {
                axis: SplitAxis::Vertical,
                ratio: 500,
                first: Box::new(LayoutNode::Leaf { pane_id: top_right }),
                second: Box::new(LayoutNode::Leaf {
                    pane_id: bottom_right,
                }),
            }),
        };

        let panes = derive_pane_frames(rect(0, 0, 1000, 500), &layout);

        assert_eq!(
            panes,
            vec![
                (left, rect(0, 0, 600, 500)),
                (top_right, rect(600, 0, 400, 250)),
                (bottom_right, rect(600, 250, 400, 250)),
            ]
        );
    }

    #[test]
    fn classifies_window_and_pane_transition_items() {
        let window = WorkspaceWindowId::new();
        let pane_a = PaneId::new();
        let pane_b = PaneId::new();
        let pane_c = PaneId::new();
        let previous = WorkspaceSceneSnapshot {
            canvas_width: 800,
            canvas_height: 480,
            windows: vec![WorkspaceWindowSnapshot {
                id: window,
                rect: rect(0, 0, 800, 480),
            }],
            panes: vec![
                PaneSceneSnapshot {
                    id: pane_a,
                    window_id: window,
                    rect: rect(0, 0, 400, 480),
                },
                PaneSceneSnapshot {
                    id: pane_b,
                    window_id: window,
                    rect: rect(400, 0, 400, 480),
                },
            ],
        };
        let next = WorkspaceSceneSnapshot {
            canvas_width: 800,
            canvas_height: 480,
            windows: vec![WorkspaceWindowSnapshot {
                id: window,
                rect: rect(0, 0, 800, 480),
            }],
            panes: vec![
                PaneSceneSnapshot {
                    id: pane_a,
                    window_id: window,
                    rect: rect(0, 0, 520, 480),
                },
                PaneSceneSnapshot {
                    id: pane_c,
                    window_id: window,
                    rect: rect(520, 0, 280, 480),
                },
            ],
        };

        let plan = plan_workspace_transition(Some(&previous), &next, TERMINAL_MOTION_SPEC);

        assert_eq!(plan.items.len(), 3);
        assert!(plan.items.contains(&TransitionItem {
            id: TransitionItemId::Pane(pane_a),
            kind: TransitionItemKind::Pane,
            phase: TransitionPhase::Reflow,
            start_rect: rect(0, 0, 400, 480),
            end_rect: rect(0, 0, 520, 480),
        }));
        assert!(plan.items.iter().any(|item| {
            item.id == TransitionItemId::Pane(pane_b) && item.phase == TransitionPhase::Exit
        }));
        assert!(plan.items.iter().any(|item| {
            item.id == TransitionItemId::Pane(pane_c) && item.phase == TransitionPhase::Enter
        }));
    }

    #[test]
    fn retargets_in_flight_items_from_presented_geometry() {
        let window = WorkspaceWindowId::new();
        let mut plan = TransitionPlan {
            canvas_width: 1200,
            canvas_height: 720,
            items: vec![TransitionItem {
                id: TransitionItemId::Window(window),
                kind: TransitionItemKind::Window,
                phase: TransitionPhase::Reflow,
                start_rect: rect(0, 0, 400, 320),
                end_rect: rect(500, 0, 400, 320),
            }],
        };
        let presented = HashMap::from([(
            TransitionItemId::Window(window),
            PresentedTransitionRect {
                x: 142.4,
                y: 8.6,
                width: 412.2,
                height: 321.5,
            },
        )]);

        retarget_transition_plan(&mut plan, &presented);

        assert_eq!(plan.items[0].start_rect, rect(142, 9, 412, 322));
    }

    #[test]
    fn collapses_new_windows_from_their_shared_insertion_edge() {
        let existing = WorkspaceWindowId::new();
        let created = WorkspaceWindowId::new();
        let previous = WorkspaceSceneSnapshot {
            canvas_width: 1200,
            canvas_height: 640,
            windows: vec![WorkspaceWindowSnapshot {
                id: existing,
                rect: rect(0, 0, 600, 640),
            }],
            panes: Vec::new(),
        };
        let next = WorkspaceSceneSnapshot {
            canvas_width: 1200,
            canvas_height: 640,
            windows: vec![
                WorkspaceWindowSnapshot {
                    id: existing,
                    rect: rect(0, 0, 600, 640),
                },
                WorkspaceWindowSnapshot {
                    id: created,
                    rect: rect(600, 0, 600, 640),
                },
            ],
            panes: Vec::new(),
        };

        let plan = plan_workspace_transition(Some(&previous), &next, TERMINAL_MOTION_SPEC);
        let created_window = plan
            .items
            .iter()
            .find(|item| item.id == TransitionItemId::Window(created))
            .expect("created window should animate");

        assert_eq!(created_window.phase, TransitionPhase::Enter);
        assert_eq!(
            created_window.start_rect,
            rect(600, 0, TERMINAL_MOTION_SPEC.window.minimum_extent_px, 640)
        );
    }

    #[test]
    fn collapses_removed_panes_toward_their_shared_split_seam() {
        let window = WorkspaceWindowId::new();
        let left = PaneId::new();
        let right = PaneId::new();
        let previous = WorkspaceSceneSnapshot {
            canvas_width: 800,
            canvas_height: 480,
            windows: vec![WorkspaceWindowSnapshot {
                id: window,
                rect: rect(0, 0, 800, 480),
            }],
            panes: vec![
                PaneSceneSnapshot {
                    id: left,
                    window_id: window,
                    rect: rect(0, 0, 400, 480),
                },
                PaneSceneSnapshot {
                    id: right,
                    window_id: window,
                    rect: rect(400, 0, 400, 480),
                },
            ],
        };
        let next = WorkspaceSceneSnapshot {
            canvas_width: 800,
            canvas_height: 480,
            windows: vec![WorkspaceWindowSnapshot {
                id: window,
                rect: rect(0, 0, 800, 480),
            }],
            panes: vec![PaneSceneSnapshot {
                id: left,
                window_id: window,
                rect: rect(0, 0, 800, 480),
            }],
        };

        let plan = plan_workspace_transition(Some(&previous), &next, TERMINAL_MOTION_SPEC);
        let removed_pane = plan
            .items
            .iter()
            .find(|item| item.id == TransitionItemId::Pane(right))
            .expect("removed pane should animate");

        assert_eq!(removed_pane.phase, TransitionPhase::Exit);
        assert_eq!(
            removed_pane.end_rect,
            rect(400, 0, TERMINAL_MOTION_SPEC.pane.minimum_extent_px, 480)
        );
    }
}

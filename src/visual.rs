use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{
    animation::{Spring, smoothstep},
    geometry::Point,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKey {
    Menu(Vec<usize>),
    Action(Vec<usize>, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    Center,
    History,
    Item,
}

#[derive(Clone, Debug)]
pub struct NodeTarget {
    pub key: NodeKey,
    pub item_path: Vec<usize>,
    pub role: NodeRole,
    pub position: Point,
    pub rest_position: Point,
    pub origin: Point,
    pub size: f64,
    pub rest_size: f64,
    pub active: bool,
    pub icon_visible: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Motion {
    pub duration: Duration,
    pub spring: Spring,
}

#[derive(Clone, Debug)]
pub struct VisualNode {
    pub key: NodeKey,
    pub item_path: Vec<usize>,
    pub role: NodeRole,
    pub position: Point,
    pub size: f64,
    pub opacity: f64,
    pub icon_opacity: f64,
    pub active: bool,
    pub selected_action: bool,
    pub return_connector: bool,
    base_position: Point,
    base_size: f64,
    hover_offset: Point,
    hover_from_offset: Point,
    hover_target_offset: Point,
    hover_scale: f64,
    hover_from_scale: f64,
    hover_target_scale: f64,
    hover_started: Instant,
    hover_duration: Duration,
    hover_spring: Spring,
    collapse_to: Point,
    from_position: Point,
    target_position: Point,
    from_scale: f64,
    target_scale: f64,
    from_opacity: f64,
    target_opacity: f64,
    icon_from: f64,
    icon_target: f64,
    icon_started: Instant,
    icon_duration: Duration,
    started: Instant,
    duration: Duration,
    spring: Spring,
    removing: bool,
    position_end: f64,
    opacity_delay: f64,
    creation_anchor: Option<CreationAnchor>,
}

#[derive(Clone, Copy, Debug)]
struct CreationAnchor {
    from: Point,
    to: Point,
    duration: Duration,
    spring: Spring,
}

struct TransitionTarget {
    position: Point,
    size: f64,
    opacity: f64,
    duration: Duration,
    spring: Spring,
    removing: bool,
    position_end: f64,
    opacity_delay: f64,
}

impl VisualNode {
    fn set_icon_visible(&mut self, visible: bool, duration: Duration, now: Instant) {
        let target = f64::from(visible);
        if (self.icon_target - target).abs() < f64::EPSILON {
            return;
        }
        self.icon_from = self.icon_opacity;
        self.icon_target = target;
        self.icon_started = now;
        self.icon_duration = duration;
        if duration.is_zero() {
            self.icon_opacity = target;
            self.icon_from = target;
        }
    }

    fn sample(&mut self, now: Instant) {
        let raw = if self.duration.is_zero() {
            1.0
        } else {
            (now - self.started).as_secs_f64() / self.duration.as_secs_f64()
        };
        let progress = raw.clamp(0.0, 1.0);
        let position_progress = (progress / self.position_end.max(f64::EPSILON)).clamp(0.0, 1.0);
        let position_movement = self.spring.sample(position_progress);
        let size_movement = self.spring.sample(progress);
        let fade = smoothstep(
            ((progress - self.opacity_delay) / (1.0 - self.opacity_delay).max(f64::EPSILON))
                .clamp(0.0, 1.0),
        );
        self.base_position = if let Some(anchor) = self.creation_anchor {
            let anchor_progress = if anchor.duration.is_zero() {
                1.0
            } else {
                ((now - self.started).as_secs_f64() / anchor.duration.as_secs_f64()).clamp(0.0, 1.0)
            };
            let anchor_movement = anchor.spring.sample(anchor_progress);
            let anchor_position = anchor.from.lerp(anchor.to, anchor_movement);
            Point {
                x: anchor_position.x + (self.target_position.x - anchor.to.x) * position_movement,
                y: anchor_position.y + (self.target_position.y - anchor.to.y) * position_movement,
            }
        } else {
            self.from_position
                .lerp(self.target_position, position_movement)
        };
        self.base_size = self.from_scale + (self.target_scale - self.from_scale) * size_movement;
        self.opacity = self.from_opacity + (self.target_opacity - self.from_opacity) * fade;
        let hover_progress = if self.hover_duration.is_zero() {
            1.0
        } else {
            ((now - self.hover_started).as_secs_f64() / self.hover_duration.as_secs_f64())
                .clamp(0.0, 1.0)
        };
        let hover_movement = self.hover_spring.sample(hover_progress);
        self.hover_offset = self
            .hover_from_offset
            .lerp(self.hover_target_offset, hover_movement);
        self.hover_scale = self.hover_from_scale
            + (self.hover_target_scale - self.hover_from_scale) * hover_movement;
        self.position = Point {
            x: self.base_position.x + self.hover_offset.x,
            y: self.base_position.y + self.hover_offset.y,
        };
        self.size = self.base_size * self.hover_scale;
        let icon_progress = if self.icon_duration.is_zero() {
            1.0
        } else {
            ((now - self.icon_started).as_secs_f64() / self.icon_duration.as_secs_f64())
                .clamp(0.0, 1.0)
        };
        self.icon_opacity =
            self.icon_from + (self.icon_target - self.icon_from) * smoothstep(icon_progress);
        if progress >= 1.0 {
            self.base_size = self.target_scale;
            self.opacity = self.target_opacity;
        }
        if self.movement_finished(now) {
            self.base_position = self.target_position;
            self.creation_anchor = None;
            self.return_connector = false;
        }
        self.position = Point {
            x: self.base_position.x + self.hover_offset.x,
            y: self.base_position.y + self.hover_offset.y,
        };
        self.size = self.base_size * self.hover_scale;
    }

    fn retarget(&mut self, target: TransitionTarget, now: Instant) {
        self.sample(now);
        if self.target_position == target.position
            && (self.target_scale - target.size).abs() < f64::EPSILON
            && (self.target_opacity - target.opacity).abs() < f64::EPSILON
            && self.removing == target.removing
            && (self.position_end - target.position_end).abs() < f64::EPSILON
            && (self.opacity_delay - target.opacity_delay).abs() < f64::EPSILON
        {
            return;
        }
        self.from_position = self.position;
        self.target_position = target.position;
        self.from_scale = self.size;
        self.target_scale = target.size;
        self.from_opacity = self.opacity;
        self.target_opacity = target.opacity;
        self.started = now;
        self.duration = target.duration;
        self.spring = target.spring;
        self.removing = target.removing;
        self.position_end = target.position_end;
        self.opacity_delay = target.opacity_delay;
        self.creation_anchor = None;
        self.base_position = self.position;
        self.base_size = self.size;
        self.hover_offset = Point::default();
        self.hover_from_offset = Point::default();
        self.hover_target_offset = Point::default();
        self.hover_scale = 1.0;
        self.hover_from_scale = 1.0;
        self.hover_target_scale = 1.0;
        self.hover_duration = Duration::ZERO;
        if self.duration.is_zero() {
            self.sample(now);
        }
    }

    fn movement_finished(&self, now: Instant) -> bool {
        let transition_finished = self.duration.is_zero() || now - self.started >= self.duration;
        let anchor_finished = self.creation_anchor.is_none_or(|anchor| {
            anchor.duration.is_zero() || now - self.started >= anchor.duration
        });
        transition_finished && anchor_finished
    }

    fn finished(&self, now: Instant) -> bool {
        self.movement_finished(now)
            && (self.hover_duration.is_zero() || now - self.hover_started >= self.hover_duration)
            && (self.icon_duration.is_zero() || now - self.icon_started >= self.icon_duration)
    }
}

#[derive(Default)]
pub struct Animator {
    nodes: HashMap<NodeKey, VisualNode>,
}

impl Animator {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn reconcile(
        &mut self,
        targets: Vec<NodeTarget>,
        movement: Motion,
        creation: Motion,
        icon_duration: Duration,
        animate: bool,
    ) {
        let move_duration = movement.duration;
        let create_duration = creation.duration;
        let move_spring = movement.spring;
        let create_spring = creation.spring;
        let now = Instant::now();
        let animate = animate && (!move_duration.is_zero() || !create_duration.is_zero());
        for node in self.nodes.values_mut() {
            node.sample(now);
        }
        let targets = targets
            .into_iter()
            .map(|target| (target.key.clone(), target))
            .collect::<HashMap<_, _>>();
        for (key, node) in &mut self.nodes {
            if !targets.contains_key(key) && !node.removing {
                let parent_key = match key {
                    NodeKey::Action(path, _) => Some(NodeKey::Menu(path.clone())),
                    NodeKey::Menu(path) if !path.is_empty() => {
                        Some(NodeKey::Menu(path[..path.len() - 1].to_vec()))
                    }
                    NodeKey::Menu(_) => None,
                };
                let destination = parent_key
                    .as_ref()
                    .and_then(|key| targets.get(key))
                    .map_or(node.collapse_to, |target| target.rest_position);
                node.retarget(
                    TransitionTarget {
                        position: destination,
                        size: 0.0,
                        opacity: 0.0,
                        duration: create_duration,
                        spring: create_spring,
                        removing: true,
                        position_end: 1.0,
                        opacity_delay: 0.0,
                    },
                    now,
                );
            }
        }
        for (key, target) in targets {
            if let Some(node) = self.nodes.get_mut(&key) {
                let returning =
                    animate && node.role == NodeRole::Center && target.role == NodeRole::Item;
                node.item_path = target.item_path;
                node.role = target.role;
                node.active = target.active;
                node.selected_action = false;
                node.collapse_to = target.origin;
                node.set_icon_visible(target.icon_visible, icon_duration, now);
                if animate {
                    node.retarget(
                        TransitionTarget {
                            position: target.rest_position,
                            size: target.rest_size,
                            opacity: 1.0,
                            duration: move_duration,
                            spring: move_spring,
                            removing: false,
                            position_end: 1.0,
                            opacity_delay: 0.0,
                        },
                        now,
                    );
                    node.return_connector = returning;
                } else {
                    node.position = target.rest_position;
                    node.size = target.rest_size;
                    node.base_position = target.rest_position;
                    node.base_size = target.rest_size;
                    node.opacity = 1.0;
                    node.from_position = target.rest_position;
                    node.target_position = target.rest_position;
                    node.from_scale = target.rest_size;
                    node.target_scale = target.rest_size;
                    node.from_opacity = 1.0;
                    node.target_opacity = 1.0;
                    node.removing = false;
                    node.return_connector = false;
                }
            } else {
                let create_animated = animate && !create_duration.is_zero();
                let initial = if create_animated { 0.0 } else { 1.0 };
                let parent_key = match &key {
                    NodeKey::Action(path, _) => Some(NodeKey::Menu(path.clone())),
                    NodeKey::Menu(path) if !path.is_empty() => {
                        Some(NodeKey::Menu(path[..path.len() - 1].to_vec()))
                    }
                    NodeKey::Menu(_) => None,
                };
                let parent_position = parent_key
                    .as_ref()
                    .and_then(|parent| self.nodes.get(parent))
                    .map(|parent| parent.position);
                let creation_origin = parent_position.unwrap_or(target.origin);
                let creation_anchor = parent_position.map(|from| CreationAnchor {
                    from,
                    to: target.origin,
                    duration: move_duration,
                    spring: move_spring,
                });
                self.nodes.insert(
                    key.clone(),
                    VisualNode {
                        key,
                        item_path: target.item_path,
                        role: target.role,
                        position: creation_origin,
                        size: target.rest_size * initial,
                        opacity: initial,
                        icon_opacity: if target.icon_visible
                            && create_animated
                            && !icon_duration.is_zero()
                        {
                            0.0
                        } else if target.icon_visible {
                            1.0
                        } else {
                            0.0
                        },
                        active: target.active,
                        selected_action: false,
                        return_connector: false,
                        base_position: creation_origin,
                        base_size: target.rest_size * initial,
                        hover_offset: Point::default(),
                        hover_from_offset: Point::default(),
                        hover_target_offset: Point::default(),
                        hover_scale: 1.0,
                        hover_from_scale: 1.0,
                        hover_target_scale: 1.0,
                        hover_started: now,
                        hover_duration: Duration::ZERO,
                        hover_spring: Spring::default(),
                        collapse_to: target.origin,
                        from_position: creation_origin,
                        target_position: target.rest_position,
                        from_scale: target.rest_size * initial,
                        target_scale: target.rest_size,
                        from_opacity: initial,
                        target_opacity: 1.0,
                        icon_from: if target.icon_visible
                            && create_animated
                            && !icon_duration.is_zero()
                        {
                            0.0
                        } else if target.icon_visible {
                            1.0
                        } else {
                            0.0
                        },
                        icon_target: f64::from(target.icon_visible),
                        icon_started: now,
                        icon_duration,
                        started: now,
                        duration: create_duration,
                        spring: create_spring,
                        removing: false,
                        position_end: 1.0,
                        opacity_delay: 0.0,
                        creation_anchor,
                    },
                );
            }
        }
        if move_duration.is_zero() && create_duration.is_zero() {
            self.tick();
        }
    }

    pub fn hover(
        &mut self,
        targets: Vec<NodeTarget>,
        duration: Duration,
        icon_duration: Duration,
        spring: Spring,
    ) {
        let now = Instant::now();
        for target in targets {
            let Some(node) = self.nodes.get_mut(&target.key) else {
                continue;
            };
            if node.removing {
                continue;
            }
            node.sample(now);
            let active_changed = node.active != target.active;
            node.item_path = target.item_path;
            node.role = target.role;
            node.selected_action = false;
            node.collapse_to = target.origin;
            node.set_icon_visible(target.icon_visible, icon_duration, now);
            let target_offset = Point {
                x: target.position.x - target.rest_position.x,
                y: target.position.y - target.rest_position.y,
            };
            let target_scale = if target.rest_size > f64::EPSILON {
                target.size / target.rest_size
            } else {
                1.0
            };
            let hover_finished =
                node.hover_duration.is_zero() || now - node.hover_started >= node.hover_duration;
            let returning_to_rest = target_offset == Point::default()
                && (target_scale - 1.0).abs() < f64::EPSILON
                && (node.hover_target_offset != Point::default()
                    || (node.hover_target_scale - 1.0).abs() >= f64::EPSILON);
            let target_changed = target_offset != node.hover_target_offset
                || (target_scale - node.hover_target_scale).abs() >= f64::EPSILON;
            let mut restart_hover = hover_finished || active_changed || returning_to_rest;
            if !restart_hover && target_changed {
                let progress = ((now - node.hover_started).as_secs_f64()
                    / node.hover_duration.as_secs_f64())
                .clamp(0.0, 1.0);
                let movement = node.hover_spring.sample(progress);
                if movement < 0.95 {
                    let remaining = 1.0 - movement;
                    node.hover_from_offset = Point {
                        x: (node.hover_offset.x - target_offset.x * movement) / remaining,
                        y: (node.hover_offset.y - target_offset.y * movement) / remaining,
                    };
                    node.hover_from_scale =
                        (node.hover_scale - target_scale * movement) / remaining;
                } else {
                    restart_hover = true;
                }
            }
            if restart_hover {
                node.hover_from_offset = node.hover_offset;
                node.hover_from_scale = node.hover_scale;
                node.hover_started = now;
                node.hover_duration = duration;
                node.hover_spring = spring;
            }
            node.hover_target_offset = target_offset;
            node.hover_target_scale = target_scale;
            node.active = target.active;
            if duration.is_zero() {
                node.hover_offset = target_offset;
                node.hover_from_offset = target_offset;
                node.hover_scale = target_scale;
                node.hover_from_scale = target_scale;
                node.position = Point {
                    x: node.base_position.x + target_offset.x,
                    y: node.base_position.y + target_offset.y,
                };
                node.size = node.base_size * target_scale;
                continue;
            }
            node.active = target.active;
        }
    }

    pub fn close(
        &mut self,
        selected: Option<(&NodeKey, Point, f64)>,
        close_duration: Duration,
        action_duration: Duration,
        spring: Spring,
    ) {
        let now = Instant::now();
        for node in self.nodes.values_mut() {
            node.sample(now);
            let selected_target = selected
                .as_ref()
                .filter(|(key, _, _)| *key == &node.key)
                .map(|(_, point, scale)| (*point, *scale));
            node.selected_action = selected_target.is_some();
            let (position, scale) = selected_target.unwrap_or((node.collapse_to, 0.0));
            let size = if selected_target.is_some() {
                node.size * scale
            } else {
                0.0
            };
            let duration = if selected_target.is_some() {
                action_duration
            } else {
                close_duration
            };
            node.retarget(
                TransitionTarget {
                    position,
                    size,
                    opacity: 0.0,
                    duration,
                    spring,
                    removing: true,
                    position_end: 1.0,
                    opacity_delay: if selected_target.is_some() {
                        2.0 / 3.0
                    } else {
                        0.0
                    },
                },
                now,
            );
        }
    }

    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        self.nodes.retain(|_, node| {
            changed |= !node.finished(now) || node.removing;
            node.sample(now);
            let finished = node.finished(now);
            !(node.removing && finished)
        });
        changed
    }

    pub fn nodes(&self) -> Vec<VisualNode> {
        self.nodes.values().cloned().collect()
    }

    pub fn is_animating(&self) -> bool {
        let now = Instant::now();
        self.nodes.values().any(|node| !node.finished(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(key: NodeKey, role: NodeRole, position: Point) -> NodeTarget {
        NodeTarget {
            item_path: match &key {
                NodeKey::Menu(path) | NodeKey::Action(path, _) => path.clone(),
            },
            key,
            role,
            position,
            rest_position: position,
            origin: Point::default(),
            size: 100.0,
            rest_size: 100.0,
            active: false,
            icon_visible: true,
        }
    }

    #[test]
    fn submenu_node_keeps_its_identity_when_it_becomes_the_center() {
        let key = NodeKey::Menu(vec![2]);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![target(
                key.clone(),
                NodeRole::Item,
                Point { x: 200.0, y: 100.0 },
            )],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            true,
        );
        animator.reconcile(
            vec![target(
                key.clone(),
                NodeRole::Center,
                Point { x: 300.0, y: 100.0 },
            )],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            true,
        );
        let nodes = animator.nodes();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].key, key);
        assert_eq!(nodes[0].role, NodeRole::Center);
        assert_eq!(nodes[0].position, Point { x: 300.0, y: 100.0 });
    }

    #[test]
    fn submenu_items_expand_from_the_moving_submenu_circle() {
        let submenu = NodeKey::Menu(vec![1]);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![target(
                submenu.clone(),
                NodeRole::Item,
                Point { x: 200.0, y: 100.0 },
            )],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );

        let mut center = target(submenu, NodeRole::Center, Point { x: 300.0, y: 100.0 });
        center.origin = Point { x: 300.0, y: 100.0 };
        let mut child = target(
            NodeKey::Action(vec![1], 0),
            NodeRole::Item,
            Point { x: 400.0, y: 100.0 },
        );
        child.origin = Point { x: 300.0, y: 100.0 };
        animator.reconcile(
            vec![center, child],
            Motion {
                duration: Duration::from_secs(1),
                spring: Spring::default(),
            },
            Motion {
                duration: Duration::from_secs(1),
                spring: Spring::default(),
            },
            Duration::ZERO,
            true,
        );

        let child = animator
            .nodes()
            .into_iter()
            .find(|node| node.key == NodeKey::Action(vec![1], 0))
            .unwrap();
        assert_eq!(child.position, Point { x: 200.0, y: 100.0 });
        assert_eq!(child.size, 0.0);
        let anchor = child.creation_anchor.unwrap();
        assert_eq!(anchor.from, Point { x: 200.0, y: 100.0 });
        assert_eq!(anchor.to, Point { x: 300.0, y: 100.0 });
    }

    #[test]
    fn follow_animation_is_independent_from_an_opening_transition() {
        let key = NodeKey::Action(vec![1], 0);
        let mut animator = Animator::default();
        let mut opening = target(key.clone(), NodeRole::Item, Point { x: 400.0, y: 100.0 });
        opening.origin = Point { x: 300.0, y: 100.0 };
        animator.reconcile(
            vec![opening],
            Motion {
                duration: Duration::from_secs(1),
                spring: Spring::default(),
            },
            Motion {
                duration: Duration::from_secs(1),
                spring: Spring::default(),
            },
            Duration::ZERO,
            true,
        );
        let started = animator.nodes[&key].started;
        let from_position = animator.nodes[&key].from_position;

        let mut followed = target(key.clone(), NodeRole::Item, Point { x: 430.0, y: 100.0 });
        followed.rest_position = Point { x: 400.0, y: 100.0 };
        animator.hover(
            vec![followed],
            Duration::from_millis(300),
            Duration::ZERO,
            Spring::default(),
        );

        let node = &animator.nodes[&key];
        assert_eq!(node.started, started);
        assert_eq!(node.from_position, from_position);
        assert_eq!(node.duration, Duration::from_secs(1));
        assert_eq!(node.target_position, Point { x: 400.0, y: 100.0 });
        assert_eq!(node.hover_target_offset, Point { x: 30.0, y: 0.0 });
        assert_eq!(node.hover_duration, Duration::from_millis(300));
    }

    #[test]
    fn follow_offset_gets_a_fresh_animation_when_returning_to_rest() {
        let key = NodeKey::Action(vec![], 0);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![target(
                key.clone(),
                NodeRole::Item,
                Point { x: 100.0, y: 100.0 },
            )],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );
        let mut followed = target(key.clone(), NodeRole::Item, Point { x: 130.0, y: 100.0 });
        followed.rest_position = Point { x: 100.0, y: 100.0 };
        let duration = Duration::from_secs(1);
        animator.hover(vec![followed], duration, Duration::ZERO, Spring::default());
        animator.nodes.get_mut(&key).unwrap().hover_started =
            Instant::now() - Duration::from_millis(500);
        animator.nodes.get_mut(&key).unwrap().sample(Instant::now());
        let displaced = animator.nodes[&key].hover_offset;

        animator.hover(
            vec![target(
                key.clone(),
                NodeRole::Item,
                Point { x: 100.0, y: 100.0 },
            )],
            duration,
            Duration::ZERO,
            Spring::default(),
        );

        let node = &animator.nodes[&key];
        assert!(node.hover_from_offset.distance(displaced) < 0.01);
        assert_eq!(node.hover_target_offset, Point::default());
        assert_eq!(node.hover_duration, duration);
    }

    #[test]
    fn changing_follow_direction_preserves_the_current_position() {
        let key = NodeKey::Action(vec![], 0);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![target(
                key.clone(),
                NodeRole::Item,
                Point { x: 100.0, y: 100.0 },
            )],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );
        let duration = Duration::from_secs(1);
        let mut first = target(key.clone(), NodeRole::Item, Point { x: 130.0, y: 100.0 });
        first.rest_position = Point { x: 100.0, y: 100.0 };
        animator.hover(vec![first], duration, Duration::ZERO, Spring::default());
        animator.nodes.get_mut(&key).unwrap().hover_started =
            Instant::now() - Duration::from_millis(250);
        animator.nodes.get_mut(&key).unwrap().sample(Instant::now());
        let before = animator.nodes[&key].position;

        let mut second = target(key.clone(), NodeRole::Item, Point { x: 100.0, y: 80.0 });
        second.rest_position = Point { x: 100.0, y: 100.0 };
        animator.hover(vec![second], duration, Duration::ZERO, Spring::default());

        assert!(animator.nodes[&key].position.distance(before) < 0.01);
        assert_eq!(
            animator.nodes[&key].hover_target_offset,
            Point { x: 0.0, y: -20.0 }
        );
    }
}

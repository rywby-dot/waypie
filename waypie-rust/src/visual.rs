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
    pub origin: Point,
    pub size: f64,
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
        let position_movement = self.spring.sample(
            position_progress,
            self.duration.as_secs_f64() * self.position_end,
        );
        let size_movement = self.spring.sample(progress, self.duration.as_secs_f64());
        let fade = smoothstep(
            ((progress - self.opacity_delay) / (1.0 - self.opacity_delay).max(f64::EPSILON))
                .clamp(0.0, 1.0),
        );
        self.position = self
            .from_position
            .lerp(self.target_position, position_movement);
        self.size = self.from_scale + (self.target_scale - self.from_scale) * size_movement;
        self.opacity = self.from_opacity + (self.target_opacity - self.from_opacity) * fade;
        let icon_progress = if self.icon_duration.is_zero() {
            1.0
        } else {
            ((now - self.icon_started).as_secs_f64() / self.icon_duration.as_secs_f64())
                .clamp(0.0, 1.0)
        };
        self.icon_opacity =
            self.icon_from + (self.icon_target - self.icon_from) * smoothstep(icon_progress);
        if progress >= 1.0 {
            self.position = self.target_position;
            self.size = self.target_scale;
            self.opacity = self.target_opacity;
            self.return_connector = false;
        }
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
        if self.duration.is_zero() {
            self.sample(now);
        }
    }

    fn movement_finished(&self, now: Instant) -> bool {
        self.duration.is_zero() || now - self.started >= self.duration
    }

    fn finished(&self, now: Instant) -> bool {
        self.movement_finished(now)
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
                    .map_or(node.collapse_to, |target| target.position);
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
                            position: target.position,
                            size: target.size,
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
                    node.position = target.position;
                    node.size = target.size;
                    node.opacity = 1.0;
                    node.from_position = target.position;
                    node.target_position = target.position;
                    node.from_scale = target.size;
                    node.target_scale = target.size;
                    node.from_opacity = 1.0;
                    node.target_opacity = 1.0;
                    node.removing = false;
                    node.return_connector = false;
                }
            } else {
                let create_animated = animate && !create_duration.is_zero();
                let initial = if create_animated { 0.0 } else { 1.0 };
                self.nodes.insert(
                    key.clone(),
                    VisualNode {
                        key,
                        item_path: target.item_path,
                        role: target.role,
                        position: target.origin,
                        size: target.size * initial,
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
                        collapse_to: target.origin,
                        from_position: target.origin,
                        target_position: target.position,
                        from_scale: target.size * initial,
                        target_scale: target.size,
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
            node.item_path = target.item_path;
            node.role = target.role;
            node.active = target.active;
            node.selected_action = false;
            node.collapse_to = target.origin;
            node.set_icon_visible(target.icon_visible, icon_duration, now);
            if duration.is_zero() {
                node.position = target.position;
                node.size = target.size;
                node.from_position = target.position;
                node.target_position = target.position;
                node.from_scale = target.size;
                node.target_scale = target.size;
                continue;
            }
            node.retarget(
                TransitionTarget {
                    position: target.position,
                    size: target.size,
                    opacity: 1.0,
                    duration,
                    spring,
                    removing: false,
                    position_end: 1.0,
                    opacity_delay: 0.0,
                },
                now,
            );
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
                    position_end: if selected_target.is_some() {
                        2.0 / 3.0
                    } else {
                        1.0
                    },
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
            origin: Point::default(),
            size: 100.0,
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
}

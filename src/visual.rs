use std::{
    collections::{HashMap, HashSet},
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

#[derive(Clone, Copy, Debug, Default)]
pub struct TransitionEffects {
    pub deletion_duration: Duration,
    pub icon_duration: Duration,
    pub connector_duration: Duration,
    pub indicator: Motion,
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
    pub connector_factor: f64,
    pub indicator_factor: f64,
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
    connector_from: f64,
    connector_target: f64,
    connector_started: Instant,
    connector_duration: Duration,
    indicator_from: f64,
    indicator_target: f64,
    indicator_started: Instant,
    indicator_duration: Duration,
    indicator_spring: Spring,
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
    use_spring: bool,
    removing: bool,
    position_end: f64,
    opacity_delay: f64,
    position_anchor: Option<PositionAnchor>,
}

#[derive(Clone, Debug)]
struct PositionAnchor {
    parent: NodeKey,
    from_offset: Point,
    target_offset: Point,
}

struct TransitionTarget {
    position: Point,
    size: f64,
    opacity: f64,
    duration: Duration,
    spring: Spring,
    use_spring: bool,
    removing: bool,
    position_end: f64,
    opacity_delay: f64,
}

fn parent_key(key: &NodeKey) -> Option<NodeKey> {
    match key {
        NodeKey::Action(path, _) => Some(NodeKey::Menu(path.clone())),
        NodeKey::Menu(path) if !path.is_empty() => {
            Some(NodeKey::Menu(path[..path.len() - 1].to_vec()))
        }
        NodeKey::Menu(_) => None,
    }
}

fn node_depth(key: &NodeKey) -> usize {
    match key {
        NodeKey::Menu(path) => path.len(),
        NodeKey::Action(path, _) => path.len() + 1,
    }
}

fn opacity_progress(progress: f64, fading_in: bool) -> f64 {
    if fading_in {
        progress.sqrt()
    } else {
        smoothstep(progress)
    }
}

impl VisualNode {
    pub fn is_removing(&self) -> bool {
        self.removing
    }

    fn set_connector_target(&mut self, target: f64, duration: Duration, now: Instant) {
        if (self.connector_target - target).abs() < f64::EPSILON {
            return;
        }
        self.connector_from = self.connector_factor;
        self.connector_target = target;
        self.connector_started = now;
        self.connector_duration = duration;
        if duration.is_zero() {
            self.connector_factor = target;
            self.connector_from = target;
        }
    }

    fn set_indicator_target(&mut self, target: f64, motion: Motion, now: Instant) {
        if (self.indicator_target - target).abs() < f64::EPSILON {
            return;
        }
        self.indicator_from = self.indicator_factor;
        self.indicator_target = target;
        self.indicator_started = now;
        self.indicator_duration = motion.duration;
        self.indicator_spring = motion.spring;
        if motion.duration.is_zero() {
            self.indicator_factor = target;
            self.indicator_from = target;
        }
    }

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
        let position_movement = if self.use_spring {
            self.spring.sample(position_progress)
        } else {
            smoothstep(position_progress)
        };
        let size_movement = if self.use_spring {
            self.spring.sample(progress)
        } else {
            smoothstep(progress)
        };
        let normalized_opacity = ((progress - self.opacity_delay)
            / (1.0 - self.opacity_delay).max(f64::EPSILON))
        .clamp(0.0, 1.0);
        let fade = opacity_progress(
            normalized_opacity,
            self.target_opacity > self.from_opacity && !self.removing,
        );
        self.base_position = self
            .from_position
            .lerp(self.target_position, position_movement);
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
        let connector_progress = if self.connector_duration.is_zero() {
            1.0
        } else {
            ((now - self.connector_started).as_secs_f64() / self.connector_duration.as_secs_f64())
                .clamp(0.0, 1.0)
        };
        self.connector_factor = self.connector_from
            + (self.connector_target - self.connector_from) * smoothstep(connector_progress);
        let indicator_progress = if self.indicator_duration.is_zero() {
            1.0
        } else {
            ((now - self.indicator_started).as_secs_f64() / self.indicator_duration.as_secs_f64())
                .clamp(0.0, 1.0)
        };
        self.indicator_factor = self.indicator_from
            + (self.indicator_target - self.indicator_from)
                * self.indicator_spring.sample(indicator_progress);
        if progress >= 1.0 {
            self.base_size = self.target_scale;
            self.opacity = self.target_opacity;
        }
        if self.movement_finished(now) {
            self.base_position = self.target_position;
            self.return_connector = false;
        }
        self.position = Point {
            x: self.base_position.x + self.hover_offset.x,
            y: self.base_position.y + self.hover_offset.y,
        };
        self.size = self.base_size * self.hover_scale;
    }

    fn retarget(&mut self, target: TransitionTarget, now: Instant) {
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
        self.use_spring = target.use_spring;
        self.removing = target.removing;
        self.position_end = target.position_end;
        self.opacity_delay = target.opacity_delay;
        self.position_anchor = None;
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
        self.duration.is_zero() || now - self.started >= self.duration
    }

    fn finished(&self, now: Instant) -> bool {
        self.movement_finished(now)
            && (self.hover_duration.is_zero() || now - self.hover_started >= self.hover_duration)
            && (self.connector_duration.is_zero()
                || now - self.connector_started >= self.connector_duration)
            && (self.indicator_duration.is_zero()
                || now - self.indicator_started >= self.indicator_duration)
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

    fn sample_all(&mut self, now: Instant) {
        for node in self.nodes.values_mut() {
            node.sample(now);
        }

        let mut anchored = self
            .nodes
            .iter()
            .filter_map(|(key, node)| node.position_anchor.as_ref().map(|_| key.clone()))
            .collect::<Vec<_>>();
        anchored.sort_by_key(node_depth);
        for key in anchored {
            let Some(anchor) = self
                .nodes
                .get(&key)
                .and_then(|node| node.position_anchor.clone())
            else {
                continue;
            };
            let Some(parent_position) = self.nodes.get(&anchor.parent).map(|node| node.position)
            else {
                continue;
            };
            let Some(node) = self.nodes.get_mut(&key) else {
                continue;
            };
            let progress = if node.duration.is_zero() {
                1.0
            } else {
                ((now - node.started).as_secs_f64() / node.duration.as_secs_f64()).clamp(0.0, 1.0)
            };
            let progress = (progress / node.position_end.max(f64::EPSILON)).clamp(0.0, 1.0);
            let movement = if node.use_spring {
                node.spring.sample(progress)
            } else {
                smoothstep(progress)
            };
            let offset = anchor.from_offset.lerp(anchor.target_offset, movement);
            node.base_position = Point {
                x: parent_position.x + offset.x,
                y: parent_position.y + offset.y,
            };
            node.position = Point {
                x: node.base_position.x + node.hover_offset.x,
                y: node.base_position.y + node.hover_offset.y,
            };
        }
    }

    pub fn reconcile(
        &mut self,
        targets: Vec<NodeTarget>,
        movement: Motion,
        creation: Motion,
        icon_duration: Duration,
        animate: bool,
    ) {
        self.reconcile_with_effects(
            targets,
            movement,
            creation,
            TransitionEffects {
                icon_duration,
                ..TransitionEffects::default()
            },
            animate,
        );
    }

    pub fn reconcile_with_effects(
        &mut self,
        targets: Vec<NodeTarget>,
        movement: Motion,
        creation: Motion,
        effects: TransitionEffects,
        animate: bool,
    ) {
        let TransitionEffects {
            deletion_duration,
            icon_duration,
            connector_duration,
            indicator,
        } = effects;
        let move_duration = movement.duration;
        let create_duration = creation.duration;
        let move_spring = movement.spring;
        let create_spring = creation.spring;
        let now = Instant::now();
        let animate = animate && (!move_duration.is_zero() || !create_duration.is_zero());
        self.sample_all(now);
        let targets = targets
            .into_iter()
            .map(|target| (target.key.clone(), target))
            .collect::<HashMap<_, _>>();
        let sampled_positions = self
            .nodes
            .iter()
            .map(|(key, node)| (key.clone(), node.position))
            .collect::<HashMap<_, _>>();
        let target_positions = targets
            .iter()
            .map(|(key, target)| (key.clone(), target.rest_position))
            .collect::<HashMap<_, _>>();
        for (key, node) in &mut self.nodes {
            if !targets.contains_key(key) && !node.removing {
                let parent_key = parent_key(key);
                if matches!(key, NodeKey::Menu(_)) {
                    node.set_indicator_target(0.0, indicator, now);
                }
                let destination = parent_key
                    .as_ref()
                    .and_then(|key| targets.get(key))
                    .map_or(node.collapse_to, |target| target.rest_position);
                node.retarget(
                    TransitionTarget {
                        position: destination,
                        size: 0.0,
                        opacity: 0.0,
                        duration: deletion_duration,
                        spring: Spring::default(),
                        use_spring: false,
                        removing: true,
                        position_end: 1.0,
                        opacity_delay: 0.0,
                    },
                    now,
                );
                if let Some(parent) = parent_key {
                    let parent_position = sampled_positions
                        .get(&parent)
                        .copied()
                        .or_else(|| targets.get(&parent).map(|target| target.rest_position))
                        .unwrap_or(destination);
                    node.position_anchor = Some(PositionAnchor {
                        parent,
                        from_offset: Point {
                            x: node.position.x - parent_position.x,
                            y: node.position.y - parent_position.y,
                        },
                        target_offset: Point::default(),
                    });
                }
            }
        }
        for (key, target) in targets {
            let parent = parent_key(&key);
            let parent_current = parent
                .as_ref()
                .and_then(|parent| sampled_positions.get(parent))
                .copied()
                .or_else(|| {
                    parent
                        .as_ref()
                        .and_then(|parent| target_positions.get(parent))
                        .copied()
                });
            let parent_target = parent
                .as_ref()
                .and_then(|parent| target_positions.get(parent))
                .copied();
            if let Some(node) = self.nodes.get_mut(&key) {
                let menu_node = matches!(&key, NodeKey::Menu(_));
                let had_indicators =
                    menu_node && matches!(node.role, NodeRole::Item | NodeRole::History);
                let wants_indicators =
                    menu_node && matches!(target.role, NodeRole::Item | NodeRole::History);
                let opening_connector = node.role == NodeRole::Item
                    && target.role == NodeRole::Center
                    && !target.item_path.is_empty();
                let returning =
                    animate && node.role == NodeRole::Center && target.role == NodeRole::Item;
                if opening_connector {
                    node.set_connector_target(1.0, connector_duration, now);
                } else if returning {
                    node.set_connector_target(0.0, connector_duration, now);
                }
                if had_indicators != wants_indicators {
                    node.set_indicator_target(f64::from(wants_indicators), indicator, now);
                }
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
                            use_spring: true,
                            removing: false,
                            position_end: 1.0,
                            opacity_delay: 0.0,
                        },
                        now,
                    );
                    if let (Some(parent), Some(parent_current), Some(parent_target)) =
                        (parent.clone(), parent_current, parent_target)
                    {
                        node.position_anchor = Some(PositionAnchor {
                            parent,
                            from_offset: Point {
                                x: node.position.x - parent_current.x,
                                y: node.position.y - parent_current.y,
                            },
                            target_offset: Point {
                                x: target.rest_position.x - parent_target.x,
                                y: target.rest_position.y - parent_target.y,
                            },
                        });
                    }
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
                    node.indicator_factor = f64::from(
                        matches!(target.role, NodeRole::Item | NodeRole::History)
                            && matches!(&key, NodeKey::Menu(_)),
                    );
                    node.indicator_from = node.indicator_factor;
                    node.indicator_target = node.indicator_factor;
                    node.indicator_duration = Duration::ZERO;
                    node.position_anchor = match (parent, parent_target) {
                        (Some(parent), Some(parent_target)) => Some(PositionAnchor {
                            parent,
                            from_offset: Point {
                                x: target.rest_position.x - parent_target.x,
                                y: target.rest_position.y - parent_target.y,
                            },
                            target_offset: Point {
                                x: target.rest_position.x - parent_target.x,
                                y: target.rest_position.y - parent_target.y,
                            },
                        }),
                        _ => None,
                    };
                }
            } else {
                let create_animated = animate && !create_duration.is_zero();
                let initial = if create_animated { 0.0 } else { 1.0 };
                let creates_connector =
                    target.role == NodeRole::Center && !target.item_path.is_empty();
                let creates_indicator = matches!(target.role, NodeRole::Item | NodeRole::History)
                    && matches!(&key, NodeKey::Menu(_));
                let parent = parent_key(&key);
                let parent_position = parent
                    .as_ref()
                    .and_then(|parent| self.nodes.get(parent))
                    .map(|parent| parent.position);
                let creation_origin = parent_position.unwrap_or(target.origin);
                let position_anchor = parent.map(|parent| PositionAnchor {
                    parent,
                    from_offset: Point::default(),
                    target_offset: Point {
                        x: target.rest_position.x - parent_target.unwrap_or(target.origin).x,
                        y: target.rest_position.y - parent_target.unwrap_or(target.origin).y,
                    },
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
                        connector_factor: 0.0,
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
                        connector_from: 0.0,
                        connector_target: if creates_connector { 1.0 } else { 0.0 },
                        connector_started: now,
                        connector_duration: if creates_connector {
                            connector_duration
                        } else {
                            Duration::ZERO
                        },
                        indicator_factor: 0.0,
                        indicator_from: 0.0,
                        indicator_target: if creates_indicator { 1.0 } else { 0.0 },
                        indicator_started: now,
                        indicator_duration: if creates_indicator {
                            indicator.duration
                        } else {
                            Duration::ZERO
                        },
                        indicator_spring: indicator.spring,
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
                        use_spring: true,
                        removing: false,
                        position_end: 1.0,
                        opacity_delay: 0.0,
                        position_anchor,
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
        let motion = Motion { duration, spring };
        self.hover_with_follow(targets, motion, motion, icon_duration);
    }

    pub fn hover_with_follow(
        &mut self,
        targets: Vec<NodeTarget>,
        hover: Motion,
        follow: Motion,
        icon_duration: Duration,
    ) {
        let now = Instant::now();
        self.sample_all(now);
        for target in targets {
            let Some(node) = self.nodes.get_mut(&target.key) else {
                continue;
            };
            if node.removing {
                continue;
            }
            let active_changed = node.active != target.active;
            let motion = if node.active || target.active {
                hover
            } else {
                follow
            };
            let duration = motion.duration;
            let spring = motion.spring;
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
        self.close_with_effects(
            selected,
            close_duration,
            action_duration,
            Duration::ZERO,
            Motion::default(),
            spring,
        );
    }

    pub fn close_with_effects(
        &mut self,
        selected: Option<(&NodeKey, Point, f64)>,
        close_duration: Duration,
        action_duration: Duration,
        connector_duration: Duration,
        indicator: Motion,
        spring: Spring,
    ) {
        let now = Instant::now();
        self.sample_all(now);
        let current_center = self
            .nodes
            .iter()
            .find(|(_, node)| node.role == NodeRole::Center && !node.removing)
            .map(|(key, node)| (key.clone(), node.position, node.target_position));
        for node in self.nodes.values_mut() {
            let selected_target = selected
                .as_ref()
                .filter(|(key, _, _)| *key == &node.key)
                .map(|(_, point, scale)| (*point, *scale));
            node.selected_action = selected_target.is_some();
            if selected_target.is_some() {
                node.connector_factor = 1.0;
                node.connector_from = 1.0;
                node.connector_target = 0.0;
                node.connector_started = now;
                node.connector_duration = connector_duration;
            } else if matches!(&node.key, NodeKey::Menu(_)) {
                let connector_target = if node.role == NodeRole::Item {
                    1.0
                } else {
                    0.0
                };
                node.set_connector_target(connector_target, connector_duration, now);
                if node.role == NodeRole::Item {
                    node.set_indicator_target(0.0, indicator, now);
                }
            }
            let collapse_position = if node.role == NodeRole::Item {
                current_center
                    .as_ref()
                    .map_or(node.collapse_to, |(_, _, target)| *target)
            } else {
                node.collapse_to
            };
            let (position, scale) = selected_target.unwrap_or((collapse_position, 0.0));
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
                    use_spring: selected_target.is_some(),
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
            if selected_target.is_none()
                && node.role == NodeRole::Item
                && let Some((parent, parent_position, _)) = &current_center
            {
                node.position_anchor = Some(PositionAnchor {
                    parent: parent.clone(),
                    from_offset: Point {
                        x: node.position.x - parent_position.x,
                        y: node.position.y - parent_position.y,
                    },
                    target_offset: Point::default(),
                });
            }
        }
    }

    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        self.sample_all(now);
        let retained_parents = self
            .nodes
            .values()
            .filter(|node| !node.finished(now))
            .filter_map(|node| {
                node.position_anchor
                    .as_ref()
                    .map(|anchor| anchor.parent.clone())
            })
            .collect::<HashSet<_>>();
        let mut changed = false;
        self.nodes.retain(|key, node| {
            changed |= !node.finished(now) || node.removing;
            let finished = node.finished(now);
            !(node.removing && finished && !retained_parents.contains(key))
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

    pub fn remaining_duration(&self) -> Duration {
        let now = Instant::now();
        self.nodes
            .values()
            .flat_map(|node| {
                [
                    (node.started, node.duration),
                    (node.hover_started, node.hover_duration),
                    (node.connector_started, node.connector_duration),
                    (node.indicator_started, node.indicator_duration),
                    (node.icon_started, node.icon_duration),
                ]
            })
            .map(|(started, duration)| duration.saturating_sub(now.duration_since(started)))
            .max()
            .unwrap_or(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearing_items_become_visible_at_the_start_of_the_transition() {
        assert_eq!(opacity_progress(0.0, true), 0.0);
        assert!(opacity_progress(0.05, true) > 0.2);
        assert_eq!(opacity_progress(1.0, true), 1.0);
        assert!(opacity_progress(0.05, true) > opacity_progress(0.05, false));
    }

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
    fn previous_menu_circle_reveals_its_own_indicators() {
        let key = NodeKey::Menu(vec![]);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![target(key.clone(), NodeRole::Center, Point::default())],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );
        let indicator = Motion {
            duration: Duration::from_secs(1),
            spring: Spring::default(),
        };
        animator.reconcile_with_effects(
            vec![target(key.clone(), NodeRole::History, Point::default())],
            Motion {
                duration: Duration::from_secs(1),
                spring: Spring::default(),
            },
            Motion::default(),
            TransitionEffects {
                indicator,
                ..TransitionEffects::default()
            },
            true,
        );

        let node = &animator.nodes[&key];
        assert_eq!(node.role, NodeRole::History);
        assert_eq!(node.indicator_target, 1.0);
        assert_eq!(node.indicator_duration, indicator.duration);
    }

    #[test]
    fn closing_during_a_return_collapses_stale_items_into_the_current_center() {
        let center_key = NodeKey::Menu(vec![]);
        let stale_item_key = NodeKey::Action(vec![1], 0);
        let center = Point { x: 300.0, y: 100.0 };
        let stale_submenu_center = Point { x: 500.0, y: 100.0 };
        let mut animator = Animator::default();

        let center_target = target(center_key, NodeRole::Center, center);
        let mut stale_item = target(
            stale_item_key.clone(),
            NodeRole::Item,
            Point { x: 550.0, y: 100.0 },
        );
        stale_item.origin = stale_submenu_center;
        animator.reconcile(
            vec![center_target, stale_item],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );

        animator.close(
            None,
            Duration::from_secs(1),
            Duration::from_secs(1),
            Spring::default(),
        );

        let stale_item = &animator.nodes[&stale_item_key];
        assert_eq!(stale_item.collapse_to, stale_submenu_center);
        assert_eq!(stale_item.target_position, center);
        assert!(stale_item.removing);
    }

    #[test]
    fn disappearing_descendant_stays_attached_when_its_parent_is_retargeted() {
        let parent_key = NodeKey::Menu(vec![1]);
        let child_key = NodeKey::Action(vec![1], 0);
        let mut animator = Animator::default();
        animator.reconcile(
            vec![
                target(
                    parent_key.clone(),
                    NodeRole::Center,
                    Point { x: 200.0, y: 100.0 },
                ),
                target(
                    child_key.clone(),
                    NodeRole::Item,
                    Point { x: 300.0, y: 100.0 },
                ),
            ],
            Motion::default(),
            Motion::default(),
            Duration::ZERO,
            false,
        );

        animator.reconcile(
            vec![target(
                parent_key.clone(),
                NodeRole::Item,
                Point { x: 100.0, y: 100.0 },
            )],
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

        assert_eq!(
            animator.nodes[&child_key]
                .position_anchor
                .as_ref()
                .map(|anchor| &anchor.parent),
            Some(&parent_key)
        );

        let halfway = Instant::now() - Duration::from_millis(500);
        animator.nodes.get_mut(&parent_key).unwrap().started = halfway;
        animator.nodes.get_mut(&child_key).unwrap().started = halfway;
        animator.sample_all(Instant::now());
        let first_parent = animator.nodes[&parent_key].position;
        let first_child = animator.nodes[&child_key].position;

        let parent = animator.nodes.get_mut(&parent_key).unwrap();
        parent.from_position.x += 40.0;
        parent.target_position.x += 40.0;
        animator.sample_all(Instant::now());
        let parent_shift = animator.nodes[&parent_key].position.x - first_parent.x;
        let child_shift = animator.nodes[&child_key].position.x - first_child.x;

        assert!((parent_shift - 40.0).abs() < 0.1);
        assert!((child_shift - parent_shift).abs() < 0.1);
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
        let anchor = child.position_anchor.unwrap();
        assert_eq!(anchor.parent, NodeKey::Menu(vec![1]));
        assert_eq!(anchor.from_offset, Point::default());
        assert_eq!(anchor.target_offset, Point { x: 100.0, y: 0.0 });
    }

    #[test]
    fn child_is_linked_when_it_is_created_together_with_its_parent() {
        let parent_key = NodeKey::Menu(vec![1]);
        let child_key = NodeKey::Action(vec![1], 0);
        let mut animator = Animator::default();
        let mut parent = target(
            parent_key.clone(),
            NodeRole::Center,
            Point { x: 200.0, y: 100.0 },
        );
        parent.origin = Point { x: 100.0, y: 100.0 };
        let mut child = target(
            child_key.clone(),
            NodeRole::Item,
            Point { x: 300.0, y: 100.0 },
        );
        child.origin = Point { x: 200.0, y: 100.0 };

        animator.reconcile(
            vec![child, parent],
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

        let anchor = animator.nodes[&child_key].position_anchor.as_ref().unwrap();
        assert_eq!(anchor.parent, parent_key);
        assert_eq!(anchor.from_offset, Point::default());
        assert_eq!(anchor.target_offset, Point { x: 100.0, y: 0.0 });
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

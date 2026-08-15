use std::{
    collections::HashMap,
    fs,
    hash::Hash,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use cosmic_text::{
    Align, Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache,
    Weight, Wrap,
};
use image::ImageReader;
use tiny_skia::{
    Color as SkColor, FillRule, FilterQuality, Mask, Paint, PathBuilder, Pattern, Pixmap, Rect,
    SpreadMode, Stroke, Transform,
};

const ICON_SOURCE_PADDING: u32 = 2;
const INDICATOR_CLIP_OVERLAP: f64 = 1.0;

use crate::{
    animation::{Spring, smoothstep},
    appearance::node_style,
    config::{Config, Item, item_at_path},
    geometry::{Point, radial_position},
    model::{MenuState, Target},
    style::{AnimationStyle, CircleStyle, Color, ItemKeyStyle, StyleSheet},
    visual::{NodeKey, NodeRole, VisualNode},
};

pub struct Scene<'a> {
    pub config: &'a Config,
    pub styles: &'a StyleSheet,
    pub state: &'a MenuState,
    pub nodes: &'a [VisualNode],
    pub icon_root: &'a Path,
}

struct IndicatorFrame<'a> {
    key: &'a NodeKey,
    item: &'a Item,
    submenu_style: &'a CircleStyle,
    styles: &'a StyleSheet,
    active: bool,
    return_circle: bool,
    skip_index: Option<usize>,
    scale: f64,
    reveal: f64,
}

struct ItemFrame<'a> {
    key: &'a NodeKey,
    center: Point,
    item: &'a Item,
    style: &'a CircleStyle,
    styles: &'a StyleSheet,
    icon_root: &'a Path,
    active: bool,
    content: ItemContent<'a>,
    indicators: bool,
    indicators_return: bool,
    indicator_skip_index: Option<usize>,
    indicator_reveal: f64,
    geometry_size: f64,
    opacity: f64,
    icon_opacity: f64,
}

#[derive(Clone, Copy)]
enum ItemContent<'a> {
    Default,
    Label(&'a str),
    IconOrLabel,
}

fn visible_label<'a>(
    content: ItemContent<'a>,
    default_label: &'a str,
    icon_drawn: bool,
) -> Option<&'a str> {
    if icon_drawn && !matches!(content, ItemContent::Label(_)) {
        return None;
    }
    Some(match content {
        ItemContent::Default | ItemContent::IconOrLabel => default_label,
        ItemContent::Label(label) => label,
    })
}

fn connector_is_active(role: NodeRole, active: bool) -> bool {
    role == NodeRole::History && active
}

fn temporary_connector_parent(node: &VisualNode) -> Option<&[usize]> {
    match &node.key {
        NodeKey::Action(path, _) if node.selected_action || node.travel_connector => {
            Some(path.as_slice())
        }
        NodeKey::Menu(path) if node.return_connector && !path.is_empty() => {
            Some(&path[..path.len() - 1])
        }
        NodeKey::Menu(path)
            if node.travel_connector && node.role == NodeRole::Item && !path.is_empty() =>
        {
            Some(&path[..path.len() - 1])
        }
        _ => None,
    }
}

fn item_key_radial_factor(distance: f64, presence: f64) -> f64 {
    if distance < 0.0 { 1.0 } else { presence }
}

fn item_key_is_under_circle(
    removing: bool,
    node_opacity: f64,
    presence: f64,
    target_presence: f64,
    anchored: bool,
) -> bool {
    (presence - target_presence).abs() > f64::EPSILON
        || !anchored && (removing || node_opacity < 1.0 - f64::EPSILON)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ColorAnimationKey {
    Overlay,
    NodeBackground(NodeKey),
    NodeBorder(NodeKey),
    NodeContent(NodeKey),
    Connector(NodeKey),
    Indicator(NodeKey),
    ItemKey(NodeKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum OpacityAnimationKey {
    Overlay,
    Node(NodeKey),
    NodeContent(NodeKey),
    Connector(NodeKey),
    Indicator(NodeKey),
    ItemKey(NodeKey),
}

#[derive(Clone, Copy)]
struct TimedTransition<T> {
    current: T,
    from: T,
    target: T,
    started: Instant,
    duration: Duration,
}

impl<T: Copy + PartialEq> TimedTransition<T> {
    fn sample(&mut self, now: Instant, interpolate: impl FnOnce(T, T, f64) -> T) -> T {
        let progress = if self.duration.is_zero() {
            1.0
        } else {
            ((now - self.started).as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0)
        };
        let progress = progress * progress * (3.0 - 2.0 * progress);
        self.current = if progress >= 1.0 {
            self.target
        } else {
            interpolate(self.from, self.target, progress)
        };
        self.current
    }

    fn finished(&self, now: Instant) -> bool {
        self.duration.is_zero() || now - self.started >= self.duration
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.duration
            .saturating_sub(now.duration_since(self.started))
    }
}

type ColorAnimation = TimedTransition<Color>;
type OpacityAnimation = TimedTransition<f64>;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ItemKeyGeometry {
    angle: f64,
    distance: f64,
    font_size: f64,
    font_weight: f64,
}

#[derive(Clone, Copy)]
struct SpringTransition<T> {
    current: T,
    from: T,
    target: T,
    started: Instant,
    spring: Spring,
}

#[derive(Clone, Copy)]
struct ItemKeyPresenceTransition {
    current: f64,
    from: f64,
    target: f64,
    started: Instant,
    duration: Duration,
    spring: Option<Spring>,
}

impl ItemKeyPresenceTransition {
    fn sample(&mut self, now: Instant) -> f64 {
        let progress = if self.duration.is_zero() {
            1.0
        } else {
            now.duration_since(self.started).as_secs_f64() / self.duration.as_secs_f64()
        };
        self.current = if progress >= 1.0 {
            self.target
        } else {
            let movement = self
                .spring
                .map_or_else(|| smoothstep(progress), |spring| spring.sample(progress));
            self.from + (self.target - self.from) * movement
        };
        self.current
    }

    fn finished(&self, now: Instant) -> bool {
        self.duration.is_zero() || now.duration_since(self.started) >= self.duration
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.duration
            .saturating_sub(now.duration_since(self.started))
    }
}

impl<T: Copy + PartialEq> SpringTransition<T> {
    fn sample(&mut self, now: Instant, interpolate: impl FnOnce(T, T, f64) -> T) -> T {
        let duration = self.spring.duration();
        let progress = if duration.is_zero() {
            1.0
        } else {
            now.duration_since(self.started).as_secs_f64() / duration.as_secs_f64()
        };
        self.current = if progress >= 1.0 {
            self.target
        } else {
            interpolate(self.from, self.target, self.spring.sample(progress))
        };
        self.current
    }

    fn finished(&self, now: Instant) -> bool {
        now.duration_since(self.started) >= self.spring.duration()
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.spring
            .duration()
            .saturating_sub(now.duration_since(self.started))
    }
}

fn animate_spring_transition<K, T>(
    animations: &mut HashMap<K, SpringTransition<T>>,
    key: K,
    target: T,
    spring: Spring,
    interpolate: impl Fn(T, T, f64) -> T + Copy,
) -> T
where
    K: Eq + Hash,
    T: Copy + PartialEq,
{
    let now = Instant::now();
    let animation = animations.entry(key).or_insert(SpringTransition {
        current: target,
        from: target,
        target,
        started: now,
        spring,
    });
    animation.sample(now, interpolate);
    if animation.target != target {
        animation.from = animation.current;
        animation.target = target;
        animation.started = now;
        animation.spring = spring;
    }
    animation.sample(now, interpolate)
}

fn animate_transition<K, T>(
    animations: &mut HashMap<K, TimedTransition<T>>,
    key: K,
    target: T,
    duration: Duration,
    interpolate: impl Fn(T, T, f64) -> T + Copy,
) -> T
where
    K: Eq + Hash,
    T: Copy + PartialEq,
{
    let now = Instant::now();
    let animation = animations.entry(key).or_insert(TimedTransition {
        current: target,
        from: target,
        target,
        started: now,
        duration: Duration::ZERO,
    });
    animation.sample(now, interpolate);
    if animation.target != target {
        animation.from = animation.current;
        animation.target = target;
        animation.started = now;
        animation.duration = duration;
    }
    animation.sample(now, interpolate)
}

pub struct Renderer {
    fonts: FontSystem,
    glyphs: SwashCache,
    icons: HashMap<(PathBuf, u32, [u8; 4]), Pixmap>,
    font_specs: HashMap<(String, u16), FontSpec>,
    configured_fonts: Vec<(String, u16)>,
    resolved_families: HashMap<String, String>,
    color_animations: HashMap<ColorAnimationKey, ColorAnimation>,
    color_duration: Duration,
    opacity_animations: HashMap<OpacityAnimationKey, OpacityAnimation>,
    opacity_duration: Duration,
    item_key_animations: HashMap<NodeKey, SpringTransition<ItemKeyGeometry>>,
    font_weight_animations: HashMap<NodeKey, SpringTransition<f64>>,
    item_key_presence: HashMap<NodeKey, ItemKeyPresenceTransition>,
    item_key_spring: Spring,
    item_key_create_spring: Spring,
    item_key_delete_duration: Duration,
    item_key_animations_enabled: bool,
}

#[derive(Clone)]
struct FontSpec {
    family: String,
    file: PathBuf,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            fonts: empty_font_system(),
            glyphs: SwashCache::new(),
            icons: HashMap::new(),
            font_specs: HashMap::new(),
            configured_fonts: vec![],
            resolved_families: HashMap::new(),
            color_animations: HashMap::new(),
            color_duration: Duration::ZERO,
            opacity_animations: HashMap::new(),
            opacity_duration: Duration::ZERO,
            item_key_animations: HashMap::new(),
            font_weight_animations: HashMap::new(),
            item_key_presence: HashMap::new(),
            item_key_spring: Spring::default(),
            item_key_create_spring: Spring::default(),
            item_key_delete_duration: Duration::ZERO,
            item_key_animations_enabled: true,
        }
    }

    pub fn configure_fonts(&mut self, mut requests: Vec<(String, u16)>) {
        requests.sort_unstable();
        requests.dedup();
        if requests == self.configured_fonts {
            return;
        }
        let mut database = cosmic_text::fontdb::Database::new();
        let mut resolved = HashMap::new();
        for (requested, weight) in &requests {
            let key = (requested.clone(), *weight);
            let spec = self
                .font_specs
                .get(&key)
                .cloned()
                .or_else(|| resolve_font(requested, *weight));
            let Some(spec) = spec else {
                eprintln!(
                    "waypie: font-family {requested:?} with weight {weight} was not found by fc-match"
                );
                continue;
            };
            if database.load_font_file(&spec.file).is_ok() {
                resolved.insert(requested.clone(), spec.family.clone());
                self.font_specs.insert(key, spec);
            }
        }
        if let Some(family) = resolved.get("Sans").or_else(|| resolved.values().next()) {
            database.set_sans_serif_family(family);
            database.set_serif_family(family);
            database.set_monospace_family(family);
        }
        self.fonts = FontSystem::new_with_locale_and_db(current_locale(), database);
        self.glyphs = SwashCache::new();
        self.configured_fonts = requests;
        self.resolved_families = resolved;
    }

    pub fn configure_animations(&mut self, animation: AnimationStyle) {
        self.color_duration = animation.color_duration;
        self.opacity_duration = animation.opacity_duration;
        self.item_key_spring = animation.hover_spring;
        self.item_key_create_spring = animation.item_create_spring;
        self.item_key_delete_duration = animation.item_delete_duration;
        self.item_key_animations_enabled = !animation.off;
        if animation.off {
            self.item_key_animations.clear();
            self.font_weight_animations.clear();
            self.item_key_presence.clear();
        }
    }

    pub fn is_animating(&self) -> bool {
        let now = Instant::now();
        self.color_animations
            .values()
            .any(|animation| !animation.finished(now) || animation.current != animation.target)
            || self
                .opacity_animations
                .values()
                .any(|animation| !animation.finished(now) || animation.current != animation.target)
            || self
                .item_key_animations
                .values()
                .any(|animation| !animation.finished(now) || animation.current != animation.target)
            || self
                .font_weight_animations
                .values()
                .any(|animation| !animation.finished(now) || animation.current != animation.target)
            || self
                .item_key_presence
                .values()
                .any(|animation| !animation.finished(now) || animation.current != animation.target)
    }

    pub fn remaining_duration(&self) -> Duration {
        let now = Instant::now();
        let color = self
            .color_animations
            .values()
            .map(|animation| animation.remaining(now))
            .max()
            .unwrap_or(Duration::ZERO);
        let opacity = self
            .opacity_animations
            .values()
            .map(|animation| animation.remaining(now))
            .max()
            .unwrap_or(Duration::ZERO);
        let item_key = self
            .item_key_animations
            .values()
            .map(|animation| animation.remaining(now))
            .max()
            .unwrap_or(Duration::ZERO);
        let font_weight = self
            .font_weight_animations
            .values()
            .map(|animation| animation.remaining(now))
            .max()
            .unwrap_or(Duration::ZERO);
        let item_key_presence = self
            .item_key_presence
            .values()
            .map(|animation| animation.remaining(now))
            .max()
            .unwrap_or(Duration::ZERO);
        color
            .max(opacity)
            .max(item_key)
            .max(font_weight)
            .max(item_key_presence)
    }

    fn animated_color(&mut self, key: ColorAnimationKey, target: Color) -> Color {
        animate_transition(
            &mut self.color_animations,
            key,
            target,
            self.color_duration,
            |from, target, progress| lerp_color(from, target, progress as f32),
        )
    }

    fn animated_opacity(&mut self, key: OpacityAnimationKey, target: f64) -> f64 {
        animate_transition(
            &mut self.opacity_animations,
            key,
            target,
            self.opacity_duration,
            |from, target, progress| from + (target - from) * progress,
        )
    }

    fn animated_item_key_geometry(
        &mut self,
        key: NodeKey,
        target: ItemKeyGeometry,
    ) -> ItemKeyGeometry {
        if !self.item_key_animations_enabled {
            return target;
        }
        animate_spring_transition(
            &mut self.item_key_animations,
            key,
            target,
            self.item_key_spring,
            |from, target, progress| {
                let angle_delta = (target.angle - from.angle + 540.0).rem_euclid(360.0) - 180.0;
                ItemKeyGeometry {
                    angle: (from.angle + angle_delta * progress).rem_euclid(360.0),
                    distance: from.distance + (target.distance - from.distance) * progress,
                    font_size: from.font_size + (target.font_size - from.font_size) * progress,
                    font_weight: from.font_weight
                        + (target.font_weight - from.font_weight) * progress,
                }
            },
        )
    }

    fn animated_font_weight(&mut self, key: NodeKey, target: u16) -> u16 {
        if !self.item_key_animations_enabled {
            return target;
        }
        animate_spring_transition(
            &mut self.font_weight_animations,
            key,
            f64::from(target),
            self.item_key_spring,
            |from, target, progress| from + (target - from) * progress,
        )
        .round()
        .clamp(1.0, 1000.0) as u16
    }

    fn animated_item_key_presence(
        &mut self,
        key: NodeKey,
        target: f64,
        initial: f64,
        anchored: bool,
    ) -> f64 {
        if !self.item_key_animations_enabled {
            return target;
        }
        let now = Instant::now();
        let transition = self
            .item_key_presence
            .entry(key)
            .or_insert(ItemKeyPresenceTransition {
                current: initial,
                from: initial,
                target: initial,
                started: now,
                duration: Duration::ZERO,
                spring: None,
            });
        transition.sample(now);
        if (transition.target - target).abs() > f64::EPSILON {
            transition.from = transition.current;
            transition.target = target;
            transition.started = now;
            if anchored {
                transition.duration = self.opacity_duration;
                transition.spring = None;
            } else if target > transition.current {
                transition.duration = self.item_key_create_spring.duration();
                transition.spring = Some(self.item_key_create_spring);
            } else {
                transition.duration = self.item_key_delete_duration;
                transition.spring = None;
            }
        }
        transition.sample(now)
    }

    pub fn render(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        let mut overlay = scene.styles.circle(&["overlay"]).unwrap_or_default();
        overlay.background_color =
            self.animated_color(ColorAnimationKey::Overlay, overlay.background_color);
        overlay.opacity = self.animated_opacity(OpacityAnimationKey::Overlay, overlay.opacity);
        pixmap.fill(to_skia(overlay.background_color, overlay.opacity));
        let path = scene.state.path();
        let current = scene.state.current(scene.config);
        if scene.nodes.is_empty() {
            return;
        }
        self.draw_connectors(pixmap, scene);
        let mut nodes = scene.nodes.iter().collect::<Vec<_>>();
        nodes.sort_by_key(|node| match node.role {
            NodeRole::History => 0,
            NodeRole::Center => 1,
            NodeRole::Item => 2,
        });
        for node in &nodes {
            let item = item_at_path(&scene.config.menu, &node.item_path);
            self.draw_node_item_key(pixmap, scene, node, item, true);
        }
        for node in &nodes {
            let item = item_at_path(&scene.config.menu, &node.item_path);
            let mut style = node_style(scene.styles, item, node.role, node.active);
            style.font_weight = self.animated_font_weight(node.key.clone(), style.font_weight);
            let content_opacity = style.content_opacity();
            style.opacity =
                self.animated_opacity(OpacityAnimationKey::Node(node.key.clone()), style.opacity);
            style.text_opacity = Some(self.animated_opacity(
                OpacityAnimationKey::NodeContent(node.key.clone()),
                content_opacity,
            ));
            style.background_color = self.animated_color(
                ColorAnimationKey::NodeBackground(node.key.clone()),
                style.background_color,
            );
            style.border_color = self.animated_color(
                ColorAnimationKey::NodeBorder(node.key.clone()),
                style.border_color,
            );
            style.color = self.animated_color(
                ColorAnimationKey::NodeContent(node.key.clone()),
                style.color,
            );
            let content = match node.role {
                NodeRole::Center if node.item_path == path => match scene.state.active() {
                    Some(Target::Item(index)) => current
                        .items
                        .get(index)
                        .map_or(ItemContent::Default, |item| ItemContent::Label(&item.label)),
                    Some(Target::Parent(_)) => ItemContent::IconOrLabel,
                    _ => ItemContent::Default,
                },
                NodeRole::History if node.active => ItemContent::IconOrLabel,
                _ => ItemContent::Default,
            };
            self.draw_item(
                pixmap,
                ItemFrame {
                    key: &node.key,
                    center: node.position,
                    item,
                    style: &style,
                    styles: scene.styles,
                    icon_root: scene.icon_root,
                    active: node.active,
                    content,
                    indicators: matches!(node.role, NodeRole::Item | NodeRole::History)
                        || (node.role == NodeRole::Center
                            && !node.is_removing()
                            && node.indicator_factor() > 0.0),
                    indicators_return: node.indicator_is_return(),
                    indicator_skip_index: if node.indicator_is_return() {
                        path.get(node.item_path.len()).copied()
                    } else {
                        None
                    },
                    indicator_reveal: (node.indicator_factor() * node.opacity)
                        .clamp(0.0, 1.0)
                        .sqrt(),
                    geometry_size: node.size,
                    opacity: node.opacity,
                    icon_opacity: node.icon_opacity(),
                },
            );
        }
        for node in &nodes {
            let item = item_at_path(&scene.config.menu, &node.item_path);
            self.draw_node_item_key(pixmap, scene, node, item, false);
        }
    }

    fn draw_node_item_key(
        &mut self,
        pixmap: &mut Pixmap,
        scene: &Scene<'_>,
        node: &VisualNode,
        item: &Item,
        under_circle: bool,
    ) {
        if item.keys.is_empty() {
            return;
        }
        let mut key_style = scene.styles.item_key(node.active).unwrap_or_default();
        let resting_distance = scene
            .styles
            .item_key(false)
            .map_or(key_style.distance, |style| style.distance);
        let anchored = resting_distance < 0.0;
        let target_presence = f64::from(
            node.role == NodeRole::Item && key_style.enabled && (!anchored || !node.is_removing()),
        );
        let initial_presence = if target_presence > 0.0 && node.opacity < 1.0 {
            0.0
        } else {
            target_presence
        };
        let presence = self
            .animated_item_key_presence(
                node.key.clone(),
                target_presence,
                initial_presence,
                anchored,
            )
            .max(0.0);
        if presence <= f64::EPSILON {
            return;
        }
        if item_key_is_under_circle(
            node.is_removing(),
            node.opacity,
            presence,
            target_presence,
            anchored,
        ) != under_circle
        {
            return;
        }
        let geometry = self.animated_item_key_geometry(
            node.key.clone(),
            ItemKeyGeometry {
                angle: key_style.angle,
                distance: key_style.distance,
                font_size: key_style.font_size * key_style.scale,
                font_weight: f64::from(key_style.font_weight),
            },
        );
        key_style.angle = geometry.angle;
        key_style.distance = geometry.distance;
        key_style.font_size = geometry.font_size;
        key_style.font_weight = geometry.font_weight.round().clamp(1.0, 1000.0) as u16;
        key_style.scale = 1.0;
        key_style.color = self.animated_color(
            ColorAnimationKey::ItemKey(node.key.clone()),
            key_style.color,
        );
        let node_opacity = if anchored { 1.0 } else { node.opacity };
        key_style.opacity = self.animated_opacity(
            OpacityAnimationKey::ItemKey(node.key.clone()),
            key_style.opacity,
        ) * node_opacity
            * presence.clamp(0.0, 1.0);
        let removal_scale = if anchored { 1.0 } else { node.removal_scale() };
        key_style.distance *= removal_scale;
        key_style.font_size *= removal_scale * presence;
        self.draw_item_key(
            pixmap,
            node.position,
            node.size,
            &item.keys,
            &key_style,
            item_key_radial_factor(resting_distance, presence),
        );
    }

    fn draw_connectors(&mut self, pixmap: &mut Pixmap, scene: &Scene<'_>) {
        let path = scene.state.path();
        if path.is_empty()
            && !scene
                .nodes
                .iter()
                .any(|node| node.selected_action || node.return_connector || node.travel_connector)
        {
            return;
        }
        let mut paint = Paint::default();
        for depth in 0..path.len() {
            let start_key = NodeKey::Menu(path[..depth].to_vec());
            let end_key = NodeKey::Menu(path[..=depth].to_vec());
            let Some(start_node) = scene.nodes.iter().find(|node| node.key == start_key) else {
                continue;
            };
            let Some(end_node) = scene.nodes.iter().find(|node| node.key == end_key) else {
                continue;
            };
            let connector_factor = end_node.connector_factor().clamp(0.0, 1.0);
            if connector_factor <= f64::EPSILON {
                continue;
            }
            let selectors = if connector_is_active(start_node.role, start_node.active) {
                ["connector", "connector.active"].as_slice()
            } else {
                ["connector"].as_slice()
            };
            let mut style = scene.styles.circle(selectors).unwrap_or_default();
            style.color = self.animated_color(
                ColorAnimationKey::Connector(end_node.key.clone()),
                style.color,
            );
            style.opacity = self.animated_opacity(
                OpacityAnimationKey::Connector(end_node.key.clone()),
                style.opacity,
            );
            draw_connector_segment(
                pixmap,
                &mut paint,
                start_node,
                end_node,
                &style,
                connector_factor,
            );
        }
        let temporary_links = scene.nodes.iter().filter_map(|node| {
            let parent_path = temporary_connector_parent(node)?;
            let parent = scene
                .nodes
                .iter()
                .find(|candidate| candidate.key == NodeKey::Menu(parent_path.to_vec()))?;
            Some((parent, node))
        });
        for (center, action) in temporary_links {
            let connector_factor = action.connector_factor().clamp(0.0, 1.0);
            if connector_factor <= f64::EPSILON {
                continue;
            }
            let selectors = if action.travel_connector && action.active {
                ["connector", "connector.active"].as_slice()
            } else {
                ["connector"].as_slice()
            };
            let mut style = scene.styles.circle(selectors).unwrap_or_default();
            style.color = self.animated_color(
                ColorAnimationKey::Connector(action.key.clone()),
                style.color,
            );
            style.opacity = self.animated_opacity(
                OpacityAnimationKey::Connector(action.key.clone()),
                style.opacity,
            );
            draw_connector_segment(pixmap, &mut paint, center, action, &style, connector_factor);
        }
    }

    fn draw_item(&mut self, pixmap: &mut Pixmap, frame: ItemFrame<'_>) {
        let size = frame.geometry_size;
        if size <= 0.0 {
            return;
        }
        let mut visual_style = frame.style.clone();
        visual_style.text_opacity = Some(frame.style.content_opacity() * frame.opacity);
        visual_style.opacity *= frame.opacity;
        if frame.indicators && frame.item.is_submenu() {
            let base_size = frame.style.width.unwrap_or(0.0) * frame.style.scale;
            self.draw_indicators(
                pixmap,
                frame.center,
                size,
                IndicatorFrame {
                    key: frame.key,
                    item: frame.item,
                    submenu_style: &visual_style,
                    styles: frame.styles,
                    active: frame.active,
                    return_circle: frame.indicators_return,
                    skip_index: frame.indicator_skip_index,
                    scale: if base_size > 0.0 {
                        (size / base_size).max(0.0)
                    } else {
                        0.0
                    },
                    reveal: frame.indicator_reveal,
                },
            );
        }
        draw_rounded_box(pixmap, frame.center, size, &visual_style);

        let icon_is_content = frame.item.icon.is_some()
            && matches!(
                frame.content,
                ItemContent::Default | ItemContent::IconOrLabel
            );
        let draw_icon =
            frame.item.icon.is_some() && (icon_is_content || frame.icon_opacity > f64::EPSILON);
        let mut icon_style = visual_style.clone();
        icon_style.text_opacity = Some(visual_style.content_opacity() * frame.icon_opacity);
        let icon_drawn = draw_icon
            && self.draw_icon(
                pixmap,
                frame.center,
                size,
                frame.item,
                &icon_style,
                frame.icon_root,
            );
        if let Some(label) = visible_label(frame.content, &frame.item.label, icon_drawn)
            && !label.is_empty()
        {
            let base_scale = frame
                .styles
                .circle(&["circle"])
                .map_or(1.0, |style| style.scale);
            self.draw_text(pixmap, frame.center, size, label, &visual_style, base_scale);
        }
    }

    fn draw_indicators(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        circle_size: f64,
        frame: IndicatorFrame<'_>,
    ) {
        let selectors = if frame.active {
            vec!["submenu-indicator", "submenu-indicator.active"]
        } else {
            vec!["submenu-indicator"]
        };
        let mut selectors = selectors;
        if frame.return_circle {
            selectors.push("submenu-indicator.return");
            if frame.active {
                selectors.push("submenu-indicator.return.active");
            }
        }
        let Ok(mut style) = frame.styles.circle(&selectors) else {
            return;
        };
        style.color =
            self.animated_color(ColorAnimationKey::Indicator(frame.key.clone()), style.color);
        style.opacity = self.animated_opacity(
            OpacityAnimationKey::Indicator(frame.key.clone()),
            style.opacity,
        );
        let child_angles = frame
            .item
            .items
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != frame.skip_index)
            .map(|(_, child)| child.angle.unwrap_or(0.0))
            .collect::<Vec<_>>();
        self.draw_indicator_angles(pixmap, center, circle_size, &frame, &style, &child_angles);
    }

    fn draw_indicator_angles(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        circle_size: f64,
        frame: &IndicatorFrame<'_>,
        style: &CircleStyle,
        angles: &[f64],
    ) {
        let size = style.width.unwrap_or(0.0) * frame.scale;
        if size <= 0.0 || style.protrusion <= 0.0 {
            return;
        }
        let protrusion = style.protrusion * frame.reveal;
        let radius = (circle_size / 2.0 - size / 2.0 + protrusion).max(0.0);
        let mut paint = Paint::default();
        paint.set_color(to_skia(style.color, style.opacity * frame.reveal));
        let clip = if style.cut_indicators {
            let clip_size = (circle_size - INDICATOR_CLIP_OVERLAP * 2.0).max(0.0);
            let rect = Rect::from_xywh(
                (center.x - clip_size / 2.0) as f32,
                (center.y - clip_size / 2.0) as f32,
                clip_size as f32,
                clip_size as f32,
            );
            rect.and_then(|rect| {
                let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
                let radius =
                    (frame.submenu_style.radius(circle_size) - INDICATOR_CLIP_OVERLAP).max(0.0);
                mask.fill_path(
                    &rounded_rect(rect, radius as f32),
                    FillRule::Winding,
                    true,
                    Transform::identity(),
                );
                mask.invert();
                Some(mask)
            })
        } else {
            None
        };
        for angle in angles {
            let position = radial_position(center, *angle, radius);
            if let Some(path) =
                PathBuilder::from_circle(position.x as f32, position.y as f32, (size / 2.0) as f32)
            {
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    clip.as_ref(),
                );
            }
        }
    }

    fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        size: f64,
        text: &str,
        style: &CircleStyle,
        base_scale: f64,
    ) {
        let layout_size = style.width.unwrap_or(size) * base_scale;
        if layout_size <= 0.0 {
            return;
        }
        let visual_scale = (size / layout_size).clamp(0.0, 1.0);
        if visual_scale <= 0.0 {
            return;
        }
        let available =
            ((layout_size - style.border_width * 2.0) * style.text_fill).max(1.0) as f32;
        let supersample = 2.0_f32;
        let source_extent = (available * supersample).ceil().max(2.0) as u32;
        let font_size = style.font_size as f32 * supersample;
        let line_height = (style.font_size * 1.15) as f32 * supersample;
        let family = self
            .resolved_families
            .get(&style.font_family)
            .cloned()
            .unwrap_or_else(|| style.font_family.clone());
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, line_height));
        {
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_size(Some(available * supersample), Some(available * supersample));
            buffer.set_wrap(Wrap::WordOrGlyph);
            let attrs = Attrs::new()
                .family(Family::Name(&family))
                .weight(Weight(style.font_weight));
            buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Center));
            buffer.shape_until_scroll(true);
        }
        let color = style.color;
        let alpha = (color.alpha as f64 * style.content_opacity()).clamp(0.0, 1.0);
        let text_color = TextColor::rgba(
            (color.red * 255.0).round() as u8,
            (color.green * 255.0).round() as u8,
            (color.blue * 255.0).round() as u8,
            (alpha * 255.0).round() as u8,
        );
        let content_height = buffer
            .layout_runs()
            .map(|run| run.line_top + run.line_height)
            .fold(0.0_f32, f32::max)
            .min(available * supersample);
        let origin_y = (source_extent as f32 - content_height) / 2.0;
        let Some(mut text_pixmap) = Pixmap::new(source_extent, source_extent) else {
            return;
        };
        let mut borrowed = buffer.borrow_with(&mut self.fonts);
        borrowed.draw(
            &mut self.glyphs,
            text_color,
            |x, y, width, height, color| {
                let Some(rect) =
                    Rect::from_xywh(x as f32, origin_y + y as f32, width as f32, height as f32)
                else {
                    return;
                };
                let mut paint = Paint::default();
                paint.set_color_rgba8(color.r(), color.g(), color.b(), color.a());
                text_pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            },
        );
        let destination_extent = available as f64 * visual_scale;
        let left = center.x - destination_extent / 2.0;
        let top = center.y - destination_extent / 2.0;
        let Some(rect) = Rect::from_xywh(
            left as f32,
            top as f32,
            destination_extent as f32,
            destination_extent as f32,
        ) else {
            return;
        };
        let scale = destination_extent as f32 / source_extent as f32;
        let paint = Paint {
            shader: Pattern::new(
                text_pixmap.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                1.0,
                Transform::from_row(scale, 0.0, 0.0, scale, left as f32, top as f32),
            ),
            ..Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }

    fn draw_item_key(
        &mut self,
        pixmap: &mut Pixmap,
        circle_center: Point,
        circle_size: f64,
        text: &str,
        style: &ItemKeyStyle,
        radial_factor: f64,
    ) {
        let font_size = style.font_size * style.scale;
        if font_size <= 0.0 || style.opacity <= f64::EPSILON {
            return;
        }
        let supersample = 2.0_f32;
        let font_size = font_size as f32 * supersample;
        let line_height = font_size * 1.15;
        let family = self
            .resolved_families
            .get(&style.font_family)
            .cloned()
            .unwrap_or_else(|| style.font_family.clone());
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_size, line_height));
        {
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_size(None, Some(line_height));
            buffer.set_wrap(Wrap::None);
            let attrs = Attrs::new()
                .family(Family::Name(&family))
                .weight(Weight(style.font_weight));
            buffer.set_text(text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(true);
        }
        let text_width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);
        if text_width <= 0.0 {
            return;
        }
        let padding = supersample * 2.0;
        let source_width = (text_width + padding * 2.0).ceil().max(2.0) as u32;
        let source_height = (line_height + padding * 2.0).ceil().max(2.0) as u32;
        let Some(mut text_pixmap) = Pixmap::new(source_width, source_height) else {
            return;
        };
        let alpha = (style.color.alpha as f64 * style.opacity).clamp(0.0, 1.0);
        let text_color = TextColor::rgba(
            (style.color.red * 255.0).round() as u8,
            (style.color.green * 255.0).round() as u8,
            (style.color.blue * 255.0).round() as u8,
            (alpha * 255.0).round() as u8,
        );
        let mut borrowed = buffer.borrow_with(&mut self.fonts);
        borrowed.draw(
            &mut self.glyphs,
            text_color,
            |x, y, width, height, color| {
                let Some(rect) = Rect::from_xywh(
                    padding + x as f32,
                    padding + y as f32,
                    width as f32,
                    height as f32,
                ) else {
                    return;
                };
                let mut paint = Paint::default();
                paint.set_color_rgba8(color.r(), color.g(), color.b(), color.a());
                text_pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            },
        );
        let width = f64::from(source_width) / f64::from(supersample);
        let height = f64::from(source_height) / f64::from(supersample);
        let radians = style.angle.to_radians();
        let radial_half_extent =
            radians.sin().abs() * width / 2.0 + radians.cos().abs() * height / 2.0;
        let center = radial_position(
            circle_center,
            style.angle,
            (circle_size / 2.0 + style.distance) * radial_factor + radial_half_extent,
        );
        let left = center.x - width / 2.0;
        let top = center.y - height / 2.0;
        let Some(rect) = Rect::from_xywh(left as f32, top as f32, width as f32, height as f32)
        else {
            return;
        };
        let scale = 1.0 / supersample;
        let paint = Paint {
            shader: Pattern::new(
                text_pixmap.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                1.0,
                Transform::from_row(scale, 0.0, 0.0, scale, left as f32, top as f32),
            ),
            ..Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }

    fn draw_icon(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        circle_size: f64,
        item: &Item,
        style: &CircleStyle,
        icon_root: &Path,
    ) -> bool {
        let (Some(theme), Some(icon)) = (&item.icon_theme, &item.icon) else {
            return false;
        };
        let path = icon_root.join(theme).join(icon);
        if !path.is_file() {
            return false;
        }
        let scaled_size = |circle_size: f64| {
            style.icon_fill.map_or_else(
                || {
                    style.icon_size.map_or(circle_size * 0.55, |icon_size| {
                        icon_size * circle_size / style.width.unwrap_or(circle_size)
                    })
                },
                |fill| (circle_size - style.border_width * 2.0).max(0.0) * fill,
            )
        };
        let size = scaled_size(circle_size);
        let size = size.max(1.0);
        // tiny-skia forces nearest-neighbour sampling for translation-only
        // pixmaps. Keep a 2x source so the final transform also contains a
        // scale and can use bilinear filtering at fractional positions.
        let final_circle_size = style.width.unwrap_or(circle_size) * style.scale;
        let source_size = (scaled_size(final_circle_size).ceil() * 2.0).max(2.0) as u32;
        let rgba = [
            (style.color.red * 255.0).round() as u8,
            (style.color.green * 255.0).round() as u8,
            (style.color.blue * 255.0).round() as u8,
            255,
        ];
        let key = (path.clone(), source_size, rgba);
        if !self.icons.contains_key(&key) {
            let loaded = if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            {
                load_svg(&path, source_size, rgba)
            } else {
                load_raster(&path, source_size)
            };
            let Some(icon) = loaded else {
                return false;
            };
            self.icons.insert(key.clone(), icon);
        }
        let icon = &self.icons[&key];
        let padding = size * f64::from(ICON_SOURCE_PADDING) / f64::from(source_size);
        let extent = size + padding * 2.0;
        let left = center.x - extent / 2.0;
        let top = center.y - extent / 2.0;
        let Some(rect) = Rect::from_xywh(left as f32, top as f32, extent as f32, extent as f32)
        else {
            return false;
        };
        let scale_x = extent as f32 / icon.width() as f32;
        let scale_y = extent as f32 / icon.height() as f32;
        let paint = Paint {
            shader: Pattern::new(
                icon.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                style.content_opacity() as f32,
                Transform::from_row(scale_x, 0.0, 0.0, scale_y, left as f32, top as f32),
            ),
            ..Paint::default()
        };
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
        true
    }
}

fn draw_rounded_box(pixmap: &mut Pixmap, center: Point, size: f64, style: &CircleStyle) {
    let Some(rect) = Rect::from_xywh(
        (center.x - size / 2.0) as f32,
        (center.y - size / 2.0) as f32,
        size as f32,
        size as f32,
    ) else {
        return;
    };
    let radius = style.radius(size) as f32;
    let path = if radius >= size as f32 / 2.0 - f32::EPSILON {
        PathBuilder::from_circle(center.x as f32, center.y as f32, size as f32 / 2.0)
            .expect("a positive circle size")
    } else {
        rounded_rect(rect, radius)
    };
    let mut fill = Paint::default();
    fill.set_color(to_skia(style.background_color, style.opacity));
    pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
    if style.border_width > 0.0 {
        let mut border = Paint::default();
        border.set_color(to_skia(style.border_color, style.opacity));
        pixmap.stroke_path(
            &path,
            &border,
            &Stroke {
                width: style.border_width as f32,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn draw_connector_segment(
    pixmap: &mut Pixmap,
    paint: &mut Paint<'_>,
    start_node: &VisualNode,
    end_node: &VisualNode,
    style: &CircleStyle,
    factor: f64,
) {
    let width = style.width.unwrap_or(0.0);
    if width <= 0.0 || factor <= f64::EPSILON {
        return;
    }
    let delta = Point {
        x: end_node.position.x - start_node.position.x,
        y: end_node.position.y - start_node.position.y,
    };
    let length = delta.x.hypot(delta.y);
    let start_radius = start_node.size / 2.0;
    let end_radius = end_node.size / 2.0;
    if length <= start_radius + end_radius || length <= f64::EPSILON {
        return;
    }
    let direction = Point {
        x: delta.x / length,
        y: delta.y / length,
    };
    let start = Point {
        x: start_node.position.x + direction.x * start_radius,
        y: start_node.position.y + direction.y * start_radius,
    };
    let end = Point {
        x: end_node.position.x - direction.x * end_radius,
        y: end_node.position.y - direction.y * end_radius,
    };
    paint.set_color(to_skia(
        style.color,
        style.opacity * factor.sqrt() * start_node.opacity.min(end_node.opacity),
    ));
    let mut path = PathBuilder::new();
    path.move_to(start.x as f32, start.y as f32);
    path.line_to(end.x as f32, end.y as f32);
    if let Some(path) = path.finish() {
        pixmap.stroke_path(
            &path,
            paint,
            &Stroke {
                width: (width * factor) as f32,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
}

fn rounded_rect(rect: Rect, radius: f32) -> tiny_skia::Path {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut path = PathBuilder::new();
    path.move_to(rect.left() + radius, rect.top());
    path.line_to(rect.right() - radius, rect.top());
    path.quad_to(rect.right(), rect.top(), rect.right(), rect.top() + radius);
    path.line_to(rect.right(), rect.bottom() - radius);
    path.quad_to(
        rect.right(),
        rect.bottom(),
        rect.right() - radius,
        rect.bottom(),
    );
    path.line_to(rect.left() + radius, rect.bottom());
    path.quad_to(
        rect.left(),
        rect.bottom(),
        rect.left(),
        rect.bottom() - radius,
    );
    path.line_to(rect.left(), rect.top() + radius);
    path.quad_to(rect.left(), rect.top(), rect.left() + radius, rect.top());
    path.close();
    path.finish().unwrap()
}

fn to_skia(color: Color, opacity: f64) -> SkColor {
    SkColor::from_rgba(
        color.red,
        color.green,
        color.blue,
        (color.alpha as f64 * opacity).clamp(0.0, 1.0) as f32,
    )
    .unwrap_or(SkColor::TRANSPARENT)
}

fn lerp_color(from: Color, target: Color, progress: f32) -> Color {
    Color {
        red: from.red + (target.red - from.red) * progress,
        green: from.green + (target.green - from.green) * progress,
        blue: from.blue + (target.blue - from.blue) * progress,
        alpha: from.alpha + (target.alpha - from.alpha) * progress,
    }
}

fn empty_font_system() -> FontSystem {
    FontSystem::new_with_locale_and_db(current_locale(), cosmic_text::fontdb::Database::new())
}

fn current_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .map(|locale| {
            locale
                .split(['.', '@'])
                .next()
                .unwrap_or("en-US")
                .replace('_', "-")
        })
        .unwrap_or_else(|| "en-US".into())
}

fn resolve_font(requested: &str, weight: u16) -> Option<FontSpec> {
    let weight = match weight {
        1..=150 => "thin",
        151..=250 => "extralight",
        251..=350 => "light",
        351..=450 => "regular",
        451..=550 => "medium",
        551..=650 => "demibold",
        651..=750 => "bold",
        751..=850 => "extrabold",
        _ => "black",
    };
    let pattern = format!("{requested}:weight={weight}");
    let output = Command::new("fc-match")
        .args(["-f", "%{family[0]}\n%{file}\n", &pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8(output.stdout).ok()?;
    let mut lines = output.lines();
    let family = lines.next()?.trim();
    let file = PathBuf::from(lines.next()?.trim());
    (!family.is_empty() && file.is_file()).then(|| FontSpec {
        family: family.to_string(),
        file,
    })
}

fn load_raster(path: &Path, size: u32) -> Option<Pixmap> {
    let image = ImageReader::open(path).ok()?.decode().ok()?;
    let mut data = image
        .resize(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .into_raw();
    for pixel in data.chunks_exact_mut(4) {
        let alpha = pixel[3] as u16;
        pixel[0] = (pixel[0] as u16 * alpha / 255) as u8;
        pixel[1] = (pixel[1] as u16 * alpha / 255) as u8;
        pixel[2] = (pixel[2] as u16 * alpha / 255) as u8;
    }
    let source = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(size, size)?)?;
    padded_pixmap(&source)
}

fn load_svg(path: &Path, size: u32, color: [u8; 4]) -> Option<Pixmap> {
    let mut source = fs::read_to_string(path).ok()?;
    let replacement = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
    if source.contains("currentColor") {
        source = source.replace("currentColor", &replacement);
    } else if !has_explicit_svg_paint(&source) {
        source = source.replacen("<svg", &format!("<svg fill=\"{replacement}\""), 1);
    }
    let tree =
        resvg::usvg::Tree::from_data(source.as_bytes(), &resvg::usvg::Options::default()).ok()?;
    let tree_size = tree.size();
    let scale = (size as f32 / tree_size.width()).min(size as f32 / tree_size.height());
    let extent = size.checked_add(ICON_SOURCE_PADDING * 2)?;
    let mut pixmap = Pixmap::new(extent, extent)?;
    let transform = Transform::from_scale(scale, scale).post_translate(
        ICON_SOURCE_PADDING as f32 + (size as f32 - tree_size.width() * scale) / 2.0,
        ICON_SOURCE_PADDING as f32 + (size as f32 - tree_size.height() * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap)
}

fn has_explicit_svg_paint(source: &str) -> bool {
    let compact = source
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    ["fill", "stroke"].iter().any(|attribute| {
        ["'#", "\"#", "'rgb", "\"rgb", "'hsl", "\"hsl"]
            .iter()
            .any(|value| compact.contains(&format!("{attribute}={value}")))
    })
}

fn padded_pixmap(source: &Pixmap) -> Option<Pixmap> {
    let extent = source.width().checked_add(ICON_SOURCE_PADDING * 2)?;
    let mut padded = Pixmap::new(extent, extent)?;
    padded.draw_pixmap(
        ICON_SOURCE_PADDING as i32,
        ICON_SOURCE_PADDING as i32,
        source.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    Some(padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_icon_falls_back_to_the_item_label() {
        assert_eq!(
            visible_label(ItemContent::IconOrLabel, "Submenu", false),
            Some("Submenu")
        );
        assert_eq!(
            visible_label(ItemContent::Default, "Action", false),
            Some("Action")
        );
    }

    #[test]
    fn drawn_icon_replaces_the_default_label_but_not_an_explicit_label() {
        assert_eq!(
            visible_label(ItemContent::IconOrLabel, "Submenu", true),
            None
        );
        assert_eq!(
            visible_label(ItemContent::Label("Hovered"), "Center", true),
            Some("Hovered")
        );
    }

    #[test]
    fn svg_paint_detection_supports_single_quotes_and_spacing() {
        assert!(has_explicit_svg_paint("<path fill = '#123456'/>"));
        assert!(has_explicit_svg_paint("<path stroke='rgb(1, 2, 3)'/>"));
        assert!(!has_explicit_svg_paint("<path fill='none'/>"));
    }

    #[test]
    fn connector_is_active_only_for_a_focused_history_circle() {
        assert!(connector_is_active(NodeRole::History, true));
        assert!(!connector_is_active(NodeRole::History, false));
        assert!(!connector_is_active(NodeRole::Center, true));
        assert!(!connector_is_active(NodeRole::Item, true));
    }

    #[test]
    fn negative_distance_keeps_a_disappearing_item_key_attached_to_its_circle() {
        assert_eq!(item_key_radial_factor(-1.0, 0.25), 1.0);
        assert_eq!(item_key_radial_factor(0.0, 0.25), 0.25);
        assert_eq!(item_key_radial_factor(10.0, 0.25), 0.25);
    }

    #[test]
    fn item_key_is_under_circles_only_while_appearing_or_disappearing() {
        assert!(!item_key_is_under_circle(false, 1.0, 1.0, 1.0, false));
        assert!(item_key_is_under_circle(false, 0.5, 1.0, 1.0, false));
        assert!(item_key_is_under_circle(false, 1.0, 0.5, 1.0, false));
        assert!(item_key_is_under_circle(true, 1.0, 1.0, 1.0, false));
        assert!(!item_key_is_under_circle(true, 0.5, 1.0, 1.0, true));
    }

    #[test]
    fn color_animation_interpolates_rgba_channels() {
        let from = Color {
            red: 0.0,
            green: 0.2,
            blue: 0.4,
            alpha: 0.6,
        };
        let target = Color {
            red: 1.0,
            green: 0.8,
            blue: 0.6,
            alpha: 0.4,
        };
        let color = lerp_color(from, target, 0.5);
        assert!((color.red - 0.5).abs() < 0.001);
        assert!((color.green - 0.5).abs() < 0.001);
        assert!((color.blue - 0.5).abs() < 0.001);
        assert!((color.alpha - 0.5).abs() < 0.001);
    }

    #[test]
    fn interrupted_color_change_continues_from_the_visible_color() {
        let mut renderer = Renderer::new();
        renderer.color_duration = Duration::from_secs(1);
        let key = ColorAnimationKey::Overlay;
        let red = Color {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        };
        let green = Color {
            red: 0.0,
            green: 1.0,
            blue: 0.0,
            alpha: 1.0,
        };
        let blue = Color {
            red: 0.0,
            green: 0.0,
            blue: 1.0,
            alpha: 1.0,
        };
        renderer.animated_color(key.clone(), red);
        renderer.animated_color(key.clone(), green);
        renderer.color_animations.get_mut(&key).unwrap().started =
            Instant::now() - Duration::from_millis(500);
        let visible = renderer.animated_color(key.clone(), green);
        let retargeted = renderer.animated_color(key, blue);
        assert!((retargeted.red - visible.red).abs() < 0.001);
        assert!((retargeted.green - visible.green).abs() < 0.001);
        assert!((retargeted.blue - visible.blue).abs() < 0.001);
    }

    #[test]
    fn interrupted_opacity_change_continues_from_the_visible_value() {
        let mut renderer = Renderer::new();
        renderer.opacity_duration = Duration::from_secs(1);
        let key = OpacityAnimationKey::Overlay;
        renderer.animated_opacity(key.clone(), 1.0);
        renderer.animated_opacity(key.clone(), 0.0);
        renderer.opacity_animations.get_mut(&key).unwrap().started =
            Instant::now() - Duration::from_millis(500);
        let visible = renderer.animated_opacity(key.clone(), 0.0);
        let retargeted = renderer.animated_opacity(key, 0.8);
        assert!((retargeted - visible).abs() < 0.001);
    }

    #[test]
    fn item_key_geometry_uses_the_shortest_angle_and_survives_retargeting() {
        let mut renderer = Renderer::new();
        let key = NodeKey::Action(vec![], 0);
        let geometry = |angle, distance, font_size, font_weight| ItemKeyGeometry {
            angle,
            distance,
            font_size,
            font_weight,
        };
        renderer.animated_item_key_geometry(key.clone(), geometry(350.0, 0.0, 10.0, 400.0));
        renderer.animated_item_key_geometry(key.clone(), geometry(10.0, 20.0, 20.0, 700.0));
        let animation = renderer.item_key_animations.get_mut(&key).unwrap();
        animation.started = Instant::now() - animation.spring.duration() / 2;
        let visible =
            renderer.animated_item_key_geometry(key.clone(), geometry(10.0, 20.0, 20.0, 700.0));

        assert!(visible.angle > 340.0 || visible.angle < 20.0);
        let retargeted = renderer.animated_item_key_geometry(key, geometry(90.0, -5.0, 8.0, 300.0));
        assert!((retargeted.angle - visible.angle).abs() < 0.01);
        assert!((retargeted.distance - visible.distance).abs() < 0.01);
        assert!((retargeted.font_size - visible.font_size).abs() < 0.01);
        assert!((retargeted.font_weight - visible.font_weight).abs() < 0.01);
    }

    #[test]
    fn animation_off_makes_item_key_geometry_and_font_weight_immediate() {
        let mut renderer = Renderer::new();
        renderer.item_key_animations_enabled = false;
        let geometry = ItemKeyGeometry {
            angle: 45.0,
            distance: -10.0,
            font_size: 18.0,
            font_weight: 800.0,
        };

        assert_eq!(
            renderer.animated_item_key_geometry(NodeKey::Action(vec![], 0), geometry),
            geometry
        );
        assert_eq!(
            renderer.animated_font_weight(NodeKey::Menu(vec![]), 800),
            800
        );
        assert_eq!(
            renderer.animated_item_key_presence(NodeKey::Menu(vec![0]), 0.0, 0.0, false),
            0.0
        );
    }

    #[test]
    fn item_key_presence_continues_smoothly_when_submenu_direction_reverses() {
        let mut renderer = Renderer::new();
        renderer.item_key_delete_duration = Duration::from_secs(1);
        let key = NodeKey::Menu(vec![0]);
        assert_eq!(
            renderer.animated_item_key_presence(key.clone(), 1.0, 1.0, false),
            1.0
        );
        assert_eq!(
            renderer.animated_item_key_presence(key.clone(), 0.0, 0.0, false),
            1.0
        );
        renderer.item_key_presence.get_mut(&key).unwrap().started =
            Instant::now() - Duration::from_millis(500);
        let visible = renderer.animated_item_key_presence(key.clone(), 0.0, 0.0, false);
        let reversed = renderer.animated_item_key_presence(key, 1.0, 1.0, false);

        assert!(visible > 0.0 && visible < 1.0);
        assert!((visible - reversed).abs() < 0.001);
    }

    #[test]
    fn anchored_item_key_presence_uses_opacity_duration_for_both_directions() {
        let mut renderer = Renderer::new();
        renderer.opacity_duration = Duration::from_millis(750);
        let key = NodeKey::Menu(vec![0]);
        renderer.animated_item_key_presence(key.clone(), 1.0, 1.0, true);
        renderer.animated_item_key_presence(key.clone(), 0.0, 0.0, true);
        let disappearing = renderer.item_key_presence[&key];
        assert_eq!(disappearing.duration, renderer.opacity_duration);
        assert!(disappearing.spring.is_none());

        renderer.item_key_presence.get_mut(&key).unwrap().started =
            Instant::now() - Duration::from_millis(300);
        renderer.animated_item_key_presence(key.clone(), 0.0, 0.0, true);
        renderer.animated_item_key_presence(key.clone(), 1.0, 1.0, true);
        let appearing = renderer.item_key_presence[&key];
        assert_eq!(appearing.duration, renderer.opacity_duration);
        assert!(appearing.spring.is_none());
    }

    #[test]
    fn zero_opacity_duration_makes_anchored_item_key_lifecycle_immediate() {
        let mut renderer = Renderer::new();
        renderer.opacity_duration = Duration::ZERO;
        let key = NodeKey::Menu(vec![0]);

        let appeared = renderer.animated_item_key_presence(key.clone(), 1.0, 0.0, true);
        assert_eq!(appeared, 1.0);
        assert!(!item_key_is_under_circle(false, 0.0, appeared, 1.0, true));

        let disappeared = renderer.animated_item_key_presence(key, 0.0, 0.0, true);
        assert_eq!(disappeared, 0.0);
    }
}

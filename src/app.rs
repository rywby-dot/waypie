use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};
use smithay_client_toolkit::{
    activation::{ActivationHandler, ActivationState, RequestData},
    compositor::{CompositorHandler, CompositorState},
    delegate_activation, delegate_compositor, delegate_keyboard, delegate_layer, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{
            CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Slot, SlotPool},
    },
};
use tiny_skia::Pixmap;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
};

use crate::{
    animation::Spring,
    appearance::{node_size, node_style},
    config::{Config, item_at_path},
    geometry::{Point, angular_distance, direction_angle},
    hover::HoverDetector,
    model::{MenuState, Target},
    render::{Renderer, Scene},
    style::{AnimationStyle, StyleSheet},
    visual::{Animator, Motion, NodeKey, NodeRole, NodeTarget, TransitionEffects},
};

const LEFT_BUTTON: u32 = 0x110;
const RIGHT_BUTTON: u32 = 0x111;

#[derive(Clone, Copy)]
struct AnimationProfile {
    menu_move: Motion,
    item_create: Motion,
    hover: Motion,
    follow: Motion,
    effects: TransitionEffects,
    close_duration: Duration,
    action: Motion,
    action_scale: f64,
}

impl Default for AnimationProfile {
    fn default() -> Self {
        Self {
            menu_move: Motion::default(),
            item_create: Motion::default(),
            hover: Motion::default(),
            follow: Motion::default(),
            effects: TransitionEffects::default(),
            close_duration: Duration::ZERO,
            action: Motion::default(),
            action_scale: 1.3,
        }
    }
}

impl From<AnimationStyle> for AnimationProfile {
    fn from(animation: AnimationStyle) -> Self {
        let motion = |spring: Spring| Motion {
            duration: spring.duration(),
            spring,
        };
        Self {
            menu_move: motion(animation.menu_move_spring),
            item_create: motion(animation.item_create_spring),
            hover: motion(animation.hover_spring),
            follow: motion(animation.follow_spring),
            effects: TransitionEffects {
                deletion_duration: animation.item_delete_duration,
                icon_duration: animation.icon_duration,
                connector_duration: animation.connector_duration,
                indicator: motion(animation.submenu_indicator_spring),
            },
            close_duration: animation.close_duration,
            action: motion(animation.action_spring),
            action_scale: animation.action_scale,
        }
    }
}

#[cfg(test)]
mod animation_profile_tests {
    use super::*;

    #[test]
    fn style_animation_is_mapped_once_without_losing_effect_timings() {
        let styles = StyleSheet::parse(
            "animation { icon-duration: 120ms; connector-duration: 140ms; \
             item-delete-duration: 160ms; close-duration: 180ms; action-scale: 1.4; }",
        );
        let profile = AnimationProfile::from(styles.animation().unwrap());

        assert_eq!(profile.effects.icon_duration, Duration::from_millis(120));
        assert_eq!(
            profile.effects.connector_duration,
            Duration::from_millis(140)
        );
        assert_eq!(
            profile.effects.deletion_duration,
            Duration::from_millis(160)
        );
        assert_eq!(profile.close_duration, Duration::from_millis(180));
        assert_eq!(profile.action_scale, 1.4);
    }
}

struct OutputLayer {
    output: wl_output::WlOutput,
    surface: LayerSurface,
    width: u32,
    height: u32,
    viewport: Option<WpViewport>,
}

impl Drop for OutputLayer {
    fn drop(&mut self) {
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
    }
}

struct RenderBuffers {
    pool: SlotPool,
    slots: [Slot; 2],
    width: u32,
    height: u32,
    next: usize,
}

impl RenderBuffers {
    fn new(width: u32, height: u32, shm: &Shm) -> Option<Self> {
        let frame_size = width as usize * height as usize * 4;
        let mut pool = SlotPool::new(frame_size * 2, shm).ok()?;
        let first = pool.new_slot(frame_size).ok()?;
        let second = pool.new_slot(frame_size).ok()?;
        Some(Self {
            pool,
            slots: [first, second],
            width,
            height,
            next: 0,
        })
    }
}

pub struct App {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub shm: Shm,
    pub activation: Option<ActivationState>,
    pub viewporter: Option<WpViewporter>,
    pub qh: QueueHandle<Self>,
    pub exit: bool,
    pub reopen_requested: bool,

    layers: Vec<OutputLayer>,
    active_layer: Option<usize>,
    visible: bool,
    buffers: Option<RenderBuffers>,
    redraw_pending: bool,
    renderer: Renderer,
    config: Option<Config>,
    styles: Option<StyleSheet>,
    animation: AnimationProfile,
    pointer: Option<ThemedPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    state: MenuState,
    config_dir: PathBuf,
    pending_activation: Option<String>,
    hover_detector: HoverDetector,
    pointer_position: Option<Point>,
    turbo_active: bool,
    animator: Animator,
    closing_until: Option<Instant>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry_state: RegistryState,
        seat_state: SeatState,
        output_state: OutputState,
        compositor: CompositorState,
        layer_shell: LayerShell,
        shm: Shm,
        activation: Option<ActivationState>,
        viewporter: Option<WpViewporter>,
        qh: QueueHandle<Self>,
    ) -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("waypie");
        Self {
            registry_state,
            seat_state,
            output_state,
            compositor,
            layer_shell,
            shm,
            activation,
            viewporter,
            qh,
            exit: false,
            reopen_requested: false,
            layers: vec![],
            active_layer: None,
            visible: false,
            buffers: None,
            redraw_pending: false,
            renderer: Renderer::new(),
            config: None,
            styles: None,
            animation: AnimationProfile::default(),
            pointer: None,
            keyboard: None,
            state: MenuState::default(),
            config_dir,
            pending_activation: None,
            hover_detector: HoverDetector::default(),
            pointer_position: None,
            turbo_active: false,
            animator: Animator::default(),
            closing_until: None,
        }
    }

    pub fn handle_control(&mut self, command: &[u8]) {
        match command {
            b"show" if self.closing_until.is_some() => {
                self.reopen_requested = true;
                self.closing_until = Some(Instant::now());
            }
            b"show" | b"quit" => self.hide(),
            _ => {}
        }
    }

    pub fn prepare_layers(&mut self) {
        let outputs = self.output_state.outputs().collect::<Vec<_>>();
        for output in outputs {
            if self.layers.iter().any(|layer| layer.output == output) {
                continue;
            }
            let wl_surface = self.compositor.create_surface(&self.qh);
            let surface = self.layer_shell.create_layer_surface(
                &self.qh,
                wl_surface,
                Layer::Overlay,
                Some("waypie"),
                Some(&output),
            );
            surface.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT);
            surface.set_size(0, 0);
            surface.set_exclusive_zone(-1);
            surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            self.set_click_through(&surface, true);
            surface.commit();
            let viewport = self
                .viewporter
                .as_ref()
                .map(|viewporter| viewporter.get_viewport(surface.wl_surface(), &self.qh, ()));
            self.layers.push(OutputLayer {
                output,
                surface,
                width: 0,
                height: 0,
                viewport,
            });
        }
    }

    pub fn show(&mut self, activation_token: Option<String>) -> Result<()> {
        if self.visible {
            return Ok(());
        }
        let config = Config::load(&self.config_dir.join("config"))?;
        let styles = StyleSheet::load(&self.config_dir.join("style.css"))?;
        let animation_style = styles.animation()?;
        let animation = AnimationProfile::from(animation_style);
        self.renderer.configure_fonts(styles.font_families());
        self.renderer.configure_animations(animation_style);
        self.prepare_layers();
        if self.layers.is_empty() {
            bail!("no Wayland outputs are available");
        }
        self.config = Some(config);
        self.styles = Some(styles);
        self.animation = animation;
        self.state.reset();
        self.hover_detector.reset(None);
        self.pending_activation = activation_token;
        self.pointer_position = None;
        self.turbo_active = false;
        self.animator.clear();
        self.closing_until = None;
        self.active_layer = None;
        self.visible = true;
        for index in 0..self.layers.len() {
            let surface = &self.layers[index].surface;
            surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            surface.set_input_region(None);
            surface.commit();
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        self.begin_hide(None);
    }

    fn animation_profile(&self) -> AnimationProfile {
        self.animation
    }

    fn begin_hide(&mut self, action: Option<(NodeKey, Point)>) {
        if self.exit || self.closing_until.is_some() {
            return;
        }
        self.visible = false;
        for index in 0..self.layers.len() {
            let surface = &self.layers[index].surface;
            surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            self.set_click_through(surface, true);
            surface.commit();
        }
        let animation = self.animation_profile();
        let mut total_duration = if action.is_some() {
            animation
                .close_duration
                .max(animation.action.duration)
                .max(animation.effects.connector_duration)
                .max(animation.effects.indicator.duration)
        } else {
            animation
                .close_duration
                .max(animation.effects.connector_duration)
                .max(animation.effects.indicator.duration)
        };
        self.animator.close_with_effects(
            action
                .as_ref()
                .map(|(key, point)| (key, *point, animation.action_scale)),
            animation.close_duration,
            animation.action.duration,
            animation.effects.connector_duration,
            animation.effects.indicator,
            animation.action.spring,
        );
        total_duration = total_duration.max(self.animator.remaining_duration());
        total_duration = total_duration.max(self.renderer.remaining_duration());
        if total_duration.is_zero() {
            self.finish_hide();
            return;
        }
        self.closing_until = Some(Instant::now() + total_duration);
        self.draw();
    }

    fn finish_hide(&mut self) {
        self.redraw_pending = false;
        self.layers.clear();
        self.exit = true;
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn hover_enabled(&self) -> bool {
        self.config
            .as_ref()
            .is_some_and(|config| config.hover_mode || self.turbo_active)
    }

    pub fn needs_tick(&self) -> bool {
        self.hover_enabled()
            || self.closing_until.is_some()
            || self.animator.is_animating()
            || self.renderer.is_animating()
    }

    fn set_click_through(&self, surface: &LayerSurface, enabled: bool) {
        if enabled {
            let region = self.compositor.wl_compositor().create_region(&self.qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
        } else {
            surface.set_input_region(None);
        }
    }

    fn select_output(&mut self, index: usize, pointer: Point) {
        if self.active_layer.is_some() || index >= self.layers.len() {
            return;
        }
        for candidate in 0..self.layers.len() {
            let surface = &self.layers[candidate].surface;
            if candidate == index {
                surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                self.set_click_through(surface, false);
            } else {
                surface.set_keyboard_interactivity(KeyboardInteractivity::None);
                self.set_click_through(surface, true);
            }
            surface.commit();
        }
        self.active_layer = Some(index);
        self.buffers = None;
        if let (Some(activation), Some(token)) =
            (self.activation.as_ref(), self.pending_activation.take())
        {
            activation.activate::<Self>(self.layers[index].surface.wl_surface(), token);
        }
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let center = if config.center_mode {
            Point {
                x: self.layers[index].width as f64 / 2.0,
                y: self.layers[index].height as f64 / 2.0,
            }
        } else {
            pointer
        };
        self.state.place_root(
            center,
            config,
            self.layers[index].width,
            self.layers[index].height,
        );
        let hitbox = self.center_hitbox();
        self.state.update_pointer(pointer, config, hitbox);
        self.hover_detector.reset(Some(center));
        self.sync_visual(false);
        self.draw();
    }

    fn center_hitbox(&self) -> f64 {
        let Some(config) = self.config.as_ref() else {
            return 0.0;
        };
        config.center_hitbox_size.unwrap_or_else(|| {
            self.styles
                .as_ref()
                .and_then(|styles| styles.circle(&["circle", "circle.center"]).ok())
                .and_then(|style| style.width.map(|width| width * style.scale))
                .unwrap_or(0.0)
        })
    }

    fn visual_targets(&self) -> Vec<NodeTarget> {
        let (Some(config), Some(styles)) = (self.config.as_ref(), self.styles.as_ref()) else {
            return vec![];
        };
        let path = self.state.path();
        let travel_enabled =
            config.travel_item_animation && (config.hover_mode || self.turbo_active);
        let centers = self.state.centers();
        let mut targets = vec![];
        for (depth, center) in centers.iter().copied().enumerate() {
            let item_path = path[..depth].to_vec();
            let role = if depth + 1 == centers.len() {
                NodeRole::Center
            } else {
                NodeRole::History
            };
            let active = match role {
                NodeRole::Center => self.state.active() == Some(Target::Center),
                NodeRole::History => self.state.active() == Some(Target::Parent(depth)),
                NodeRole::Item => false,
            };
            let item = item_at_path(&config.menu, &item_path);
            let rest_style = node_style(styles, item, role, false);
            let style = node_style(styles, item, role, active);
            targets.push(NodeTarget {
                key: NodeKey::Menu(item_path.clone()),
                item_path,
                role,
                position: center,
                rest_position: center,
                origin: center,
                size: node_size(&style),
                rest_size: node_size(&rest_style),
                active,
                traveling: false,
                icon_visible: role != NodeRole::Center
                    || !matches!(self.state.active(), Some(Target::Item(_))),
            });
        }
        let Some(center) = centers.last().copied() else {
            return targets;
        };
        let current = self.state.current(config);
        let pointer_angle = self
            .state
            .pointer()
            .filter(|_| matches!(self.state.active(), Some(Target::Item(_))))
            .map(|pointer| {
                direction_angle(Point {
                    x: pointer.x - center.x,
                    y: pointer.y - center.y,
                })
            });
        for (index, item) in current.items.iter().enumerate() {
            let active = self.state.active() == Some(Target::Item(index));
            let traveling = travel_enabled && active;
            let rest_style = node_style(styles, item, NodeRole::Item, false);
            let style = node_style(styles, item, NodeRole::Item, active);
            let extra_distance = if active {
                style.distance.unwrap_or(0.0)
            } else if let Some(pointer_angle) = pointer_angle {
                let active_style = node_style(styles, item, NodeRole::Item, true);
                let angle_difference = angular_distance(pointer_angle, item.angle.unwrap_or(0.0));
                let angular_factor = (1.0 + angle_difference.to_radians().cos()) / 2.0;
                active_style.distance.unwrap_or(0.0) * active_style.follow_distance * angular_factor
            } else {
                0.0
            };
            let distance = (config.menu_radius + extra_distance).max(0.0);
            let position = if traveling {
                self.state.pointer().unwrap_or_else(|| {
                    crate::geometry::radial_position(center, item.angle.unwrap_or(0.0), distance)
                })
            } else {
                crate::geometry::radial_position(center, item.angle.unwrap_or(0.0), distance)
            };
            let rest_position = crate::geometry::radial_position(
                center,
                item.angle.unwrap_or(0.0),
                config.menu_radius,
            );
            let mut item_path = path.to_vec();
            item_path.push(index);
            let key = if item.is_submenu() {
                NodeKey::Menu(item_path.clone())
            } else {
                NodeKey::Action(path.to_vec(), index)
            };
            targets.push(NodeTarget {
                key,
                item_path,
                role: NodeRole::Item,
                position,
                rest_position,
                origin: center,
                size: node_size(&style),
                rest_size: node_size(&rest_style),
                active,
                traveling,
                icon_visible: true,
            });
        }
        targets
    }

    fn sync_visual(&mut self, animate: bool) {
        let targets = self.visual_targets();
        let animation = self.animation_profile();
        if animate {
            self.animator.reconcile_with_effects(
                targets,
                animation.menu_move,
                animation.item_create,
                animation.effects,
                true,
            );
        } else if self.animator.is_empty() {
            self.animator.reconcile_with_effects(
                targets,
                Motion::default(),
                Motion::default(),
                TransitionEffects::default(),
                false,
            );
        } else {
            self.animator.hover_with_follow(
                targets,
                animation.hover,
                animation.follow,
                animation.effects.icon_duration,
                animation.effects.connector_duration,
            );
        }
    }

    fn update_pointer(&mut self, position: Point) {
        self.pointer_position = Some(position);
        let (Some(config), Some(index)) = (self.config.as_ref(), self.active_layer) else {
            return;
        };
        if self.state.centers().is_empty() {
            let center = if config.center_mode {
                Point {
                    x: self.layers[index].width as f64 / 2.0,
                    y: self.layers[index].height as f64 / 2.0,
                }
            } else {
                position
            };
            self.state.place_root(
                center,
                config,
                self.layers[index].width,
                self.layers[index].height,
            );
        }
        let changed = self
            .state
            .update_pointer(position, config, self.center_hitbox());
        let selection = (config.hover_mode || self.turbo_active)
            .then(|| self.hover_detector.on_motion(position, Instant::now()))
            .flatten();
        let following = matches!(self.state.active(), Some(Target::Item(_)));
        if changed || following {
            self.sync_visual(false);
            self.draw();
        }
        if let Some(selection) = selection {
            self.activate_at(selection, true, self.turbo_active);
        }
    }

    fn activate(&mut self, position: Point) {
        self.activate_at(position, false, false);
    }

    fn complete_navigation(&mut self) {
        self.hover_detector
            .reset(self.state.centers().last().copied());
        self.sync_visual(true);
        self.draw();
    }

    fn return_to(&mut self, depth: usize, position: Point, layer_index: usize) {
        let (Some(config), Some(layer)) = (self.config.as_ref(), self.layers.get(layer_index))
        else {
            return;
        };
        self.state
            .return_to(depth, position, config, layer.width, layer.height);
        self.complete_navigation();
    }

    fn open_submenu(&mut self, item_index: usize, position: Point, layer_index: usize) {
        let (Some(config), Some(layer)) = (self.config.as_ref(), self.layers.get(layer_index))
        else {
            return;
        };
        self.state
            .open_submenu(item_index, position, config, layer.width, layer.height);
        self.complete_navigation();
    }

    fn activate_at(&mut self, position: Point, hover: bool, submenu_only: bool) {
        let Some(index) = self.active_layer else {
            return;
        };
        let Some(config) = self.config.as_ref() else {
            return;
        };
        self.state
            .update_pointer(position, config, self.center_hitbox());
        let Some(target) = self.state.active() else {
            return;
        };
        if hover && target == Target::Center {
            return;
        }
        match target {
            Target::Center => {
                if !self.state.path().is_empty() && !config.close_submenu_on_center_click {
                    let depth = self.state.path().len() - 1;
                    self.return_to(depth, position, index);
                } else {
                    self.hide();
                }
            }
            Target::Parent(depth) => {
                self.return_to(depth, position, index);
            }
            Target::Item(item_index) => {
                let item = self.state.current(config).items[item_index].clone();
                if item.is_submenu() {
                    self.open_submenu(item_index, position, index);
                } else if !submenu_only && let Some(command) = item.command {
                    launch(&command);
                    let key = NodeKey::Action(self.state.path().to_vec(), item_index);
                    self.begin_hide(Some((key, position)));
                }
            }
        }
    }

    pub fn tick_hover(&mut self) {
        if self.hover_enabled()
            && let Some(position) = self.hover_detector.on_timeout(Instant::now())
        {
            self.activate_at(position, true, self.turbo_active);
        }
        if self.animator.tick() || self.renderer.is_animating() {
            self.draw();
        }
        if self
            .closing_until
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.finish_hide();
        }
    }

    fn draw(&mut self) {
        self.redraw_pending = true;
        let (Some(index), Some(config), Some(styles)) = (
            self.active_layer,
            self.config.as_ref(),
            self.styles.as_ref(),
        ) else {
            return;
        };
        let width = self.layers[index].width;
        let height = self.layers[index].height;
        if width == 0 || height == 0 || self.state.centers().is_empty() {
            return;
        }
        let size_changed = self
            .buffers
            .as_ref()
            .is_none_or(|buffers| buffers.width != width || buffers.height != height);
        if size_changed {
            self.buffers = RenderBuffers::new(width, height, &self.shm);
        }
        let Some(buffers) = self.buffers.as_mut() else {
            return;
        };
        let slot_index = (0..buffers.slots.len())
            .map(|offset| (buffers.next + offset) % buffers.slots.len())
            .find(|index| buffers.pool.canvas(&buffers.slots[*index]).is_some());
        let Some(slot_index) = slot_index else {
            return;
        };
        let slot = buffers.slots[slot_index].clone();
        let Ok(buffer) = buffers.pool.create_buffer_in(
            &slot,
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };
        let Some(canvas) = buffers.pool.canvas(&slot) else {
            return;
        };
        let Some(mut pixmap) = Pixmap::new(width, height) else {
            return;
        };
        let nodes = self.animator.nodes();
        self.renderer.render(
            &mut pixmap,
            &Scene {
                config,
                styles,
                state: &self.state,
                nodes: &nodes,
                icon_root: &self.config_dir.join("icons"),
            },
        );
        copy_pixmap_to_argb(&pixmap, canvas);
        let surface = &self.layers[index].surface;
        if let Some(viewport) = self.layers[index].viewport.as_ref() {
            viewport.set_destination(width as i32, height as i32);
        }
        surface
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
        if buffer.attach_to(surface.wl_surface()).is_ok() {
            surface.commit();
            buffers.next = (slot_index + 1) % buffers.slots.len();
            self.redraw_pending = false;
        }
    }

    pub fn flush_redraw(&mut self) {
        if self.redraw_pending {
            self.draw();
        }
    }

    fn attach_transparent(&mut self, index: usize) {
        let Some(layer) = self.layers.get(index) else {
            return;
        };
        let (width, height) = (layer.width, layer.height);
        if width == 0 || height == 0 {
            return;
        }
        let use_viewport = layer.viewport.is_some();
        let buffer_width = if use_viewport { 1 } else { width };
        let buffer_height = if use_viewport { 1 } else { height };
        let needed = buffer_width as usize * buffer_height as usize * 4;
        let Ok(mut pool) = SlotPool::new(needed, &self.shm) else {
            return;
        };
        let Ok((buffer, canvas)) = pool.create_buffer(
            buffer_width as i32,
            buffer_height as i32,
            buffer_width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };
        canvas.fill(0);
        let surface = &self.layers[index].surface;
        if let Some(viewport) = self.layers[index].viewport.as_ref() {
            viewport.set_destination(width as i32, height as i32);
        }
        surface
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
        if buffer.attach_to(surface.wl_surface()).is_ok() {
            surface.commit();
        }
    }
}

fn copy_pixmap_to_argb(pixmap: &Pixmap, canvas: &mut [u8]) {
    for (source, target) in pixmap
        .data()
        .chunks_exact(4)
        .zip(canvas.chunks_exact_mut(4))
    {
        target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
    }
}

fn launch(command: &str) {
    let _ = Command::new("sh").arg("-c").arg(command).spawn();
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &LayerSurface) {
        if let Some(index) = self
            .layers
            .iter()
            .position(|layer| &layer.surface == surface)
        {
            let selected = self.active_layer == Some(index);
            self.layers.remove(index);
            self.active_layer = self.active_layer.and_then(|active| {
                if active == index {
                    None
                } else {
                    Some(active - usize::from(active > index))
                }
            });
            if selected {
                self.hide();
            }
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(index) = self
            .layers
            .iter()
            .position(|layer| &layer.surface == surface)
        else {
            return;
        };
        self.layers[index].width = configure.new_size.0;
        self.layers[index].height = configure.new_size.1;
        if self.active_layer == Some(index) {
            self.draw();
        } else {
            self.attach_transparent(index);
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            let cursor_surface = self.compositor.create_surface(qh);
            self.pointer = self
                .seat_state
                .get_pointer_with_theme(
                    qh,
                    &seat,
                    self.shm.wl_shm(),
                    cursor_surface,
                    ThemeSpec::default(),
                )
                .ok();
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.pointer.take();
        }
        if capability == Capability::Keyboard {
            self.keyboard.take();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if !self.visible {
                continue;
            }
            let Some(index) = self
                .layers
                .iter()
                .position(|layer| &event.surface == layer.surface.wl_surface())
            else {
                continue;
            };
            let position = Point {
                x: event.position.0,
                y: event.position.1,
            };
            if matches!(event.kind, PointerEventKind::Enter { .. }) {
                if let Some(pointer) = self.pointer.as_ref() {
                    let _ = pointer.set_cursor(conn, CursorIcon::Default);
                }
                self.select_output(index, position);
            }
            if self.active_layer != Some(index) {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.update_pointer(position)
                }
                PointerEventKind::Press { button, .. } if button == LEFT_BUTTON => {
                    self.activate(position)
                }
                PointerEventKind::Press { button, .. } if button == RIGHT_BUTTON => self.hide(),
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            self.hide();
        }
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        let was_active = self.turbo_active;
        let held = modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.logo;
        self.turbo_active = self
            .config
            .as_ref()
            .is_some_and(|config| config.turbo_mode && held);
        if !was_active && self.turbo_active {
            self.hover_detector
                .reset(self.state.centers().last().copied());
        } else if was_active && !self.turbo_active {
            self.hover_detector.reset(None);
            if let Some(position) = self.pointer_position {
                self.activate_at(position, true, false);
            }
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.prepare_layers();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.prepare_layers();
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(index) = self.layers.iter().position(|layer| layer.output == output) {
            let selected = self.active_layer == Some(index);
            self.layers.remove(index);
            if selected {
                self.active_layer = None;
                self.hide();
            } else if let Some(active) = self.active_layer
                && active > index
            {
                self.active_layer = Some(active - 1);
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Dispatch<WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewporter has no events")
    }
}

impl Dispatch<WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewport has no events")
    }
}

impl ActivationHandler for App {
    type RequestData = RequestData;

    fn new_token(&mut self, _: String, _: &Self::RequestData) {}
}

delegate_activation!(App);
delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);
wayland_client::delegate_noop!(App: ignore wl_region::WlRegion);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

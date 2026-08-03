use std::{
    collections::HashMap,
    env, fs,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
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
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use tiny_skia::Pixmap;
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
};

use crate::{
    animation::{Spring, smoothstep},
    config::{Config, Item, item_at_path},
    geometry::{Point, angular_distance, clamp_center, direction_angle, radial_position},
    hover::HoverDetector,
    render::{ActionFrame, Renderer, Scene, Target},
    style::StyleSheet,
};

const LEFT_BUTTON: u32 = 0x110;
const RIGHT_BUTTON: u32 = 0x111;

struct NavigationState {
    started: Instant,
    duration: Duration,
    from: Vec<Point>,
}

struct CloseState {
    started: Instant,
    duration: Duration,
    action: Option<(usize, Point, Point)>,
    spring: Spring,
    action_scale: f64,
}

struct OutputLayer {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
}

pub struct App {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub shm: Shm,
    pub qh: QueueHandle<Self>,
    pub exit: bool,

    layers: Vec<OutputLayer>,
    active_layer: Option<usize>,
    active: bool,
    pool: Option<SlotPool>,
    renderer: Option<Renderer>,
    config: Option<Config>,
    styles: Option<StyleSheet>,
    width: u32,
    height: u32,
    configured: bool,
    pointer: Option<ThemedPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer_position: Option<Point>,
    modifiers: Modifiers,
    turbo_active: bool,
    hover_detector: HoverDetector,
    path: Vec<usize>,
    centers: Vec<Point>,
    display_centers: Vec<Point>,
    link_lengths: Vec<f64>,
    hovered: Option<Target>,
    hover_origins: HashMap<Target, f64>,
    hover_started: Option<Instant>,
    config_dir: PathBuf,
    navigation: Option<NavigationState>,
    item_reveal: f64,
    closing: Option<CloseState>,
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
            qh,
            exit: false,
            layers: vec![],
            active_layer: None,
            active: false,
            pool: None,
            renderer: Some(Renderer::new()),
            config: None,
            styles: None,
            width: 0,
            height: 0,
            configured: false,
            pointer: None,
            keyboard: None,
            pointer_position: None,
            modifiers: Modifiers::default(),
            turbo_active: false,
            hover_detector: HoverDetector::default(),
            path: vec![],
            centers: vec![],
            display_centers: vec![],
            link_lengths: vec![],
            hovered: None,
            hover_origins: HashMap::new(),
            hover_started: None,
            config_dir,
            navigation: None,
            item_reveal: 1.0,
            closing: None,
        }
    }

    pub fn handle_control(&mut self, command: &[u8]) {
        match command {
            b"show" => {
                if self.active {
                    self.begin_hide(None);
                } else if let Err(error) = self.show() {
                    eprintln!("waypie: {error:#}");
                }
            }
            b"quit" => {
                self.finish_hide();
                self.layers.clear();
                self.exit = true;
            }
            _ => {}
        }
    }

    pub fn prepare_layers(&mut self) {
        let outputs = self.output_state.outputs().collect::<Vec<_>>();
        for output in outputs {
            if self.layers.iter().any(|layer| layer.output == output) {
                continue;
            }
            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("waypie"),
                Some(&output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            let region = self.compositor.wl_compositor().create_region(&self.qh, ());
            layer.set_input_region(Some(&region));
            region.destroy();
            layer.commit();
            self.layers.push(OutputLayer {
                output,
                layer,
                width: 0,
                height: 0,
                configured: false,
            });
        }
    }

    pub fn show(&mut self) -> Result<()> {
        if self.active {
            return Ok(());
        }
        let config = Config::load(&self.config_dir.join("config"))?;
        let styles = StyleSheet::load(&self.config_dir.join("style.css"))?;
        styles.animation()?;

        self.prepare_layers();
        if self.layers.is_empty() {
            anyhow::bail!("no Wayland outputs are available");
        }
        for output in &self.layers {
            output
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            output.layer.set_input_region(None);
            output.layer.commit();
        }

        self.config = Some(config);
        self.styles = Some(styles);
        self.active = true;
        self.active_layer = None;
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new());
        }
        self.pool = None;
        self.pointer_position = None;
        self.path.clear();
        self.centers.clear();
        self.display_centers.clear();
        self.link_lengths.clear();
        self.hovered = None;
        self.hover_origins.clear();
        self.hover_started = None;
        self.turbo_active = false;
        self.hover_detector.reset(None);
        self.navigation = None;
        self.item_reveal = 0.0;
        self.closing = None;
        Ok(())
    }

    pub fn finish_hide(&mut self) {
        self.active = false;
        for output in &self.layers {
            output
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            let region = self.compositor.wl_compositor().create_region(&self.qh, ());
            output.layer.set_input_region(Some(&region));
            region.destroy();
            output.layer.commit();
        }
        self.pool = None;
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.clear_icons();
        }
        self.config = None;
        self.styles = None;
        self.path.clear();
        self.centers.clear();
        self.display_centers.clear();
        self.link_lengths.clear();
        self.pointer_position = None;
        self.hovered = None;
        self.hover_origins.clear();
        self.hover_started = None;
        self.turbo_active = false;
        self.hover_detector.reset(None);
        self.navigation = None;
        self.item_reveal = 1.0;
        self.closing = None;
        self.active_layer = None;
        for index in 0..self.layers.len() {
            self.attach_bootstrap_buffer(index);
        }
    }

    pub fn begin_hide(&mut self, action: Option<(usize, Point, Point)>) {
        if !self.active || self.closing.is_some() {
            return;
        }
        let animation = self
            .styles
            .as_ref()
            .and_then(|styles| styles.animation().ok());
        let duration = animation
            .as_ref()
            .map_or(Duration::ZERO, |animation| animation.close_duration);
        if duration.is_zero() {
            self.finish_hide();
            return;
        }
        for output in &self.layers {
            output
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            let region = self.compositor.wl_compositor().create_region(&self.qh, ());
            output.layer.set_input_region(Some(&region));
            region.destroy();
            output.layer.commit();
        }
        self.set_hover(None);
        self.closing = Some(CloseState {
            started: Instant::now(),
            duration,
            action,
            spring: animation
                .as_ref()
                .map_or(Spring::default(), |value| value.action_spring),
            action_scale: animation.as_ref().map_or(1.3, |value| value.action_scale),
        });
        self.draw();
    }

    pub fn visible(&self) -> bool {
        self.active
    }

    fn select_layer(&mut self, index: usize, position: Point) {
        if self.active_layer.is_some() || index >= self.layers.len() {
            return;
        }
        for (candidate, output) in self.layers.iter().enumerate() {
            if candidate == index {
                output
                    .layer
                    .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                output.layer.set_input_region(None);
            } else {
                output
                    .layer
                    .set_keyboard_interactivity(KeyboardInteractivity::None);
                let region = self.compositor.wl_compositor().create_region(&self.qh, ());
                output.layer.set_input_region(Some(&region));
                region.destroy();
            }
            output.layer.commit();
        }
        self.active_layer = Some(index);
        self.width = self.layers[index].width;
        self.height = self.layers[index].height;
        self.configured = self.layers[index].configured;
        self.pointer_position = Some(position);
        self.initialize_center(Some(position));
        self.draw();
    }

    pub fn tick(&mut self) {
        if !self.visible() {
            return;
        }
        let now = Instant::now();
        let mut redraw = false;
        if let Some(navigation) = &self.navigation {
            let raw = (now - navigation.started).as_secs_f64()
                / navigation.duration.as_secs_f64().max(f64::EPSILON);
            let progress = raw.clamp(0.0, 1.0);
            let animation = self.styles.as_ref().unwrap().animation().unwrap();
            let move_progress = animation
                .menu_move_spring
                .sample(progress, navigation.duration.as_secs_f64());
            self.display_centers = navigation
                .from
                .iter()
                .copied()
                .zip(self.centers.iter().copied())
                .map(|(from, target)| from.lerp(target, move_progress))
                .collect();
            self.item_reveal = animation
                .item_create_spring
                .sample(progress, navigation.duration.as_secs_f64());
            redraw = true;
            if progress >= 1.0 {
                self.display_centers = self.centers.clone();
                self.item_reveal = 1.0;
                self.navigation = None;
            }
        }
        if let Some(closing) = &self.closing {
            if now - closing.started >= closing.duration {
                self.finish_hide();
                return;
            }
            redraw = true;
        }
        if self.hover_started.is_some() && self.hover_progress(now) < 1.0 {
            redraw = true;
        }
        let hover_enabled = self
            .config
            .as_ref()
            .is_some_and(|config| config.hover_mode || self.turbo_active);
        if hover_enabled && let Some(position) = self.hover_detector.on_timeout(now) {
            self.activate_mode(position, true, self.turbo_active);
        }
        if redraw {
            self.draw();
        }
    }

    fn initialize_center(&mut self, pointer: Option<Point>) {
        if !self.centers.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        let config = self.config.as_ref().unwrap();
        let center = if config.center_mode {
            Point {
                x: self.width as f64 / 2.0,
                y: self.height as f64 / 2.0,
            }
        } else if let Some(pointer) = pointer {
            pointer
        } else {
            return;
        };
        self.centers.push(clamp_center(
            center,
            self.width,
            self.height,
            config.minimum_edge_distance,
        ));
        self.display_centers = self.centers.clone();
        self.start_navigation(self.display_centers.clone());
    }

    fn current(&self) -> &Item {
        item_at_path(&self.config.as_ref().unwrap().menu, &self.path)
    }

    fn target_at(&self, position: Point) -> Option<Target> {
        let center = *self.centers.last()?;
        let config = self.config.as_ref()?;
        let current = self.current();
        let center_size = config.center_hitbox_size.unwrap_or_else(|| {
            self.styles
                .as_ref()
                .and_then(|styles| styles.circle(&["circle", "circle.center"]).ok())
                .and_then(|style| style.width.map(|width| width * style.scale))
                .unwrap_or(0.0)
        });
        if center_size > 0.0 && center.distance(position) <= center_size / 2.0 {
            return Some(Target::Center);
        }
        let angle = direction_angle(Point {
            x: position.x - center.x,
            y: position.y - center.y,
        });
        let mut candidates = current
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (Target::Item(index), item.angle.unwrap_or(0.0)))
            .collect::<Vec<_>>();
        if !self.path.is_empty() {
            candidates.push((
                Target::Parent(self.path.len() - 1),
                current.return_angle.unwrap_or(180.0),
            ));
        }
        candidates
            .into_iter()
            .min_by(|left, right| {
                angular_distance(angle, left.1).total_cmp(&angular_distance(angle, right.1))
            })
            .map(|candidate| candidate.0)
    }

    fn activate(&mut self, position: Point) {
        self.activate_mode(position, false, false);
    }

    fn activate_mode(&mut self, position: Point, hover: bool, submenu_only: bool) {
        let Some(target) = self.target_at(position) else {
            return;
        };
        if hover && target == Target::Center {
            return;
        }
        match target {
            Target::Center => {
                if !self.path.is_empty()
                    && !self.config.as_ref().unwrap().close_submenu_on_center_click
                {
                    self.return_to_parent(self.path.len() - 1, position);
                } else {
                    self.begin_hide(None);
                }
            }
            Target::Parent(depth) => self.return_to_parent(depth, position),
            Target::Item(index) => {
                let item = self.current().items[index].clone();
                if item.is_submenu() {
                    let config = self.config.as_ref().unwrap();
                    let parent = *self.centers.last().unwrap();
                    let child = clamp_center(
                        position,
                        self.width,
                        self.height,
                        config.minimum_edge_distance,
                    );
                    self.link_lengths
                        .push(parent.distance(child).max(config.menu_radius));
                    self.path.push(index);
                    self.centers.push(child);
                    let mut from = self.display_centers.clone();
                    from.push(position);
                    self.display_centers = from.clone();
                    self.align_chain();
                    self.start_navigation(from);
                    self.reset_hover_visual();
                    self.hover_detector.reset(Some(child));
                    self.draw();
                } else if !submenu_only && let Some(command) = item.command {
                    launch(&command);
                    let center = *self.display_centers.last().unwrap_or(&position);
                    let style = self
                        .styles
                        .as_ref()
                        .and_then(|styles| styles.circle(&["circle", "circle.item"]).ok());
                    let distance = self.config.as_ref().unwrap().menu_radius
                        + style.and_then(|style| style.distance).unwrap_or(0.0);
                    let start = radial_position(center, item.angle.unwrap_or(0.0), distance);
                    self.begin_hide(Some((index, start, position)));
                }
            }
        }
    }

    fn return_to_parent(&mut self, depth: usize, position: Point) {
        self.path.truncate(depth);
        self.centers.truncate(depth + 1);
        let mut from = self.display_centers[..self.display_centers.len().min(depth + 1)].to_vec();
        self.link_lengths.truncate(depth);
        let config = self.config.as_ref().unwrap();
        if let Some(center) = self.centers.last_mut() {
            *center = clamp_center(
                position,
                self.width,
                self.height,
                config.minimum_edge_distance,
            );
        }
        self.align_chain();
        while from.len() < self.centers.len() {
            from.push(*self.centers.last().unwrap());
        }
        self.display_centers = from.clone();
        self.start_navigation(from);
        self.reset_hover_visual();
        self.hover_detector.reset(self.centers.last().copied());
        self.draw();
    }

    fn align_chain(&mut self) {
        let config = self.config.as_ref().unwrap();
        for depth in (1..=self.path.len()).rev() {
            let child = item_at_path(&config.menu, &self.path[..depth]);
            self.centers[depth - 1] = radial_position(
                self.centers[depth],
                child.return_angle.unwrap_or(180.0),
                self.link_lengths[depth - 1],
            );
        }
    }

    fn start_navigation(&mut self, from: Vec<Point>) {
        let duration = self
            .styles
            .as_ref()
            .and_then(|styles| styles.animation().ok())
            .map_or(Duration::ZERO, |animation| animation.menu_duration);
        if duration.is_zero() {
            self.display_centers = self.centers.clone();
            self.item_reveal = 1.0;
            self.navigation = None;
        } else {
            self.item_reveal = 0.0;
            self.navigation = Some(NavigationState {
                started: Instant::now(),
                duration,
                from,
            });
        }
    }

    fn set_hover(&mut self, target: Option<Target>) {
        if target == self.hovered {
            return;
        }
        let now = Instant::now();
        let progress = self.hover_progress(now);
        let mut targets = self.hover_origins.keys().copied().collect::<Vec<_>>();
        targets.extend(self.hovered);
        targets.extend(target);
        targets.sort_by_key(|target| match target {
            Target::Center => (0, 0),
            Target::Parent(index) => (1, *index),
            Target::Item(index) => (2, *index),
        });
        targets.dedup();
        self.hover_origins = targets
            .into_iter()
            .map(|candidate| {
                let origin = self.hover_origins.get(&candidate).copied().unwrap_or(0.0);
                let destination = f64::from(self.hovered == Some(candidate));
                (candidate, origin + (destination - origin) * progress)
            })
            .collect();
        self.hovered = target;
        self.hover_started = Some(now);
    }

    fn reset_hover_visual(&mut self) {
        self.hovered = None;
        self.hover_origins.clear();
        self.hover_started = None;
    }

    fn hover_progress(&self, now: Instant) -> f64 {
        let Some(started) = self.hover_started else {
            return 1.0;
        };
        let Some(animation) = self
            .styles
            .as_ref()
            .and_then(|styles| styles.animation().ok())
        else {
            return 1.0;
        };
        if animation.hover_duration.is_zero() {
            return 1.0;
        }
        let progress = ((now - started).as_secs_f64() / animation.hover_duration.as_secs_f64())
            .clamp(0.0, 1.0);
        animation
            .hover_spring
            .sample(progress, animation.hover_duration.as_secs_f64())
    }

    fn draw(&mut self) {
        let (Some(layer_index), Some(config), Some(styles)) = (
            self.active_layer,
            self.config.as_ref(),
            self.styles.as_ref(),
        ) else {
            return;
        };
        let layer = &self.layers[layer_index].layer;
        if !self.configured || self.centers.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        let needed = self.width as usize * self.height as usize * 4;
        let (close_scale, close_opacity) = self.close_frame();
        let action = self.action_frame();
        let hover_progress = self.hover_progress(Instant::now());
        if self.pool.is_none() {
            self.pool = SlotPool::new(needed, &self.shm).ok();
        }
        let Some(pool) = self.pool.as_mut() else {
            return;
        };
        let Ok((buffer, canvas)) = pool.create_buffer(
            self.width as i32,
            self.height as i32,
            self.width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };
        let Some(mut pixmap) = Pixmap::new(self.width, self.height) else {
            return;
        };
        let scene = Scene {
            config,
            styles,
            path: &self.path,
            centers: &self.display_centers,
            pointer: self.pointer_position,
            hovered: self.hovered,
            hover_origins: &self.hover_origins,
            hover_progress,
            icon_root: &self.config_dir.join("icons"),
            item_reveal: self.item_reveal,
            close_scale,
            close_opacity,
            action,
        };
        self.renderer.as_mut().unwrap().render(&mut pixmap, &scene);
        for (source, target) in pixmap
            .data()
            .chunks_exact(4)
            .zip(canvas.chunks_exact_mut(4))
        {
            target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
        }
        layer
            .wl_surface()
            .damage_buffer(0, 0, self.width as i32, self.height as i32);
        if buffer.attach_to(layer.wl_surface()).is_ok() {
            layer.commit();
        }
    }

    /// Map the fullscreen layer before the compositor has reported the pointer
    /// position. A transparent buffer breaks the Wayland bootstrap cycle:
    /// pointer enter requires a mapped surface, while cursor-centered drawing
    /// requires the coordinates supplied by pointer enter.
    fn attach_bootstrap_buffer(&mut self, layer_index: usize) {
        let Some(output) = self.layers.get(layer_index) else {
            return;
        };
        let layer = &output.layer;
        let width = output.width;
        let height = output.height;
        if width == 0 || height == 0 {
            return;
        }
        let needed = width as usize * height as usize * 4;
        if self.pool.is_none() {
            self.pool = SlotPool::new(needed, &self.shm).ok();
        }
        let Some(pool) = self.pool.as_mut() else {
            return;
        };
        let Ok((buffer, canvas)) = pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };
        canvas.fill(0);
        layer
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
        if buffer.attach_to(layer.wl_surface()).is_ok() {
            layer.commit();
        }
    }

    fn close_frame(&self) -> (f64, f64) {
        let Some(closing) = &self.closing else {
            return (1.0, 1.0);
        };
        let progress = ((Instant::now() - closing.started).as_secs_f64()
            / closing.duration.as_secs_f64())
        .clamp(0.0, 1.0);
        (
            1.0 - smoothstep(progress),
            1.0 - smoothstep((progress / 0.8).min(1.0)),
        )
    }

    fn action_frame(&self) -> Option<ActionFrame> {
        let closing = self.closing.as_ref()?;
        let (index, start, target) = closing.action?;
        let progress = ((Instant::now() - closing.started).as_secs_f64()
            / closing.duration.as_secs_f64())
        .clamp(0.0, 1.0);
        let flight_end = 2.0 / 3.0;
        let flight = (progress / flight_end).min(1.0);
        let fade = ((progress - flight_end) / (1.0 - flight_end)).clamp(0.0, 1.0);
        Some(ActionFrame {
            index,
            start,
            target,
            position_progress: closing
                .spring
                .sample(flight, closing.duration.as_secs_f64() * flight_end),
            growth_progress: closing
                .spring
                .sample(progress, closing.duration.as_secs_f64()),
            opacity: 1.0 - smoothstep(fade),
            final_scale: closing.action_scale,
        })
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(index) = self.layers.iter().position(|output| &output.layer == layer) {
            let was_active = self.active_layer == Some(index);
            self.layers.remove(index);
            self.active_layer = self.active_layer.and_then(|active| {
                if active == index {
                    None
                } else {
                    Some(active - usize::from(active > index))
                }
            });
            if was_active {
                self.finish_hide();
            }
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(index) = self.layers.iter().position(|output| &output.layer == layer) else {
            return;
        };
        let output = &mut self.layers[index];
        output.width = configure.new_size.0;
        output.height = configure.new_size.1;
        output.configured = output.width > 0 && output.height > 0;
        if self.active_layer == Some(index) {
            self.width = output.width;
            self.height = output.height;
            self.configured = output.configured;
            self.initialize_center(self.pointer_position);
            if self.centers.is_empty() {
                self.attach_bootstrap_buffer(index);
            } else {
                self.draw();
            }
        } else {
            self.attach_bootstrap_buffer(index);
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
            if !self.active {
                continue;
            }
            let Some(layer_index) = self
                .layers
                .iter()
                .position(|output| &event.surface == output.layer.wl_surface())
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
                self.select_layer(layer_index, position);
            }
            if self.active_layer != Some(layer_index) {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.pointer_position = Some(position);
                    self.initialize_center(Some(position));
                    let target = self.target_at(position);
                    if target != self.hovered {
                        self.set_hover(target);
                        self.draw();
                    }
                    let hover_enabled = self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.hover_mode || self.turbo_active);
                    if hover_enabled
                        && let Some(selection) =
                            self.hover_detector.on_motion(position, Instant::now())
                    {
                        self.activate_mode(selection, true, self.turbo_active);
                    }
                }
                PointerEventKind::Press { button, .. } if button == LEFT_BUTTON => {
                    self.activate(position);
                }
                PointerEventKind::Press { button, .. } if button == RIGHT_BUTTON => {
                    self.begin_hide(None)
                }
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
            self.begin_hide(None);
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
        self.modifiers = modifiers;
        let held = modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.logo;
        let turbo_enabled = self.config.as_ref().is_some_and(|config| config.turbo_mode);
        self.turbo_active = turbo_enabled && held;
        if !was_active && self.turbo_active {
            self.hover_detector.reset(self.centers.last().copied());
        } else if was_active
            && !self.turbo_active
            && let Some(position) = self.pointer_position
        {
            self.activate_mode(position, true, false);
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
            self.layers.remove(index);
            if self.active_layer == Some(index) {
                self.active_layer = None;
                self.finish_hide();
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

pub fn runtime_paths() -> (PathBuf, PathBuf) {
    let runtime = env::var_os("WAYPIE_RUNTIME_DIR")
        .or_else(|| env::var_os("XDG_RUNTIME_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("waypie");
    let display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland".into());
    (
        runtime.join(format!("control-{display}.sock")),
        runtime.join(format!("process-{display}.pid")),
    )
}

pub fn bind_control_socket() -> Result<(UnixDatagram, PathBuf, PathBuf)> {
    let (socket_path, pid_path) = runtime_paths();
    fs::create_dir_all(socket_path.parent().unwrap())?;
    let socket = UnixDatagram::bind(&socket_path)
        .with_context(|| format!("cannot bind {}", socket_path.display()))?;
    socket.set_nonblocking(true)?;
    fs::write(&pid_path, format!("{}\n", std::process::id()))?;
    Ok((socket, socket_path, pid_path))
}

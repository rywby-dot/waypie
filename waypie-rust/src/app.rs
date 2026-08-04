use std::{env, fs, os::unix::net::UnixDatagram, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
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
    config::Config,
    geometry::Point,
    model::{MenuState, Target},
    render::{Renderer, Scene},
    style::StyleSheet,
};

const LEFT_BUTTON: u32 = 0x110;
const RIGHT_BUTTON: u32 = 0x111;

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

    layers: Vec<OutputLayer>,
    active_layer: Option<usize>,
    visible: bool,
    buffers: Option<RenderBuffers>,
    redraw_pending: bool,
    renderer: Renderer,
    config: Option<Config>,
    styles: Option<StyleSheet>,
    pointer: Option<ThemedPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    state: MenuState,
    config_dir: PathBuf,
    pending_activation: Option<String>,
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
            layers: vec![],
            active_layer: None,
            visible: false,
            buffers: None,
            redraw_pending: false,
            renderer: Renderer::new(),
            config: None,
            styles: None,
            pointer: None,
            keyboard: None,
            state: MenuState::default(),
            config_dir,
            pending_activation: None,
        }
    }

    pub fn handle_control(&mut self, command: &[u8]) {
        let (name, token) = if let Some(separator) = command.iter().position(|byte| *byte == 0) {
            let (name, token) = command.split_at(separator);
            (
                name,
                std::str::from_utf8(&token[1..]).ok().map(str::to_owned),
            )
        } else {
            (command, None)
        };
        match name {
            b"show" if self.visible => self.hide(),
            b"show" => {
                if let Err(error) = self.show(token) {
                    eprintln!("waypie: {error:#}");
                }
            }
            b"quit" => {
                self.hide();
                self.layers.clear();
                self.exit = true;
            }
            b"ping" => {}
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
        self.renderer.configure_fonts(styles.font_families());
        self.prepare_layers();
        if self.layers.is_empty() {
            bail!("no Wayland outputs are available");
        }
        self.config = Some(config);
        self.styles = Some(styles);
        self.state.reset();
        self.pending_activation = activation_token;
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
        if !self.visible && self.config.is_none() {
            return;
        }
        self.visible = false;
        self.active_layer = None;
        self.state.reset();
        self.config = None;
        self.styles = None;
        self.renderer.clear_icons();
        self.buffers = None;
        self.redraw_pending = false;
        self.pending_activation = None;
        for index in 0..self.layers.len() {
            let surface = &self.layers[index].surface;
            surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            self.set_click_through(surface, true);
            surface.commit();
        }
        for index in 0..self.layers.len() {
            self.attach_transparent(index);
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
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
        let config = self.config.as_ref().expect("visible menu has config");
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

    fn update_pointer(&mut self, position: Point) {
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
        if self
            .state
            .update_pointer(position, config, self.center_hitbox())
        {
            self.draw();
        }
    }

    fn activate(&mut self, position: Point) {
        let Some(index) = self.active_layer else {
            return;
        };
        self.update_pointer(position);
        let Some(target) = self.state.active() else {
            return;
        };
        match target {
            Target::Center => {
                let config = self.config.as_ref().unwrap();
                if !self.state.path().is_empty() && !config.close_submenu_on_center_click {
                    let depth = self.state.path().len() - 1;
                    self.state.return_to(
                        depth,
                        position,
                        config,
                        self.layers[index].width,
                        self.layers[index].height,
                    );
                    self.draw();
                } else {
                    self.hide();
                }
            }
            Target::Parent(depth) => {
                let config = self.config.as_ref().unwrap();
                self.state.return_to(
                    depth,
                    position,
                    config,
                    self.layers[index].width,
                    self.layers[index].height,
                );
                self.draw();
            }
            Target::Item(item_index) => {
                let item =
                    self.state.current(self.config.as_ref().unwrap()).items[item_index].clone();
                if item.is_submenu() {
                    let config = self.config.as_ref().unwrap();
                    self.state.open_submenu(
                        item_index,
                        position,
                        config,
                        self.layers[index].width,
                        self.layers[index].height,
                    );
                    self.draw();
                } else if let Some(command) = item.command {
                    launch(&command);
                    self.hide();
                }
            }
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
        self.renderer.render(
            &mut pixmap,
            &Scene {
                config,
                styles,
                state: &self.state,
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
        _: Modifiers,
        _: u32,
    ) {
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

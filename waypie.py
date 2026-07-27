#!/usr/bin/env python3

import ctypes.util
import math
import os
import re
import socket
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path


LAYER_SHELL_LIBRARY = "libgtk4-layer-shell.so"
CONFIG_DIR = Path.home() / ".config" / "waypie"
CONFIG_PATH = CONFIG_DIR / "config"
STYLE_PATH = CONFIG_DIR / "style.css"


def control_socket_path():
    runtime_dir = os.environ.get("XDG_RUNTIME_DIR", "/tmp")
    display = os.environ.get("WAYLAND_DISPLAY", "wayland")
    return Path(runtime_dir) / "waypie" / f"control-{display}.sock"


def send_fast_control_command():
    if sys.argv[1:] != ["--show"] or not os.environ.get("WAYLAND_DISPLAY"):
        return

    connection = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    try:
        connection.sendto(b"show", str(control_socket_path()))
    except OSError:
        return
    finally:
        connection.close()
    raise SystemExit(0)


send_fast_control_command()


def preload_layer_shell():
    if LAYER_SHELL_LIBRARY in os.environ.get("LD_PRELOAD", ""):
        return

    library = ctypes.util.find_library("gtk4-layer-shell")
    if not library:
        for candidate in (
            "/usr/lib64/libgtk4-layer-shell.so",
            "/usr/lib/libgtk4-layer-shell.so",
        ):
            if Path(candidate).exists():
                library = candidate
                break
    if not library:
        raise SystemExit("waypie: libgtk4-layer-shell.so was not found")

    environment = os.environ.copy()
    previous = environment.get("LD_PRELOAD", "")
    environment["LD_PRELOAD"] = f"{library}:{previous}" if previous else library
    os.execve(sys.executable, [sys.executable, *sys.argv], environment)


preload_layer_shell()

import cairo
import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("Gtk4LayerShell", "1.0")

from gi.repository import Gdk, GLib, Gtk, Gtk4LayerShell

Gtk.Window.set_auto_startup_notification(False)


@dataclass
class Item:
    label: str
    command: str | None = None
    angle: float | None = None
    distance: float | None = None
    x: float | None = None
    y: float | None = None
    size: float | None = None
    items: list["Item"] = field(default_factory=list)


@dataclass
class Settings:
    circle_size: float
    menu_radius: float
    root: Item


DEFAULT_STYLE = {
    "background-color": (0.0, 0.0, 0.0, 0.0),
    "border-color": (0.0, 0.0, 0.0, 0.0),
    "border-width": 0.0,
    "border-radius": "50%",
    "color": (1.0, 1.0, 1.0, 1.0),
    "font-size": 14.0,
    "font-family": "Sans",
}


def load_config():
    try:
        with CONFIG_PATH.open("rb") as file:
            source = tomllib.load(file)
    except OSError as error:
        raise SystemExit(f"waypie: cannot read {CONFIG_PATH}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise SystemExit(f"waypie: invalid config: {error}") from error

    menu = source.get("menu")
    if not isinstance(menu, dict):
        raise SystemExit("waypie: config requires a [menu] table")

    circle_size = positive_number(source.get("circle-size", 100), "circle-size")
    menu_radius = positive_number(source.get("menu-radius", 150), "menu-radius")
    return Settings(circle_size, menu_radius, parse_item(menu, "menu", True))


def parse_item(source, location, root=False):
    if not isinstance(source, dict):
        raise SystemExit(f"waypie: {location} must be a table")

    label = source.get("label", "")
    command = source.get("command")
    children = source.get("items", [])
    if not isinstance(label, str):
        raise SystemExit(f"waypie: {location}.label must be text")
    if command is not None and not isinstance(command, str):
        raise SystemExit(f"waypie: {location}.command must be text")
    if not isinstance(children, list):
        raise SystemExit(f"waypie: {location}.items must be an array")
    if command and children:
        raise SystemExit(f"waypie: {location} cannot have command and items")
    if not root and not command and not children:
        raise SystemExit(f"waypie: {location} needs command or items")

    angle = optional_number(source.get("angle"), f"{location}.angle")
    distance = optional_positive(source.get("distance"), f"{location}.distance")
    x = optional_number(source.get("x"), f"{location}.x")
    y = optional_number(source.get("y"), f"{location}.y")
    size = optional_positive(source.get("size"), f"{location}.size")
    if (x is None) != (y is None):
        raise SystemExit(f"waypie: {location}.x and .y must be used together")

    return Item(
        label=label,
        command=command,
        angle=angle,
        distance=distance,
        x=x,
        y=y,
        size=size,
        items=[
            parse_item(child, f"{location}.items[{index}]")
            for index, child in enumerate(children)
        ],
    )


def positive_number(value, location):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SystemExit(f"waypie: {location} must be a number")
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise SystemExit(f"waypie: {location} must be positive")
    return value


def optional_positive(value, location):
    return None if value is None else positive_number(value, location)


def optional_number(value, location):
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SystemExit(f"waypie: {location} must be a number")
    value = float(value)
    if not math.isfinite(value):
        raise SystemExit(f"waypie: {location} must be finite")
    return value


def load_styles():
    try:
        source = STYLE_PATH.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"waypie: cannot read {STYLE_PATH}: {error}") from error

    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    rules = {}
    for selectors, declarations in re.findall(r"([^{}]+)\{([^{}]*)\}", source):
        properties = {}
        for declaration in declarations.split(";"):
            if ":" not in declaration:
                continue
            name, value = declaration.split(":", 1)
            properties[name.strip().lower()] = value.strip()
        for selector in selectors.split(","):
            rules.setdefault(selector.strip().lower(), {}).update(properties)
    return rules


def computed_style(rules, selectors):
    style = dict(DEFAULT_STYLE)
    style["background-color"] = (0x24 / 255, 0x24 / 255, 0x24 / 255, 1.0)
    for selector in selectors:
        for name, value in rules.get(selector, {}).items():
            if name in {"background-color", "border-color", "color"}:
                style[name] = parse_color(value, name)
            elif name in {"border-width", "font-size"}:
                style[name] = parse_pixels(value, name)
            elif name == "border-radius":
                style[name] = value
            elif name == "font-family":
                style[name] = value.strip("\"'")
    return style


def parse_pixels(value, name):
    match = re.fullmatch(r"(-?(?:\d+(?:\.\d*)?|\.\d+))(?:px)?", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    return max(0.0, float(match.group(1)))


def parse_color(value, name):
    value = value.strip().lower()
    if value == "transparent":
        return 0.0, 0.0, 0.0, 0.0
    if match := re.fullmatch(r"#([0-9a-f]{6})([0-9a-f]{2})?", value):
        rgb, alpha = match.groups()
        return (
            int(rgb[0:2], 16) / 255,
            int(rgb[2:4], 16) / 255,
            int(rgb[4:6], 16) / 255,
            int(alpha, 16) / 255 if alpha else 1.0,
        )
    if match := re.fullmatch(r"rgba?\(([^)]+)\)", value):
        parts = [part.strip() for part in match.group(1).split(",")]
        if len(parts) in (3, 4):
            try:
                rgb = [max(0, min(255, float(part))) / 255 for part in parts[:3]]
                alpha = max(0.0, min(1.0, float(parts[3]))) if len(parts) == 4 else 1.0
                return *rgb, alpha
            except ValueError:
                pass
    raise SystemExit(f"waypie: invalid {name}: {value}")


class Waypie(Gtk.Application):
    def __init__(self, settings, styles, start_visible):
        super().__init__(application_id="dev.waypie.Waypie")
        self.settings = settings
        self.styles = styles
        self.start_visible = start_visible
        self.started = False
        self.visible = False
        self.path = []
        self.hits = []
        self.canvas = None
        self.window = None
        self.menu_position = None
        self.menu_centers = []
        self.control_socket = None
        self.control_source = None

    def do_activate(self):
        if self.started:
            return
        self.started = True

        self.window = Gtk.ApplicationWindow(application=self)
        self.window.set_decorated(False)

        Gtk4LayerShell.init_for_window(self.window)
        Gtk4LayerShell.set_namespace(self.window, "waypie")
        Gtk4LayerShell.set_layer(self.window, Gtk4LayerShell.Layer.OVERLAY)
        Gtk4LayerShell.set_exclusive_zone(self.window, -1)
        Gtk4LayerShell.set_keyboard_mode(
            self.window, Gtk4LayerShell.KeyboardMode.NONE
        )
        for edge in (
            Gtk4LayerShell.Edge.TOP,
            Gtk4LayerShell.Edge.RIGHT,
            Gtk4LayerShell.Edge.BOTTOM,
            Gtk4LayerShell.Edge.LEFT,
        ):
            Gtk4LayerShell.set_anchor(self.window, edge, True)

        self.canvas = Gtk.DrawingArea()
        self.canvas.set_cursor_from_name("default")
        self.canvas.set_content_width(0)
        self.canvas.set_content_height(0)
        self.canvas.set_draw_func(self.draw)
        self.window.set_default_size(0, 0)
        self.window.set_cursor_from_name("default")

        self.window.add_css_class("transparent")
        css = Gtk.CssProvider()
        css.load_from_string(
            ".transparent, .transparent * {"
            "background-color: rgba(0,0,0,0); background: none;}"
        )
        Gtk.StyleContext.add_provider_for_display(
            self.window.get_display(), css, Gtk.STYLE_PROVIDER_PRIORITY_USER
        )

        click = Gtk.GestureClick()
        click.set_button(1)
        click.connect("released", self.on_click)
        self.canvas.add_controller(click)

        pointer = Gtk.EventControllerMotion()
        pointer.connect("enter", self.on_pointer_event)
        pointer.connect("motion", self.on_pointer_event)
        self.canvas.add_controller(pointer)

        self.window.set_child(self.canvas)
        self.start_control_server()
        self.hold()

        if self.start_visible:
            self.show_menu()
        else:
            self.window.set_visible(False)

    def start_control_server(self):
        path = control_socket_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            path.unlink()
        except FileNotFoundError:
            pass

        self.control_socket = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.control_socket.bind(str(path))
        self.control_socket.setblocking(False)
        self.control_source = GLib.io_add_watch(
            self.control_socket.fileno(),
            GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR,
            self.on_control_command,
        )

    def on_control_command(self, _file_descriptor, condition):
        if condition & (GLib.IO_HUP | GLib.IO_ERR):
            return True
        try:
            command = self.control_socket.recv(64)
        except BlockingIOError:
            return True
        if command == b"show":
            if self.visible:
                self.hide_menu()
            else:
                self.show_menu()
        return True

    def show_menu(self):
        self.visible = True
        self.menu_position = None
        self.menu_centers = []
        self.hits = []
        self.window.set_cursor_from_name("default")
        self.canvas.set_cursor_from_name("default")
        self.window.set_visible(True)
        self.canvas.queue_draw()

    def hide_menu(self):
        self.visible = False
        self.path.clear()
        self.menu_position = None
        self.menu_centers = []
        self.hits = []
        self.window.set_visible(False)

    def on_pointer_event(self, _controller, x, y):
        if self.visible and self.menu_position is None:
            self.menu_position = (x, y)
            self.menu_centers = [(x, y)]
            self.canvas.queue_draw()

    def current_item(self):
        item = self.settings.root
        for index in self.path:
            item = item.items[index]
        return item

    def draw(self, _canvas, context, width, height):
        self.hits = []

        overlay = computed_style(self.styles, ("overlay",))
        context.set_source_rgba(*overlay["background-color"])
        context.paint()

        if self.menu_position is None:
            return

        center_x, center_y = self.menu_centers[-1]
        current = self.current_item()
        size = current.size or self.settings.circle_size
        self.draw_item(context, center_x, center_y, size, current, True)
        self.hits.append((center_x, center_y, size, "center", None))

        for depth, (parent_x, parent_y) in enumerate(self.menu_centers[:-1]):
            parent = self.item_at_path(self.path[:depth])
            parent_size = parent.size or self.settings.circle_size
            self.draw_item(
                context,
                parent_x,
                parent_y,
                parent_size,
                parent,
                False,
                parent=True,
            )
            self.hits.append(
                (parent_x, parent_y, parent_size, "parent", depth)
            )

        count = len(current.items)
        step = 360 / count if count else 0
        for index, item in enumerate(current.items):
            if item.x is not None:
                x = center_x + item.x
                y = center_y + item.y
            else:
                angle = item.angle if item.angle is not None else index * step
                distance = item.distance or self.settings.menu_radius
                radians = math.radians(angle)
                x = center_x + distance * math.sin(radians)
                y = center_y - distance * math.cos(radians)
            size = item.size or self.settings.circle_size
            self.draw_item(context, x, y, size, item, False)
            self.hits.append((x, y, size, "item", index))

    def draw_item(self, context, x, y, size, item, center, parent=False):
        selectors = ["circle"]
        if center:
            selectors.append("circle.center")
        elif parent:
            selectors.append("circle.parent")
        else:
            selectors.append("circle.item")
        if item.items:
            selectors.append("circle.submenu")
        style = computed_style(self.styles, selectors)
        radius = resolve_radius(style["border-radius"], size)
        left = x - size / 2
        top = y - size / 2

        rounded_rectangle(context, left, top, size, size, radius)
        context.set_source_rgba(*style["background-color"])
        context.fill_preserve()
        if style["border-width"] > 0:
            context.set_source_rgba(*style["border-color"])
            context.set_line_width(style["border-width"])
            context.stroke()
        else:
            context.new_path()

        if not item.label:
            return
        context.select_font_face(
            style["font-family"], cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        context.set_font_size(style["font-size"])
        context.set_source_rgba(*style["color"])
        label = truncate(item.label, max(1, int(size / style["font-size"] * 1.5)))
        extents = context.text_extents(label)
        context.move_to(
            x - extents.width / 2 - extents.x_bearing,
            y - extents.height / 2 - extents.y_bearing,
        )
        context.show_text(label)

    def on_click(self, _gesture, _presses, x, y):
        if not self.hits:
            return

        center_x, center_y, center_size, _kind, _index = self.hits[0]
        for hit_x, hit_y, hit_size, kind, target in reversed(self.hits):
            if (
                kind in {"center", "parent"}
                and math.hypot(x - hit_x, y - hit_y) <= hit_size / 2
            ):
                if kind == "parent":
                    self.path = self.path[:target]
                    self.menu_centers = self.menu_centers[: target + 1]
                    self.canvas.queue_draw()
                elif not self.path:
                    self.hide_menu()
                return

        item_hits = [hit for hit in self.hits if hit[3] == "item"]
        if not item_hits:
            return

        pointer_angle = direction_angle(x - center_x, y - center_y)
        selected = min(
            item_hits,
            key=lambda hit: angular_distance(
                pointer_angle,
                direction_angle(hit[0] - center_x, hit[1] - center_y),
            ),
        )
        index = selected[4]
        item = self.current_item().items[index]
        if item.items:
            self.path.append(index)
            self.menu_centers.append((x, y))
            self.canvas.queue_draw()
        else:
            launch(item.command)
            self.hide_menu()

    def item_at_path(self, path):
        item = self.settings.root
        for index in path:
            item = item.items[index]
        return item

    def do_shutdown(self):
        if self.control_source is not None:
            GLib.source_remove(self.control_source)
            self.control_source = None
        if self.control_socket is not None:
            self.control_socket.close()
            self.control_socket = None
        try:
            control_socket_path().unlink()
        except FileNotFoundError:
            pass
        Gtk.Application.do_shutdown(self)


def resolve_radius(value, size):
    value = value.strip().lower()
    if value.endswith("%"):
        try:
            return min(size / 2, max(0, size * float(value[:-1]) / 100))
        except ValueError:
            pass
    return min(size / 2, parse_pixels(value, "border-radius"))


def rounded_rectangle(context, x, y, width, height, radius):
    radius = min(radius, width / 2, height / 2)
    context.new_sub_path()
    context.arc(x + width - radius, y + radius, radius, -math.pi / 2, 0)
    context.arc(x + width - radius, y + height - radius, radius, 0, math.pi / 2)
    context.arc(x + radius, y + height - radius, radius, math.pi / 2, math.pi)
    context.arc(x + radius, y + radius, radius, math.pi, 3 * math.pi / 2)
    context.close_path()


def truncate(text, limit):
    if len(text) <= limit:
        return text
    return text[: max(1, limit - 1)] + "…"


def direction_angle(x, y):
    """Return clockwise degrees where zero points upwards."""
    return math.degrees(math.atan2(x, -y)) % 360


def angular_distance(first, second):
    return abs((first - second + 180) % 360 - 180)


def launch(command):
    try:
        subprocess.Popen(
            command,
            shell=True,
            executable="/bin/sh",
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as error:
        print(f"waypie: failed to start {command}: {error}", file=sys.stderr)


if __name__ == "__main__":
    start_visible = sys.argv[1:] == ["--show"]
    try:
        exit_code = Waypie(load_config(), load_styles(), start_visible).run(
            [sys.argv[0]]
        )
    except KeyboardInterrupt:
        exit_code = 0
    raise SystemExit(exit_code)

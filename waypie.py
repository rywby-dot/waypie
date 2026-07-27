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
    return_angle: float | None = None
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
    "distance": None,
    "font-size": 14.0,
    "font-family": "Sans",
    "opacity": 1.0,
    "scale": 1.0,
    "width": None,
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
    root = parse_item(menu, "menu", True)
    resolve_angles(root, root=True)
    return Settings(circle_size, menu_radius, root)


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


def resolve_angles(item, root=False):
    extra_items = 0 if root else 1
    count = len(item.items) + extra_items
    step = 360 / count if count else 0
    for index, child in enumerate(item.items):
        if child.angle is None:
            child.angle = index * step
        resolve_angles(child)
    if not root and item.items:
        item.return_angle = largest_gap_angle(
            [child.angle % 360 for child in item.items]
        )


def largest_gap_angle(angles):
    if not angles:
        return None
    ordered = sorted(angles)
    best_start = ordered[0]
    best_gap = -1.0
    for index, start in enumerate(ordered):
        end = ordered[(index + 1) % len(ordered)]
        if index == len(ordered) - 1:
            end += 360
        gap = end - start
        if gap > best_gap:
            best_start = start
            best_gap = gap
    return (best_start + best_gap / 2) % 360


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
    for selector in selectors:
        for name, value in rules.get(selector, {}).items():
            if name in {"background-color", "border-color", "color"}:
                style[name] = parse_color(value, name)
            elif name in {"border-width", "distance", "font-size", "width"}:
                style[name] = parse_pixels(value, name)
            elif name == "opacity":
                style[name] = parse_opacity(value)
            elif name == "scale":
                style[name] = positive_number_string(value, name)
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


def parse_opacity(value):
    try:
        opacity = float(value)
    except ValueError:
        raise SystemExit(f"waypie: invalid opacity: {value}") from None
    if not math.isfinite(opacity) or not 0 <= opacity <= 1:
        raise SystemExit(f"waypie: opacity must be between 0 and 1: {value}")
    return opacity


def positive_number_string(value, name):
    try:
        number = float(value)
    except ValueError:
        raise SystemExit(f"waypie: invalid {name}: {value}") from None
    if not math.isfinite(number) or number <= 0:
        raise SystemExit(f"waypie: {name} must be positive: {value}")
    return number


class Waypie(Gtk.Application):
    def __init__(self, settings, styles, start_visible):
        super().__init__(application_id="dev.waypie.Waypie")
        self.settings = settings
        self.styles = styles
        self.start_visible = start_visible
        self.path = []
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        self.canvas = None
        self.window = None
        self.menu_centers = []
        self.control_socket = None
        self.control_source = None

    def do_activate(self):
        if self.window is not None:
            return

        self.window = Gtk.ApplicationWindow(application=self)
        self.window.set_decorated(False)

        Gtk4LayerShell.init_for_window(self.window)
        Gtk4LayerShell.set_namespace(self.window, "waypie")
        Gtk4LayerShell.set_layer(self.window, Gtk4LayerShell.Layer.OVERLAY)
        Gtk4LayerShell.set_exclusive_zone(self.window, -1)
        Gtk4LayerShell.set_keyboard_mode(self.window, Gtk4LayerShell.KeyboardMode.NONE)
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

        keyboard = Gtk.EventControllerKey()
        keyboard.connect("key-pressed", self.on_key_pressed)
        self.window.add_controller(keyboard)

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
            if self.window.get_visible():
                self.hide_menu()
            else:
                self.show_menu()
        return True

    def show_menu(self):
        self.menu_centers = []
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        self.window.set_cursor_from_name("default")
        self.canvas.set_cursor_from_name("default")
        Gtk4LayerShell.set_keyboard_mode(
            self.window, Gtk4LayerShell.KeyboardMode.EXCLUSIVE
        )
        self.window.set_visible(True)
        self.canvas.queue_draw()

    def hide_menu(self):
        self.path.clear()
        self.menu_centers = []
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        Gtk4LayerShell.set_keyboard_mode(self.window, Gtk4LayerShell.KeyboardMode.NONE)
        self.window.set_visible(False)

    def on_key_pressed(self, _controller, keyval, _keycode, _state):
        if keyval == Gdk.KEY_Escape:
            self.hide_menu()
            return True
        return False

    def on_pointer_event(self, _controller, x, y):
        if not self.window.get_visible():
            return
        self.pointer_position = (x, y)
        if not self.menu_centers:
            self.menu_centers = [(x, y)]
            self.canvas.queue_draw()
            return
        hovered_hit = self.target_at(x, y)
        if hovered_hit != self.hovered_hit:
            self.hovered_hit = hovered_hit
            self.canvas.queue_draw()

    def draw(self, _canvas, context, _width, _height):
        self.hits = []

        overlay = computed_style(self.styles, ("overlay",))
        context.set_source_rgba(*overlay["background-color"])
        context.paint()

        if not self.menu_centers:
            return

        center_x, center_y = self.menu_centers[-1]
        current = self.item_at_path(self.path)
        style = self.item_style(
            current, center=True, active=self.hovered_hit == ("center", None)
        )
        size = self.item_size(current, style)
        self.draw_item(context, center_x, center_y, size, current, style)
        self.hits.append((center_x, center_y, size, "center", None, None))

        if len(self.path) > 1:
            ancestor = self.item_at_path(self.path[:-2])
            style = self.item_style(ancestor, ancestor=True)
            ancestor_size = self.item_size(ancestor, style)
            ancestor_x, ancestor_y = self.menu_centers[-3]
            self.draw_item(
                context,
                ancestor_x,
                ancestor_y,
                ancestor_size,
                ancestor,
                style,
            )

        if self.path:
            depth = len(self.path) - 1
            parent = self.item_at_path(self.path[:-1])
            style = self.item_style(
                parent, parent=True, active=self.hovered_hit == ("parent", depth)
            )
            parent_size = self.item_size(parent, style)
            parent_x, parent_y = self.menu_centers[depth]
            self.draw_item(
                context,
                parent_x,
                parent_y,
                parent_size,
                parent,
                style,
            )
            self.hits.append(
                (
                    parent_x,
                    parent_y,
                    parent_size,
                    "parent",
                    depth,
                    current.return_angle,
                )
            )

        for index, item in enumerate(current.items):
            style = self.item_style(item, active=self.hovered_hit == ("item", index))
            if item.x is not None:
                x = center_x + item.x
                y = center_y + item.y
            else:
                x, y = self.radial_position(
                    (center_x, center_y),
                    item.angle,
                    style,
                    item.distance,
                )
            size = self.item_size(item, style)
            self.draw_item(context, x, y, size, item, style)
            selection_angle = None if item.x is not None else item.angle
            self.hits.append((x, y, size, "item", index, selection_angle))

        if self.pointer_position is not None:
            hovered_hit = self.target_at(*self.pointer_position)
            if hovered_hit != self.hovered_hit:
                self.hovered_hit = hovered_hit
                self.canvas.queue_draw()

    def item_style(
        self, item, center=False, parent=False, ancestor=False, active=False
    ):
        selectors = ["circle"]
        if item.items:
            selectors.append("circle.submenu")
        if center:
            selectors.append("circle.center")
        elif parent:
            selectors.append("circle.parent")
        elif ancestor:
            selectors.append("circle.ancestor")
        else:
            selectors.append("circle.item")
        if active:
            selectors.append("circle.active")
        return computed_style(self.styles, selectors)

    def item_size(self, item, style):
        if style["width"] is not None:
            size = style["width"]
        else:
            size = item.size or self.settings.circle_size
        return size * style["scale"]

    def radial_position(self, center, angle, style, configured_distance=None):
        distance = (
            style["distance"]
            if style["distance"] is not None
            else configured_distance or self.settings.menu_radius
        )
        radians = math.radians(angle)
        return (
            center[0] + distance * math.sin(radians),
            center[1] - distance * math.cos(radians),
        )

    def draw_item(self, context, x, y, size, item, style):
        radius = resolve_radius(style["border-radius"], size)
        left = x - size / 2
        top = y - size / 2

        rounded_rectangle(context, left, top, size, size, radius)
        set_source_color(context, style["background-color"], style["opacity"])
        context.fill_preserve()
        if style["border-width"] > 0:
            set_source_color(context, style["border-color"], style["opacity"])
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
        set_source_color(context, style["color"], style["opacity"])
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

        target = self.target_at(x, y)
        if target is None:
            return
        kind, index = target
        if kind == "center":
            self.hide_menu()
            return
        if kind == "parent":
            self.path = self.path[:index]
            self.menu_centers = self.menu_centers[: index + 1]
            self.hovered_hit = None
            self.canvas.queue_draw()
            return

        item = self.item_at_path(self.path).items[index]
        if item.items:
            self.path.append(index)
            self.menu_centers.append((x, y))
            self.hovered_hit = None
            self.canvas.queue_draw()
        else:
            launch(item.command)
            self.hide_menu()

    def target_at(self, x, y):
        if not self.hits:
            return None
        center_x, center_y, _center_size, _kind, _index, _angle = self.hits[0]
        for hit_x, hit_y, hit_size, kind, target, _angle in reversed(self.hits):
            if kind in {"center", "parent"} and (
                math.hypot(x - hit_x, y - hit_y) <= hit_size / 2
            ):
                return kind, target
        item_hits = [hit for hit in self.hits if hit[3] in {"item", "parent"}]
        if not item_hits:
            return None
        pointer_angle = direction_angle(x - center_x, y - center_y)
        selected = min(
            item_hits,
            key=lambda hit: angular_distance(
                pointer_angle,
                (
                    hit[5]
                    if hit[5] is not None
                    else direction_angle(hit[0] - center_x, hit[1] - center_y)
                ),
            ),
        )
        return selected[3], selected[4]

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


def set_source_color(context, color, opacity):
    red, green, blue, alpha = color
    context.set_source_rgba(red, green, blue, alpha * opacity)


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


def main():
    start_visible = sys.argv[1:] == ["--show"]
    try:
        exit_code = Waypie(load_config(), load_styles(), start_visible).run(
            [sys.argv[0]]
        )
    except KeyboardInterrupt:
        exit_code = 0
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()

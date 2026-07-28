#!/usr/bin/env python3

import ctypes.util
import math
import os
import re
import socket
import subprocess
import sys
from itertools import pairwise
from pathlib import Path

from waypie_common import (
    angular_distance,
    animation_duration,
    computed_style,
    content_opacity,
    direction_angle,
    ease_out_cubic,
    icon_path,
    load_config,
    load_styles,
    resolve_radius,
    rounded_rectangle,
    set_source_color,
    truncate,
)

LAYER_SHELL_LIBRARY = "libgtk4-layer-shell.so"
CONFIGURATOR_MODE = os.environ.get("WAYPIE_CONFIGURATOR") == "1" or sys.argv[1:] == [
    "--configure"
]


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
    if CONFIGURATOR_MODE:
        return
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
gi.require_version("GdkPixbuf", "2.0")
if CONFIGURATOR_MODE:
    from gi.repository import Gdk, GdkPixbuf, GLib, Gtk

    Gtk4LayerShell = None
else:
    gi.require_version("Gtk4LayerShell", "1.0")
    from gi.repository import Gdk, GdkPixbuf, GLib, Gtk, Gtk4LayerShell

Gtk.Window.set_auto_startup_notification(False)


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
        self.display_centers = []
        self.menu_progress = 1.0
        self.menu_animation_started = None
        self.menu_animation_from = 1.0
        self.menu_animation_to = 1.0
        self.menu_start_centers = []
        self.transition_progress = 1.0
        self.current_scene = []
        self.current_connectors = []
        self.departing_scene = []
        self.departing_connectors = []
        self.visual_positions = {}
        self.scale_values = {}
        self.scale_animations = {}
        self.distance_values = {}
        self.distance_animations = {}
        self.animation_tick = None
        self.closing = False
        self.action_closing = False
        self.action_animation_started = None
        self.action_progress = 1.0
        self.action_item_index = None
        self.action_start_position = None
        self.action_target_position = None
        self.canvas = None
        self.window = None
        self.menu_centers = []
        self.menu_link_lengths = []
        self.control_socket = None
        self.control_source = None
        self.icon_cache = {}

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
        try:
            self.settings = load_config()
        except SystemExit as error:
            print(error, file=sys.stderr)
        self.menu_centers = []
        self.menu_link_lengths = []
        self.display_centers = []
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        self.current_scene = []
        self.current_connectors = []
        self.departing_scene = []
        self.departing_connectors = []
        self.visual_positions = {}
        self.closing = False
        self.action_closing = False
        self.action_animation_started = None
        self.action_progress = 1.0
        self.action_item_index = None
        self.action_start_position = None
        self.action_target_position = None
        self.window.set_cursor_from_name("default")
        self.canvas.set_cursor_from_name("default")
        Gtk4LayerShell.set_keyboard_mode(
            self.window, Gtk4LayerShell.KeyboardMode.EXCLUSIVE
        )
        self.set_click_through(False)
        self.window.set_visible(True)
        self.set_click_through(False)
        self.canvas.queue_draw()

    def hide_menu(self):
        if self.closing or self.action_closing or not self.window.get_visible():
            return
        Gtk4LayerShell.set_keyboard_mode(self.window, Gtk4LayerShell.KeyboardMode.NONE)
        self.set_click_through(True)
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        self.scale_animations.clear()
        self.distance_animations.clear()
        # A submenu placed in the departing scene is already logically closed.
        # Do not restart its disappearance as part of the whole-menu animation.
        self.departing_scene = []
        self.departing_connectors = []
        self.transition_progress = 1.0
        duration = animation_duration(self.styles, "menu-duration")
        if duration == 0:
            self.finish_hide()
            return
        self.closing = True
        self.menu_start_centers = list(self.display_centers)
        self.menu_animation_from = self.menu_progress
        self.menu_animation_to = 0.0
        self.menu_animation_started = GLib.get_monotonic_time() / 1_000_000
        self.ensure_animation_tick()
        self.canvas.queue_draw()

    def finish_hide(self, from_animation=False):
        self.closing = False
        self.action_closing = False
        self.action_animation_started = None
        self.action_progress = 1.0
        self.action_item_index = None
        self.action_start_position = None
        self.action_target_position = None
        self.path.clear()
        self.menu_centers = []
        self.menu_link_lengths = []
        self.display_centers = []
        self.hits = []
        self.hovered_hit = None
        self.pointer_position = None
        self.menu_progress = 1.0
        self.menu_animation_started = None
        self.menu_animation_from = 1.0
        self.menu_animation_to = 1.0
        self.transition_progress = 1.0
        self.current_scene = []
        self.current_connectors = []
        self.departing_scene = []
        self.departing_connectors = []
        self.visual_positions = {}
        self.scale_values.clear()
        self.scale_animations.clear()
        self.distance_values.clear()
        self.distance_animations.clear()
        if self.animation_tick is not None and not from_animation:
            self.canvas.remove_tick_callback(self.animation_tick)
        self.animation_tick = None
        Gtk4LayerShell.set_keyboard_mode(self.window, Gtk4LayerShell.KeyboardMode.NONE)
        self.window.set_visible(False)

    def set_click_through(self, enabled):
        surface = self.window.get_surface()
        if surface is not None:
            surface.set_input_region(cairo.Region() if enabled else None)

    def on_key_pressed(self, _controller, keyval, _keycode, _state):
        if keyval == Gdk.KEY_Escape:
            self.hide_menu()
            return True
        return False

    def on_pointer_event(self, _controller, x, y):
        if not self.window.get_visible() or self.closing or self.action_closing:
            return
        self.pointer_position = (x, y)
        if not self.menu_centers:
            target = self.clamp_menu_position(x, y)
            self.menu_centers = [target]
            self.display_centers = [(x, y)]
            if target != (x, y):
                self.start_menu_animation(reveal_items=False)
            self.canvas.queue_draw()
            return
        hovered_hit = self.target_at(x, y)
        if hovered_hit != self.hovered_hit:
            self.hovered_hit = hovered_hit
        self.canvas.queue_draw()

    def draw(self, _canvas, context, width, height):
        self.hits = []

        overlay = computed_style(self.styles, ("overlay",))
        context.set_source_rgba(*overlay["background-color"])
        context.paint()

        if (
            not self.menu_centers
            and self.settings.center_mode
            and width > 0
            and height > 0
        ):
            center = self.clamp_menu_position(width / 2, height / 2)
            self.menu_centers = [center]
            self.display_centers = [center]

        if not self.menu_centers:
            return

        self.draw_connectors(context)
        self.draw_departing(context)
        scene = []
        self.visual_positions = {}

        center_x, center_y = self.display_centers[-1]
        target_center_x, target_center_y = self.menu_centers[-1]
        current = self.item_at_path(self.path)
        if (
            self.hovered_hit is not None
            and self.hovered_hit[0] == "item"
            and not 0 <= self.hovered_hit[1] < len(current.items)
        ):
            self.hovered_hit = None
        pointer_angle = None
        if (
            self.pointer_position is not None
            and self.hovered_hit is not None
            and self.hovered_hit[0] == "item"
        ):
            pointer_x, pointer_y = self.pointer_position
            if math.hypot(pointer_x - center_x, pointer_y - center_y) > 1e-6:
                pointer_angle = direction_angle(
                    pointer_x - center_x,
                    pointer_y - center_y,
                )
        center_label = None
        if (
            self.settings.active_label_in_center
            and self.hovered_hit is not None
            and self.hovered_hit[0] == "item"
        ):
            center_label = current.items[self.hovered_hit[1]].label
        elif (
            self.settings.active_label_in_center
            and self.hovered_hit is not None
            and self.hovered_hit[0] == "parent"
            and self.path
        ):
            center_label = self.item_at_path(self.path[:-1]).label
        style = self.item_style(
            current, center=True, active=self.hovered_hit == ("center", None)
        )
        size = self.animated_item_size(
            current,
            style,
            ("center", None),
            self.item_style(current, center=True)["scale"],
        )
        reveal = self.menu_progress
        closing_reveal = reveal if self.closing else 1.0
        self.draw_item(
            context,
            center_x,
            center_y,
            size * closing_reveal,
            current,
            style,
            closing_reveal,
            active=(
                self.hovered_hit == ("center", None)
                and not self.settings.active_label_in_center
            ),
            label_override=center_label,
            hide_label=(self.settings.active_label_in_center and center_label is None),
        )
        scene.append(
            (
                center_x,
                center_y,
                size * closing_reveal,
                current,
                style,
                closing_reveal,
                False,
            )
        )
        hitbox_size = (
            size
            if self.settings.center_hitbox_size is None
            else self.settings.center_hitbox_size
        )
        self.hits.append(
            (
                target_center_x,
                target_center_y,
                hitbox_size,
                "center",
                None,
                None,
            )
        )

        for depth in range(len(self.path)):
            history_item = self.item_at_path(self.path[:depth])
            is_parent = depth == len(self.path) - 1
            parent_hovered = is_parent and self.hovered_hit == ("parent", depth)
            style = self.item_style(
                history_item,
                history=True,
                active=parent_hovered,
            )
            history_size = (
                self.animated_item_size(
                    history_item,
                    style,
                    ("parent", depth),
                    self.item_style(history_item, history=True)["scale"],
                )
                if is_parent
                else self.item_size(history_item, style)
            )
            history_x, history_y = self.display_centers[depth]
            self.draw_item(
                context,
                history_x,
                history_y,
                history_size * closing_reveal,
                history_item,
                style,
                closing_reveal,
                active=(parent_hovered and not self.settings.active_label_in_center),
                hide_label=(parent_hovered and self.settings.active_label_in_center),
            )
            if is_parent:
                self.hits.append(
                    (
                        history_x,
                        history_y,
                        history_size,
                        "parent",
                        depth,
                        current.return_angle,
                    )
                )

        for index, item in enumerate(current.items):
            item_active = (
                self.hovered_hit
                == (
                    "item",
                    index,
                )
                or index == self.action_item_index
            )
            style = self.item_style(item, active=item_active)
            resting_distance = self.settings.menu_radius
            active_style = self.item_style(item, active=True)
            distance_offset = active_style["distance"] or 0
            if item_active:
                distance_factor = 1.0
            elif pointer_angle is not None:
                angle_difference = angular_distance(pointer_angle, item.angle)
                angular_falloff = (1 + math.cos(math.radians(angle_difference))) / 2
                distance_factor = active_style["follow-distance"] * angular_falloff
            else:
                distance_factor = 0.0
            target_distance = max(
                0,
                self.settings.menu_radius + distance_offset * distance_factor,
            )
            distance = self.animated_item_distance(
                target_distance,
                ("item", index),
                resting_distance,
            )
            x, y = self.radial_position(
                (center_x, center_y),
                item.angle,
                distance,
            )
            hit_x, hit_y = self.radial_position(
                (target_center_x, target_center_y),
                item.angle,
                target_distance,
            )
            x = center_x + (x - center_x) * reveal
            y = center_y + (y - center_y) * reveal
            if (
                index == self.action_item_index
                and self.action_start_position is not None
                and self.action_target_position is not None
            ):
                x = (
                    self.action_start_position[0]
                    + (self.action_target_position[0] - self.action_start_position[0])
                    * self.action_progress
                )
                y = (
                    self.action_start_position[1]
                    + (self.action_target_position[1] - self.action_start_position[1])
                    * self.action_progress
                )
            size = self.animated_item_size(
                item,
                style,
                ("item", index),
                self.item_style(item)["scale"],
            )
            self.draw_item(
                context,
                x,
                y,
                size * reveal,
                item,
                style,
                reveal,
                active=(item_active and not self.settings.active_label_in_center),
                hide_label=(
                    item_active
                    and self.settings.active_label_in_center
                    and bool(item.icon)
                ),
                submenu_indicators=bool(item.items),
                submenu_indicators_active=item_active,
            )
            scene.append((x, y, size * reveal, item, style, reveal, bool(item.items)))
            self.visual_positions[("item", index)] = (x, y)
            self.hits.append((hit_x, hit_y, size, "item", index, item.angle))

        if self.pointer_position is not None:
            hovered_hit = self.target_at(*self.pointer_position)
            if hovered_hit != self.hovered_hit:
                self.hovered_hit = hovered_hit
                self.canvas.queue_draw()
        if self.closing or self.action_closing:
            self.hits = []
        self.current_scene = scene

    def draw_departing(self, context):
        remaining = 1 - self.transition_progress
        if remaining <= 0:
            return
        for start, end, style in self.departing_connectors:
            set_source_color(
                context,
                style["color"],
                style["opacity"] * remaining,
            )
            context.set_line_width(style["width"])
            context.move_to(*start)
            context.line_to(
                start[0] + (end[0] - start[0]) * remaining,
                start[1] + (end[1] - start[1]) * remaining,
            )
            context.stroke()
        for (
            x,
            y,
            size,
            item,
            style,
            opacity,
            submenu_indicators,
        ) in self.departing_scene:
            self.draw_item(
                context,
                x,
                y,
                size * remaining,
                item,
                style,
                opacity * remaining,
                submenu_indicators=submenu_indicators,
            )

    def draw_connectors(self, context):
        self.current_connectors = []
        style = computed_style(self.styles, ("connector",))
        if style["width"] is None or style["width"] == 0:
            return
        nodes = []
        for depth in range(len(self.path)):
            item = self.item_at_path(self.path[:depth])
            is_parent = depth == len(self.path) - 1
            item_style = self.item_style(
                item,
                history=True,
                active=is_parent and self.hovered_hit == ("parent", depth),
            )
            nodes.append(
                (
                    self.display_centers[depth],
                    (
                        self.animated_inner_radius(
                            item,
                            item_style,
                            ("parent", depth),
                        )
                        if is_parent
                        else self.circle_inner_radius(item, item_style)
                    ),
                )
            )
        item = self.item_at_path(self.path)
        item_style = self.item_style(
            item,
            center=True,
            active=self.hovered_hit == ("center", None),
        )
        nodes.append(
            (
                self.display_centers[-1],
                self.animated_inner_radius(
                    item,
                    item_style,
                    ("center", None),
                ),
            )
        )
        set_source_color(
            context,
            style["color"],
            style["opacity"] * (self.menu_progress if self.closing else 1.0),
        )
        context.set_line_width(style["width"])
        for (start, start_radius), (end, end_radius) in pairwise(nodes):
            delta_x = end[0] - start[0]
            delta_y = end[1] - start[1]
            length = math.hypot(delta_x, delta_y)
            if length <= start_radius + end_radius:
                continue
            unit_x = delta_x / length
            unit_y = delta_y / length
            context.move_to(
                start[0] + unit_x * start_radius,
                start[1] + unit_y * start_radius,
            )
            context.line_to(
                end[0] - unit_x * end_radius,
                end[1] - unit_y * end_radius,
            )
            self.current_connectors.append(
                (
                    (
                        start[0] + unit_x * start_radius,
                        start[1] + unit_y * start_radius,
                    ),
                    (
                        end[0] - unit_x * end_radius,
                        end[1] - unit_y * end_radius,
                    ),
                    style,
                )
            )
        context.stroke()

        if (
            self.action_item_index is None
            or self.action_start_position is None
            or self.action_target_position is None
        ):
            return
        action_item = self.item_at_path(self.path).items[self.action_item_index]
        action_style = self.item_style(action_item, active=True)
        start, start_radius = nodes[-1]
        end = (
            self.action_start_position[0]
            + (self.action_target_position[0] - self.action_start_position[0])
            * self.action_progress,
            self.action_start_position[1]
            + (self.action_target_position[1] - self.action_start_position[1])
            * self.action_progress,
        )
        end_radius = self.animated_inner_radius(
            action_item,
            action_style,
            ("item", self.action_item_index),
        )
        delta_x = end[0] - start[0]
        delta_y = end[1] - start[1]
        length = math.hypot(delta_x, delta_y)
        if length <= start_radius + end_radius:
            return
        unit_x = delta_x / length
        unit_y = delta_y / length
        context.move_to(
            start[0] + unit_x * start_radius,
            start[1] + unit_y * start_radius,
        )
        context.line_to(
            end[0] - unit_x * end_radius,
            end[1] - unit_y * end_radius,
        )
        context.stroke()

    def circle_inner_radius(self, item, style):
        return max(
            0,
            self.item_size(item, style) / 2 - style["border-width"] / 2,
        )

    def animated_inner_radius(self, item, style, key):
        scale = self.scale_values.get(key, style["scale"])
        size = style["width"]
        return max(0, size * scale / 2 - style["border-width"] / 2)

    def item_style(self, item, center=False, history=False, active=False):
        selectors = ["circle"]
        if center:
            role_selectors = ["circle.center"]
        elif history:
            role_selectors = ["circle.history"]
        else:
            role_selectors = ["circle.item"]
            if item.items:
                role_selectors.append("circle.submenu")
        selectors.extend(role_selectors)
        if active:
            selectors.append("circle.active")
            selectors.extend(f"{selector}.active" for selector in role_selectors)
        return computed_style(self.styles, selectors)

    def item_size(self, item, style):
        return style["width"] * style["scale"]

    def animated_item_size(self, item, style, key, resting_scale):
        target = style["scale"]
        current = self.scale_values.setdefault(key, resting_scale)
        duration = animation_duration(self.styles, "hover-duration")
        animation = self.scale_animations.get(key)
        if duration == 0:
            self.scale_values[key] = target
            self.scale_animations.pop(key, None)
            current = target
        elif abs(current - target) > 1e-6 and (
            animation is None or animation[1] != target
        ):
            started = GLib.get_monotonic_time() / 1_000_000
            self.scale_animations[key] = (current, target, started, duration)
            self.ensure_animation_tick()
        base_size = style["width"]
        return base_size * current

    def animated_item_distance(self, target, key, resting_distance):
        current = self.distance_values.setdefault(key, resting_distance)
        duration = animation_duration(self.styles, "hover-duration")
        animation = self.distance_animations.get(key)
        if duration == 0:
            self.distance_values[key] = target
            self.distance_animations.pop(key, None)
            current = target
        elif abs(current - target) > 1e-6 and (
            animation is None or animation[1] != target
        ):
            started = GLib.get_monotonic_time() / 1_000_000
            self.distance_animations[key] = (current, target, started, duration)
            self.ensure_animation_tick()
        return current

    def radial_position(self, center, angle, distance):
        radians = math.radians(angle)
        return (
            center[0] + distance * math.sin(radians),
            center[1] - distance * math.cos(radians),
        )

    def draw_item(
        self,
        context,
        x,
        y,
        size,
        item,
        style,
        opacity=1.0,
        active=False,
        label_override=None,
        hide_label=False,
        submenu_indicators=False,
        submenu_indicators_active=False,
    ):
        if size <= 0:
            return
        if submenu_indicators:
            self.draw_submenu_indicators(
                context,
                x,
                y,
                size,
                item,
                style,
                opacity,
                submenu_indicators_active,
            )
        radius = resolve_radius(style["border-radius"], size)
        left = x - size / 2
        top = y - size / 2

        rounded_rectangle(context, left, top, size, size, radius)
        set_source_color(
            context,
            style["background-color"],
            style["opacity"] * opacity,
        )
        context.fill_preserve()
        if style["border-width"] > 0:
            set_source_color(
                context,
                style["border-color"],
                style["opacity"] * opacity,
            )
            context.set_line_width(style["border-width"])
            context.stroke()
        else:
            context.new_path()

        if (
            item.icon
            and label_override is None
            and not active
            and self.draw_icon(context, x, y, size, item, style, opacity)
        ):
            return
        label_text = item.label if label_override is None else label_override
        if hide_label or not label_text:
            return
        context.select_font_face(
            style["font-family"], cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_NORMAL
        )
        context.set_font_size(style["font-size"])
        set_source_color(context, style["color"], content_opacity(style) * opacity)
        label = truncate(label_text, max(1, int(size / style["font-size"] * 1.5)))
        extents = context.text_extents(label)
        context.move_to(
            x - extents.width / 2 - extents.x_bearing,
            y - extents.height / 2 - extents.y_bearing,
        )
        context.show_text(label)

    def draw_submenu_indicators(
        self,
        context,
        x,
        y,
        circle_size,
        item,
        submenu_style,
        opacity,
        active=False,
    ):
        selectors = ["submenu-indicator"]
        if active:
            selectors.append("submenu-indicator.active")
        style = computed_style(self.styles, selectors)
        indicator_size = style["width"]
        protrusion = style["protrusion"]
        if (
            not item.items
            or indicator_size is None
            or indicator_size <= 0
            or protrusion <= 0
        ):
            return
        orbit = max(0, circle_size / 2 - indicator_size / 2 + protrusion)
        context.save()
        if style["cut-indicators"]:
            clip_x1, clip_y1, clip_x2, clip_y2 = context.clip_extents()
            context.rectangle(
                clip_x1,
                clip_y1,
                clip_x2 - clip_x1,
                clip_y2 - clip_y1,
            )
            radius = resolve_radius(submenu_style["border-radius"], circle_size)
            rounded_rectangle(
                context,
                x - circle_size / 2,
                y - circle_size / 2,
                circle_size,
                circle_size,
                radius,
            )
            context.set_fill_rule(cairo.FILL_RULE_EVEN_ODD)
            context.clip()
        for child in item.items:
            indicator_x, indicator_y = self.radial_position(
                (x, y),
                child.angle,
                orbit,
            )
            context.arc(
                indicator_x,
                indicator_y,
                indicator_size / 2,
                0,
                math.tau,
            )
            set_source_color(
                context,
                style["color"],
                style["opacity"] * opacity,
            )
            context.fill()
        context.restore()

    def draw_icon(self, context, x, y, circle_size, item, style, opacity):
        path = icon_path(item.icon_theme, item.icon)
        if path is None:
            return False
        size = round(style.get("icon-size") or circle_size * 0.55)
        if size <= 0:
            return False
        color = style["color"]
        key = (str(path), path.stat().st_mtime_ns, size, color)
        pixbuf = self.icon_cache.get(key)
        if pixbuf is None:
            try:
                if path.suffix.lower() == ".svg":
                    red, green, blue, _alpha = color
                    replacement = (
                        f"#{round(red * 255):02x}{round(green * 255):02x}"
                        f"{round(blue * 255):02x}"
                    )
                    source = path.read_text(encoding="utf-8")
                    if "currentColor" in source:
                        source = source.replace("currentColor", replacement)
                    elif not re.search(
                        r"""(?:fill|stroke)\s*=\s*["'](?:#|rgb|hsl)""",
                        source,
                        re.IGNORECASE,
                    ):
                        source = source.replace(
                            "<svg",
                            f'<svg fill="{replacement}"',
                            1,
                        )
                    loader = GdkPixbuf.PixbufLoader.new_with_type("svg")
                    loader.set_size(size, size)
                    loader.write(source.encode())
                    loader.close()
                    pixbuf = loader.get_pixbuf()
                else:
                    pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                        str(path), size, size, True
                    )
            except (GLib.Error, OSError, UnicodeError):
                return False
            self.icon_cache[key] = pixbuf
        context.save()
        Gdk.cairo_set_source_pixbuf(
            context,
            pixbuf,
            x - pixbuf.get_width() / 2,
            y - pixbuf.get_height() / 2,
        )
        context.paint_with_alpha(content_opacity(style) * opacity)
        context.restore()
        return True

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
            self.departing_scene = list(self.current_scene)
            self.departing_connectors = self.current_connectors[-1:]
            self.path = self.path[:index]
            self.menu_centers = self.menu_centers[: index + 1]
            self.menu_link_lengths = self.menu_link_lengths[:index]
            self.menu_centers[-1] = self.clamp_menu_position(x, y)
            self.display_centers = self.display_centers[: index + 1]
            self.align_menu_chain()
            self.hovered_hit = None
            self.reset_item_animations()
            self.start_menu_animation(reveal_items=True)
            self.canvas.queue_draw()
            return

        item = self.item_at_path(self.path).items[index]
        if item.items:
            start_position = self.visual_positions.get(
                ("item", index),
                (x, y),
            )
            self.departing_scene = []
            self.departing_connectors = []
            parent_center = self.menu_centers[-1]
            child_center = self.clamp_menu_position(x, y)
            link_length = max(
                math.hypot(
                    child_center[0] - parent_center[0],
                    child_center[1] - parent_center[1],
                ),
                self.settings.menu_radius,
            )
            self.path.append(index)
            self.menu_centers.append(child_center)
            self.menu_link_lengths.append(link_length)
            self.display_centers.append(start_position)
            self.align_menu_chain()
            self.hovered_hit = None
            self.reset_item_animations()
            self.start_menu_animation(reveal_items=True)
            self.canvas.queue_draw()
        else:
            launch(item.command)
            self.hide_after_action(index, x, y)

    def hide_after_action(self, index, x, y):
        Gtk4LayerShell.set_keyboard_mode(self.window, Gtk4LayerShell.KeyboardMode.NONE)
        self.set_click_through(True)
        self.hits = []
        self.pointer_position = None
        self.departing_scene = []
        self.departing_connectors = []
        self.transition_progress = 1.0
        self.action_item_index = index
        self.action_start_position = self.visual_positions.get(
            ("item", index),
            (x, y),
        )
        self.action_target_position = (x, y)
        self.action_progress = 0.0
        duration = animation_duration(self.styles, "action-duration")
        if duration == 0:
            self.action_progress = 1.0
            self.hide_menu()
            return
        self.action_closing = True
        self.action_animation_started = GLib.get_monotonic_time() / 1_000_000
        self.ensure_animation_tick()
        self.canvas.queue_draw()

    def clamp_menu_position(self, x, y):
        width = self.canvas.get_width()
        height = self.canvas.get_height()
        margin = self.settings.minimum_edge_distance

        def clamp(value, extent):
            if extent <= 0:
                return value
            if extent <= margin * 2:
                return extent / 2
            return min(max(value, margin), extent - margin)

        return clamp(x, width), clamp(y, height)

    def align_menu_chain(self):
        for depth in range(len(self.path), 0, -1):
            child = self.item_at_path(self.path[:depth])
            self.menu_centers[depth - 1] = self.radial_position(
                self.menu_centers[depth],
                child.return_angle,
                self.menu_link_lengths[depth - 1],
            )

    def target_at(self, x, y):
        if not self.hits:
            return None
        center_x, center_y, _center_size, _kind, _index, _angle = self.hits[0]
        for hit_x, hit_y, hit_size, kind, target, _angle in reversed(self.hits):
            if (
                kind == "center"
                and hit_size > 0
                and math.hypot(x - hit_x, y - hit_y) <= hit_size / 2
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

    def reset_item_animations(self):
        self.scale_values.clear()
        self.scale_animations.clear()
        self.distance_values.clear()
        self.distance_animations.clear()

    def start_menu_animation(self, reveal_items):
        duration = animation_duration(self.styles, "menu-duration")
        self.menu_start_centers = list(self.display_centers)
        start = 0.0 if reveal_items else 1.0
        if duration == 0:
            self.display_centers = list(self.menu_centers)
            self.menu_progress = 1.0
            self.transition_progress = 1.0
            self.menu_animation_started = None
            self.departing_scene = []
            self.departing_connectors = []
            return
        self.menu_progress = start
        self.transition_progress = 0.0
        self.menu_animation_from = start
        self.menu_animation_to = 1.0
        self.menu_animation_started = GLib.get_monotonic_time() / 1_000_000
        self.ensure_animation_tick()

    def ensure_animation_tick(self):
        if self.animation_tick is None:
            self.animation_tick = self.canvas.add_tick_callback(self.on_animation_frame)

    def on_animation_frame(self, _canvas, frame_clock):
        now = frame_clock.get_frame_time() / 1_000_000
        animating = False

        if self.menu_animation_started is not None:
            duration = animation_duration(self.styles, "menu-duration")
            progress = min(1.0, (now - self.menu_animation_started) / duration)
            eased = ease_out_cubic(progress)
            self.transition_progress = eased
            self.menu_progress = (
                self.menu_animation_from
                + (self.menu_animation_to - self.menu_animation_from) * eased
            )
            self.display_centers = [
                (
                    start[0] + (target[0] - start[0]) * eased,
                    start[1] + (target[1] - start[1]) * eased,
                )
                for start, target in zip(
                    self.menu_start_centers,
                    self.menu_centers,
                    strict=True,
                )
            ]
            if progress < 1:
                animating = True
            else:
                self.menu_animation_started = None
                self.departing_scene = []
                self.departing_connectors = []
                if self.closing:
                    self.finish_hide(from_animation=True)
                    return False

        if self.action_animation_started is not None:
            duration = animation_duration(self.styles, "action-duration")
            progress = min(1.0, (now - self.action_animation_started) / duration)
            self.action_progress = ease_out_cubic(progress)
            if progress < 1:
                animating = True
            else:
                self.action_progress = 1.0
                self.action_animation_started = None
                self.action_closing = False
                self.hide_menu()
                animating = True

        for key, (start, target, started, duration) in list(
            self.scale_animations.items()
        ):
            progress = min(1.0, (now - started) / duration)
            self.scale_values[key] = start + (target - start) * ease_out_cubic(progress)
            if progress < 1:
                animating = True
            else:
                self.scale_values[key] = target
                del self.scale_animations[key]

        for key, (start, target, started, duration) in list(
            self.distance_animations.items()
        ):
            progress = min(1.0, (now - started) / duration)
            self.distance_values[key] = start + (target - start) * ease_out_cubic(
                progress
            )
            if progress < 1:
                animating = True
            else:
                self.distance_values[key] = target
                del self.distance_animations[key]

        self.canvas.queue_draw()
        if not animating:
            self.animation_tick = None
        return animating

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
    if sys.argv[1:] == ["--configure"]:
        configurator_main()
        return
    start_visible = sys.argv[1:] == ["--show"]
    try:
        exit_code = Waypie(load_config(), load_styles(), start_visible).run(
            [sys.argv[0]]
        )
    except KeyboardInterrupt:
        exit_code = 0
    raise SystemExit(exit_code)


def configurator_main():
    from waypie_config import main as config_main

    config_main()


if __name__ == "__main__":
    main()

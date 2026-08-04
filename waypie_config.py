import copy
import json
import math
import shutil
import sys

import cairo
import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
gi.require_version("GdkPixbuf", "2.0")

from gi.repository import Gdk, GdkPixbuf, GLib, Gtk

from waypie_common import (
    CONFIG_DIR,
    CONFIG_PATH,
    ICON_DIR,
    Item,
    angular_delta,
    angular_distance,
    animation_duration,
    colored_svg_source,
    computed_style,
    content_opacity,
    direction_angle,
    draw_wrapped_text,
    fixed_text_geometry,
    icon_path,
    icon_themes,
    largest_gap_angle,
    load_config,
    load_icon_theme_history,
    load_styles,
    remember_icon_theme,
    resolve_angles,
    resolve_radius,
    rounded_rectangle,
    scaled_icon_size,
    set_source_color,
    sort_icon_themes,
    spring_duration,
    spring_value,
    theme_icons,
)

ALIGNMENT_ANGLES = tuple(range(0, 360, 5))
DEFAULT_PARENT_LINK = {
    "color": "#ff8c00",
    "opacity": "0.35",
    "width": "12px",
}
DEFAULT_HISTORY = {"opacity": "0.35", "scale": "2"}
ALL_ICON_THEMES = "__waypie_all_icon_themes__"
INDICATOR_CLIP_OVERLAP = 1.0


class Configurator(Gtk.Application):
    def __init__(self, settings, styles):
        super().__init__(application_id="waypie.config")
        self.settings = settings
        self.styles = styles
        self.window = None
        self.canvas = None
        self.tree = None
        self.status = None
        self.label_entry = None
        self.command_entry = None
        self.angle_spin = None
        self.icon_button = None
        self.preserve_check = None
        self.alignment_check = None
        self.show_icons_check = None
        self.center_mode_check = None
        self.close_submenu_on_center_click_check = None
        self.hover_mode_check = None
        self.turbo_mode_check = None
        self.setting_spins = {}
        self.selected_path = ()
        self.current_path = ()
        self.row_paths = {}
        self.path_rows = {}
        self.drag_index = None
        self.drag_origin = (0.0, 0.0)
        self.drag_start_angle = 0.0
        self.drag_initial_angles = []
        self.drag_reorder_armed = False
        self.drag_item = None
        self.drag_original_index = None
        self.drag_group_base = 0
        self.drag_active = False
        self.drag_happened = False
        self.click_origin = (0.0, 0.0)
        self.updating_fields = False
        self.preview_animations = {}
        self.preview_color_animations = {}
        self.preview_departures = []
        self.preview_tick = None
        self.preview_hover_target = None
        self.icon_cache = {}
        self.rebuilding_tree = False
        self.restoring_undo = False
        self.undo_history = []
        self.drag_undo_recorded = False

    def do_activate(self):
        if self.window is not None:
            self.window.present()
            return

        self.window = Gtk.ApplicationWindow(application=self)
        self.window.set_title("Waypie Configurator")
        self.window.set_default_size(1100, 700)

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        root.set_margin_top(8)
        root.set_margin_bottom(8)
        root.set_margin_start(8)
        root.set_margin_end(8)

        toolbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        for label, shortcut, callback in (
            ("Add command", "Ctrl+Q", self.on_add_command),
            ("Add submenu", "Ctrl+X", self.on_add_submenu),
            ("Delete", "Ctrl+D", self.on_delete),
            ("Center layout", "Ctrl+A", self.on_center_layout),
            ("Save", "Ctrl+S", self.on_save),
        ):
            button = Gtk.Button()
            button_content = Gtk.Box(
                orientation=Gtk.Orientation.VERTICAL,
                spacing=1,
            )
            button_content.append(Gtk.Label(label=label))
            button_content.append(Gtk.Label(label=shortcut))
            button.set_child(button_content)
            button.connect("clicked", callback)
            toolbar.append(button)
        self.preserve_check = Gtk.CheckButton(label="Preserve proportions")
        self.preserve_check.set_active(self.settings.preserve_proportions)
        self.preserve_check.connect("toggled", self.on_layout_option_changed)
        toolbar.append(self.preserve_check)
        self.alignment_check = Gtk.CheckButton(label="Auto alignment")
        self.alignment_check.set_active(self.settings.auto_alignment)
        self.alignment_check.connect("toggled", self.on_layout_option_changed)
        toolbar.append(self.alignment_check)
        self.show_icons_check = Gtk.CheckButton(label="Show icons")
        self.show_icons_check.set_active(self.settings.configurator_show_icons)
        self.show_icons_check.connect("toggled", self.on_show_icons_changed)
        toolbar.append(self.show_icons_check)
        self.status = Gtk.Label(xalign=0)
        self.status.set_text("Saved")
        self.status.set_width_chars(8)
        self.status.set_size_request(70, -1)
        toolbar.append(self.status)
        root.append(toolbar)

        content = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        content.set_vexpand(True)

        self.tree = Gtk.ListBox()
        self.tree.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.tree.connect("row-selected", self.on_tree_selected)
        self.tree.connect("row-activated", self.on_tree_activated)
        tree_scroll = Gtk.ScrolledWindow()
        tree_scroll.set_size_request(260, -1)
        tree_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        tree_scroll.set_child(self.tree)
        content.append(tree_scroll)

        self.canvas = Gtk.DrawingArea()
        self.canvas.set_hexpand(True)
        self.canvas.set_vexpand(True)
        self.canvas.set_draw_func(self.draw_preview)
        click = Gtk.GestureClick()
        click.set_button(1)
        click.connect("pressed", self.on_preview_press)
        click.connect("released", self.on_preview_click)
        self.canvas.add_controller(click)
        drag = Gtk.GestureDrag()
        drag.set_button(1)
        drag.connect("drag-begin", self.on_drag_begin)
        drag.connect("drag-update", self.on_drag_update)
        drag.connect("drag-end", self.on_drag_end)
        self.canvas.add_controller(drag)
        click.group(drag)
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", self.on_preview_motion)
        motion.connect("leave", self.on_preview_leave)
        self.canvas.add_controller(motion)
        content.append(self.canvas)

        properties = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        properties.set_size_request(280, -1)
        item_title = Gtk.Label(label="Selected item", xalign=0)
        item_title.add_css_class("heading")
        properties.append(item_title)
        self.label_entry = self.add_property(properties, "Label", Gtk.Entry())
        self.command_entry = self.add_property(properties, "Command", Gtk.Entry())
        self.angle_spin = self.add_property(
            properties,
            "Angle (degrees)",
            Gtk.SpinButton.new_with_range(0, 359, 1),
        )
        self.angle_spin.set_digits(0)
        self.icon_button = Gtk.Button(label="Choose icon…")
        self.icon_button.connect("clicked", self.on_choose_icon)
        self.add_property(properties, "Icon", self.icon_button)
        hint = Gtk.Label(
            label=(
                "Drag a command to change its angle.\n"
                "Click a submenu to edit its children."
            ),
            xalign=0,
            wrap=True,
        )
        properties.append(hint)
        separator = Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL)
        properties.append(separator)
        settings_title = Gtk.Label(label="Config settings", xalign=0)
        settings_title.add_css_class("heading")
        properties.append(settings_title)
        for name, label, lower, upper, value in (
            (
                "menu_radius",
                "menu-radius",
                1,
                8192,
                self.settings.menu_radius,
            ),
            (
                "center_hitbox_size",
                "center-hitbox-size",
                0,
                4096,
                (
                    self.settings.center_hitbox_size
                    if self.settings.center_hitbox_size is not None
                    else computed_style(self.styles, ("circle",))["width"]
                ),
            ),
            (
                "minimum_edge_distance",
                "minimum-edge-distance",
                0,
                8192,
                self.settings.minimum_edge_distance,
            ),
        ):
            spin = Gtk.SpinButton.new_with_range(lower, upper, 1)
            spin.set_digits(0)
            spin.set_value(value)
            spin.connect("value-changed", self.on_config_setting_changed, name)
            self.setting_spins[name] = spin
            self.add_property(properties, label, spin)
        self.center_mode_check = Gtk.CheckButton(label="Center mode")
        self.center_mode_check.set_active(self.settings.center_mode)
        self.center_mode_check.connect("toggled", self.on_center_mode_changed)
        properties.append(self.center_mode_check)
        self.close_submenu_on_center_click_check = Gtk.CheckButton(
            label="Close on click"
        )
        self.close_submenu_on_center_click_check.set_active(
            self.settings.close_submenu_on_center_click
        )
        self.close_submenu_on_center_click_check.connect(
            "toggled",
            self.on_close_submenu_on_center_click_changed,
        )
        properties.append(self.close_submenu_on_center_click_check)
        self.hover_mode_check = Gtk.CheckButton(label="Hover mode")
        self.hover_mode_check.set_active(self.settings.hover_mode)
        self.hover_mode_check.set_tooltip_text(
            "Select an item by pausing over it or turning the pointer."
        )
        self.hover_mode_check.connect("toggled", self.on_hover_mode_changed)
        properties.append(self.hover_mode_check)
        self.turbo_mode_check = Gtk.CheckButton(label="Turbo mode")
        self.turbo_mode_check.set_active(self.settings.turbo_mode)
        self.turbo_mode_check.set_tooltip_text(
            "Hold Super, Alt, Ctrl, or Shift while opening the menu; "
            "release it to activate the selected item."
        )
        self.turbo_mode_check.connect("toggled", self.on_turbo_mode_changed)
        properties.append(self.turbo_mode_check)
        properties_scroll = Gtk.ScrolledWindow()
        properties_scroll.set_policy(
            Gtk.PolicyType.NEVER,
            Gtk.PolicyType.AUTOMATIC,
        )
        properties_scroll.set_child(properties)
        content.append(properties_scroll)
        root.append(content)

        self.label_entry.connect("changed", self.on_property_changed)
        self.command_entry.connect("changed", self.on_property_changed)
        self.angle_spin.connect("value-changed", self.on_property_changed)

        self.window.set_child(root)
        shortcuts = Gtk.EventControllerKey()
        shortcuts.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
        shortcuts.connect("key-pressed", self.on_shortcut_pressed)
        self.window.add_controller(shortcuts)
        focus_click = Gtk.GestureClick()
        focus_click.set_button(0)
        focus_click.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
        focus_click.connect("pressed", self.on_window_pressed)
        self.window.add_controller(focus_click)
        self.rebuild_tree()
        self.sync_fields()
        self.window.present()

    @staticmethod
    def add_property(container, label, widget):
        container.append(Gtk.Label(label=label, xalign=0))
        container.append(widget)
        return widget

    def item_at(self, path):
        item = self.settings.root
        for index in path:
            item = item.items[index]
        return item

    def rebuild_tree(self):
        while row := self.tree.get_row_at_index(0):
            self.tree.remove(row)
        self.row_paths.clear()
        self.path_rows.clear()

        def add(item, path, depth):
            suffix = " ▸" if item.is_submenu else ""
            row = Gtk.ListBoxRow()
            label = Gtk.Label(label=f"{'  ' * depth}{item.label}{suffix}", xalign=0)
            label.set_margin_top(4)
            label.set_margin_bottom(4)
            row.set_child(label)
            self.tree.append(row)
            self.row_paths[row] = path
            self.path_rows[path] = row
            for index, child in enumerate(item.items):
                add(child, (*path, index), depth + 1)

        add(self.settings.root, (), 0)
        row = self.path_rows.get(self.selected_path)
        if row is not None:
            self.rebuilding_tree = True
            try:
                self.tree.select_row(row)
            finally:
                self.rebuilding_tree = False

    def on_tree_selected(self, _tree, row):
        if row is None or self.rebuilding_tree:
            return
        self.selected_path = self.row_paths[row]
        if self.selected_path:
            self.current_path = self.selected_path[:-1]
        else:
            self.current_path = ()
        self.sync_fields()
        self.canvas.queue_draw()

    def on_tree_activated(self, _tree, row):
        path = self.row_paths[row]
        if self.item_at(path).is_submenu:
            self.open_submenu(path)

    def sync_fields(self):
        item = self.item_at(self.selected_path)
        is_root = not self.selected_path
        self.updating_fields = True
        self.label_entry.set_text(item.label)
        self.command_entry.set_text(item.command or "")
        self.command_entry.set_sensitive(not is_root and not item.is_submenu)
        self.angle_spin.set_value(item.angle or 0)
        self.angle_spin.set_sensitive(not is_root)
        self.icon_button.set_label(
            f"{item.icon_theme}: {item.icon}" if item.icon else "Choose icon…"
        )
        self.updating_fields = False

    def on_property_changed(self, _widget):
        if self.updating_fields:
            return
        self.push_undo()
        item = self.item_at(self.selected_path)
        item.label = self.label_entry.get_text()
        if self.selected_path and not item.is_submenu:
            item.command = self.command_entry.get_text()
        if self.selected_path:
            requested_angle = round(self.angle_spin.get_value()) % 360
            if self.preserve_check.get_active():
                menu = self.item_at(self.selected_path[:-1])
                initial = [child.angle for child in menu.items]
                delta = angular_delta(requested_angle, item.angle)
                self.rotate_group(menu, initial, delta)
            else:
                self.set_item_angle(item, self.aligned_angle(requested_angle))
        row = self.path_rows.get(self.selected_path)
        if row is not None:
            suffix = " ▸" if item.is_submenu else ""
            row.get_child().set_text(
                f"{'  ' * len(self.selected_path)}{item.label}{suffix}"
            )
        self.set_status("Unsaved changes")
        self.canvas.queue_draw()

    def on_config_setting_changed(self, spin, name):
        if self.restoring_undo:
            return
        self.push_undo()
        value = round(spin.get_value())
        setattr(self.settings, name, value)
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def on_add_command(self, _button):
        self.push_undo()
        menu = self.item_at(self.current_path)
        angle = largest_gap_angle([item.angle % 360 for item in menu.items])
        if angle is None:
            angle = 0
        angle = round(angle) % 360
        menu.items.append(Item("New command", command="true", angle=angle))
        self.selected_path = (*self.current_path, len(menu.items) - 1)
        self.after_structure_change()

    def on_add_submenu(self, _button):
        self.push_undo()
        menu = self.item_at(self.current_path)
        angle = largest_gap_angle([item.angle % 360 for item in menu.items])
        if angle is None:
            angle = 0
        angle = round(angle) % 360
        submenu = Item(
            "New menu",
            angle=angle,
            items=[Item("New command", command="true", angle=0)],
        )
        menu.items.append(submenu)
        self.selected_path = (*self.current_path, len(menu.items) - 1)
        self.after_structure_change()

    def on_delete(self, _button):
        if not self.selected_path:
            self.set_status("The root menu cannot be deleted", error=True)
            return
        self.push_undo()
        parent_path = self.selected_path[:-1]
        parent = self.item_at(parent_path)
        self.capture_preview_departures([self.item_at(self.selected_path)])
        del parent.items[self.selected_path[-1]]
        self.selected_path = parent_path
        self.current_path = parent_path
        self.after_structure_change()

    def open_submenu(self, path):
        item = self.item_at(path)
        if not item.is_submenu:
            self.set_status("The selected item is not a submenu", error=True)
            return
        self.preview_animations.pop(("item", id(item)), None)
        self.selected_path = path
        self.current_path = path
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status(f"Editing: {item.label}")

    def on_up(self, _button):
        if not self.current_path:
            return
        self.capture_preview_departures(self.item_at(self.current_path).items)
        self.selected_path = self.current_path
        self.current_path = self.current_path[:-1]
        self.rebuild_tree()
        self.sync_fields()
        self.canvas.queue_draw()

    def on_shortcut_pressed(self, _controller, keyval, _keycode, state):
        focus = self.window.get_focus()
        if self.is_editable_widget(focus):
            return False
        control = bool(state & Gdk.ModifierType.CONTROL_MASK)
        keyval = Gdk.keyval_to_lower(keyval)
        if control and keyval == Gdk.KEY_d:
            self.on_delete(None)
        elif control and keyval == Gdk.KEY_q:
            self.on_add_command(None)
        elif control and keyval == Gdk.KEY_x:
            self.on_add_submenu(None)
        elif control and keyval == Gdk.KEY_s:
            self.on_save(None)
        elif control and keyval == Gdk.KEY_a:
            self.on_center_layout(None)
        elif control and keyval == Gdk.KEY_z:
            self.undo()
        else:
            return False
        return True

    @staticmethod
    def is_editable_widget(widget):
        while widget is not None:
            if isinstance(
                widget,
                (
                    Gtk.Entry,
                    Gtk.SearchEntry,
                    Gtk.SpinButton,
                    Gtk.Text,
                    Gtk.TextView,
                ),
            ):
                return True
            widget = widget.get_parent()
        return False

    def on_window_pressed(self, _gesture, _presses, x, y):
        target = self.window.pick(x, y, Gtk.PickFlags.DEFAULT)
        if not self.is_editable_widget(target):
            self.window.set_focus(None)

    def push_undo(self):
        if self.restoring_undo:
            return
        self.undo_history.append(
            (
                copy.deepcopy(self.settings),
                self.selected_path,
                self.current_path,
            )
        )
        if len(self.undo_history) > 100:
            del self.undo_history[0]

    def undo(self):
        if not self.undo_history:
            return
        settings, selected_path, current_path = self.undo_history.pop()
        self.restoring_undo = True
        try:
            self.settings = settings
            self.selected_path = selected_path
            self.current_path = current_path
            for name, spin in self.setting_spins.items():
                value = getattr(self.settings, name)
                if value is None:
                    value = computed_style(self.styles, ("circle",))["width"]
                spin.set_value(value)
            for check, value in (
                (self.preserve_check, self.settings.preserve_proportions),
                (self.alignment_check, self.settings.auto_alignment),
                (self.show_icons_check, self.settings.configurator_show_icons),
                (self.center_mode_check, self.settings.center_mode),
                (
                    self.close_submenu_on_center_click_check,
                    self.settings.close_submenu_on_center_click,
                ),
                (self.hover_mode_check, self.settings.hover_mode),
                (self.turbo_mode_check, self.settings.turbo_mode),
            ):
                check.set_active(value)
        finally:
            self.restoring_undo = False
        self.preview_animations.clear()
        self.preview_color_animations.clear()
        self.preview_departures.clear()
        self.rebuild_tree()
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def after_structure_change(self):
        resolve_angles(self.settings.root, root=True)
        if self.preserve_check.get_active():
            self.align_current_menu()
        self.rebuild_tree()
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def preview_item_offset(self, item):
        radians = math.radians(item.angle)
        return (
            self.settings.menu_radius * math.sin(radians),
            -self.settings.menu_radius * math.cos(radians),
        )

    def preview_history_style(self):
        rules = dict(DEFAULT_HISTORY)
        rules.update(self.styles.get("configurator-history", {}))
        return computed_style(
            {"configurator-history": rules},
            ("configurator-history",),
        )

    def preview_menu_center(self, path, previous=False):
        width = self.canvas.get_width()
        height = self.canvas.get_height()
        center = width / 2, height / 2
        if previous and self.current_path and path == self.current_path[:-1]:
            parent = self.item_at(path)
            opened_item = parent.items[self.current_path[-1]]
            offset_x, offset_y = self.preview_item_offset(opened_item)
            offset_scale = self.preview_history_style()["scale"]
            center = (
                center[0] - offset_x * offset_scale,
                center[1] - offset_y * offset_scale,
            )
        return center

    def preview_layout(self, path=None, previous=False):
        path = self.current_path if path is None else path
        menu = self.item_at(path)
        center = self.preview_menu_center(path, previous)
        circles = []
        for index, item in enumerate(menu.items):
            if (
                previous
                and self.current_path
                and path == self.current_path[:-1]
                and index == self.current_path[-1]
            ):
                continue
            if not previous and self.drag_reorder_armed and item is self.drag_item:
                continue
            selected = not previous and (
                ("item", index) == self.selected_target()
                or ("item", index) == self.preview_hover_target
            )
            style = self.preview_style(item, selected, previous=previous)
            size = self.preview_size(item, style)
            radians = math.radians(item.angle)
            x = center[0] + self.settings.menu_radius * math.sin(radians)
            y = center[1] - self.settings.menu_radius * math.cos(radians)
            circles.append((index, item, style, x, y, size))
        return menu, center, circles

    def selected_target(self):
        if self.selected_path and self.selected_path[:-1] == self.current_path:
            return "item", self.selected_path[-1]
        return None

    def preview_style(self, item, selected=False, center=False, previous=False):
        selectors = ["circle"]
        role_selectors = ["circle.center" if center else "circle.item"]
        if not center and item.is_submenu:
            role_selectors.append("circle.submenu")
        selectors.extend(role_selectors)
        if previous:
            selectors.append("circle.previous")
        if selected:
            selectors.append("circle.active")
            selectors.extend(f"{selector}.active" for selector in role_selectors)
        return computed_style(self.styles, selectors)

    def preview_size(self, item, style):
        return style["width"] * style["scale"]

    def animated_preview_geometry(
        self,
        key,
        target,
        spawn=None,
    ):
        now = GLib.get_monotonic_time() / 1_000_000
        animation = self.preview_animations.get(key)
        if animation is None:
            initial = spawn if spawn is not None else target
            spring_name = "item-create" if spawn is not None else "menu-move"
            animation = {
                "current": initial,
                "start": initial,
                "target": target,
                "started": now,
                "duration": spring_duration(self.styles, spring_name),
                "spring": spring_name,
            }
            self.preview_animations[key] = animation
        else:
            self.update_preview_animation(animation, now)
            if animation["target"] != target:
                current = animation["current"]
                position_changed = current[:2] != target[:2]
                opacity_changed = current[3] != target[3]
                spring_name = (
                    "menu-move" if position_changed or opacity_changed else "hover"
                )
                animation.update(
                    start=current,
                    target=target,
                    started=now,
                    duration=spring_duration(self.styles, spring_name),
                    spring=spring_name,
                )
        self.update_preview_animation(animation, now)
        if animation["current"] != animation["target"]:
            self.ensure_preview_tick()
        return animation["current"]

    def update_preview_animation(self, animation, now):
        duration = animation["duration"]
        if duration == 0:
            animation["current"] = animation["target"]
            return
        progress = min(1.0, (now - animation["started"]) / duration)
        eased = spring_value(self.styles, animation["spring"], progress)
        values = []
        for index, (start, target) in enumerate(
            zip(animation["start"], animation["target"], strict=True)
        ):
            value_easing = (
                math.sqrt(progress) if index == 3 and target > start else eased
            )
            values.append(start + (target - start) * value_easing)
        animation["current"] = tuple(values)

    def ensure_preview_tick(self):
        if self.preview_tick is None:
            self.preview_tick = self.canvas.add_tick_callback(
                self.on_preview_animation_frame
            )

    def animated_preview_color(self, key, target):
        now = GLib.get_monotonic_time() / 1_000_000
        duration = animation_duration(self.styles, "color-duration")
        animation = self.preview_color_animations.get(key)
        if animation is None:
            animation = {
                "current": target,
                "start": target,
                "target": target,
                "started": now,
                "duration": 0,
            }
            self.preview_color_animations[key] = animation
        self.update_preview_color(animation, now)
        if animation["target"] != target:
            animation.update(
                start=animation["current"],
                target=target,
                started=now,
                duration=duration,
            )
        self.update_preview_color(animation, now)
        if animation["current"] != animation["target"]:
            self.ensure_preview_tick()
        return animation["current"]

    @staticmethod
    def update_preview_color(animation, now):
        duration = animation["duration"]
        progress = (
            1.0 if duration == 0 else min(1.0, (now - animation["started"]) / duration)
        )
        if progress >= 1:
            animation["current"] = animation["target"]
            return
        progress = progress * progress * (3 - 2 * progress)
        animation["current"] = tuple(
            start + (target - start) * progress
            for start, target in zip(
                animation["start"], animation["target"], strict=True
            )
        )

    def capture_preview_departures(self, items):
        if self.canvas is None:
            return
        _menu, center, circles = self.preview_layout()
        circles_by_id = {
            id(item): (item, style, x, y, size)
            for _index, item, style, x, y, size in circles
        }
        now = GLib.get_monotonic_time() / 1_000_000
        duration = animation_duration(self.styles, "item-delete-duration")
        for item in items:
            node = circles_by_id.get(id(item))
            if node is None:
                continue
            _item, style, x, y, size = node
            animation = self.preview_animations.pop(("item", id(item)), None)
            if animation is not None:
                self.update_preview_animation(animation, now)
                x, y, size, opacity = animation["current"]
            else:
                opacity = 1.0
            self.preview_departures.append(
                {
                    "item": item,
                    "style": style,
                    "start": (x, y, size, opacity),
                    "target": (center[0], center[1], size * 0.75, 0.0),
                    "started": now,
                    "duration": duration,
                }
            )
        if self.preview_departures:
            self.ensure_preview_tick()

    def draw_preview_departures(self, context):
        now = GLib.get_monotonic_time() / 1_000_000
        remaining = []
        for departure in self.preview_departures:
            duration = departure["duration"]
            progress = (
                1.0
                if duration == 0
                else min(1.0, (now - departure["started"]) / duration)
            )
            eased = progress * progress * (3 - 2 * progress)
            geometry = tuple(
                start + (target - start) * eased
                for start, target in zip(
                    departure["start"],
                    departure["target"],
                    strict=True,
                )
            )
            if geometry[3] > 0:
                self.draw_preview_item(
                    context,
                    *geometry[:3],
                    departure["item"],
                    departure["style"],
                    show_icon=self.show_icons_check.get_active(),
                    opacity=geometry[3],
                    submenu_indicators=bool(departure["item"].items),
                )
            if progress < 1:
                remaining.append(departure)
        self.preview_departures = remaining

    def on_preview_animation_frame(self, _canvas, _frame_clock):
        now = GLib.get_monotonic_time() / 1_000_000
        active = False
        for animation in self.preview_animations.values():
            self.update_preview_animation(animation, now)
            if animation["current"] != animation["target"]:
                active = True
        for animation in self.preview_color_animations.values():
            self.update_preview_color(animation, now)
            if animation["current"] != animation["target"]:
                active = True
        if any(
            departure["duration"] == 0
            or now - departure["started"] < departure["duration"]
            for departure in self.preview_departures
        ):
            active = True
        self.canvas.queue_draw()
        if active:
            return True
        self.preview_tick = None
        return False

    def animate_preview_layout(
        self,
        menu,
        center,
        circles,
        opacity=1.0,
        previous=False,
    ):
        hover_target = "parent-center" if previous else "center"
        center_style = self.preview_style(
            menu,
            selected=(
                self.preview_hover_target == (hover_target, None)
                or (not previous and self.selected_path == self.current_path)
            ),
            center=True,
            previous=previous,
        )
        center_size = self.preview_size(menu, center_style)
        center_key = (
            "previous-center" if previous else "current-center",
            id(menu),
        )
        fixed_center = (
            self.canvas.get_width() / 2,
            self.canvas.get_height() / 2,
        )
        center_geometry = self.animated_preview_geometry(
            center_key,
            (*center, center_size, opacity),
            (
                *(fixed_center if previous else center),
                center_size * 0.75,
                0.0,
            ),
        )
        animated_circles = []
        for index, item, style, x, y, size in circles:
            geometry = self.animated_preview_geometry(
                ("item", id(item)),
                (x, y, size, opacity),
                (
                    center_geometry[0],
                    center_geometry[1],
                    size * 0.75,
                    0.0,
                ),
            )
            animated_circles.append((index, item, style, *geometry[:3], geometry[3]))
        return center_style, center_geometry, animated_circles

    def draw_preview(self, _canvas, context, width, height):
        overlay = computed_style(self.styles, ("overlay",))
        context.set_source_rgba(*overlay["background-color"])
        context.paint()

        parent = None
        parent_center_style = None
        parent_geometry = None
        if self.current_path:
            history_opacity = self.preview_history_style()["opacity"]
            parent_path = self.current_path[:-1]
            parent, parent_center, parent_circles = self.preview_layout(
                parent_path,
                previous=True,
            )
            (
                parent_center_style,
                parent_geometry,
                parent_circles,
            ) = self.animate_preview_layout(
                parent,
                parent_center,
                parent_circles,
                history_opacity,
                previous=True,
            )
            connector = computed_style(self.styles, ("connector",))
            if connector["width"]:
                context.set_line_width(connector["width"])
                for _index, _item, _style, x, y, _size, opacity in parent_circles:
                    set_source_color(
                        context,
                        connector["color"],
                        connector["opacity"] * min(parent_geometry[3], opacity),
                    )
                    context.move_to(*parent_geometry[:2])
                    context.line_to(x, y)
                    context.stroke()
            self.draw_preview_item(
                context,
                *parent_geometry[:3],
                parent,
                parent_center_style,
                show_icon=self.show_icons_check.get_active(),
                opacity=parent_geometry[3],
                submenu_indicators=bool(parent.items),
                submenu_indicators_active=self.preview_hover_target
                == ("parent-center", None),
                submenu_indicators_return=True,
                submenu_indicator_skip_index=self.current_path[-1],
            )
            for _index, item, style, x, y, size, opacity in parent_circles:
                self.draw_preview_item(
                    context,
                    x,
                    y,
                    size,
                    item,
                    style,
                    show_icon=self.show_icons_check.get_active(),
                    opacity=opacity,
                    submenu_indicators=bool(item.items),
                )

        self.draw_preview_departures(context)

        menu, center, circles = self.preview_layout()
        if self.drag_reorder_armed and self.drag_item is not None:
            hidden_style = self.preview_style(self.drag_item, selected=True)
            hidden_size = self.preview_size(self.drag_item, hidden_style)
            self.animated_preview_geometry(
                ("item", id(self.drag_item)),
                (center[0], center[1], hidden_size * 0.75, 0.0),
            )
        center_style, center_geometry, circles = self.animate_preview_layout(
            menu,
            center,
            circles,
        )

        if self.current_path:
            parent_rules = dict(DEFAULT_PARENT_LINK)
            parent_rules.update(self.styles.get("parent-link", {}))
            parent_link = computed_style(
                {"parent-link": parent_rules},
                ("parent-link",),
            )
            if parent_link["width"]:
                delta_x = parent_geometry[0] - center_geometry[0]
                delta_y = parent_geometry[1] - center_geometry[1]
                distance = math.hypot(delta_x, delta_y)
                direction_x = delta_x / distance if distance else 0
                direction_y = delta_y / distance if distance else 0
                start_distance = center_geometry[2] / 2
                end_distance = max(
                    start_distance,
                    distance - parent_geometry[2] / 2,
                )
                set_source_color(
                    context,
                    parent_link["color"],
                    parent_link["opacity"]
                    * min(center_geometry[3], parent_geometry[3]),
                )
                context.set_line_width(parent_link["width"])
                context.set_line_cap(cairo.LINE_CAP_ROUND)
                context.move_to(
                    center_geometry[0] + start_distance * direction_x,
                    center_geometry[1] + start_distance * direction_y,
                )
                context.line_to(
                    center_geometry[0] + end_distance * direction_x,
                    center_geometry[1] + end_distance * direction_y,
                )
                context.stroke()
                context.set_line_cap(cairo.LINE_CAP_BUTT)

        connector = computed_style(self.styles, ("connector",))
        if connector["width"]:
            context.set_line_width(connector["width"])
            for _index, _item, _style, x, y, _size, opacity in circles:
                set_source_color(
                    context,
                    connector["color"],
                    connector["opacity"] * min(center_geometry[3], opacity),
                )
                context.move_to(*center_geometry[:2])
                context.line_to(x, y)
                context.stroke()

        self.draw_preview_item(
            context,
            *center_geometry[:3],
            menu,
            center_style,
            show_icon=self.show_icons_check.get_active(),
            opacity=center_geometry[3],
        )
        for index, item, style, x, y, size, opacity in circles:
            item_active = ("item", index) == self.selected_target() or (
                "item",
                index,
            ) == self.preview_hover_target
            self.draw_preview_item(
                context,
                x,
                y,
                size,
                item,
                style,
                show_icon=self.show_icons_check.get_active(),
                opacity=opacity,
                submenu_indicators=bool(item.items),
                submenu_indicators_active=item_active,
            )

    def draw_preview_item(
        self,
        context,
        x,
        y,
        size,
        item,
        style,
        show_icon=False,
        opacity=1.0,
        submenu_indicators=False,
        submenu_indicators_active=False,
        submenu_indicators_return=False,
        submenu_indicator_skip_index=None,
    ):
        style = dict(style)
        for property_name in ("background-color", "border-color", "color"):
            style[property_name] = self.animated_preview_color(
                ("item-color", id(item), property_name),
                style[property_name],
            )
        if submenu_indicators:
            self.draw_preview_submenu_indicators(
                context,
                x,
                y,
                size,
                item,
                style,
                opacity,
                submenu_indicators_active,
                submenu_indicators_return,
                submenu_indicator_skip_index,
            )
        radius = resolve_radius(style["border-radius"], size)
        rounded_rectangle(
            context,
            x - size / 2,
            y - size / 2,
            size,
            size,
            radius,
        )
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
        if item.icon and show_icon:
            path = icon_path(item.icon_theme, item.icon)
            if path is not None:
                icon_size = round(scaled_icon_size(style, size))
                try:
                    pixbuf = self.load_icon_pixbuf(path, icon_size, style["color"])
                    context.save()
                    Gdk.cairo_set_source_pixbuf(
                        context,
                        pixbuf,
                        x - pixbuf.get_width() / 2,
                        y - pixbuf.get_height() / 2,
                    )
                    context.paint_with_alpha(content_opacity(style) * opacity)
                    context.restore()
                    return
                except (GLib.Error, OSError, UnicodeError):
                    pass
        if not item.label:
            return
        layout_size, text_scale = fixed_text_geometry(
            style,
            size,
            computed_style(self.styles, ("circle",))["scale"],
        )
        draw_wrapped_text(
            context,
            x,
            y,
            layout_size,
            item.label,
            style,
            opacity,
            text_scale,
        )

    def load_icon_pixbuf(self, path, size, color):
        key = (str(path), path.stat().st_mtime_ns, size, color)
        pixbuf = self.icon_cache.get(key)
        if pixbuf is not None:
            return pixbuf
        if path.suffix.lower() == ".svg":
            source = colored_svg_source(path, color)
            loader = GdkPixbuf.PixbufLoader.new_with_type("svg")
            loader.set_size(size, size)
            loader.write(source.encode())
            loader.close()
            pixbuf = loader.get_pixbuf()
        else:
            pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                str(path),
                size,
                size,
                True,
            )
        self.icon_cache[key] = pixbuf
        return pixbuf

    def draw_preview_submenu_indicators(
        self,
        context,
        x,
        y,
        circle_size,
        item,
        submenu_style,
        opacity,
        active=False,
        return_circle=False,
        skip_index=None,
    ):
        selectors = ["submenu-indicator"]
        if active:
            selectors.append("submenu-indicator.active")
        if return_circle:
            selectors.append("submenu-indicator.return")
            if active:
                selectors.append("submenu-indicator.return.active")
        style = computed_style(self.styles, selectors)
        style["color"] = self.animated_preview_color(
            ("indicator-color", id(item)),
            style["color"],
        )

        def draw_angles(indicator_style, angles):
            indicator_size = indicator_style["width"]
            protrusion = indicator_style["protrusion"]
            if indicator_size is None or indicator_size <= 0 or protrusion <= 0:
                return
            orbit = max(0, circle_size / 2 - indicator_size / 2 + protrusion)
            context.save()
            if indicator_style["cut-indicators"]:
                clip_x1, clip_y1, clip_x2, clip_y2 = context.clip_extents()
                context.rectangle(
                    clip_x1,
                    clip_y1,
                    clip_x2 - clip_x1,
                    clip_y2 - clip_y1,
                )
                clip_size = max(0, circle_size - INDICATOR_CLIP_OVERLAP * 2)
                radius = max(
                    0,
                    resolve_radius(submenu_style["border-radius"], circle_size)
                    - INDICATOR_CLIP_OVERLAP,
                )
                rounded_rectangle(
                    context,
                    x - clip_size / 2,
                    y - clip_size / 2,
                    clip_size,
                    clip_size,
                    radius,
                )
                context.set_fill_rule(cairo.FILL_RULE_EVEN_ODD)
                context.clip()
            for angle in angles:
                radians = math.radians(angle)
                indicator_x = x + orbit * math.sin(radians)
                indicator_y = y - orbit * math.cos(radians)
                context.arc(
                    indicator_x,
                    indicator_y,
                    indicator_size / 2,
                    0,
                    math.tau,
                )
                set_source_color(
                    context,
                    indicator_style["color"],
                    indicator_style["opacity"] * opacity,
                )
                context.fill()
            context.restore()

        draw_angles(
            style,
            (
                child.angle
                for index, child in enumerate(item.items)
                if index != skip_index
            ),
        )

    def preview_hit(self, x, y):
        _menu, _center, circles = self.preview_layout()
        for index, _item, _style, circle_x, circle_y, size in reversed(circles):
            if math.hypot(x - circle_x, y - circle_y) <= size / 2:
                return index
        return None

    def on_preview_motion(self, _controller, x, y):
        if self.preview_parent_center_hit(x, y):
            target = ("parent-center", None)
        elif self.preview_center_hit(x, y):
            target = ("center", None)
        else:
            index = self.preview_hit(x, y)
            target = ("item", index) if index is not None else None
        if target != self.preview_hover_target:
            self.preview_hover_target = target
            self.canvas.queue_draw()

    def on_preview_leave(self, _controller):
        if self.preview_hover_target is not None:
            self.preview_hover_target = None
            self.canvas.queue_draw()

    def preview_center_hit(self, x, y):
        menu, center, _circles = self.preview_layout()
        style = self.preview_style(menu, center=True)
        return math.hypot(x - center[0], y - center[1]) <= (
            self.preview_size(menu, style) / 2
        )

    def preview_parent_center_hit(self, x, y):
        if not self.current_path:
            return False
        parent_path = self.current_path[:-1]
        parent, center, _circles = self.preview_layout(parent_path, previous=True)
        style = self.preview_style(parent, center=True, previous=True)
        return math.hypot(x - center[0], y - center[1]) <= (
            self.preview_size(parent, style) / 2
        )

    def select_preview_item(self, index):
        if index is None:
            return
        path = (*self.current_path, index)
        row = self.path_rows.get(path)
        if row is not None:
            self.tree.select_row(row)

    def on_preview_press(self, _gesture, _presses, x, y):
        self.click_origin = x, y
        self.drag_happened = False

    def on_preview_click(self, _gesture, _presses, x, y):
        moved = math.hypot(x - self.click_origin[0], y - self.click_origin[1])
        if moved > 6 or self.drag_happened:
            return
        if self.preview_parent_center_hit(x, y):
            self.on_up(None)
            return
        if self.preview_center_hit(x, y):
            path = self.current_path
            row = self.path_rows.get(path)
            if row is not None:
                self.tree.select_row(row)
            self.selected_path = path
            self.current_path = path
            self.sync_fields()
            self.canvas.queue_draw()
            return
        index = self.preview_hit(x, y)
        self.select_preview_item(index)
        if index is not None:
            path = (*self.current_path, index)
            if self.item_at(path).is_submenu:
                self.open_submenu(path)

    def on_drag_begin(self, _gesture, x, y):
        self.drag_active = False
        self.drag_undo_recorded = False
        self.drag_index = None
        if not self.preview_center_hit(x, y) and not self.preview_parent_center_hit(
            x, y
        ):
            self.drag_index = self.preview_hit(x, y)
        self.drag_origin = x, y
        center_x, center_y = self.preview_menu_center(self.current_path)
        self.drag_start_angle = direction_angle(x - center_x, y - center_y)
        menu = self.item_at(self.current_path)
        self.drag_initial_angles = [item.angle for item in menu.items]
        self.drag_reorder_armed = False
        self.drag_item = (
            menu.items[self.drag_index] if self.drag_index is not None else None
        )
        self.drag_original_index = self.drag_index
        self.drag_group_base = menu.items[0].angle if menu.items else 0
        self.select_preview_item(self.drag_index)

    def on_drag_update(self, _gesture, offset_x, offset_y):
        if self.drag_index is None:
            return
        if not self.drag_active:
            if math.hypot(offset_x, offset_y) <= 6:
                return
            self.drag_active = True
            self.drag_happened = True
            if not self.drag_undo_recorded:
                self.push_undo()
                self.drag_undo_recorded = True
        x = self.drag_origin[0] + offset_x
        y = self.drag_origin[1] + offset_y
        center_x, center_y = self.preview_menu_center(self.current_path)
        pointer_distance = math.hypot(x - center_x, y - center_y)
        pointer_angle = round(direction_angle(x - center_x, y - center_y)) % 360
        menu = self.item_at(self.current_path)
        item = self.drag_item

        if self.preserve_check.get_active():
            center_style = self.preview_style(menu, center=True)
            center_radius = self.preview_size(menu, center_style) / 2
            enter_radius = center_radius * 0.85
            exit_radius = center_radius * 1.15
            if not self.drag_reorder_armed and pointer_distance <= enter_radius:
                self.drag_reorder_armed = True
                self.align_current_menu(excluded=item)
            if self.drag_reorder_armed and pointer_distance < exit_radius:
                self.set_status("Move outward to place the item in another slot")
                self.sync_fields()
                self.canvas.queue_draw()
                return
            if self.drag_reorder_armed:
                self.insert_dragged_item(pointer_angle)
                self.drag_reorder_armed = False
                self.drag_initial_angles = [child.angle for child in menu.items]
                self.drag_start_angle = pointer_angle
            if self.current_path:
                delta = angular_delta(pointer_angle, self.drag_start_angle)
                self.rotate_group(menu, self.drag_initial_angles, delta)
            else:
                delta = angular_delta(pointer_angle, self.drag_start_angle)
                self.rotate_group(menu, self.drag_initial_angles, delta)
        else:
            self.set_item_angle(item, self.aligned_angle(pointer_angle))
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def on_drag_end(self, _gesture, _offset_x, _offset_y):
        if self.drag_reorder_armed:
            self.align_current_menu()
        menu = self.item_at(self.current_path)
        reordered = (
            self.drag_item is not None
            and self.drag_original_index is not None
            and menu.items.index(self.drag_item) != self.drag_original_index
        )
        self.drag_index = None
        self.drag_active = False
        self.drag_undo_recorded = False
        self.drag_initial_angles = []
        self.drag_reorder_armed = False
        self.drag_item = None
        self.drag_original_index = None
        if reordered:
            self.rebuild_tree()
            self.sync_fields()
            self.canvas.queue_draw()

    def on_layout_option_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.preserve_proportions = self.preserve_check.get_active()
        self.settings.auto_alignment = self.alignment_check.get_active()
        if self.preserve_check.get_active():
            self.align_current_menu()
        elif self.alignment_check.get_active():
            menu = self.item_at(self.current_path)
            for item in menu.items:
                self.set_item_angle(item, self.aligned_angle(item.angle))
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def on_show_icons_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.configurator_show_icons = self.show_icons_check.get_active()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def on_center_mode_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.center_mode = self.center_mode_check.get_active()
        self.set_status("Unsaved changes")

    def on_close_submenu_on_center_click_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.close_submenu_on_center_click = (
            self.close_submenu_on_center_click_check.get_active()
        )
        self.set_status("Unsaved changes")

    def on_hover_mode_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.hover_mode = self.hover_mode_check.get_active()
        self.set_status("Unsaved changes")

    def on_turbo_mode_changed(self, _check):
        if self.restoring_undo:
            return
        self.push_undo()
        self.settings.turbo_mode = self.turbo_mode_check.get_active()
        self.set_status("Unsaved changes")

    def aligned_angle(self, angle):
        if self.alignment_check.get_active():
            return min(
                ALIGNMENT_ANGLES,
                key=lambda candidate: angular_distance(angle, candidate),
            )
        return round(angle) % 360

    def on_center_layout(self, _button):
        menu = self.item_at(self.current_path)
        if not menu.items:
            return
        self.push_undo()

        if self.current_path:
            step = 360 / (len(menu.items) + 1)
            angles = [
                round(menu.return_angle + (index + 1) * step) % 360
                for index in range(len(menu.items))
            ]
        else:
            step = 360 / len(menu.items)
            candidates = (
                [round(base + index * step) % 360 for index in range(len(menu.items))]
                for base in (0, 90, 180, 270)
            )
            angles = min(
                candidates,
                key=lambda candidate: self.layout_assignment_cost(
                    menu.items,
                    candidate,
                ),
            )

        for item, angle in self.layout_assignment(menu.items, angles):
            self.set_item_angle(item, angle)
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def proportional_angles(self, items, base=None):
        if not items:
            return []
        if self.current_path:
            menu = self.item_at(self.current_path)
            parent_angle = largest_gap_angle(
                [item.angle for item in items],
                preferred=menu.return_angle,
            )
            step = 360 / (len(items) + 1)
            return [
                round(parent_angle + (index + 1) * step) % 360
                for index in range(len(items))
            ]
        if base is None:
            base = items[0].angle
        if self.alignment_check.get_active():
            base = self.aligned_angle(base)
        step = 360 / len(items)
        return [round(base + index * step) % 360 for index in range(len(items))]

    @staticmethod
    def layout_assignment(items, target_angles):
        ordered_items = sorted(items, key=lambda item: item.angle)
        ordered_targets = sorted(target_angles)
        if not ordered_items:
            return []
        assignments = []
        for shift in range(len(ordered_targets)):
            shifted = ordered_targets[shift:] + ordered_targets[:shift]
            assignments.append(list(zip(ordered_items, shifted, strict=True)))
        return min(
            assignments,
            key=lambda assignment: sum(
                angular_distance(item.angle, angle) for item, angle in assignment
            ),
        )

    def layout_assignment_cost(self, items, target_angles):
        return sum(
            angular_distance(item.angle, angle)
            for item, angle in self.layout_assignment(items, target_angles)
        )

    def align_current_menu(self, excluded=None):
        menu = self.item_at(self.current_path)
        items = [item for item in menu.items if item is not excluded]
        if not items:
            return
        base = self.drag_group_base if excluded is not None else None
        angles = self.proportional_angles(items, base)
        for item, angle in self.layout_assignment(items, angles):
            self.set_item_angle(item, angle)

    def rotate_group(self, menu, initial_angles, delta):
        if not initial_angles:
            return
        if self.alignment_check.get_active():
            rotated_base = initial_angles[0] + delta
            snapped_base = self.aligned_angle(rotated_base)
            delta = angular_delta(snapped_base, initial_angles[0])
        for item, initial in zip(menu.items, initial_angles, strict=True):
            self.set_item_angle(item, round(initial + delta) % 360)

    def insert_dragged_item(self, pointer_angle):
        menu = self.item_at(self.current_path)
        item = self.drag_item
        current_index = menu.items.index(item)
        candidates = list(menu.items)
        candidates.pop(current_index)
        best_index = 0
        best_difference = 181
        for index in range(len(candidates) + 1):
            order = list(candidates)
            order.insert(index, item)
            angle = self.proportional_angles(order, self.drag_group_base)[index]
            difference = angular_distance(pointer_angle, angle)
            if difference < best_difference:
                best_index = index
                best_difference = difference
        menu.items.pop(current_index)
        menu.items.insert(best_index, item)
        for child, angle in zip(
            menu.items,
            self.proportional_angles(menu.items, self.drag_group_base),
            strict=True,
        ):
            self.set_item_angle(child, angle)
        self.drag_index = best_index
        self.selected_path = (*self.current_path, best_index)

    def set_item_angle(self, item, angle):
        angle = round(angle) % 360
        delta = angular_delta(angle, item.angle)
        item.angle = angle
        self.rotate_descendants(item, delta)

    def rotate_descendants(self, item, delta):
        if not item.items:
            return
        item.return_angle = round(item.return_angle + delta) % 360
        for child in item.items:
            child.angle = round(child.angle + delta) % 360
            self.rotate_descendants(child, delta)

    def on_choose_icon(self, _button):
        themes = sort_icon_themes(icon_themes(), load_icon_theme_history())
        if not themes:
            ICON_DIR.mkdir(parents=True, exist_ok=True)
            self.set_status(f"Add icon folders to {ICON_DIR}", error=True)
            return

        item = self.item_at(self.selected_path)
        icon_style = self.preview_style(
            item,
            center=self.selected_path == self.current_path,
        )
        window = Gtk.Window(
            title="Choose icon",
            transient_for=self.window,
            modal=True,
            default_width=720,
            default_height=620,
        )
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        content.set_margin_top(10)
        content.set_margin_bottom(10)
        content.set_margin_start(10)
        content.set_margin_end(10)

        controls = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        theme_select = Gtk.ComboBoxText()
        theme_select.append(ALL_ICON_THEMES, "All icon sets")
        for theme in themes:
            theme_select.append(theme, theme)
        theme_select.set_active_id(
            item.icon_theme if item.icon_theme in themes else ALL_ICON_THEMES
        )
        theme_select.set_hexpand(True)
        search = Gtk.SearchEntry(placeholder_text="Search icons…")
        search.set_hexpand(True)
        clear = Gtk.Button(label="Remove icon")
        controls.append(theme_select)
        controls.append(search)
        controls.append(clear)
        content.append(controls)

        result_label = Gtk.Label(xalign=0)
        content.append(result_label)
        flow = Gtk.FlowBox(
            selection_mode=Gtk.SelectionMode.NONE,
            homogeneous=True,
            row_spacing=6,
            column_spacing=6,
            max_children_per_line=8,
            min_children_per_line=4,
        )
        scroll = Gtk.ScrolledWindow(vexpand=True)
        scroll.set_child(flow)
        content.append(scroll)

        icons_by_theme = {}

        def icons_for(theme):
            if theme not in icons_by_theme:
                icons_by_theme[theme] = theme_icons(theme)
            return icons_by_theme[theme]

        def choose(theme, icon):
            self.push_undo()
            item.icon_theme = theme
            item.icon = icon
            remember_icon_theme(item.icon_theme)
            self.sync_fields()
            self.canvas.queue_draw()
            self.set_status("Unsaved changes")
            window.close()

        def rebuild(*_args):
            while child := flow.get_child_at_index(0):
                flow.remove(child)
            selected_theme = theme_select.get_active_id()
            term = search.get_text().strip().casefold()
            searched_themes = (
                themes if selected_theme == ALL_ICON_THEMES else [selected_theme]
            )
            visible = []
            match_count = 0
            for theme in searched_themes:
                theme_matches = term and term in theme.casefold()
                for icon in icons_for(theme):
                    if term and not theme_matches and term not in icon.casefold():
                        continue
                    match_count += 1
                    if len(visible) < 400:
                        visible.append((theme, icon))
            result_label.set_text(
                f"{match_count} icons"
                + (" — refine the search to see more" if match_count > 400 else "")
            )
            for theme, icon in visible:
                path = icon_path(theme, icon)
                button = Gtk.Button(tooltip_text=f"{theme}: {icon}")
                try:
                    pixbuf = self.load_icon_pixbuf(path, 56, icon_style["color"])
                except (GLib.Error, OSError, UnicodeError):
                    continue
                picture = Gtk.Picture.new_for_pixbuf(pixbuf)
                picture.set_content_fit(Gtk.ContentFit.CONTAIN)
                picture.set_size_request(56, 56)
                button.set_child(picture)
                button.connect(
                    "clicked",
                    lambda _button, selected_theme=theme, name=icon: choose(
                        selected_theme,
                        name,
                    ),
                )
                flow.append(button)

        def remove_icon(_button):
            self.push_undo()
            item.icon_theme = None
            item.icon = None
            self.sync_fields()
            self.canvas.queue_draw()
            self.set_status("Unsaved changes")
            window.close()

        theme_select.connect("changed", rebuild)
        search.connect("search-changed", rebuild)
        clear.connect("clicked", remove_icon)
        window.set_child(content)
        rebuild()
        window.present()

    def on_save(self, _button):
        try:
            validate_editable_tree(self.settings.root)
            text = serialize_config(self.settings)
            CONFIG_DIR.mkdir(parents=True, exist_ok=True)
            backup = CONFIG_PATH.with_name(f"{CONFIG_PATH.name}.bak")
            if CONFIG_PATH.exists() and not backup.exists():
                shutil.copy2(
                    CONFIG_PATH,
                    backup,
                )
            temporary = CONFIG_PATH.with_name(f".{CONFIG_PATH.name}.tmp")
            temporary.write_text(text, encoding="utf-8")
            temporary.replace(CONFIG_PATH)
        except (OSError, ValueError) as error:
            self.set_status(str(error), error=True)
            return
        self.set_status(f"Saved {CONFIG_PATH}")

    def set_status(self, message, error=False):
        if message.startswith("Saved"):
            self.status.set_text("Saved")
        elif not error and not message.startswith("Editing:"):
            self.status.set_text("Unsaved")


def validate_editable_tree(item, root=True, location="menu"):
    if not item.label:
        raise ValueError(f"{location}: label cannot be empty")
    if not root and not item.is_submenu and not item.command:
        raise ValueError(f"{location}: command cannot be empty")
    for index, child in enumerate(item.items):
        validate_editable_tree(
            child,
            root=False,
            location=f"{location}.items[{index}]",
        )


def toml_number(value):
    return str(int(value)) if float(value).is_integer() else repr(float(value))


def serialize_config(settings):
    lines = [f"menu-radius = {toml_number(settings.menu_radius)}"]
    if settings.center_hitbox_size is not None:
        lines.append(f"center-hitbox-size = {toml_number(settings.center_hitbox_size)}")
    lines.append(
        f"minimum-edge-distance = {toml_number(settings.minimum_edge_distance)}"
    )
    lines.append(f"center-mode = {str(settings.center_mode).lower()}")
    lines.append(
        "close-submenu-on-center-click = "
        f"{str(settings.close_submenu_on_center_click).lower()}"
    )
    lines.append(f"hover-mode = {str(settings.hover_mode).lower()}")
    lines.append(f"turbo-mode = {str(settings.turbo_mode).lower()}")
    lines.append(f"preserve-proportions = {str(settings.preserve_proportions).lower()}")
    lines.append(f"auto-alignment = {str(settings.auto_alignment).lower()}")
    lines.append(
        f"configurator-show-icons = {str(settings.configurator_show_icons).lower()}"
    )

    def append_item(item, header):
        lines.extend(("", header, f"label = {json.dumps(item.label)}"))
        if item.command is not None:
            lines.append(f"command = {json.dumps(item.command)}")
        if item.icon_theme and item.icon:
            lines.append(f"icon-theme = {json.dumps(item.icon_theme)}")
            lines.append(f"icon = {json.dumps(item.icon)}")
        if item.angle is not None:
            lines.append(f"angle = {toml_number(round(item.angle) % 360)}")
        for child in item.items:
            append_item(child, f"[[{header.strip('[]')}.items]]")

    append_item(settings.root, "[menu]")
    return "\n".join(lines).lstrip() + "\n"


def main():
    try:
        exit_code = Configurator(load_config(), load_styles()).run([sys.argv[0]])
    except KeyboardInterrupt:
        exit_code = 0
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()

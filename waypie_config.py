import json
import math
import shutil
import sys

import cairo
import gi

gi.require_version("Gtk", "4.0")

from gi.repository import Gtk

from waypie_common import (
    CONFIG_DIR,
    CONFIG_PATH,
    Item,
    angular_delta,
    angular_distance,
    computed_style,
    direction_angle,
    largest_gap_angle,
    load_config,
    load_styles,
    resolve_angles,
    resolve_radius,
    rounded_rectangle,
    set_source_color,
    truncate,
)

ALIGNMENT_ANGLES = tuple(
    sorted({angle % 360 for step in (30, 45) for angle in range(0, 360, step)})
)
DEFAULT_PARENT_LINK = {
    "color": "#ff8c00",
    "opacity": "0.35",
    "width": "12px",
}


class Configurator(Gtk.Application):
    def __init__(self, settings, styles):
        super().__init__(application_id="dev.waypie.Configurator")
        self.settings = settings
        self.styles = styles
        self.window = None
        self.canvas = None
        self.tree = None
        self.status = None
        self.label_entry = None
        self.command_entry = None
        self.angle_spin = None
        self.preserve_check = None
        self.alignment_check = None
        self.selected_path = ()
        self.current_path = ()
        self.row_paths = {}
        self.path_rows = {}
        self.drag_index = None
        self.drag_origin = (0.0, 0.0)
        self.drag_start_angle = 0.0
        self.drag_initial_angles = []
        self.drag_initial_proportion_angle = None
        self.drag_reorder_armed = False
        self.drag_item = None
        self.drag_original_index = None
        self.drag_group_base = 0
        self.updating_fields = False

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
        for label, callback in (
            ("Add command", self.on_add_command),
            ("Add submenu", self.on_add_submenu),
            ("Delete", self.on_delete),
            ("Open submenu", self.on_open_submenu),
            ("Up", self.on_up),
            ("Save", self.on_save),
        ):
            button = Gtk.Button(label=label)
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
        self.status = Gtk.Label(xalign=0)
        self.status.set_hexpand(True)
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
        click.connect("released", self.on_preview_click)
        self.canvas.add_controller(click)
        drag = Gtk.GestureDrag()
        drag.set_button(1)
        drag.connect("drag-begin", self.on_drag_begin)
        drag.connect("drag-update", self.on_drag_update)
        drag.connect("drag-end", self.on_drag_end)
        self.canvas.add_controller(drag)
        content.append(self.canvas)

        properties = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        properties.set_size_request(280, -1)
        self.label_entry = self.add_property(properties, "Label", Gtk.Entry())
        self.command_entry = self.add_property(properties, "Command", Gtk.Entry())
        self.angle_spin = self.add_property(
            properties,
            "Angle (degrees)",
            Gtk.SpinButton.new_with_range(0, 359, 1),
        )
        self.angle_spin.set_digits(0)
        hint = Gtk.Label(
            label=(
                "Drag a circle to change its angle.\n"
                "Double-click a submenu to edit its children."
            ),
            xalign=0,
            wrap=True,
        )
        properties.append(hint)
        content.append(properties)
        root.append(content)

        self.label_entry.connect("changed", self.on_property_changed)
        self.command_entry.connect("changed", self.on_property_changed)
        self.angle_spin.connect("value-changed", self.on_property_changed)

        self.window.set_child(root)
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
            suffix = " ▸" if item.items else ""
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
            self.tree.select_row(row)

    def on_tree_selected(self, _tree, row):
        if row is None:
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
        if self.item_at(path).items:
            self.open_submenu(path)

    def sync_fields(self):
        item = self.item_at(self.selected_path)
        is_root = not self.selected_path
        self.updating_fields = True
        self.label_entry.set_text(item.label)
        self.command_entry.set_text(item.command or "")
        self.command_entry.set_sensitive(not is_root and not item.items)
        self.angle_spin.set_value(item.angle or 0)
        self.angle_spin.set_sensitive(not is_root)
        self.updating_fields = False

    def on_property_changed(self, _widget):
        if self.updating_fields:
            return
        item = self.item_at(self.selected_path)
        item.label = self.label_entry.get_text()
        if self.selected_path and not item.items:
            item.command = self.command_entry.get_text()
        if self.selected_path:
            requested_angle = round(self.angle_spin.get_value()) % 360
            if self.preserve_check.get_active():
                menu = self.item_at(self.selected_path[:-1])
                initial = [child.angle for child in menu.items]
                delta = angular_delta(requested_angle, item.angle)
                initial_proportion = (
                    menu.proportion_angle if self.selected_path[:-1] else None
                )
                self.rotate_group(menu, initial, delta, initial_proportion)
            else:
                self.set_item_angle(item, self.aligned_angle(requested_angle))
            item.x = None
            item.y = None
        row = self.path_rows.get(self.selected_path)
        if row is not None:
            suffix = " ▸" if item.items else ""
            row.get_child().set_text(
                f"{'  ' * len(self.selected_path)}{item.label}{suffix}"
            )
        self.set_status("Unsaved changes")
        self.canvas.queue_draw()

    def on_add_command(self, _button):
        menu = self.item_at(self.current_path)
        angle = largest_gap_angle([item.angle % 360 for item in menu.items])
        if angle is None:
            angle = 0
        angle = round(angle) % 360
        menu.items.append(Item("New command", command="true", angle=angle))
        self.selected_path = (*self.current_path, len(menu.items) - 1)
        self.after_structure_change()

    def on_add_submenu(self, _button):
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
        parent_path = self.selected_path[:-1]
        parent = self.item_at(parent_path)
        del parent.items[self.selected_path[-1]]
        self.selected_path = parent_path
        self.current_path = parent_path
        self.after_structure_change()

    def on_open_submenu(self, _button):
        self.open_submenu(self.selected_path)

    def open_submenu(self, path):
        item = self.item_at(path)
        if not item.items:
            self.set_status("The selected item is not a submenu", error=True)
            return
        self.selected_path = path
        self.current_path = path
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status(f"Editing: {item.label}")

    def on_up(self, _button):
        if not self.current_path:
            return
        self.selected_path = self.current_path
        self.current_path = self.current_path[:-1]
        self.rebuild_tree()
        self.sync_fields()
        self.canvas.queue_draw()

    def after_structure_change(self):
        resolve_angles(self.settings.root, root=True)
        if self.preserve_check.get_active():
            self.align_current_menu()
        self.rebuild_tree()
        self.sync_fields()
        self.canvas.queue_draw()
        self.set_status("Unsaved changes")

    def preview_layout(self):
        menu = self.item_at(self.current_path)
        width = self.canvas.get_width()
        height = self.canvas.get_height()
        center = width / 2, height / 2
        circles = []
        for index, item in enumerate(menu.items):
            if self.drag_reorder_armed and item is self.drag_item:
                continue
            style = self.preview_style(item, ("item", index) == self.selected_target())
            size = self.preview_size(item, style)
            if item.x is not None:
                x = center[0] + item.x
                y = center[1] + item.y
            else:
                distance = (
                    style["distance"]
                    if style["distance"] is not None
                    else self.settings.menu_radius
                )
                radians = math.radians(item.angle)
                x = center[0] + distance * math.sin(radians)
                y = center[1] - distance * math.cos(radians)
            circles.append((index, item, style, x, y, size))
        return menu, center, circles

    def selected_target(self):
        if self.selected_path and self.selected_path[:-1] == self.current_path:
            return "item", self.selected_path[-1]
        return None

    def preview_style(self, item, selected=False, center=False):
        selectors = ["circle"]
        if item.items:
            selectors.append("circle.submenu")
        selectors.append("circle.center" if center else "circle.item")
        if selected:
            selectors.append("circle.active")
        return computed_style(self.styles, selectors)

    def preview_size(self, item, style):
        size = (
            style["width"]
            if style["width"] is not None
            else item.size or self.settings.circle_size
        )
        return size * style["scale"]

    def draw_preview(self, _canvas, context, width, height):
        overlay = computed_style(self.styles, ("overlay",))
        context.set_source_rgba(*overlay["background-color"])
        context.paint()
        menu, center, circles = self.preview_layout()

        if self.current_path:
            parent_rules = dict(DEFAULT_PARENT_LINK)
            parent_rules.update(self.styles.get("parent-link", {}))
            parent_link = computed_style(
                {"parent-link": parent_rules},
                ("parent-link",),
            )
            if parent_link["width"]:
                distance = (
                    computed_style(self.styles, ("circle",))["distance"]
                    or self.settings.menu_radius
                )
                radians = math.radians(menu.return_angle)
                center_style = self.preview_style(menu, center=True)
                start_distance = self.preview_size(menu, center_style) / 2
                set_source_color(
                    context,
                    parent_link["color"],
                    parent_link["opacity"],
                )
                context.set_line_width(parent_link["width"])
                context.set_line_cap(cairo.LINE_CAP_ROUND)
                context.move_to(
                    center[0] + start_distance * math.sin(radians),
                    center[1] - start_distance * math.cos(radians),
                )
                context.line_to(
                    center[0] + distance * math.sin(radians),
                    center[1] - distance * math.cos(radians),
                )
                context.stroke()
                context.set_line_cap(cairo.LINE_CAP_BUTT)

        connector = computed_style(self.styles, ("connector",))
        if connector["width"]:
            set_source_color(context, connector["color"], connector["opacity"])
            context.set_line_width(connector["width"])
            for _index, _item, _style, x, y, _size in circles:
                context.move_to(*center)
                context.line_to(x, y)
            context.stroke()

        center_style = self.preview_style(menu, center=True)
        self.draw_preview_item(
            context,
            center[0],
            center[1],
            self.preview_size(menu, center_style),
            menu,
            center_style,
        )
        for _index, item, style, x, y, size in circles:
            self.draw_preview_item(context, x, y, size, item, style)

    @staticmethod
    def draw_preview_item(context, x, y, size, item, style):
        radius = resolve_radius(style["border-radius"], size)
        rounded_rectangle(
            context,
            x - size / 2,
            y - size / 2,
            size,
            size,
            radius,
        )
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
            style["font-family"],
            cairo.FONT_SLANT_NORMAL,
            cairo.FONT_WEIGHT_NORMAL,
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

    def preview_hit(self, x, y):
        _menu, _center, circles = self.preview_layout()
        for index, _item, _style, circle_x, circle_y, size in reversed(circles):
            if math.hypot(x - circle_x, y - circle_y) <= size / 2:
                return index
        return None

    def preview_center_hit(self, x, y):
        if not self.current_path:
            return False
        menu, center, _circles = self.preview_layout()
        style = self.preview_style(menu, center=True)
        return math.hypot(x - center[0], y - center[1]) <= (
            self.preview_size(menu, style) / 2
        )

    def select_preview_item(self, index):
        if index is None:
            return
        path = (*self.current_path, index)
        row = self.path_rows.get(path)
        if row is not None:
            self.tree.select_row(row)

    def on_preview_click(self, _gesture, presses, x, y):
        if self.preview_center_hit(x, y):
            self.on_up(None)
            return
        index = self.preview_hit(x, y)
        self.select_preview_item(index)
        if presses == 2 and index is not None:
            path = (*self.current_path, index)
            if self.item_at(path).items:
                self.open_submenu(path)

    def on_drag_begin(self, _gesture, x, y):
        self.drag_index = (
            None if self.preview_center_hit(x, y) else self.preview_hit(x, y)
        )
        self.drag_origin = x, y
        center_x = self.canvas.get_width() / 2
        center_y = self.canvas.get_height() / 2
        self.drag_start_angle = direction_angle(x - center_x, y - center_y)
        menu = self.item_at(self.current_path)
        self.drag_initial_angles = [item.angle for item in menu.items]
        self.drag_initial_proportion_angle = menu.proportion_angle
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
        x = self.drag_origin[0] + offset_x
        y = self.drag_origin[1] + offset_y
        center_x = self.canvas.get_width() / 2
        center_y = self.canvas.get_height() / 2
        pointer_distance = math.hypot(x - center_x, y - center_y)
        pointer_angle = round(direction_angle(x - center_x, y - center_y)) % 360
        menu = self.item_at(self.current_path)
        item = self.drag_item

        if self.preserve_check.get_active():
            center_style = self.preview_style(menu, center=True)
            center_radius = self.preview_size(menu, center_style) / 2
            if pointer_distance <= center_radius:
                if not self.drag_reorder_armed:
                    self.drag_reorder_armed = True
                    self.align_current_menu(excluded=item)
                self.set_status("Move outward to place the item in another slot")
                self.sync_fields()
                self.canvas.queue_draw()
                return
            if self.drag_reorder_armed:
                self.insert_dragged_item(pointer_angle)
                self.drag_reorder_armed = False
                self.drag_initial_angles = [child.angle for child in menu.items]
                self.drag_initial_proportion_angle = menu.proportion_angle
                self.drag_start_angle = pointer_angle
            if self.current_path:
                delta = angular_delta(pointer_angle, self.drag_start_angle)
                self.rotate_group(
                    menu,
                    self.drag_initial_angles,
                    delta,
                    self.drag_initial_proportion_angle,
                )
            else:
                delta = angular_delta(pointer_angle, self.drag_start_angle)
                self.rotate_group(menu, self.drag_initial_angles, delta)
        else:
            self.set_item_angle(item, self.aligned_angle(pointer_angle))
        item.x = None
        item.y = None
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
        self.drag_initial_angles = []
        self.drag_initial_proportion_angle = None
        self.drag_reorder_armed = False
        self.drag_item = None
        self.drag_original_index = None
        if reordered:
            self.rebuild_tree()
            self.sync_fields()
            self.canvas.queue_draw()

    def on_layout_option_changed(self, _check):
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

    def aligned_angle(self, angle):
        if self.alignment_check.get_active():
            return min(
                ALIGNMENT_ANGLES,
                key=lambda candidate: angular_distance(angle, candidate),
            )
        return round(angle) % 360

    def proportional_angles(self, items, base=None):
        if not items:
            return []
        if self.current_path:
            menu = self.item_at(self.current_path)
            parent_angle = menu.proportion_angle
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

    def align_current_menu(self, excluded=None):
        menu = self.item_at(self.current_path)
        items = [item for item in menu.items if item is not excluded]
        if not items:
            return
        base = self.drag_group_base if excluded is not None else None
        for item, angle in zip(
            items,
            self.proportional_angles(items, base),
            strict=True,
        ):
            self.set_item_angle(item, angle)
            item.x = None
            item.y = None

    def rotate_group(
        self,
        menu,
        initial_angles,
        delta,
        initial_proportion_angle=None,
    ):
        if not initial_angles:
            return
        if self.alignment_check.get_active():
            rotated_base = initial_angles[0] + delta
            snapped_base = self.aligned_angle(rotated_base)
            delta = angular_delta(snapped_base, initial_angles[0])
        for item, initial in zip(menu.items, initial_angles, strict=True):
            self.set_item_angle(item, round(initial + delta) % 360)
            item.x = None
            item.y = None
        if initial_proportion_angle is not None:
            menu.proportion_angle = round(initial_proportion_angle + delta) % 360

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
        item.proportion_angle = round(item.proportion_angle + delta) % 360
        for child in item.items:
            child.angle = round(child.angle + delta) % 360
            child.x = None
            child.y = None
            self.rotate_descendants(child, delta)

    def on_save(self, _button):
        try:
            validate_editable_tree(self.settings.root)
            text = serialize_config(self.settings)
            CONFIG_DIR.mkdir(parents=True, exist_ok=True)
            if CONFIG_PATH.exists():
                shutil.copy2(
                    CONFIG_PATH,
                    CONFIG_PATH.with_name(f"{CONFIG_PATH.name}.bak"),
                )
            temporary = CONFIG_PATH.with_name(f".{CONFIG_PATH.name}.tmp")
            temporary.write_text(text, encoding="utf-8")
            temporary.replace(CONFIG_PATH)
        except (OSError, ValueError) as error:
            self.set_status(str(error), error=True)
            return
        self.set_status(f"Saved {CONFIG_PATH}")

    def set_status(self, message, error=False):
        prefix = "Error: " if error else ""
        self.status.set_text(f"{prefix}{message}")


def validate_editable_tree(item, root=True, location="menu"):
    if not item.label:
        raise ValueError(f"{location}: label cannot be empty")
    if not root and bool(item.command) == bool(item.items):
        raise ValueError(f"{location}: use either a command or child items")
    for index, child in enumerate(item.items):
        validate_editable_tree(
            child,
            root=False,
            location=f"{location}.items[{index}]",
        )


def toml_number(value):
    return str(int(value)) if float(value).is_integer() else repr(float(value))


def serialize_config(settings):
    lines = [
        f"circle-size = {toml_number(settings.circle_size)}",
        f"menu-radius = {toml_number(settings.menu_radius)}",
    ]
    if settings.center_hitbox_size is not None:
        lines.append(f"center-hitbox-size = {toml_number(settings.center_hitbox_size)}")
    lines.append(
        f"minimum-edge-distance = {toml_number(settings.minimum_edge_distance)}"
    )
    lines.append(f"preserve-proportions = {str(settings.preserve_proportions).lower()}")
    lines.append(f"auto-alignment = {str(settings.auto_alignment).lower()}")

    def append_item(item, header):
        lines.extend(("", header, f"label = {json.dumps(item.label)}"))
        if item.command is not None:
            lines.append(f"command = {json.dumps(item.command)}")
        for name in ("angle", "x", "y", "size"):
            value = getattr(item, name)
            if value is not None:
                if name == "angle":
                    value = round(value) % 360
                lines.append(f"{name} = {toml_number(value)}")
        if item.items and item.proportion_angle is not None and header != "[menu]":
            lines.append(f"proportion-angle = {toml_number(item.proportion_angle)}")
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

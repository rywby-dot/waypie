import json
import math
import re
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

CONFIG_DIR = Path.home() / ".config" / "waypie"
CONFIG_PATH = CONFIG_DIR / "config"
STYLE_PATH = CONFIG_DIR / "style.css"
ICON_DIR = CONFIG_DIR / "icons"
ICON_HISTORY_PATH = CONFIG_DIR / ".icon-history.json"
ICON_EXTENSIONS = {".svg", ".png", ".webp", ".jpg", ".jpeg", ".gif"}
MAX_ANIMATION_SECONDS = 60 * 60
CSS_NUMBER = r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?"
CIRCLE_PROPERTIES = {
    "background-color",
    "border-color",
    "border-width",
    "border-radius",
    "color",
    "cut-indicators",
    "distance",
    "font-size",
    "font-family",
    "font-weight",
    "follow-distance",
    "icon-fill",
    "icon-size",
    "opacity",
    "text-fill",
    "text-opacity",
    "protrusion",
    "scale",
    "width",
}
ITEM_KEY_PROPERTIES = {
    "angle",
    "color",
    "distance",
    "off",
    "font-family",
    "font-size",
    "font-weight",
    "opacity",
    "scale",
}
CIRCLE_SELECTORS = {
    "overlay",
    "circle",
    "circle.active",
    "circle.item",
    "circle.item.active",
    "circle.submenu",
    "circle.submenu.active",
    "circle.center",
    "circle.center.active",
    "circle.history",
    "circle.history.active",
    "circle.previous",
    "connector",
    "connector.active",
    "submenu-indicator",
    "submenu-indicator.active",
    "submenu-indicator.return",
    "submenu-indicator.return.active",
    "parent-link",
    "configurator-history",
}
ANIMATION_PROPERTIES = {
    "off",
    "color-duration",
    "opacity-duration",
    "icon-duration",
    "connector-duration",
    "item-delete-duration",
    "close-duration",
    "action-scale",
    "hover-duration",
    "follow-duration",
    "menu-duration",
    "menu-move-duration",
    "item-create-duration",
    "action-duration",
    "submenu-indicator-duration",
} | {
    f"{prefix}-{suffix}"
    for prefix in (
        "hover",
        "follow",
        "menu-move",
        "item-create",
        "action",
        "submenu-indicator",
    )
    for suffix in ("damping-ratio", "stiffness", "epsilon")
}


@dataclass
class Item:
    label: str
    keys: str = ""
    command: str | None = None
    angle: float | None = None
    return_angle: float | None = None
    icon_theme: str | None = None
    icon: str | None = None
    items: list["Item"] = field(default_factory=list)

    @property
    def is_submenu(self):
        return self.command is None


@dataclass
class Settings:
    menu_radius: float
    center_hitbox_size: float | None
    minimum_edge_distance: float
    center_mode: bool
    always_center_mode: bool
    back_keys: str
    autogenerate_key_sets: str
    close_submenu_on_center_click: bool
    hover_mode: bool
    turbo_mode: bool
    hold_to_turbo: bool
    travel_item_animation: bool
    preserve_proportions: bool
    auto_alignment: bool
    configurator_show_icons: bool
    root: Item


DEFAULT_STYLE = {
    "background-color": (0.0, 0.0, 0.0, 0.0),
    "border-color": (0.0, 0.0, 0.0, 0.0),
    "border-width": 0.0,
    "border-radius": "50%",
    "color": (1.0, 1.0, 1.0, 1.0),
    "cut-indicators": True,
    "distance": None,
    "font-size": 14.0,
    "font-family": "Sans",
    "font-weight": 400,
    "follow-distance": 0.0,
    "icon-fill": None,
    "icon-size": None,
    "opacity": 1.0,
    "text-fill": 1.0,
    "text-opacity": None,
    "protrusion": 0.0,
    "scale": 1.0,
    "width": None,
}


def load_config(path=CONFIG_PATH):
    path = Path(path)
    try:
        with path.open("rb") as file:
            source = tomllib.load(file)
    except OSError as error:
        raise SystemExit(f"waypie: cannot read {path}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise SystemExit(f"waypie: invalid config: {error}") from error

    menu = source.get("menu")
    if not isinstance(menu, dict):
        raise SystemExit("waypie: config requires a [menu] table")
    known_settings = {
        "menu-radius",
        "center-hitbox-size",
        "minimum-edge-distance",
        "center-mode",
        "always-center-mode",
        "back-keys",
        "autogenerate-key-sets",
        "close-submenu-on-center-click",
        "hover-mode",
        "turbo-mode",
        "hold-to-turbo",
        "travel-item-animation",
        "preserve-proportions",
        "auto-alignment",
        "configurator-show-icons",
        "menu",
    }
    unknown = sorted(set(source) - known_settings)
    if unknown:
        raise SystemExit(f"waypie: unknown config setting: {unknown[0]}")

    menu_radius = positive_number(source.get("menu-radius", 170), "menu-radius")
    center_hitbox_size = optional_nonnegative(
        source.get("center-hitbox-size"),
        "center-hitbox-size",
    )
    minimum_edge_distance = nonnegative_number(
        source.get("minimum-edge-distance", 0),
        "minimum-edge-distance",
    )
    center_mode = boolean(source.get("center-mode", False), "center-mode")
    always_center_mode = boolean(
        source.get("always-center-mode", False),
        "always-center-mode",
    )
    back_keys = parse_keys(source.get("back-keys", ""), "back-keys")
    autogenerate_key_sets = parse_key_sets(
        source.get("autogenerate-key-sets", ""),
        "autogenerate-key-sets",
    )
    close_submenu_on_center_click = boolean(
        source.get("close-submenu-on-center-click", False),
        "close-submenu-on-center-click",
    )
    hover_mode = boolean(source.get("hover-mode", False), "hover-mode")
    turbo_mode = boolean(source.get("turbo-mode", False), "turbo-mode")
    hold_to_turbo = boolean(source.get("hold-to-turbo", False), "hold-to-turbo")
    travel_item_animation = boolean(
        source.get("travel-item-animation", False),
        "travel-item-animation",
    )
    preserve_proportions = boolean(
        source.get("preserve-proportions", False),
        "preserve-proportions",
    )
    auto_alignment = boolean(
        source.get("auto-alignment", False),
        "auto-alignment",
    )
    configurator_show_icons = boolean(
        source.get("configurator-show-icons", False),
        "configurator-show-icons",
    )
    root = parse_item(menu, "menu", True)
    resolve_angles(root, root=True)
    return Settings(
        menu_radius,
        center_hitbox_size,
        minimum_edge_distance,
        center_mode,
        always_center_mode,
        back_keys,
        autogenerate_key_sets,
        close_submenu_on_center_click,
        hover_mode,
        turbo_mode,
        hold_to_turbo,
        travel_item_animation,
        preserve_proportions,
        auto_alignment,
        configurator_show_icons,
        root,
    )


def parse_item(source, location, root=False):
    if not isinstance(source, dict):
        raise SystemExit(f"waypie: {location} must be a table")
    unknown = sorted(
        set(source)
        - {"label", "keys", "command", "angle", "icon-theme", "icon", "items"}
    )
    if unknown:
        raise SystemExit(f"waypie: unknown {location} setting: {unknown[0]}")

    label = source.get("label", "")
    keys = parse_keys(source.get("keys", ""), f"{location}.keys")
    command = source.get("command")
    children = source.get("items", [])
    if not isinstance(label, str):
        raise SystemExit(f"waypie: {location}.label must be text")
    if command is not None and not isinstance(command, str):
        raise SystemExit(f"waypie: {location}.command must be text")
    if not isinstance(children, list):
        raise SystemExit(f"waypie: {location}.items must be an array")
    if command is not None and children:
        raise SystemExit(f"waypie: {location} cannot have command and items")
    if command is not None and not command:
        raise SystemExit(f"waypie: {location}.command cannot be empty")

    angle = optional_number(source.get("angle"), f"{location}.angle")
    if angle is not None:
        angle = round(angle) % 360
    icon_theme = source.get("icon-theme")
    icon = source.get("icon")
    if icon_theme is not None and not isinstance(icon_theme, str):
        raise SystemExit(f"waypie: {location}.icon-theme must be text")
    if icon is not None and not isinstance(icon, str):
        raise SystemExit(f"waypie: {location}.icon must be text")
    if bool(icon_theme) != bool(icon):
        raise SystemExit(
            f"waypie: {location}.icon-theme and .icon must be used together"
        )
    parsed_children = [
        parse_item(child, f"{location}.items[{index}]")
        for index, child in enumerate(children)
    ]
    assigned = {}
    for index, child in enumerate(parsed_children):
        for key in normalized_keys(child.keys):
            if key in assigned:
                raise SystemExit(
                    f"waypie: {location}.items[{assigned[key]}] and "
                    f"{location}.items[{index}] both use key {key!r}"
                )
            assigned[key] = index
    return Item(
        label=label,
        keys=keys,
        command=command,
        angle=angle,
        icon_theme=icon_theme,
        icon=icon,
        items=parsed_children,
    )


def normalized_keys(keys):
    return [character.lower() for character in keys]


def parse_keys(value, location):
    if not isinstance(value, str):
        raise SystemExit(f"waypie: {location} must be text")
    if any(character.isspace() or not character.isprintable() for character in value):
        raise SystemExit(
            f"waypie: {location} must contain only visible non-whitespace characters"
        )
    normalized = normalized_keys(value)
    if len(normalized) != len(set(normalized)):
        raise SystemExit(f"waypie: {location} contains duplicate keys")
    return value


def parse_key_sets(value, location="autogenerate-key-sets"):
    if not isinstance(value, str):
        raise SystemExit(f"waypie: {location} must be text")
    groups = value.split()
    assigned = {}
    for index, group in enumerate(groups):
        parse_keys(group, f"{location} group {index + 1}")
        for key in normalized_keys(group):
            if key in assigned:
                raise SystemExit(
                    f"waypie: {location} groups {assigned[key] + 1} and "
                    f"{index + 1} both use key {key!r}"
                )
            assigned[key] = index
    return value


def autogenerate_keys(root, key_sets):
    groups = key_sets.split()
    resolve_angles(root, root=True)
    assigned_count = 0
    for menu_index, menu in enumerate(menus_breadth_first(root)):
        root_menu = menu_index == 0
        start = 0 if root_menu else menu.return_angle
        start = 0 if start is None else start
        ordered = sorted(
            enumerate(menu.items),
            key=lambda entry: (
                (entry[1].angle - start) % 360,
                entry[0],
            ),
        )
        for number, (_index, item) in enumerate(ordered):
            item.keys = groups[number] if number < len(groups) else ""
            assigned_count += 1
    return assigned_count


def menus_breadth_first(root):
    pending = [root]
    for menu in pending:
        yield menu
        pending.extend(item for item in menu.items if item.is_submenu)


def nonnegative_number(value, location):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SystemExit(f"waypie: {location} must be a number")
    value = float(value)
    if not math.isfinite(value) or value < 0:
        raise SystemExit(f"waypie: {location} cannot be negative")
    return value


def positive_number(value, location):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SystemExit(f"waypie: {location} must be a number")
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise SystemExit(f"waypie: {location} must be positive")
    return value


def optional_nonnegative(value, location):
    return None if value is None else nonnegative_number(value, location)


def optional_number(value, location):
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SystemExit(f"waypie: {location} must be a number")
    value = float(value)
    if not math.isfinite(value):
        raise SystemExit(f"waypie: {location} must be finite")
    return value


def boolean(value, location):
    if not isinstance(value, bool):
        raise SystemExit(f"waypie: {location} must be true or false")
    return value


def resolve_angles(item, root=False):
    count = len(item.items)
    step = 360 / count if count else 0
    for index, child in enumerate(item.items):
        if child.angle is None:
            child.angle = round(index * step) % 360
        resolve_angles(child)
    if not root and item.is_submenu:
        preferred = ((item.angle or 0) + 180) % 360
        item.return_angle = largest_gap_angle(
            [child.angle % 360 for child in item.items],
            preferred,
        )
        if item.return_angle is None:
            item.return_angle = preferred
        item.return_angle = round(item.return_angle) % 360


def geometric_return_angle(item):
    return round((item.angle or 0) + 180) % 360


def centered_submenu_angles(item):
    return_angle = geometric_return_angle(item)
    step = 360 / (len(item.items) + 1)
    return return_angle, [
        round(return_angle + (index + 1) * step) % 360
        for index in range(len(item.items))
    ]


def largest_gap_angle(angles, preferred=None):
    if not angles:
        return None
    ordered = sorted(angles)
    gaps = []
    for index, start in enumerate(ordered):
        end = ordered[(index + 1) % len(ordered)]
        if index == len(ordered) - 1:
            end += 360
        gap = end - start
        gaps.append((gap, (start + gap / 2) % 360))
    largest = max(gap for gap, _angle in gaps)
    candidates = [angle for gap, angle in gaps if gap == largest]
    if preferred is None:
        return candidates[0]
    return min(
        candidates,
        key=lambda angle: abs((angle - preferred + 180) % 360 - 180),
    )


def load_styles(path=STYLE_PATH):
    path = Path(path)
    try:
        source = path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"waypie: cannot read {path}: {error}") from error

    comments = source
    while True:
        start = comments.find("/*")
        if start < 0:
            break
        if "*/" in comments[:start]:
            raise SystemExit("waypie: style contains an unmatched comment terminator")
        end = comments.find("*/", start + 2)
        if end < 0:
            raise SystemExit("waypie: style contains an unclosed comment")
        comments = comments[end + 2 :]
    if "*/" in comments:
        raise SystemExit("waypie: style contains an unmatched comment terminator")
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    depth = 0
    for character in source:
        if character == "{":
            if depth:
                raise SystemExit("waypie: nested style blocks are not supported")
            depth = 1
        elif character == "}":
            if not depth:
                raise SystemExit("waypie: style contains an unmatched closing brace")
            depth = 0
    if depth:
        raise SystemExit("waypie: style contains an unclosed block")
    rules = {}
    for selectors, declarations in re.findall(r"([^{}]+)\{([^{}]*)\}", source):
        properties = {}
        if any(
            line.strip().lower() == "off"
            for statement in declarations.split(";")
            for line in statement.splitlines()
        ):
            properties["off"] = "true"
        declarations = "\n".join(
            line for line in declarations.splitlines() if line.strip().lower() != "off"
        )
        for declaration in declarations.split(";"):
            if declaration.strip() and ":" not in declaration:
                raise SystemExit(
                    f"waypie: invalid style declaration: {declaration.strip()}"
                )
            if ":" not in declaration:
                continue
            name, value = declaration.split(":", 1)
            properties[name.strip().lower()] = value.strip()
        for selector in selectors.split(","):
            rules.setdefault(selector.strip().lower(), {}).update(properties)
    # Ordinary radial distance is menu geometry. State selectors such as
    # circle.active may still override it for visual interaction.
    rules.get("circle", {}).pop("distance", None)
    base_circle = computed_style(rules, ("circle",))
    for name in ("width",):
        if base_circle[name] is None:
            raise SystemExit(f"waypie: style.css requires circle {{ {name}: ...; }}")
    validate_styles(rules)
    return rules


def validate_styles(rules):
    for selector in rules:
        if selector == "animation":
            unknown = set(rules[selector]) - ANIMATION_PROPERTIES
            if unknown:
                raise SystemExit(f"waypie: unknown animation property: {min(unknown)}")
            for name in (
                "color-duration",
                "opacity-duration",
                "icon-duration",
                "connector-duration",
                "item-delete-duration",
                "close-duration",
            ):
                animation_duration(rules, name)
            for prefix in (
                "hover",
                "follow",
                "menu-move",
                "item-create",
                "action",
                "submenu-indicator",
            ):
                spring_duration(rules, prefix)
            animation_number(rules, "action-scale", 1.3)
        elif selector in {"item-key", "item-key.active"}:
            unknown = set(rules[selector]) - ITEM_KEY_PROPERTIES
            if unknown:
                raise SystemExit(f"waypie: unknown {selector} property: {min(unknown)}")
            computed_item_key_style(rules, selector == "item-key.active")
        else:
            if selector not in CIRCLE_SELECTORS:
                raise SystemExit(f"waypie: unknown style selector: {selector}")
            unknown = set(rules[selector]) - CIRCLE_PROPERTIES
            if unknown:
                raise SystemExit(f"waypie: unknown {selector} property: {min(unknown)}")
            style = computed_style(rules, (selector,))
            resolve_radius(style["border-radius"], style["width"] or 1.0)


def computed_style(rules, selectors):
    style = dict(DEFAULT_STYLE)
    for selector in selectors:
        for name, value in rules.get(selector, {}).items():
            if name in {"background-color", "border-color", "color"}:
                style[name] = parse_color(value, name)
            elif name == "distance":
                style[name] = parse_signed_pixels(value, name)
            elif name == "follow-distance":
                style[name] = parse_percentage(value, name)
            elif name in {"icon-fill", "text-fill"}:
                style[name] = parse_percentage(value, name, bounded=False)
            elif name == "cut-indicators":
                normalized = value.lower()
                if normalized not in {"true", "false"}:
                    raise SystemExit(
                        "waypie: style.css cut-indicators must be true or false"
                    )
                style[name] = normalized == "true"
            elif name in {
                "border-width",
                "font-size",
                "icon-size",
                "protrusion",
                "width",
            }:
                style[name] = parse_pixels(value, name)
            elif name in {"opacity", "text-opacity"}:
                style[name] = parse_opacity(value)
            elif name == "scale":
                style[name] = positive_number_string(value, name)
            elif name == "border-radius":
                style[name] = value
            elif name == "font-family":
                style[name] = value.strip("\"'")
            elif name == "font-weight":
                style[name] = parse_font_weight(value)
    return style


def computed_item_key_style(rules, active=False):
    style = {
        "angle": 0.0,
        "color": (1.0, 1.0, 1.0, 1.0),
        "distance": 8.0,
        "enabled": True,
        "font-family": "Sans",
        "font-size": 12.0,
        "font-weight": 600,
        "opacity": 1.0,
        "scale": 1.0,
    }
    selectors = ("item-key", "item-key.active") if active else ("item-key",)
    for selector in selectors:
        for name, value in rules.get(selector, {}).items():
            if name == "angle":
                angle = value.strip().removesuffix("deg").strip()
                try:
                    angle = float(angle)
                except ValueError:
                    raise SystemExit(f"waypie: invalid angle: {value}") from None
                if not math.isfinite(angle) or not 0 <= angle < 360:
                    raise SystemExit("waypie: angle must be between 0 and 359 degrees")
                style[name] = angle
            elif name == "color":
                style[name] = parse_color(value, name)
            elif name == "distance":
                style[name] = parse_signed_pixels(value, name)
            elif name == "off":
                style["enabled"] = False
            elif name == "font-family":
                style[name] = value.strip("\"'")
            elif name == "font-size":
                style[name] = parse_pixels(value, name)
            elif name == "font-weight":
                style[name] = parse_font_weight(value)
            elif name == "opacity":
                style[name] = parse_opacity(value)
            elif name == "scale":
                style[name] = positive_number_string(value, name)
    return style


def content_opacity(style):
    value = style["text-opacity"]
    return style["opacity"] if value is None else value


def scaled_icon_size(style, circle_size):
    icon_fill = style.get("icon-fill")
    if icon_fill is not None:
        inner_size = max(0.0, circle_size - 2 * style.get("border-width", 0.0))
        return inner_size * icon_fill
    icon_size = style.get("icon-size")
    if icon_size is None:
        return circle_size * 0.55
    base_circle_size = style.get("width")
    if base_circle_size is None or base_circle_size <= 0:
        return icon_size
    return icon_size * circle_size / base_circle_size


def colored_svg_source(path, color):
    red, green, blue, _alpha = color
    replacement = (
        f"#{round(red * 255):02x}{round(green * 255):02x}{round(blue * 255):02x}"
    )
    source = path.read_text(encoding="utf-8")
    if "currentColor" in source:
        return source.replace("currentColor", replacement)
    if not re.search(
        r"""(?:fill|stroke)\s*=\s*["'](?:#|rgb|hsl)""",
        source,
        re.IGNORECASE,
    ):
        return source.replace("<svg", f'<svg fill="{replacement}"', 1)
    return source


def icon_themes():
    try:
        return sorted(
            entry.name
            for entry in ICON_DIR.iterdir()
            if entry.is_dir() and not entry.name.startswith(".")
        )
    except OSError:
        return []


def load_icon_theme_history():
    try:
        source = json.loads(ICON_HISTORY_PATH.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return {}
    if not isinstance(source, dict):
        return {}
    return {
        theme: selected_at
        for theme, selected_at in source.items()
        if isinstance(theme, str)
        and not isinstance(selected_at, bool)
        and isinstance(selected_at, (int, float))
    }


def sort_icon_themes(themes, history):
    return sorted(
        themes,
        key=lambda theme: (-history.get(theme, 0), theme.casefold(), theme),
    )


def remember_icon_theme(theme):
    history = load_icon_theme_history()
    selected_at = time.time_ns()
    if history:
        selected_at = max(selected_at, max(history.values()) + 1)
    history[theme] = selected_at
    try:
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        temporary = ICON_HISTORY_PATH.with_suffix(".json.tmp")
        temporary.write_text(
            json.dumps(history, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        temporary.replace(ICON_HISTORY_PATH)
    except OSError:
        pass


def theme_icons(theme):
    directory = ICON_DIR / theme
    if not directory.is_dir() or directory.parent != ICON_DIR:
        return []
    try:
        return sorted(
            path.relative_to(directory).as_posix()
            for path in directory.rglob("*")
            if path.is_file() and path.suffix.lower() in ICON_EXTENSIONS
        )
    except OSError:
        return []


def icon_path(theme, icon):
    if not theme or not icon:
        return None
    directory = (ICON_DIR / theme).resolve()
    try:
        path = (directory / icon).resolve()
        path.relative_to(directory)
    except (OSError, ValueError):
        return None
    return path if path.is_file() and path.suffix.lower() in ICON_EXTENSIONS else None


def parse_pixels(value, name):
    match = re.fullmatch(rf"({CSS_NUMBER})(?:px)?", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    number = float(match.group(1))
    if not math.isfinite(number):
        raise SystemExit(f"waypie: invalid {name}: {value}")
    return max(0.0, number)


def parse_signed_pixels(value, name):
    match = re.fullmatch(rf"({CSS_NUMBER})(?:px)?", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    number = float(match.group(1))
    if not math.isfinite(number):
        raise SystemExit(f"waypie: invalid {name}: {value}")
    return number


def parse_percentage(value, name, bounded=True):
    match = re.fullmatch(rf"({CSS_NUMBER})%", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    percentage = float(match.group(1))
    if not math.isfinite(percentage):
        raise SystemExit(f"waypie: invalid {name}: {value}")
    if percentage < 0:
        raise SystemExit(f"waypie: {name} cannot be negative")
    if bounded and percentage > 100:
        raise SystemExit(f"waypie: {name} must be between 0% and 100%")
    return percentage / 100


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
                channels = [float(part) for part in parts]
                if any(not math.isfinite(channel) for channel in channels):
                    raise ValueError
                rgb = [max(0, min(255, channel)) / 255 for channel in channels[:3]]
                alpha = max(0.0, min(1.0, channels[3])) if len(parts) == 4 else 1.0
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


def parse_font_weight(value):
    normalized = value.strip().lower()
    if normalized == "normal":
        return 400
    if normalized == "bold":
        return 700
    try:
        weight = int(normalized)
    except ValueError:
        raise SystemExit(f"waypie: invalid font-weight: {value}") from None
    if not 1 <= weight <= 1000:
        raise SystemExit("waypie: font-weight must be between 1 and 1000")
    return weight


def animation_duration(rules, name, fallback_name=None):
    if animations_off(rules):
        return 0.0
    animation = rules.get("animation", {})
    value = animation.get(name)
    if value is None and fallback_name is not None:
        value = animation.get(fallback_name)
    value = (value or "0ms").strip().lower()
    match = re.fullmatch(rf"({CSS_NUMBER})\s*(ms|s)", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    duration, unit = match.groups()
    seconds = float(duration)
    if seconds < 0:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    seconds = seconds / 1000 if unit == "ms" else seconds
    if not math.isfinite(seconds) or seconds > MAX_ANIMATION_SECONDS:
        raise SystemExit(f"waypie: {name} cannot exceed one hour")
    return seconds


def spring_duration(rules, prefix):
    if animations_off(rules):
        return 0.0
    damping = animation_number(rules, f"{prefix}-damping-ratio", 1.0)
    stiffness = animation_number(rules, f"{prefix}-stiffness", 1000.0)
    epsilon = animation_number(rules, f"{prefix}-epsilon", 0.0001)
    if not 0.1 <= damping <= 10:
        raise SystemExit(f"waypie: {prefix}-damping-ratio must be between 0.1 and 10")
    if epsilon >= 1:
        raise SystemExit(f"waypie: {prefix}-epsilon must be less than 1")

    def error(seconds):
        if damping < 1:
            return math.exp(-damping * seconds) / math.sqrt(1 - damping * damping)
        if damping == 1:
            return (1 + seconds) * math.exp(-seconds)
        root = math.sqrt(damping * damping - 1)
        first = -(damping - root)
        second = -(damping + root)
        first_weight = -second / (second - first)
        second_weight = first / (second - first)
        return abs(
            first_weight * math.exp(first * seconds)
            + second_weight * math.exp(second * seconds)
        )

    low, high = 0.0, 1.0
    while error(high) > epsilon and high < 1_000_000:
        high *= 2
    for _ in range(64):
        middle = (low + high) / 2
        if error(middle) > epsilon:
            low = middle
        else:
            high = middle
    duration = high / math.sqrt(stiffness)
    if not math.isfinite(duration):
        raise SystemExit(
            f"waypie: {prefix} spring parameters produce an unsupported duration"
        )
    if duration > MAX_ANIMATION_SECONDS:
        raise SystemExit(f"waypie: {prefix} spring duration cannot exceed one hour")
    return duration


def spring_value(rules, prefix, progress):
    progress = min(1.0, max(0.0, progress))
    if animations_off(rules):
        return 1.0
    if progress in {0.0, 1.0}:
        return progress
    damping = animation_number(rules, f"{prefix}-damping-ratio", 1.0)
    duration = spring_duration(rules, prefix)
    stiffness = animation_number(rules, f"{prefix}-stiffness", 1000.0)
    elapsed = progress * duration * math.sqrt(stiffness)
    if damping < 1:
        root = math.sqrt(1 - damping * damping)
        decay = math.exp(-damping * elapsed)
        return 1 - decay * (
            math.cos(root * elapsed) + damping / root * math.sin(root * elapsed)
        )
    if damping == 1:
        return 1 - (1 + elapsed) * math.exp(-elapsed)
    root = math.sqrt(damping * damping - 1)
    first = -(damping - root)
    second = -(damping + root)
    first_weight = -second / (second - first)
    second_weight = first / (second - first)
    return (
        1
        - first_weight * math.exp(first * elapsed)
        - second_weight * math.exp(second * elapsed)
    )


def animation_number(rules, name, default):
    value = rules.get("animation", {}).get(name)
    if value is None:
        return default
    return positive_number_string(value.strip(), name)


def animations_off(rules):
    return "off" in rules.get("animation", {})


def resolve_radius(value, size):
    value = value.strip().lower()
    if value.endswith("%"):
        try:
            percentage = float(value[:-1])
            if math.isfinite(percentage):
                return min(size / 2, max(0, size * percentage / 100))
        except ValueError:
            pass
    return min(size / 2, parse_pixels(value, "border-radius"))


def set_source_color(context, color, opacity):
    red, green, blue, alpha = color
    context.set_source_rgba(red, green, blue, alpha * opacity)


def draw_connector(context, start, end, style, opacity=1.0):
    width = style.get("width")
    if width is None or width <= 0 or opacity <= 0:
        return
    delta_x = end[0] - start[0]
    delta_y = end[1] - start[1]
    length = math.hypot(delta_x, delta_y)
    start_radius = start[2] / 2
    end_radius = end[2] / 2
    if length <= start_radius + end_radius or length <= 0:
        return
    direction_x = delta_x / length
    direction_y = delta_y / length
    context.set_line_width(width)
    set_source_color(context, style["color"], style["opacity"] * opacity)
    context.move_to(
        start[0] + direction_x * start_radius,
        start[1] + direction_y * start_radius,
    )
    context.line_to(
        end[0] - direction_x * end_radius,
        end[1] - direction_y * end_radius,
    )
    context.stroke()


def rounded_rectangle(context, x, y, width, height, radius):
    radius = min(radius, width / 2, height / 2)
    context.new_sub_path()
    context.arc(x + width - radius, y + radius, radius, -math.pi / 2, 0)
    context.arc(x + width - radius, y + height - radius, radius, 0, math.pi / 2)
    context.arc(x + radius, y + height - radius, radius, math.pi / 2, math.pi)
    context.arc(x + radius, y + radius, radius, math.pi, 3 * math.pi / 2)
    context.close_path()


def fit_text_prefix(text, width, measure):
    end = 0
    for index in range(1, len(text) + 1):
        if measure(text[:index]) > width:
            break
        end = index
    return end


def middle_ellipsis(text, width, measure):
    suffix = text[-3:]
    marker = f"...{suffix}"
    if measure(marker) > width:
        return "." * fit_text_prefix("...", width, measure)
    prefix_limit = fit_text_prefix(text, width - measure(marker), measure)
    prefix = text[:prefix_limit].rstrip()
    while prefix and measure(f"{prefix}{marker}") > width:
        prefix = prefix[:-1].rstrip()
    return f"{prefix}{marker}"


def wrap_text_to_widths(text, widths, measure):
    remaining = " ".join(text.split())
    lines = []
    if not remaining:
        return lines, True
    for index, width in enumerate(widths):
        if width <= 0:
            lines.append("")
            continue
        if measure(remaining) <= width:
            lines.append(remaining)
            return lines, True
        if index == len(widths) - 1:
            lines.append(middle_ellipsis(remaining, width, measure))
            return lines, False
        prefix_length = fit_text_prefix(remaining, width, measure)
        if prefix_length == 0:
            lines.append("")
            continue
        prefix = remaining[:prefix_length]
        if prefix_length < len(remaining) and remaining[prefix_length].isspace():
            lines.append(prefix.rstrip())
            remaining = remaining[prefix_length:].lstrip()
            continue
        word_break = prefix.rfind(" ")
        if word_break > 0:
            lines.append(prefix[:word_break].rstrip())
            remaining = remaining[word_break + 1 :].lstrip()
        else:
            lines.append(prefix.rstrip())
            remaining = remaining[prefix_length:].lstrip()
    return lines, not remaining


def fixed_text_geometry(style, circle_size, base_scale):
    layout_size = style["width"] * base_scale
    if layout_size <= 0:
        return 0.0, 0.0
    return layout_size, min(1.0, max(0.0, circle_size / layout_size))


def draw_wrapped_text(
    context,
    x,
    y,
    layout_size,
    text,
    style,
    opacity=1.0,
    visual_scale=1.0,
):
    if not text or layout_size <= 0 or visual_scale <= 0:
        return
    context.save()
    context.translate(x, y)
    context.scale(visual_scale, visual_scale)
    context.select_font_face(
        style["font-family"],
        0,
        1 if style["font-weight"] >= 600 else 0,
    )
    context.set_font_size(style["font-size"])
    font_ascent, font_descent, font_height, _max_x, _max_y = context.font_extents()
    line_height = max(font_height, style["font-size"])
    border_width = style["border-width"]
    full_inner_half = max(0.0, layout_size / 2 - border_width)
    inner_half = full_inner_half * style.get("text-fill", 1.0)
    if inner_half <= 0 or line_height <= 0:
        context.restore()
        return
    corner_radius = max(
        0.0,
        resolve_radius(style["border-radius"], layout_size)
        - border_width
        - (full_inner_half - inner_half),
    )
    max_lines = max(1, int(2 * inner_half // line_height))
    measure = lambda value: context.text_extents(value).width

    def widths_for(line_count):
        widths = []
        glyph_half_height = min(
            line_height / 2,
            (font_ascent + font_descent) / 2,
        )
        for index in range(line_count):
            offset = (index - (line_count - 1) / 2) * line_height
            vertical_extent = abs(offset) + glyph_half_height
            if vertical_extent > inner_half:
                widths.append(0.0)
                continue
            straight_half_height = inner_half - corner_radius
            if corner_radius == 0 or vertical_extent <= straight_half_height:
                horizontal_half = inner_half
            else:
                corner_y = vertical_extent - straight_half_height
                horizontal_half = (
                    inner_half
                    - corner_radius
                    + math.sqrt(max(0.0, corner_radius**2 - corner_y**2))
                )
            widths.append(2 * horizontal_half)
        return widths

    lines = []
    for line_count in range(1, max_lines + 1):
        candidate, complete = wrap_text_to_widths(
            text,
            widths_for(line_count),
            measure,
        )
        lines = candidate
        if complete:
            break
    if not lines:
        context.restore()
        return

    set_source_color(context, style["color"], content_opacity(style) * opacity)
    for index, line in enumerate(lines):
        if not line:
            continue
        extents = context.text_extents(line)
        offset = (index - (len(lines) - 1) / 2) * line_height
        context.move_to(
            -extents.width / 2 - extents.x_bearing,
            offset - extents.y_bearing - extents.height / 2,
        )
        context.show_text(line)
    context.restore()


def direction_angle(x, y):
    """Return clockwise degrees where zero points upwards."""
    return math.degrees(math.atan2(x, -y)) % 360


def angular_distance(first, second):
    return abs((first - second + 180) % 360 - 180)


def angular_delta(target, start):
    return (target - start + 180) % 360 - 180

import math
import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

CONFIG_DIR = Path.home() / ".config" / "waypie"
CONFIG_PATH = CONFIG_DIR / "config"
STYLE_PATH = CONFIG_DIR / "style.css"
ICON_DIR = CONFIG_DIR / "icons"
ICON_EXTENSIONS = {".svg", ".png", ".webp", ".jpg", ".jpeg", ".gif"}


@dataclass
class Item:
    label: str
    command: str | None = None
    angle: float | None = None
    return_angle: float | None = None
    icon_theme: str | None = None
    icon: str | None = None
    items: list["Item"] = field(default_factory=list)


@dataclass
class Settings:
    menu_radius: float
    center_hitbox_size: float | None
    minimum_edge_distance: float
    center_mode: bool
    active_label_in_center: bool
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
    "distance": None,
    "font-size": 14.0,
    "font-family": "Sans",
    "follow-distance": 0.0,
    "icon-size": None,
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
    active_label_in_center = boolean(
        source.get("active-label-in-center", False),
        "active-label-in-center",
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
        active_label_in_center,
        preserve_proportions,
        auto_alignment,
        configurator_show_icons,
        root,
    )


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
    return Item(
        label=label,
        command=command,
        angle=angle,
        icon_theme=icon_theme,
        icon=icon,
        items=[
            parse_item(child, f"{location}.items[{index}]")
            for index, child in enumerate(children)
        ],
    )


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
    extra_items = 0 if root else 1
    count = len(item.items) + extra_items
    step = 360 / count if count else 0
    for index, child in enumerate(item.items):
        if child.angle is None:
            child.angle = round(index * step) % 360
        resolve_angles(child)
    if not root and item.items:
        item.return_angle = (item.angle + 180) % 360


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
    # Ordinary radial distance is menu geometry. State selectors such as
    # circle.active may still override it for visual interaction.
    rules.get("circle", {}).pop("distance", None)
    base_circle = computed_style(rules, ("circle",))
    for name in ("width",):
        if base_circle[name] is None:
            raise SystemExit(f"waypie: style.css requires circle {{ {name}: ...; }}")
    return rules


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
            elif name in {
                "border-width",
                "font-size",
                "icon-size",
                "width",
            }:
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


def icon_themes():
    try:
        return sorted(
            entry.name
            for entry in ICON_DIR.iterdir()
            if entry.is_dir() and not entry.name.startswith(".")
        )
    except OSError:
        return []


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
    match = re.fullmatch(r"(-?(?:\d+(?:\.\d*)?|\.\d+))(?:px)?", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    return max(0.0, float(match.group(1)))


def parse_signed_pixels(value, name):
    match = re.fullmatch(r"(-?(?:\d+(?:\.\d*)?|\.\d+))(?:px)?", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    return float(match.group(1))


def parse_percentage(value, name):
    match = re.fullmatch(r"(\d+(?:\.\d*)?|\.\d+)%", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    percentage = float(match.group(1))
    if percentage > 100:
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


def animation_duration(rules, name):
    value = rules.get("animation", {}).get(name, "0ms").strip().lower()
    match = re.fullmatch(r"(\d+(?:\.\d*)?|\.\d+)(ms|s)", value)
    if not match:
        raise SystemExit(f"waypie: invalid {name}: {value}")
    duration, unit = match.groups()
    seconds = float(duration)
    return seconds / 1000 if unit == "ms" else seconds


def ease_out_cubic(progress):
    return 1 - (1 - progress) ** 3


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


def angular_delta(target, start):
    return (target - start + 180) % 360 - 180

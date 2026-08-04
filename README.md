# Waypie

<img width="960" height="540" alt="Waypie menu" src="https://github.com/user-attachments/assets/ef3cfe74-ffd4-4c3b-8ef9-32a8a9b49c32" />
<img width="960" height="540" alt="Waypie configurator" src="https://github.com/user-attachments/assets/b534ad4f-f8cf-4159-a3a7-6fad2846d2d6" />

Waypie is a lightweight Kando-like radial menu for Wayland. The menu runtime is
written in Rust and uses a native layer-shell surface. The graphical
configurator remains a Python/GTK 4 application.

Waypie supports arbitrarily nested submenus, animated navigation, angular
selection, Hover Mode, Turbo Mode, CSS-like styling, monochrome and color
icons, and multiple monitors.

> [!WARNING]
> Waypie is experimental software, primarily built with AI.

## Requirements

The runtime requires:

- a Wayland compositor with the `wlr-layer-shell` protocol;
- Rust and Cargo to build from source;
- `libxkbcommon` and Fontconfig at runtime.

The configurator additionally requires Python 3.11 or newer, GTK 4, PyGObject,
Pycairo, and pipx.

On Arch Linux, the required packages can be installed with:

```sh
sudo pacman -S --needed git rust cargo libxkbcommon fontconfig python python-pipx gtk4 python-gobject python-cairo
```

Package names differ between distributions.

## Installation

Clone the Rust branch and run the Makefile:

```sh
git clone --branch rust-rewrite --single-branch https://github.com/rywby-dot/waypie.git
cd waypie
make install
```

By default this does four things:

1. builds the Rust crate in release mode using the committed `Cargo.lock`;
2. installs the Rust binary as `~/.local/bin/waypie`;
3. installs the Python configurator as `waypie-config` using pipx;
4. installs missing example files and bundled icons under
   `~/.config/waypie/` without replacing existing user files.

Make sure `~/.local/bin` is in `PATH`. For a custom binary prefix:

```sh
make install-runtime PREFIX=/usr/local
```

This may require root privileges for a system directory. The configurator and
user configuration can also be installed separately:

```sh
make install-configurator
make install-config
```

### Updating

```sh
cd waypie
git pull
make install
```

Existing `config`, `style.css`, and icons are preserved.

### Uninstalling

```sh
make uninstall
```

This removes the installed Rust binary and pipx configurator. User data in
`~/.config/waypie` is intentionally retained.

## Running the menu

Add this command to a compositor key binding:

```sh
waypie --show
```

Waypie is not a persistent daemon. Each invocation starts the native Rust
runtime, opens the menu, and exits after the menu closes. While a menu is open,
a small per-display control socket allows another `waypie --show` invocation to
close it. No Waypie process remains in memory afterward.

Running `waypie` without arguments also opens the menu. Available commands are:

```text
waypie --show       Open the menu, or close the currently open instance
waypie --configure  Open the graphical configurator
waypie --kill       Ask the currently open instance to close
```

The complete menu can be closed with:

- `Escape`;
- right click anywhere on the layer;
- another `waypie --show`;
- the root menu's central hitbox;
- a submenu center when **Close on click** is enabled;
- selection of a command.

Closing immediately makes the layer click-through, then plays the configured
closing animation. Selecting a command also activates it at the beginning of
the animation, so Waypie never delays interaction with the launched program.

If `center-mode` is disabled, the first menu is placed at the pointer position
reported by the compositor. Some compositors do not provide that position
until the pointer moves. Enable **Center mode** if the menu must always appear
immediately at the center of the active output.

## Selection and navigation

The visible circles are indicators, not ordinary button hitboxes. Apart from
the optional central hitbox, selection is determined by the pointer direction
from the current menu center. The complete angular sector remains selectable
even when the pointer is beyond the visible circle.

Opening a submenu moves its circle to the new center. Its children grow and
spread out from that moving circle. Previous menus remain connected by lines
and can be selected by their return direction. Clicking a submenu center
returns to its parent unless **Close on click** is enabled.

### Hover Mode

Enable `hover-mode` in the configurator to navigate without clicking. After a
sufficiently long pointer stroke, the current direction is selected when the
pointer pauses or turns sharply. Hover Mode can open submenus, return through
the menu chain, and execute commands.

The gesture constants are grouped near the top of
`src/hover.rs` for developers who want to tune them.

### Turbo Mode

Enable `turbo-mode` to use the modifier held by the compositor shortcut as a
temporary mouse button:

1. Press a shortcut such as `Super+R`, `Alt+Space`, or `Ctrl+Shift+Space`.
2. Keep its modifier held while navigating through the menu.
3. Point toward the final action.
4. Release the last modifier to execute it.

Waypie reads the modifiers delivered through the Wayland keyboard protocol, so
the shortcut does not need to be duplicated in `config`. Support depends on the
compositor forwarding modifier press and release events to the layer.

## Graphical configurator

<img width="960" height="540" alt="Graphical configurator" src="https://github.com/user-attachments/assets/2acca4ac-9ee7-49b3-a194-94b79a1f2b4c" />

Open it with either command:

```sh
waypie-config
```

```sh
waypie --configure
```

Use the GUI to add, remove, reorder, and position actions and submenus; edit
labels and commands; select icons; and change runtime geometry and interaction
settings.

The main configurator options are:

- **Preserve proportions** keeps items evenly distributed while rotating a
  group. A submenu's return direction occupies one invisible slot.
- **Auto alignment** snaps group rotation to a 5-degree grid.
- **Show icons** changes only the configurator preview.
- **Center layout** performs one equal-spacing operation independently of the
  two alignment checkboxes.
- **Center mode**, **Hover mode**, **Turbo mode**, and **Close on click** control
  the corresponding runtime behavior.

Shortcuts work while focus is outside text-entry fields:

- `Ctrl+D` — delete the selected item;
- `Ctrl+Q` — add a command;
- `Ctrl+X` — add a submenu;
- `Ctrl+S` — save;
- `Ctrl+A` — center the current layout;
- `Ctrl+Z` — undo the last edit.

Press **Save** to write `~/.config/waypie/config`; the previous version is saved
as `config.bak`. Because the Rust runtime starts fresh for every opening, the
next `waypie --show` automatically reads the new configuration. No daemon
restart or hot-reload step is required.

The configurator uses application ID `waypie.config`. Its icon picker is a
modal transient window of the same application.

## Configuration

Runtime geometry and behavior are stored in:

```text
~/.config/waypie/config
```

The GUI manages these settings:

- `menu-radius` — normal distance from the current center to its items;
- `center-hitbox-size` — diameter of the central hitbox; `0` disables it;
- `minimum-edge-distance` — minimum safe distance for a newly opened menu
  center from an output edge;
- `center-mode`, `hover-mode`, `turbo-mode`, and
  `close-submenu-on-center-click` — runtime switches;
- `preserve-proportions`, `auto-alignment`, and `configurator-show-icons` —
  configurator-only persistent preferences.

Each menu item can contain a `label`, `angle`, optional icon pair
(`icon-theme` and `icon`), and either a `command` or nested `items`. Empty
submenus are valid. Angles are rounded to whole degrees when saved and loaded.

## Styling

All runtime colors, sizes, borders, fonts, opacity, indicators, connector
appearance, and animation parameters are stored in:

```text
~/.config/waypie/style.css
```

The GUI reads this file for its preview but never rewrites it. Since every menu
opening is a new process, saving `style.css` is enough; the next opening uses
the new style.

The cascade starts with `circle` and can be overridden by:

- `circle.active`;
- `circle.item` and `circle.item.active`;
- `circle.submenu` and `circle.submenu.active`;
- `circle.center` and `circle.center.active`;
- `circle.history` and `circle.history.active`;
- `submenu-indicator` and `submenu-indicator.active`.

The `animation` block independently controls hover, icon fading, menu movement,
item creation, closing, and selected-action durations and spring parameters.
See `style.example.css` for every supported property.

## Icons

Waypie scans immediate child directories of:

```text
~/.config/waypie/icons/
```

Each child directory is an icon theme, and files below it are scanned
recursively. Supported formats are SVG, PNG, WebP, JPEG, and GIF.

```text
~/.config/waypie/icons/
├── tabler-icons/
│   ├── outline/terminal.svg
│   └── filled/calculator.svg
└── Papirus-Dark/
    └── 16x16/actions/configuration.svg
```

Use the searchable icon picker in the configurator to select a theme and icon,
or search all themes at once. Themes are ordered by their recent selection
history stored in `~/.config/waypie/.icon-history.json`.

Monochrome SVG files using `currentColor` inherit the circle's CSS `color`.
Suitable monochrome collections include Tabler Icons, Lucide, Material
Symbols, and Simple Icons. Papirus and other desktop icon themes provide large
full-color collections.

At runtime, an assigned icon is shown normally and fades when it is replaced by
the active label. Text appears immediately. If an item or central menu has no
icon, its text remains visible in every state.

## Development

Run all Rust and Python checks with:

```sh
make check
```

Build only the optimized runtime with:

```sh
make build
```

See `CONTRIBUTING.md` for more details.

## Inspired by

- [Kando](https://github.com/kando-menu/kando)
- [Driftmap](https://github.com/rywby-dot/driftwm-minimap)
- [Touchview](https://github.com/malbiruk/touchview)
- [Driftwm](https://github.com/malbiruk/driftwm)
- [Wlogout](https://github.com/ArtsyMacaw/wlogout)

https://github.com/user-attachments/assets/fddcc941-bbec-448d-bd93-f253433c687d

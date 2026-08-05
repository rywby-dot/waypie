# Waypie

- Waypie is a lightweight and fast Kando-like radial menu for Wayland, written with Rust
- It is not need a daemon
- Support Point and Click Mode, Marking Mode, Turbo Mode, Hover Mode, Centered Mode [from Kando](https://kando.menu/usage/)
- It is also recommended to [read about Kando](https://kando.menu/intro/) first, before using Waypie 

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
```sh
git clone https://github.com/rywby-dot/waypie.git
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

The configurator and user configuration can also be installed separately:

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

To deliberately replace both configuration files with the repository defaults:

```sh
make forceinstall
```

run `make help` to see avalible options

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
or
```sh
waypie
```

Available commands are:

```text
waypie --show       Open the menu, or close the currently open instance
waypie --configure  Open the graphical configurator
waypie --kill       Ask the currently open instance to close
waypie --help       Show help menu
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
- **Center mode**, **Hover mode**, **Turbo mode**, **Hold to turbo**,
  **Travel item animation**, and **Close on click** control the corresponding
  runtime behavior.

Shortcuts work while focus is outside text-entry fields:

- `Ctrl+D` — delete the selected item;
- `Ctrl+Q` — add a command;
- `Ctrl+X` — add a submenu;
- `Ctrl+S` — save;
- `Ctrl+A` — center the current layout;
- `Ctrl+Z` — undo the last edit.

Press **Save** to write `~/.config/waypie/config`. If `config.bak` does not yet
exist, the configurator creates it from the previous configuration. An existing
backup — including one made by `make forceinstall` — is never overwritten by
the configurator. Because the Rust runtime starts fresh for every opening, the
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
- `center-mode`, `hover-mode`, `turbo-mode`, `hold-to-turbo`,
  `travel-item-animation`, and
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

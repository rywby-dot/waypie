# Waypie

* Waypie is a lightweight and fast [Kando](https://kando.menu/)-like radial menu for Wayland, written in Rust.
* It does not require a daemon.
* It supports Point-and-Click Mode, Marking Mode, Turbo Mode, Hover Mode, and Centered Mode, inspired by [Kando's interaction modes](https://kando.menu/usage/).
* It is recommended that you [read about Kando](https://kando.menu/intro/) before using Waypie.


https://github.com/user-attachments/assets/9a103a3d-474e-484e-b687-93899f1d0e1b

<img width="960" height="540" alt="ScreenShot-2026-08-05_16-59-58" src="https://github.com/user-attachments/assets/8e091dbd-4aa2-4a95-9c4c-378b9c34664b" />

> [!WARNING]
> Waypie is experimental software and was developed primarily with the assistance of AI.

## Requirements

The runtime requires:

* a Wayland compositor that supports the `wlr-layer-shell` protocol (Tested on sway, niri, driftwm, hevel, labwc. Others also should work fine);
* Rust and Cargo to build the project from source;
* `libxkbcommon` and Fontconfig at runtime.

The configurator additionally requires Python 3.11 or newer, GTK 4, PyGObject,
Pycairo, and pipx.

### Arch Linux

```sh
sudo pacman -S git rust cargo libxkbcommon fontconfig python python-pipx gtk4 python-gobject python-cairo
```

### Void Linux

```sh
sudo xbps-install -S git rust cargo libxkbcommon-devel fontconfig-devel python3 python3-pipx gtk4-devel python3-gobject python3-cairo
```

### Debian

```sh
sudo apt install -y git rustc cargo libxkbcommon-dev libfontconfig-dev python3 pipx libgtk-4-dev python3-gi python3-cairo python3-gi-cairo
```

Package names may differ between distributions.

## Installation

```sh
git clone https://github.com/rywby-dot/waypie.git
cd waypie
make install
```

By default, this command performs four actions:

1. builds the Rust crate in release mode using the committed `Cargo.lock`;
2. installs the Rust binary as `~/.local/bin/waypie`;
3. installs the Python configurator as `waypie-config` using pipx;
4. installs any missing example files and bundled icons under
   `~/.config/waypie/` without replacing existing user files.

Make sure that `~/.local/bin` is included in your `PATH`, for example, by
configuring it in `~/.zshrc`.

A custom binary installation prefix is also supported. Run `make help` for
details.

The configurator and user configuration files can also be installed separately:

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

Existing configuration, style, and icon files are preserved.

To deliberately replace the configuration and bundled style files with the
repository defaults, run:

```sh
make forceinstall
```

Run `make help` to see the other available options.

### Uninstalling

```sh
make uninstall
```

This removes the installed Rust binary and the pipx-managed configurator. User
data in `~/.config/waypie/` is intentionally retained.

## Running the menu

Add one of the following commands to a compositor key binding:

```sh
waypie --show
```

or:

```sh
waypie
```

Available commands:

```text
waypie --show       Open the menu or close the currently open instance
waypie --configure  Open the graphical configurator
waypie --kill       Ask the currently open instance to close
waypie --help       Show the help message
```

The entire menu can be closed by:

* pressing `Escape`;
* right-clicking anywhere on the layer;
* running `waypie --show` again;
* clicking the root menu's central hitbox;
* clicking the center of a submenu when **Close on click** is enabled;
* selecting a command.

When closing, Waypie immediately makes the layer click-through and then plays
the configured closing animation. When a command is selected, it is also
activated at the beginning of the animation, so Waypie never delays interaction
with the launched program.

When `center-mode` is disabled, the root menu is placed at the pointer position
reported by the compositor. Some compositors do not provide this position until
the pointer moves. Enable **Center mode** if the menu must always appear
immediately in the center of the active output.

Enable **Always center mode** (`always-center-mode`) to additionally keep the
active menu anchored at that output center while navigating. This corresponds
to Kando's Anchored Mode: opening or closing a submenu moves the surrounding
items, history circles, and connectors while the current menu center stays
fixed.

Items can also be selected directly from the keyboard. Set `keys` on an action
or submenu; every Unicode character in the string is an independent activation
key, and letters are matched case-insensitively. Digits and symbols are also
supported. Only children of the currently open menu participate, so a key
assigned to the submenu itself never acts as a return key after that submenu
has opened. `back-keys` returns to the parent, or closes the root menu. If an
item and `back-keys` share a character, the current item takes priority.

The configurator can fill these shortcuts for the whole tree. Enter
space-separated alternatives such as `aф rы sв tа gп` in **Autogenerate key
sets**, then press **Autogenerate**. Root items are numbered clockwise from
`0°`; submenu items are numbered clockwise from their return connector. The
connector itself is not an item and continues to use `back-keys`. The template
is saved as `autogenerate-key-sets` and remains available the next time the
configurator opens.

```toml
back-keys = "qй"
autogenerate-key-sets = "aф rы sв tа gп"

[[menu.items]]
label = "Applications"
keys = "aф1"
```

## Selection and navigation

The visible circles are indicators rather than conventional button hitboxes.
Except for the optional central hitbox, selection is determined by the pointer's
direction relative to the current menu center. The entire angular sector remains
selectable even when the pointer moves beyond the visible circle.

* **Hover mode** selects an item after the pointer stops over it or turns toward
  another direction.
* **Turbo mode** lets you keep the modifier used to open Waypie held while
  navigating, then select the current item by releasing it.
* **Hold to turbo** provides the same behavior while the left mouse button is
  held and the pointer is moving. A regular click keeps its normal action.
* **Travel item animation** makes the selected circle follow the pointer in
  Hover and Turbo modes.

Opening a submenu moves its circle to the new center. Its children grow and
spread outward from the moving circle. Previous menus remain connected by lines
and can be selected by moving in their return direction. Clicking the center of
a submenu returns to its parent unless **Close on click** is enabled.

## Configuration

<img width="1747" height="1177" alt="image" src="https://github.com/user-attachments/assets/20c10c6d-61bf-400d-93eb-f6127a208731" />

Open the graphical configurator with:

```sh
waypie-config
```

or:

```sh
waypie --configure
```

To use different files for one invocation, pass `-c` and/or `-s`. The options
may appear in any order and work both with the menu and the configurator:

```sh
waypie --show -c /path/to/config -s /path/to/style.css
waypie --config -s /path/to/style.css -c /path/to/config
```

`--config` is a short alias for `--configure`. These paths are not saved as new
defaults: the next regular invocation uses the files in `~/.config/waypie/`
again. Icons continue to be loaded from `~/.config/waypie/icons/`.

* **Preserve proportions** keeps items evenly distributed when a group is
  rotated. A submenu's return direction occupies one invisible slot.
* **Auto alignment** snaps the group rotation to a 5-degree grid.
* **Show icons** affects only the configurator preview.
* **Center layout** (`Ctrl+A`) centers the currently open menu. **Center all
  layouts** (`Ctrl+Shift+A`) centers the root first, followed by every submenu
  level in breadth-first order.
* Actions and entire submenus can be copied, cut, and pasted with `Ctrl+C`,
  `Ctrl+X`, and `Ctrl+V`.
* **Center mode**, **Hover mode**, **Turbo mode**, **Hold to turbo**,
  **Travel item animation**, and **Close on click** control the corresponding
  runtime behavior.

Settings are stored in:

```text
~/.config/waypie/config
```

You should not edit this file manually. Use the graphical configurator instead.

## Styling

<img width="1244" height="394" alt="image" src="https://github.com/user-attachments/assets/678a51c8-c770-41f6-a702-a2013317f2b2" />

All runtime colors, sizes, borders, fonts, opacity values, indicators, connector
styles, and animation parameters are stored in:

```text
~/.config/waypie/style.css
```

Edit this file manually to customize the appearance of the menu.
Waypie validates its supported CSS subset before opening the menu, so malformed
blocks, unknown selectors or properties, invalid colors, and unsafe animation
values are reported in the terminal instead of failing later while drawing.

To show an item's activation keys next to its circle, remove `off` from the
`item-key` block. Their normal and selected appearance can be styled separately
with `item-key` and `item-key.active`.

All animations can be disabled at once:

```css
animation {
  off
}
```

Bundled alternatives are installed as `style_foot.css`, `style_neon.css`,
`style_kando.css`, and `style_boring.css`. Try any of them for one invocation:

```sh
waypie -s ~/.config/waypie/style_neon.css
```

The graphical configurator reads this file for its preview but never modifies
it.

## Icons

Waypie scans the immediate child directories of:

```text
~/.config/waypie/icons/
```

Each child directory represents an icon theme. Files inside each theme are
scanned recursively. Supported formats include SVG, PNG, WebP, JPEG, and GIF.

```text
~/.config/waypie/icons/
├── tabler-icons/
│   ├── outline/terminal.svg
│   └── filled/calculator.svg
└── simple-icons/
    └── cobalt.svg
```

Use the searchable icon picker in the configurator to select a theme and an
icon, or search across all themes at once. Themes are ordered according to their
recent selection history, which is stored in:

```text
~/.config/waypie/.icon-history.json
```

Monochrome SVG files that use `currentColor` inherit the circle's CSS `color`.
Suitable monochrome icon collections include Tabler Icons, Lucide, Material
Symbols, and Simple Icons. Papirus and other desktop icon themes provide larger
full-color collections.

At runtime, an assigned icon is displayed normally and fades out when replaced
by the active label. The text appears immediately. If an item or central menu
has no icon, its text remains visible in every state.

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

* [Kando](https://github.com/kando-menu/kando)
* [Driftmap](https://github.com/rywby-dot/driftwm-minimap)
* [Touchview](https://github.com/malbiruk/touchview)
* [Driftwm](https://github.com/malbiruk/driftwm)
* [Wlogout](https://github.com/ArtsyMacaw/wlogout)

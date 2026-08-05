# Waypie

* Waypie is a lightweight and fast [Kando](https://kando.menu/)-like radial menu for Wayland, written in Rust.
* It does not require a daemon.
* It supports Point-and-Click Mode, Marking Mode, Turbo Mode, Hover Mode, and Centered Mode, inspired by [Kando's interaction modes](https://kando.menu/usage/).
* It is recommended that you [read about Kando](https://kando.menu/intro/) before using Waypie.

https://github.com/user-attachments/assets/bbad9bca-8f19-4e19-b169-acb0bc045aef

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

Existing `config`, `style.css`, and icon files are preserved.

To deliberately replace both configuration files with the repository defaults,
run:

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

## Selection and navigation

The visible circles are indicators rather than conventional button hitboxes.
Except for the optional central hitbox, selection is determined by the pointer's
direction relative to the current menu center. The entire angular sector remains
selectable even when the pointer moves beyond the visible circle.

Opening a submenu moves its circle to the new center. Its children grow and
spread outward from the moving circle. Previous menus remain connected by lines
and can be selected by moving in their return direction. Clicking the center of
a submenu returns to its parent unless **Close on click** is enabled.

## Configuration

<img width="960" height="540" alt="ScreenShot-2026-08-05_15-55-54" src="https://github.com/user-attachments/assets/017347b0-3fd8-4184-b0ea-c885308e1a80" />

Open the graphical configurator with:

```sh
waypie-config
```

or:

```sh
waypie --configure
```

* **Preserve proportions** keeps items evenly distributed when a group is
  rotated. A submenu's return direction occupies one invisible slot.
* **Auto alignment** snaps the group rotation to a 5-degree grid.
* **Show icons** affects only the configurator preview.
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

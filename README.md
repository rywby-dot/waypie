# Waypie

Waypie is a Kando-like radial menu for Wayland. It uses an overlay
layer-shell surface, supports arbitrarily nested submenus, animated navigation,
icons, angular selection, and a graphical menu configurator.

https://github.com/user-attachments/assets/e75f9d29-e1e5-4616-b96a-955b666a2e6a

> [!WARNING]
> Waypie is experimental software, primarily built with AI.

## Installation

Waypie requires Python 3.11 or newer, GTK 4, PyGObject, Cairo/Pycairo, and
gtk4-layer-shell.

Clone the repository and install it into an isolated environment with pipx:

```sh
git clone https://github.com/rywby-dot/waypie.git
cd waypie
pipx install .
```

Create the configuration directory and install the example files:

```sh
mkdir -p ~/.config/waypie
cp config.example ~/.config/waypie/config
cp style.example.css ~/.config/waypie/style.css
```

To update an existing installation:

```sh
cd waypie
git pull
pipx upgrade waypie
```

## Starting and showing the menu

Start the persistent Waypie process:

```sh
waypie
```

It starts hidden and waits for a show command. Add this command to the
autostart configuration of your compositor if Waypie should always be
available.

Show the menu from a key binding:

```sh
waypie --show
```

`waypie --show` is a toggle. Running it again while the menu is visible closes
the menu. Escape and clicking the central close hitbox also close it. If no
Waypie process is running, `waypie --show` starts one and opens the menu.

Example compositor binding:

```text
waypie --show
```

## How selection and navigation work

The cursor direction selects an item. The visible item circle is a visual
indicator; the complete angular sector is the actual hit area. This allows an
action to remain selectable even when the cursor is farther away from its
circle.

The central circle belongs to the currently open menu. Its optional central
hitbox closes Waypie. Hovering an item applies the `circle.active` style, and a
left click executes its command or opens its submenu.

When a submenu opens:

- its center appears at the click position, constrained by
  `minimum-edge-distance`;
- the menu centers are connected by an animated line;
- the previous menu becomes the return target;
- menus two generations behind remain visible but translucent and
  non-interactive;
- moving in the automatically calculated return direction and clicking returns
  to the previous menu.

Return directions are derived automatically from the available item angles.
They are not stored as extra items in the configuration.

Angles are clockwise:

- `0` is up;
- `90` is right;
- `180` is down;
- `270` is left.

All positions use integer angles. There is no separate absolute `x/y`
positioning mode.

## Graphical configurator

Open the configurator with either command:

```sh
waypie-config
```

```sh
waypie --configure
```

Menu contents and geometry should normally be edited through this GUI rather
than by writing TOML manually.

The configurator can:

- add commands and submenus at any nesting depth;
- delete items;
- edit labels, shell commands, angles, and icons;
- open a submenu with a click and return by clicking the translucent previous
  menu center;
- drag circles to change their angles;
- reorder an item by dragging it through the center and outward into a new
  slot;
- preview submenu history, icons, active styles, movement, and layout
  animations;
- edit `menu-radius`, `center-hitbox-size`, and
  `minimum-edge-distance`.

The toolbar options are:

- **Preserve proportions** keeps all items evenly distributed while a group is
  moved. A submenu's invisible return direction occupies one slot in the
  distribution.
- **Auto alignment** snaps rotation to a 5-degree grid.
- **Show icons** controls only the configurator preview. It does not change
  runtime icon behavior.
- **Center layout** performs one complete equal-spacing operation regardless
  of the checkboxes. In a submenu it centers the group around the fixed return
  direction. In the root menu it chooses the closest axis-based rotation.

A click selects an item. Clicking and holding, then moving beyond the drag
threshold, moves it instead. Clicking the current central circle selects the
menu itself so its label and icon can be edited.

### Saving and applying changes

Edits exist only in the configurator's memory until **Save** is pressed.
Pressing **Save** writes:

```text
~/.config/waypie/config
```

The previous configuration is copied to:

```text
~/.config/waypie/config.bak
```

The running Waypie process does not reload configuration automatically. After
pressing **Save**, stop and restart the persistent `waypie` process. If Waypie
is managed by your compositor, user service, or process supervisor, restart it
there. Then use `waypie --show` normally.

The configurator never edits `style.css`.

For compositor window rules, the configurator uses the application ID
`waypie.config`. The icon picker is a modal transient window belonging to the
same application.

## Configuration file

The menu configuration is stored at:

```text
~/.config/waypie/config
```

The GUI maintains this file, including these top-level settings:

- `menu-radius` — normal distance in pixels between a menu center and its item
  circles;
- `center-hitbox-size` — diameter of the central close hitbox; `0` disables
  it;
- `minimum-edge-distance` — minimum distance between a newly opened menu
  center and every screen edge;
- `preserve-proportions` — saved state of the configurator checkbox;
- `auto-alignment` — saved state of the configurator checkbox;
- `configurator-show-icons` — saved state of the configurator-only icon
  preview.

Each menu item has:

- `label` — displayed text;
- `angle` — its integer clockwise angle;
- `command` — a shell command, for an action item;
- nested `items` — for a submenu;
- optional `icon-theme` and `icon`.

An item must contain either a command or nested items, but not both. Submenus
may be nested without a fixed depth limit.

See `config.example` for a complete working tree.

## Styling

All visual settings remain in:

```text
~/.config/waypie/style.css
```

Edit this file manually. The GUI deliberately does not duplicate CSS settings.
Restart the running Waypie process after changing it.

The example stylesheet documents every supported section:

- `overlay` — full-screen overlay background;
- `animation` — hover and menu animation durations;
- `connector` — lines between opened runtime menus;
- `parent-link` — thick return-direction line in the configurator;
- `configurator-history` — appearance and distance of the previous-menu
  preview;
- `circle` — base circle appearance;
- `circle.item` — command items;
- `circle.submenu` — items that open submenus;
- `circle.center` — the currently open menu;
- `circle.parent` — the interactive previous-menu circle;
- `circle.ancestor` — non-interactive older history;
- `circle.previous` — translucent previous-menu preview in the configurator;
- `circle.active` — the action currently selected by cursor direction.

Colors, opacity, borders, circle diameters, font settings, icon size, animation
durations, and active-state scale are controlled here. The normal radial
distance is `menu-radius` in the configuration, while the absolute hovered
distance can be set in CSS:

```css
circle.active {
  scale: 1.15;
  distance: 195px;
}
```

`distance` is the final distance from the center while active, not an amount
added to `menu-radius`. Runtime circles animate to it. The configurator
intentionally previews active growth without moving circles away from the
center.

Monochrome SVG icons using `currentColor` inherit the CSS `color` property.

## Icons

Waypie does not bundle an icon library. Put downloaded icon sets below:

```text
~/.config/waypie/icons/
```

Every immediate child directory is treated as a separate icon theme. Files in
that directory are scanned recursively.

Example:

```text
~/.config/waypie/icons/
├── tabler-icons/
│   ├── outline/
│   │   ├── apps.svg
│   │   └── terminal.svg
│   └── filled/
│       └── calculator.svg
└── Papirus-Dark/
    └── 16x16/
        └── actions/
            ├── configuration.svg
            └── cm_runterm.svg
```

Supported file types are:

- SVG;
- PNG;
- WebP;
- JPEG;
- GIF.

In the configurator:

1. Select a circle or the current central menu.
2. Press **Choose icon…**.
3. Select the icon theme. The theme name is the directory name under
   `icons/`.
4. Search by file name.
5. Click an icon.
6. Press **Save** and restart Waypie.

The icon path stored in the configuration is relative to its theme directory:

```toml
icon-theme = "tabler-icons"
icon = "outline/terminal.svg"
```

At runtime, a circle normally shows its icon. When that action becomes active,
the icon disappears and the label is shown instead, matching Kando's
interaction style. If no icon is assigned, the label is always shown.

`circle { icon-size: ...; }` and role-specific overrides control icon size.
SVG files that use `currentColor` can be recolored through CSS. Multicolor SVG
and raster icons retain their original colors.

Useful sources include monochrome SVG collections such as Tabler Icons,
Lucide, Material Symbols, and Simple Icons, and full-color desktop themes such
as Papirus. Download or clone a collection, then place the directory containing
its icon files under `~/.config/waypie/icons/`.

## Development

Contributor checks and packaging instructions are documented in
`CONTRIBUTING.md`.

## Inspired by

- [Kando](https://github.com/kando-menu/kando)
- [Driftmap](https://github.com/rywby-dot/driftwm-minimap)
- [Touchview](https://github.com/malbiruk/touchview)
- [Driftwm](https://github.com/malbiruk/driftwm)
- [Wlogout](https://github.com/ArtsyMacaw/wlogout)

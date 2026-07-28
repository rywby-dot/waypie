# Waypie
<img width="960" height="540" alt="ScreenShot-2026-07-28_14-09-30" src="https://github.com/user-attachments/assets/85c0c301-8623-4e02-b313-f2c37d63a128" />

**Waypie is a Kando-like radial menu for Wayland. It uses an overlay
layer-shell surface, supports arbitrarily nested submenus, animated navigation,
icons, angular selection, and a graphical menu configurator.**

https://github.com/user-attachments/assets/ee4063bb-6680-47c7-b00f-0e30f6bd66ac

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
the menu. Escape, right click or clicking the central of the menu also close it. If no
Waypie process is running, `waypie --show` starts one and opens the menu.

In the default pointer mode, the initial menu center is taken from the first
pointer-motion event received by the newly activated layer. On some
compositors, the menu therefore remains invisible after `waypie --show` until
the pointer is moved. Enable **Center mode** in the configurator if the menu
must appear immediately in the center without querying the pointer position.

Example compositor binding:

```text
[keybindings]
"mod+d" = "spawn waypie --show"
```

## How selection and navigation work

The cursor direction selects an item. The visible item circle is a visual
indicator; the complete angular sector is the actual hit area. This allows an
action to remain selectable even when the cursor is farther away from its
circle (see kando behavior here https://youtu.be/ZTdfnUDMO9k?t=37&si=x9jvLQj-XF3lyw7R).

The central circle belongs to the currently open menu. Its optional central
hitbox closes Waypie. Hovering an item applies the `circle.active` style, and a
left click executes its command or opens its submenu.

When a submenu opens:

- its center appears at the click position, constrained by
  `minimum-edge-distance`;
- the click sets the length of the new connection, with `menu-radius` used as
  the minimum safe distance;
- the previous menu moves around the new center until the connection points
  exactly through the middle of the return hit sector;
- every older connection keeps its stored length and is realigned in the same
  way, so the complete chain remains geometrically valid;
- old menu circles may approach an edge or move outside the visible screen;
  edge constraints apply only to the newly active menu center;
- the menu centers are connected by straight animated lines;
- the previous menu becomes the return target;
- menus two generations behind remain visible but translucent and
  non-interactive;
- moving in the automatically calculated return direction and clicking returns
  to the previous menu.

Returning to a previous menu uses the click as that menu's new center and
realigns its older chain while preserving every stored connection length.
Because every return circle is always centered in its angular hit sector, no
post-transition position correction is necessary.

## Graphical configurator

<img width="960" height="540" alt="ScreenShot-2026-07-28_14-09-41" src="https://github.com/user-attachments/assets/78f30d4c-cf64-46e4-99bd-1e4bf73cb0df" />

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
  `minimum-edge-distance`;
- enable **Center mode** to open the root menu immediately at the screen
  center;
- enable **Show active label in center** (`active-label-in-center`) to show the hovered command or
  submenu label in the current central circle instead of on the hovered item.

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

The persistent Waypie process reloads `config` every time the menu changes from
hidden to visible. After pressing **Save**, close an already visible menu and
run `waypie --show` again. The next opening immediately uses the new geometry,
commands, angles, icons, and configurator settings; the process does not need
to be restarted.

The configurator never edits or hot-reloads `style.css`. Restart Waypie after
changing visual styles.

For compositor window rules, the configurator uses the application ID
`waypie.config`. The icon picker is a modal transient window belonging to the
same application.

## Styling

All visual settings remain in:

```text
~/.config/waypie/style.css
```

<img width="960" height="540" alt="ScreenShot-2026-07-28_14-09-59" src="https://github.com/user-attachments/assets/f493fe2a-6a99-439f-909a-0551e9c525c3" />

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
- `circle.history` — every previous runtime menu at every history depth;
- `circle.previous` — translucent previous-menu preview in the configurator;
- `circle.active` — the action currently selected by cursor direction.

All previous runtime menu centers remain rendered for the complete open chain.
They share the same `circle.history` background, border, radius, text, icon,
opacity, scale, and width settings; history depth does not change their
appearance. The nearest history circle still provides the return direction,
while older circles are visual only.

Every link in that chain uses `connector`. Its `color`, `opacity`, and `width`
properties control line color, transparency, and thickness; `width: 0px`
disables the lines.

Colors, opacity, borders, circle diameters, font settings, icon size, animation
durations, and active-state scale are controlled here. The normal radial
distance is `menu-radius` in the configuration, while the absolute hovered
distance can be set in CSS:

```css
circle.active {
  scale: 1.15;
  distance: 20px;
  follow-distance: 20%;
}
```

`distance` is added to `menu-radius`: `20px` moves the active circle 20 pixels
outward and `-20px` moves it 20 pixels inward. The final radial distance cannot
be less than zero. Runtime circles animate between the normal and offset
positions. The configurator intentionally previews active growth without
moving circles away from the center.

`follow-distance` gives non-active circles a pointer-reactive share of
`distance`. With `follow-distance: 20%`, a non-active circle in exactly the
same angular direction as the pointer receives 20% of the configured offset.
The share decreases smoothly with angular difference and reaches 0% on the
opposite side at 180°. Only the pointer angle is used: moving the pointer
farther from or closer to the center does not change the effect. The active
circle always receives the full `distance` and does not use this percentage.

Monochrome SVG icons using `currentColor` inherit the CSS `color` property.

## Icons

<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/fa3fb5f0-a64c-4216-96af-55f1c2ac155d" />

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
6. Press **Save**, close the currently visible menu if necessary, and open it
   again with `waypie --show`.

The icon path stored in the configuration is relative to its theme directory:

```toml
icon-theme = "tabler-icons"
icon = "outline/terminal.svg"
```

At runtime, a circle normally shows its icon. When that action becomes active,
the icon disappears and the label is shown instead, matching Kando's
interaction style. If no icon is assigned, the label is always shown.

With **Show active label in center** enabled, the hovered item keeps its icon
and its label replaces the icon or label of the current central circle. This
works for commands, submenu items, and return targets. The active outer or
return circle never shows its own label; without an icon it stays visually
empty while its label is displayed in the center. When no outer item is active,
including while the center itself is focused, the central circle shows only
its own icon and never its own label. A center without an assigned icon is
therefore empty in this mode.

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

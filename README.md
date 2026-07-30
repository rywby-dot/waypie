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
the menu. If no Waypie process is running, `waypie --show` starts one and opens
the menu.

The complete menu can be closed with:

- `Escape`;
- right click anywhere on the layer;
- another `waypie --show`;
- a left click on the root menu's central hitbox;
- a left click on a submenu center when **Close on click** is enabled;
- selection of a command.

When **Close on click** is disabled, clicking a submenu's central circle returns
to its parent instead. A central hitbox size of `0` disables center clicks, but
does not change directional selection.

If the layer becomes unresponsive, kill only the current Waypie process:

```sh
waypie --kill
```

The command reads Waypie's PID from `$XDG_RUNTIME_DIR/waypie` and verifies the
process owner and command before sending `SIGKILL`. Other layer-shell
applications are not affected. If the desktop remains captured after this
command has killed Waypie, the Wayland client and its surfaces no longer exist;
the compositor failed to clear the destroyed layer's input focus.

Closing immediately removes keyboard focus and makes the layer click-through,
then plays the internal closing animation for `close-duration`. The layer-shell
window is hidden only after the animation finishes, so the transition does not
delay interaction with windows below it.

Selecting a command starts it and makes the layer click-through immediately.
During the single `close-duration` animation, its circle moves to the pointer,
keeps growing for the complete transition, and starts fading as soon as it
arrives. At the same time, the current menu items collapse into its center,
while central and history circles shrink in place. Their opacity reaches zero
before their spring-scaled geometry reaches zero. `action-scale` sets the
selected circle's final size multiplier and defaults to `1.3`. There is no
additional action delay.

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

The central circle belongs to the currently open menu. Its optional hitbox
closes the root menu and either returns from or closes a submenu according to
**Close on click**. Hovering an item applies the `circle.active` style, and a
left click executes its command or opens its submenu.

### Pointer mode

Pointer mode is always available:

- move the pointer into an item's angular sector to focus it;
- left click to execute a command or open a submenu;
- click the nearest previous-menu direction to return;
- click the current center to close the root menu or apply the configured
  submenu-center behavior.

The visible circles are not button hitboxes. Except for an enabled central
hitbox, selection depends only on the direction from the active menu center.

### Hover Mode

Enable `hover-mode` in the configurator to navigate and select without
clicking. Once the pointer has made a sufficiently long movement, Waypie
selects the current direction when either:

- the pointer remains nearly stationary for a short time; or
- the movement makes a sufficiently sharp turn.

Hover Mode can open submenus, return to previous menus, and immediately execute
commands. The detector uses the same default thresholds as Kando: a `15px`
activation distance, `150px` minimum stroke, `20deg` turn, `10px` jitter
threshold, and `100ms` pause. Developers can tune these constants together at
the top of `waypie_hover.py`.

### Turbo Mode

Enable `turbo-mode` to use the modifier from the compositor shortcut as a
temporary mouse button:

1. Press a shortcut such as `Super+R`, `Alt+Space`, or `Ctrl+Shift+Space`.
2. Keep its modifier key or keys held after Waypie opens.
3. Use pauses or turns to move through submenus without clicking.
4. Point toward the final action and release the last held modifier.

While a modifier is held, gesture selections open submenus and return through
the menu chain, but do not execute final commands. The current action is
executed only when the last held `Super`, `Alt`, `Ctrl`, or `Shift` key is
released. Releasing the non-modifier part of the shortcut first is supported.

Waypie reads the modifiers that are actually held from GDK pointer events, so
the shortcut does not need to be duplicated in `config`. Hover Mode and Turbo
Mode can be enabled independently. A compositor that does not forward the
modifier-release event may not support Turbo Mode reliably.

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
- enable **Show active label in center** (`active-label-in-center`) to show the
  hovered command or submenu label in the current central circle instead of on
  the hovered item;
- enable **Close on click** (`close-submenu-on-center-click`) to make a
  submenu's central circle close Waypie instead of returning to its parent.
  The root central circle always closes Waypie;
- enable **Hover mode** (`hover-mode`) to select without clicking: move toward
  an item and either pause briefly or turn toward the next item;
- enable **Turbo mode** (`turbo-mode`) to navigate while keeping `Super`,
  `Alt`, `Ctrl`, or `Shift` held after the opening shortcut. Pauses and turns
  open submenus; releasing the last held modifier activates the current item.

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

Toolbar actions also have global configurator shortcuts, shown below each
button label:

- `Ctrl+D` — delete the selected item;
- `Ctrl+Q` — add a command to the currently open menu;
- `Ctrl+X` — add a submenu;
- `Ctrl+S` — save;
- `Ctrl+A` — center the current layout;
- `Ctrl+Z` — undo the last edit, including additions, deletions, layout
  changes, dragging, settings, and icon selection.

Configurator shortcuts are global only while a non-editable widget has focus.
When a text or numeric input field is focused, `Ctrl+…` shortcuts are left to
that field and do not modify the menu structure.

A click selects an item. Clicking and holding, then moving beyond the drag
threshold, moves it instead. Clicking the current central circle selects the
menu itself so its label and icon can be edited.

### Configuration settings

The configurator reads and writes all non-visual top-level settings:

- `menu-radius` — normal radial distance from the active center to its items
  and the minimum safe length of a newly created submenu connection;
- `center-hitbox-size` — diameter of the central click target; use `0` to
  disable center clicks;
- `minimum-edge-distance` — minimum permitted distance between a newly active
  menu center and a screen edge;
- `center-mode` — open the root menu at the screen center instead of waiting
  for an initial pointer event;
- `active-label-in-center` — move the focused item's label to the active
  center;
- `close-submenu-on-center-click` — close Waypie from a submenu center instead
  of returning to its parent;
- `hover-mode` — enable pause-and-turn selection without clicking;
- `turbo-mode` — enable gesture navigation while a shortcut modifier is held
  and final selection on modifier release;
- `preserve-proportions`, `auto-alignment`, and
  `configurator-show-icons` — persistent preferences used by the configurator.

Each menu or action supports `label`, optional `icon-theme` and `icon`, and an
integer `angle`. Actions contain a `command`; submenus contain further
`items`. Submenus can be nested to any depth. An item cannot contain both a
command and children. A submenu may contain no children: it remains editable,
can be opened in the configurator, and is saved without being converted into
an action.

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
- `animation` — separate durations for hover movement, icon fading, menu
  transitions, and selected-action movement;
- `connector` — lines between opened runtime menus;
- `parent-link` — thick return-direction line in the configurator;
- `configurator-history` — appearance and distance of the previous-menu
  preview;
- `circle` — base circle appearance;
- `circle.item` — command items;
- `circle.submenu` — selectable items that open submenus, independently styled
  from command items;
- `submenu-indicator` — child-direction circles on selectable submenu
  items;
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

Selectable submenu circles use `circle.submenu` after `circle.item` in the
style cascade, so their fill, border, diameter, opacity, icon, and text settings
can be completely different from command circles. This selector does not
affect the current central circle or history circles.

Each child of a selectable submenu is represented by one
`submenu-indicator`, placed at that child's angle. The indicator is drawn
as a full circle behind the submenu circle. Its settings are `width`, `color`,
`opacity`, `protrusion`, and `cut-indicators`. `width: 0px` disables the
indicators. `protrusion: 0px` keeps them completely hidden; increasing it
reveals more of each circle outward. With `cut-indicators: true`, the part
inside the submenu circle is clipped. With `cut-indicators: false`, the small
circles remain complete and can be seen through a translucent submenu circle.
These indicators are also shown in the configurator preview.

Colors, opacity, borders, circle diameters, font settings, icon size, animation
durations, and active-state scale are controlled here. The normal radial
distance is `menu-radius` in the configuration, while the absolute hovered
distance can be set in CSS:

Labels are centered and wrapped inside the actual inner shape of each circle.
Word boundaries are preferred; words that are too long are split across
lines. If the complete label still cannot fit, Waypie uses three dots in the
final line and preserves the label's final three characters. Available line
width is calculated from the base `circle` scale, border, corner radius, and
font size. Enlarging a circle does not resize or reflow its text. If a circle
shrinks below the base `circle` scale, the complete precomputed text block
shrinks with it without changing its line breaks.

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
The effect starts only after an item sector becomes active. It is disabled
while the central hitbox, the return direction, or no action is focused.

Monochrome SVG icons using `currentColor` inherit the CSS `color` property.

## Icons

<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/fa3fb5f0-a64c-4216-96af-55f1c2ac155d" />

Waypie does not bundle an icon library. Put downloaded icon sets below:

```text
~/.config/waypie/icons/
```

Every immediate child directory is treated as a separate icon theme. Files in
that directory are scanned recursively.

The icon picker lists recently used themes first. Recency is updated only when
an icon is actually chosen, not when a theme is merely viewed. This
configurator-only history persists in:

```text
~/.config/waypie/.icon-history.json
```

Themes with no recorded selection are listed alphabetically after the used
themes. Removing this history file resets the order and does not affect menu
icons or `config`.

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
return circle hides its own label only when it has an icon to show instead.
Likewise, the central circle prefers its icon when available. Across all
roles and both label modes, a missing or unloadable icon always falls back to
text; Waypie never intentionally leaves an iconless labeled circle blank.

`circle { icon-size: ...; }` and role-specific overrides control icon size.
When a circle is scaled by an active style or animation, its icon scales with
the circle while its text size remains unchanged. SVG icons are rendered at
each animated size instead of stretching a cached low-resolution frame. SVG
files that use `currentColor` can be recolored through CSS. Multicolor SVG and
raster icons retain their original colors.

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

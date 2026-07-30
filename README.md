# Waypie
<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/ef3cfe74-ffd4-4c3b-8ef9-32a8a9b49c32" />
<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/b534ad4f-f8cf-4159-a3a7-6fad2846d2d6" />

**Waypie is a Kando-like radial menu for Wayland. It uses an overlay
layer-shell surface, supports arbitrarily nested submenus, animated navigation,
icons, angular selection, and a graphical menu configurator.**

> [!WARNING]
> Waypie is experimental software, primarily built with AI.

It is recommended to read about Kando first https://kando.menu/intro/

## Installation

Waypie requires Python 3.11 or newer, GTK 4, PyGObject, Cairo/Pycairo, and
gtk4-layer-shell.

Clone the repository and install it into an isolated environment with pipx:

```sh
git clone https://github.com/rywby-dot/waypie.git
cd waypie
pipx install .
```

Create the configuration directory and install the example files and bundled
icon sets:

```sh
mkdir -p ~/.config/waypie
cp config.example ~/.config/waypie/config
cp style.example.css ~/.config/waypie/style.css
cp -r icons ~/.config/waypie
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

The visible circles are not button hitboxes. Except for an enabled central
hitbox, selection depends only on the direction from the active menu center.

### Hover Mode
(same as in kando)
Enable `hover-mode` in the configurator to navigate and select without
clicking. Once the pointer has made a sufficiently long movement, Waypie
selects the current direction when either:

- the pointer remains nearly stationary for a short time; or
- the movement makes a sufficiently sharp turn.

Hover Mode can open submenus, return to previous menus, and immediately execute
commands. The detector uses the same default thresholds as Kando. Developers can
tune these constants together at the top of `waypie_hover.py`.

### Turbo Mode
(same as in kando)
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

## Graphical configurator

<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/2acca4ac-9ee7-49b3-a194-94b79a1f2b4c" />

Open the configurator with either command:

```sh
waypie-config
```

```sh
waypie --configure
```

Menu contents and geometry should normally be edited through this GUI rather
than by writing TOML manually.

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

### Configuration settings

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

<img width="960" height="540" alt="image" src="https://github.com/user-attachments/assets/df232c03-c11b-4661-bcde-c9f16a30072a" />

Edit this file manually. The GUI deliberately does not duplicate CSS settings.
Restart the running Waypie process after changing it.

Example from style.css:
```css
circle.active {
  scale: 1.15;
  distance: 20px;
  follow-distance: 20%;
}
```

Monochrome SVG icons using `currentColor` inherit the CSS `color` property.

## Icons

The repository includes the monochrome Simple Icons and Tabler Icons
collections. The installation commands above copy them to:

```text
~/.config/waypie/icons/
```

Every immediate child directory is treated as a separate icon theme. Files in
that directory are scanned recursively. Additional downloaded themes can be
copied alongside the bundled ones.

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

At runtime, a circle normally shows its icon. When that action becomes active,
the icon disappears and the label is shown instead, matching Kando's
interaction style. If no icon is assigned, the label is always shown.

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



https://github.com/user-attachments/assets/fddcc941-bbec-448d-bd93-f253433c687d


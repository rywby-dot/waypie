# Waypie
Kando-like pie menu for wayland using shell's overlay layer


https://github.com/user-attachments/assets/e75f9d29-e1e5-4616-b96a-955b666a2e6a


> [!WARNING]
> This is experimental software, primarily built with AI.

## Installation

```sh
git clone https://github.com/rywby-dot/waypie.git
cd waypie
pipx install .
```

Required at runtime:

- Python 3.11 or newer;
- GTK 4 and PyGObject;
- Cairo/Pycairo;
- gtk4-layer-shell.

Update an existing installation with:

```sh
cd waypie
git pull
pipx upgrade waypie
```

## Visual configurator

Open the menu editor with either command:

```sh
waypie --configure
waypie-config
```

The configurator can add and remove commands and submenus, edit labels and
commands, and change item angles by dragging circles on the
preview. Saving rewrites `~/.config/waypie/config` and keeps the previous file
as `~/.config/waypie/config.bak`. The configurator does not modify
`~/.config/waypie/style.css`.

## Icon themes

Put each icon set in its own directory below
`~/.config/waypie/icons/`. Subdirectories are scanned recursively for SVG,
PNG, WebP, JPEG, and GIF files:

```text
~/.config/waypie/icons/
├── tabler/
│   ├── home.svg
│   └── terminal.svg
└── papirus/
    └── apps/
        └── utilities-terminal.svg
```

Use the icon button in the configurator to select a directory and search its
icons. An item with an icon shows the icon normally and replaces it with the
item label while hovered. SVG files using `currentColor` use the circle's CSS
`color`; raster and multicolor SVG files retain their original colors.
`icon-size` controls the rendered size.

## Inspired by:
  - [Kando](https://github.com/kando-menu/kando)
  - [Driftmap](https://github.com/rywby-dot/driftwm-minimap)
  - [Touchview](https://github.com/malbiruk/touchview)
  - [Driftwm](https://github.com/malbiruk/driftwm)
  - [Wlogout](https://github.com/ArtsyMacaw/wlogout)

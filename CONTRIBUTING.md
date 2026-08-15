# Contributing

Waypie is experimental software, but focused pull requests and bug reports are
welcome.

The runtime lives in the root Cargo crate (`Cargo.toml` and `src/`) and is
entirely written in Rust. Python is used only by the GTK configurator:

- `waypie_config.py` — configurator UI and editing logic;
- `waypie_common.py` — configurator model, TOML, CSS, icon, animation, and
  drawing helpers.

## Pull requests

Keep each pull request focused on one concern and do not commit generated
`target/`, Python caches, wheels, or local configuration files.

Run all required checks with:

```sh
make check
```

This command runs:

- `cargo fmt --check`;
- Clippy for every Rust target with warnings denied;
- all Rust unit and documentation tests;
- Ruff formatting and lint checks for the configurator;
- Python compilation and unit tests.

Also verify that the optimized binary builds:

```sh
make build
```

Test installation without touching the normal binary or configuration paths by
using temporary directories:

```sh
make install-runtime PREFIX=/tmp/waypie-install
make install-config CONFIG_DIR=/tmp/waypie-config
```

The graphical configurator and layer-shell runtime must be tested separately
inside a Wayland session. Exercise multiple outputs, submenu entry and return,
Hover Mode, Turbo Mode, icons, repeated `waypie --show`, and each closing path.

## Reporting bugs

Include:

- expected and actual behavior;
- exact reproduction steps;
- distribution and Wayland compositor;
- Waypie commit, Rust version, and GPU/driver information when rendering is
  involved;
- terminal output from `waypie --show`;
- the relevant `config` and `style.css` fragments.

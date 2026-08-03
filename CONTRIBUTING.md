# Contributing

Waypie is experimental software, but focused pull requests and bug reports are
welcome.

## Pull requests

Keep each pull request focused on one concern.

Before submitting a change, run the same checks as CI:

```sh
ruff format --check waypie.py waypie_animation.py waypie_backend.py waypie_config.py waypie_common.py
ruff check waypie.py waypie_animation.py waypie_backend.py waypie_config.py waypie_common.py tests
python -m py_compile waypie.py waypie_animation.py waypie_backend.py waypie_config.py waypie_common.py
python -m unittest discover -s tests
python -m build
```

These checks verify formatting, static analysis, Python syntax, and the package
used by `pipx install .`. A full runtime check must be performed separately in
a Wayland session with GTK 4, PyGObject, Cairo, and gtk4-layer-shell installed,
or an X11 session with GTK 4, PyGObject, Cairo, libX11, and a compositor.

## Reporting bugs

Include:

- what you expected and what happened;
- exact steps to reproduce;
- distribution, display protocol, and Wayland compositor or X11 window manager;
- Python, GTK 4, and gtk4-layer-shell versions;
- terminal output from Waypie, if any.

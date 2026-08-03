import unittest
from typing import ClassVar

from waypie_backend import (
    WaylandBackend,
    X11Backend,
    display_backend,
    display_name,
    safe_display_name,
)


class DisplayDetectionTests(unittest.TestCase):
    def test_wayland_is_preferred_when_both_displays_exist(self):
        environment = {"WAYLAND_DISPLAY": "wayland-1", "DISPLAY": ":0"}

        self.assertEqual(display_backend(environment), "wayland")
        self.assertEqual(display_name(environment), "wayland-1")

    def test_x11_is_selected_without_a_wayland_display(self):
        environment = {"DISPLAY": ":1.0"}

        self.assertEqual(display_backend(environment), "x11")
        self.assertEqual(display_name(environment), ":1.0")
        self.assertEqual(safe_display_name(environment), "_1.0")

    def test_gdk_backend_can_force_x11_under_wayland(self):
        environment = {
            "GDK_BACKEND": "x11",
            "WAYLAND_DISPLAY": "wayland-1",
            "DISPLAY": ":0",
        }

        self.assertEqual(display_backend(environment), "x11")

    def test_gdk_backend_can_force_wayland(self):
        environment = {"GDK_BACKEND": "wayland", "DISPLAY": ":0"}

        self.assertEqual(display_backend(environment), "wayland")


class FakeWindow:
    def __init__(self, surface=None):
        self.surface = surface
        self.visible = False
        self.presented = False
        self.callbacks = {}

    def connect(self, signal, callback):
        self.callbacks[signal] = callback

    def get_surface(self):
        return self.surface

    def set_visible(self, visible):
        self.visible = visible

    def get_visible(self):
        return self.visible

    def present(self):
        self.presented = True


class FakeSurface:
    def get_xid(self):
        return 42


class FakeGdkX11:
    X11Surface = FakeSurface


class FakeGLib:
    removed: ClassVar[list] = []

    @staticmethod
    def idle_add(callback, *args):
        callback(*args)
        return 7

    @classmethod
    def source_remove(cls, source):
        cls.removed.append(source)


class FakeX11Api:
    def __init__(self):
        self.configured = []
        self.acquired = []
        self.releases = 0
        self.closed = False

    def configure_overlay(self, xid):
        self.configured.append(xid)

    def acquire_keyboard(self, xid):
        self.acquired.append(xid)
        return True

    def release_keyboard(self):
        self.releases += 1

    def close(self):
        self.closed = True


class X11BackendTests(unittest.TestCase):
    def setUp(self):
        self.api = FakeX11Api()
        self.backend = X11Backend(FakeGLib, FakeGdkX11, self.api)
        self.window = FakeWindow(FakeSurface())

    def test_configures_override_redirect_surface_when_realized(self):
        self.backend.configure(self.window)
        self.window.callbacks["notify::surface"](self.window)

        self.assertEqual(self.backend.xid, 42)
        self.assertEqual(self.api.configured, [42])

    def test_show_presents_window_and_grabs_keyboard(self):
        self.backend.xid = 42

        self.backend.show(self.window)

        self.assertTrue(self.window.visible)
        self.assertTrue(self.window.presented)
        self.assertEqual(self.api.configured, [42])
        self.assertEqual(self.api.acquired, [42])

    def test_release_and_shutdown_ungrab_keyboard(self):
        self.backend.release_input(self.window)
        self.backend.shutdown()

        self.assertEqual(self.api.releases, 2)
        self.assertTrue(self.api.closed)


class FakeLayerShell:
    class Layer:
        OVERLAY = "overlay"

    class KeyboardMode:
        NONE = "none"
        EXCLUSIVE = "exclusive"

    class Edge:
        TOP, RIGHT, BOTTOM, LEFT = range(4)

    def __init__(self):
        self.calls = []

    def __getattr__(self, name):
        return lambda *args: self.calls.append((name, *args))


class WaylandBackendTests(unittest.TestCase):
    def test_existing_layer_shell_contract_is_preserved(self):
        layer = FakeLayerShell()
        window = FakeWindow()
        backend = WaylandBackend(layer)

        backend.configure(window)
        backend.show(window)
        backend.release_input(window)

        self.assertIn(("set_layer", window, "overlay"), layer.calls)
        self.assertIn(("set_exclusive_zone", window, -1), layer.calls)
        self.assertIn(("set_keyboard_mode", window, "exclusive"), layer.calls)
        self.assertIn(("set_keyboard_mode", window, "none"), layer.calls)
        self.assertTrue(window.visible)


if __name__ == "__main__":
    unittest.main()

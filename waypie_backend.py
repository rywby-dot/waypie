import ctypes
import ctypes.util
import os
import re


def display_backend(environment=None):
    environment = os.environ if environment is None else environment
    requested = environment.get("GDK_BACKEND", "")
    backends = [part.strip().lower() for part in requested.split(",") if part.strip()]
    if "x11" in backends and "wayland" not in backends:
        return "x11"
    if "wayland" in backends and "x11" not in backends:
        return "wayland"
    if environment.get("WAYLAND_DISPLAY"):
        return "wayland"
    if environment.get("DISPLAY"):
        return "x11"
    return "wayland"


def display_name(environment=None):
    environment = os.environ if environment is None else environment
    backend = display_backend(environment)
    variable = "WAYLAND_DISPLAY" if backend == "wayland" else "DISPLAY"
    return environment.get(variable, backend)


def safe_display_name(environment=None):
    return re.sub(r"[^A-Za-z0-9_.-]", "_", display_name(environment))


class WaylandBackend:
    def __init__(self, layer_shell):
        self.layer_shell = layer_shell

    def configure(self, window):
        layer = self.layer_shell
        layer.init_for_window(window)
        layer.set_namespace(window, "waypie")
        layer.set_layer(window, layer.Layer.OVERLAY)
        layer.set_exclusive_zone(window, -1)
        layer.set_keyboard_mode(window, layer.KeyboardMode.NONE)
        for edge in (
            layer.Edge.TOP,
            layer.Edge.RIGHT,
            layer.Edge.BOTTOM,
            layer.Edge.LEFT,
        ):
            layer.set_anchor(window, edge, True)

    def show(self, window):
        self.layer_shell.set_keyboard_mode(
            window, self.layer_shell.KeyboardMode.EXCLUSIVE
        )
        window.set_visible(True)

    def release_input(self, window):
        self.layer_shell.set_keyboard_mode(window, self.layer_shell.KeyboardMode.NONE)

    def shutdown(self):
        pass


class X11Api:
    CW_OVERRIDE_REDIRECT = 1 << 9
    GRAB_MODE_ASYNC = 1
    GRAB_SUCCESS = 0
    REVERT_TO_POINTER_ROOT = 1
    CURRENT_TIME = 0

    class WindowAttributes(ctypes.Structure):
        _fields_ = [
            ("background_pixmap", ctypes.c_ulong),
            ("background_pixel", ctypes.c_ulong),
            ("border_pixmap", ctypes.c_ulong),
            ("border_pixel", ctypes.c_ulong),
            ("bit_gravity", ctypes.c_int),
            ("win_gravity", ctypes.c_int),
            ("backing_store", ctypes.c_int),
            ("backing_planes", ctypes.c_ulong),
            ("backing_pixel", ctypes.c_ulong),
            ("save_under", ctypes.c_int),
            ("event_mask", ctypes.c_long),
            ("do_not_propagate_mask", ctypes.c_long),
            ("override_redirect", ctypes.c_int),
            ("colormap", ctypes.c_ulong),
            ("cursor", ctypes.c_ulong),
        ]

    def __init__(self, display=None):
        library = ctypes.util.find_library("X11")
        if not library:
            raise RuntimeError("libX11 was not found")
        self.x11 = ctypes.CDLL(library)
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.x11.XDefaultScreen.argtypes = [ctypes.c_void_p]
        self.x11.XDisplayWidth.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.x11.XDisplayHeight.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.x11.XChangeWindowAttributes.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.POINTER(self.WindowAttributes),
        ]
        self.x11.XMoveResizeWindow.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint,
            ctypes.c_uint,
        ]
        self.x11.XRaiseWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XSetInputFocus.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        self.x11.XGrabKeyboard.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        self.x11.XGrabKeyboard.restype = ctypes.c_int
        self.x11.XUngrabKeyboard.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XFlush.argtypes = [ctypes.c_void_p]
        self.owns_display = display is None
        self.display = self.x11.XOpenDisplay(None) if display is None else display
        if not self.display:
            raise RuntimeError("cannot open the X11 display")
        self.screen = self.x11.XDefaultScreen(self.display)

    def close(self):
        if self.display and self.owns_display:
            self.x11.XCloseDisplay(self.display)
        self.display = None

    @classmethod
    def from_gdk_display(cls, display):
        # PyGObject represents the opaque xlib.Display pointer as a GI boxed
        # value; its hash is the wrapped pointer address.
        xdisplay = display.get_xdisplay()
        return cls(ctypes.c_void_p(hash(xdisplay)))

    def configure_overlay(self, xid):
        attributes = self.WindowAttributes()
        attributes.override_redirect = 1
        self.x11.XChangeWindowAttributes(
            self.display, xid, self.CW_OVERRIDE_REDIRECT, ctypes.byref(attributes)
        )
        width = self.x11.XDisplayWidth(self.display, self.screen)
        height = self.x11.XDisplayHeight(self.display, self.screen)
        self.x11.XMoveResizeWindow(self.display, xid, 0, 0, width, height)
        self.x11.XFlush(self.display)

    def acquire_keyboard(self, xid):
        self.x11.XRaiseWindow(self.display, xid)
        self.x11.XSetInputFocus(
            self.display, xid, self.REVERT_TO_POINTER_ROOT, self.CURRENT_TIME
        )
        result = self.x11.XGrabKeyboard(
            self.display,
            xid,
            True,
            self.GRAB_MODE_ASYNC,
            self.GRAB_MODE_ASYNC,
            self.CURRENT_TIME,
        )
        self.x11.XFlush(self.display)
        return result == self.GRAB_SUCCESS

    def release_keyboard(self):
        self.x11.XUngrabKeyboard(self.display, self.CURRENT_TIME)
        self.x11.XFlush(self.display)


class X11Backend:
    def __init__(self, GLib, GdkX11, api=None):
        self.GLib = GLib
        self.GdkX11 = GdkX11
        self.api = api
        self.xid = None
        self.grab_source = None

    def configure(self, window):
        window.connect("notify::surface", self.on_surface_changed)

    def on_surface_changed(self, window, _property=None):
        surface = window.get_surface()
        if surface is None:
            return
        if not isinstance(surface, self.GdkX11.X11Surface):
            raise TypeError("Waypie selected X11 but GTK did not create an X11 surface")
        if self.api is None:
            self.api = X11Api.from_gdk_display(surface.get_display())
        self.xid = surface.get_xid()
        self.api.configure_overlay(self.xid)

    def show(self, window):
        window.set_visible(True)
        window.present()
        if self.xid is not None:
            self.api.configure_overlay(self.xid)
        self.cancel_grab()
        self.grab_source = self.GLib.idle_add(self.acquire_keyboard, window)

    def acquire_keyboard(self, window):
        self.grab_source = None
        if window.get_visible() and self.xid is not None:
            self.api.acquire_keyboard(self.xid)
        return False

    def release_input(self, _window):
        self.cancel_grab()
        if self.api is not None:
            self.api.release_keyboard()

    def cancel_grab(self):
        if self.grab_source is not None:
            self.GLib.source_remove(self.grab_source)
            self.grab_source = None

    def shutdown(self):
        self.cancel_grab()
        if self.api is not None:
            self.api.release_keyboard()
            self.api.close()
            self.api = None

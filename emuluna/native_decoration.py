"""Optional KDE Wayland palette requests without a KDE platform-theme dependency.

Protocol: plasma-wayland-protocols/server-decoration-palette.xml. Qt's private
surface lookup is checked at runtime; unsupported desktops keep native colors.
"""
import ctypes as C
from pathlib import Path
from weakref import WeakKeyDictionary

from PySide6.QtCore import QByteArray, QLibraryInfo, Qt
from PySide6.QtWidgets import QApplication
from shiboken6 import getCppPointer


class Interface(C.Structure):
    pass


class Message(C.Structure):
    _fields_ = [('name', C.c_char_p), ('signature', C.c_char_p),
                ('types', C.POINTER(C.POINTER(Interface)))]


Interface._fields_ = [('name', C.c_char_p), ('version', C.c_int),
                      ('method_count', C.c_int), ('methods', C.POINTER(Message)),
                      ('event_count', C.c_int), ('events', C.POINTER(Message))]


class KdeDecorationPalette:
    def __init__(self, app):
        self.app = app
        self.windows = WeakKeyDictionary()
        self.manager = None
        self.lib = C.CDLL('libwayland-client.so.0')
        pointer = C.c_void_p
        self.lib.wl_proxy_marshal_flags.restype = pointer
        self.lib.wl_proxy_marshal_flags.argtypes = [pointer, C.c_uint, C.POINTER(Interface), C.c_uint, C.c_uint]
        for name in ('wl_proxy_destroy', 'wl_event_queue_destroy'):
            getattr(self.lib, name).argtypes = [pointer]
        self.lib.wl_proxy_set_queue.argtypes = [pointer, pointer]
        self.lib.wl_proxy_add_listener.argtypes = [pointer, C.POINTER(pointer), pointer]
        self.lib.wl_display_create_queue.argtypes = [pointer]
        self.lib.wl_display_create_queue.restype = pointer
        self.lib.wl_display_roundtrip_queue.argtypes = [pointer, pointer]
        self.lib.wl_display_flush.argtypes = [pointer]
        self.display = int(app.nativeInterface().display())
        libraries = Path(QLibraryInfo.path(QLibraryInfo.LibrariesPath))
        self.qt_gui = C.CDLL(str(libraries / 'libQt6Gui.so.6'))
        self.qt_wayland = C.CDLL(str(libraries / 'libQt6WaylandClient.so.6'))
        native = getattr(self.qt_gui, '_ZN15QGuiApplication23platformNativeInterfaceEv')
        native.restype = pointer
        self.native = native()
        self.surface_lookup = getattr(self.qt_wayland,
            '_ZN15QtWaylandClient23QWaylandNativeInterface23nativeResourceForWindowERK10QByteArrayP7QWindow')
        self.surface_lookup.restype = pointer
        self.surface_lookup.argtypes = [pointer, pointer, pointer]
        self.surface_name = QByteArray(b'surface')
        self.palette_methods = (Message * 2)(Message(b'set_palette', b's', None), Message(b'release', b'', None))
        self.palette_interface = Interface(b'org_kde_kwin_server_decoration_palette', 1, 2, self.palette_methods, 0, None)
        surface_interface = Interface.in_dll(self.lib, 'wl_surface_interface')
        self.create_types = (C.POINTER(Interface) * 2)(C.pointer(self.palette_interface), C.pointer(surface_interface))
        self.manager_methods = (Message * 1)(Message(b'create', b'no', self.create_types))
        self.manager_interface = Interface(b'org_kde_kwin_server_decoration_palette_manager', 1, 1, self.manager_methods, 0, None)
        registry_interface = Interface.in_dll(self.lib, 'wl_registry_interface')
        self.queue = self.lib.wl_display_create_queue(self.display)
        self.registry = self.marshal(self.display, 1, C.byref(registry_interface), 1, 0, pointer())
        self.lib.wl_proxy_set_queue(self.registry, self.queue)
        global_callback = C.CFUNCTYPE(None, pointer, pointer, C.c_uint, C.c_char_p, C.c_uint)
        remove_callback = C.CFUNCTYPE(None, pointer, pointer, C.c_uint)
        self.global_callback = global_callback(self.global_added)
        self.remove_callback = remove_callback(lambda *_: None)
        self.listener = (pointer * 2)(C.cast(self.global_callback, pointer), C.cast(self.remove_callback, pointer))
        self.lib.wl_proxy_add_listener(self.registry, self.listener, None)
        self.lib.wl_display_roundtrip_queue(self.display, self.queue)
        app.aboutToQuit.connect(self.close)

    def marshal(self, proxy, opcode, interface=None, version=1, flags=0, *args):
        return self.lib.wl_proxy_marshal_flags(proxy, opcode, interface, version, flags, *args)

    def global_added(self, data, registry, name, interface, version):
        if interface == self.manager_interface.name and not self.manager:
            self.manager = self.marshal(registry, 0, C.byref(self.manager_interface), 1, 0,
                                        C.c_uint(name), C.c_char_p(interface), C.c_uint(1), C.c_void_p())

    def apply(self, widget, path):
        window = widget.windowHandle()
        if not self.manager or not window or window.type() in (Qt.Popup, Qt.ToolTip):
            return
        surface = self.surface_lookup(self.native, getCppPointer(self.surface_name)[0], getCppPointer(window)[0])
        if not surface:
            return
        previous = self.windows.get(window)
        if previous and previous[0] != surface:
            self.release(window)
            previous = None
        if not previous:
            palette = self.marshal(self.manager, 0, C.byref(self.palette_interface), 1, 0,
                                   C.c_void_p(), C.c_void_p(surface))
            previous = (surface, palette, None)
        if previous[2] != path:
            self.marshal(previous[1], 0, None, 1, 0, C.c_char_p(path.encode()))
            self.lib.wl_display_flush(self.display)
        self.windows[window] = (surface, previous[1], path)

    def release(self, window):
        if window is None:
            return
        previous = self.windows.pop(window, None)
        if previous:
            self.marshal(previous[1], 1, None, 1, 1)

    def close(self):
        for window in list(self.windows):
            self.release(window)
        if self.manager:
            self.lib.wl_proxy_destroy(self.manager)
            self.manager = None
        if self.registry:
            self.lib.wl_proxy_destroy(self.registry)
            self.registry = None
        if self.queue:
            self.lib.wl_event_queue_destroy(self.queue)
            self.queue = None


def native_decoration_binding(app):
    if not QApplication.platformName().startswith('wayland'):
        return None
    try:
        return KdeDecorationPalette(app)
    except (OSError, AttributeError, TypeError, ValueError) as error:
        print(f'Native title-bar palette unavailable: {error}', flush=True)
        return None

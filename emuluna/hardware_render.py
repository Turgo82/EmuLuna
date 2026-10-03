"""Isolated OpenGL context for libretro cores that request hardware rendering."""
import ctypes as C
from functools import lru_cache
import os
from pathlib import Path

from PySide6.QtCore import QSize
from PySide6.QtGui import QImage, QOffscreenSurface, QOpenGLContext, QSurfaceFormat
from PySide6.QtOpenGL import QOpenGLFramebufferObject, QOpenGLFramebufferObjectFormat


class HardwareRenderError(RuntimeError):
    pass


@lru_cache(maxsize=1)
def probe_vulkan_context():
    """Try creating an actual headless Vulkan device, not just loading its library."""
    from .core import ROOT
    host = Path(os.environ.get("EMULUNA_CORE_DIR", ROOT / "build/cores")) / "libemuluna_host.so"
    if not host.is_file():
        return False
    try:
        library = C.CDLL(str(host))
        library.el_vulkan_available.restype = C.c_int
        return bool(library.el_vulkan_available())
    except (OSError, AttributeError):
        return False


class VulkanCoreDisplay:
    """Use the libretro Vulkan interface; the native host reads back each frame."""
    description = "Vulkan (experimental)"
    framebuffer = 0

    def __init__(self, core):
        if core.hardware_context_type != 6:
            raise HardwareRenderError("The core did not request a Vulkan context.")
        self.core = core
        if not core.lib.el_vulkan_initialize(core.handle):
            raise HardwareRenderError(core.error_message("Could not initialize the core's Vulkan context."))
        core.attach_hardware(self)

    def begin(self):
        pass

    def end(self):
        pass

    def capture(self, width, height, _bottom_left):
        return C.string_at(self.core.lib.el_pixels(self.core.handle), width * height * 4)

    def close(self):
        pass


@lru_cache(maxsize=1)
def probe_hardware_contexts():
    """Return the OpenGL versions this Qt runtime can actually create."""
    versions = []
    for profile, version in ((QSurfaceFormat.CompatibilityProfile, (3, 3)),
                             (QSurfaceFormat.CoreProfile, (3, 3))):
        fmt = QSurfaceFormat()
        fmt.setVersion(*version)
        fmt.setProfile(profile)
        surface = QOffscreenSurface()
        surface.setFormat(fmt)
        surface.create()
        context = QOpenGLContext()
        context.setFormat(fmt)
        actual_version = (0, 0)
        try:
            if surface.isValid() and context.create() and context.makeCurrent(surface):
                actual = context.format()
                if ((actual.majorVersion(), actual.minorVersion()) >= version
                        and actual.profile() == profile):
                    actual_version = (actual.majorVersion(), actual.minorVersion())
                context.doneCurrent()
        finally:
            surface.destroy()
        versions.extend(actual_version)
    return tuple(versions)


class HardwareCoreDisplay:
    """Own a core render target without changing EmuLuna's normal display path."""
    def __init__(self, core):
        self.core = core
        if core.hardware_context_type not in (1, 3):
            raise HardwareRenderError("The core requested a hardware context EmuLuna cannot provide.")
        fmt = QSurfaceFormat()
        major, minor = max((3, 3), core.hardware_version)
        fmt.setVersion(major, minor)
        fmt.setProfile(QSurfaceFormat.CoreProfile if core.hardware_context_type == 3
                       else QSurfaceFormat.CompatibilityProfile)
        fmt.setDepthBufferSize(24)
        fmt.setStencilBufferSize(8)
        fmt.setSamples(0)

        self.surface = QOffscreenSurface()
        self.surface.setFormat(fmt)
        self.surface.create()
        self.context = QOpenGLContext()
        self.context.setFormat(fmt)
        if not self.surface.isValid() or not self.context.create():
            self.surface.destroy()
            raise HardwareRenderError("OpenGL could not create the requested offscreen context.")
        if not self.context.makeCurrent(self.surface):
            self.surface.destroy()
            raise HardwareRenderError("OpenGL could not activate the experimental core context.")
        try:
            actual = self.context.format()
            if (actual.majorVersion(), actual.minorVersion()) < (major, minor):
                raise HardwareRenderError(
                    f"The core requires OpenGL {major}.{minor}, but only "
                    f"{actual.majorVersion()}.{actual.minorVersion()} is available."
                )
            if actual.profile() != fmt.profile():
                raise HardwareRenderError("OpenGL did not create the core's requested context profile.")
            target = QOpenGLFramebufferObjectFormat()
            target.setAttachment(QOpenGLFramebufferObject.CombinedDepthStencil)
            self.fbo = QOpenGLFramebufferObject(
                QSize(core.hardware_max_width, core.hardware_max_height), target)
            if not self.fbo.isValid() or not self.fbo.bind():
                raise HardwareRenderError("OpenGL could not create the core render target.")
            self._load_readback_functions()
            core.attach_hardware(self)
        except Exception:
            if hasattr(self, 'fbo'):
                self.fbo = None
            self.context.doneCurrent()
            self.surface.destroy()
            raise
        self.context.doneCurrent()

    @property
    def description(self):
        fmt = self.context.format()
        return f"OpenGL {fmt.majorVersion()}.{fmt.minorVersion()} (experimental)"

    def begin(self):
        if not self.context.makeCurrent(self.surface) or not self.fbo.bind():
            raise HardwareRenderError("The experimental OpenGL context was lost.")
        functions = self.context.functions()
        functions.glClearColor(0.0, 0.0, 0.0, 1.0)
        functions.glClear(0x00004000 | 0x00000100 | 0x00000400)  # color, depth, stencil

    def _load_readback_functions(self):
        """Load the small GL subset needed for deterministic CPU readback."""
        def function(name, result, *arguments):
            address = int(self.context.getProcAddress(name.encode()))
            if not address:
                raise HardwareRenderError(f"OpenGL does not provide {name}.")
            return C.CFUNCTYPE(result, *arguments)(address)

        self._gl_get_integer = function("glGetIntegerv", None, C.c_uint, C.POINTER(C.c_int))
        self._gl_bind_framebuffer = function("glBindFramebuffer", None, C.c_uint, C.c_uint)
        self._gl_bind_buffer = function("glBindBuffer", None, C.c_uint, C.c_uint)
        self._gl_read_buffer = function("glReadBuffer", None, C.c_uint)
        self._gl_pixel_store = function("glPixelStorei", None, C.c_uint, C.c_int)
        self._gl_read_pixels = function(
            "glReadPixels", None, C.c_int, C.c_int, C.c_int, C.c_int,
            C.c_uint, C.c_uint, C.c_void_p)

    def _integer(self, name):
        value = C.c_int()
        self._gl_get_integer(name, C.byref(value))
        return value.value

    def end(self):
        self.context.doneCurrent()

    def capture(self, width, height, bottom_left):
        if width <= 0 or height <= 0 or width > self.fbo.width() or height > self.fbo.height():
            raise HardwareRenderError("The core returned an invalid OpenGL frame size.")

        # A core may leave one of its private framebuffers selected for reading.
        # Read our final target explicitly, then restore every state value that
        # was changed so the core sees the same context on its next frame.
        read_framebuffer = self._integer(0x8CAA)  # GL_READ_FRAMEBUFFER_BINDING
        pixel_pack_buffer = self._integer(0x88ED)  # GL_PIXEL_PACK_BUFFER_BINDING
        read_buffer = self._integer(0x0C02)  # GL_READ_BUFFER
        pack_names = (0x0D05, 0x0D02, 0x0D04, 0x0D03)  # alignment, row length, skips
        pack_values = tuple(self._integer(name) for name in pack_names)
        output = (C.c_ubyte * (width * height * 4))()
        try:
            self._gl_bind_framebuffer(0x8CA8, self.fbo.handle())  # GL_READ_FRAMEBUFFER
            self._gl_read_buffer(0x8CE0)  # GL_COLOR_ATTACHMENT0
            self._gl_bind_buffer(0x88EB, 0)  # GL_PIXEL_PACK_BUFFER
            for name, value in zip(pack_names, (1, 0, 0, 0)):
                self._gl_pixel_store(name, value)
            self._gl_read_pixels(0, 0, width, height, 0x1908, 0x1401,
                                 C.cast(output, C.c_void_p))  # RGBA, UNSIGNED_BYTE
        finally:
            for name, value in zip(pack_names, pack_values):
                self._gl_pixel_store(name, value)
            self._gl_bind_buffer(0x88EB, pixel_pack_buffer)
            self._gl_bind_framebuffer(0x8CA8, read_framebuffer)
            self._gl_read_buffer(read_buffer)

        image = QImage(bytes(output), width, height, width * 4, QImage.Format_RGBA8888).copy()
        if bottom_left:
            image = image.mirrored(False, True)
        return image.convertToFormat(QImage.Format_RGB32)

    def close(self):
        if self.context.makeCurrent(self.surface):
            if self.fbo is not None:
                self.fbo.release()
            self.fbo = None
            self.context.doneCurrent()
        else:
            # The native context was lost; its GPU objects are gone with it.
            self.fbo = None
        self.surface.destroy()

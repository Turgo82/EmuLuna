"""Retain EmuLuna's existing shader presets with Vulkan presentation."""
from PySide6.QtCore import QSize
from PySide6.QtGui import QOffscreenSurface, QOpenGLContext, QSurfaceFormat
from PySide6.QtOpenGL import QOpenGLFramebufferObject

from .shaders import ShaderRenderer, ShaderError


class OpenGLFilterProcessor:
    """Render a selected shader into an offscreen image for Qt Quick upload."""
    def __init__(self):
        fmt = QSurfaceFormat()
        fmt.setVersion(3, 3)
        fmt.setProfile(QSurfaceFormat.CoreProfile)
        self.surface = QOffscreenSurface()
        self.surface.setFormat(fmt)
        self.surface.create()
        self.context = QOpenGLContext()
        self.context.setFormat(fmt)
        if not self.surface.isValid() or not self.context.create() or not self.context.makeCurrent(self.surface):
            self.surface.destroy()
            raise ShaderError("OpenGL 3.3 is required for this video filter.")
        self.fbo = None
        try:
            self.renderer = ShaderRenderer()
        except Exception:
            self.context.doneCurrent()
            self.surface.destroy()
            raise
        self.context.doneCurrent()

    def process(self, frame, size, key, parameters):
        if not self.context.makeCurrent(self.surface):
            raise ShaderError("The video filter's OpenGL context was lost.")
        try:
            if self.fbo is None or self.fbo.size() != size:
                self.fbo = QOpenGLFramebufferObject(QSize(size))
                if not self.fbo.isValid():
                    raise ShaderError("OpenGL could not create the video filter target.")
            viewport = (0, 0, size.width(), size.height())
            self.renderer.display(frame, size, key, parameters, self.fbo.handle(), viewport)
            # QOpenGLFramebufferObject reads bottom-up; the QML image provider
            # expects the first scanline at the top.
            return self.fbo.toImage(False).mirrored(False, True)
        finally:
            self.context.doneCurrent()

    def close(self):
        if self.context.makeCurrent(self.surface):
            self.renderer.close()
            self.fbo = None
            self.context.doneCurrent()
        self.surface.destroy()

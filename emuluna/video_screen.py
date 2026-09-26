"""Direct GL gameplay display with a readable, unfiltered fallback."""
import sys
from PySide6.QtCore import Qt,QRect,QSize,QTimer,Signal
from PySide6.QtGui import QImage,QPainter,QColor,QSurfaceFormat
from PySide6.QtWidgets import QApplication,QWidget
from PySide6.QtOpenGLWidgets import QOpenGLWidget
from .shaders import ShaderRenderer,ShaderError
from .video_filters import VideoRenderer


class GLViewport(QOpenGLWidget):
    def __init__(self,screen):
        super().__init__(screen)
        self.screen=screen;self.renderer=None
        fmt=QSurfaceFormat();fmt.setVersion(3,3);fmt.setProfile(QSurfaceFormat.CoreProfile)
        fmt.setDepthBufferSize(0);fmt.setStencilBufferSize(0);fmt.setSamples(0)
        fmt.setSwapInterval(1)
        self.setFormat(fmt)
        self.setAttribute(Qt.WA_TransparentForMouseEvents)
        self.setFocusPolicy(Qt.NoFocus)

    def initializeGL(self):
        self.context().aboutToBeDestroyed.connect(self.cleanup)
        try:
            self.renderer=ShaderRenderer()
            renderer=self.renderer
            self.screen.backend=('Hardware acceleration · ' if renderer.hardware else 'Software rendering · ')+renderer.renderer
            self.screen.backend_changed.emit(self.screen.backend)
            print('Video renderer: '+self.screen.backend,flush=True)
        except (ShaderError,OSError,RuntimeError) as error:
            self.screen.fail(str(error),fatal=True)

    def paintGL(self):
        r=self.renderer;s=self.screen
        if r is None:return
        if not r.hardware and s.video_filter not in ('nearest','linear'):
            s.fail('Your graphics driver is using software rendering ('+r.renderer+'). Filters are disabled for this session to protect game speed. Check your graphics driver.')
        dpr=self.devicePixelRatioF()
        target=s.target_rect()
        # GL viewport coordinates are physical pixels with a bottom-left origin.
        physical=QSize(round(target.width()*dpr),round(target.height()*dpr))
        viewport=(round(target.x()*dpr),round((s.height()-target.bottom()-1)*dpr),physical.width(),physical.height())
        size=physical
        if s.filter_resolution and size.height()>s.filter_resolution:
            size=QSize(round(size.width()*s.filter_resolution/size.height()),s.filter_resolution)
        try:
            r.display(s.frame,size,s.video_filter,s.shader_parameters,self.defaultFramebufferObject(),viewport)
        except (ShaderError,OSError,RuntimeError,ValueError) as error:
            s.fail(str(error))
            try:r.display(s.frame,size,'nearest',{},self.defaultFramebufferObject(),viewport)
            except (ShaderError,OSError,RuntimeError):s.fail('OpenGL display failed. Using an unfiltered display.',fatal=True)

    def cleanup(self):
        if self.renderer:
            self.makeCurrent()
            self.renderer.close();self.renderer=None
            self.doneCurrent()


class Screen(QWidget):
    shader_failed=Signal(str)
    backend_changed=Signal(str)

    def __init__(self):
        super().__init__()
        self.frame=QImage();self.integer_scale=False;self.display_aspect=None
        self.video_filter='nearest';self.shader_parameters={};self.filter_resolution=0
        self.backend='OpenGL initializes when the game window opens'
        self.renderer=VideoRenderer();self.canvas=None;self._fatal=False
        self.setMinimumSize(320,288);self.setFocusPolicy(Qt.NoFocus)
        # Qt's headless platform plugins cannot host QOpenGLWidget. Production
        # Linux/Windows platforms use GL; tests can exercise the same renderer
        # independently in a headless EGL context.
        if QApplication.platformName() not in ('offscreen','minimal'):
            self.canvas=GLViewport(self)
        else:self.backend='Unfiltered display · no window-system OpenGL context'

    def target_rect(self):
        if self.frame.isNull():return self.rect()
        if self.display_aspect:
            # Cores can expose non-square pixels (notably N64's 640x240 VI
            # modes). Scale whole logical scanlines, then apply the core's
            # display aspect instead of preserving the raw buffer ratio.
            base_h=self.frame.height()
            base_w=max(1,round(base_h*self.display_aspect))
            scale=min(self.width()/base_w,self.height()/base_h)
            if self.integer_scale and scale>=1:
                scale=int(scale)
                h=base_h*scale;w=round(h*self.display_aspect)
            else:
                w=min(self.width(),round(self.height()*self.display_aspect));h=round(w/self.display_aspect)
        else:
            scale=min(self.width()/self.frame.width(),self.height()/self.frame.height())
            if self.integer_scale and scale>=1:scale=int(scale)
            w,h=round(self.frame.width()*scale),round(self.frame.height()*scale)
        return QRect((self.width()-w)//2,(self.height()-h)//2,w,h)

    def integer_size(self):
        """Largest exact integer-scaled display size for the current viewport."""
        if self.frame.isNull():
            return self.size()
        base_h=self.frame.height()
        base_w=max(1,round(base_h*self.display_aspect)) if self.display_aspect else self.frame.width()
        scale=max(1,int(min(self.width()/base_w,self.height()/base_h)))
        return QSize(round(base_h*scale*self.display_aspect),base_h*scale) if self.display_aspect \
            else QSize(base_w*scale,base_h*scale)

    def fail(self,message,fatal=False):
        self.video_filter='nearest'
        if fatal:
            self._fatal=True;self.backend='Unfiltered display · OpenGL unavailable'
            self.backend_changed.emit(self.backend)
            # Do not destroy/hide a GL surface in the middle of paintGL.
            QTimer.singleShot(0,self.fallback)
        self.shader_failed.emit(message)

    def fallback(self):
        if self.canvas:self.canvas.hide()
        self.update()

    def showEvent(self,event):
        super().showEvent(event)
        if self.canvas:QTimer.singleShot(250,self.check_context)

    def check_context(self):
        if self.isVisible() and self.canvas and not self.canvas.isValid() and not self._fatal:
            self.fail('OpenGL 3.3 could not initialize. Update your graphics driver to enable filters.',fatal=True)

    def resizeEvent(self,event):
        super().resizeEvent(event)
        if self.canvas:self.canvas.setGeometry(self.rect())

    def update(self,*args):
        super().update(*args)
        if self.canvas and not self._fatal:self.canvas.update()

    def paintEvent(self,event):
        if self.canvas and not self._fatal:return
        painter=QPainter(self)
        painter.fillRect(self.rect(),QColor('#0c0d10'))
        if not self.frame.isNull():self.renderer.draw(painter,self.frame,self.target_rect(),self.video_filter)
        painter.end()
        if self.video_filter not in ('nearest','linear'):
            self.fail('Filters require a hardware-accelerated OpenGL display. Using an unfiltered display for this session.')

    def close_renderer(self):
        if self.canvas:self.canvas.cleanup()

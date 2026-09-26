"""One shared preset catalog for settings and the in-game filter menu."""
from .shaders import PRESETS
from PySide6.QtGui import QPainter

FILTERS={key:spec['name'] for key,spec in PRESETS.items() if not spec.get('additional')}
ALL_FILTERS={key:spec['name'] for key,spec in PRESETS.items()}
# Preserve older preferences while retiring the CPU-painted approximations.
ALIASES={'scanlines':'shader:zfast-crt','crt':'slang:crt-geom','lcd':'slang:lcd-psp'}


def valid_filter(value):
    value=ALIASES.get(value,value)
    return value if value in ALL_FILTERS else 'nearest'


class VideoRenderer:
    """Unfiltered fallback for systems without a usable OpenGL context."""
    def draw(self,painter,frame,target,mode='nearest',strength=100):
        painter.save()
        painter.setRenderHint(QPainter.SmoothPixmapTransform,mode=='linear')
        painter.drawImage(target,frame)
        painter.restore()

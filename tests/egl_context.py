"""Headless desktop OpenGL for shader tests, even without an X/Wayland socket.

This does not impersonate GPU hardware: tests report GL_RENDERER explicitly.
"""
import ctypes as C


class EGLContext:
    def __init__(self):
        self.egl=C.CDLL('libEGL.so.1');e=self.egl
        signatures={
            'eglGetProcAddress':(C.c_void_p,C.c_char_p),
            'eglInitialize':(C.c_uint,C.c_void_p,C.POINTER(C.c_int),C.POINTER(C.c_int)),
            'eglBindAPI':(C.c_uint,C.c_uint),
            'eglChooseConfig':(C.c_uint,C.c_void_p,C.POINTER(C.c_int),C.POINTER(C.c_void_p),C.c_int,C.POINTER(C.c_int)),
            'eglCreateContext':(C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p,C.POINTER(C.c_int)),
            'eglCreatePbufferSurface':(C.c_void_p,C.c_void_p,C.c_void_p,C.POINTER(C.c_int)),
            'eglMakeCurrent':(C.c_uint,C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p),
            'eglDestroyContext':(C.c_uint,C.c_void_p,C.c_void_p),
            'eglDestroySurface':(C.c_uint,C.c_void_p,C.c_void_p),
            'eglTerminate':(C.c_uint,C.c_void_p),
        }
        for name,(result,*args) in signatures.items():
            fn=getattr(e,name);fn.restype=result;fn.argtypes=args
        address=e.eglGetProcAddress(b'eglGetPlatformDisplayEXT')
        if not address:raise RuntimeError('Surfaceless EGL unavailable')
        get_display=C.CFUNCTYPE(C.c_void_p,C.c_uint,C.c_void_p,C.POINTER(C.c_int))(address)
        self.display=get_display(0x31DD,None,None)
        major=C.c_int();minor=C.c_int()
        if not e.eglInitialize(self.display,C.byref(major),C.byref(minor)):raise RuntimeError('Cannot initialize headless EGL')
        e.eglBindAPI(0x30A2)
        attributes=(C.c_int*11)(0x3033,1,0x3040,8,0x3024,8,0x3023,8,0x3022,8,0x3038)
        config=C.c_void_p();count=C.c_int()
        e.eglChooseConfig(self.display,attributes,C.byref(config),1,C.byref(count))
        if not count.value:raise RuntimeError('No desktop GL config')
        attributes=(C.c_int*7)(0x3098,3,0x30FB,3,0x30FD,1,0x3038)
        self.context=e.eglCreateContext(self.display,config,None,attributes)
        attributes=(C.c_int*5)(0x3057,16,0x3056,16,0x3038)
        self.surface=e.eglCreatePbufferSurface(self.display,config,attributes)
        if not self.context or not self.surface or not e.eglMakeCurrent(self.display,self.surface,self.surface,self.context):raise RuntimeError('Could not create headless OpenGL 3.3')

    def get_proc(self,name):return self.egl.eglGetProcAddress(name)

    def close(self):
        e=self.egl;e.eglMakeCurrent(self.display,None,None,None)
        e.eglDestroySurface(self.display,self.surface);e.eglDestroyContext(self.display,self.context)
        e.eglTerminate(self.display)

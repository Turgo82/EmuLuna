#!/usr/bin/env python3
"""Compare native shader output to the existing renderer on the same GPU.

Requires the built Rust test executable, an exported pack and the existing Qt
Python environment. Only diagnostic image data and temporary assets are used.
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from PySide6.QtCore import QSize
from PySide6.QtGui import QImage,QSurfaceFormat,QOpenGLContext,QOffscreenSurface
from PySide6.QtWidgets import QApplication
from PySide6.QtOpenGL import QOpenGLFramebufferObject
sys.path.insert(0, str(ROOT / "archive/legacy-python"))  # Reference-only parity tooling
from emuluna.shaders import PRESETS
from emuluna.shader_renderer import ShaderRenderer


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pack',type=Path,default=ROOT/'build/rust/shader-pack')
    parser.add_argument('--dynamic',action='store_true',help='Use changing frames to validate temporal history')
    parser.add_argument('--cargo',type=Path,default=ROOT/'build/rust/cargo/bin/cargo')
    args=parser.parse_args()
    output=Path(tempfile.mkdtemp(prefix='emuluna-shader-parity-'))
    env={**os.environ,'EMULUNA_TEST_SHADER_PACK':str(args.pack.resolve()),'EMULUNA_TEST_CAPTURE_DIR':str(output)}
    if args.dynamic:env['EMULUNA_TEST_DYNAMIC_PATTERN']='1'
    result=subprocess.run([str(args.cargo),'test','--manifest-path',str(ROOT/'Cargo.toml'),'-p','emuluna-gl-presenter','live_all_shipped_presets','--','--ignored','--test-threads=1'],env=env,capture_output=True,text=True,timeout=120)
    if result.returncode:
        raise RuntimeError(result.stdout+result.stderr)
    app=QApplication([])
    fmt=QSurfaceFormat();fmt.setVersion(3,3);fmt.setProfile(QSurfaceFormat.CoreProfile)
    surface=QOffscreenSurface();surface.setFormat(fmt);surface.create()
    context=QOpenGLContext();context.setFormat(fmt)
    assert context.create() and context.makeCurrent(surface)
    renderer=ShaderRenderer();target=QOpenGLFramebufferObject(QSize(960,720))
    base=QImage(64,64,QImage.Format_RGB32)
    for y in range(64):
        for x in range(64):base.setPixel(x,y,0xff000000 | ((x*4)<<16) | ((y*4)<<8) | 128)
    frames=[]
    for frame in range(3):
        image=base.copy()
        if args.dynamic:
            for y in range(64):
                for x in range(64):
                    red=255-x*4 if frame==1 else x*4
                    green=255-y*4 if frame==2 else y*4
                    blue={1:64,2:224}.get(frame,128)
                    image.setPixel(x,y,0xff000000 | red<<16 | green<<8 | blue)
        frames.append(image)
    failures=[]
    try:
        for key in sorted(PRESETS):
            for image in frames:renderer.display(image.copy(),QSize(960,720),key,{},target.handle(),(0,0,960,720))
            reference=target.toImage().convertToFormat(QImage.Format_RGB32)
            native=QImage(str(output/(key.replace(':','_')+'.ppm'))).convertToFormat(QImage.Format_RGB32)
            assert reference.size()==native.size(),key
            left=bytes(reference.constBits());right=bytes(native.constBits())
            total=sum(abs(a-b) for index,(a,b) in enumerate(zip(left,right)) if index%4!=3)
            mean=total/(960*720*3)
            peak=max(abs(a-b) for a,b in zip(left,right))
            print(f'{key}: mean_rgb_error={mean:.5f} peak={peak}',flush=True)
            reference.save(str(output/(key.replace(':','_')+'-reference.png')))
            if mean>0.5:failures.append((key,mean,peak))
    finally:
        renderer.close();target=None;context.doneCurrent();surface.destroy()
    print('Diagnostic captures:',output,flush=True)
    if failures:raise AssertionError(failures)
    print('NATIVE_SHADER_PARITY_OK',flush=True)


if __name__=='__main__':main()

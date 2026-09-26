"""Exercise the real GL pipeline, source provenance and live filter controls."""
import ctypes as C
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QImage,QColor,QPainter
from PySide6.QtCore import QCoreApplication,QEvent
from emuluna.shaders import PRESETS,ROOT,ShaderRenderer,ShaderError,parameter_values,is_software_renderer,preset
from emuluna.shader_renderer import Texture
from emuluna.player import Player
from emuluna.library import Library
from core_fixture import install_core
from snes_rom import snes


def pattern():
    source=QImage(64,64,QImage.Format_RGB32);source.fill(QColor('red'))
    p=QPainter(source);p.fillRect(32,0,32,32,QColor('lime'))
    p.fillRect(0,32,32,32,QColor('blue'));p.fillRect(32,32,32,32,QColor('white'));p.end()
    return source


def read_texture(renderer,texture):
    # GPU readback is confined to tests/screenshots, never the game loop.
    gl=renderer.gl;gl.BindFramebuffer(0x8D40,texture.fbo)
    gl.bind('glReadPixels',None,C.c_int,C.c_int,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_void_p)
    data=(C.c_ubyte*(texture.size[0]*texture.size[1]*4))()
    gl.ReadPixels(0,0,*texture.size,0x1908,0x1401,data)
    return QImage(bytes(data),*texture.size,QImage.Format_RGBA8888).copy().flipped()


class CommunityShaders(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.app=QApplication.instance() or QApplication([])

    def test_sources_and_presets_are_complete(self):
        self.assertEqual(len([s for s in PRESETS.values() if s['format']=='slang']),19)
        for spec in PRESETS.values():
            checks=spec.get('files',{spec.get('source',''):spec.get('sha256','')})
            for path,digest in checks.items():self.assertEqual(hashlib.sha256((ROOT/path).read_bytes()).hexdigest(),digest,path)
        for key in PRESETS:
            info=preset(key)
            self.assertEqual(len(info['passes']),PRESETS[key]['passes'])
            for render_pass in info['passes']:self.assertTrue(render_pass['source'].is_file())
            for tex in info['textures'].values():self.assertTrue(tex['path'].is_file())
        self.assertTrue(is_software_renderer('llvmpipe (LLVM)'))
        self.assertTrue(is_software_renderer('GDI Generic'))
        self.assertFalse(is_software_renderer('AMD Radeon RX 6600'))
        self.assertEqual(parameter_values('shader:zfast-lcd',{'BORDERMULT':float('nan')})['BORDERMULT'],14)

    def test_real_gl_all_presets_orientation_resize_caching_and_parameters(self):
        try:
            from egl_context import EGLContext
            context=EGLContext()
        except (OSError,RuntimeError) as error:self.skipTest(str(error))
        renderer=ShaderRenderer(context.get_proc)
        print('Shader test renderer:',renderer.renderer)
        output=Texture(renderer.gl,(256,256),framebuffer=True)
        source=pattern();original=bytes(source.constBits());original_key=source.cacheKey();checksums=[]
        try:
            for key in PRESETS:
                with self.subTest(preset=key):
                    renderer.display(source,(256,256),key,{},output.fbo,(0,0,256,256))
                    image=read_texture(renderer,output)
                    self.assertGreater(image.pixelColor(70,70).red(),image.pixelColor(70,70).blue(),key)
                    self.assertGreater(image.pixelColor(70,190).blue(),image.pixelColor(70,190).red(),key)
                    checksums.append(hashlib.sha256(bytes(image.constBits())).hexdigest())
                    frames=renderer.frames
                    allocations=[t.id.value for group in renderer.outputs for t in group]
                    renderer.render(source,(256,256),key)
                    self.assertEqual(renderer.frames,frames,'HUD repaint must not advance frame history')
                    self.assertEqual(allocations,[t.id.value for group in renderer.outputs for t in group])
                    renderer.render(source,(320,240),key)
                    if key not in ('nearest','linear'):
                        expected=tuple(PRESETS[key].get('minimum_viewport',(320,240)))
                        self.assertEqual(renderer.output.size,expected)
            self.assertGreater(len(set(checksums)),14)
            renderer.display(source,(256,256),'shader:zfast-lcd',{},output.fbo,(0,0,256,256))
            before=bytes(read_texture(renderer,output).constBits())
            renderer.display(source,(256,256),'shader:zfast-lcd',{'BORDERMULT':-40},output.fbo,(0,0,256,256))
            self.assertNotEqual(before,bytes(read_texture(renderer,output).constBits()))
            self.assertEqual(original,bytes(source.constBits()))
            self.assertEqual(source.cacheKey(),original_key,'Uploading must not detach or mutate the core frame')
            # Temporal effects retain previous input frames and only rotate when
            # the core supplies a new frame, not when a HUD or dialog repaints.
            renderer.render(source,(256,256),'slang:motion-blur')
            frames=renderer.frames;source.fill(QColor('blue'))
            renderer.render(source,(256,256),'slang:motion-blur')
            self.assertEqual(renderer.frames,frames+1)
            self.assertGreater(renderer.history_count,0)
            # MAME phosphor feedback must retain light after the source turns
            # black; re-rendering that same paused frame must not decay it.
            source.fill(QColor('red'))
            decay={'phosphortoggle':1,'phosphor_r':.9,'phosphor_g':.9,'phosphor_b':.9}
            renderer.render(source,(256,256),'slang:mame-hlsl',decay)
            source.fill(QColor('black'))
            renderer.render(source,(256,256),'slang:mame-hlsl',decay)
            feedback=read_texture(renderer,renderer.outputs[8][renderer.phase])
            self.assertGreater(feedback.pixelColor(64,64).red(),5)
            renderer.render(source,(256,256),'slang:mame-hlsl',decay)
            unchanged=read_texture(renderer,renderer.outputs[8][renderer.phase])
            self.assertEqual(feedback,unchanged)
            renderer.gl.bind('glGetError',C.c_uint)
            self.assertEqual(renderer.gl.GetError(),0)
        finally:output.close();renderer.close();context.close()

    def test_controls_persist_per_console_and_driver_failure_keeps_saved_choice(self):
        with tempfile.TemporaryDirectory() as folder:
            lib=Library(folder);rom=Path(folder)/'Shader.sfc';rom.write_bytes(snes())
            game=lib.import_file(rom)[0];install_core(lib)
            with patch.object(Player,'setup_audio'):player=Player(lib,game,muted=True)
            player.timer.stop()
            try:
                player.set_video_filter('shader:zfast-crt');player.adjust_shader()
                player.shader_dialog.fields['MASK_DARK'].setValue(.55)
                player.set_video_filter('nearest');player.set_video_filter('shader:zfast-crt')
                self.assertAlmostEqual(player.screen.shader_parameters['MASK_DARK'],.55)
                player.adjust_shader();dialog=player.shader_dialog
                dialog.selector.setCurrentIndex(dialog.selector.findData('slang:crt-geom'))
                self.assertEqual(player.screen.video_filter,'slang:crt-geom')
                dialog.fields['CURVATURE'].setValue(0)
                dialog.resolution.setCurrentIndex(dialog.resolution.findData(720))
                player.persist_shader_parameters()
                self.assertEqual(lib.setting('filter_resolution.snes'),'720')
                dialog.close();self.assertIsNone(player.shader_dialog)
                QCoreApplication.sendPostedEvents(None,QEvent.DeferredDelete)
                player.adjust_shader();self.assertEqual(player.shader_dialog.fields['CURVATURE'].value(),0)
                player.screen.fail('Software rendering detected')
                player.shader_failed('Software rendering detected')
                self.assertEqual(player.screen.video_filter,'nearest');self.assertFalse(player.closed)
                self.assertIn('unavailable',player.notice.text())
                self.assertEqual(lib.setting('video_filter.snes'),'slang:crt-geom')
            finally:player.close();lib.close()

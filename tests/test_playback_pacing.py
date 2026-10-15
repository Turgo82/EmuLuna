"""Audio ordering and bounded catch-up without requiring game files."""
from media_stub import isolate_audio
isolate_audio()
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch
from PySide6.QtGui import QImage
from emuluna.player import Player


class PlaybackPacing(unittest.TestCase):
    def harness(self):
        events=[]
        image=QImage(2,2,QImage.Format_RGB32);image.fill(0xff123456)
        core=Mock(sample_rate=48000,fps=60,aspect=4/3,hardware=False)
        core.frame.side_effect=lambda *args: (events.append('emulate') or (image,b'abcd'))
        core.rumble.return_value=(0,0)
        sink=Mock();sink.bytesFree.return_value=4096
        device=Mock();device.write.side_effect=lambda data: (events.append('audio') or len(data))
        pad=Mock(axes=(0,0,0,0));pad.poll.return_value=0
        screen=Mock();screen.update.side_effect=lambda: events.append('present')
        player=SimpleNamespace(core=core,audio=sink,audio_device=device,audio_pending=bytearray(b'old!'),
            paused=False,focus_paused=False,fast=False,next_frame=100,fast_speed=3,pending_state=None,
            pads=[pad],axis_bindings_by_player=[{}],pressed_axes_by_player=[set()],extra_keys=[],keys=0,
            rumble_enabled=False,rumble_intensity=100,rumble_unavailable_notified=False,
            screen=screen,use_system_aspect=True,_window_aspect=4/3,gameplay_aspect=lambda:4/3,
            n64_border_settings=None,frames=0,frame_limit=0,capture_path=None,skipped_presentations=0,fps_sample_frames=0,
            fps_sample_started=100,diagnostics=Mock(),timer=Mock(),close=Mock())
        player.drain_audio=lambda samples=b'':Player.drain_audio(player,samples)
        player.presentation_due=lambda now,period:Player.presentation_due(player,now,period)
        return player,events

    def test_audio_is_written_before_core_work_and_video_presentation(self):
        player,events=self.harness()
        with patch('emuluna.player.combined_axes',return_value=(0,0,0,0)), \
             patch('emuluna.player.time.perf_counter',side_effect=[100,100.018,100.019]):
            Player.tick(player)
        self.assertEqual(events,['audio','emulate','audio','present'])
        self.assertEqual(player.frames,1)
        self.assertFalse(player.screen.frame.isNull())

    def test_late_normal_frames_keep_every_presentation(self):
        player,events=self.harness()
        with patch('emuluna.player.combined_axes',return_value=(0,0,0,0)), \
             patch('emuluna.player.time.perf_counter',side_effect=[100,100.06,100.06,100.06,100.08,100.08,100.08,100.10,100.10]):
            for _ in range(3):Player.tick(player)
        self.assertEqual(player.core.frame.call_count,3)
        self.assertEqual(events.count('audio'),4)
        self.assertEqual(events.count('present'),3)
        self.assertEqual(player.frames,3)
        self.assertFalse(player.screen.frame.isNull())

    def test_queued_audio_drains_even_when_next_frame_is_not_due(self):
        player,events=self.harness();player.next_frame=101
        with patch('emuluna.player.time.perf_counter',return_value=100):Player.tick(player)
        self.assertEqual(events,['audio'])
        player.core.frame.assert_not_called()

    def test_partial_audio_write_keeps_unwritten_samples_and_fast_forward_skips_audio(self):
        player,_=self.harness();player.audio.bytesFree.return_value=4
        player.audio_device.write.return_value=4
        player.audio_device.write.side_effect=None
        Player.drain_audio(player,b'new!')
        self.assertEqual(player.audio_pending,b'new!')
        player.fast=True;Player.drain_audio(player,b'more')
        self.assertEqual(player.audio_pending,b'new!')

    def test_hardware_frames_and_final_capture_are_always_presented(self):
        player,_=self.harness();player.core.hardware=True
        self.assertTrue(Player.presentation_due(player,102,1/60))
        player.core.hardware=False;player.frame_limit=1
        self.assertTrue(Player.presentation_due(player,102,1/60))

    def test_recovered_deadline_resumes_full_presentation(self):
        player,_=self.harness();player.fast=True
        self.assertFalse(Player.presentation_due(player,101,1/60))
        self.assertTrue(Player.presentation_due(player,100,1/60))
        self.assertEqual(player.skipped_presentations,0)

    def test_normal_speed_keeps_both_phases_of_blinking_sprites(self):
        player,_=self.harness()
        visible=QImage(2,2,QImage.Format_RGB32);visible.fill(0xffff0000)
        invisible=QImage(2,2,QImage.Format_RGB32);invisible.fill(0xff000000)
        frames=[visible,invisible]*3
        player.core.frame.side_effect=[(frame,b'abcd') for frame in frames]
        shown=[]
        player.screen.update.side_effect=lambda: shown.append(player.screen.frame.pixel(0,0))
        clock=[100.0]
        with patch('emuluna.player.combined_axes',return_value=(0,0,0,0)), \
             patch('emuluna.player.time.perf_counter',side_effect=lambda: clock[0]):
            for _ in frames:
                clock[0]+=.06
                Player.tick(player)
        self.assertEqual(shown,[frame.pixel(0,0) for frame in frames])

    def test_missed_deadline_does_not_run_a_burst_of_normal_frames(self):
        player,_=self.harness();player.next_frame=99.94
        with patch('emuluna.player.combined_axes',return_value=(0,0,0,0)), \
             patch('emuluna.player.time.perf_counter',return_value=100):
            Player.tick(player)
        self.assertAlmostEqual(player.next_frame,100+1/60)
        with patch('emuluna.player.time.perf_counter',return_value=100.006):
            Player.tick(player)
        self.assertEqual(player.core.frame.call_count,1)

"""Rolling gameplay timing metrics exclude pauses and expire old drops."""
import os
import unittest
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Signal
from PySide6.QtWidgets import QApplication, QWidget
from emuluna.gameplay import GameplayDiagnostics


class DiagnosticScreen(QWidget):
    backend_changed = Signal(str)
    backend = 'Software'


class PerformanceDisplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.screen = DiagnosticScreen()
        self.screen.resize(640, 480)
        self.display = GameplayDiagnostics(self.screen, show_fps=True)

    def tearDown(self):
        self.screen.close()

    def test_one_percent_low_uses_slowest_intervals_and_graph_renders(self):
        now = 0.0
        self.display.record_frame(now)
        for interval in [1/60] * 99 + [.1]:
            now += interval
            self.display.record_frame(now)
        low, current, peak = self.display.frame_statistics()
        self.assertAlmostEqual(low, 10)
        self.assertAlmostEqual(current, 100)
        self.assertAlmostEqual(peak, 100)
        self.display.set_fps(57)
        self.assertIn('1% low: 10.0 FPS', self.display.text())
        self.assertFalse(self.display.grab().isNull())

    def test_pauses_fast_forward_and_old_spikes_do_not_pollute_metrics(self):
        self.display.record_frame(0)
        self.display.record_frame(.1)
        self.display.reset_frame_clock()
        self.display.record_frame(30)
        self.assertIsNone(self.display.frame_statistics())
        self.display.record_frame(31, normal_speed=False)
        self.display.record_frame(40)
        self.display.record_frame(40 + 1/60)
        low, current, peak = self.display.frame_statistics()
        self.assertIsNone(low)
        self.assertAlmostEqual(current, 1000/60)
        self.assertAlmostEqual(peak, 1000/60)

    def test_renderer_only_display_collects_no_frame_history(self):
        self.display.show_fps = False
        self.display.show_renderer = True
        self.display.record_frame(0)
        self.display.record_frame(1)
        self.display.refresh()
        self.assertEqual(len(self.display.frame_times), 0)
        self.assertNotIn('1% low', self.display.text())
        self.assertEqual(self.display.contentsMargins().bottom(), 0)


if __name__ == '__main__':
    unittest.main()

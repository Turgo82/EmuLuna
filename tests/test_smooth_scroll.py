"""Discrete wheels animate; touchpads and direct navigation remain immediate."""
import os
import unittest
from types import SimpleNamespace
from unittest.mock import Mock
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Qt, QPoint, QPointF, QRect
from PySide6.QtGui import QWheelEvent
from PySide6.QtWidgets import QApplication, QAbstractScrollArea
from PySide6.QtTest import QTest
from emuluna.smooth_scroll import SmoothScroll
from emuluna.app import Window


class SmoothScrollTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.view = QAbstractScrollArea()
        self.controller = SmoothScroll(self.view)
        self.bar = self.view.verticalScrollBar()
        self.bar.setRange(0, 2000)
        self.bar.setValue(500)

    def tearDown(self):
        self.controller.stop()
        self.view.close()

    def wheel(self, angle=-120, pixels=0):
        return QWheelEvent(QPointF(), QPointF(), QPoint(0, pixels), QPoint(0, angle),
                           Qt.NoButton, Qt.NoModifier, Qt.NoScrollPhase, False)

    def test_wheel_animates_accumulates_and_reverses_without_old_momentum(self):
        self.assertTrue(self.controller.handle_wheel(self.wheel()))
        first = self.controller.target
        self.assertGreater(first, 500)
        self.assertEqual(self.bar.value(), 500)
        self.controller.animation.setCurrentTime(80)
        self.assertGreater(self.bar.value(), 500)
        self.assertLess(self.bar.value(), first)
        self.controller.handle_wheel(self.wheel())
        self.assertGreater(self.controller.target, first)
        current = self.bar.value()
        self.controller.handle_wheel(self.wheel(120))
        self.assertLess(self.controller.target, current)
        self.controller.animation.setCurrentTime(160)
        self.assertEqual(self.bar.value(), round(self.controller.target))

    def test_pixel_scrolling_is_direct_and_scrollbar_navigation_cancels_animation(self):
        self.controller.handle_wheel(self.wheel())
        self.controller.handle_wheel(self.wheel(angle=0, pixels=40))
        self.assertEqual(self.bar.value(), 460)
        self.controller.handle_wheel(self.wheel())
        self.bar.setValue(1500)
        QTest.qWait(180)
        self.assertEqual(self.bar.value(), 1500)
        self.controller.handle_wheel(self.wheel())
        self.controller.animation.setCurrentTime(160)
        self.assertGreater(self.bar.value(), 1500)

    def test_scroll_is_clamped_at_library_bounds(self):
        self.bar.setValue(1990)
        self.controller.handle_wheel(self.wheel(-1200))
        self.controller.animation.setCurrentTime(160)
        self.assertEqual(self.bar.value(), 2000)

    def test_prefetch_skips_offscreen_cards_in_large_library(self):
        count = 10000
        items = [SimpleNamespace(data=lambda role, i=i: str(i)) for i in range(count)]
        games = SimpleNamespace(count=lambda: count, item=lambda i: items[i],
            isVisible=lambda: True, cover_height=256,
            viewport=lambda: SimpleNamespace(rect=lambda: QRect(0, 0, 800, 600)))
        games.visualItemRect = Mock(side_effect=lambda item:
            QRect(0, int(item.data(Qt.UserRole))//4*300-9000, 200, 300))
        window = SimpleNamespace(closing=False, library_tab='library', view_mode='grid',
            games=games, rows={str(i): {'id':str(i)} for i in range(count)}, thumbnails=Mock())
        Window.prefetch_covers(window)
        self.assertLess(games.visualItemRect.call_count, 60)
        self.assertGreater(window.thumbnails.icon.call_count, 0)
        self.assertLess(window.thumbnails.icon.call_count, 40)


if __name__ == '__main__':
    unittest.main()

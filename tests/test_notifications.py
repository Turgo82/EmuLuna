"""Library messages stay accessible without a footer or intrusive popups."""
import os
import tempfile
import unittest
from unittest.mock import Mock, patch
from media_stub import isolate_audio
isolate_audio()
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QLabel, QStatusBar
from emuluna.app import Window
from emuluna.library import Library


class Notifications(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.library = Library(self.temp.name)
        self.window = Window(self.library, auto_artwork=False)
        self.window.show()
        QTest.qWait(20)

    def tearDown(self):
        self.window.worker = None
        self.window.close()
        self.temp.cleanup()

    def test_footer_removed_bell_marks_read_and_history_is_bounded(self):
        window = self.window
        self.assertFalse(hasattr(window, 'selection'))
        self.assertFalse(hasattr(window, 'count'))
        self.assertFalse(hasattr(window, 'media_open_button'))
        self.assertIsNone(window.findChild(QStatusBar))
        self.assertFalse(any('Select a game to play' in label.text() for label in window.findChildren(QLabel)))
        bell = window.notifications
        bell.post('Artwork finished')
        self.assertEqual(bell.unread, 1)
        self.assertFalse(bell.panel.isVisible())
        QTest.mouseClick(bell, Qt.LeftButton)
        self.assertTrue(bell.panel.isVisible())
        self.assertEqual(bell.unread, 0)
        bell.panel.hide()
        for i in range(250): bell.post(f'Message {i}')
        self.assertEqual(bell.history.count(), 100)
        self.assertIn('Message 249', bell.history.item(0).text())
        bell.toggle_panel()
        QTest.mouseClick(bell.clear_button, Qt.LeftButton)
        self.assertEqual(bell.history.count(), 0)
        self.assertTrue(bell.empty.isVisible())

    def test_progress_coalesces_cancel_and_issue_resolution_remain_accessible(self):
        window = self.window
        bell = window.notifications
        window.worker = Mock()
        window.worker.isRunning.return_value = False
        window.import_progress.show()
        window.cancel_import.show()
        for i in range(101): window.import_progress_changed(i, 100, f'Importing {i}/100')
        self.assertEqual(bell.history.count(), 1)
        self.assertEqual(window.import_progress.value(), 100)
        bell.toggle_panel()
        self.assertTrue(window.cancel_import.isVisible())
        QTest.mouseClick(window.cancel_import, Qt.LeftButton)
        window.worker.requestInterruption.assert_called_once()
        bell.finish('import', 'Import finished')
        self.assertEqual(bell.history.count(), 1)
        self.assertNotIn('import', bell.active)
        self.library.record_import_issue('/example/game.bin', 'ambiguous', 'Choose a console')
        window.update_issues()
        self.assertTrue(window.issues_button.isVisible())
        with patch('emuluna.app.ImportIssuesDialog') as dialog:
            QTest.mouseClick(window.issues_button, Qt.LeftButton)
            dialog.return_value.show.assert_called_once()
        window.issue_dialog = None
        self.assertFalse(bell.panel.isVisible())
        self.library.resolve_import_issue('/example/game.bin')
        window.update_issues()
        self.assertEqual(bell.last_message, 'Import issues resolved.')
        self.assertTrue(window.issues_button.isHidden())
        # A late importer signal must not touch a database after the window closes.
        window.worker = None
        window.close()
        window.import_done(1, [])

    def test_sidebar_add_button_transitions_through_real_task_progress(self):
        activity = self.window.sidebar_activity
        sidebar = self.window.findChild(type(activity), 'sidebarActivity').parentWidget()
        self.assertEqual(activity.pages.currentIndex(), 0)
        self.assertTrue(activity.add_button.isVisible())
        self.assertGreaterEqual(activity.geometry().top(), sidebar.height() - activity.height() - 14)
        self.assertEqual([action.text() for action in activity.add_button.menu().actions() if not action.isSeparator()],
                         ['Import games…', 'Import folder…', 'New collection…', 'New smart collection…'])

        cancelled = []
        activity.begin('import', 'Game Scanner', 'Scanning folders…', cancel=lambda: cancelled.append(True))
        QTest.qWait(240)
        self.assertEqual(activity.pages.currentIndex(), 1)
        self.assertEqual(activity.maximumHeight(), activity.EXPANDED_HEIGHT)
        activity.update('import', detail='Processed 3 of 5 files', current=3, total=5)
        self.assertEqual((activity.progress.value(), activity.progress.maximum()), (3, 5))
        QTest.mouseClick(activity.cancel_button, Qt.LeftButton)
        self.assertEqual(cancelled, [True])

        activity.update_message('artwork', 'Downloading covers',
                                'Finding box art… 4 of 9 · Example Game')
        self.assertEqual((activity.progress.value(), activity.progress.maximum()), (4, 9))
        activity.finish('artwork')
        self.assertEqual(activity.title.text(), 'Game Scanner')
        activity.finish('import')
        QTest.qWait(700)
        self.assertEqual(activity.pages.currentIndex(), 0)
        self.assertEqual(activity.maximumHeight(), activity.COLLAPSED_HEIGHT)

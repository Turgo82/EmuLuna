"""First-run settings persist only after successful setup; developer previews never do."""
import os
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtWidgets import QApplication, QDialog
from PySide6.QtTest import QTest
from emuluna.library import Library
from emuluna.onboarding import OnboardingDialog, needs_onboarding
from emuluna.settings import SettingsDialog


class Onboarding(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name)
        self.lib=Library(self.root/'app')
        self.dialogs=[]

    def tearDown(self):
        for dialog in self.dialogs: dialog.close()
        self.lib.close();self.temp.cleanup()

    def dialog(self,preview=False):
        dialog=OnboardingDialog(self.lib,preview=preview)
        self.dialogs.append(dialog)
        return dialog

    def test_new_library_defaults_and_completion_survive_reopen(self):
        self.assertTrue(needs_onboarding(self.lib))
        dialog=self.dialog()
        self.assertEqual(dialog.theme.currentData(),'luna')
        self.assertEqual(dialog.path.text(),str(self.lib.root/'consoles'))
        dialog.theme.setCurrentIndex(dialog.theme.findData('daylight'))
        dialog.copy_games.setChecked(False)
        dialog.finish_setup()
        self.assertEqual(dialog.result(),QDialog.Accepted)
        self.assertFalse(needs_onboarding(self.lib))
        self.lib.close();self.lib=Library(self.root/'app')
        self.assertFalse(needs_onboarding(self.lib))
        self.assertEqual(self.lib.setting('appearance.theme'),'daylight')
        self.assertEqual(self.lib.setting('copy_games'),'0')
        self.assertTrue(self.lib.console_layout())

    def test_dismissal_keeps_setup_pending_without_changes(self):
        dialog=self.dialog();dialog.theme.setCurrentIndex(dialog.theme.findData('classic'))
        dialog.reject()
        self.assertTrue(needs_onboarding(self.lib))
        self.assertEqual(self.lib.setting('appearance.theme','luna'),'luna')

    def test_existing_library_does_not_get_setup_on_update(self):
        self.lib.db.execute("DELETE FROM settings WHERE key='onboarding.complete'");self.lib.db.commit()
        self.lib.close();self.lib=Library(self.root/'app')
        self.assertFalse(needs_onboarding(self.lib))

    def test_preview_is_side_effect_free_even_with_new_folder(self):
        dialog=self.dialog(preview=True)
        target=self.root/'chosen';target.mkdir()
        with patch('emuluna.onboarding.QFileDialog.getExistingDirectory',return_value=str(target)):
            dialog.choose_folder()
        dialog.theme.setCurrentIndex(dialog.theme.findData('classic'))
        dialog.copy_games.setChecked(False)
        dialog.finish_setup()
        self.assertEqual(dialog.result(),QDialog.Accepted)
        self.assertTrue(needs_onboarding(self.lib))
        self.assertEqual(self.lib.setting('appearance.theme','luna'),'luna')
        self.assertEqual(self.lib.setting('copy_games','1'),'1')
        self.assertFalse((target/'consoles').exists())

    def test_setup_prepares_selected_drive_and_new_imports_use_it(self):
        dialog=self.dialog();target=self.root/'chosen';target.mkdir()
        dialog.destination=target
        dialog.finish_setup()
        for _ in range(200):
            if not dialog.worker: break
            QTest.qWait(10)
        self.assertIsNone(dialog.worker)
        self.assertEqual(dialog.result(),QDialog.Accepted)
        self.assertFalse(needs_onboarding(self.lib))
        self.assertEqual((self.lib.root/'consoles').resolve(),target/'consoles')
        source=self.root/'Example.nes';source.write_bytes(b'new game')
        key=self.lib.import_file(source)[0]
        self.assertTrue(self.lib.validate_game(key).resolve().is_relative_to(target))

    def test_invalid_destination_leaves_setup_pending_and_original_location(self):
        dialog=self.dialog();target=self.root/'chosen';(target/'consoles').mkdir(parents=True)
        (target/'consoles/precious').write_text('keep')
        dialog.destination=target;dialog.finish_setup()
        for _ in range(200):
            if not dialog.worker: break
            QTest.qWait(10)
        self.assertTrue(needs_onboarding(self.lib))
        self.assertTrue(dialog.finish.isEnabled())
        self.assertIn('already contains',dialog.status.text())
        self.assertEqual((target/'consoles/precious').read_text(),'keep')
        self.assertFalse((self.lib.root/'consoles').is_symlink())

    def test_advanced_dev_options_launch_preview_without_resetting_flag(self):
        settings=SettingsDialog(self.lib,advanced_unlocked=True)
        self.dialogs.append(settings)
        settings.show_page('advanced')
        with patch('emuluna.onboarding.OnboardingDialog') as fake:
            settings.preview_onboarding_button.click()
            fake.assert_called_once_with(self.lib,settings,preview=True)
            fake.return_value.exec.assert_called_once()
        self.assertTrue(needs_onboarding(self.lib))

    def test_back_next_flow_keeps_choices_and_applies_only_on_last_page(self):
        dialog=self.dialog()
        dialog.show()
        self.assertEqual(dialog.step_label.text(),'1 of 5')
        self.assertFalse(dialog.back.isEnabled())
        for _ in range(2): dialog.finish.click()
        self.assertEqual(dialog.pages.currentIndex(),2)
        dialog.theme.setCurrentIndex(dialog.theme.findData('classic'))
        dialog.finish.click()
        dialog.auto_cores.setChecked(False)
        dialog.back.click()
        self.assertEqual(dialog.theme.currentData(),'classic')
        self.assertEqual(self.lib.setting('appearance.theme','luna'),'luna')
        self.assertTrue(needs_onboarding(self.lib))
        dialog.finish.click();dialog.finish.click()
        self.assertEqual(dialog.pages.currentIndex(),4)
        self.assertIn('Classic',dialog.summary.text())
        self.assertIn('Install manually',dialog.summary.text())
        self.assertEqual(dialog.finish.text(),'Let’s go!')
        dialog.finish.click()
        self.assertFalse(needs_onboarding(self.lib))
        self.assertEqual(self.lib.setting('appearance.theme'),'classic')
        self.assertEqual(self.lib.setting('core_auto_install'),'0')

    def test_animation_continues_across_setup_without_restarting_and_stops_after_close(self):
        dialog=self.dialog(preview=True);dialog.show()
        QTest.qWait(45)
        self.assertTrue(dialog.welcome_logo.timer.isActive())
        first=dialog.welcome_logo.grab().toImage()
        QTest.qWait(90)
        self.assertGreater(dialog.welcome_logo.frame_time,0)
        self.assertNotEqual(first,dialog.welcome_logo.grab().toImage())
        before=dialog.welcome_logo.frame_time
        dialog.finish.click()
        self.assertTrue(dialog.welcome_logo.timer.isActive())
        QTest.qWait(45)
        self.assertGreater(dialog.welcome_logo.frame_time,before)
        for _ in range(50):
            if dialog.page_opacity.opacity() == 1.0: break
            QTest.qWait(20)
        self.assertEqual(dialog.page_opacity.opacity(),1.0)
        before=dialog.welcome_logo.frame_time
        dialog.back.click()
        self.assertTrue(dialog.welcome_logo.timer.isActive())
        self.assertGreaterEqual(dialog.welcome_logo.frame_time,before)
        for page in range(5):
            dialog.show_step(page)
            before=dialog.welcome_logo.frame_time
            QTest.qWait(45)
            self.assertGreater(dialog.welcome_logo.frame_time,before)
        dialog.close()
        self.assertFalse(dialog.welcome_logo.timer.isActive())
        self.assertTrue(needs_onboarding(self.lib))

    def test_rapid_navigation_does_not_leave_pages_faded_or_timers_running(self):
        dialog=self.dialog(preview=True);dialog.show()
        for index in (1,0,3,2,4): dialog.show_step(index)
        for _ in range(50):
            if dialog.page_opacity.opacity() == 1.0: break
            QTest.qWait(20)
        self.assertEqual(dialog.pages.currentIndex(),4)
        self.assertEqual(dialog.page_opacity.opacity(),1.0)
        self.assertTrue(dialog.welcome_logo.timer.isActive())
        dialog.finish.click()
        self.assertEqual(dialog.result(),QDialog.Accepted)
        self.assertFalse(dialog.welcome_logo.timer.isActive())
        self.assertTrue(needs_onboarding(self.lib))

    def test_responsive_layout_and_fullscreen_escape_keep_setup_open(self):
        from PySide6.QtCore import Qt
        from PySide6.QtWidgets import QBoxLayout
        dialog=self.dialog(preview=True);dialog.show()
        for width,height,direction in ((680,800,QBoxLayout.TopToBottom),
                                      (1100,720,QBoxLayout.LeftToRight),
                                      (1920,1080,QBoxLayout.LeftToRight)):
            dialog.resize(width,height);QTest.qWait(25)
            self.assertEqual(dialog.content_layout.direction(),direction)
            for step in range(5):
                dialog.show_step(step);QTest.qWait(10)
                self.assertTrue(dialog.rect().contains(dialog.card.geometry().translated(dialog.content.pos())))
                self.assertTrue(dialog.finish.isVisible())
        dialog.fullscreen.click();QTest.qWait(25)
        self.assertTrue(dialog.isFullScreen())
        QTest.keyClick(dialog,Qt.Key_Escape);QTest.qWait(25)
        self.assertFalse(dialog.isFullScreen())
        self.assertTrue(dialog.isVisible())
        self.assertTrue(needs_onboarding(self.lib))
        dialog.close()

    def test_manual_core_picker_preview_keeps_choices_without_downloads(self):
        from emuluna.systems import SYSTEMS
        dialog=self.dialog(preview=True);dialog.show_step(3);dialog.show()
        self.assertTrue(dialog.console_picker.isHidden())
        dialog.auto_cores.setChecked(False)
        self.assertFalse(dialog.console_picker.isHidden())
        key=next(key for key,check in dialog.console_checks.items() if check.isEnabled())
        dialog.console_checks[key].setChecked(True)
        dialog.auto_cores.setChecked(True);dialog.auto_cores.setChecked(False)
        self.assertEqual(dialog.selected_consoles(),[key])
        self.assertEqual(len(dialog.console_checks),len(SYSTEMS))
        dialog.show_step(4)
        self.assertIn('1 selected consoles',dialog.summary.text())
        with patch('emuluna.core_manager.CoreWorker.start') as download:
            dialog.finish_setup()
            download.assert_not_called()
        self.assertTrue(needs_onboarding(self.lib))

    def test_manual_downloads_deduplicate_shared_cores_and_keep_auto_off(self):
        from emuluna.core_manager import CoreWorker
        from emuluna.systems import SYSTEMS
        dialog=self.dialog();dialog.auto_cores.setChecked(False)
        keys=[key for key,check in dialog.console_checks.items() if check.isEnabled()][:6]
        for key in keys: dialog.console_checks[key].setChecked(True)
        expected=list(dict.fromkeys(SYSTEMS[key].default_core for key in keys))
        def run(worker):
            worker.run();worker.finished.emit()
        with patch('emuluna.core_manager.CoreManager.installed',return_value={}), \
             patch('emuluna.core_manager.CoreManager.remote_index',return_value={}), \
             patch('emuluna.core_manager.CoreManager.download',return_value=(None,True)) as download, \
             patch.object(CoreWorker,'start',run):
            dialog.finish_setup()
        self.assertEqual([call.args[0] for call in download.call_args_list],expected)
        self.assertEqual(self.lib.setting('core_auto_install'),'0')
        self.assertFalse(needs_onboarding(self.lib))

    def test_manual_core_failure_keeps_setup_pending_and_allows_changes(self):
        from emuluna.core_manager import CoreWorker
        dialog=self.dialog();dialog.auto_cores.setChecked(False)
        next(check for check in dialog.console_checks.values() if check.isEnabled()).setChecked(True)
        def run(worker):
            worker.run();worker.finished.emit()
        with patch('emuluna.core_manager.CoreManager.installed',return_value={}), \
             patch('emuluna.core_manager.CoreManager.remote_index',return_value={}), \
             patch('emuluna.core_manager.CoreManager.download',side_effect=RuntimeError('Offline')), \
             patch.object(CoreWorker,'start',run):
            dialog.finish_setup()
        self.assertTrue(needs_onboarding(self.lib))
        self.assertIsNone(dialog.worker)
        self.assertTrue(dialog.console_picker.isEnabled())
        self.assertIn('Offline',dialog.status.text())

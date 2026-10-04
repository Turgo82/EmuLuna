"""Alphabet navigation respects the current scope, search and sort order."""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import tempfile
import time
import unittest
from pathlib import Path
from media_stub import isolate_audio
isolate_audio()
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from emuluna.app import Window
from emuluna.library import Library
from emuluna.settings import SettingsDialog
from emuluna.library_widgets import title_initial


class AlphabetNavigation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_jumps_scoped_results_in_both_views(self):
        with tempfile.TemporaryDirectory() as folder:
            lib = Library(Path(folder))
            records = []
            for i, letter in enumerate('ABCDEFGHIJKLMNOPQRSTUVWXYZ'):
                for number in range(6):
                    name = f'{letter} game {number:02}'
                    records.append((f'{i*6+number:064x}', name, 'snes' if number % 2 else 'nes',
                                    'roms/' + name + '.rom', '', time.time(), name + '.rom'))
            records.append(('f'*64, '2020 Baseball', 'snes', 'roms/2020.sfc', '', time.time(), '2020.sfc'))
            with lib.db:
                lib.db.executemany('INSERT INTO games(id,title,system,rom_path,source,added,original_filename) VALUES(?,?,?,?,?,?,?)', records)
            window = Window(lib, auto_artwork=False)
            try:
                window.show(); QTest.qWait(40)
                rail = window.alphabet_index
                self.assertTrue(rail.isVisible())
                self.assertEqual(window.table.rowCount(), 0)  # Hidden list stays lazy.
                for size in ((1140,750), (850,560)):
                    window.resize(*size); QTest.qWait(30)
                    self.assertTrue(window.library_scrollbar.isVisible())
                    self.assertTrue(rail.rect().contains(rail.buttons['Z'].geometry()))
                    self.assertGreater(window.library_scrollbar.geometry().left(), rail.geometry().right())
                    edge = window.library_scrollbar.mapTo(window.centralWidget(), window.library_scrollbar.rect().bottomRight())
                    self.assertEqual(edge.x(), window.centralWidget().rect().right())
                    self.assertEqual(edge.y(), window.centralWidget().rect().bottom())
                    self.assertFalse(window.games.verticalScrollBar().isVisible())
                    QTest.mouseClick(rail.buttons['Z'], Qt.LeftButton)
                    self.assertEqual(window.rows[window.selected_ids()[0]]['title'], 'Z game 00')
                    self.assertTrue(window.games.viewport().rect().intersects(window.games.visualItemRect(window.games.currentItem())))
                    self.assertGreater(window.games.verticalScrollBar().value(), 0)
                    self.assertEqual(window.library_scrollbar.value(), window.games.verticalScrollBar().value())
                    window.library_scrollbar.setValue(window.library_scrollbar.maximum())
                    self.assertEqual(window.games.verticalScrollBar().value(), window.library_scrollbar.maximum())
                    QTest.mouseClick(rail.buttons['#'], Qt.LeftButton)
                    self.assertEqual(window.rows[window.selected_ids()[0]]['title'], '2020 Baseball')
                snes = next(i for i in range(window.nav.count()) if window.nav.item(i).data(Qt.UserRole) == 'snes')
                window.nav.setCurrentRow(snes)
                QTest.mouseClick(rail.buttons['Z'], Qt.LeftButton)
                self.assertEqual(window.rows[window.selected_ids()[0]]['title'], 'Z game 01')
                window.search.setText('M game')
                QTest.qWait(30)
                # Large fixed covers can still need scrolling after filtering
                # to three games in a narrow window. Follow the actual bounds.
                rects = [window.games.visualItemRect(window.games.item(i))
                         for i in range(window.games.count())]
                self.assertEqual(window.library_scrollbar.isVisible(),
                                 max(rect.bottom() for rect in rects) + 1 - min(rect.top() for rect in rects)
                                 > window.games.viewport().height())
                self.assertEqual([key for key,button in rail.buttons.items() if button.isEnabled()], ['M'])
                window.change_view('list')
                QTest.qWait(30)
                self.assertFalse(window.library_scrollbar.isVisible())
                window.table.sortItems(0, Qt.DescendingOrder)
                self.assertIs(window.library_scrollbar.view, window.table)
                self.assertFalse(window.table.verticalScrollBar().isVisible())
                QTest.mouseClick(rail.buttons['M'], Qt.LeftButton)
                self.assertEqual(window.rows[window.selected_ids()[0]]['title'], 'M game 05')
                self.assertEqual(window.search.text(), 'M game')
                self.assertEqual(window.table.horizontalHeader().sortIndicatorOrder(), Qt.DescendingOrder)
                window.change_library_tab('states')
                self.assertFalse(rail.isVisible())
                self.assertFalse(window.library_scrollbar.isVisible())
                window.change_library_tab('screenshots')
                self.assertFalse(rail.isVisible())
                window.change_library_tab('library')
                self.assertTrue(rail.isVisible())
                window.search.setText('nothing matches')
                self.assertFalse(rail.isVisible())
                self.assertFalse(any(button.isEnabled() for button in rail.buttons.values()))
                window.search.clear()
                QTest.qWait(30)
                self.assertTrue(window.library_scrollbar.isVisible())
                collection = lib.save_collection('Mixed selection')
                lib.add_to_collection(collection, [records[0][0], records[-2][0]])
                window.rebuild_sidebar(f'collection:{collection}'); window.refresh()
                self.assertEqual([key for key,button in rail.buttons.items() if button.isEnabled()], ['A','Z'])
                settings = SettingsDialog(lib, window)
                settings.changed.connect(window.refresh)
                settings.show_alphabet_index.setChecked(False)
                self.assertFalse(rail.isVisible())
                self.assertEqual(lib.setting('library.show_alphabet_index'), '0')
                settings.show_alphabet_index.setChecked(True)
                self.assertTrue(rail.isVisible())
                self.assertEqual(lib.setting('library.show_alphabet_index'), '1')
                settings.close()
            finally:
                window.close()

    def test_title_initials(self):
        for title, expected in [('Éclair','E'), ('  “Mario”','M'), ('2020 Baseball','#'),
                                ('!','#'), ('日本語','#'), ('The Legend','T')]:
            self.assertEqual(title_initial(title), expected)

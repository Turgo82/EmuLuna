"""First-run setup and a side-effect-free developer preview."""
from pathlib import Path
import math

from PySide6.QtCore import Qt, QTimer, QElapsedTimer, QRectF, QPointF, QPropertyAnimation, QEasingCurve
from PySide6.QtGui import QPixmap, QPainter, QLinearGradient, QRadialGradient, QPalette, QColor, QPen, QShortcut, QKeySequence, QPainterPath
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QBoxLayout, QSizePolicy, QFormLayout,
    QLabel, QLineEdit, QPushButton, QFileDialog, QMessageBox, QWidget, QStackedWidget, QFrame, QGraphicsOpacityEffect, QScrollArea)

from .branding import LOGO
from .systems import SYSTEMS, CATALOG
from .library_paths import console_root
from .settings_style import SettingsComboBox, SettingsCheckBox
from .theme import apply_theme_style, follow_system_theme, select_theme
from .theme_files import ThemeRegistry


def needs_onboarding(library):
    # Only databases created with first-run setup receive a pending flag.
    # Existing libraries are not unexpectedly sent through setup after an update.
    return library.setting('onboarding.complete', '1') == '0'


class AnimatedWelcomeLogo(QWidget):
    """Native Qt intro; no video decoder or emulation graphics context required."""
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setMinimumSize(240, 180)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.scaled_logo = None
        self.scaled_logo_key = None
        self.setAccessibleName('EmuLuna animated welcome logo')
        self.logo = QPixmap(str(LOGO))
        self.clock = QElapsedTimer()
        self.frame_time = 0.0
        self.motion = True
        self.timer = QTimer(self)
        self.timer.setInterval(33)
        self.timer.timeout.connect(self.advance)

    def advance(self):
        self.frame_time = self.clock.elapsed() / 1000.0
        self.update()
        self.window().update()

    def set_motion(self, enabled):
        self.motion = enabled
        if enabled and self.isVisible():
            self.clock.restart()
            self.frame_time = 0.0
            self.timer.start()
        else:
            self.timer.stop()
        self.update()

    def showEvent(self, event):
        super().showEvent(event)
        self.set_motion(self.motion)

    def hideEvent(self, event):
        self.timer.stop()
        super().hideEvent(event)

    def paintEvent(self, event):
        time = self.frame_time if self.motion else 0.9
        progress = max(0.0, min(1.0, time / 0.9))
        ease = 1.0 - (1.0 - progress) ** 3
        painter = QPainter(self)
        painter.setRenderHint(QPainter.SmoothPixmapTransform)
        painter.setRenderHint(QPainter.Antialiasing)
        star_color = QColor('#d9deff')
        painter.save()
        painter.scale(self.width() / 560, self.height() / 312)
        for index, (x, y, radius) in enumerate(((32, 55, 2), (96, 135, 1.5),
                (145, 29, 2.5), (435, 42, 2), (517, 115, 2.8), (467, 238, 2),
                (60, 264, 2.5), (490, 288, 1.5), (108, 222, 1.4))):
            pulse = (1 + math.sin(time * (1.5 + index * .13) + index * 1.7)) / 2
            color = QColor(star_color)
            color.setAlphaF(ease * (.18 + .65 * pulse))
            painter.setPen(QPen(color, 1.2, Qt.SolidLine, Qt.RoundCap))
            span = radius * (.7 + .5 * pulse)
            painter.drawLine(QPointF(x - span, y), QPointF(x + span, y))
            painter.drawLine(QPointF(x, y - span), QPointF(x, y + span))
        painter.restore()
        # Keep the artwork at its final size throughout the reveal. The existing
        # logo contains its own lighting; additional glow obscures its edges.
        target_height = min(self.height() * .94, self.width() * .94, 540.0)
        target_width = target_height * self.logo.width() / max(1, self.logo.height())
        target = QRectF((self.width() - target_width) / 2,
                        (self.height() - target_height) / 2 + 8 * (1 - ease),
                        target_width, target_height)
        painter.setOpacity(ease)
        # Pre-filter the high-resolution PNG at the physical screen resolution.
        # This avoids rough edges from a single large painter downscale on HiDPI.
        dpr = self.devicePixelRatioF()
        key = (round(target_width * dpr), round(target_height * dpr), dpr)
        if key != self.scaled_logo_key:
            image = self.logo.toImage().scaled(key[0], key[1], Qt.KeepAspectRatio,
                                              Qt.SmoothTransformation)
            self.scaled_logo = QPixmap.fromImage(image)
            self.scaled_logo.setDevicePixelRatio(dpr)
            self.scaled_logo_key = key
        painter.save()
        center = target.center()
        if self.motion:
            painter.translate(center.x() + 3 * math.sin(time * .65) * ease,
                              center.y() + 4 * math.sin(time * .85) * ease)
            painter.rotate(1.3 * math.sin(time * .55) * ease)
            painter.translate(-center.x(), -center.y())
        painter.drawPixmap(target, self.scaled_logo, QRectF(self.scaled_logo.rect()))
        painter.restore()


class OnboardingDialog(QDialog):
    def __init__(self, library, parent=None, *, preview=False):
        super().__init__(parent)
        self.library, self.preview = library, preview
        self.worker = None
        self.setWindowTitle('EmuLuna Setup Assistant' if not preview else 'Setup Assistant preview — EmuLuna')
        self.setWindowFlags(self.windowFlags() | Qt.WindowMaximizeButtonHint)
        self.resize(1100, 720)
        self.setMinimumSize(640, 620)
        follow_system_theme(library)
        self.setObjectName('onboarding')
        apply_theme_style(self, 'onboarding')
        layout = QVBoxLayout(self)
        self.outer_layout = layout
        layout.setContentsMargins(36, 24, 36, 24)
        layout.setSpacing(16)
        top = QHBoxLayout()
        eyebrow = QLabel('EMULUNA  /  SETUP')
        eyebrow.setObjectName('assistantEyebrow')
        top.addWidget(eyebrow); top.addStretch()
        self.fullscreen = QPushButton('Fullscreen')
        self.fullscreen.setObjectName('assistantFullscreen')
        self.fullscreen.clicked.connect(self.toggle_fullscreen)
        top.addWidget(self.fullscreen)
        layout.addLayout(top)
        self.fullscreen_shortcut = QShortcut(QKeySequence('F11'), self)
        self.fullscreen_shortcut.activated.connect(self.toggle_fullscreen)
        self.content = QWidget()
        self.content.setMaximumWidth(1440)
        self.content_layout = QBoxLayout(QBoxLayout.LeftToRight, self.content)
        self.content_layout.setContentsMargins(0, 0, 0, 0)
        self.content_layout.setSpacing(40)
        layout.addWidget(self.content, 1, Qt.AlignHCenter)
        self.welcome_logo = AnimatedWelcomeLogo(self)
        self.content_layout.addWidget(self.welcome_logo, 1)
        card = QFrame()
        card.setObjectName('assistantCard')
        self.card = card
        card.setMinimumWidth(380)
        card.setMaximumWidth(560)
        body = QVBoxLayout(card)
        body.setContentsMargins(32, 34, 32, 26)
        body.setSpacing(16)
        self.pages = QStackedWidget()
        self.page_opacity = QGraphicsOpacityEffect(self.pages)
        self.page_opacity.setOpacity(1.0)
        self.pages.setGraphicsEffect(self.page_opacity)
        self.page_fade = QPropertyAnimation(self.page_opacity,b'opacity',self)
        self.page_fade.setDuration(220)
        self.page_fade.setEasingCurve(QEasingCurve.OutCubic)
        body.addWidget(self.pages,1)
        if preview:
            notice = QLabel('Developer preview · No files or settings will change.')
            notice.setWordWrap(True)
            body.addWidget(notice)
        self.status = QLabel('')
        self.status.setTextFormat(Qt.PlainText)
        self.status.setWordWrap(True)
        self.status.hide()
        body.addWidget(self.status)
        line = QFrame(); line.setFrameShape(QFrame.HLine)
        body.addWidget(line)
        buttons = QHBoxLayout()
        self.back = QPushButton('Back')
        self.back.clicked.connect(lambda: self.show_step(self.pages.currentIndex()-1))
        buttons.addWidget(self.back)
        self.step_label = QLabel()
        self.step_label.setAlignment(Qt.AlignCenter)
        buttons.addWidget(self.step_label,1)
        self.finish = QPushButton('Next')
        self.finish.setObjectName('assistantPrimary')
        self.finish.setDefault(True)
        self.finish.clicked.connect(self.next_step)
        buttons.addWidget(self.finish)
        body.addLayout(buttons)
        self.content_layout.addWidget(card, 1)
        bottom = QHBoxLayout()
        self.later = QPushButton('Close preview' if preview else 'Set up later')
        self.later.setObjectName('assistantLater')
        self.later.setFlat(True)
        self.later.clicked.connect(self.reject)
        bottom.addWidget(self.later)
        bottom.addStretch()
        layout.addLayout(bottom)

        welcome = self.add_page('Your next adventure\nstarts here.',
            'Welcome to EmuLuna. All your favorite worlds, in one place.')
        welcome.addWidget(self.paragraph('This assistant will help you choose a look, pick a home for your '
            'console files and get your emulator cores ready.'))
        welcome.addWidget(self.paragraph('You can revisit these choices anytime in Settings.'))
        welcome.addStretch()
        welcome.addWidget(self.paragraph('Click Next to get started.'))

        storage = self.add_page('A home for your games',
            'Choose a folder or drive for your console library.')
        self.destination = console_root(library.root).resolve().parent
        self.path = QLineEdit(str(self.destination / 'consoles'))
        self.path.setReadOnly(True)
        self.path.setCursorPosition(0)
        self.choose = QPushButton('Choose folder…')
        self.choose.clicked.connect(self.choose_folder)
        row = QHBoxLayout(); row.addWidget(self.path,1); row.addWidget(self.choose)
        storage.addLayout(row)
        self.copy_games = SettingsCheckBox('Copy imported games into the library')
        self.copy_games.setChecked(library.setting('copy_games','1') == '1')
        storage.addWidget(self.copy_games)
        storage.addWidget(self.paragraph('Each console keeps its ROMs, saves, save states and screenshots '
            'together. Original imports are kept.'))
        storage.addWidget(self.paragraph('The game list, app settings and emulator cores stay in app data.'))
        storage.addStretch()

        appearance = self.add_page('Make it yours', 'Choose the look of your library.')
        self.theme = SettingsComboBox()
        theme_registry = ThemeRegistry(library.root)
        for theme in theme_registry.themes.values():
            self.theme.addItem(theme.name, theme.id)
        self.theme.setCurrentIndex(max(0,self.theme.findData(theme_registry.get(library.setting('appearance.theme','luna')).id)))
        form = QFormLayout();form.addRow('Appearance',self.theme)
        appearance.addLayout(form)
        appearance.addWidget(self.paragraph('Luna brings EmuLuna’s signature accent. Daylight keeps things '
            'bright. Classic uses an OpenEmu-inspired charcoal look.'))
        appearance.addWidget(self.paragraph('Want your own colors? Create a custom theme in Settings.'))
        appearance.addStretch()

        cores = self.add_page('Emulator cores', 'The right emulator for each console.')
        self.auto_cores = SettingsCheckBox('Download recommended cores when I add games')
        self.auto_cores.setChecked(library.setting('core_auto_install','1') == '1')
        cores.addWidget(self.auto_cores)
        self.core_auto_help = self.paragraph('EmuLuna can automatically install a compatible core for each console '
            'you import. Downloads need an internet connection.')
        cores.addWidget(self.core_auto_help)
        self.console_picker = QWidget()
        picker_layout = QVBoxLayout(self.console_picker)
        picker_layout.setContentsMargins(0,0,0,0);picker_layout.setSpacing(8)
        picker_layout.addWidget(self.paragraph('Choose consoles to download now, or leave them unchecked to install cores later.'))
        scroll = QScrollArea();scroll.setWidgetResizable(True)
        scroll.setMinimumHeight(130)
        rows = QWidget();row_layout = QVBoxLayout(rows)
        row_layout.setContentsMargins(8,6,8,6);row_layout.setSpacing(2)
        self.console_checks = {}
        for key, system in SYSTEMS.items():
            row = QHBoxLayout()
            icon = QLabel();icon.setFixedSize(28,28)
            icon.setPixmap(QPixmap(str(system.icon)).scaled(24,24,Qt.KeepAspectRatio,Qt.SmoothTransformation))
            row.addWidget(icon)
            check = SettingsCheckBox(system.name)
            check.setEnabled(system.default_core in CATALOG)
            if not check.isEnabled(): check.setToolTip('Install a compatible core manually in Settings → Cores.')
            row.addWidget(check,1);row_layout.addLayout(row)
            self.console_checks[key] = check
        row_layout.addStretch();scroll.setWidget(rows)
        picker_layout.addWidget(scroll,1)
        cores.addWidget(self.console_picker,1)
        self.auto_cores.toggled.connect(self.update_core_picker)
        self.update_core_picker(self.auto_cores.isChecked())
        cores.addWidget(self.paragraph('You can also install or change cores in Settings → Cores. Some consoles '
            'need BIOS files; add your own through System Files.'))
        cores.addStretch()

        ready = self.add_page('You’re ready to play!', 'Add your games and start playing.')
        self.summary = QLabel()
        self.summary.setWordWrap(True)
        self.summary.setTextFormat(Qt.PlainText)
        ready.addWidget(self.summary)
        ready.addWidget(self.paragraph('Use the + button or drag your ROM files into the library to add games. '
            'Set up your controller in Settings → Controls.'))
        ready.addStretch()
        self.show_step(0)

    def update_core_picker(self, automatic):
        self.console_picker.setVisible(not automatic)
        self.core_auto_help.setVisible(automatic)

    def selected_consoles(self):
        return [key for key, check in self.console_checks.items() if check.isEnabled() and check.isChecked()]

    @staticmethod
    def paragraph(text):
        label = QLabel(text)
        label.setWordWrap(True)
        return label

    def add_page(self, title, subtitle):
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(0,0,0,0)
        layout.setSpacing(18)
        heading = QLabel(title)
        heading.setObjectName('assistantHeading')
        font = heading.font();font.setPointSize(26);font.setBold(True);heading.setFont(font)
        heading.setWordWrap(True)
        layout.addWidget(heading)
        layout.addWidget(self.paragraph(subtitle))
        self.pages.addWidget(page)
        return layout

    def show_step(self, index):
        if self.worker:
            return
        index = max(0,min(self.pages.count()-1,index))
        previous = self.pages.currentIndex()
        self.pages.setCurrentIndex(index)
        if previous != index and self.isVisible():
            self.page_fade.stop()
            self.page_fade.setStartValue(0.2)
            self.page_fade.setEndValue(1.0)
            self.page_fade.start()
        self.back.setEnabled(index>0)
        self.step_label.setText(f'{index+1} of {self.pages.count()}')
        self.finish.setText(('Finish preview' if self.preview else 'Let’s go!') if index==self.pages.count()-1 else 'Next')
        if index == self.pages.count()-1:
            self.summary.setText('Appearance: ' + self.theme.currentText() + '\n'
                'Console files: ' + str(self.destination/'consoles') + '\n'
                'Imported games: ' + ('Copy into library' if self.copy_games.isChecked() else 'Keep in original folders') + '\n'
                'Recommended cores: ' + ('Download as needed' if self.auto_cores.isChecked() else
                 f'Download for {len(self.selected_consoles())} selected consoles' if self.selected_consoles() else 'Install manually'))

    def next_step(self):
        if self.pages.currentIndex() < self.pages.count()-1:
            self.show_step(self.pages.currentIndex()+1)
        else:
            self.finish_setup()

    def toggle_fullscreen(self):
        if self.isFullScreen():
            self.showNormal()
            self.fullscreen.setText('Fullscreen')
        else:
            self.showFullScreen()
            self.fullscreen.setText('Exit fullscreen')

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape and self.isFullScreen():
            self.toggle_fullscreen()
            event.accept()
            return
        super().keyPressEvent(event)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        if not hasattr(self, 'content_layout'):
            return
        wide = self.width() >= 900
        self.content_layout.setDirection(QBoxLayout.LeftToRight if wide else QBoxLayout.TopToBottom)
        self.content.setFixedWidth(min(1440, self.width() - 72))
        self.welcome_logo.setMaximumHeight(16777215 if wide else max(180, min(270, self.height() // 3)))
        self.content_layout.setSpacing(40 if wide else 12)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        width, height = self.width(), self.height()
        time = self.welcome_logo.frame_time if hasattr(self, 'welcome_logo') else 0
        gradient = QLinearGradient(0, 0, 0, height)
        gradient.setColorAt(0, QColor('#030615'))
        gradient.setColorAt(.5, QColor('#100d2b'))
        gradient.setColorAt(1, QColor('#172344'))
        painter.fillRect(self.rect(), gradient)
        # Broad, soft light pools create depth without competing with the logo.
        for x, y, radius, color in ((.08, .65, .55, '#543bdf'),
                (.87, .95, .65, '#b32bb8'), (.45, 1.1, .7, '#147bcc')):
            glow = QRadialGradient(width*x, height*y, width*radius)
            tint = QColor(color); tint.setAlpha(65)
            clear = QColor(color); clear.setAlpha(0)
            glow.setColorAt(0, tint); glow.setColorAt(1, clear)
            painter.fillRect(self.rect(), glow)
        # A sparse sky and drifting, feathered clouds stay behind the content.
        painter.setPen(Qt.NoPen)
        for index in range(78):
            x = ((index * .61803398875) % 1) * width
            y = ((index * .41421356237) % 1) * height * .86
            alpha = .18 + .25 * (1 + math.sin(time*.7 + index)) / 2
            color = QColor('#ccdcff'); color.setAlphaF(alpha)
            painter.setBrush(color)
            radius = .7 if index % 4 else 1.2
            painter.drawEllipse(QPointF(x, y), radius, radius)
        # A fresh path for each pass, starting after three seconds. Low-discrepancy
        # positions avoid a conspicuous repeating pattern without random repaint jitter.
        if self.welcome_logo.motion and time >= 3:
            cycle = int((time - 3) / 7.0)
            elapsed = (time - 3) % 7.0
            if elapsed < 2.2:
                fraction = elapsed / 2.2
                direction = 1 if cycle % 2 == 0 else -1
                start_x = -width * .12 if direction == 1 else width * 1.12
                start_y = (.025 + .15 * ((cycle * .41421356237) % 1)) * height
                travel_x = direction * width * 1.24
                travel_y = height * (.035 + .09 * ((cycle * .7320508) % 1))
                head = QPointF(start_x + travel_x * fraction, start_y + travel_y * fraction)
                tail = QPointF(head.x() - travel_x * .09, head.y() - travel_y * .09)
                bright = QColor('#e6f4ff'); bright.setAlphaF(math.sin(math.pi * fraction))
                clear = QColor(bright); clear.setAlpha(0)
                trail = QLinearGradient(tail, head)
                trail.setColorAt(0, clear); trail.setColorAt(1, bright)
                painter.setPen(QPen(trail, 2.0, Qt.SolidLine, Qt.RoundCap))
                painter.drawLine(tail, head)
                painter.setPen(Qt.NoPen); painter.setBrush(bright)
                painter.drawEllipse(head, 2, 2)
        # Three procedural cloud banks: rounded contour harmonics and
        # different drift speeds give a gentle parallax effect. The moving
        # coordinates remain continuous, so no cloud pops at a tile boundary.
        for layer, (speed, base, color) in enumerate(((18, .91, '#8e89db'),
                (-25, .95, '#bd8eda'), (32, .99, '#75a5dc'))):
            baseline = height * base
            amplitude = min(42, height * .055)
            def contour(x):
                drift = x - time * speed
                return baseline + amplitude * (
                    .55 * math.sin(drift / 105 + layer * 2.1) +
                    .30 * math.sin(drift / 48 + layer) +
                    .15 * math.sin(drift / 26 + layer * .7))
            path = QPainterPath()
            step = 16
            start = -32
            path.moveTo(start, contour(start))
            for x in range(start, width + 32, step):
                end = x + step
                slope_start = (contour(x + 1) - contour(x - 1)) / 2
                slope_end = (contour(end + 1) - contour(end - 1)) / 2
                path.cubicTo(x + step/3, contour(x) + slope_start * step/3,
                             end - step/3, contour(end) - slope_end * step/3,
                             end, contour(end))
            path.lineTo(width + 48, height + 60)
            path.lineTo(start, height + 60); path.closeSubpath()
            fill = QLinearGradient(0, baseline-amplitude, 0, height + 35)
            rim = QColor(color); rim.setAlpha(38)
            body = QColor(color); body.setAlpha(62)
            fill.setColorAt(0, rim); fill.setColorAt(1, body)
            painter.setPen(Qt.NoPen);painter.setBrush(fill)
            painter.drawPath(path)
        # Feathered mist blends the banks into the colored horizon.
        for index in range(13):
            span = width * 1.6
            speed = 18.0 if index % 2 else -25.0
            x = ((index / 13 * span + time * speed) % span) - width * .3
            y = height*(.99 + .035*math.sin(index*2.1)) + math.sin(time*.12 + index)*7
            radius = width*(.13 + .025*math.sin(index))
            cloud = QRadialGradient(x, y, radius)
            tint = QColor('#8299ef' if index % 2 else '#bf88dc'); tint.setAlpha(22)
            clear = QColor(tint); clear.setAlpha(0)
            cloud.setColorAt(0, tint); cloud.setColorAt(.4, tint); cloud.setColorAt(1, clear)
            painter.fillRect(self.rect(), cloud)

    def hideEvent(self, event):
        self.page_fade.stop()
        self.page_opacity.setOpacity(1.0)
        self.welcome_logo.timer.stop()
        super().hideEvent(event)

    def choose_folder(self):
        folder = QFileDialog.getExistingDirectory(self, 'Choose where to store the consoles folder', str(self.destination))
        if folder:
            self.destination = Path(folder).resolve()
            self.path.setText(str(self.destination / 'consoles'))
            self.path.setCursorPosition(0)

    def finish_setup(self):
        if self.preview:
            self.accept()
            return
        if self.worker:
            return
        if self.destination / 'consoles' == console_root(self.library.root).resolve():
            self.complete_setup()
            return
        # Usually first-run libraries are empty; protect populated pending setups too.
        if self.library.games():
            from .library_audit import LibraryAuditDialog
            audit = LibraryAuditDialog(self.library, self, migration=True, operation='move')
            accepted = audit.exec() == QDialog.Accepted
            audit.deleteLater()
            if not accepted:
                return
            if QMessageBox.question(self, 'Move console files?',
                    'Move your console files to:\n' + str(self.destination / 'consoles') +
                    '\n\nCopies are verified before switching locations. Original imports stay in place.',
                    QMessageBox.Yes | QMessageBox.No, QMessageBox.No) != QMessageBox.Yes:
                return
        from .library_move import LibraryMoveWorker
        self.worker = LibraryMoveWorker(self.library.root, self.destination, self)
        self.worker.progress.connect(self.status.setText)
        self.worker.result.connect(self.status.setText)
        self.worker.finished.connect(self.move_finished)
        for widget in (self.theme,self.copy_games,self.auto_cores,self.choose,self.finish,self.back,self.later,self.console_picker):
            widget.setEnabled(False)
        self.status.show()
        self.status.setText('Preparing your console folder…')
        self.worker.start()

    def move_finished(self):
        succeeded = self.worker.succeeded
        self.worker.deleteLater()
        self.worker = None
        for widget in (self.theme,self.copy_games,self.auto_cores,self.choose,self.finish,self.back,self.later,self.console_picker):
            widget.setEnabled(True)
        if succeeded:
            self.complete_setup()

    def complete_setup(self):
        selected = self.selected_consoles() if not self.auto_cores.isChecked() else []
        if selected:
            from .core_manager import CoreManager, CoreWorker
            try:
                installed = CoreManager(self.library.root).installed()
                ids = list(dict.fromkeys(SYSTEMS[key].default_core for key in selected
                                         if SYSTEMS[key].default_core not in installed))
            except Exception as error:
                self.status.setText(str(error));self.status.show()
                return
            if ids:
                self.worker = CoreWorker(self.library.root, core_ids=ids)
                self.core_download_ok = False
                self.worker.progress.connect(self.status.setText)
                self.worker.result.connect(self.core_download_result)
                self.worker.finished.connect(self.core_download_finished)
                for widget in (self.theme,self.copy_games,self.auto_cores,self.choose,
                               self.finish,self.back,self.later,self.console_picker):
                    widget.setEnabled(False)
                self.status.setText('Downloading your selected console cores…');self.status.show()
                self.worker.start()
                return
        self.commit_setup()

    def core_download_result(self, message, succeeded):
        self.core_download_ok = succeeded
        self.status.setText(message)

    def core_download_finished(self):
        self.worker.deleteLater();self.worker = None
        for widget in (self.theme,self.copy_games,self.auto_cores,self.choose,
                       self.finish,self.back,self.later,self.console_picker):
            widget.setEnabled(True)
        if self.core_download_ok:
            self.commit_setup()
        else:
            self.status.setText(self.status.text() + '\nTry again, or go Back to change your selections.')

    def commit_setup(self):
        select_theme(self.library,self.theme.currentData())
        self.library.set_setting('copy_games', '1' if self.copy_games.isChecked() else '0')
        self.library.set_setting('core_auto_install', '1' if self.auto_cores.isChecked() else '0')
        self.library.set_setting('onboarding.complete','1')
        self.accept()

    def done(self, result):
        if not self.worker:
            super().done(result)

    def closeEvent(self, event):
        if self.worker:
            event.ignore()
        else:
            super().closeEvent(event)

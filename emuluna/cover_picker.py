"""Visual, reviewable box-art search using the Libretro thumbnail catalog."""
from contextlib import closing
import hashlib
from pathlib import Path
import re
import sqlite3

from PySide6.QtCore import QRect, QSize, Qt, QThread, Signal, QTimer
from PySide6.QtGui import QIcon, QImage, QPainter, QPixmap
from PySide6.QtWidgets import (QDialog, QDialogButtonBox, QHBoxLayout, QLabel,
    QLineEdit, QListWidget, QListWidgetItem, QPushButton, QStyledItemDelegate,
    QStyle, QVBoxLayout)

from .artwork import (BackupArt, Cancelled, Catalog, Downloads, MIB,
                      closest_cover_names, image_png, preferred_region,
                      thumbnail_url)
from .library import SYSTEMS
from .metadata import metadata_for_cover, title_catalog_rows


def explicit_cover_region(title):
    """Read an explicit regional variant from a thumbnail filename."""
    tags = [part.strip() for group in re.findall(r"\(([^)]*)\)", title)
            for part in group.split(",")]
    aliases = {"USA": "USA", "US": "USA", "Japan": "Japan", "JP": "Japan",
               "Europe": "Europe", "EU": "Europe"}
    return next((aliases[tag] for tag in tags if tag in aliases), None)


def cover_region(title):
    return explicit_cover_region(title) or preferred_region()


def cover_label(candidate):
    """Keep dump/revision tags in the tooltip instead of stretching the card."""
    title = re.split(r"\s*[\[(]", candidate["title"], maxsplit=1)[0].strip()
    metadata = candidate.get("metadata", {})
    region = metadata.get("region") or explicit_cover_region(candidate["title"])
    variant = " · ".join(value for value in
        (region, metadata.get("release_date")) if value)
    return title, variant


class CoverResultDelegate(QStyledItemDelegate):
    """Fixed-size cover cards that cannot grow with long catalog filenames."""
    card_size = QSize(178, 226)

    def sizeHint(self, option, index):
        return self.card_size

    def paint(self, painter, option, index):
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing)
        card = option.rect.adjusted(4, 3, -4, -3)
        if option.state & (QStyle.State_Selected | QStyle.State_MouseOver):
            color = option.palette.highlight().color()
            color.setAlpha(72 if option.state & QStyle.State_Selected else 32)
            painter.setPen(option.palette.highlight().color()
                           if option.state & QStyle.State_Selected else Qt.NoPen)
            painter.setBrush(color)
            painter.drawRoundedRect(card, 6, 6)

        art_rect = QRect(card.left() + 7, card.top() + 7, card.width() - 14, 170)
        icon = index.data(Qt.DecorationRole)
        pixmap = icon.pixmap(art_rect.size()) if isinstance(icon, QIcon) else QPixmap()
        if not pixmap.isNull():
            size = pixmap.size().scaled(art_rect.size(), Qt.KeepAspectRatio)
            target = QRect(art_rect.center().x() - size.width() // 2,
                           art_rect.center().y() - size.height() // 2,
                           size.width(), size.height())
            painter.drawPixmap(target, pixmap)

        title, variant = cover_label(index.data(Qt.UserRole))
        text_rect = QRect(card.left() + 5, art_rect.bottom() + 6,
                          card.width() - 10, card.bottom() - art_rect.bottom() - 10)
        painter.setPen(option.palette.highlightedText().color()
                       if option.state & QStyle.State_Selected
                       else option.palette.text().color())
        metrics = option.fontMetrics
        title = metrics.elidedText(title, Qt.ElideRight, text_rect.width())
        painter.drawText(QRect(text_rect.left(), text_rect.top(), text_rect.width(), metrics.height()),
                         Qt.AlignHCenter | Qt.AlignVCenter, title)
        if variant:
            painter.setPen(option.palette.placeholderText().color()
                           if not option.state & QStyle.State_Selected
                           else option.palette.highlightedText().color())
            variant = metrics.elidedText(variant, Qt.ElideRight, text_rect.width())
            painter.drawText(QRect(text_rect.left(), text_rect.top() + metrics.height(),
                                   text_rect.width(), metrics.height()),
                             Qt.AlignHCenter | Qt.AlignVCenter, variant)
        painter.restore()


class CoverSearchWorker(QThread):
    candidate = Signal(object)
    stage = Signal(str)
    result = Signal(dict)

    def __init__(self, root, system, query, parent=None):
        super().__init__(parent)
        self.root = root
        self.system = system
        self.query = query

    def run(self):
        summary = {"found": 0, "failed": 0, "metadata": 0,
                   "metadata_error": "", "error": "", "cancelled": False}
        try:
            downloads = Downloads(self.isInterruptionRequested)
            self.stage.emit("Reading the cover catalog…")
            names = BackupArt(self.root, downloads).index(self.system)
            ranked = closest_cover_names(names, self.query, limit=30, minimum=0.25)
            metadata_rows = []
            try:
                self.stage.emit("Checking regional game information…")
                path = Catalog(self.root, downloads).ensure(self.stage.emit)
                with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)) as db:
                    db.row_factory = sqlite3.Row
                    db.execute("PRAGMA trusted_schema=OFF")
                    metadata_rows = title_catalog_rows(db, self.system)
            except Cancelled:
                raise
            except Exception as error:
                summary["metadata_error"] = str(error)
            self.stage.emit("Loading cover previews…")
            seen_images = set()
            for name, score in ranked:
                if summary["found"] >= 18:
                    break
                downloads.check()
                url = thumbnail_url(self.system, name)
                try:
                    image = image_png(downloads.get(url, 12 * MIB))
                except Cancelled:
                    raise
                except Exception:
                    summary["failed"] += 1
                    continue
                digest = hashlib.sha256(image).digest()
                if digest in seen_images:
                    continue
                seen_images.add(digest)
                title = Path(name).stem
                match = metadata_for_cover(
                    metadata_rows, title, cover_region(title)) if metadata_rows else None
                candidate = {"title": title, "url": url, "image": image,
                             "score": score, "metadata": match.fields if match else {},
                             "metadata_source": match.source_url if match else ""}
                if match:
                    summary["metadata"] += 1
                self.candidate.emit(candidate)
                summary["found"] += 1
        except Cancelled:
            summary["cancelled"] = True
        except Exception as error:
            summary["error"] = str(error)
        self.result.emit(summary)


class CoverPickerDialog(QDialog):
    """Plex-style cover chooser with a title search and visual result grid."""

    def __init__(self, root, system, title, parent=None):
        super().__init__(parent)
        self.root = root
        self.system = system
        self.worker = None
        self.selection = None
        self.setWindowTitle("Find cover art")
        self.resize(830, 650)
        self.setMinimumSize(620, 480)

        layout = QVBoxLayout(self)
        heading = QLabel(f"Find a cover for {title}")
        heading.setStyleSheet("font-size:18px;font-weight:600")
        layout.addWidget(heading)
        explanation = QLabel(
            f"Search {SYSTEMS[system].name} artwork. Results are ranked by title; "
            "choose the cover you want to use.")
        explanation.setWordWrap(True)
        explanation.setStyleSheet("color:palette(placeholder-text)")
        layout.addWidget(explanation)

        search_row = QHBoxLayout()
        self.query = QLineEdit(title)
        self.query.setClearButtonEnabled(True)
        self.query.setPlaceholderText("Game title")
        self.query.returnPressed.connect(self.start_search)
        search_row.addWidget(self.query, 1)
        self.search_button = QPushButton("Search")
        self.search_button.clicked.connect(self.start_search)
        search_row.addWidget(self.search_button)
        layout.addLayout(search_row)

        self.results = QListWidget()
        self.results.setViewMode(QListWidget.IconMode)
        self.results.setResizeMode(QListWidget.Adjust)
        self.results.setMovement(QListWidget.Static)
        self.results.setWrapping(True)
        self.results.setUniformItemSizes(True)
        self.results.setIconSize(QSize(156, 170))
        self.results.setGridSize(CoverResultDelegate.card_size)
        self.results.setSpacing(2)
        self.results.setWordWrap(False)
        self.results.setItemDelegate(CoverResultDelegate(self.results))
        self.results.itemSelectionChanged.connect(self.update_use_button)
        self.results.itemDoubleClicked.connect(lambda _: self.use_selected())
        layout.addWidget(self.results, 1)

        self.status = QLabel()
        self.status.setWordWrap(True)
        self.status.setStyleSheet("color:palette(placeholder-text)")
        layout.addWidget(self.status)
        self.buttons = QDialogButtonBox(QDialogButtonBox.Cancel)
        self.use_button = self.buttons.addButton("Use selected cover", QDialogButtonBox.AcceptRole)
        self.use_button.setObjectName("primary")
        self.use_button.setEnabled(False)
        self.buttons.rejected.connect(self.reject)
        self.use_button.clicked.connect(self.use_selected)
        layout.addWidget(self.buttons)
        QTimer.singleShot(0, self.start_search)

    def start_search(self):
        if self.worker and self.worker.isRunning():
            return
        query = self.query.text().strip()
        if not query:
            self.query.setFocus()
            return
        self.results.clear()
        self.selection = None
        self.use_button.setEnabled(False)
        self.search_button.setEnabled(False)
        self.query.setEnabled(False)
        self.status.setText("Finding likely covers…")
        self.worker = CoverSearchWorker(self.root, self.system, query, self)
        self.worker.candidate.connect(self.add_candidate)
        self.worker.stage.connect(self.status.setText)
        self.worker.result.connect(self.search_finished)
        self.worker.finished.connect(self.worker.deleteLater)
        self.worker.start()

    def add_candidate(self, candidate):
        image = QImage.fromData(candidate["image"])
        if image.isNull():
            return
        pixmap = QPixmap.fromImage(image)
        metadata = candidate.get("metadata", {})
        title, variant = cover_label(candidate)
        item = QListWidgetItem(QIcon(pixmap), title + (("\n" + variant) if variant else ""))
        item.setData(Qt.UserRole, candidate)
        item.setSizeHint(CoverResultDelegate.card_size)
        details = [candidate["title"], f"Match: {round(candidate['score'] * 100)}%"]
        if metadata:
            details.append("Game information: " + " · ".join(
                value for value in (metadata.get("region"), metadata.get("publisher"),
                                    metadata.get("release_date")) if value))
        item.setToolTip("\n".join(details))
        self.results.addItem(item)

    def search_finished(self, summary):
        self.worker = None
        self.search_button.setEnabled(True)
        self.query.setEnabled(True)
        if summary["cancelled"]:
            self.status.setText("Artwork search cancelled.")
        elif summary["error"]:
            self.status.setText("Artwork search is temporarily unavailable: " + summary["error"])
        elif not summary["found"]:
            self.status.setText("No likely covers were found. Try a shorter or alternate title.")
        else:
            detail = f"Showing {summary['found']} likely cover{'s' if summary['found'] != 1 else ''}."
            if summary["failed"]:
                detail += f" {summary['failed']} preview{'s' if summary['failed'] != 1 else ''} could not load."
            if summary["metadata"]:
                detail += f" Game information is available for {summary['metadata']}."
            elif summary["metadata_error"]:
                detail += " Covers can still be selected, but game information is temporarily unavailable."
            self.status.setText(detail)
            self.results.setCurrentRow(0)
        self.update_use_button()

    def update_use_button(self):
        self.use_button.setEnabled(self.worker is None and bool(self.results.selectedItems()))

    def use_selected(self):
        items = self.results.selectedItems()
        if not items or self.worker is not None:
            return
        self.selection = items[0].data(Qt.UserRole)
        self.accept()

    def reject(self):
        if self.worker and self.worker.isRunning():
            self.worker.requestInterruption()
            self.status.setText("Stopping artwork search…")
            self.search_button.setEnabled(False)
            self.buttons.button(QDialogButtonBox.Cancel).setEnabled(False)
            self.worker.finished.connect(super().reject)
            return
        super().reject()

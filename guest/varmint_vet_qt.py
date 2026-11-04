"""Qt interface for Vet."""

from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import threading

from PyQt6.QtCore import QObject, Qt, QTimer, pyqtSignal
from PyQt6.QtGui import QGuiApplication, QIcon, QTextBlockFormat, QTextCursor
from PyQt6.QtNetwork import QLocalServer, QLocalSocket
from PyQt6.QtWidgets import (
    QApplication,
    QButtonGroup,
    QCheckBox,
    QComboBox,
    QDialog,
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QRadioButton,
    QScrollArea,
    QSizePolicy,
    QStackedWidget,
    QTextBrowser,
    QVBoxLayout,
    QWidget,
)

from varmint_vet import (
    Preferences,
    VetError,
    VetService,
    current_host_backend_warning,
    current_setup_warning,
    diagnostics_wrapper_required,
    display_graphics,
    display_proton,
    format_launch_requirements,
    is_copyable_guide_code,
    is_setup_profile_block,
    split_markdown_sections,
    transform_launch_options,
)
from varmint_vet_actions import (
    configure_manually,
    copy_to_clipboard,
    override_plan,
    switch_backend_plan,
)


STYLE = """
QWidget { color: #f1f3f5; }
QDialog { background: #232543; color: #f1f3f5; font-size: 14px; }
QLabel#appTitle { font-size: 24px; font-weight: 700; }
QLabel#gameTitle { font-size: 22px; font-weight: 700; }
QLabel#muted { color: #a3a6bc; }
QLabel#section { color: #b3b7d2; font-size: 12px; font-weight: 700; }
QLabel#warning { color: #e2bd6f; font-weight: 600; }
QLabel#requirements {
    background: #171a30; border: 1px solid #40445e; border-radius: 8px;
    color: #f1f3f5; font-family: monospace; padding: 7px 9px;
}
QLabel#requirementLabel { color: #a3a6bc; font-size: 12px; font-weight: 600; }
QLabel#warningBanner {
    background: #393342; border: 1px solid #806b42; border-radius: 9px;
    color: #e2bd6f; font-weight: 600; padding: 10px 12px;
}
QFrame#card { background: #2d3254; border: 1px solid #40445e; border-radius: 12px; }
QFrame#panel { background: #1c203a; border: 1px solid #40445e; border-radius: 12px; }
QLineEdit, QComboBox, QListWidget {
    background: #1c203a; border: 1px solid #40445e; border-radius: 9px;
    color: #f1f3f5; padding: 8px;
}
QListWidget { outline: none; padding: 5px; }
QListWidget::item { border-radius: 8px; padding: 10px 8px; }
QListWidget::item:selected { background: #999db8; color: #171925; }
QListWidget::item:hover:!selected { background: #2d3254; }
QPushButton {
    background: #2d3254; border: 1px solid #40445e; border-radius: 9px;
    color: #f1f3f5; padding: 10px 14px; font-weight: 600;
}
QPushButton:hover { background: #40445e; }
QPushButton:pressed { background: #1c203a; }
QPushButton#primary { background: #999db8; border-color: #999db8; color: #171925; }
QPushButton#primary:hover { background: #b3b7d2; }
QPushButton:checked { background: #999db8; border-color: #b3b7d2; color: #171925; }
QPushButton:disabled { color: #7c809a; background: #252941; border-color: #343850; }
QRadioButton { color: #f1f3f5; font-weight: 600; spacing: 8px; padding: 4px; }
QRadioButton::indicator { width: 18px; height: 18px; }
QCheckBox { color: #b3b7d2; spacing: 8px; padding: 4px; }
QCheckBox::indicator { width: 18px; height: 18px; }
QTextBrowser {
    background: #1c203a; border: 1px solid #40445e; border-radius: 10px;
    color: #f1f3f5; padding: 12px;
}
QTextBrowser#guideProse { background: transparent; border: none; padding: 0; font-size: 13px; }
QScrollArea#guideScroll { background: #1c203a; border: 1px solid #40445e; border-radius: 10px; }
QScrollArea#guideScroll > QWidget > QWidget { background: #1c203a; }
QFrame#codeBlock { background: #171a30; border: 1px solid #40445e; border-radius: 9px; }
QLabel#codeText {
    background: #171a30; border: none; color: #f1f3f5; padding: 6px;
    font-family: monospace;
}
QPushButton#copyCode { padding: 5px 12px; }
"""


def color_scheme_path():
    for path in (
        Path.home() / ".local/share/color-schemes/VarmintVet.colors",
        Path("/usr/local/share/color-schemes/VarmintVet.colors"),
    ):
        if path.is_file():
            return str(path)
    return None


def icon_path():
    for path in (
        Path.home() / ".local/share/icons/hicolor/512x512/apps/varmint-vet.png",
        Path("/usr/local/share/icons/hicolor/512x512/apps/varmint-vet.png"),
    ):
        if path.is_file():
            return str(path)
    return None


def capture_age(value):
    try:
        captured = datetime.fromisoformat(value)
        seconds = max(
            0,
            int(
                (
                    datetime.now(timezone.utc) - captured.astimezone(timezone.utc)
                ).total_seconds()
            ),
        )
    except (TypeError, ValueError):
        return "Captured previously"
    if seconds < 90:
        return "Captured just now"
    if seconds < 90 * 60:
        return f"Captured {seconds // 60} minutes ago"
    if seconds < 36 * 60 * 60:
        return f"Captured {seconds // 3600} hours ago"
    return f"Captured {seconds // 86400} days ago"


class AsyncSignals(QObject):
    prepared = pyqtSignal(int, object)
    applied = pyqtSignal(int, object)
    failed = pyqtSignal(int, str)


class ScrollTextBrowser(QTextBrowser):
    def wheelEvent(self, event):
        if not event.modifiers() & Qt.KeyboardModifier.ControlModifier:
            return super().wheelEvent(event)

        delta = event.pixelDelta()
        if delta.isNull():
            delta = event.angleDelta()
            horizontal = abs(delta.x()) > abs(delta.y())
            amount = delta.x() if horizontal else delta.y()
            bar = self.horizontalScrollBar() if horizontal else self.verticalScrollBar()
            amount = amount / 120 * bar.singleStep() * 3
        else:
            horizontal = abs(delta.x()) > abs(delta.y())
            amount = delta.x() if horizontal else delta.y()
            bar = self.horizontalScrollBar() if horizontal else self.verticalScrollBar()
        bar.setValue(bar.value() - round(amount))
        event.accept()


class DetailsDialog(QDialog):
    def __init__(self, title, text, parent=None):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setWindowFlag(Qt.WindowType.Dialog, True)
        self.setFixedSize(760, 500)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        browser = ScrollTextBrowser()
        browser.setPlainText(text)
        layout.addWidget(browser)
        close = QPushButton("Close")
        close.clicked.connect(self.accept)
        layout.addWidget(close, alignment=Qt.AlignmentFlag.AlignRight)


class GuideProse(QTextBrowser):
    def __init__(self, markdown, parent=None):
        super().__init__(parent)
        self.setObjectName("guideProse")
        self.setOpenLinks(False)
        self.setOpenExternalLinks(False)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self.document().setDefaultStyleSheet(
            "body { color: #f1f3f5; font-size: 13px; line-height: 135%; }"
            "h1 { font-size: 22px; }"
            "h2 { font-size: 17px; }"
            "h3 { font-size: 14px; }"
            "ul, ol { margin-left: 20px; }"
        )
        self.setMarkdown(markdown)
        self._space_blocks()
        self.document().documentLayout().documentSizeChanged.connect(self._fit_height)
        QTimer.singleShot(0, self._fit_height)

    def _space_blocks(self):
        block = self.document().begin()
        while block.isValid():
            formatting = block.blockFormat()
            heading = formatting.headingLevel()
            formatting.setLineHeight(
                135, QTextBlockFormat.LineHeightTypes.ProportionalHeight.value
            )
            if heading == 1:
                formatting.setTopMargin(0)
                formatting.setBottomMargin(28)
            elif heading == 2:
                formatting.setTopMargin(30)
                formatting.setBottomMargin(18)
            elif heading == 3:
                formatting.setTopMargin(24)
                formatting.setBottomMargin(14)
            else:
                formatting.setBottomMargin(22)
            cursor = QTextCursor(block)
            cursor.setBlockFormat(formatting)
            block = block.next()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.document().setTextWidth(max(1, self.viewport().width()))
        self._fit_height()

    def _fit_height(self, *_):
        height = math.ceil(self.document().size().height()) + 4
        if self.height() != height:
            self.setFixedHeight(height)


class GuideCodeBlock(QFrame):
    def __init__(self, code, copyable, parent=None):
        super().__init__(parent)
        self.code = code
        self.setObjectName("codeBlock")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 8, 10, 10)
        layout.setSpacing(4)
        self.copy_button = None
        if copyable:
            header = QHBoxLayout()
            header.addStretch()
            self.copy_button = QPushButton("Copy")
            self.copy_button.setObjectName("copyCode")
            self.copy_button.clicked.connect(self._copy)
            header.addWidget(self.copy_button)
            layout.addLayout(header)
        text = QLabel(code)
        text.setObjectName("codeText")
        text.setWordWrap(True)
        text.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        layout.addWidget(text)

    def _copy(self):
        try:
            copy_to_clipboard(self.code)
            self.copy_button.setText("Copied")
        except VetError:
            self.copy_button.setText("Copy failed")
        QTimer.singleShot(2000, lambda: self.copy_button.setText("Copy"))


class GuideDialog(QDialog):
    def __init__(self, markdown, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Setup guide")
        self.setWindowFlag(Qt.WindowType.Dialog, True)
        self.setFixedSize(860, 640)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        scroll = QScrollArea()
        scroll.setObjectName("guideScroll")
        scroll.setWidgetResizable(True)
        content = QWidget()
        content_layout = QVBoxLayout(content)
        content_layout.setContentsMargins(14, 14, 14, 14)
        content_layout.setSpacing(12)
        for kind, language, text in split_markdown_sections(markdown):
            if kind == "code" and is_setup_profile_block(text):
                continue
            widget = GuideCodeBlock(
                text, is_copyable_guide_code(language, text)
            ) if kind == "code" else GuideProse(text)
            content_layout.addWidget(widget)
        content_layout.addStretch()
        scroll.setWidget(content)
        layout.addWidget(scroll, 1)
        close = QPushButton("Close")
        close.clicked.connect(self.accept)
        layout.addWidget(close, alignment=Qt.AlignmentFlag.AlignRight)


class ReportDialog(QDialog):
    def __init__(self, report, parent=None):
        super().__init__(parent)
        self.report = report
        self.setWindowTitle("Compatibility report")
        self.setWindowFlag(Qt.WindowType.Dialog, True)
        self.setFixedSize(820, 600)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        heading = QLabel("Report ready")
        heading.setStyleSheet("font-size: 18px; font-weight: 700;")
        note = QLabel(
            "Review it, add what you saw, then paste it into a Varmint GitHub issue."
        )
        note.setObjectName("muted")
        browser = ScrollTextBrowser()
        browser.setPlainText(report)
        self.status = QLabel("")
        self.status.setObjectName("muted")
        buttons = QHBoxLayout()
        copy = QPushButton("Copy report")
        copy.setObjectName("primary")
        copy.clicked.connect(self._copy)
        close = QPushButton("Close")
        close.clicked.connect(self.accept)
        buttons.addWidget(copy)
        buttons.addStretch()
        buttons.addWidget(close)
        layout.addWidget(heading)
        layout.addWidget(note)
        layout.addWidget(browser, 1)
        layout.addWidget(self.status)
        layout.addLayout(buttons)

    def _copy(self):
        try:
            copy_to_clipboard(self.report)
            self.status.setText("Copied to the clipboard.")
            self.status.setStyleSheet("")
        except VetError as exception:
            self.status.setText(str(exception))
            self.status.setStyleSheet("color: #ff9b94;")


class VetWindow(QDialog):
    def __init__(self, service, selected_appid=None, diagnostic_run_id=None):
        super().__init__()
        self.service = service
        self.preferences = Preferences()
        self.games = service.steam.installed_games()
        self.state = None
        self.prepared = None
        self.confirm_return_panel = None
        self.configuration_revision = None
        self.diagnostic_snapshot = None
        self.initial_diagnostic_run_id = diagnostic_run_id
        self.request = 0
        self.signals = AsyncSignals()
        self.signals.prepared.connect(self._prepared)
        self.signals.applied.connect(self._applied)
        self.signals.failed.connect(self._failed)

        flags = (
            Qt.WindowType.Window
            | Qt.WindowType.WindowTitleHint
            | Qt.WindowType.WindowCloseButtonHint
        )
        self.setWindowFlags(flags)
        self.setWindowTitle("Vet")
        self.setFixedSize(980, 780)
        self.setStyleSheet(STYLE)
        self._build()
        self._populate_games(selected_appid)
        self.refresh_timer = QTimer(self)
        self.refresh_timer.setInterval(1000)
        self.refresh_timer.timeout.connect(self._refresh_from_steam)
        self.refresh_timer.start()
        self._center()
        if diagnostic_run_id:
            QTimer.singleShot(
                0, lambda: self._show_diagnostic_report(diagnostic_run_id)
            )

    def _build(self):
        root = QHBoxLayout(self)
        root.setContentsMargins(22, 22, 22, 22)
        root.setSpacing(22)

        sidebar = QVBoxLayout()
        title = QLabel("Vet")
        title.setObjectName("appTitle")
        subtitle = QLabel("Game setup assistant")
        subtitle.setObjectName("muted")
        self.search = QLineEdit()
        self.search.setPlaceholderText("Search installed games")
        self.search.textChanged.connect(self._filter_games)
        self.game_list = QListWidget()
        self.game_list.currentItemChanged.connect(self._game_selected)
        self.suggestion_notifications = QCheckBox("Automatic setup suggestions")
        self.suggestion_notifications.setToolTip(
            "Notify when Vet finds a setup for a newly installed game."
        )
        self.suggestion_notifications.setChecked(self.preferences.suggestions_enabled())
        self.suggestion_notifications.toggled.connect(
            self._set_suggestion_notifications
        )
        sidebar.addWidget(title)
        sidebar.addWidget(subtitle)
        sidebar.addSpacing(12)
        sidebar.addWidget(self.search)
        sidebar.addWidget(self.game_list, 1)
        sidebar.addWidget(self.suggestion_notifications)
        sidebar_widget = QWidget()
        sidebar_widget.setLayout(sidebar)
        sidebar_widget.setFixedWidth(350)
        root.addWidget(sidebar_widget)

        content = QVBoxLayout()
        content.setSpacing(12)
        self.game_title = QLabel("Select a game")
        self.game_title.setObjectName("gameTitle")
        self.game_subtitle = QLabel(
            "Vet reads the current configuration directly from Steam."
        )
        self.game_subtitle.setObjectName("muted")
        content.addWidget(self.game_title)
        content.addWidget(self.game_subtitle)

        config = QFrame()
        config.setObjectName("card")
        grid = QGridLayout(config)
        grid.setContentsMargins(18, 16, 18, 16)
        grid.setHorizontalSpacing(24)
        grid.setVerticalSpacing(10)
        self.value_names = {}
        self.values = {}
        for row, key in enumerate(
            (
                "Graphics",
                "Host backend",
                "Proton override",
                "Effective Proton",
                "Launch Options",
            )
        ):
            name = QLabel(key.upper())
            name.setObjectName("section")
            value = QLabel("—")
            value.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
            value.setWordWrap(True)
            grid.addWidget(name, row, 0)
            grid.addWidget(value, row, 1)
            self.value_names[key] = name
            self.values[key] = value
        self.value_names["Host backend"].hide()
        self.values["Host backend"].hide()
        grid.setColumnStretch(1, 1)
        content.addWidget(config)

        self.runtime_warning = QLabel("")
        self.runtime_warning.setObjectName("warningBanner")
        self.runtime_warning.setWordWrap(True)
        self.runtime_warning.hide()
        content.addWidget(self.runtime_warning)

        actions = QGridLayout()
        actions.setSpacing(10)
        self.recommend_button = QPushButton("Recommended setup")
        self.recommend_button.setObjectName("primary")
        self.recommend_button.clicked.connect(self._research)
        self.backend_button = QPushButton("Switch backend")
        self.backend_button.clicked.connect(
            lambda: self.stack.setCurrentWidget(self.backend_panel)
        )
        self.proton_button = QPushButton("Change Proton")
        self.proton_button.clicked.connect(
            lambda: self.stack.setCurrentWidget(self.proton_panel)
        )
        self.diagnostics_button = QPushButton("Troubleshoot")
        self.diagnostics_button.clicked.connect(self._open_diagnostics)
        actions.addWidget(self.recommend_button, 0, 0)
        actions.addWidget(self.backend_button, 0, 1)
        actions.addWidget(self.proton_button, 1, 0)
        actions.addWidget(self.diagnostics_button, 1, 1)
        actions.setColumnStretch(0, 1)
        actions.setColumnStretch(1, 1)
        content.addLayout(actions)

        self.stack = QStackedWidget()
        self.stack.setMinimumHeight(292)
        self.home_panel = self._message_panel(
            "Choose an action above. Manual changes use Steam's own Properties window and do not restart Steam."
        )
        self.backend_panel = self._build_backend_panel()
        self.proton_panel = self._build_proton_panel()
        self.recommend_panel = self._build_recommend_panel()
        self.confirm_panel = self._build_confirm_panel()
        self.diagnostics_panel = self._build_diagnostics_panel()
        for panel in (
            self.home_panel,
            self.backend_panel,
            self.proton_panel,
            self.recommend_panel,
            self.confirm_panel,
            self.diagnostics_panel,
        ):
            self.stack.addWidget(panel)
        content.addWidget(self.stack)

        self.status = QLabel("")
        self.status.setObjectName("muted")
        self.status.setWordWrap(True)
        self.status.setContentsMargins(0, 4, 0, 4)
        self.status.setMinimumHeight(self.status.fontMetrics().lineSpacing() + 8)
        content.addWidget(self.status)
        content.addStretch()
        content_widget = QWidget()
        content_widget.setLayout(content)
        root.addWidget(content_widget, 1)

    def _message_panel(self, text):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        label = QLabel(text)
        label.setWordWrap(True)
        label.setObjectName("muted")
        layout.addWidget(label)
        layout.addStretch()
        return frame

    def _build_backend_panel(self):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        heading = QLabel("Switch graphics backend")
        heading.setStyleSheet("font-size: 17px; font-weight: 700;")
        self.backend_note = QLabel(
            "Choose how Vet should apply the backend, then select one below."
        )
        self.backend_note.setObjectName("muted")
        self.backend_note.setWordWrap(True)
        method_heading = QLabel("HOW TO APPLY")
        method_heading.setObjectName("section")
        method = QHBoxLayout()
        self.backend_method_group = QButtonGroup(self)
        self.backend_method_group.setExclusive(True)
        manual_card = QFrame()
        manual_card.setObjectName("card")
        manual_layout = QVBoxLayout(manual_card)
        manual_layout.setContentsMargins(12, 9, 12, 9)
        self.backend_manual_method = QRadioButton("Copy && Open Steam")
        self.backend_manual_method.setMinimumHeight(34)
        self.backend_manual_method.setChecked(True)
        self.backend_manual_method.clicked.connect(
            lambda: setattr(self, "backend_automatic", False)
        )
        manual_hint = QLabel("Recommended · no restart")
        manual_hint.setObjectName("muted")
        manual_layout.addWidget(self.backend_manual_method)
        manual_layout.addWidget(manual_hint)

        automatic_card = QFrame()
        automatic_card.setObjectName("card")
        automatic_layout = QVBoxLayout(automatic_card)
        automatic_layout.setContentsMargins(12, 9, 12, 9)
        self.backend_auto_method = QRadioButton("Apply automatically")
        self.backend_auto_method.setMinimumHeight(34)
        self.backend_auto_method.clicked.connect(
            lambda: setattr(self, "backend_automatic", True)
        )
        automatic_hint = QLabel("Closes and restarts Steam")
        automatic_hint.setObjectName("muted")
        automatic_layout.addWidget(self.backend_auto_method)
        automatic_layout.addWidget(automatic_hint)
        self.backend_method_group.addButton(self.backend_manual_method)
        self.backend_method_group.addButton(self.backend_auto_method)
        method.addWidget(manual_card, 1)
        method.addWidget(automatic_card, 1)
        self.backend_automatic = False
        backend_heading = QLabel("CHOOSE BACKEND")
        backend_heading.setObjectName("section")
        row = QHBoxLayout()
        self.backend_buttons = {}
        self.backend_selection_group = QButtonGroup(self)
        self.backend_selection_group.setExclusive(True)
        for mode, label in (
            ("neptune", "Neptune"),
            ("neptune-dx12", "Neptune DX12"),
            ("venus", "Venus"),
        ):
            button = QPushButton(label)
            button.setCheckable(True)
            button.clicked.connect(
                lambda _checked=False, value=mode: self._choose_backend(value)
            )
            self.backend_selection_group.addButton(button)
            self.backend_buttons[mode] = button
            row.addWidget(button)
        back = QPushButton("Back")
        back.clicked.connect(lambda: self.stack.setCurrentWidget(self.home_panel))
        layout.addWidget(heading)
        layout.addWidget(self.backend_note)
        layout.addSpacing(4)
        layout.addWidget(method_heading)
        layout.addLayout(method)
        layout.addSpacing(6)
        layout.addWidget(backend_heading)
        layout.addLayout(row)
        layout.addStretch()
        layout.addWidget(back, alignment=Qt.AlignmentFlag.AlignRight)
        return frame

    def _build_proton_panel(self):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        heading = QLabel("Change Proton")
        heading.setStyleSheet("font-size: 17px; font-weight: 700;")
        self.proton_combo = QComboBox()
        self.proton_combo.addItem("Steam default", None)
        row = QHBoxLayout()
        manual = QPushButton("Open Steam Properties")
        manual.setObjectName("primary")
        manual.clicked.connect(self._manual_proton)
        automatic = QPushButton("Apply automatically")
        automatic.clicked.connect(self._automatic_proton)
        back = QPushButton("Back")
        back.clicked.connect(lambda: self.stack.setCurrentWidget(self.home_panel))
        row.addWidget(manual)
        row.addWidget(automatic)
        row.addStretch()
        row.addWidget(back)
        layout.addWidget(heading)
        layout.addWidget(self.proton_combo)
        layout.addLayout(row)
        layout.addStretch()
        return frame

    def _build_recommend_panel(self):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        self.recommend_title = QLabel("Recommended setup")
        self.recommend_title.setStyleSheet("font-size: 17px; font-weight: 700;")
        self.recommend_text = QLabel("")
        self.recommend_text.setWordWrap(True)
        self.recommend_text.setObjectName("muted")
        self.recommend_arguments_label = QLabel("Launch arguments")
        self.recommend_arguments_label.setObjectName("requirementLabel")
        self.recommend_arguments = QLabel("")
        self.recommend_arguments.setWordWrap(True)
        self.recommend_arguments.setObjectName("requirements")
        self.recommend_environment_label = QLabel("Environment")
        self.recommend_environment_label.setObjectName("requirementLabel")
        self.recommend_environment = QLabel("")
        self.recommend_environment.setWordWrap(True)
        self.recommend_environment.setObjectName("requirements")
        self.recommend_warning = QLabel("")
        self.recommend_warning.setWordWrap(True)
        self.recommend_warning.setObjectName("warning")
        actions = QHBoxLayout()
        actions.setSpacing(10)
        self.manual_button = QPushButton("Copy && Open Steam")
        self.manual_button.setObjectName("primary")
        self.manual_button.clicked.connect(self._manual_recommendation)
        self.auto_button = QPushButton("Apply automatically")
        self.auto_button.clicked.connect(self._confirm_recommendation)
        self.guide_button = QPushButton("Open setup guide")
        self.guide_button.clicked.connect(self._open_setup_guide)
        self.details_button = QPushButton("Details")
        self.details_button.clicked.connect(self._details)
        actions.addWidget(self.manual_button, 2)
        actions.addWidget(self.auto_button, 2)
        actions.addWidget(self.guide_button, 2)
        actions.addWidget(self.details_button, 1)
        layout.addWidget(self.recommend_title)
        layout.addWidget(self.recommend_text)
        layout.addWidget(self.recommend_arguments_label)
        layout.addWidget(self.recommend_arguments)
        layout.addWidget(self.recommend_environment_label)
        layout.addWidget(self.recommend_environment)
        layout.addWidget(self.recommend_warning)
        layout.addStretch()
        layout.addLayout(actions)
        return frame

    def _build_confirm_panel(self):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        heading = QLabel("Restart Steam and apply?")
        heading.setStyleSheet("font-size: 17px; font-weight: 700;")
        self.confirm_text = QLabel("")
        self.confirm_text.setWordWrap(True)
        self.confirm_text.setObjectName("muted")
        row = QHBoxLayout()
        apply = QPushButton("Restart Steam && Apply")
        apply.setObjectName("primary")
        apply.clicked.connect(self._apply_confirmed)
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self._cancel_confirmation)
        row.addWidget(apply)
        row.addWidget(cancel)
        row.addStretch()
        layout.addWidget(heading)
        layout.addWidget(self.confirm_text)
        layout.addStretch()
        layout.addLayout(row)
        return frame

    def _build_diagnostics_panel(self):
        frame = QFrame()
        frame.setObjectName("panel")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(20, 18, 20, 18)
        self.diagnostics_title = QLabel("Capture diagnostics")
        self.diagnostics_title.setStyleSheet("font-size: 17px; font-weight: 700;")
        self.diagnostics_text = QLabel("")
        self.diagnostics_text.setWordWrap(True)
        self.diagnostics_text.setObjectName("muted")
        row = QHBoxLayout()
        self.capture_button = QPushButton("Capture next launch")
        self.capture_button.setObjectName("primary")
        self.capture_button.clicked.connect(self._arm_diagnostics)
        self.report_button = QPushButton("View report")
        self.report_button.clicked.connect(self._view_latest_report)
        self.cancel_capture_button = QPushButton("Cancel capture")
        self.cancel_capture_button.clicked.connect(self._cancel_diagnostics)
        back = QPushButton("Back")
        back.clicked.connect(lambda: self.stack.setCurrentWidget(self.home_panel))
        row.addWidget(self.capture_button)
        row.addWidget(self.report_button)
        row.addWidget(self.cancel_capture_button)
        row.addStretch()
        row.addWidget(back)
        layout.addWidget(self.diagnostics_title)
        layout.addWidget(self.diagnostics_text)
        layout.addStretch()
        layout.addLayout(row)
        return frame

    def _populate_games(self, selected_appid):
        self.game_list.blockSignals(True)
        self.game_list.clear()
        selected = None
        for game in self.games:
            item = QListWidgetItem(game["name"])
            item.setData(Qt.ItemDataRole.UserRole, game)
            self.game_list.addItem(item)
            if game["appid"] == selected_appid:
                selected = item
        if self.games:
            selected = selected or self.game_list.item(0)
            self.game_list.setCurrentItem(selected)
            self._filter_games(self.search.text())
            self.game_list.blockSignals(False)
            if not self.state or self.state["appid"] != selected.data(
                Qt.ItemDataRole.UserRole
            )["appid"]:
                self._game_selected(selected, None)
        else:
            self.game_list.blockSignals(False)
            self.state = None
            self.game_title.setText("No installed games")
            self.game_subtitle.setText(
                "Steam did not report any fully installed games."
            )
            self._set_actions_enabled(False)

    def _filter_games(self, text):
        needle = text.casefold().strip()
        for index in range(self.game_list.count()):
            item = self.game_list.item(index)
            item.setHidden(needle not in item.text().casefold())

    def _game_selected(self, current, _previous):
        if current is None:
            return
        game = current.data(Qt.ItemDataRole.UserRole)
        try:
            self.configuration_revision = self.service.steam.configuration_revision()
            state = self.service.game_state(game["appid"])
        except Exception as exception:
            self._show_error(str(exception))
            return
        self._show_game_state(game, state)

    def _show_game_state(self, game, state):
        self.state = state
        self.prepared = None
        self.backend_automatic = False
        self.backend_manual_method.setChecked(True)
        self.game_title.setText(self.state["name"] or game["name"])
        self.game_subtitle.setText(f"Steam App {game['appid']}")
        self.values["Graphics"].setText(
            "Unrecognized"
            if self.state["launch_options_error"]
            else display_graphics(self.state["effective_graphics_mode"])
            + (" (Steam default)" if not self.state["graphics_mode"] else "")
        )
        host_backend = self.state["effective_host_backend"]
        self.value_names["Host backend"].setVisible(bool(host_backend))
        self.values["Host backend"].setVisible(bool(host_backend))
        if host_backend:
            self.values["Host backend"].setText(host_backend)
        warning = current_setup_warning(self.state)
        self.runtime_warning.setText(f"⚠ {warning}" if warning else "")
        self.runtime_warning.setVisible(bool(warning))
        self.values["Proton override"].setText(
            display_proton(self.state["proton"])
            if self.state["proton"]
            else "Steam default"
        )
        self.values["Effective Proton"].setText(
            display_proton(self.state["effective_proton"])
        )
        self.values["Launch Options"].setText(self.state["launch_options"] or "Empty")
        self._mark_current_backend()
        self.backend_note.setText(
            "Choose how to apply the change, then pick a backend. "
            "Venus supports Proton 10 or older."
        )
        self._reload_proton_choices()
        if self.state["launch_options_error"]:
            self._show_warning(self.state["launch_options_error"])
        else:
            self._show_status("")
        self._update_diagnostic_panel()
        self.stack.setCurrentWidget(self.home_panel)
        self._fit_stack_to_contents()
        self._set_actions_enabled(True)

    def _fit_stack_to_contents(self):
        heights = [
            self.stack.widget(index).sizeHint().height()
            for index in range(self.stack.count())
        ]
        if heights:
            self.stack.setMinimumHeight(max(self.stack.minimumHeight(), *heights))

    def _mark_current_backend(self):
        current = self.state["effective_graphics_mode"]
        self.backend_selection_group.setExclusive(False)
        for mode, button in self.backend_buttons.items():
            selected = mode == current
            button.setChecked(selected)
            button.setText(
                f"{display_graphics(mode)} · Current"
                if selected
                else display_graphics(mode)
            )
        self.backend_selection_group.setExclusive(True)

    def _reload_proton_choices(self):
        selected = self.state["proton"] if self.state else None
        self.proton_combo.clear()
        self.proton_combo.addItem("Steam default", None)
        for tool in self.service.steam.proton_tools():
            self.proton_combo.addItem(tool["display_name"], tool["name"])
        if selected and self.proton_combo.findData(selected) < 0:
            self.proton_combo.addItem(
                f"{display_proton(selected)} (current, unavailable)", selected
            )
        index = self.proton_combo.findData(selected)
        self.proton_combo.setCurrentIndex(max(index, 0))

    def _refresh_from_steam(self):
        try:
            games = self.service.steam.installed_games()
        except Exception:
            games = self.games
        if [(game["appid"], game["name"]) for game in games] != [
            (game["appid"], game["name"]) for game in self.games
        ]:
            current = self.game_list.currentItem()
            selected_appid = (
                current.data(Qt.ItemDataRole.UserRole)["appid"] if current else None
            )
            self.games = games
            self._populate_games(selected_appid)
        current = self.game_list.currentItem()
        if current is None:
            return
        self._refresh_diagnostics()
        try:
            revision = self.service.steam.configuration_revision()
        except Exception:
            return
        if revision == self.configuration_revision:
            return
        game = current.data(Qt.ItemDataRole.UserRole)
        try:
            state = self.service.game_state(game["appid"])
        except Exception:
            return
        self.configuration_revision = revision
        if state == self.state:
            return
        self._show_game_state(game, state)
        if not state["launch_options_error"]:
            self._show_status("Updated from Steam.")

    def activate_request(self, selected_appid=None, diagnostic_run_id=None):
        current = self.game_list.currentItem()
        current_appid = (
            current.data(Qt.ItemDataRole.UserRole)["appid"] if current else None
        )
        target_appid = selected_appid or current_appid
        if selected_appid:
            self.search.clear()
        try:
            self.games = self.service.steam.installed_games()
        except Exception:
            pass
        self._populate_games(target_appid)
        if target_appid:
            for index in range(self.game_list.count()):
                item = self.game_list.item(index)
                if item.data(Qt.ItemDataRole.UserRole)["appid"] == target_appid:
                    if item == self.game_list.currentItem():
                        self._game_selected(item, None)
                    else:
                        self.game_list.setCurrentItem(item)
                    break
        if diagnostic_run_id:
            self._show_diagnostic_report(diagnostic_run_id)
        self.showNormal()
        self.raise_()
        self.activateWindow()

    def _open_diagnostics(self):
        self._update_diagnostic_panel()
        self._show_status("")
        self.stack.setCurrentWidget(self.diagnostics_panel)

    def _refresh_diagnostics(self):
        if not self.state:
            return
        try:
            snapshot = self.service.diagnostic_status(self.state["appid"])
        except Exception:
            return
        if snapshot == self.diagnostic_snapshot:
            return
        previous = self.diagnostic_snapshot
        self.diagnostic_snapshot = snapshot
        self._render_diagnostic_snapshot(snapshot)
        if (
            previous
            and not previous.get("latest_completed")
            and snapshot.get("latest_completed")
        ):
            self._show_status("Diagnostic captured. The report is ready.")

    def _update_diagnostic_panel(self):
        if not self.state:
            return
        try:
            self.diagnostic_snapshot = self.service.diagnostic_status(
                self.state["appid"]
            )
            self._render_diagnostic_snapshot(self.diagnostic_snapshot)
        except Exception as exception:
            self.diagnostics_title.setText("Diagnostics unavailable")
            self.diagnostics_text.setText(str(exception))
            self.capture_button.setVisible(False)
            self.report_button.setVisible(False)
            self.cancel_capture_button.setVisible(False)

    def _render_diagnostic_snapshot(self, snapshot):
        pending = snapshot.get("pending")
        completed = snapshot.get("latest_completed")
        pending_changes = snapshot.get("pending_setup_changes", [])
        latest_changes = snapshot.get("latest_setup_changes", [])
        age = capture_age(snapshot.get("latest_completed_at"))
        managed = bool(
            self.state
            and self.state["graphics_mode"]
            and not self.state["launch_options_error"]
        )
        needs_wrapper = bool(
            self.state and diagnostics_wrapper_required(self.state)
        )
        if pending:
            if pending_changes:
                self.diagnostics_title.setText("Setup changed after capture was armed")
                self.diagnostics_text.setText(
                    f"Differences: {', '.join(pending_changes)}. Cancel this capture and arm a new one for the current setup."
                )
            else:
                self.diagnostics_title.setText("Ready for the next launch")
                self.diagnostics_text.setText(
                    "Launch the game normally in Steam and reproduce the problem. "
                    "Vet will capture this launch only."
                )
        elif completed:
            if latest_changes:
                self.diagnostics_title.setText("Previous setup captured")
                self.diagnostics_text.setText(
                    f"{age}. It no longer matches the current setup. Differences: {', '.join(latest_changes)}."
                )
            else:
                self.diagnostics_title.setText("Current setup captured")
                self.diagnostics_text.setText(
                    f"{age}. The report matches the current game setup."
                )
        elif snapshot.get("latest_run_id"):
            self.diagnostics_title.setText("Capture in progress")
            self.diagnostics_text.setText(
                "Wait for the game to exit. Vet will prepare the report automatically."
            )
        elif managed:
            self.diagnostics_title.setText("Capture diagnostics")
            self.diagnostics_text.setText(
                "Capture a Proton log from one launch. After arming it, start the game normally in Steam."
            )
        elif needs_wrapper:
            self.diagnostics_title.setText("Enable diagnostic capture")
            self.diagnostics_text.setText(
                "Venus is already active through Steam defaults. Vet needs the "
                "Varmint Venus wrapper in Launch Options to capture a Proton log."
            )
        else:
            self.diagnostics_title.setText("Diagnostics unavailable")
            self.diagnostics_text.setText(
                "Fix the current Launch Options before capturing diagnostics."
            )
        self.capture_button.setVisible((managed and not pending) or needs_wrapper)
        if needs_wrapper:
            self.capture_button.setText("Enable capture")
        else:
            self.capture_button.setText(
                "Capture current setup"
                if completed and latest_changes
                else "Capture again"
                if completed
                else "Capture next launch"
            )
        self.report_button.setText(
            "View previous report" if completed and latest_changes else "View report"
        )
        self.report_button.setVisible(bool(completed))
        self.cancel_capture_button.setVisible(bool(pending))

    def _arm_diagnostics(self):
        if diagnostics_wrapper_required(self.state):
            plan = override_plan(
                self.state,
                launch_options=transform_launch_options(
                    self.state["launch_options"], "venus"
                ),
            )
            configure_manually(plan)
            self._show_status(
                "Launch Options copied. Paste them in Steam Properties, then return here."
            )
            return
        try:
            self.service.arm_diagnostics(self.state["appid"])
            self._update_diagnostic_panel()
            self._show_status("Diagnostics armed for the next launch.")
            self.stack.setCurrentWidget(self.diagnostics_panel)
        except VetError as exception:
            self._show_error(str(exception))

    def _cancel_diagnostics(self):
        self.service.cancel_diagnostics(self.state["appid"])
        self._update_diagnostic_panel()
        self._show_status("Diagnostic capture cancelled.")

    def _view_latest_report(self):
        run_id = (self.diagnostic_snapshot or {}).get("latest_run_id")
        if run_id:
            self._show_diagnostic_report(run_id)

    def _show_diagnostic_report(self, run_id):
        try:
            report = self.service.diagnostic_report(run_id)
        except VetError as exception:
            self._show_error(str(exception))
            return
        ReportDialog(report, self).exec()

    def _research(self):
        if not self.state:
            return
        self.request += 1
        request = self.request
        appid = self.state["appid"]
        name = self.state["name"]
        self._show_status("Checking Varmint guides and PCGamingWiki…")
        self._set_actions_enabled(False)

        def work():
            try:
                self.signals.prepared.emit(request, self.service.prepare(appid, name))
            except Exception as exception:
                self.signals.failed.emit(request, str(exception))

        threading.Thread(target=work, daemon=True).start()

    def _prepared(self, request, value):
        if request != self.request:
            return
        self.prepared = value
        game, state, research, recommendation, plan = value
        configurable = bool(recommendation.graphics_mode or recommendation.proton)
        actionable = configurable and recommendation.setup != "manual"
        setup = [
            display_graphics(recommendation.graphics_mode)
            if recommendation.graphics_mode
            else None,
            f"Host backend: {recommendation.host_backend}"
            if recommendation.host_backend
            else None,
            display_proton(recommendation.proton) if recommendation.proton else None,
        ]
        text = " · ".join(value for value in setup if value)
        if recommendation.message:
            text = (
                f"{text}\n{recommendation.message}" if text else recommendation.message
            )
        if recommendation.setup == "assisted":
            text += "\nThis guide includes required manual steps."
        elif recommendation.setup == "manual":
            text += "\nFollow the Varmint guide to finish setup."
        has_changes = any(plan["changes"].values())
        if actionable and not has_changes:
            text += (
                "\nSteam settings are already configured."
                if recommendation.setup == "assisted" or plan.get("host_warning")
                else "\nAlready configured."
            )
        if recommendation.setup == "manual" and recommendation.reason.startswith(
            "varmint_"
        ):
            title = "Manual setup"
        else:
            title = "Recommended setup" if actionable else "No verified setup"
        self.recommend_title.setText(title)
        self.recommend_text.setText(text)
        arguments, environment = format_launch_requirements(
            recommendation.append_args, recommendation.environment
        )
        self.recommend_arguments.setText(arguments)
        self.recommend_arguments.setVisible(bool(arguments))
        self.recommend_arguments_label.setVisible(bool(arguments))
        self.recommend_environment.setText(environment)
        self.recommend_environment.setVisible(bool(environment))
        self.recommend_environment_label.setVisible(bool(environment))
        warning = (
            None if current_host_backend_warning(state) else plan.get("host_warning")
        )
        self.recommend_warning.setText(f"⚠ {warning}" if warning else "")
        self.recommend_warning.setVisible(bool(warning))
        facts = research.varmint_doc.facts
        has_guide = bool(facts and facts.guide_markdown)
        guide_only = has_guide and (
            recommendation.setup == "manual" or not has_changes
        )
        self.manual_button.setVisible((actionable and has_changes) or guide_only)
        if guide_only:
            self.manual_button.setText("Open setup guide")
        else:
            self.manual_button.setText(
                "Copy && Open Steam"
                if plan["changes"]["launch_options"]
                else "Open Steam Properties"
            )
        self.guide_button.setVisible(
            has_guide and recommendation.setup == "assisted" and has_changes
        )
        self.auto_button.setVisible(
            recommendation.setup == "automatic" and actionable and has_changes
        )
        self.details_button.setVisible(True)
        self._show_status("")
        self.stack.setCurrentWidget(self.recommend_panel)
        self._fit_stack_to_contents()
        self._set_actions_enabled(True)

    def _choose_backend(self, mode):
        self._mark_current_backend()
        try:
            plan = switch_backend_plan(self.state, mode)
        except VetError as exception:
            self._show_error(str(exception))
            return
        if not any(plan["changes"].values()):
            self._show_status(f"Steam already uses {display_graphics(mode)}.")
            return
        if self.backend_automatic:
            self.prepared = (None, self.state, None, None, plan)
            self._show_confirmation(plan, self.backend_panel)
            return
        configure_manually(plan)
        if plan["changes"]["proton"]:
            self._show_status(
                "Steam Properties opened. Apply the Launch Options and Proton selection."
                if plan["changes"]["launch_options"]
                else "Steam Properties opened. Select Proton 10 in Compatibility."
            )
        else:
            self._show_status(
                "Launch Options copied. Paste them in the Steam Properties window."
            )

    def _manual_proton(self):
        plan = override_plan(self.state, proton=self.proton_combo.currentData())
        if configure_manually(plan):
            self._show_status(
                "Steam Properties opened. Apply the Proton selection there."
            )
        else:
            self._show_status("Steam already uses this Proton selection.")

    def _automatic_proton(self):
        plan = override_plan(self.state, proton=self.proton_combo.currentData())
        if not any(plan["changes"].values()):
            self._show_status("Steam already uses this Proton selection.")
            return
        self.prepared = (None, self.state, None, None, plan)
        self._show_confirmation(plan, self.proton_panel)

    def _manual_recommendation(self):
        research = self.prepared[2]
        recommendation = self.prepared[3]
        plan = self.prepared[4]
        facts = research.varmint_doc.facts
        if facts and facts.guide_markdown and (
            recommendation.setup == "manual" or not any(plan["changes"].values())
        ):
            self._open_setup_guide()
            return
        if configure_manually(plan):
            if plan["changes"]["launch_options"] and plan["changes"]["proton"]:
                self._show_status(
                    "Launch Options copied. Paste them in General and select "
                    f"{display_proton(plan['after']['proton'])} in Compatibility."
                )
            elif plan["changes"]["launch_options"]:
                self._show_status(
                    "Launch Options copied. Paste them in Steam Properties."
                )
            else:
                self._show_status(
                    "Steam Properties opened. Select "
                    f"{display_proton(plan['after']['proton'])} in Compatibility."
                )
        else:
            self._show_status("Steam already has this configuration.")

    def _open_setup_guide(self):
        if not self.prepared:
            return
        facts = self.prepared[2].varmint_doc.facts
        if facts and facts.guide_markdown:
            GuideDialog(facts.guide_markdown, self).exec()

    def _confirm_recommendation(self):
        self._show_confirmation(self.prepared[4], self.recommend_panel)

    def _show_confirmation(self, plan, return_panel):
        if self.service.steam.any_game_running():
            self._show_error(
                "Close the running game before changing its Steam configuration."
            )
            return
        if self.service.steam.running():
            message = "Steam must close before Vet edits its configuration. Vet will restart it after applying the changes."
        else:
            message = "Steam is already closed. Vet will apply the changes without starting it."
        launch = (
            plan["after"]["launch_options"]
            if plan["changes"]["launch_options"]
            else None
        )
        if launch:
            message += f"\n\nLaunch Options: {launch}"
        self.confirm_text.setText(message)
        self.confirm_return_panel = return_panel
        self.stack.setCurrentWidget(self.confirm_panel)
        self._fit_stack_to_contents()

    def _cancel_confirmation(self):
        self.stack.setCurrentWidget(self.confirm_return_panel or self.home_panel)

    def _apply_confirmed(self):
        plan = self.prepared[4]
        self.request += 1
        request = self.request
        self._show_status("Applying Steam configuration…")
        self._set_actions_enabled(False)

        def work():
            try:
                self.signals.applied.emit(
                    request, self.service.apply(plan, restart_steam=True)
                )
            except Exception as exception:
                self.signals.failed.emit(request, str(exception))

        threading.Thread(target=work, daemon=True).start()

    def _applied(self, request, result):
        if request != self.request:
            return
        message = "Steam configuration updated."
        if result["steam_restarted"]:
            message += " Steam was restarted."
        current = self.game_list.currentItem()
        self._game_selected(current, None)
        self._show_status(message)

    def _details(self):
        if not self.prepared:
            return
        game, state, research, recommendation, _plan = self.prepared
        graphics = display_graphics(state["effective_graphics_mode"])
        if not state["graphics_mode"]:
            graphics += " (Steam default)"
        proton = display_proton(state["effective_proton"])
        proton_source = "per-game override" if state["proton"] else "Steam default"
        sections = [
            f"CURRENT SETUP\n{graphics} · {proton} ({proton_source})",
            f"LAUNCH OPTIONS\n{state['launch_options'] or 'None'}",
        ]
        if state["effective_host_backend"]:
            sections.insert(
                1, f"HOST GRAPHICS BACKEND\n{state['effective_host_backend']}"
            )
        if research.parent_game:
            parent = research.parent_game
            sections.append(
                f"FULL-GAME DATA\n{parent.name} · Steam App {parent.appid}\n{parent.source_url}"
            )
        recommended = " · ".join(
            value
            for value in (
                display_graphics(recommendation.graphics_mode)
                if recommendation.graphics_mode
                else None,
                f"Host backend: {recommendation.host_backend}"
                if recommendation.host_backend
                else None,
                display_proton(recommendation.proton)
                if recommendation.proton
                else None,
            )
            if value
        )
        if recommended:
            sections.append(f"RECOMMENDATION\n{recommended}\n{recommendation.message}")
        if recommendation.details:
            sections.append("NOTES\n" + recommendation.details)
        if recommendation.sources:
            seen = set()
            sources = []
            for source in recommendation.sources:
                if source.url in seen:
                    continue
                seen.add(source.url)
                label = (
                    "Varmint guide" if source.kind == "varmint_guide" else source.title
                )
                sources.append(f"{label}\n{source.url}")
            sections.append("SOURCES\n" + "\n\n".join(sources))
        if not recommended and research.warnings:
            warnings = [
                "No matching PCGamingWiki page was found for this Steam game."
                if warning.startswith("PCGamingWiki has no exact AppID match")
                else warning
                for warning in research.warnings
            ]
            sections.append("DIAGNOSTICS\n" + "\n".join(warnings))
        DetailsDialog("Setup details", "\n\n".join(sections), self).exec()

    def _set_suggestion_notifications(self, enabled):
        self.preferences.set_suggestions_enabled(enabled)
        self._show_status(
            "Automatic setup suggestions enabled."
            if enabled
            else "Automatic setup suggestions disabled."
        )

    def _failed(self, request, message):
        if request != self.request:
            return
        self._show_error(message)
        self._set_actions_enabled(True)

    def _show_error(self, message):
        self.status.setText(message)
        self.status.setStyleSheet("color: #ff9b94;")

    def _show_warning(self, message):
        self.status.setText(message)
        self.status.setStyleSheet("color: #e2bd6f;")

    def _show_status(self, message):
        self.status.setText(message)
        self.status.setStyleSheet("")

    def _set_actions_enabled(self, enabled):
        self.recommend_button.setEnabled(enabled)
        self.backend_button.setEnabled(enabled)
        self.proton_button.setEnabled(enabled)
        self.diagnostics_button.setEnabled(enabled)
        self.game_list.setEnabled(enabled)

    def _center(self):
        screen = QGuiApplication.primaryScreen()
        if screen:
            self.move(screen.availableGeometry().center() - self.rect().center())


def run(service=None, selected_appid=None, diagnostic_run_id=None):
    application = QApplication.instance() or QApplication([])
    application.setApplicationName("Vet")
    application.setDesktopFileName("vet")
    if icon := icon_path():
        application.setWindowIcon(QIcon(icon))
    if scheme := color_scheme_path():
        application.setProperty("KDE_COLOR_SCHEME_PATH", scheme)
    request = {
        "appid": selected_appid,
        "diagnostic_run_id": diagnostic_run_id,
    }
    server_name = f"varmint-vet-{os.getuid()}"
    client = QLocalSocket()
    client.connectToServer(server_name)
    if client.waitForConnected(200):
        client.write((json.dumps(request) + "\n").encode())
        client.waitForBytesWritten(500)
        client.disconnectFromServer()
        return 0

    server = QLocalServer(application)
    if not server.listen(server_name):
        client.connectToServer(server_name)
        if client.waitForConnected(500):
            client.write((json.dumps(request) + "\n").encode())
            client.waitForBytesWritten(500)
            client.disconnectFromServer()
            return 0
        QLocalServer.removeServer(server_name)
        if not server.listen(server_name):
            raise RuntimeError(f"Could not open Vet instance socket: {server.errorString()}")

    window = VetWindow(service or VetService(), selected_appid, diagnostic_run_id)

    clients = {}

    def read_request(socket):
        buffer = clients.get(socket)
        if buffer is None:
            return
        buffer.extend(bytes(socket.readAll()))
        while b"\n" in buffer:
            line, _, remainder = buffer.partition(b"\n")
            buffer = bytearray(remainder)
            clients[socket] = buffer
            try:
                message = json.loads(line)
                if not isinstance(message, dict):
                    continue
                window.activate_request(
                    message.get("appid"), message.get("diagnostic_run_id")
                )
            except (json.JSONDecodeError, TypeError, ValueError):
                pass

    def accept_requests():
        while server.hasPendingConnections():
            socket = server.nextPendingConnection()
            clients[socket] = bytearray()
            socket.readyRead.connect(lambda socket=socket: read_request(socket))
            socket.disconnected.connect(lambda socket=socket: clients.pop(socket, None))
            read_request(socket)

    server.newConnection.connect(accept_requests)
    window.show()
    result = application.exec()
    server.close()
    return result

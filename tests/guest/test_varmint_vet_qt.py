"""Qt-specific Vet regressions that run when PyQt6 is available."""

import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest


source = os.environ.get("VARMINT_GUEST_SOURCE")
GUEST = Path(source) if source else Path(__file__).resolve().parents[2] / "guest"
sys.path.insert(0, str(GUEST))
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

try:
    from PyQt6.QtCore import Qt
    from PyQt6.QtWidgets import QApplication
    import varmint_vet_qt as qt
except ModuleNotFoundError:
    qt = None


@unittest.skipIf(qt is None, "PyQt6 is only installed in the guest")
class GameListRefreshTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.application = QApplication.instance() or QApplication([])

    def setUp(self):
        self.steam = SimpleNamespace(
            games=[{"appid": 1, "name": "First", "manifest_time": 1}],
            installed_games=lambda: list(self.steam.games),
            configuration_revision=lambda: 0,
        )
        service = SimpleNamespace(steam=self.steam)

        class Window(qt.VetWindow):
            def _game_selected(window, current, _previous):
                if current:
                    game = current.data(Qt.ItemDataRole.UserRole)
                    window.state = {"appid": game["appid"]}
                    window.configuration_revision = 0

            def _refresh_diagnostics(window):
                pass

        self.window = Window(service)
        self.window.refresh_timer.stop()

    def tearDown(self):
        self.window.close()

    def test_refresh_adds_games_without_losing_selection(self):
        self.steam.games.append(
            {"appid": 2, "name": "Second", "manifest_time": 2}
        )

        self.window._refresh_from_steam()

        self.assertEqual(self.window.game_list.count(), 2)
        selected = self.window.game_list.currentItem().data(Qt.ItemDataRole.UserRole)
        self.assertEqual(selected["appid"], 1)

    def test_activation_refreshes_and_selects_new_game(self):
        self.steam.games.append(
            {"appid": 2, "name": "Second", "manifest_time": 2}
        )

        self.window.activate_request(2)

        self.assertEqual(self.window.game_list.count(), 2)
        selected = self.window.game_list.currentItem().data(Qt.ItemDataRole.UserRole)
        self.assertEqual(selected["appid"], 2)


if __name__ == "__main__":
    unittest.main()

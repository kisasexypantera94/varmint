"""Tests for the Varmint Steam launcher."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "guest/varmint-steam"


class SteamLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.steam = self.home / ".local/share/Steam"
        self.client_dir = self.steam / "steamrtarm64"
        self.client_dir.mkdir(parents=True)
        self.client = self.client_dir / "steam"
        self.client.write_text(
            """#!/bin/sh
printf '%s\\n' "$@" > "$HOME/client-args"
if [ -d "$HOME/.local/share/Steam/steamrt64" ] && \
   [ ! -L "$HOME/.local/share/Steam/steamrt64" ]; then
    touch "$HOME/client-saw-separate-steamrt64"
fi
if [ ! -e "$HOME/.local/share/Steam/.steam-enable-steamrt64-client" ]; then
    touch "$HOME/client-saw-no-steamrt64-marker"
fi
exit 23
"""
        )
        self.client.chmod(0o755)
        package = self.steam / "package"
        package.mkdir()
        (package / "steam_client_publicbeta_linuxarm64.installed").touch()

    def run_launcher(self, *arguments):
        environment = os.environ.copy()
        environment["HOME"] = str(self.home)
        return subprocess.run(
            [str(LAUNCHER), *arguments],
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_migrates_v040_directory_alias_before_launch(self):
        steamrt64 = self.steam / "steamrt64"
        steamrt64.symlink_to("steamrtarm64")
        marker = self.steam / ".steam-enable-steamrt64-client"
        marker.touch()

        result = self.run_launcher("-silent", "steam://open/games")

        self.assertEqual(result.returncode, 23)
        self.assertTrue(steamrt64.is_dir())
        self.assertFalse(steamrt64.is_symlink())
        self.assertFalse(marker.exists())
        self.assertTrue((self.home / "client-saw-separate-steamrt64").exists())
        self.assertTrue((self.home / "client-saw-no-steamrt64-marker").exists())
        self.assertEqual(
            (self.home / "client-args").read_text().splitlines(),
            ["-silent", "steam://open/games"],
        )

    def test_migrates_absolute_v040_directory_alias(self):
        steamrt64 = self.steam / "steamrt64"
        steamrt64.symlink_to(self.client_dir)

        result = self.run_launcher()

        self.assertEqual(result.returncode, 23)
        self.assertTrue(steamrt64.is_dir())
        self.assertFalse(steamrt64.is_symlink())

    def test_preserves_marker_for_existing_separate_layout(self):
        (self.steam / "steamrt64").mkdir()
        marker = self.steam / ".steam-enable-steamrt64-client"
        marker.touch()

        result = self.run_launcher()

        self.assertEqual(result.returncode, 23)
        self.assertTrue(marker.exists())

    def test_relaunch_after_bootstrap_preserves_only_user_arguments(self):
        installed = (
            self.steam
            / "package/steam_client_publicbeta_linuxarm64.installed"
        )
        installed.unlink()
        self.client.write_text(
            f"""#!/bin/sh
if [ ! -f "$HOME/bootstrap-ran" ]; then
    touch "$HOME/bootstrap-ran"
    touch "{installed}"
    printf '%s\\n' "$@" > "$HOME/bootstrap-args"
    exit 42
fi
printf '%s\\n' "$@" > "$HOME/relaunch-args"
exit 0
"""
        )
        self.client.chmod(0o755)

        result = self.run_launcher("-silent")

        self.assertEqual(result.returncode, 0)
        self.assertEqual((self.home / "bootstrap-args").read_text(), "-silent\n")
        self.assertEqual((self.home / "relaunch-args").read_text(), "-silent\n")

    def test_refuses_unexpected_directory_alias(self):
        steamrt64 = self.steam / "steamrt64"
        steamrt64.symlink_to("custom-runtime")

        result = self.run_launcher()

        self.assertEqual(result.returncode, 1)
        self.assertTrue(steamrt64.is_symlink())
        self.assertEqual(os.readlink(steamrt64), "custom-runtime")
        self.assertFalse((self.home / "client-args").exists())
        self.assertIn("refusing to replace unexpected steamrt64 link", result.stderr)

    def test_refuses_non_directory_steamrt64(self):
        steamrt64 = self.steam / "steamrt64"
        steamrt64.write_text("user data")

        result = self.run_launcher()

        self.assertEqual(result.returncode, 1)
        self.assertEqual(steamrt64.read_text(), "user data")
        self.assertFalse((self.home / "client-args").exists())
        self.assertIn("steamrt64 is not a directory", result.stderr)


if __name__ == "__main__":
    unittest.main()

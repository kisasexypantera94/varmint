"""Regression tests for guest-side Vet ownership, research, and Steam mutation."""

import importlib
import json
import runpy
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from pathlib import Path
from unittest import mock


GUEST = Path(__file__).resolve().parents[2] / "guest"
sys.path.insert(0, str(GUEST))
vet = importlib.import_module("varmint_vet")
catalog = importlib.import_module("varmint_vet_catalog")
vdf = importlib.import_module("varmint_vdf")
actions = importlib.import_module("varmint_vet_actions")
diagnostics = importlib.import_module("varmint_vet_diagnostics")


LOCALCONFIG = """"UserLocalConfigStore"
{
\t"Software"
\t{
\t\t"Valve"
\t\t{
\t\t\t"Steam"
\t\t\t{
\t\t\t\t"apps"
\t\t\t\t{
\t\t\t\t\t"123"
\t\t\t\t\t{
\t\t\t\t\t\t"LaunchOptions"\t\t"MANGOHUD=1 %command% -novid"
\t\t\t\t\t}
\t\t\t\t}
\t\t\t}
\t\t}
\t}
}
"""

CONFIG = """"InstallConfigStore"
{
\t"Software"
\t{
\t\t"Valve"
\t\t{
\t\t\t"Steam"
\t\t\t{
\t\t\t\t"CompatToolMapping"
\t\t\t\t{
\t\t\t\t\t"0"
\t\t\t\t\t{
\t\t\t\t\t\t"name"\t\t"proton_experimental"
\t\t\t\t\t}
\t\t\t\t}
\t\t\t}
\t\t}
\t}
}
"""

MANIFEST = """"AppState"
{
\t"appid"\t\t"123"
\t"name"\t\t"Test Game"
\t"StateFlags"\t\t"4"
}
"""

CUSTOM_PROTON = """"compatibilitytools"
{
    "compat_tools"
    {
        "proton_9"
        {
            "display_name" "Proton 9"
            "install_path" "."
        }
    }
}
"""


def appinfo_v29(applications):
    keys = ["appinfo", "common", "name", "type", "config", "installdir"]
    indexes = {key: index for index, key in enumerate(keys)}

    def field(key, value):
        prefix = struct.pack("<BI", 0 if isinstance(value, dict) else 1, indexes[key])
        if isinstance(value, dict):
            return (
                prefix
                + b"".join(field(child, item) for child, item in value.items())
                + b"\x08"
            )
        return prefix + value.encode() + b"\0"

    records = []
    for appid, name, installdir in applications:
        payload = (
            field(
                "appinfo",
                {
                    "common": {"name": name, "type": "Tool"},
                    "config": {"installdir": installdir},
                },
            )
            + b"\x08"
        )
        records.append(struct.pack("<I", appid) + bytes(64) + payload)
    body = b"".join(records) + struct.pack("<I", 0)
    table_offset = 16 + len(body)
    table = struct.pack("<I", len(keys)) + b"".join(
        key.encode() + b"\0" for key in keys
    )
    return struct.pack("<IIQ", vdf.APPINFO_V29, 1, table_offset) + body + table


class LaunchOptionsTests(unittest.TestCase):
    def test_empty_default(self):
        self.assertEqual(
            vet.transform_launch_options("", "venus"),
            "varmint-graphics venus %command%",
        )

    def test_existing_wrapper_is_replaced(self):
        self.assertEqual(
            vet.transform_launch_options(
                "varmint-graphics neptune %command%", "neptune-dx12"
            ),
            "varmint-graphics neptune-dx12 %command%",
        )

    def test_switches_neptune_dx12_back_to_neptune(self):
        self.assertEqual(
            vet.transform_launch_options(
                "varmint-graphics neptune-dx12 %command% -novid", "neptune"
            ),
            "varmint-graphics neptune %command% -novid",
        )

    def test_venus_preserves_user_environment_and_arguments(self):
        self.assertEqual(
            vet.transform_launch_options(
                "MANGOHUD=1 FOO_MAGIC=banana %command% -novid", "venus"
            ),
            "MANGOHUD=1 FOO_MAGIC=banana varmint-graphics venus %command% -novid",
        )

    def test_removes_only_exact_known_conflict(self):
        self.assertEqual(
            vet.transform_launch_options(
                "varmint-graphics neptune %command% -dx11 -novid -dx11-tweaks",
                "neptune-dx12",
                remove_args=["-dx11"],
            ),
            "varmint-graphics neptune-dx12 %command% -novid -dx11-tweaks",
        )

    def test_adds_only_explicit_documented_argument(self):
        self.assertEqual(
            vet.transform_launch_options(
                "%command% -novid", "neptune", append_args=["-d3d11"]
            ),
            "varmint-graphics neptune %command% -novid -d3d11",
        )

    def test_adds_approved_game_argument_without_treating_it_as_graphics_state(self):
        self.assertEqual(
            vet.transform_launch_options(
                "MANGOHUD=1 FOO=bar %command% -novid",
                "neptune-dx12",
                append_args=["--launcher-skip"],
            ),
            "MANGOHUD=1 FOO=bar varmint-graphics neptune-dx12 "
            "%command% -novid --launcher-skip",
        )

    def test_rejects_ambiguous_or_unsafe_options(self):
        values = (
            "%command% %command%",
            "varmint-graphics neptune %command% | tee log",
            "varmint-graphics neptune %command% $(touch nope)",
            '"unterminated %command%',
            "varmint-graphics mystery %command%",
        )
        for value in values:
            with self.subTest(value=value), self.assertRaises(vet.VetError):
                vet.transform_launch_options(value, "neptune")


class ProtonNameTests(unittest.TestCase):
    def test_official_names_map_to_steam_keys(self):
        expected = {
            "Proton Experimental": "proton_experimental",
            "Proton Experimental (ARM64)": "proton-experimental-arm64",
            "Proton Hotfix": "proton_hotfix",
            "Proton 11.0": "proton_11",
            "Proton 10.0 (ARM64)": "proton_10-arm64",
            "Proton 7.0": "proton_7",
            "Proton 6.3": "proton_63",
            "Proton 5.0": "proton_5",
            "Proton 5.13": "proton_513",
        }
        for name, key in expected.items():
            with self.subTest(name=name):
                self.assertEqual(vet.official_proton_key(name), key)

    def test_non_proton_tool_has_no_key(self):
        self.assertIsNone(vet.official_proton_key("Proton EasyAntiCheat Runtime"))


class VdfTests(unittest.TestCase):
    PATH = (*vet.Steam.APP_PATH, "123", "LaunchOptions")

    def test_set_value_preserves_unrelated_text(self):
        updated = vdf.Document(LOCALCONFIG).set_value(self.PATH, 'FOO="bar" %command%')
        self.assertEqual(vdf.Document(updated).value(self.PATH), 'FOO="bar" %command%')
        self.assertIn('"UserLocalConfigStore"', updated)

    def test_delete_value_removes_only_leaf(self):
        document = vdf.Document(CONFIG)
        path = (*vet.Steam.COMPAT_PATH, "0", "name")
        updated = document.delete_value(path)
        self.assertIsNone(vdf.Document(updated).value(path))
        self.assertIn('"CompatToolMapping"', updated)

    def test_delete_node_removes_only_mapping(self):
        path = (*vet.Steam.COMPAT_PATH, "0")
        updated = vdf.Document(CONFIG).delete_node(path)
        self.assertIsNone(vdf.Document(updated).find(path))
        self.assertIn('"CompatToolMapping"', updated)


class TransactionSteam(vet.Steam):
    def __init__(self, root, running=False, final_write=None):
        super().__init__(root)
        self.running_state = running
        self.final_write = final_write
        self.shutdowns = 0
        self.restarts = 0

    def running(self):
        return self.running_state

    def game_running(self, appid):
        return False

    def shutdown(self, timeout=30):
        self.shutdowns += 1
        if self.final_write:
            self.final_write()
        self.running_state = False

    def restart(self):
        self.restarts += 1


class SteamTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.local = self.root / "userdata/1/config/localconfig.vdf"
        self.config = self.root / "config/config.vdf"
        self.local.parent.mkdir(parents=True)
        self.config.parent.mkdir(parents=True)
        self.local.write_text(LOCALCONFIG)
        self.config.write_text(CONFIG)
        custom = self.root / "compatibilitytools.d/Proton 9"
        custom.mkdir(parents=True)
        (custom / "compatibilitytool.vdf").write_text(CUSTOM_PROTON)
        (custom / "proton").write_text("")

    def tearDown(self):
        self.temporary.cleanup()

    @property
    def expected(self):
        return {"launch_options": "MANGOHUD=1 %command% -novid", "proton": None}

    def test_launch_options_mutation(self):
        steam = TransactionSteam(self.root)
        result = steam.update(
            123, self.expected, "MANGOHUD=1 varmint-graphics neptune %command% -novid"
        )
        self.assertEqual(steam.launch_options(123), result["after"]["launch_options"])
        self.assertIsNone(steam.proton_selection(123))

    def test_compat_tool_mapping_mutation_and_removal(self):
        steam = TransactionSteam(self.root)
        steam.update(123, self.expected, proton="proton_9")
        self.assertEqual(steam.proton_selection(123), "proton_9")
        document = vdf.Document(self.config.read_text())
        mapping = (*vet.Steam.COMPAT_PATH, "123")
        self.assertEqual(document.value((*mapping, "config")), "")
        self.assertEqual(document.value((*mapping, "priority")), "250")
        steam.update(
            123,
            {"launch_options": self.expected["launch_options"], "proton": "proton_9"},
            proton=None,
        )
        self.assertIsNone(steam.proton_selection(123))
        self.assertEqual(steam.effective_proton(123), "proton_experimental")

    def test_combined_change_is_one_shutdown_restart(self):
        steam = TransactionSteam(self.root, running=True)
        steam.update(
            123,
            self.expected,
            "MANGOHUD=1 varmint-graphics venus %command% -novid",
            "proton_9",
            allow_restart=True,
        )
        self.assertEqual((steam.shutdowns, steam.restarts), (1, 1))
        self.assertEqual(steam.proton_selection(123), "proton_9")
        self.assertIn("varmint-graphics venus", steam.launch_options(123))

    def test_rereads_after_steam_final_write(self):
        def final_write():
            document = vdf.Document(self.local.read_text())
            self.local.write_text(
                document.set_value(
                    vet.Steam.APP_PATH + ("123", "LaunchOptions"), "%command% -changed"
                )
            )

        steam = TransactionSteam(self.root, running=True, final_write=final_write)
        with self.assertRaisesRegex(vet.VetError, "changed after confirmation"):
            steam.update(
                123,
                self.expected,
                "varmint-graphics neptune %command%",
                allow_restart=True,
            )
        self.assertEqual(steam.launch_options(123), "%command% -changed")
        self.assertEqual(steam.restarts, 1)

    def test_restarts_steam_when_configuration_read_fails_after_shutdown(self):
        steam = TransactionSteam(self.root, running=True)
        self.config.unlink()
        with self.assertRaisesRegex(vet.VetError, "configuration was not found"):
            steam.update(
                123,
                self.expected,
                "varmint-graphics neptune %command%",
                allow_restart=True,
            )
        self.assertEqual((steam.shutdowns, steam.restarts), (1, 1))

    def test_rolls_back_both_files_on_failure(self):
        steam = TransactionSteam(self.root)
        original_local = self.local.read_text()
        original_config = self.config.read_text()
        real_write = vet.atomic_write
        failed = False

        def write(path, data, mode=0o600):
            nonlocal failed
            if Path(path) == self.config and not failed:
                failed = True
                raise OSError("injected failure")
            return real_write(path, data, mode)

        with mock.patch.object(vet, "atomic_write", side_effect=write):
            with self.assertRaises(OSError):
                steam.update(
                    123, self.expected, "varmint-graphics neptune %command%", "proton_9"
                )
        self.assertEqual(self.local.read_text(), original_local)
        self.assertEqual(self.config.read_text(), original_config)

    def test_lists_only_fully_installed_games(self):
        steamapps = self.root / "steamapps"
        steamapps.mkdir()
        (steamapps / "appmanifest_123.acf").write_text(MANIFEST)
        (steamapps / "appmanifest_456.acf").write_text(
            MANIFEST.replace('"123"', '"456"')
            .replace('"Test Game"', '"Updating"')
            .replace('"4"', '"2"')
        )
        (steamapps / "appmanifest_789.acf").write_text(
            MANIFEST.replace('"123"', '"789"').replace(
                '"Test Game"', '"Proton Experimental"'
            )
        )
        self.assertEqual(
            [game["appid"] for game in TransactionSteam(self.root).installed_games()],
            [123],
        )

    def test_discovers_installed_official_and_custom_proton(self):
        steamapps = self.root / "steamapps"
        common = steamapps / "common/Proton 10.0"
        common.mkdir(parents=True)
        (common / "proton").write_text("")
        proton_manifest = MANIFEST.replace('"123"', '"3658110"').replace(
            '"Test Game"', '"Proton 10.0"\n\t"installdir"\t\t"Proton 10.0"'
        )
        (steamapps / "appmanifest_3658110.acf").write_text(proton_manifest)
        self.assertEqual(
            TransactionSteam(self.root).proton_tools(),
            [
                {"name": "proton_10", "display_name": "Proton 10.0"},
                {"name": "proton_9", "display_name": "Proton 9"},
            ],
        )

    def test_discovers_downloadable_proton_from_steam_catalog(self):
        appcache = self.root / "appcache"
        appcache.mkdir()
        (appcache / "appinfo.vdf").write_bytes(
            appinfo_v29(
                [
                    (4628710, "Proton 11.0", "Proton 11.0"),
                    (
                        1826330,
                        "Proton EasyAntiCheat Runtime",
                        "Proton EasyAntiCheat Runtime",
                    ),
                ]
            )
        )
        self.assertEqual(
            TransactionSteam(self.root).proton_tools(),
            [
                {"name": "proton_11", "display_name": "Proton 11.0"},
                {"name": "proton_9", "display_name": "Proton 9"},
            ],
        )

    def test_steam_registration_supplies_exact_key_and_filters_other_tools(self):
        appcache = self.root / "appcache"
        appcache.mkdir()
        (appcache / "appinfo.vdf").write_bytes(
            appinfo_v29(
                [
                    (4628740, "Proton 11.0 (ARM64)", "Proton 11.0 (ARM64)"),
                    (2230260, "Proton Next", "Proton Next"),
                    (1070560, "FEX", "FEX-Emu"),
                ]
            )
        )
        logs = self.root / "logs"
        logs.mkdir()
        (logs / "compat_log.txt").write_text(
            "Registering tool proton_11-arm64, AppID 4628740\n"
            "Registering tool fex, AppID 1070560\n"
        )
        self.assertEqual(
            TransactionSteam(self.root).proton_tools(),
            [
                {"name": "proton_11-arm64", "display_name": "Proton 11.0 (ARM64)"},
                {"name": "proton_9", "display_name": "Proton 9"},
            ],
        )

    def test_rejects_proton_missing_from_steam_catalog(self):
        with self.assertRaisesRegex(vet.VetError, "not available"):
            TransactionSteam(self.root).update(123, self.expected, proton="proton_10")

    def test_steam_default_is_proton_experimental_without_mapping(self):
        self.config.write_text(
            CONFIG.replace(
                '\t\t\t\t\t"0"\n\t\t\t\t\t{\n\t\t\t\t\t\t"name"\t\t"proton_experimental"\n\t\t\t\t\t}\n',
                "",
            )
        )
        self.assertEqual(
            TransactionSteam(self.root).effective_proton(123), "proton_experimental"
        )

    def test_configuration_revision_changes_with_steam_files(self):
        steam = TransactionSteam(self.root)
        before = steam.configuration_revision()
        vet.atomic_write(self.local, LOCALCONFIG.replace("MANGOHUD=1", "MANGOHUD=0"))
        self.assertNotEqual(steam.configuration_revision(), before)


class SteamProcessTests(unittest.TestCase):
    class ProcessSteam(vet.Steam):
        def __init__(self, processes):
            self.processes = processes

        def _processes(self):
            return iter((command, b"") for command in self.processes)

    def test_only_real_client_counts_as_running(self):
        noise = [
            ["/usr/local/bin/steam-edge-resize"],
            ["/home/user/.local/share/Steam/steamrtarm64/steamwebhelper"],
            ["/home/user/.local/share/Steam/steamrtarm64/steam", "-shutdown"],
        ]
        self.assertFalse(self.ProcessSteam(noise).running())
        self.assertTrue(
            self.ProcessSteam(
                noise + [["/home/user/.local/share/Steam/steamrtarm64/steam"]]
            ).running()
        )

    def test_restart_uses_separate_user_service(self):
        with mock.patch.object(vet.subprocess, "run") as run:
            vet.Steam().restart()
        self.assertEqual(
            run.call_args.args[0],
            [
                "systemd-run",
                "--user",
                "--collect",
                "--quiet",
                "/usr/local/bin/varmint-steam",
            ],
        )


class GraphicsDiagnosticTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.graphics = runpy.run_path(str(GUEST / "neptune/graphics.py"))

    def test_capture_bookkeeping_failure_does_not_change_game_exit_status(self):
        diagnostic = mock.Mock()
        diagnostic.environment.return_value = {"PROTON_LOG": "1"}
        diagnostic.complete.side_effect = OSError("read-only state")

        with mock.patch("builtins.print"):
            status = self.graphics["run_with_diagnostics"](
                ["game"],
                {},
                lambda _command, environment: (
                    7 if environment["PROTON_LOG"] == "1" else 1
                ),
                diagnostic,
            )

        self.assertEqual(status, 7)

    def test_capture_claim_failure_does_not_block_game_launch(self):
        function = self.graphics["claim_diagnostic"]
        with (
            mock.patch.dict(
                function.__globals__,
                {
                    "claim_from_environment": mock.Mock(
                        side_effect=OSError("read-only state")
                    )
                },
            ),
            mock.patch("builtins.print"),
        ):
            self.assertIsNone(function())


class UiFlowTests(unittest.TestCase):
    def setUp(self):
        self.plan = {
            "appid": 123,
            "before": {"launch_options": "%command%", "proton": None},
            "after": {
                "launch_options": "varmint-graphics neptune %command%",
                "proton": "proton_experimental",
            },
            "changes": {"launch_options": True, "proton": True},
        }

    def test_manual_setup_copies_options_and_opens_steam_properties(self):
        with (
            mock.patch.object(actions, "copy_to_clipboard") as copy,
            mock.patch.object(actions, "open_game_properties") as open_properties,
            mock.patch.object(actions, "notify") as notify,
        ):
            actions.configure_manually(self.plan)
        copy.assert_called_once_with("varmint-graphics neptune %command%")
        open_properties.assert_called_once_with(123)
        self.assertIn("Paste into General", notify.call_args.args[1])

    def test_notifications_expire(self):
        with (
            mock.patch.object(actions, "notification_icon", return_value="/icon.svg"),
            mock.patch.object(actions.subprocess, "run") as run,
        ):
            actions.notify("Title", "Message")
        self.assertIn("--expire-time=10000", run.call_args.args[0])
        self.assertIn("/icon.svg", run.call_args.args[0])

    def test_override_plan_keeps_unselected_fields(self):
        state = {"appid": 123, "launch_options": "%command%", "proton": None}
        plan = actions.override_plan(state, proton="proton_10")
        self.assertEqual(
            plan["after"], {"launch_options": "%command%", "proton": "proton_10"}
        )
        self.assertEqual(plan["changes"], {"launch_options": False, "proton": True})


class FakeHttp:
    def __init__(self, callback):
        self.callback = callback

    def fetch(self, url, limit, max_age):
        return self.callback(url).encode()


class ResearchTests(unittest.TestCase):
    def test_markdown_guide_is_split_into_prose_and_copyable_code(self):
        markdown = (
            "# Setup\n\nRun this:\n\n```bash\necho hello\necho world\n```\n\nDone."
        )
        self.assertEqual(
            vet.split_markdown_sections(markdown),
            [
                ("prose", "", "# Setup\n\nRun this:"),
                ("code", "bash", "echo hello\necho world"),
                ("prose", "", "Done."),
            ],
        )

    def test_setup_profile_metadata_is_not_presented_as_copyable_code(self):
        profile = """Profile: remastered-dx12
Recommendation: Recommended
Evidence: Tested
Setup: Assisted
Launch options: varmint-graphics neptune-dx12 %command% --launcher-skip"""
        self.assertTrue(vet.is_setup_profile_block(profile))
        self.assertFalse(vet.is_copyable_guide_code("text", profile))

    def test_commands_remain_copyable(self):
        self.assertTrue(
            vet.is_copyable_guide_code(
                "text", "varmint-graphics neptune-dx12 %command% --launcher-skip"
            )
        )
        self.assertTrue(vet.is_copyable_guide_code("bash", "echo hello"))

    def test_launch_requirements_use_only_structured_profile_values(self):
        self.assertEqual(
            vet.format_launch_requirements(
                ["--launcher-skip"], {"FEX_X87REDUCEDPRECISION": "0"}
            ),
            ("--launcher-skip", "FEX_X87REDUCEDPRECISION=0"),
        )

    def test_varmint_catalog_matches_exact_appid(self):
        provider = vet.VarmintDocsProvider(
            catalog_path=GUEST / "varmint-vet-catalog.json"
        )
        match, warnings = provider.find(548430, "A title Vet must not need to match")
        self.assertEqual((match.confidence, warnings), ("exact", []))
        self.assertEqual(match.facts.graphics_mode, "neptune")
        self.assertEqual(match.facts.proton, "proton_experimental")
        self.assertEqual(match.facts.launch_args, [])
        self.assertIn("# Running Deep Rock Galactic", match.facts.guide_markdown)

    def test_varmint_catalog_selects_witcher_profile_by_branch(self):
        provider = vet.VarmintDocsProvider(
            catalog_path=GUEST / "varmint-vet-catalog.json"
        )
        classic, _ = provider.find(292030, "The Witcher 3", "classic")
        remastered, _ = provider.find(292030, "The Witcher 3", "public")
        self.assertEqual(classic.facts.profile, "classic-neptune")
        self.assertEqual(classic.facts.graphics_mode, "neptune")
        self.assertEqual(remastered.facts.profile, "remastered-dx12")
        self.assertEqual(remastered.facts.recommendation, "recommended")
        self.assertEqual(remastered.facts.graphics_mode, "neptune-dx12")
        self.assertEqual(remastered.facts.host_backend, "D3DMetal")
        self.assertEqual(remastered.facts.proton, "proton_experimental")
        self.assertEqual(remastered.facts.environment, {})
        self.assertEqual(remastered.facts.launch_args, ["--launcher-skip"])

    def test_varmint_catalog_does_not_guess_an_unknown_game_branch(self):
        provider = vet.VarmintDocsProvider(
            catalog_path=GUEST / "varmint-vet-catalog.json"
        )
        match, warnings = provider.find(292030, "The Witcher 3", "future-preview")
        self.assertEqual((match.confidence, match.facts), ("exact", None))
        self.assertIn("no setup profile for game version future-preview", warnings[0])
        research = vet.Research(
            match, vet.ExternalFacts([11, 12], "x64", [], "high", []), warnings
        )
        recommendation = vet.DeterministicRecommendationProvider().recommend(
            {"appid": 292030, "name": "The Witcher 3"},
            {"launch_options": ""},
            research,
        )
        self.assertIsNone(recommendation.graphics_mode)
        self.assertEqual(recommendation.reason, "varmint_version_unmatched")

    def test_invalid_local_catalog_does_not_fall_back_to_remote(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "catalog.json"
            path.write_text('{"schema_version": 999}', encoding="utf-8")
            http = FakeHttp(lambda _url: self.fail("local mode contacted the network"))
            match, warnings = vet.VarmintDocsProvider(http, path).find(
                548430, "Deep Rock Galactic"
            )
        self.assertEqual(match.confidence, "none")
        self.assertIn("unsupported catalog schema", warnings[0])

    def test_incompatible_remote_catalog_falls_back_to_bundled_catalog(self):
        remote = json.dumps(
            {"schema_version": 999, "minimum_vet_version": 999, "games": []}
        )
        with (
            tempfile.TemporaryDirectory() as directory,
            mock.patch.object(vet, "STATE_ROOT", Path(directory)),
            mock.patch.object(
                vet.VarmintDocsProvider, "BUNDLED", GUEST / "varmint-vet-catalog.json"
            ),
        ):
            match, warnings = vet.VarmintDocsProvider(
                FakeHttp(lambda _url: remote)
            ).find(548430, "Deep Rock Galactic")
        self.assertEqual(match.confidence, "exact")
        self.assertIn("unsupported catalog schema", warnings[0])

    def test_pcgamingwiki_requires_exact_appid(self):
        matching = "|steam appid = 123\n|direct3d versions = 11, 12\n|windows 64-bit exe = true\n|direct3d notes = Use -dx11"

        def response(url):
            if "opensearch" in url:
                return json.dumps(["Test", ["Test Game"], [], []])
            return json.dumps({"parse": {"wikitext": matching}})

        facts, warnings = vet.PCGamingWikiProvider(FakeHttp(response)).find(
            123, "Test Game"
        )
        self.assertEqual(warnings, [])
        self.assertEqual(facts.confidence, "high")
        self.assertEqual(facts.direct3d_versions, [11, 12])
        self.assertEqual(facts.architecture, "x64")
        self.assertEqual(facts.api_selection[0]["value"], "-dx11")

        facts, warnings = vet.PCGamingWikiProvider(FakeHttp(response)).find(
            456, "Test Game"
        )
        self.assertEqual(facts.confidence, "none")
        self.assertIn("no exact AppID", warnings[0])

    def test_pcgamingwiki_removes_trademark_markers_from_search(self):
        urls = []

        def response(url):
            urls.append(url)
            if "opensearch" in url:
                return json.dumps(["Stellar Blade", [], [], []])
            raise AssertionError(url)

        vet.PCGamingWikiProvider(FakeHttp(response)).find(3489700, "Stellar Blade™")
        self.assertIn("search=Stellar+Blade&", urls[0])

    def test_pcgamingwiki_retries_a_shorter_title_with_the_same_appid(self):
        urls = []
        matching = "|steam appid = 292030\n|direct3d versions = 11, 12\n|windows 64-bit exe = true"

        def response(url):
            urls.append(url)
            if "opensearch" in url:
                titles = [] if "Remastered" in url else ["The Witcher 3: Wild Hunt"]
                return json.dumps(["Witcher", titles, [], []])
            return json.dumps({"parse": {"wikitext": matching}})

        facts, warnings = vet.PCGamingWikiProvider(FakeHttp(response)).find(
            292030, "The Witcher 3: Wild Hunt — Remastered"
        )
        self.assertEqual(warnings, [])
        self.assertEqual(facts.direct3d_versions, [11, 12])
        self.assertGreaterEqual(sum("opensearch" in url for url in urls), 2)

    def test_all_guides_compile_into_the_checked_in_catalog(self):
        compiled = catalog.compile_catalog(GUEST.parent / "docs" / "games")
        checked_in = json.loads(
            (GUEST / "varmint-vet-catalog.json").read_text(encoding="utf-8")
        )
        self.assertEqual(compiled, checked_in)
        self.assertEqual(len(compiled["games"]), 41)
        profiles = [
            profile for game in compiled["games"] for profile in game["profiles"]
        ]
        self.assertEqual(len(profiles), 49)
        self.assertEqual(sum(profile["setup"] == "manual" for profile in profiles), 7)
        self.assertEqual(sum(profile["setup"] == "assisted" for profile in profiles), 6)
        self.assertEqual(
            sum(profile["evidence"] == "reported" for profile in profiles), 3
        )

    def test_remote_catalog_rejects_an_incompatible_host_backend(self):
        value = json.loads(
            (GUEST / "varmint-vet-catalog.json").read_text(encoding="utf-8")
        )
        profile = next(
            profile
            for game in value["games"]
            for profile in game["profiles"]
            if profile.get("graphics_mode") == "neptune-dx12"
        )
        profile["host_backend"] = "DXMT"
        with self.assertRaisesRegex(
            catalog.CatalogError, "Renderer and Host backend disagree"
        ):
            catalog.validate_catalog(value)

    def test_profiles_use_the_expected_host_graphics_backend(self):
        compiled = catalog.compile_catalog(GUEST.parent / "docs" / "games")
        profiles = [
            profile for game in compiled["games"] for profile in game["profiles"]
        ]
        for profile in profiles:
            with self.subTest(profile=profile["id"]):
                if profile.get("graphics_mode") == "neptune":
                    self.assertEqual(profile["host_backend"], "DXMT")
                elif profile.get("graphics_mode") == "neptune-dx12":
                    self.assertEqual(profile["host_backend"], "D3DMetal")
                elif (
                    profile.get("runtime") == "native linux"
                    or profile.get("graphics_mode") == "venus"
                ):
                    self.assertEqual(profile["host_backend"], "MoltenVK")

    def test_new_dx12_profiles_have_the_declared_priority_and_proton(self):
        compiled = catalog.compile_catalog(GUEST.parent / "docs" / "games")
        by_title = {game["title"]: game for game in compiled["games"]}
        expected = {
            "Deep Rock Galactic": ("alternative", "proton_experimental"),
            "Grounded": ("alternative", "proton_10"),
            'Resident Evil 2 "R.P.D. Demo"': ("alternative", "proton_experimental"),
            "Ghostrunner Demo": ("recommended", "proton_experimental"),
            "Stellar Blade Demo": ("recommended", "proton-experimental-arm64"),
            "Black Myth: Wukong Benchmark Tool": (
                "recommended",
                "proton-experimental-arm64",
            ),
            "The Witcher 3": ("recommended", "proton_experimental"),
        }
        for title, result in expected.items():
            profiles = [
                profile
                for profile in by_title[title]["profiles"]
                if profile.get("graphics_mode") == "neptune-dx12"
            ]
            with self.subTest(game=title):
                self.assertEqual(len(profiles), 1)
                self.assertEqual(
                    (profiles[0]["recommendation"], profiles[0]["proton"]), result
                )

    def test_remote_shell_text_requires_a_manual_profile(self):
        template = """# Running Shell Test in Varmint

```text
Steam App IDs: 123
Profile: shell-test
Recommendation: Recommended
Evidence: Tested
Setup: {setup}
Runtime: Proton
Proton version: Proton 10
Renderer: Venus
Host backend: MoltenVK
Launch options: bash -c 'touch /tmp/not-allowed' -- %command%
```
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "shell-test.md"
            path.write_text(template.format(setup="Automatic"), encoding="utf-8")
            with self.assertRaisesRegex(catalog.CatalogError, "Setup: Manual"):
                catalog.compile_guide(path)
            path.write_text(template.format(setup="Manual"), encoding="utf-8")
            profile = catalog.compile_guide(path)["profiles"][0]
        self.assertEqual(profile["setup"], "manual")
        self.assertEqual(profile["launch_args"], [])

    def test_steam_store_resolves_demo_to_full_game(self):
        response = {
            "3564860": {
                "success": True,
                "data": {
                    "type": "demo",
                    "name": "Stellar Blade Demo",
                    "fullgame": {"appid": "3489700", "name": "Stellar Blade"},
                },
            },
        }
        parent = vet.SteamStoreProvider(
            FakeHttp(lambda _url: json.dumps(response))
        ).parent_game(3564860)
        self.assertEqual((parent.appid, parent.name), (3489700, "Stellar Blade"))

    def test_research_falls_back_to_full_game_appid(self):
        parent = vet.ParentGame(
            3489700, "Stellar Blade", "https://store.steampowered.com/app/3489700"
        )

        class Docs:
            def find(self, _appid, _name, _game_version=None):
                return vet.GuideMatch(), []

        class Pcgw:
            def __init__(self):
                self.calls = []

            def find(self, appid, name):
                self.calls.append((appid, name))
                if appid == 3489700:
                    source = vet.Source(
                        "pcgamingwiki",
                        "https://example.invalid/stellar",
                        "high",
                        "PCGamingWiki: Stellar Blade",
                    )
                    return vet.ExternalFacts([12], "x64", [], "high", [source]), []
                return vet.ExternalFacts(), ["no exact demo AppID"]

        class Store:
            def parent_game(self, _appid):
                return parent

        pcgw = Pcgw()
        research = vet.GameResearchProvider(Docs(), pcgw, Store()).research(
            3564860, "Stellar Blade Demo"
        )
        self.assertEqual(
            pcgw.calls, [(3564860, "Stellar Blade Demo"), (3489700, "Stellar Blade")]
        )
        self.assertEqual(research.parent_game, parent)
        self.assertTrue(research.external_from_parent)
        self.assertEqual(research.external.direct3d_versions, [12])
        self.assertNotIn("no exact demo AppID", research.warnings)

        state = {"launch_options": "%command%"}
        recommendation = vet.DeterministicRecommendationProvider().recommend(
            {"appid": 3564860, "name": "Stellar Blade Demo"}, state, research
        )
        self.assertEqual(recommendation.graphics_mode, "neptune-dx12")
        self.assertIn("full game", recommendation.message)


def external(versions, architecture="x64", api_selection=None, confidence="high"):
    return vet.Research(
        vet.GuideMatch(),
        vet.ExternalFacts(versions, architecture, api_selection or [], confidence, []),
        [],
    )


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.provider = vet.DeterministicRecommendationProvider()
        self.game = {"appid": 123, "name": "Test Game"}
        self.state = {"launch_options": "%command% -novid"}

    def mode(self, research):
        return self.provider.recommend(self.game, self.state, research).graphics_mode

    def test_mandatory_d3d_policy(self):
        cases = (
            ([9], "x86", "venus"),
            ([11], "x86", "venus"),
            ([11], "x64", "neptune"),
            ([12], "unknown", "neptune-dx12"),
            ([11, 12], "x64", "neptune"),
            ([11], "unknown", None),
            ([], "x64", None),
        )
        for versions, architecture, expected in cases:
            with self.subTest(versions=versions, architecture=architecture):
                self.assertEqual(self.mode(external(versions, architecture)), expected)

    def test_exact_appid_evidence_wins_over_an_ambiguous_guide_name(self):
        research = external([11, 12])
        research.varmint_doc = vet.GuideMatch("ambiguous")
        recommendation = self.provider.recommend(self.game, self.state, research)
        self.assertEqual(recommendation.graphics_mode, "neptune")
        self.assertEqual(recommendation.reason, "multiple_apis_prefer_d3d11")

    def test_failed_research_does_not_repeat_warnings_as_evidence(self):
        research = vet.Research(
            vet.GuideMatch(), vet.ExternalFacts(), ["lookup failed"]
        )
        recommendation = self.provider.recommend(self.game, self.state, research)
        self.assertEqual(recommendation.details, "")

    def test_arm64_proton_names_are_human_readable(self):
        self.assertEqual(
            vet.display_proton("proton-experimental-arm64"),
            "Proton Experimental (ARM64)",
        )

    def test_supported_api_does_not_invent_launch_argument(self):
        recommendation = self.provider.recommend(
            self.game, self.state, external([11], "x64")
        )
        self.assertEqual(recommendation.append_args, [])

    def test_generic_policy_names_the_host_backend(self):
        cases = (
            ([9], "x86", "MoltenVK", "automatic"),
            ([11], "x64", "DXMT", "automatic"),
            ([12], "x64", "D3DMetal", "automatic"),
        )
        for versions, architecture, backend, setup in cases:
            with self.subTest(versions=versions):
                recommendation = self.provider.recommend(
                    self.game, self.state, external(versions, architecture)
                )
                self.assertEqual(
                    (recommendation.host_backend, recommendation.setup),
                    (backend, setup),
                )

    def test_explicit_source_argument_is_preserved(self):
        selection = [{"api": "d3d11", "kind": "argument", "value": "-dx11"}]
        recommendation = self.provider.recommend(
            self.game, self.state, external([11, 12], "x64", selection)
        )
        self.assertEqual(recommendation.append_args, ["-dx11"])

    def test_varmint_guide_overrides_generic_policy(self):
        facts = vet.GuideFacts(
            "proton_9", "venus", [], {}, "https://example.invalid/guide"
        )
        pcgw = vet.Source(
            "pcgamingwiki",
            "https://example.invalid/pcgw",
            "high",
            "PCGamingWiki: Test Game",
        )
        research = vet.Research(
            vet.GuideMatch("exact", "docs/games/test.md", facts),
            vet.ExternalFacts([12], "x64", [], "high", [pcgw]),
            [],
        )
        recommendation = self.provider.recommend(self.game, self.state, research)
        self.assertEqual(
            (recommendation.graphics_mode, recommendation.proton), ("venus", "proton_9")
        )
        self.assertEqual(
            [source.kind for source in recommendation.sources],
            ["varmint_guide", "pcgamingwiki"],
        )
        self.assertEqual(recommendation.details, "")


class FakeInstalledSteam:
    def __init__(self, games):
        self.games = games

    def installed_games(self):
        return self.games


class InstallMonitorTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.path = Path(self.temporary.name) / "installed.json"
        self.games = [{"appid": 123, "name": "Existing", "manifest_time": 1}]
        self.monitor = vet.InstallMonitor(FakeInstalledSteam(self.games), self.path)

    def tearDown(self):
        self.temporary.cleanup()

    def test_baselines_existing_games_and_requires_stability(self):
        self.assertEqual(self.monitor.poll(), [])
        self.games.append({"appid": 456, "name": "New", "manifest_time": 2})
        self.assertEqual(self.monitor.poll(), [])
        self.assertEqual(self.monitor.poll(), [{"appid": 456, "name": "New"}])
        self.assertEqual(self.monitor.poll(), [])

    def test_update_does_not_create_install_event(self):
        self.monitor.poll()
        self.games[0]["manifest_time"] = 99
        self.assertEqual(self.monitor.poll(), [])


class MonitorFlowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.launcher = runpy.run_path(str(GUEST / "varmint-vet"))

    def recommendation(
        self, graphics="neptune", proton=None, changed=True, host_warning=None
    ):
        recommendation = SimpleNamespace(graphics_mode=graphics, proton=proton)
        plan = {
            "changes": {"launch_options": changed, "proton": False},
            "host_warning": host_warning,
        }
        service = mock.Mock()
        service.prepare.return_value = (None, None, None, recommendation, plan)
        return service

    def call_notification(self, service, *, suggestions_enabled=True, output=""):
        notification = mock.Mock(return_value=(output, 0))
        preferences = SimpleNamespace(suggestions_enabled=lambda: suggestions_enabled)
        function = self.launcher["notify_recommendation"]
        with mock.patch.dict(
            function.__globals__,
            {
                "Preferences": lambda: preferences,
                "notification": notification,
            },
        ):
            function(service, {"appid": 123, "name": "Test Game"})
        return notification

    def test_actionable_change_shows_notification(self):
        notification = self.call_notification(self.recommendation())
        notification.assert_called_once()

    def test_paused_suggestions_skip_research_and_notification(self):
        service = self.recommendation()
        notification = self.call_notification(service, suggestions_enabled=False)
        service.prepare.assert_not_called()
        notification.assert_not_called()

    def test_missing_recommendation_stays_quiet(self):
        notification = self.call_notification(
            self.recommendation(graphics=None, proton=None)
        )
        notification.assert_not_called()

    def test_already_configured_game_stays_quiet(self):
        notification = self.call_notification(self.recommendation(changed=False))
        notification.assert_not_called()

    def test_host_backend_mismatch_shows_notification(self):
        notification = self.call_notification(
            self.recommendation(changed=False, host_warning="DX12 needs D3DMetal")
        )
        self.assertIn("DX12 needs D3DMetal", notification.call_args.args[0])

    def test_running_game_defers_install_poll(self):
        steam = mock.Mock()
        steam.any_game_running.return_value = True
        monitor_state = mock.Mock()
        function = self.launcher["monitor_once"]
        function(steam, mock.Mock(), monitor_state)
        monitor_state.poll.assert_not_called()

    def test_view_opens_vet_on_the_installed_game(self):
        run = mock.Mock()
        module = SimpleNamespace(run=run)
        with mock.patch.dict(sys.modules, {"varmint_vet_qt": module}):
            self.call_notification(self.recommendation(), output="view")
        run.assert_called_once()
        self.assertEqual(run.call_args.args[1], 123)

    def test_completed_diagnostic_opens_its_report(self):
        run = mock.Mock()
        module = SimpleNamespace(run=run)
        service = mock.Mock()
        service.diagnostics.completed_unnotified.return_value = [
            {
                "request": {"appid": 123, "game": "Test Game", "run_id": "run-id"},
            }
        ]
        function = self.launcher["notify_completed_diagnostics"]
        with (
            mock.patch.dict(sys.modules, {"varmint_vet_qt": module}),
            mock.patch.dict(
                function.__globals__,
                {
                    "notification": mock.Mock(return_value=("view", 0)),
                },
            ),
        ):
            function(service)
        service.diagnostics.mark_notified.assert_called_once_with("run-id")
        run.assert_called_once_with(service, 123, "run-id")


class DiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.store = diagnostics.DiagnosticStore(Path(self.temporary.name))
        self.state = {
            "appid": 123,
            "name": "Test Game",
            "graphics_mode": "neptune",
            "effective_proton": "proton_experimental",
            "launch_options": "varmint-graphics neptune %command%",
            "build_id": "100",
        }

    def tearDown(self):
        self.temporary.cleanup()

    def test_capture_is_claimed_once_and_completed(self):
        request = self.store.arm(self.state)
        self.assertEqual(self.store.pending(123)["run_id"], request["run_id"])
        claim = self.store.claim(123)
        self.assertIsNotNone(claim)
        self.assertIsNone(self.store.claim(123))
        self.assertEqual(claim.environment()["PROTON_LOG"], "1")
        self.assertEqual(claim.environment()["PROTON_LOG_DIR"], str(claim.directory))
        claim.complete(7)
        run = self.store.run(request["run_id"])
        self.assertEqual(run["completed"]["exit_code"], 7)
        self.assertEqual(run["request"]["build_id"], "100")

    def test_status_distinguishes_current_and_changed_setup(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        claim.complete(0)
        steam = mock.Mock()
        steam.launch_options.return_value = self.state["launch_options"]
        steam.manifest.return_value = {
            "name": "Test Game",
            "installed": True,
            "build_id": "100",
        }
        steam.proton_selection.return_value = None
        steam.effective_proton.return_value = "proton_experimental"
        service = vet.VetService(steam=steam, diagnostics=self.store)

        current = service.diagnostic_status(123)
        self.assertEqual(current["latest_run_id"], request["run_id"])
        self.assertEqual(current["latest_setup_changes"], [])
        self.assertIsNotNone(current["latest_completed_at"])

        steam.launch_options.return_value = "varmint-graphics neptune-dx12 %command%"
        changed = service.diagnostic_status(123)
        self.assertEqual(
            changed["latest_setup_changes"], ["graphics backend", "Launch Options"]
        )

    def test_capture_records_the_host_backend(self):
        state = {**self.state, "effective_host_backend": "D3DMetal"}
        request = self.store.arm(state)
        self.assertEqual(request["host_backend"], "D3DMetal")

    def test_status_marks_game_update_as_a_setup_change(self):
        self.store.arm(self.state)
        claim = self.store.claim(123)
        claim.complete(0)
        steam = mock.Mock()
        steam.launch_options.return_value = self.state["launch_options"]
        steam.manifest.return_value = {
            "name": "Test Game",
            "installed": True,
            "build_id": "101",
        }
        steam.proton_selection.return_value = None
        steam.effective_proton.return_value = "proton_experimental"
        service = vet.VetService(steam=steam, diagnostics=self.store)
        self.assertEqual(
            service.diagnostic_status(123)["latest_setup_changes"], ["game build"]
        )

    def test_legacy_capture_reports_missing_build_identity(self):
        request = {
            "graphics_mode": self.state["graphics_mode"],
            "proton": self.state["effective_proton"],
            "launch_options": self.state["launch_options"],
        }
        self.assertEqual(
            vet.diagnostic_setup_changes(request, self.state),
            ["game build was not recorded"],
        )

    def test_small_report_contains_the_complete_log_and_is_redacted(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        log = claim.directory / "steam-123.log"
        log.write_text(
            "header /home/alice/game\n"
            "err: repeated 0x12345678\n"
            "err: repeated 0x87654321\n"
            "warn: ordinary\n"
            "fatal: renderer failed\n",
            encoding="utf-8",
        )
        claim.complete(1)
        report = self.store.report(request["run_id"])
        self.assertIn("# Varmint compatibility report", report)
        self.assertIn("Exit status: 1", report)
        self.assertNotIn("/home/alice", report)
        self.assertIn("--- complete log: steam-123.log ---", report)
        self.assertIn("warn: ordinary", report)

    def test_large_report_keeps_middle_context_and_deduplicates_signals(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        log = claim.directory / "steam-123.log"
        prefix = "".join(f"ordinary prefix {number:05d}\n" for number in range(8000))
        suffix = "".join(f"ordinary suffix {number:05d}\n" for number in range(8000))
        log.write_text(
            prefix
            + "context before one\ncontext before two\n"
            + "err: device failed 0x12345678\n"
            + "context after one\ncontext after two\n"
            + "err: device failed 0x87654321\n"
            + suffix,
            encoding="utf-8",
        )
        claim.complete(1)
        with mock.patch.object(diagnostics, "FULL_LOG_LIMIT", 1024):
            report = self.store.report(request["run_id"])
        self.assertIn("--- relevant excerpts with context ---", report)
        self.assertIn("context before two", report)
        self.assertIn("context after two", report)
        relevant = report.split("--- relevant excerpts with context ---", 1)[1].split(
            "--- log end (last 30 lines) ---", 1
        )[0]
        self.assertEqual(relevant.count("err: device failed"), 1)

    def test_report_includes_multi_megabyte_log_in_full(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        log = claim.directory / "steam-123.log"
        marker = "middle failure that must not depend on parser coverage"
        log.write_text(
            "ordinary startup line\n" * 70000
            + marker
            + "\n"
            + "ordinary shutdown line\n" * 70000,
            encoding="utf-8",
        )
        self.assertGreater(log.stat().st_size, 2 * 1024 * 1024)
        claim.complete(1)
        report = self.store.report(request["run_id"])
        self.assertIn("complete chronological log", report)
        self.assertIn(marker, report)
        self.assertNotIn("relevant excerpts with context", report)

    def test_late_severe_signal_displaces_early_noise(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        log = claim.directory / "steam-123.log"
        early = "".join(f"err: startup noise {number}\n" for number in range(40))
        filler = "".join(
            f"ordinary line {number:05d} padding padding\n" for number in range(9000)
        )
        log.write_text(
            early
            + "Load module d3d11.dll\n"
            + filler
            + "context before terminal event\n"
            + "warn:seh:dispatch_exception backtrace: --- Exception 0xc06d007e.\n"
            + "context after terminal event\n",
            encoding="utf-8",
        )
        claim.complete(-2)
        report = self.store.report(request["run_id"])
        self.assertIn("0xc06d007e", report)
        self.assertIn("MSVC delay-load failure", report)
        self.assertIn(
            "A d3d11.dll load was observed; no d3d12.dll load was observed.", report
        )
        self.assertIn("loading d3d11.dll does not prove", report)
        self.assertIn("launch wrapper ended from signal 2", report)
        self.assertIn("may be wrong", report)
        self.assertIn("context before terminal event", report)

    def test_report_separates_detected_exceptions_from_possible_explanations(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        (claim.directory / "steam-123.log").write_text(
            "warn:seh:dispatch_exception backtrace: --- Exception 0xc0000005.\n"
            "Unhandled exception code c0000409\n",
            encoding="utf-8",
        )
        claim.complete(0)
        report = self.store.report(request["run_id"])
        detected = report.split("### Detected", 1)[1].split(
            "### Possible explanations", 1
        )[0]
        explanations = report.split("### Possible explanations", 1)[1].split(
            "## Relevant Proton log", 1
        )[0]
        self.assertIn("0xc0000005 (access violation)", detected)
        self.assertIn("0xc0000409 (fail-fast)", detected)
        self.assertNotIn("underlying cause", detected)
        self.assertIn("does not identify the underlying cause", explanations)
        self.assertIn("may be wrong", explanations)

    def test_completed_notification_is_one_shot(self):
        request = self.store.arm(self.state)
        claim = self.store.claim(123)
        claim.complete(0)
        self.assertEqual(len(self.store.completed_unnotified()), 1)
        self.store.mark_notified(request["run_id"])
        self.assertEqual(self.store.completed_unnotified(), [])

    def test_service_requires_existing_graphics_wrapper(self):
        steam = mock.Mock()
        steam.launch_options.return_value = "%command%"
        steam.manifest.return_value = {"name": "Test Game", "installed": True}
        steam.proton_selection.return_value = None
        steam.effective_proton.return_value = "proton_experimental"
        service = vet.VetService(steam=steam, diagnostics=self.store)
        with self.assertRaisesRegex(vet.VetError, "Choose a Varmint graphics backend"):
            service.arm_diagnostics(123)

    def test_service_arms_managed_game(self):
        steam = mock.Mock()
        steam.launch_options.return_value = "varmint-graphics neptune %command%"
        steam.manifest.return_value = {"name": "Test Game", "installed": True}
        steam.proton_selection.return_value = None
        steam.effective_proton.return_value = "proton_experimental"
        steam.game_running.return_value = False
        with mock.patch.object(vet, "append_history"):
            request = vet.VetService(
                steam=steam, diagnostics=self.store
            ).arm_diagnostics(123)
        self.assertEqual(request["appid"], 123)


class StateOwnershipTests(unittest.TestCase):
    class FakeSteam:
        def __init__(self):
            self.options = "varmint-graphics venus %command%"

        def launch_options(self, appid):
            return self.options

        def manifest(self, appid):
            return {"name": "Test", "installed": True}

        def proton_selection(self, appid):
            return None

        def effective_proton(self, appid):
            return "proton_experimental"

    def test_each_read_uses_current_steam_state(self):
        steam = self.FakeSteam()
        service = vet.VetService(steam=steam)
        self.assertEqual(service.game_state(123)["graphics_mode"], "venus")
        steam.options = "varmint-graphics neptune %command%"
        self.assertEqual(service.game_state(123)["graphics_mode"], "neptune")

    def test_kernel_cmdline_reports_the_last_authoritative_backend(self):
        cmdline = "varmint.neptune_backend=dxmt quiet varmint.neptune_backend=d3dmetal"
        self.assertEqual(vet.kernel_neptune_backend(cmdline), "D3DMetal")
        self.assertIsNone(vet.kernel_neptune_backend("quiet splash"))

    def test_effective_host_backend_uses_moltenvk_for_venus(self):
        self.assertEqual(vet.effective_host_backend("venus", "D3DMetal"), "MoltenVK")
        self.assertEqual(vet.effective_host_backend("neptune", "D3DMetal"), "D3DMetal")

    def test_plan_warns_when_dx12_needs_a_different_host_backend(self):
        steam = self.FakeSteam()
        service = vet.VetService(steam=steam)
        with mock.patch.object(vet, "kernel_neptune_backend", return_value="DXMT"):
            state = service.game_state(123)
        recommendation = vet.Recommendation(
            "neptune-dx12",
            None,
            [],
            [],
            {},
            "test",
            "high",
            "",
            "",
            [],
            host_backend="D3DMetal",
        )
        plan = service.plan(state, recommendation)
        self.assertIn("DX12 needs D3DMetal", plan["host_warning"])

    def test_current_dx12_setup_warns_immediately_when_vm_uses_dxmt(self):
        state = {
            "effective_graphics_mode": "neptune-dx12",
            "host_neptune_backend": "DXMT",
        }
        self.assertIn("needs D3DMetal", vet.current_host_backend_warning(state))
        state["host_neptune_backend"] = "D3DMetal"
        self.assertIsNone(vet.current_host_backend_warning(state))

    def test_dx11_setup_does_not_show_an_immediate_host_warning(self):
        state = {
            "effective_graphics_mode": "neptune",
            "host_neptune_backend": "D3DMetal",
        }
        self.assertIsNone(vet.current_host_backend_warning(state))

    def test_dx11_recommendation_accepts_d3dmetal_without_a_warning(self):
        state = {
            "effective_graphics_mode": "neptune",
            "host_neptune_backend": "D3DMetal",
        }
        recommendation = vet.Recommendation(
            "neptune",
            None,
            [],
            [],
            {},
            "test",
            "high",
            "",
            "",
            [],
            host_backend="DXMT",
        )
        self.assertIsNone(vet.host_backend_warning(state, recommendation))

    def test_empty_launch_options_use_venus_as_the_effective_backend(self):
        steam = self.FakeSteam()
        steam.options = ""
        state = vet.VetService(steam=steam).game_state(123)
        self.assertIsNone(state["graphics_mode"])
        self.assertEqual(state["effective_graphics_mode"], "venus")
        self.assertEqual(state["effective_host_backend"], "MoltenVK")

    def test_recommending_effective_venus_does_not_add_a_wrapper(self):
        steam = self.FakeSteam()
        steam.options = ""
        service = vet.VetService(steam=steam)
        state = service.game_state(123)
        recommendation = vet.Recommendation(
            "venus", None, [], [], {}, "test", "high", "", "", []
        )
        plan = service.plan(state, recommendation)
        self.assertFalse(plan["changes"]["launch_options"])
        self.assertEqual(plan["after"]["launch_options"], "")

    def test_venus_recommendation_keeps_required_launch_arguments(self):
        steam = self.FakeSteam()
        steam.options = ""
        service = vet.VetService(steam=steam)
        state = service.game_state(123)
        recommendation = vet.Recommendation(
            "venus", None, [], ["-dx11"], {}, "test", "high", "", "", []
        )
        plan = service.plan(state, recommendation)
        self.assertTrue(plan["changes"]["launch_options"])
        self.assertEqual(
            plan["after"]["launch_options"], "varmint-graphics venus %command% -dx11"
        )

    def test_malformed_options_remain_visible_as_unrecognized_state(self):
        steam = self.FakeSteam()
        steam.options = "varmint-graphics neptune-1 %command%"
        state = vet.VetService(steam=steam).game_state(123)
        self.assertEqual(state["launch_options"], steam.options)
        self.assertIsNone(state["graphics_mode"])
        self.assertIsNone(state["effective_graphics_mode"])
        self.assertIn("unsupported Varmint mode", state["launch_options_error"])

    def test_persistent_state_has_no_per_game_policy_file(self):
        self.assertNotIn("policy", vet.InstallMonitor.__dict__)
        self.assertNotIn("managed_by_varmint", Path(vet.__file__).read_text())

    def test_default_state_is_owned_by_the_user(self):
        self.assertEqual(vet.STATE_ROOT, Path.home() / ".local/state/varmint/vet")


class PreferencesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.path = Path(self.temporary.name) / "preferences.json"
        self.preferences = vet.Preferences(self.path)

    def tearDown(self):
        self.temporary.cleanup()

    def test_suggestions_are_enabled_by_default(self):
        self.assertTrue(self.preferences.suggestions_enabled())

    def test_suggestions_can_be_paused_and_resumed(self):
        self.preferences.set_suggestions_enabled(False)
        self.assertFalse(self.preferences.suggestions_enabled())
        self.preferences.set_suggestions_enabled(True)
        self.assertTrue(self.preferences.suggestions_enabled())

    def test_writing_new_preference_removes_legacy_per_game_ignores(self):
        self.path.write_text('{"ignored_appids": [123]}', encoding="utf-8")
        self.preferences.set_suggestions_enabled(True)
        self.assertEqual(
            json.loads(self.path.read_text(encoding="utf-8")),
            {"automatic_setup_suggestions": True},
        )


if __name__ == "__main__":
    unittest.main()

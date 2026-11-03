"""Deterministic guest-side Vet research and Steam configuration."""

from dataclasses import dataclass, field
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import tempfile
import time
import unicodedata
from urllib.parse import quote, urlencode, urlparse
from urllib.request import Request, urlopen
import uuid

from varmint_vdf import Document, VdfError, read_appinfo
from varmint_vet_catalog import CatalogError, validate_catalog
from varmint_vet_diagnostics import DiagnosticError, DiagnosticStore


GRAPHICS_MODES = {"venus", "neptune", "neptune-dx12"}
# Varmint's Steam client uses Proton Experimental when no compatibility
# mapping overrides it for a game.
STEAM_DEFAULT_PROTON = "proton_experimental"
GRAPHICS_ARGUMENTS = {"-dx11", "-d3d11", "-dx12", "-d3d12"}
ALLOWED_ENVIRONMENT = {"FEX_X87REDUCEDPRECISION"}
UNCHANGED = object()
STATE_ROOT = Path(
    os.environ.get(
        "VARMINT_VET_STATE_DIR",
        Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
        / "varmint/vet",
    )
)
STEAM_ROOT = Path(
    os.environ.get("VARMINT_STEAM_ROOT", str(Path.home() / ".local/share/Steam"))
)
STEAM_TOOL_NAMES = (
    re.compile(r"^Proton(?: |$)"),
    re.compile(r"^Steam Linux Runtime(?: |$)"),
    re.compile(r"^Steamworks Common Redistributables$"),
    re.compile(r"^FEX$"),
    re.compile(r"AntiCheat Runtime$"),
)


def official_proton_key(name):
    arm64 = name.endswith(" (ARM64)")
    base = name[:-8] if arm64 else name
    if base == "Proton Experimental":
        return "proton-experimental-arm64" if arm64 else "proton_experimental"
    if base == "Proton Hotfix":
        return "proton-hotfix-arm64" if arm64 else "proton_hotfix"
    match = re.fullmatch(r"Proton (\d+)(?:\.(\d+))?(?: \(Beta\))?", base)
    if not match:
        return None
    major = int(match.group(1))
    minor = match.group(2)
    if arm64:
        return f"proton_{major}-arm64"
    if major >= 7 or minor == "0":
        return f"proton_{major}"
    return f"proton_{major}{minor or ''}"


def proton_tool_sort_key(tool):
    name = tool["display_name"]
    arm64 = name.endswith(" (ARM64)")
    base = name[:-8] if arm64 else name
    if base == "Proton Experimental":
        return 0, 0, 0, arm64, ""
    if base == "Proton Hotfix":
        return 1, 0, 0, arm64, ""
    match = re.fullmatch(r"Proton (\d+)(?:\.(\d+))?(?: \(Beta\))?", base)
    if match:
        return 2, -int(match.group(1)), -int(match.group(2) or 0), arm64, ""
    return 3, 0, 0, False, name.casefold()


class VetError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def now():
    return datetime.now(timezone.utc).isoformat()


def validate_appid(value):
    if (
        isinstance(value, bool)
        or not isinstance(value, int)
        or value <= 0
        or value > 0xFFFFFFFF
    ):
        raise VetError("invalid_appid", "AppID must be a positive 32-bit integer")
    return value


def kernel_neptune_backend(cmdline=None):
    if cmdline is None:
        try:
            cmdline = Path("/proc/cmdline").read_text(encoding="utf-8")
        except OSError:
            return None
    backend = None
    for token in cmdline.split():
        if token.startswith("varmint.neptune_backend="):
            backend = token.split("=", 1)[1]
    return {"dxmt": "DXMT", "d3dmetal": "D3DMetal"}.get(backend)


def effective_host_backend(graphics_mode, neptune_backend):
    if graphics_mode == "venus":
        return "MoltenVK"
    if graphics_mode in {"neptune", "neptune-dx12"}:
        return neptune_backend
    return None


def atomic_write(path, data, mode=0o600):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    except Exception:
        try:
            os.close(fd)
        except OSError:
            pass
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def _launch_tokens(options):
    tokens = []
    start = None
    quote_char = None
    escaped = False
    for index, char in enumerate(options):
        if escaped:
            escaped = False
            continue
        if char == "\\":
            escaped = True
            if start is None:
                start = index
            continue
        if quote_char:
            if char == quote_char:
                quote_char = None
            continue
        if char in "'\"":
            quote_char = char
            if start is None:
                start = index
            continue
        if char in "\r\n":
            raise VetError(
                "unsafe_launch_options", "Steam LaunchOptions contain a line break"
            )
        if char in ";|&<>`(){}#" or options.startswith("$(", index):
            raise VetError(
                "unsafe_launch_options",
                "Steam LaunchOptions contain shell control syntax",
            )
        if char.isspace():
            if start is not None:
                tokens.append(options[start:index])
                start = None
        elif start is None:
            start = index
    if escaped or quote_char:
        raise VetError(
            "unsafe_launch_options", "Steam LaunchOptions contain malformed quoting"
        )
    if start is not None:
        tokens.append(options[start:])
    return tokens


def _wrapper(tokens):
    command = [i for i, token in enumerate(tokens) if token == "%command%"]
    if len(command) > 1 or any(
        "%command%" in token and token != "%command%" for token in tokens
    ):
        raise VetError(
            "unsafe_launch_options",
            "Steam LaunchOptions contain an ambiguous %command%",
        )
    wrappers = [
        i
        for i, token in enumerate(tokens)
        if token.rsplit("/", 1)[-1] == "varmint-graphics"
    ]
    if not wrappers and any("varmint-graphics" in token for token in tokens):
        raise VetError(
            "unsafe_launch_options",
            "Steam LaunchOptions contain an unrecognized Varmint wrapper",
        )
    if wrappers:
        index = wrappers[0]
        if len(wrappers) != 1 or len(command) != 1 or index + 2 != command[0]:
            raise VetError(
                "unsafe_launch_options",
                "Steam LaunchOptions contain an unrecognized Varmint wrapper",
            )
        if index + 1 >= len(tokens) or tokens[index + 1] not in GRAPHICS_MODES:
            raise VetError(
                "unsafe_launch_options",
                "Steam LaunchOptions contain an unsupported Varmint mode",
            )
        return index, command[0], tokens[index + 1]
    return None, command[0] if command else None, None


def launch_state(options):
    tokens = _launch_tokens(options.strip())
    _, _, mode = _wrapper(tokens)
    return {"graphics_mode": mode, "tokens": tokens}


def transform_launch_options(
    options, graphics_mode, remove_args=(), append_args=(), environment=None
):
    if graphics_mode not in GRAPHICS_MODES:
        raise VetError("invalid_graphics_mode", "unsupported graphics mode")
    remove_args = list(remove_args)
    append_args = list(append_args)
    environment = dict(environment or {})
    if any(value not in GRAPHICS_ARGUMENTS for value in remove_args + append_args):
        raise VetError(
            "unsafe_arguments", "Vet may only change known graphics-selection arguments"
        )
    if any(
        name not in ALLOWED_ENVIRONMENT or not isinstance(value, str) or "\0" in value
        for name, value in environment.items()
    ):
        raise VetError(
            "unsafe_environment", "Vet may only change approved environment variables"
        )

    tokens = _launch_tokens(options.strip())
    wrapper_index, command_index, _ = _wrapper(tokens)
    if wrapper_index is not None:
        prefix = tokens[:wrapper_index]
        arguments = tokens[command_index + 1 :]
    elif command_index is not None:
        prefix = tokens[:command_index]
        arguments = tokens[command_index + 1 :]
    else:
        prefix = []
        arguments = tokens

    for name, value in sorted(environment.items()):
        replacement = f"{name}={shlex.quote(value)}"
        indexes = [
            index for index, token in enumerate(prefix) if token.startswith(name + "=")
        ]
        if len(indexes) > 1:
            raise VetError(
                "unsafe_launch_options", f"Steam LaunchOptions contain duplicate {name}"
            )
        if indexes:
            prefix[indexes[0]] = replacement
        else:
            prefix.append(replacement)

    remove = set(remove_args)
    arguments = [argument for argument in arguments if argument not in remove]
    for argument in append_args:
        if argument not in arguments:
            arguments.append(argument)
    return " ".join(
        [*prefix, "varmint-graphics", graphics_mode, "%command%", *arguments]
    )


class Steam:
    APP_PATH = ("UserLocalConfigStore", "Software", "Valve", "Steam", "apps")
    COMPAT_PATH = (
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    )

    def __init__(self, root=STEAM_ROOT):
        self.root = Path(root)

    def localconfig(self):
        configs = sorted(self.root.glob("userdata/*/config/localconfig.vdf"))
        if len(configs) == 1:
            return configs[0]
        loginusers = self.root / "config/loginusers.vdf"
        if configs and loginusers.is_file():
            document = Document(loginusers.read_text(encoding="utf-8"))
            users = document.find(("users",))
            for node in users.children if users and users.children else []:
                if node.child("MostRecent") and node.child("MostRecent").value == "1":
                    candidate = (
                        self.root
                        / "userdata"
                        / str(int(node.key) & 0xFFFFFFFF)
                        / "config/localconfig.vdf"
                    )
                    if candidate in configs:
                        return candidate
        raise VetError(
            "steam_user_not_found" if not configs else "steam_user_ambiguous",
            "Steam user configuration was not found unambiguously",
        )

    def config(self):
        path = self.root / "config/config.vdf"
        if not path.is_file():
            raise VetError(
                "steam_config_not_found",
                "Steam compatibility configuration was not found",
            )
        return path

    def launch_options(self, appid, document=None):
        document = document or Document(self.localconfig().read_text(encoding="utf-8"))
        return document.value(
            (*self.APP_PATH, str(validate_appid(appid)), "LaunchOptions"), ""
        )

    def proton_selection(self, appid, document=None):
        document = document or Document(self.config().read_text(encoding="utf-8"))
        value = document.value(
            (*self.COMPAT_PATH, str(validate_appid(appid)), "name"), ""
        )
        return value or None

    def effective_proton(self, appid, document=None):
        document = document or Document(self.config().read_text(encoding="utf-8"))
        return (
            self.proton_selection(appid, document)
            or document.value((*self.COMPAT_PATH, "0", "name"), "")
            or STEAM_DEFAULT_PROTON
        )

    def proton_tools(self):
        tools = {}
        registered = {}
        try:
            text = (self.root / "logs/compat_log.txt").read_text(
                encoding="utf-8", errors="replace"
            )
            for key, appid in re.findall(
                r"Registering tool ([^,\s]+), AppID (\d+)", text
            ):
                registered[int(appid)] = key
        except OSError:
            pass
        try:
            for appid, info in read_appinfo(self.root / "appcache/appinfo.vdf"):
                common = info.get("common", {})
                name = common.get("name", "")
                official_key = official_proton_key(name)
                if not official_key:
                    continue
                key = registered.get(appid)
                if registered and not key:
                    continue
                key = key or official_key
                if common.get("type") == "Tool" and key:
                    tools[key] = {"name": key, "display_name": name}
        except (OSError, VdfError):
            pass

        steamapps = self.root / "steamapps"
        for path in steamapps.glob("appmanifest_*.acf"):
            try:
                document = Document(path.read_text(encoding="utf-8"))
                if not int(document.value(("AppState", "StateFlags"), "0")) & 4:
                    continue
                name = document.value(("AppState", "name"), "")
                key = official_proton_key(name)
                installdir = document.value(("AppState", "installdir"), "")
                if (
                    key
                    and installdir
                    and (steamapps / "common" / installdir / "proton").is_file()
                ):
                    tools[key] = {"name": key, "display_name": name}
            except (OSError, ValueError, VdfError):
                continue

        custom_root = self.root / "compatibilitytools.d"
        for path in custom_root.glob("*/compatibilitytool.vdf"):
            try:
                document = Document(path.read_text(encoding="utf-8"))
                entries = document.find(("compatibilitytools", "compat_tools"))
                if (
                    entries is None
                    or entries.children is None
                    or not (path.parent / "proton").is_file()
                ):
                    continue
                for entry in entries.children:
                    if entry.children is None:
                        continue
                    display = entry.child("display_name")
                    tools[entry.key] = {
                        "name": entry.key,
                        "display_name": display.value
                        if display and display.value
                        else entry.key,
                    }
            except (OSError, VdfError):
                continue
        return sorted(tools.values(), key=proton_tool_sort_key)

    def configuration_revision(self):
        revision = []
        for path in (self.localconfig(), self.config()):
            stat = path.stat()
            revision.append(
                (str(path), stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns)
            )
        return tuple(revision)

    def _processes(self):
        uid = os.getuid()
        for process in Path("/proc").glob("[0-9]*"):
            try:
                if process.stat().st_uid != uid:
                    continue
                command = [
                    value.decode(errors="replace")
                    for value in (process / "cmdline").read_bytes().split(b"\0")
                    if value
                ]
                environment = (process / "environ").read_bytes()
            except (FileNotFoundError, PermissionError, ProcessLookupError):
                continue
            yield command, environment

    def running(self):
        return any(
            command and Path(command[0]).name == "steam" and "-shutdown" not in command
            for command, _ in self._processes()
        )

    def game_running(self, appid):
        marker = f"SteamAppId={validate_appid(appid)}".encode()
        return any(
            marker in environment.split(b"\0") for _, environment in self._processes()
        )

    def any_game_running(self):
        return any(
            any(value.startswith(b"SteamAppId=") for value in environment.split(b"\0"))
            for _, environment in self._processes()
        )

    def shutdown(self, timeout=30):
        client = self.root / "steamrtarm64/steam"
        if not client.is_file():
            raise VetError(
                "steam_shutdown_failed", "Steam client executable is missing"
            )
        subprocess.Popen(
            [client, "-shutdown"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        deadline = time.monotonic() + timeout
        while self.running() and time.monotonic() < deadline:
            time.sleep(0.2)
        if self.running():
            raise VetError("steam_shutdown_failed", "Steam did not exit cleanly")

    def restart(self):
        subprocess.run(
            [
                "systemd-run",
                "--user",
                "--collect",
                "--quiet",
                "/usr/local/bin/varmint-steam",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=True,
        )

    def update(
        self,
        appid,
        expected,
        launch_options=UNCHANGED,
        proton=UNCHANGED,
        allow_restart=False,
    ):
        appid = validate_appid(appid)
        if proton is not UNCHANGED and proton is not None:
            available = {tool["name"] for tool in self.proton_tools()}
            if proton not in available:
                raise VetError(
                    "invalid_proton",
                    "the selected Proton is not available in Steam's compatibility tool catalog",
                )
        was_running = self.running()
        if self.game_running(appid):
            raise VetError("game_running", "the game is currently running")
        if was_running:
            if not allow_restart:
                raise VetError(
                    "steam_restart_required",
                    "Steam must restart to change its game configuration",
                )
            self.shutdown()

        originals = {}
        try:
            launch_path = self.localconfig()
            config_path = self.config()
            current_launch = self.launch_options(appid)
            current_proton = self.proton_selection(appid)
            if expected and (
                current_launch != expected["launch_options"]
                or current_proton != expected["proton"]
            ):
                raise VetError(
                    "steam_state_changed",
                    "Steam configuration changed after confirmation; review it again",
                )
            target_launch = (
                current_launch if launch_options is UNCHANGED else launch_options
            )
            target_proton = current_proton if proton is UNCHANGED else proton
            if target_launch != current_launch:
                text = launch_path.read_text(encoding="utf-8")
                originals[launch_path] = text
                atomic_write(
                    launch_path,
                    Document(text).set_value(
                        (*self.APP_PATH, str(appid), "LaunchOptions"), target_launch
                    ),
                )
            if target_proton != current_proton:
                text = config_path.read_text(encoding="utf-8")
                originals[config_path] = text
                document = Document(text)
                path = (*self.COMPAT_PATH, str(appid))
                if target_proton:
                    updated = document.set_value((*path, "name"), target_proton)
                    updated = Document(updated).set_value((*path, "config"), "")
                    updated = Document(updated).set_value((*path, "priority"), "250")
                else:
                    updated = document.delete_node(path)
                atomic_write(config_path, updated)
            if (
                self.launch_options(appid) != target_launch
                or self.proton_selection(appid) != target_proton
            ):
                raise VetError(
                    "steam_write_failed", "Steam configuration verification failed"
                )
            return {
                "before": {"launch_options": current_launch, "proton": current_proton},
                "after": {"launch_options": target_launch, "proton": target_proton},
                "steam_restarted": was_running,
            }
        except Exception:
            for path, original in originals.items():
                atomic_write(path, original)
            raise
        finally:
            if was_running:
                self.restart()

    def libraries(self):
        libraries = [self.root]
        path = self.root / "steamapps/libraryfolders.vdf"
        if path.is_file():
            root = Document(path.read_text(encoding="utf-8")).find(("libraryfolders",))
            for entry in root.children if root and root.children else []:
                child = entry.child("path") if entry.children else None
                if child and child.value:
                    libraries.append(Path(child.value))
        return list(dict.fromkeys(libraries))

    def manifest(self, appid):
        for library in self.libraries():
            path = library / "steamapps" / f"appmanifest_{validate_appid(appid)}.acf"
            if path.is_file():
                doc = Document(path.read_text(encoding="utf-8"))
                return {
                    "name": doc.value(("AppState", "name")),
                    "installed": int(doc.value(("AppState", "StateFlags"), "0")) & 4
                    != 0,
                    "build_id": doc.value(("AppState", "buildid")),
                    "game_version": doc.value(("AppState", "BetaKey"), "public")
                    or "public",
                }
        return {
            "name": None,
            "installed": False,
            "build_id": None,
            "game_version": "public",
        }

    def installed_games(self):
        games = {}
        for library in self.libraries():
            for path in (library / "steamapps").glob("appmanifest_*.acf"):
                try:
                    doc = Document(path.read_text(encoding="utf-8"))
                    appid = int(doc.value(("AppState", "appid"), "0"))
                    name = doc.value(("AppState", "name"))
                    if not name:
                        continue
                    is_tool = any(pattern.search(name) for pattern in STEAM_TOOL_NAMES)
                    if (
                        appid > 0
                        and name
                        and not is_tool
                        and int(doc.value(("AppState", "StateFlags"), "0")) & 4
                    ):
                        games[appid] = {
                            "appid": appid,
                            "name": name,
                            "manifest_time": int(path.stat().st_mtime),
                        }
                except (OSError, ValueError, VdfError):
                    pass
        return sorted(
            games.values(),
            key=lambda game: (-game["manifest_time"], game["name"].casefold()),
        )


@dataclass
class Source:
    kind: str
    url: str
    confidence: str
    title: str


@dataclass
class GuideFacts:
    proton: str | None
    graphics_mode: str | None
    launch_args: list[str]
    environment: dict[str, str]
    source_url: str
    profile: str = "default"
    recommendation: str = "recommended"
    evidence: str = "tested"
    setup: str = "automatic"
    game_version: str | None = None
    host_backend: str | None = None
    guide_markdown: str = ""


@dataclass
class GuideMatch:
    confidence: str = "none"
    path: str | None = None
    facts: GuideFacts | None = None


@dataclass
class ExternalFacts:
    direct3d_versions: list[int] = field(default_factory=list)
    architecture: str = "unknown"
    api_selection: list[dict] = field(default_factory=list)
    confidence: str = "none"
    sources: list[Source] = field(default_factory=list)


@dataclass
class Research:
    varmint_doc: GuideMatch
    external: ExternalFacts
    warnings: list[str]
    parent_game: "ParentGame | None" = None
    guide_from_parent: bool = False
    external_from_parent: bool = False


@dataclass
class ParentGame:
    appid: int
    name: str
    source_url: str


@dataclass
class Recommendation:
    graphics_mode: str | None
    proton: str | None
    remove_args: list[str]
    append_args: list[str]
    environment: dict[str, str]
    reason: str
    confidence: str
    message: str
    details: str
    sources: list[Source]
    setup: str = "automatic"
    host_backend: str | None = None


class HttpCache:
    HOSTS = {
        "api.github.com",
        "raw.githubusercontent.com",
        "www.pcgamingwiki.com",
        "store.steampowered.com",
    }
    MAX_FILES = 128

    def __init__(self, directory=None):
        self.directory = Path(directory or STATE_ROOT / "cache")

    @classmethod
    def allowed(cls, url):
        parsed = urlparse(url)
        return parsed.scheme == "https" and parsed.hostname in cls.HOSTS

    def fetch(self, url, limit, max_age):
        if not self.allowed(url):
            raise VetError("invalid_research_url", "research URL is not allowed")
        cache = self.directory / (hashlib.sha256(url.encode()).hexdigest() + ".cache")
        try:
            if time.time() - cache.stat().st_mtime <= max_age:
                data = cache.read_bytes()
                if len(data) <= limit:
                    return data
        except OSError:
            pass
        try:
            request = Request(url, headers={"User-Agent": "Varmint-Vet/1"})
            with urlopen(request, timeout=8) as response:
                if not self.allowed(response.geturl()):
                    raise VetError(
                        "invalid_research_url", "research redirect is not allowed"
                    )
                data = response.read(limit + 1)
            if len(data) > limit:
                raise VetError(
                    "research_too_large", "research response exceeded its size limit"
                )
            self.directory.mkdir(parents=True, exist_ok=True)
            atomic_write(cache, data.decode("utf-8"))
            entries = sorted(
                self.directory.glob("*.cache"),
                key=lambda path: path.stat().st_mtime,
                reverse=True,
            )
            for old in entries[self.MAX_FILES :]:
                try:
                    old.unlink()
                except OSError:
                    pass
            return data
        except Exception:
            try:
                data = cache.read_bytes()
                if len(data) <= limit:
                    return data
            except OSError:
                pass
            raise


def template_field(field_name, text):
    pattern = re.sub(r"\\ ", r"\\s+", re.escape(field_name))
    match = re.search(rf"(?im)^\s*(?:\|\s*)?{pattern}\s*(?:=|:)\s*([^\r\n]*)", text)
    value = match.group(1).strip() if match else ""
    return value or None


def split_markdown_sections(markdown):
    sections = []
    position = 0
    pattern = re.compile(r"^```([^\n`]*)\n(.*?)^```[ \t]*(?:\n|$)", re.M | re.S)
    for match in pattern.finditer(markdown):
        prose = markdown[position : match.start()].strip()
        if prose:
            sections.append(("prose", "", prose))
        sections.append(("code", match.group(1).strip(), match.group(2).rstrip("\n")))
        position = match.end()
    prose = markdown[position:].strip()
    if prose:
        sections.append(("prose", "", prose))
    return sections


class VarmintDocsProvider:
    REMOTE = "https://raw.githubusercontent.com/kisasexypantera94/varmint/master/guest/varmint-vet-catalog.json"
    GUIDE = "https://github.com/kisasexypantera94/varmint/blob/master/"
    BUNDLED = Path("/usr/local/share/varmint/vet/catalog.json")

    def __init__(self, http=None, catalog_path=None):
        self.http = http or HttpCache()
        value = (
            catalog_path
            if catalog_path is not None
            else os.environ.get("VARMINT_VET_CATALOG")
        )
        self.catalog_path = Path(value) if value else None
        self._by_appid = None
        self._load_warnings = []

    def _decode(self, data):
        value = json.loads(data)
        return validate_catalog(value)

    def _load(self):
        if self._by_appid is not None:
            return
        if self.catalog_path:
            self._by_appid = self._decode(self.catalog_path.read_text(encoding="utf-8"))
            return
        last_good = STATE_ROOT / "research" / "catalog.json"
        warnings = []
        try:
            data = self.http.fetch(self.REMOTE, 2 * 1024 * 1024, 6 * 3600).decode(
                "utf-8"
            )
            self._by_appid = self._decode(data)
            atomic_write(last_good, data)
            return
        except Exception as error:
            warnings.append(f"Varmint catalog unavailable: {error}")
        for path in (last_good, self.BUNDLED):
            try:
                self._by_appid = self._decode(path.read_text(encoding="utf-8"))
                self._load_warnings = warnings
                return
            except Exception as error:
                warnings.append(
                    f"Varmint catalog fallback unavailable at {path}: {error}"
                )
        raise CatalogError("; ".join(warnings))

    @staticmethod
    def _version(value):
        return re.sub(r"[^a-z0-9]+", "-", (value or "public").casefold()).strip("-")

    def find(self, appid, name=None, game_version=None):
        try:
            self._load()
        except Exception as error:
            return GuideMatch(), [f"Varmint catalog unavailable: {error}"]
        game = self._by_appid.get(validate_appid(appid))
        if not game:
            return GuideMatch(), list(self._load_warnings)
        profiles = game["profiles"]
        version = self._version(game_version)
        matching = [
            profile
            for profile in profiles
            if profile.get("game_version")
            and self._version(profile["game_version"]) == version
        ]
        generic = [profile for profile in profiles if not profile.get("game_version")]
        candidates = matching or generic
        if not candidates:
            warning = f"Varmint guide has no setup profile for game version {game_version or 'public'}"
            warnings = [*self._load_warnings, warning]
            return GuideMatch("exact", game["guide"]), warnings
        profile = next(
            (value for value in candidates if value["recommendation"] == "recommended"),
            candidates[0],
        )
        source_url = self.GUIDE + game["guide"]
        facts = GuideFacts(
            proton=profile.get("proton"),
            graphics_mode=profile.get("graphics_mode"),
            launch_args=profile.get("launch_args", []),
            environment=profile.get("environment", {}),
            source_url=source_url,
            profile=profile["id"],
            recommendation=profile["recommendation"],
            evidence=profile["evidence"],
            setup=profile["setup"],
            game_version=profile.get("game_version"),
            host_backend=profile.get("host_backend"),
            guide_markdown=game["guide_markdown"],
        )
        return GuideMatch("exact", game["guide"], facts), list(self._load_warnings)


class PCGamingWikiProvider:
    API = "https://www.pcgamingwiki.com/w/api.php"

    def __init__(self, http=None):
        self.http = http or HttpCache()

    @staticmethod
    def contains_appid(text, appid):
        value = template_field("steam appid", text) or ""
        return re.search(rf"(?<![0-9]){appid}(?![0-9])", value) is not None

    @staticmethod
    def direct3d_versions(value):
        return sorted(
            {
                int(match.group(0).split(".")[0])
                for match in re.finditer(
                    r"(?<![0-9])(?:2|3|5|6|7|8|9|10|11|12)(?:\.[0-9]+[a-z]?)?", value
                )
            }
        )

    @staticmethod
    def architecture(bit32, bit64):
        values = ((bit32 or "").lower() == "true", (bit64 or "").lower() == "true")
        return {(True, True): "mixed", (True, False): "x86", (False, True): "x64"}.get(
            values, "unknown"
        )

    @staticmethod
    def api_selection(notes):
        result = []
        for token in ("-dx11", "-d3d11", "-dx12", "-d3d12"):
            if re.search(
                rf"(?<![A-Za-z0-9_-]){re.escape(token)}(?![A-Za-z0-9_-])", notes, re.I
            ):
                result.append(
                    {
                        "api": "d3d12" if "12" in token else "d3d11",
                        "kind": "argument",
                        "value": token,
                    }
                )
        return result

    def _wikitext(self, title):
        query = urlencode(
            {
                "action": "parse",
                "page": title,
                "redirects": "1",
                "prop": "wikitext",
                "format": "json",
                "formatversion": "2",
            }
        )
        return (
            json.loads(self.http.fetch(self.API + "?" + query, 512 * 1024, 24 * 3600))
            .get("parse", {})
            .get("wikitext")
        )

    @staticmethod
    def search_names(name):
        name = unicodedata.normalize(
            "NFKC", name.replace("™", "").replace("®", "")
        ).strip()
        candidates = [name]
        separators = list(re.finditer(r"\s+[—–]\s+|:\s+", name))
        candidates += [name[: match.start()].strip() for match in reversed(separators)]
        shorter = re.sub(
            r"(?i)\s+(?:demo|remastered|benchmark(?:\s+tool)?)$", "", name
        ).strip()
        if shorter != name:
            candidates.append(shorter)
        return list(dict.fromkeys(value for value in candidates if value))

    def find(self, appid, name):
        try:
            fetched = {}
            for search_name in self.search_names(name):
                query = urlencode(
                    {
                        "action": "opensearch",
                        "search": search_name,
                        "redirects": "resolve",
                        "limit": "3",
                        "format": "json",
                        "formatversion": "2",
                    }
                )
                titles = json.loads(
                    self.http.fetch(self.API + "?" + query, 256 * 1024, 24 * 3600)
                )[1]
                matches = []
                for title in titles:
                    if title not in fetched:
                        fetched[title] = self._wikitext(title)
                    text = fetched[title]
                    if text and self.contains_appid(text, appid):
                        matches.append((title, text))
                if len(matches) > 1:
                    sources = [
                        Source(
                            "pcgamingwiki",
                            f"https://www.pcgamingwiki.com/wiki/{quote(title.replace(' ', '_'))}",
                            "ambiguous",
                            f"PCGamingWiki: {title}",
                        )
                        for title, _ in matches
                    ]
                    return ExternalFacts(confidence="ambiguous", sources=sources), [
                        f"More than one PCGamingWiki article contains AppID {appid}"
                    ]
                if len(matches) == 1:
                    title, text = matches[0]
                    versions = self.direct3d_versions(
                        template_field("direct3d versions", text) or ""
                    )
                    notes = template_field("direct3d notes", text) or ""
                    source = Source(
                        "pcgamingwiki",
                        f"https://www.pcgamingwiki.com/wiki/{quote(title.replace(' ', '_'))}",
                        "high",
                        f"PCGamingWiki: {title}",
                    )
                    facts = ExternalFacts(
                        versions,
                        self.architecture(
                            template_field("windows 32-bit exe", text),
                            template_field("windows 64-bit exe", text),
                        ),
                        self.api_selection(notes),
                        "high" if versions else "none",
                        [source],
                    )
                    return facts, [] if versions else [
                        "PCGamingWiki did not expose structured Direct3D versions"
                    ]
            return ExternalFacts(), [
                f"PCGamingWiki has no exact AppID match for {name}"
            ]
        except Exception as error:
            return ExternalFacts(), [f"PCGamingWiki lookup unavailable: {error}"]


class SteamStoreProvider:
    API = "https://store.steampowered.com/api/appdetails"

    def __init__(self, http=None):
        self.http = http or HttpCache()

    def parent_game(self, appid):
        appid = validate_appid(appid)
        query = urlencode({"appids": appid, "l": "english", "filters": "basic"})
        response = json.loads(
            self.http.fetch(self.API + "?" + query, 128 * 1024, 24 * 3600)
        )
        entry = response.get(str(appid), {})
        data = entry.get("data", {}) if entry.get("success") else {}
        fullgame = data.get("fullgame") if data.get("type") == "demo" else None
        if not isinstance(fullgame, dict):
            return None
        try:
            parent_appid = validate_appid(int(fullgame.get("appid", 0)))
        except (TypeError, ValueError, VetError):
            return None
        name = fullgame.get("name")
        if parent_appid == appid or not isinstance(name, str) or not name.strip():
            return None
        return ParentGame(
            parent_appid,
            name.strip(),
            f"https://store.steampowered.com/app/{parent_appid}",
        )


class GameResearchProvider:
    def __init__(self, docs=None, pcgw=None, store=None):
        self.docs = docs or VarmintDocsProvider()
        self.pcgw = pcgw or PCGamingWikiProvider()
        self.store = store or SteamStoreProvider()

    def research(self, appid, name, game_version=None):
        guide, guide_warnings = self.docs.find(appid, name, game_version)
        external, external_warnings = self.pcgw.find(appid, name)
        parent = None
        guide_from_parent = False
        external_from_parent = False
        store_warnings = []
        if external.confidence != "high":
            try:
                parent = self.store.parent_game(appid)
            except Exception as error:
                store_warnings.append(f"Steam full-game lookup unavailable: {error}")
        if parent:
            if not (guide.confidence in {"exact", "strong"} and guide.facts):
                parent_guide, parent_guide_warnings = self.docs.find(
                    parent.appid, parent.name
                )
                if (
                    parent_guide.confidence in {"exact", "strong"}
                    and parent_guide.facts
                ):
                    guide = parent_guide
                    guide_warnings = parent_guide_warnings
                    guide_from_parent = True
            parent_external, parent_external_warnings = self.pcgw.find(
                parent.appid, parent.name
            )
            if parent_external.confidence == "high":
                external = parent_external
                external_warnings = parent_external_warnings
                external_from_parent = True
            else:
                external_warnings += parent_external_warnings
        return Research(
            guide,
            external,
            guide_warnings + external_warnings + store_warnings,
            parent,
            guide_from_parent,
            external_from_parent,
        )


class DeterministicRecommendationProvider:
    def recommend(self, game, state, research):
        guide = research.varmint_doc
        if guide.confidence in {"exact", "strong"} and guide.path and not guide.facts:
            source = Source(
                "varmint_guide",
                VarmintDocsProvider.GUIDE + guide.path,
                guide.confidence,
                "Varmint game guide",
            )
            return Recommendation(
                None,
                None,
                [],
                [],
                {},
                "varmint_version_unmatched",
                "none",
                "No verified setup is available for this game version.",
                "",
                [source, *research.external.sources],
            )
        if guide.confidence in {"exact", "strong"} and guide.facts:
            parent = research.parent_game if research.guide_from_parent else None
            facts = guide.facts
            source = Source(
                "varmint_guide",
                facts.source_url,
                guide.confidence,
                "Varmint game guide",
            )
            if facts.recommendation == "unsupported":
                version = f" ({facts.game_version})" if facts.game_version else ""
                return Recommendation(
                    None,
                    None,
                    [],
                    [],
                    {},
                    "varmint_unsupported",
                    "high",
                    f"This game version{version} is marked unsupported by the Varmint guide.",
                    "",
                    [source, *research.external.sources],
                    "manual",
                    facts.host_backend,
                )
            evidence = (
                "Tested by Varmint."
                if facts.evidence == "tested"
                else "Reported working in Varmint."
            )
            message = (
                "Based on the tested setup for the full game." if parent else evidence
            )
            notes = []
            if facts.setup == "assisted":
                notes.append("Additional steps in the Varmint guide are required.")
            elif facts.setup == "manual":
                notes.append("This setup requires manual steps from the Varmint guide.")
            return self._recommendation(
                state,
                facts.graphics_mode,
                facts.proton,
                facts.launch_args,
                facts.environment,
                "varmint_specific_guide",
                "high",
                message,
                [source, *research.external.sources],
                notes,
                facts.setup,
                facts.host_backend,
            )
        if research.external.confidence != "high":
            ambiguous = (
                guide.confidence == "ambiguous"
                or research.external.confidence == "ambiguous"
            )
            return Recommendation(
                None,
                None,
                [],
                [],
                {},
                "ambiguous_evidence" if ambiguous else "insufficient_evidence",
                "ambiguous" if ambiguous else "none",
                "No reliable setup found.",
                "",
                research.external.sources,
            )
        parent = research.parent_game if research.external_from_parent else None
        versions = set(research.external.direct3d_versions)
        if 11 in versions and 12 in versions:
            mode, reason, api = "neptune", "multiple_apis_prefer_d3d11", "d3d11"
        elif 12 in versions:
            mode, reason, api = "neptune-dx12", "direct3d12_generic_policy", "d3d12"
        elif 11 in versions and research.external.architecture in {"x64", "x86"}:
            mode, reason, api = (
                ("neptune" if research.external.architecture == "x64" else "venus"),
                "direct3d11_generic_policy",
                "d3d11",
            )
        elif 9 in versions:
            mode, reason, api = "venus", "direct3d9_generic_policy", "d3d9"
        else:
            return Recommendation(
                None,
                None,
                [],
                [],
                {},
                "insufficient_evidence",
                "none",
                "No reliable setup found.",
                "",
                research.external.sources,
            )
        arguments = [
            item["value"]
            for item in research.external.api_selection
            if item["kind"] == "argument"
            and item["api"] == api
            and item["value"] in GRAPHICS_ARGUMENTS
        ]
        message = (
            f"Based on the full game's Direct3D {', '.join(map(str, sorted(versions)))} support."
            if parent
            else f"Based on Direct3D {', '.join(map(str, sorted(versions)))} support."
        )
        return self._recommendation(
            state,
            mode,
            None,
            arguments,
            {},
            reason,
            "high",
            message,
            research.external.sources,
            [],
        )

    def _recommendation(
        self,
        state,
        mode,
        proton,
        append_args,
        environment,
        reason,
        confidence,
        message,
        sources,
        notes,
        setup="automatic",
        host_backend=None,
    ):
        host_backend = host_backend or {
            "venus": "MoltenVK",
            "neptune": "DXMT",
            "neptune-dx12": "D3DMetal",
        }.get(mode)
        opposite = (
            {"-dx12", "-d3d12"}
            if any(value in {"-dx11", "-d3d11"} for value in append_args)
            else {"-dx11", "-d3d11"}
            if any(value in {"-dx12", "-d3d12"} for value in append_args)
            else set()
        )
        try:
            current = set(_launch_tokens(state["launch_options"]))
        except VetError:
            current = set()
        remove = sorted(current & opposite)
        details = [*notes]
        return Recommendation(
            mode,
            proton,
            remove,
            append_args,
            environment,
            reason,
            confidence,
            message,
            "\n".join(value for value in details if value),
            sources,
            setup,
            host_backend,
        )


def display_graphics(mode):
    return {"venus": "Venus", "neptune": "Neptune", "neptune-dx12": "Neptune DX12"}.get(
        mode, mode
    )


def display_proton(proton):
    known = {
        "proton_9": "Proton 9",
        "proton_10": "Proton 10",
        "proton_experimental": "Proton Experimental",
        "proton_hotfix": "Proton Hotfix",
        "proton_9-arm64": "Proton 9 (ARM64)",
        "proton_10-arm64": "Proton 10 (ARM64)",
        "proton-experimental-arm64": "Proton Experimental (ARM64)",
        "proton-hotfix-arm64": "Proton Hotfix (ARM64)",
    }
    if proton in known:
        return known[proton]
    match = re.fullmatch(r"proton_(\d+)", proton or "")
    return f"Proton {match.group(1)}" if match else proton


def current_host_backend_warning(state):
    if (
        state.get("effective_graphics_mode") == "neptune-dx12"
        and state.get("host_neptune_backend") == "DXMT"
    ):
        return (
            "Neptune DX12 needs D3DMetal. This VM is using DXMT. "
            "Change the host backend in the Varmint launcher and restart the VM."
        )
    return None


def host_backend_warning(state, recommendation):
    current = state.get("host_neptune_backend")
    required = recommendation.host_backend
    if required != "D3DMetal" or current != "DXMT":
        return None
    return (
        "DX12 needs D3DMetal, but this VM started with DXMT. "
        "Select D3DMetal in the Varmint launcher and restart the VM."
    )


def diagnostic_setup_changes(request, state):
    comparisons = (
        ("graphics backend", request.get("graphics_mode"), state.get("graphics_mode")),
        (
            "host graphics backend",
            request.get("host_backend"),
            state.get("effective_host_backend"),
        ),
        ("Proton", request.get("proton"), state.get("effective_proton")),
        (
            "Launch Options",
            request.get("launch_options", ""),
            state.get("launch_options", ""),
        ),
        ("game build", request.get("build_id"), state.get("build_id")),
    )
    changes = [label for label, captured, current in comparisons if captured != current]
    if "game build" in changes and "build_id" not in request:
        changes[changes.index("game build")] = "game build was not recorded"
    return changes


class VetService:
    def __init__(
        self, steam=None, research=None, recommendations=None, diagnostics=None
    ):
        self.steam = steam or Steam()
        self.research = research or GameResearchProvider()
        self.recommendations = recommendations or DeterministicRecommendationProvider()
        self.diagnostics = diagnostics or DiagnosticStore()

    def game_state(self, appid):
        appid = validate_appid(appid)
        options = self.steam.launch_options(appid)
        manifest = self.steam.manifest(appid)
        try:
            graphics_mode = launch_state(options)["graphics_mode"]
            effective_graphics_mode = graphics_mode or "venus"
            launch_options_error = None
        except VetError as exception:
            graphics_mode = None
            effective_graphics_mode = None
            launch_options_error = str(exception)
        host_neptune_backend = kernel_neptune_backend()
        return {
            "appid": appid,
            "name": manifest["name"],
            "installed": manifest["installed"],
            "build_id": manifest.get("build_id"),
            "game_version": manifest.get("game_version", "public"),
            "host_neptune_backend": host_neptune_backend,
            "effective_host_backend": effective_host_backend(
                effective_graphics_mode, host_neptune_backend
            ),
            "launch_options": options,
            "graphics_mode": graphics_mode,
            "effective_graphics_mode": effective_graphics_mode,
            "launch_options_error": launch_options_error,
            "proton": self.steam.proton_selection(appid),
            "effective_proton": self.steam.effective_proton(appid),
        }

    def prepare(self, appid, name=None):
        state = self.game_state(appid)
        game = {"appid": appid, "name": name or state["name"]}
        research = self.research.research(appid, game["name"], state["game_version"])
        recommendation = self.recommendations.recommend(game, state, research)
        plan = self.plan(state, recommendation)
        return game, state, research, recommendation, plan

    def plan(self, state, recommendation):
        target_options = state["launch_options"]
        graphics_change = recommendation.graphics_mode and (
            recommendation.graphics_mode != state["effective_graphics_mode"]
            or recommendation.remove_args
            or recommendation.append_args
            or recommendation.environment
        )
        if graphics_change:
            target_options = transform_launch_options(
                target_options,
                recommendation.graphics_mode,
                recommendation.remove_args,
                recommendation.append_args,
                recommendation.environment,
            )
        target_proton = state["proton"]
        if recommendation.proton and recommendation.proton != state["effective_proton"]:
            target_proton = recommendation.proton
        return {
            "appid": state["appid"],
            "before": {
                "launch_options": state["launch_options"],
                "proton": state["proton"],
            },
            "after": {"launch_options": target_options, "proton": target_proton},
            "changes": {
                "launch_options": target_options != state["launch_options"],
                "proton": target_proton != state["proton"],
            },
            "host_warning": host_backend_warning(state, recommendation),
        }

    def apply(self, plan, restart_steam=True):
        launch = (
            plan["after"]["launch_options"]
            if plan["changes"]["launch_options"]
            else UNCHANGED
        )
        proton = plan["after"]["proton"] if plan["changes"]["proton"] else UNCHANGED
        result = self.steam.update(
            plan["appid"], plan["before"], launch, proton, restart_steam
        )
        entry = {
            "id": str(uuid.uuid4()),
            "time": now(),
            "appid": plan["appid"],
            "action": "configure_game",
            "changed_fields": [
                name for name, changed in plan["changes"].items() if changed
            ],
            "before": result["before"],
            "after": result["after"],
        }
        append_history(entry)
        result["action_id"] = entry["id"]
        return result

    def arm_diagnostics(self, appid):
        state = self.game_state(appid)
        if state["launch_options_error"]:
            raise VetError("diagnostics_unavailable", state["launch_options_error"])
        if not state["graphics_mode"]:
            raise VetError(
                "diagnostics_unavailable",
                "Choose a Varmint graphics backend before capturing diagnostics.",
            )
        if self.steam.game_running(appid):
            raise VetError(
                "game_running", "Close the game before arming its next launch."
            )
        try:
            request = self.diagnostics.arm(state)
        except DiagnosticError as exception:
            raise VetError("diagnostics_failed", str(exception)) from exception
        append_history(
            {
                "id": str(uuid.uuid4()),
                "time": now(),
                "appid": state["appid"],
                "action": "capture_next_launch",
                "run_id": request["run_id"],
            }
        )
        return request

    def cancel_diagnostics(self, appid):
        return self.diagnostics.cancel(appid)

    def diagnostic_status(self, appid):
        state = self.game_state(appid)
        snapshot = self.diagnostics.status(appid)
        pending = snapshot.get("pending")
        latest = snapshot.get("latest_request")
        snapshot["pending_setup_changes"] = (
            diagnostic_setup_changes(pending, state) if pending else []
        )
        snapshot["latest_setup_changes"] = (
            diagnostic_setup_changes(latest, state) if latest else []
        )
        return snapshot

    def diagnostic_report(self, run_id):
        try:
            return self.diagnostics.report(run_id)
        except DiagnosticError as exception:
            raise VetError("diagnostics_failed", str(exception)) from exception


def append_history(entry):
    path = (
        STATE_ROOT / "history" / f"{entry['time'].replace(':', '-')}-{entry['id']}.json"
    )
    atomic_write(path, json.dumps(entry, indent=2, sort_keys=True) + "\n")


class Preferences:
    def __init__(self, path=None):
        self.path = Path(path or STATE_ROOT / "preferences.json")

    def _load(self):
        try:
            return json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}

    def suggestions_enabled(self):
        return self._load().get("automatic_setup_suggestions", True) is not False

    def set_suggestions_enabled(self, enabled):
        value = {"automatic_setup_suggestions": bool(enabled)}
        atomic_write(self.path, json.dumps(value, indent=2, sort_keys=True) + "\n")


class InstallMonitor:
    def __init__(self, steam, path=None):
        self.steam = steam
        self.path = Path(path or STATE_ROOT / "installed-games.json")

    def _load(self):
        try:
            value = json.loads(self.path.read_text(encoding="utf-8"))
            return value if value.get("schema_version") == 1 else None
        except (OSError, ValueError):
            return None

    def poll(self):
        current = {str(game["appid"]): game for game in self.steam.installed_games()}
        state = self._load()
        if state is None:
            atomic_write(
                self.path,
                json.dumps(
                    {
                        "schema_version": 1,
                        "seen": {key: value["name"] for key, value in current.items()},
                        "candidates": {},
                    },
                    indent=2,
                )
                + "\n",
            )
            return []
        seen = {
            key: value for key, value in state.get("seen", {}).items() if key in current
        }
        candidates = {
            key: value
            for key, value in state.get("candidates", {}).items()
            if key in current and key not in seen
        }
        events = []
        for key, game in current.items():
            if key in seen:
                continue
            stamp = game["manifest_time"]
            if candidates.get(key) == stamp:
                events.append({"appid": game["appid"], "name": game["name"]})
                seen[key] = game["name"]
                candidates.pop(key, None)
            else:
                candidates[key] = stamp
        atomic_write(
            self.path,
            json.dumps(
                {"schema_version": 1, "seen": seen, "candidates": candidates},
                indent=2,
                sort_keys=True,
            )
            + "\n",
        )
        return events

"""Compile and validate Varmint's human-readable game guide catalog."""

from dataclasses import dataclass
import json
from pathlib import Path
import re
import shlex


SCHEMA_VERSION = 2
VET_VERSION = 1
MAX_GUIDE_BYTES = 128 * 1024
RECOMMENDATIONS = {"recommended", "alternative", "unsupported"}
EVIDENCE = {"tested", "reported"}
RUNTIMES = {"proton", "native linux"}
SETUP_KINDS = {"automatic", "assisted", "manual"}
RENDERERS = {
    "venus": "venus",
    "neptune": "neptune",
    "neptune dx12": "neptune-dx12",
    "vulkan": None,
}
GRAPHICS_ARGUMENTS = {"-dx11", "-d3d11", "-dx12", "-d3d12"}
ALLOWED_ENVIRONMENT = {"FEX_X87REDUCEDPRECISION"}
PROFILE_FIELDS = {
    "Steam App IDs",
    "Profile",
    "Recommendation",
    "Evidence",
    "Setup",
    "Game version",
    "Runtime",
    "Proton version",
    "Renderer",
    "Host backend",
    "Launch options",
    "Graphics settings",
    "RAM",
    "Patch",
    "snd_mixahead",
}
HOST_BACKENDS = {"DXMT", "D3DMetal", "MoltenVK"}
HOST_BACKENDS_BY_RENDERER = {
    "venus": {"MoltenVK"},
    "neptune": {"DXMT", "D3DMetal"},
    "neptune-dx12": {"D3DMetal"},
    None: {"MoltenVK"},
}


class CatalogError(ValueError):
    pass


@dataclass(frozen=True)
class ParsedLaunchOptions:
    arguments: list[str]
    environment: dict[str, str]


def _fields(block, source):
    result = {}
    for line in block.splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"([^:]+):\s*(.*)", line)
        if not match:
            raise CatalogError(f"{source}: malformed profile line: {line!r}")
        key, value = match.group(1).strip(), match.group(2).strip()
        if key not in PROFILE_FIELDS:
            raise CatalogError(f"{source}: unsupported profile field {key!r}")
        if key in result:
            raise CatalogError(f"{source}: duplicate field {key!r}")
        result[key] = value
    return result


def _choice(fields, name, allowed, source, required=True):
    value = fields.get(name)
    if value is None:
        if required:
            raise CatalogError(f"{source}: missing {name}")
        return None
    normalized = value.casefold()
    if normalized not in allowed:
        raise CatalogError(f"{source}: unsupported {name}: {value}")
    return normalized


def _proton(value, source):
    normalized = value.casefold()
    if normalized == "steam default":
        return None
    arm64 = "arm64" in normalized
    if "experimental" in normalized:
        return "proton-experimental-arm64" if arm64 else "proton_experimental"
    if "hotfix" in normalized:
        return "proton-hotfix-arm64" if arm64 else "proton_hotfix"
    match = re.fullmatch(r"proton\s+(\d+)(?:\.0)?(?:\s*\(arm64\))?", normalized)
    if match:
        return f"proton_{match.group(1)}-arm64" if arm64 else f"proton_{match.group(1)}"
    raise CatalogError(f"{source}: unsupported Proton version: {value}")


def _launch_options(value, renderer, setup, source):
    if value.casefold() in {"none", "unchanged"}:
        return ParsedLaunchOptions([], {})
    if setup == "manual":
        return ParsedLaunchOptions([], {})
    if any(character in value for character in ";|&<>`(){}#") or "$(" in value:
        raise CatalogError(f"{source}: executable shell syntax requires Setup: Manual")
    try:
        tokens = shlex.split(value)
    except ValueError as error:
        raise CatalogError(f"{source}: malformed Launch options: {error}") from error
    if tokens and tokens[0].rsplit("/", 1)[-1] in {"sh", "bash", "dash", "zsh"}:
        raise CatalogError(f"{source}: executable shell syntax requires Setup: Manual")
    wrapper = [
        index
        for index, token in enumerate(tokens)
        if token.rsplit("/", 1)[-1] == "varmint-graphics"
    ]
    command = [index for index, token in enumerate(tokens) if token == "%command%"]
    if wrapper:
        if len(wrapper) != 1 or len(command) != 1 or wrapper[0] + 2 != command[0]:
            raise CatalogError(f"{source}: malformed varmint-graphics wrapper")
        mode = tokens[wrapper[0] + 1] if wrapper[0] + 1 < len(tokens) else None
        if mode != renderer:
            raise CatalogError(f"{source}: Renderer and varmint-graphics mode disagree")
    environment = {}
    for token in tokens[: command[0] if command else len(tokens)]:
        if "=" not in token:
            continue
        name, value = token.split("=", 1)
        if name not in ALLOWED_ENVIRONMENT:
            raise CatalogError(f"{source}: environment {name} requires Setup: Manual")
        environment[name] = value
    arguments = tokens[command[0] + 1 :] if command else []
    unsupported = [
        argument for argument in arguments if argument not in GRAPHICS_ARGUMENTS
    ]
    if unsupported:
        raise CatalogError(
            f"{source}: launch arguments {unsupported!r} require Setup: Manual"
        )
    return ParsedLaunchOptions(arguments, environment)


def compile_guide(path):
    path = Path(path)
    text = path.read_text(encoding="utf-8")
    title_match = re.search(r"(?m)^# Running (.+?) in Varmint\s*$", text)
    if not title_match:
        raise CatalogError(f"{path}: missing game title")
    blocks = []
    for match in re.finditer(r"```text\s*\n(.*?)```", text, re.S):
        if not re.search(r"(?m)^Profile\s*:", match.group(1)):
            continue
        fields = _fields(match.group(1), path)
        blocks.append(fields)
    if not blocks:
        raise CatalogError(f"{path}: no structured setup profile")
    appid_value = blocks[0].get("Steam App IDs")
    if not appid_value:
        raise CatalogError(f"{path}: first profile must contain Steam App IDs")
    try:
        appids = [int(value.strip()) for value in appid_value.split(",")]
    except ValueError as error:
        raise CatalogError(f"{path}: malformed Steam App IDs") from error
    if not appids or any(value <= 0 or value > 0xFFFFFFFF for value in appids):
        raise CatalogError(f"{path}: invalid Steam App IDs")
    profiles = []
    profile_ids = set()
    for index, fields in enumerate(blocks):
        source = f"{path}:{fields.get('Profile', index + 1)}"
        profile_id = fields.get("Profile", "").strip()
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", profile_id):
            raise CatalogError(f"{source}: invalid Profile")
        if profile_id in profile_ids:
            raise CatalogError(f"{source}: duplicate Profile")
        profile_ids.add(profile_id)
        recommendation = _choice(fields, "Recommendation", RECOMMENDATIONS, source)
        evidence = _choice(fields, "Evidence", EVIDENCE, source)
        setup = _choice(fields, "Setup", SETUP_KINDS, source)
        if recommendation == "unsupported":
            profiles.append(
                {
                    "id": profile_id,
                    "recommendation": recommendation,
                    "evidence": evidence,
                    "setup": "manual",
                    "game_version": fields.get("Game version"),
                }
            )
            continue
        runtime = _choice(fields, "Runtime", RUNTIMES, source)
        if runtime == "native linux" and setup != "manual":
            raise CatalogError(
                f"{source}: native runtime currently requires Setup: Manual"
            )
        proton = None
        if runtime == "proton":
            if "Proton version" not in fields:
                raise CatalogError(f"{source}: missing Proton version")
            proton = _proton(fields["Proton version"], source)
        renderer_name = fields.get("Renderer", "").casefold()
        if renderer_name not in RENDERERS:
            raise CatalogError(
                f"{source}: unsupported Renderer: {fields.get('Renderer')}"
            )
        renderer = RENDERERS[renderer_name]
        host_backend = fields.get("Host backend")
        if host_backend not in HOST_BACKENDS:
            raise CatalogError(f"{source}: unsupported Host backend: {host_backend}")
        if host_backend not in HOST_BACKENDS_BY_RENDERER[renderer]:
            raise CatalogError(f"{source}: Renderer and Host backend disagree")
        launch = _launch_options(
            fields.get("Launch options", "Unchanged"), renderer, setup, source
        )
        profiles.append(
            {
                "id": profile_id,
                "recommendation": recommendation,
                "evidence": evidence,
                "setup": setup,
                "game_version": fields.get("Game version"),
                "runtime": runtime,
                "proton": proton,
                "graphics_mode": renderer,
                "host_backend": host_backend,
                "launch_args": launch.arguments,
                "environment": launch.environment,
            }
        )
    if not any(profile["recommendation"] == "recommended" for profile in profiles):
        raise CatalogError(f"{path}: no recommended profile")
    return {
        "title": title_match.group(1),
        "appids": appids,
        "guide": f"docs/games/{path.name}",
        "guide_markdown": text,
        "profiles": profiles,
    }


def compile_catalog(directory):
    directory = Path(directory)
    games = [
        compile_guide(path)
        for path in sorted(directory.glob("*.md"))
        if path.name != "common.md"
    ]
    owners = {}
    for game in games:
        for appid in game["appids"]:
            if appid in owners:
                raise CatalogError(
                    f"Steam App {appid} is declared by both {owners[appid]} and {game['guide']}"
                )
            owners[appid] = game["guide"]
    return {
        "schema_version": SCHEMA_VERSION,
        "minimum_vet_version": VET_VERSION,
        "games": games,
    }


def validate_catalog(value):
    if not isinstance(value, dict):
        raise CatalogError("catalog must be an object")
    if value.get("schema_version") != SCHEMA_VERSION:
        raise CatalogError("unsupported catalog schema")
    minimum = value.get("minimum_vet_version", 0)
    if not isinstance(minimum, int) or minimum > VET_VERSION:
        raise CatalogError("catalog requires a newer Vet")
    games = value.get("games")
    if not isinstance(games, list):
        raise CatalogError("catalog games must be a list")
    by_appid = {}
    for game in games:
        appids = game.get("appids") if isinstance(game, dict) else None
        profiles = game.get("profiles") if isinstance(game, dict) else None
        if (
            not isinstance(game, dict)
            or not isinstance(game.get("title"), str)
            or not re.fullmatch(r"docs/games/[a-z0-9-]+\.md", game.get("guide", ""))
            or not isinstance(game.get("guide_markdown"), str)
            or not game["guide_markdown"].strip()
            or len(game["guide_markdown"].encode("utf-8")) > MAX_GUIDE_BYTES
            or not isinstance(appids, list)
            or not appids
            or not isinstance(profiles, list)
            or not profiles
        ):
            raise CatalogError("invalid catalog game")
        for appid in appids:
            if not isinstance(appid, int) or appid <= 0 or appid in by_appid:
                raise CatalogError("invalid or duplicate catalog AppID")
            by_appid[appid] = game
        profile_ids = set()
        recommended = False
        for profile in profiles:
            profile_id = profile.get("id", "") if isinstance(profile, dict) else ""
            recommendation = (
                profile.get("recommendation") if isinstance(profile, dict) else None
            )
            renderer = (
                profile.get("graphics_mode") if isinstance(profile, dict) else None
            )
            host_backend = (
                profile.get("host_backend") if isinstance(profile, dict) else None
            )
            if (
                not isinstance(profile, dict)
                or not re.fullmatch(r"[a-z0-9][a-z0-9-]*", profile_id)
                or profile_id in profile_ids
                or recommendation not in RECOMMENDATIONS
                or profile.get("evidence") not in EVIDENCE
                or profile.get("setup") not in SETUP_KINDS
                or renderer not in {*RENDERERS.values(), None}
                or host_backend not in {*HOST_BACKENDS, None}
                or not isinstance(profile.get("launch_args", []), list)
                or any(
                    value not in GRAPHICS_ARGUMENTS
                    for value in profile.get("launch_args", [])
                )
                or not isinstance(profile.get("environment", {}), dict)
                or any(
                    name not in ALLOWED_ENVIRONMENT
                    for name in profile.get("environment", {})
                )
                or any(
                    not isinstance(value, str)
                    for value in profile.get("environment", {}).values()
                )
                or (
                    profile.get("proton") is not None
                    and not isinstance(profile.get("proton"), str)
                )
            ):
                raise CatalogError("invalid catalog profile")
            profile_ids.add(profile_id)
            recommended |= recommendation == "recommended"
            if recommendation == "unsupported":
                if (
                    profile.get("setup") != "manual"
                    or renderer is not None
                    or host_backend is not None
                ):
                    raise CatalogError("invalid unsupported catalog profile")
            elif host_backend not in HOST_BACKENDS_BY_RENDERER[renderer]:
                raise CatalogError("catalog Renderer and Host backend disagree")
        if not recommended:
            raise CatalogError("catalog game has no recommended profile")
    return by_appid


def catalog_json(value):
    validate_catalog(value)
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"

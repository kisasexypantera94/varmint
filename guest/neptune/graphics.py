#!/usr/bin/env python3

import fcntl
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

for library in (
    Path.home() / ".local/lib/varmint",
    Path("/usr/local/lib/varmint"),
):
    if library.is_dir():
        sys.path.insert(0, str(library))

try:
    from varmint_vet_diagnostics import claim_from_environment
except ImportError:
    claim_from_environment = None

SCHEMA = "shadow-v9"
DEFAULT_NEPTUNE = Path("/usr/local/lib/varmint/neptune")
DEFAULT_CACHE = Path.home() / ".cache/varmint/proton-graphics"
PE_AMD64 = 0x8664
PE_ARM64 = 0xAA64
UNIX_ELF_MACHINES = {"x86_64-unix": 62, "aarch64-unix": 183}


def graphics_policy(mode):
    if mode == "neptune":
        return "d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64,nvofapi64="
    if mode == "neptune-dx12":
        return "d3d11,dxgi=n,b;nptunix=b;d3d12=n,b;d3d12core=;nvapi,nvapi64,nvofapi64="
    fail(f"unsupported graphics mode: {mode}")


def neptune_dlls(mode):
    # D3D11 stays on Neptune in DX12 mode too: DXVK's d3d11 cannot run on
    # Neptune's dxgi, and both Neptune DLLs share one device per process.
    if mode == "neptune":
        return ("dxgi", "d3d11")
    return ("dxgi", "d3d11", "d3d12")


def fail(message):
    print(f"varmint-graphics: {message}", file=sys.stderr)
    raise SystemExit(1)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def pe_machine(path):
    try:
        with open(path, "rb") as f:
            header = f.read(64)
            if len(header) < 64 or header[:2] != b"MZ":
                return None
            pe_offset = struct.unpack_from("<I", header, 0x3C)[0]
            f.seek(pe_offset)
            pe = f.read(6)
            if len(pe) != 6 or pe[:4] != b"PE\0\0":
                return None
            return struct.unpack_from("<H", pe, 4)[0]
    except OSError:
        return None


def elf_machine(path):
    try:
        with open(path, "rb") as f:
            header = f.read(20)
            if (
                len(header) < 20
                or header[:4] != b"\x7fELF"
                or header[4] != 2
                or header[5] != 1
            ):
                return None
            return struct.unpack_from("<H", header, 18)[0]
    except OSError:
        return None


def replace_with_copy(src, dst):
    dst.parent.mkdir(parents=True, exist_ok=True)
    if os.path.lexists(dst):
        if dst.is_dir() and not dst.is_symlink():
            shutil.rmtree(dst)
        else:
            dst.unlink()
    shutil.copy2(src, dst)


def link_regular_file(src, dst):
    os.symlink(os.path.abspath(src), dst)


def proton_wine_roots(root):
    roots = []
    seen = set()
    for runtime_name in ("files", "dist"):
        runtime = root / runtime_name
        for lib_name in ("lib", "lib64"):
            wine = runtime / lib_name / "wine"
            if not wine.is_dir():
                continue
            try:
                key = wine.resolve(strict=True)
            except OSError:
                continue
            if key in seen:
                continue
            seen.add(key)
            roots.append(wine)
    return roots


def find_proton(command):
    matches = []
    for index, arg in enumerate(command):
        path = Path(arg)
        if path.name != "proton":
            continue
        try:
            path = path.resolve(strict=True)
        except OSError:
            continue
        root = path.parent
        if path.is_file() and proton_wine_roots(root):
            matches.append((index, root))

    if not matches:
        fail("could not find a supported Proton launcher in %command%")
    if len(matches) != 1:
        found = ", ".join(str(root / "proton") for _, root in matches)
        fail(f"found multiple Proton launchers: {found}")
    return matches[0]


def require_file(path, kind):
    if not path.is_file() or path.stat().st_size == 0:
        fail(f"missing {kind}: {path}")


def require_amd64_pe(path, kind):
    require_file(path, kind)
    machine = pe_machine(path)
    if machine != PE_AMD64:
        machine_text = "not PE" if machine is None else f"PE machine 0x{machine:04x}"
        fail(f"unsupported {kind}: {path} ({machine_text}, expected AMD64)")


def require_unix_elf(path, kind, unix_arch):
    require_file(path, kind)
    machine = elf_machine(path)
    if machine != UNIX_ELF_MACHINES[unix_arch]:
        machine_text = (
            "not 64-bit little-endian ELF"
            if machine is None
            else f"ELF machine {machine}"
        )
        fail(f"unsupported {kind}: {path} ({machine_text}, expected {unix_arch})")


def validate_artifacts(paths, unix_arch):
    for name, path in paths.items():
        if name != "nptso":
            require_amd64_pe(path, f"Neptune {path.name}")
    require_unix_elf(paths["nptso"], "Neptune nptunix.so", unix_arch)
    return paths


def prebuilt_artifact_paths(mode, base, unix_arch):
    windows = base / "x86_64-windows"
    paths = {name: windows / f"{name}.dll" for name in neptune_dlls(mode)}
    paths["nptdll"] = windows / "nptunix.dll"
    paths["nptso"] = base / unix_arch / "nptunix.so"
    return validate_artifacts(paths, unix_arch)


def build_artifact_paths(mode, base, unix_arch):
    paths = {name: base / f"{name}.dll" for name in neptune_dlls(mode)}
    paths["nptdll"] = base / "nptunix" / "nptunix.dll"
    paths["nptso"] = base / "nptunix" / "nptunix.so"
    return validate_artifacts(paths, unix_arch)


def proton_uses_arm64(root):
    # Mirrors the runtime selection in Proton's own launcher.
    return (
        os.environ.get("PROTON_USE_ARM64", "1") == "1"
        and platform.machine() == "aarch64"
        and (root / "files/bin-arm64").is_dir()
    )


def find_component_dir(root, wine_roots, component, names, machine):
    matches = []
    seen = set()

    for wine in wine_roots:
        component_root = wine / component
        if not component_root.is_dir():
            continue

        for first in component_root.rglob(names[0]):
            directory = first.parent
            if directory in seen:
                continue
            candidates = [directory / name for name in names]
            if all(path.is_file() and pe_machine(path) == machine for path in candidates):
                seen.add(directory)
                matches.append(directory)

    machine_text = "ARM64" if machine == PE_ARM64 else "AMD64"
    if not matches:
        wanted = " + ".join(names)
        fail(f"unsupported Proton layout: no {machine_text} {wanted} found under {component}")
    if len(matches) != 1:
        found = ", ".join(str(path.relative_to(root)) for path in matches)
        fail(f"unsupported Proton layout: ambiguous {machine_text} {component} directories: {found}")
    return matches[0]


def discover_proton_layout(mode, root, arm64):
    wine_roots = proton_wine_roots(root)
    if not wine_roots:
        fail("unsupported Proton layout: no files/dist lib*/wine root found")

    # Neptune's PE frontend is AMD64 in both runtimes. ARM64EC Wine loads an
    # AMD64 builtin from x86_64-windows and its unixlib from aarch64-unix.
    # Proton copies the ARM64X DXVK/vkd3d-proton DLLs into ARM64 prefixes.
    unix_arch = "aarch64-unix" if arm64 else "x86_64-unix"
    component_machine = PE_ARM64 if arm64 else PE_AMD64

    arch_pairs = []
    for wine in wine_roots:
        windows = wine / "x86_64-windows"
        unix = wine / unix_arch
        if windows.is_dir() and unix.is_dir():
            arch_pairs.append((windows, unix))

    if not arch_pairs:
        fail(f"unsupported Proton layout: no x86_64-windows/{unix_arch} Wine pair found")
    if len(arch_pairs) != 1:
        found = ", ".join(str(w.relative_to(root).parent) for w, _ in arch_pairs)
        fail(f"unsupported Proton layout: ambiguous {unix_arch} Wine roots: {found}")

    dxvk_dir = find_component_dir(
        root, wine_roots, "dxvk", ["dxgi.dll", "d3d11.dll"], component_machine
    )

    windows, unix = arch_pairs[0]
    layout = {
        "dxgi": dxvk_dir / "dxgi.dll",
        "d3d11": dxvk_dir / "d3d11.dll",
        "npt_windows": windows,
        "npt_unix": unix,
    }
    if mode == "neptune-dx12":
        vkd3d_dir = find_component_dir(
            root, wine_roots, "vkd3d-proton", ["d3d12.dll"], component_machine
        )
        layout["d3d12"] = vkd3d_dir / "d3d12.dll"
    return layout


def proton_slot_id(proton_root):
    label = re.sub(r"[^A-Za-z0-9._-]+", "_", proton_root.name).strip("._-")
    if not label:
        label = "proton"
    digest = hashlib.sha256(str(proton_root).encode("utf-8")).hexdigest()[:16]
    return f"{label[:48]}-{digest}"


def fingerprint(mode, proton_root, artifacts, layout):
    h = hashlib.sha256()

    def add(value):
        h.update(value.encode("utf-8"))
        h.update(b"\0")

    add(SCHEMA)
    add(mode)
    add(graphics_policy(mode))
    add(str(proton_root))

    for rel in ("proton", "version", "steampipe_fixups.json"):
        path = proton_root / rel
        add(rel)
        add(sha256(path) if path.is_file() else "<missing>")

    for name in neptune_dlls(mode):
        path = layout[name]
        add(f"layout:{name}")
        add(str(path.relative_to(proton_root)))
        add(sha256(path))

    for name in ("npt_windows", "npt_unix"):
        add(f"layout:{name}")
        add(str(layout[name].relative_to(proton_root)))

    for name in sorted(artifacts):
        add(f"artifact:{name}")
        add(sha256(artifacts[name]))

    return h.hexdigest()


def prepare_steampipe_state(proton_root, shadow):
    fixups = proton_root / "steampipe_fixups.json"
    if not fixups.is_file():
        return

    try:
        with open(fixups, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except (OSError, ValueError) as e:
        fail(f"cannot read Proton SteamPipe fixups: {e}")

    if not isinstance(manifest, dict):
        fail("unsupported Proton SteamPipe fixups: manifest is not an object")

    supported_keys = {"id", "empty_dirs", "no_write_paths"}
    unknown_keys = set(manifest) - supported_keys
    if unknown_keys:
        names = ", ".join(sorted(unknown_keys))
        fail(f"unsupported Proton SteamPipe fixups: unknown fields: {names}")

    for key in ("empty_dirs", "no_write_paths"):
        values = manifest.get(key)
        if not isinstance(values, list) or not all(isinstance(value, str) for value in values):
            fail(f"unsupported Proton SteamPipe fixups: invalid {key}")

        for value in values:
            rel = Path(value)
            if rel.is_absolute() or ".." in rel.parts:
                fail(f"unsupported Proton SteamPipe path: {value}")

    # Proton's current fixups contain only empty directory restoration and
    # chmod(no_write_paths). Recreate empty directories locally, but skip chmod:
    # regular shadow files are symlinks into the Steam-managed Proton tree.
    for rel in manifest["empty_dirs"]:
        target = shadow / rel
        require_shadow_local(shadow, target, "SteamPipe directory", include_leaf=True)
        target.mkdir(parents=True, exist_ok=True)

    marker = shadow / "files/steampipe_fixups_mtime"
    require_shadow_local(shadow, marker, "SteamPipe marker")
    marker.parent.mkdir(parents=True, exist_ok=True)
    if os.path.lexists(marker):
        marker.unlink()
    marker.write_text(str(os.path.getmtime(fixups)) + "\n", encoding="utf-8")

def shadow_path(shadow, proton_root, original):
    return shadow / original.relative_to(proton_root)


def require_shadow_local(shadow, path, kind, include_leaf=False):
    root = shadow.resolve()
    check = path if include_leaf else path.parent
    try:
        resolved = check.resolve(strict=False)
        resolved.relative_to(root)
    except (OSError, ValueError):
        fail(f"unsafe shadow {kind}: {path}")


def patch_shadow_proton_for_nptunix(shadow, nptunix_rel, policy):
    proton = shadow / "proton"
    try:
        text = proton.read_text(encoding="utf-8")
    except OSError as e:
        fail(f"cannot read shadow Proton launcher: {e}")

    # Proton 7 through 11 and current Experimental all prepare the prefix
    # through this block. Install the carrier after setup_prefix(), but outside
    # the conditional, so every Neptune launch repairs a missing/stale carrier.
    prefix_anchor = (
        "        if update_prefix_files:\n"
        "            g_compatdata.setup_prefix()\n"
    )
    if text.count(prefix_anchor) != 1:
        fail("unsupported Proton launcher: could not locate unique setup_prefix() block")

    # Proton merges user_settings.py before making its prefix decisions. Strip
    # only DLL overrides owned by Neptune after that merge. Otherwise a user
    # dxgi=b/path-specific override can make Proton skip copying our DXGI.
    settings_anchor = (
        '                log("%s" % sys.exc_info()[1])\n'
        '                log("************************************************")\n'
        "\n"
        '        if "PROTON_LOG" in self.env and nonzero(self.env["PROTON_LOG"]):\n'
    )
    if text.count(settings_anchor) != 1:
        fail("unsupported Proton launcher: could not locate unique user-settings hook")

    # Proton appends its own DLL overrides after setup_prefix(). Neptune's
    # policy must come after that append or Proton can override it again.
    overrides_anchor = (
        '        append_to_env_str(self.env, "WINEDLLOVERRIDES", s, ";")\n'
    )
    if text.count(overrides_anchor) != 1:
        fail("unsupported Proton launcher: could not locate unique DLL override hook")

    if nptunix_rel.is_absolute() or ".." in nptunix_rel.parts:
        fail(f"invalid shadow nptunix path: {nptunix_rel}")
    require_amd64_pe(shadow / nptunix_rel, "shadow Neptune nptunix.dll")

    rel = nptunix_rel.as_posix()
    managed = tuple(sorted(policy_dll_names(policy)))

    settings_hook = settings_anchor[:-len(
        '        if "PROTON_LOG" in self.env and nonzero(self.env["PROTON_LOG"]):\n'
    )] + (
        f"        varmint_managed_dlls = {managed!r}\n"
        '        varmint_overrides = self.env.get("WINEDLLOVERRIDES", "")\n'
        "        if varmint_overrides:\n"
        "            varmint_kept = []\n"
        '            for varmint_entry in varmint_overrides.split(";"):\n'
        "                varmint_entry = varmint_entry.strip()\n"
        "                if not varmint_entry:\n"
        "                    continue\n"
        '                varmint_modules, varmint_sep, varmint_order = varmint_entry.partition("=")\n'
        "                if not varmint_sep:\n"
        "                    varmint_kept.append(varmint_entry)\n"
        "                    continue\n"
        "                varmint_names = []\n"
        '                for varmint_name in varmint_modules.replace(",", " ").replace("\t", " ").split():\n'
        '                    varmint_key = varmint_name.lower().replace(chr(92), "/").rsplit("/", 1)[-1]\n'
        '                    varmint_key = varmint_key.lstrip("*")\n'
        '                    if varmint_key.endswith(".dll"):\n'
        "                        varmint_key = varmint_key[:-4]\n"
        "                    if varmint_key not in varmint_managed_dlls:\n"
        "                        varmint_names.append(varmint_name)\n"
        "                if varmint_names:\n"
        '                    varmint_kept.append(",".join(varmint_names) + "=" + varmint_order)\n'
        '            self.env["WINEDLLOVERRIDES"] = ";".join(varmint_kept)\n'
        "\n"
        '        if "PROTON_LOG" in self.env and nonzero(self.env["PROTON_LOG"]):\n'
    )

    prefix_hook = prefix_anchor + (
        "\n"
        "        import os as varmint_os\n"
        "        import shutil as varmint_shutil\n"
        "        import tempfile as varmint_tempfile\n"
        "        varmint_nptunix = varmint_os.path.join(\n"
        "            varmint_os.path.dirname(varmint_os.path.abspath(__file__)),\n"
        f"            {rel!r},\n"
        "        )\n"
        "        varmint_system32 = varmint_os.path.join(\n"
        '            g_compatdata.prefix_dir, "drive_c", "windows", "system32"\n'
        "        )\n"
        "        varmint_os.makedirs(varmint_system32, exist_ok=True)\n"
        '        varmint_dst = varmint_os.path.join(varmint_system32, "nptunix.dll")\n'
        "        varmint_fd, varmint_tmp = varmint_tempfile.mkstemp(\n"
        '            prefix=".nptunix.dll.varmint-", dir=varmint_system32\n'
        "        )\n"
        "        varmint_os.close(varmint_fd)\n"
        "        try:\n"
        "            varmint_shutil.copy2(varmint_nptunix, varmint_tmp)\n"
        "            varmint_os.replace(varmint_tmp, varmint_dst)\n"
        "        finally:\n"
        "            try:\n"
        "                varmint_os.unlink(varmint_tmp)\n"
        "            except FileNotFoundError:\n"
        "                pass\n"
    )

    policy_hook = overrides_anchor + (
        f'        append_to_env_str(self.env, "WINEDLLOVERRIDES", {policy!r}, ";")\n'
    )

    patched = text.replace(settings_anchor, settings_hook)
    patched = patched.replace(prefix_anchor, prefix_hook)
    patched = patched.replace(overrides_anchor, policy_hook)
    try:
        compile(patched, str(proton), "exec")
    except SyntaxError as e:
        fail(f"generated shadow Proton launcher is invalid: {e}")

    proton.write_text(patched, encoding="utf-8")

def build_shadow(mode, proton_root, shadow, artifacts, layout):
    shutil.copytree(
        proton_root,
        shadow,
        symlinks=True,
        copy_function=link_regular_file,
        dirs_exist_ok=True,
    )

    replace_with_copy(proton_root / "proton", shadow / "proton")
    version = proton_root / "version"
    if version.is_file():
        replace_with_copy(version, shadow / "version")

    dist_lock = shadow / "dist.lock"
    if os.path.lexists(dist_lock):
        dist_lock.unlink()
    dist_lock.touch()

    # Proton 9+ removes a legacy dist/ tree during startup. Do not leave an
    # old dist symlink pointing back into the Steam-managed installation.
    legacy_dist = shadow / "dist"
    if legacy_dist.is_symlink():
        legacy_dist.unlink()

    prepare_steampipe_state(proton_root, shadow)

    shadow_nptdll = shadow_path(shadow, proton_root, layout["npt_windows"]) / "nptunix.dll"
    shadow_nptso = shadow_path(shadow, proton_root, layout["npt_unix"]) / "nptunix.so"

    replacements = [
        (artifacts[name], shadow_path(shadow, proton_root, layout[name]), f"{name}.dll destination")
        for name in neptune_dlls(mode)
    ]
    replacements += [
        (artifacts["nptdll"], shadow_nptdll, "nptunix.dll destination"),
        (artifacts["nptso"], shadow_nptso, "nptunix.so destination"),
    ]

    for _, dst, kind in replacements:
        require_shadow_local(shadow, dst, kind)

    for src, dst, _ in replacements:
        replace_with_copy(src, dst)
    patch_shadow_proton_for_nptunix(
        shadow,
        shadow_nptdll.relative_to(shadow),
        graphics_policy(mode),
    )

    info = [
        f"schema={SCHEMA}",
        f"mode={mode}",
        f"proton={proton_root}",
        *(f"{name}={layout[name].relative_to(proton_root)}" for name in neptune_dlls(mode)),
        f"wine64={layout['npt_windows'].parent.relative_to(proton_root)}",
    ]
    (shadow / ".varmint-info").write_text("\n".join(info) + "\n", encoding="utf-8")
    (shadow / ".use.lock").touch()
    (shadow / ".varmint-complete").touch()


def read_current(mode_dir):
    try:
        value = (mode_dir / "current").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    return value if re.fullmatch(r"[0-9a-f]{64}", value) else None


def write_current(mode_dir, value):
    fd, temp_name = tempfile.mkstemp(prefix=".current-", dir=mode_dir, text=True)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(value + "\n")
            f.flush()
            os.fsync(f.fileno())
        os.replace(temp_name, mode_dir / "current")
    finally:
        try:
            os.unlink(temp_name)
        except FileNotFoundError:
            pass


def gc_generations(mode_dir, current):
    for entry in mode_dir.iterdir():
        if not entry.is_dir() or not re.fullmatch(r"[0-9a-f]{64}", entry.name):
            continue
        if entry.name == current:
            continue

        lock_path = entry / ".use.lock"
        try:
            lock = open(lock_path, "a+")
        except OSError:
            shutil.rmtree(entry, ignore_errors=True)
            continue

        try:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                continue
            shutil.rmtree(entry, ignore_errors=True)
        finally:
            lock.close()

    for entry in mode_dir.iterdir():
        if entry.is_dir() and entry.name.startswith(".build-"):
            shutil.rmtree(entry, ignore_errors=True)


def acquire_shadow(mode, proton_root, artifacts, layout):
    cache_root = Path(os.environ.get("VARMINT_GRAPHICS_CACHE", str(DEFAULT_CACHE))).expanduser()
    mode_dir = cache_root / proton_slot_id(proton_root) / mode
    mode_dir.mkdir(parents=True, exist_ok=True)

    wanted = fingerprint(mode, proton_root, artifacts, layout)
    generation = mode_dir / wanted
    build_lock_path = mode_dir / ".build.lock"

    with open(build_lock_path, "a+") as build_lock:
        fcntl.flock(build_lock, fcntl.LOCK_EX)

        complete = generation / ".varmint-complete"
        if not complete.is_file():
            if generation.exists():
                shutil.rmtree(generation)
            temp = Path(tempfile.mkdtemp(prefix=".build-", dir=mode_dir))
            try:
                build_shadow(mode, proton_root, temp, artifacts, layout)
                os.rename(temp, generation)
            except Exception:
                shutil.rmtree(temp, ignore_errors=True)
                raise

        use_lock = open(generation / ".use.lock", "a+")
        fcntl.flock(use_lock, fcntl.LOCK_SH)

        write_current(mode_dir, wanted)
        gc_generations(mode_dir, wanted)

    return generation, use_lock, mode_dir


def cleanup_after_run(mode_dir, use_lock):
    use_lock.close()
    build_lock_path = mode_dir / ".build.lock"
    try:
        with open(build_lock_path, "a+") as build_lock:
            fcntl.flock(build_lock, fcntl.LOCK_EX)
            current = read_current(mode_dir)
            if current:
                gc_generations(mode_dir, current)
    except OSError:
        pass


def normalize_override_name(name):
    name = name.strip().lower().replace("\\", "/").rsplit("/", 1)[-1]
    name = name.lstrip("*")
    if name.endswith(".dll"):
        name = name[:-4]
    return name


def policy_dll_names(policy):
    names = set()
    for entry in policy.split(";"):
        modules, sep, _order = entry.partition("=")
        if not sep:
            continue
        for name in re.split(r"[, \t]+", modules.strip()):
            if name:
                names.add(normalize_override_name(name))
    return names

def run_child(command, env, mode_dir, use_lock):
    child = subprocess.Popen(command, env=env)

    def forward(signum, _frame):
        if child.poll() is None:
            try:
                child.send_signal(signum)
            except ProcessLookupError:
                pass

    old_handlers = {}
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        old_handlers[signum] = signal.signal(signum, forward)

    try:
        return child.wait()
    finally:
        for signum, handler in old_handlers.items():
            signal.signal(signum, handler)
        cleanup_after_run(mode_dir, use_lock)


def run_plain_child(command, env):
    child = subprocess.Popen(command, env=env)

    def forward(signum, _frame):
        if child.poll() is None:
            try:
                child.send_signal(signum)
            except ProcessLookupError:
                pass

    old_handlers = {}
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        old_handlers[signum] = signal.signal(signum, forward)
    try:
        return child.wait()
    finally:
        for signum, handler in old_handlers.items():
            signal.signal(signum, handler)


def claim_diagnostic():
    if claim_from_environment is None:
        return None
    try:
        return claim_from_environment()
    except Exception as error:
        print(f"varmint-graphics: could not start diagnostic capture: {error}", file=sys.stderr)
        return None


def complete_diagnostic(diagnostic, **result):
    try:
        diagnostic.complete(**result)
    except Exception as error:
        print(f"varmint-graphics: could not finish diagnostic capture: {error}", file=sys.stderr)


def run_with_diagnostics(command, env, runner, diagnostic):
    env.update(diagnostic.environment())
    try:
        status = runner(command, env)
    except BaseException as error:
        complete_diagnostic(diagnostic, error=f"{type(error).__name__}: {error}")
        raise
    complete_diagnostic(diagnostic, exit_code=status)
    return status


def main():
    if len(sys.argv) < 3:
        fail("usage: varmint-graphics <venus|neptune|neptune-dx12> <command> [args...]")

    mode = sys.argv[1]
    command = sys.argv[2:]

    if mode == "venus":
        diagnostic = claim_diagnostic()
        if diagnostic:
            raise SystemExit(run_with_diagnostics(
                command,
                os.environ.copy(),
                run_plain_child,
                diagnostic,
            ))
        os.execvpe(command[0], command, os.environ.copy())

    if mode not in ("neptune", "neptune-dx12"):
        fail(f"unsupported graphics mode: {mode}")

    proton_index, proton_root = find_proton(command)
    arm64 = proton_uses_arm64(proton_root)
    layout = discover_proton_layout(mode, proton_root, arm64)
    unix_arch = layout["npt_unix"].name

    build = os.environ.get("VARMINT_NEPTUNE_BUILD")
    if build:
        artifacts = build_artifact_paths(
            mode, Path(build).expanduser().resolve(), unix_arch
        )
    else:
        base = Path(
            os.environ.get("VARMINT_NEPTUNE_PREBUILT", str(DEFAULT_NEPTUNE))
        ).expanduser()
        artifacts = prebuilt_artifact_paths(mode, base, unix_arch)

    shadow, use_lock, mode_dir = acquire_shadow(
        mode, proton_root, artifacts, layout
    )

    command[proton_index] = str(shadow / "proton")

    env = os.environ.copy()

    # Make prefix setup copy the translation DLLs that the shadow replaced.
    # PROTON_USE_WINED3D being present with value 0 also suppresses the legacy
    # PROTON_USE_WINED3D11 alias in Proton 7+.
    env["PROTON_USE_WINED3D"] = "0"

    # Pin Proton to the Wine runtime the shadow was built for.
    env["PROTON_USE_ARM64"] = "1" if arm64 else "0"

    # Neptune does not implement NVAPI. Proton 7/8 use ENABLE_NVAPI, while
    # Proton 9+ use DISABLE_NVAPI/FORCE_NVAPI. Set both generations explicitly
    # so app-specific Proton workarounds cannot turn NVAPI back on.
    env["PROTON_ENABLE_NVAPI"] = "0"
    env["PROTON_DISABLE_NVAPI"] = "1"
    env["PROTON_FORCE_NVAPI"] = "0"

    # Both modes provide D3D11; an app-specific Proton workaround may
    # otherwise disable it.
    env["PROTON_NO_D3D11"] = "0"
    diagnostic = claim_diagnostic()
    if diagnostic:
        raise SystemExit(run_with_diagnostics(
            command,
            env,
            lambda child, child_env: run_child(child, child_env, mode_dir, use_lock),
            diagnostic,
        ))
    raise SystemExit(run_child(command, env, mode_dir, use_lock))


if __name__ == "__main__":
    main()

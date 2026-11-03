"""One-shot diagnostic capture and deterministic report generation for Vet."""

from collections import deque
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import tempfile
import uuid


STATE_ROOT = Path(
    os.environ.get(
        "VARMINT_VET_STATE_DIR",
        Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
        / "varmint/vet",
    )
)
DIAGNOSTIC_ROOT = STATE_ROOT / "diagnostics"
FULL_LOG_LIMIT = 12 * 1024 * 1024
CONTEXT_BEFORE = 4
CONTEXT_AFTER = 6
MAX_EXCERPTS = 16
MAX_CONTEXT_LINES = 240
MAX_CANDIDATES = 128
MAX_REPORT_LINE = 1200


class DiagnosticError(Exception):
    pass


def timestamp():
    return datetime.now(timezone.utc).isoformat()


def atomic_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
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


def _read_json(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError, TypeError):
        return None


def _validate_appid(appid):
    if (
        isinstance(appid, bool)
        or not isinstance(appid, int)
        or appid <= 0
        or appid > 0xFFFFFFFF
    ):
        raise DiagnosticError("invalid Steam AppID")
    return appid


def _redact(text):
    text = re.sub(r"/home/[^/\s]+", "~", text)
    text = re.sub(r"/Users/[^/\s]+", "~", text)
    text = re.sub(r"(?<=/userdata/)\d+", "[steam-user]", text)
    return text


def _line(value):
    value = _redact(value.rstrip("\r\n"))
    if len(value) > MAX_REPORT_LINE:
        return value[:MAX_REPORT_LINE] + " … [line truncated]"
    return value


def _signal_priority(line):
    value = line.casefold()
    if "gameoverlayrenderer.so" in value and "ignored" in value:
        return 0
    if any(
        marker in value
        for marker in (
            "handled unaligned atomic",
            "handled self-modifying code",
            "handled inline self-modifying code",
            "exception_wine_name_thread",
        )
    ):
        return 0
    if re.search(
        r"(?:directx|dx(?:11|12)|d3d(?:11|12)).*(?:not supported|unsupported|requires)",
        value,
    ):
        return 110
    if "c06d007e" in value:
        return 105
    if any(
        marker in value
        for marker in (
            "unhandled exception",
            "fatal",
            "panic",
            "assertion failed",
            "dxgi_error",
            "vk_error",
            "device lost",
            "backtrace: --- exception",
        )
    ):
        return 100
    if "err:" in value:
        return 70
    if "exception: code:" in value:
        return 55
    if "neptune:" in value:
        return 45
    if re.search(r"load(?:ed)? module (?:d3d9|d3d11|d3d12|dxgi)\.dll", value):
        return 35
    if "fex: loaded" in value:
        return 20
    return 0


def _signal_key(line):
    value = line.casefold()
    for code in re.findall(
        r"(?<![0-9a-f])(?:c06d007e|c0000005|80000002)(?![0-9a-f])", value
    ):
        return f"exception:{code}"
    value = re.sub(r"0x[0-9a-f]{6,}", "<id>", value)
    value = re.sub(r"\b[0-9a-f]{12,}\b", "<id>", value)
    return value


def _diagnostic_analysis(completed, extracted):
    signals = extracted.get("signals", [])
    combined = "\n".join(signals).casefold()
    detected = []
    possible_explanations = []
    if "c06d007e" in combined:
        detected.append("Exception 0xc06d007e appears in the captured Proton log.")
        possible_explanations.append(
            "0xc06d007e commonly indicates an MSVC delay-load failure: a delayed DLL could not be loaded. "
            "The captured lines do not identify that DLL or prove which process caused the failure."
        )
    if "c0000005" in combined:
        detected.append(
            "Exception 0xc0000005 (access violation) appears in the captured Proton log."
        )
        possible_explanations.append(
            "An access violation means a process accessed invalid memory, but the exception address alone "
            "does not identify the underlying cause."
        )
    if "c0000409" in combined:
        detected.append(
            "Exception 0xc0000409 (fail-fast) appears in the captured Proton log."
        )
        possible_explanations.append(
            "A fail-fast exception may come from the game or a helper process. The captured exception code "
            "alone does not identify the original failure."
        )
    loaded_d3d11 = extracted.get("observed_d3d11", False)
    loaded_d3d12 = extracted.get("observed_d3d12", False)
    if loaded_d3d11 and not loaded_d3d12:
        detected.append(
            "A d3d11.dll load was observed; no d3d12.dll load was observed."
        )
        possible_explanations.append(
            "This may indicate Direct3D 11 activity, but loading d3d11.dll does not prove that the game used "
            "Direct3D 11 for rendering. Games and helper processes can load graphics DLLs for other reasons."
        )
    elif loaded_d3d11 and loaded_d3d12:
        detected.append("Both d3d11.dll and d3d12.dll loads were observed.")
    elif loaded_d3d12:
        detected.append(
            "A d3d12.dll load was observed; no d3d11.dll load was observed."
        )
    exit_code = completed.get("exit_code")
    if isinstance(exit_code, int) and exit_code < 0:
        detected.append(f"The launch wrapper ended from signal {-exit_code}.")
        possible_explanations.append(
            "A wrapper signal can result from process cleanup or external termination and does not by itself "
            "prove that the game crashed."
        )
    if not detected:
        detected.append(
            "Vet did not recognize a specific failure signature in the captured Proton log."
        )
    if not possible_explanations:
        possible_explanations.append(
            "Vet does not have enough evidence to suggest a likely cause from the recognized details."
        )
    return detected, possible_explanations


def _guest_description():
    name = "Linux guest"
    try:
        for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
            if line.startswith("PRETTY_NAME="):
                name = line.split("=", 1)[1].strip().strip('"')
                break
    except OSError:
        pass
    return f"{name} · Linux {platform.release()} · {platform.machine()}"


class DiagnosticClaim:
    def __init__(self, directory, request):
        self.directory = Path(directory)
        self.request = request

    def environment(self):
        return {
            "PROTON_LOG": "1",
            "PROTON_LOG_DIR": str(self.directory),
        }

    def complete(self, exit_code=None, error=None):
        value = {
            "completed_at": timestamp(),
            "exit_code": exit_code,
            "error": error,
        }
        atomic_json(self.directory / "completed.json", value)


class DiagnosticStore:
    def __init__(self, root=None):
        self.root = Path(root or DIAGNOSTIC_ROOT)

    def arm(self, state):
        appid = _validate_appid(state["appid"])
        armed = self.root / "armed" / f"{appid}.json"
        if armed.exists():
            existing = _read_json(armed)
            if existing:
                return existing
        run_id = str(uuid.uuid4())
        request = {
            "run_id": run_id,
            "appid": appid,
            "game": state.get("name") or f"Steam App {appid}",
            "armed_at": timestamp(),
            "graphics_mode": state.get("graphics_mode"),
            "host_backend": state.get("effective_host_backend"),
            "proton": state.get("effective_proton"),
            "launch_options": state.get("launch_options", ""),
            "build_id": state.get("build_id"),
        }
        atomic_json(armed, request)
        return request

    def cancel(self, appid):
        path = self.root / "armed" / f"{_validate_appid(appid)}.json"
        try:
            path.unlink()
            return True
        except FileNotFoundError:
            return False

    def claim(self, appid):
        appid = _validate_appid(appid)
        armed = self.root / "armed" / f"{appid}.json"
        request = _read_json(armed)
        if not request or request.get("appid") != appid or not request.get("run_id"):
            return None
        directory = self.root / "runs" / request["run_id"]
        directory.mkdir(parents=True, exist_ok=True)
        claimed = directory / "request.json"
        try:
            os.replace(armed, claimed)
        except FileNotFoundError:
            try:
                directory.rmdir()
            except OSError:
                pass
            return None
        atomic_json(directory / "started.json", {"started_at": timestamp()})
        return DiagnosticClaim(directory, request)

    def pending(self, appid):
        return _read_json(self.root / "armed" / f"{_validate_appid(appid)}.json")

    def run(self, run_id):
        if not re.fullmatch(r"[0-9a-f-]{36}", str(run_id)):
            raise DiagnosticError("invalid diagnostic run ID")
        directory = self.root / "runs" / str(run_id)
        request = _read_json(directory / "request.json")
        if not request:
            raise DiagnosticError("diagnostic run was not found")
        return {
            "directory": directory,
            "request": request,
            "started": _read_json(directory / "started.json"),
            "completed": _read_json(directory / "completed.json"),
            "notified": (directory / "notified").exists(),
        }

    def runs(self, appid=None, completed_only=False):
        appid = _validate_appid(appid) if appid is not None else None
        result = []
        for request_path in (self.root / "runs").glob("*/request.json"):
            request = _read_json(request_path)
            if not request or (appid is not None and request.get("appid") != appid):
                continue
            run = self.run(request["run_id"])
            if completed_only and not run["completed"]:
                continue
            result.append(run)
        return sorted(
            result,
            key=lambda run: (run["started"] or {}).get(
                "started_at", run["request"].get("armed_at", "")
            ),
            reverse=True,
        )

    def status(self, appid):
        pending = self.pending(appid)
        runs = self.runs(appid)
        latest = runs[0] if runs else None
        return {
            "pending": pending,
            "latest_run_id": latest["request"]["run_id"] if latest else None,
            "latest_completed": bool(latest and latest["completed"]),
            "latest_request": latest["request"] if latest else None,
            "latest_completed_at": latest["completed"].get("completed_at")
            if latest and latest["completed"]
            else None,
        }

    def completed_unnotified(self):
        return [run for run in self.runs(completed_only=True) if not run["notified"]]

    def mark_notified(self, run_id):
        run = self.run(run_id)
        path = run["directory"] / "notified"
        path.touch(mode=0o600, exist_ok=True)

    def extract(self, run_id):
        run = self.run(run_id)
        logs = sorted(
            run["directory"].glob("steam-*.log"), key=lambda path: path.stat().st_mtime
        )
        if not logs:
            return {
                "text": "No Proton log was produced.",
                "size": 0,
                "files": [],
                "signals": [],
                "observed_d3d11": False,
                "observed_d3d12": False,
                "description": "no Proton log",
            }
        size = sum(path.stat().st_size for path in logs)
        if size <= FULL_LOG_LIMIT:
            sections = []
            detected = []
            seen = set()
            observed_d3d11 = False
            observed_d3d12 = False
            for path in logs:
                try:
                    with path.open("r", encoding="utf-8", errors="replace") as stream:
                        lines = [_line(line) for line in stream]
                except OSError:
                    continue
                heading = f"--- complete log: {path.name} ---"
                sections.append(heading + ("\n" + "\n".join(lines) if lines else ""))
                for line in lines:
                    value = line.casefold()
                    observed_d3d11 |= "d3d11.dll" in value
                    observed_d3d12 |= "d3d12.dll" in value
                    priority = _signal_priority(line)
                    key = _signal_key(line) if priority else None
                    if priority and key not in seen:
                        seen.add(key)
                        detected.append((priority, line))
            return {
                "text": "\n\n".join(sections) or "The Proton log was empty.",
                "size": size,
                "files": [path.name for path in logs],
                "signals": [
                    line
                    for _priority, line in sorted(detected, reverse=True)[:MAX_EXCERPTS]
                ],
                "observed_d3d11": observed_d3d11,
                "observed_d3d12": observed_d3d12,
                "description": f"complete chronological log (within the {FULL_LOG_LIMIT // (1024 * 1024)} MiB report budget)",
            }
        head = []
        tail = deque(maxlen=30)
        candidates = []
        seen = set()
        observed_d3d11 = False
        observed_d3d12 = False
        for file_index, path in enumerate(logs):
            before = deque(maxlen=CONTEXT_BEFORE)
            pending = []
            try:
                with path.open("r", encoding="utf-8", errors="replace") as stream:
                    for number, raw in enumerate(stream, 1):
                        line = _line(raw)
                        located = f"[{path.name}:{number}] {line}"
                        value = line.casefold()
                        observed_d3d11 |= "d3d11.dll" in value
                        observed_d3d12 |= "d3d12.dll" in value
                        if len(head) < 20:
                            head.append(located)
                        tail.append(located)
                        priority = _signal_priority(line)
                        key = _signal_key(line) if priority else None
                        duplicate = priority and key in seen
                        if not duplicate:
                            for candidate in pending:
                                candidate["lines"].append(located)
                                candidate["end"] = number
                                candidate["remaining"] -= 1
                            pending = [
                                candidate
                                for candidate in pending
                                if candidate["remaining"] > 0
                            ]
                        if priority and not duplicate:
                            seen.add(key)
                            candidate = {
                                "priority": priority,
                                "file_index": file_index,
                                "start": max(1, number - len(before)),
                                "end": number,
                                "lines": [*before, located],
                                "remaining": CONTEXT_AFTER,
                                "signal": line,
                            }
                            candidates.append(candidate)
                            pending.append(candidate)
                            if len(candidates) > MAX_CANDIDATES:
                                lowest = min(
                                    candidates,
                                    key=lambda item: (
                                        item["priority"],
                                        item["file_index"],
                                        item["start"],
                                    ),
                                )
                                candidates.remove(lowest)
                                pending = [
                                    item for item in pending if item is not lowest
                                ]
                        if not duplicate:
                            before.append(located)
            except OSError:
                continue
        selected = sorted(
            candidates,
            key=lambda item: (item["priority"], item["file_index"], item["start"]),
            reverse=True,
        )[:MAX_EXCERPTS]
        selected.sort(key=lambda item: (item["file_index"], item["start"]))
        excerpts = []
        signals = []
        context_lines = 0
        for candidate in selected:
            if context_lines >= MAX_CONTEXT_LINES:
                break
            available = MAX_CONTEXT_LINES - context_lines
            lines = candidate["lines"][:available]
            excerpts.append(lines)
            signals.append(candidate["signal"])
            context_lines += len(lines)
        sections = []
        if head:
            sections.append("--- log start (first 20 lines) ---\n" + "\n".join(head))
        if excerpts:
            sections.append(
                "--- relevant excerpts with context ---\n"
                + "\n...\n".join("\n".join(excerpt) for excerpt in excerpts if excerpt)
            )
        if tail:
            sections.append("--- log end (last 30 lines) ---\n" + "\n".join(tail))
        return {
            "text": "\n\n".join(sections) or "The Proton log was empty.",
            "size": size,
            "files": [path.name for path in logs],
            "signals": signals,
            "observed_d3d11": observed_d3d11,
            "observed_d3d12": observed_d3d12,
            "description": (
                f"first 20 lines, last 30 lines, and the {MAX_EXCERPTS} highest-priority "
                f"excerpts with {CONTEXT_BEFORE} lines before and {CONTEXT_AFTER} after"
            ),
        }

    def report(self, run_id):
        run = self.run(run_id)
        request = run["request"]
        completed = run["completed"] or {}
        started = run["started"] or {}
        extracted = self.extract(run_id)
        size = extracted["size"]
        size_text = (
            f"{size / (1024 * 1024):.1f} MiB"
            if size >= 1024 * 1024
            else f"{size / 1024:.1f} KiB"
        )
        exit_code = completed.get("exit_code")
        exit_text = "unknown" if exit_code is None else str(exit_code)
        detected, possible_explanations = _diagnostic_analysis(completed, extracted)
        detected_text = "\n".join(f"- {finding}" for finding in detected)
        explanations_text = "\n".join(
            f"- {explanation}" for explanation in possible_explanations
        )
        return _redact(f"""# Varmint compatibility report

## Game

- Name: {request["game"]}
- Steam AppID: {request["appid"]}
- Graphics: {request.get("graphics_mode") or "Steam default"}
- Host graphics backend: {request.get("host_backend") or "Not reported"}
- Proton: {request.get("proton") or "Steam default"}
- Launch Options: `{request.get("launch_options") or "(empty)"}`
- Guest: {_guest_description()}

## What happened

<!-- Describe the problem and the last visible screen or error. -->

## Expected behavior

<!-- Describe what you expected to happen. -->

## Diagnostic run

- Started: {started.get("started_at", "unknown")}
- Completed: {completed.get("completed_at", "unknown")}
- Exit status: {exit_text}
- Proton log size: {size_text}
- Included log data: {extracted["description"]}

## Vet analysis

### Detected

{detected_text}

### Possible explanations

These are hypotheses based on the detected details and may be wrong.

{explanations_text}

## Relevant Proton log

```text
{extracted["text"]}
```
""")


def claim_from_environment(store=None):
    value = os.environ.get("SteamAppId") or os.environ.get("SteamGameId")
    try:
        appid = int(value or "")
    except ValueError:
        return None
    return (store or DiagnosticStore()).claim(appid)

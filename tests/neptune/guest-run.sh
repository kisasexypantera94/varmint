#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_ROOT="${VARMINT_NEPTUNE_TEST_BUILD:-$HOME/.cache/varmint/neptune-tests}"
NEPTUNE_BUILD="${VARMINT_NEPTUNE_BUILD:-$HOME/virtio-win-mesa-neptune/builddir-win64-x86unix/src/virtio/neptune}"
CXX="${VARMINT_MINGW_CXX:-x86_64-w64-mingw32-g++}"
STEAM_ROOT="${VARMINT_STEAM_ROOT:-$HOME/.local/share/Steam}"
FEX_COMPAT_TOOL="$STEAM_ROOT/steamapps/common/FEX-Emu/fex-compat-tool"
TIMEOUT="${VARMINT_NEPTUNE_TEST_TIMEOUT:-45s}"

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <d3d11|d3d12> <smoke|fullscreen|empty_cs>" >&2
    exit 2
fi

API="$1"
TEST="$2"

case "$API:$TEST" in
    d3d11:smoke)
        MODE=neptune
        LIBRARIES=(-ld3d11)
        ;;
    d3d12:smoke)
        MODE=neptune-dx12
        LIBRARIES=(-ld3d12)
        ;;
    d3d12:fullscreen)
        MODE=neptune-dx12
        LIBRARIES=(-ld3d12 -ldxgi)
        ;;
    d3d12:empty_cs)
        MODE=neptune-dx12
        LIBRARIES=(-ld3d12)
        ;;
    *)
        echo "unsupported Neptune test: $API $TEST" >&2
        exit 2
        ;;
esac

command -v "$CXX" >/dev/null || {
    echo "missing guest MinGW compiler: $CXX" >&2
    exit 1
}
command -v varmint-graphics >/dev/null || {
    echo "missing guest runner: varmint-graphics" >&2
    exit 1
}
test -d "$NEPTUNE_BUILD" || {
    echo "missing Neptune development build: $NEPTUNE_BUILD" >&2
    exit 1
}

if [ -n "${VARMINT_TEST_PROTON:-}" ]; then
    PROTON="$VARMINT_TEST_PROTON"
elif [ -f "$STEAM_ROOT/steamapps/common/Proton - Experimental/proton" ]; then
    PROTON="$STEAM_ROOT/steamapps/common/Proton - Experimental/proton"
else
    mapfile -t proton_candidates < <(
        find "$STEAM_ROOT/steamapps/common" -maxdepth 2 -type f -name proton -print 2>/dev/null | sort
    )
    if [ "${#proton_candidates[@]}" -ne 1 ]; then
        echo "could not select Proton; set VARMINT_TEST_PROTON to a Proton launcher" >&2
        exit 1
    fi
    PROTON="${proton_candidates[0]}"
fi

test -f "$PROTON" || {
    echo "missing Proton launcher: $PROTON" >&2
    exit 1
}
# ARM64 Proton runs natively; x86_64 Proton runs through FEX.
if [ -d "$(dirname "$PROTON")/files/bin-arm64" ]; then
    WINESERVER="$(dirname "$PROTON")/files/bin-arm64/wineserver"
    RUNNER=()
    COMPATDATA="$BUILD_ROOT/compatdata-$API-arm64"
else
    WINESERVER="$(dirname "$PROTON")/files/bin/wineserver"
    test -x "$FEX_COMPAT_TOOL" || {
        echo "missing FEX compatibility tool: $FEX_COMPAT_TOOL" >&2
        exit 1
    }
    RUNNER=("$FEX_COMPAT_TOOL" run)
    COMPATDATA="$BUILD_ROOT/compatdata-$API"
fi
test -x "$WINESERVER" || {
    echo "missing Proton wineserver: $WINESERVER" >&2
    exit 1
}

# Swap chain tests need the desktop session's display; SSH has none, but
# the session exports it to the systemd user manager.
if [ -z "${DISPLAY:-}" ]; then
    while IFS= read -r var; do
        case "$var" in
            DISPLAY=* | WAYLAND_DISPLAY=* | XAUTHORITY=*) export "$var" ;;
        esac
    done < <(systemctl --user show-environment 2>/dev/null)
fi

mkdir -p "$BUILD_ROOT/bin" "$COMPATDATA"
EXE="$BUILD_ROOT/bin/$API-$TEST.exe"

echo "building $API $TEST" >&2
# GUI subsystem, like a game. Wine gives a console executable a windowed
# conhost, which cannot start without a display (e.g. over SSH), and every
# console write then blocks until the run is killed.
"$CXX" \
    -std=c++17 \
    -O2 \
    -Wall \
    -Wextra \
    -static-libgcc \
    -static-libstdc++ \
    -mwindows \
    -I"$ROOT/common" \
    "$ROOT/$API/$TEST.cpp" \
    -o "$EXE" \
    "${LIBRARIES[@]}"

echo "running $API $TEST through $MODE" >&2
export VARMINT_NEPTUNE_BUILD="$NEPTUNE_BUILD"
export STEAM_COMPAT_CLIENT_INSTALL_PATH="$STEAM_ROOT"
export STEAM_COMPAT_DATA_PATH="$COMPATDATA"
export STEAM_FEX_TSOENABLED="${STEAM_FEX_TSOENABLED:-0}"
export PROTON_LOG=1
export PROTON_LOG_DIR="$BUILD_ROOT"
export WINEDEBUG=+debugstr
case "$API" in
    d3d11) export SteamGameId=9000011 ;;
    d3d12) export SteamGameId=9000012 ;;
esac
STDOUT_LOG="$BUILD_ROOT/$API-$TEST.stdout.log"
STDERR_LOG="$BUILD_ROOT/steam-$SteamGameId.log"
LAUNCH_STDERR_LOG="$BUILD_ROOT/$API-$TEST.launch.stderr.log"

valid_result() {
    python3 -c '
import json
import pathlib
import sys

lines = pathlib.Path(sys.argv[1]).read_text().splitlines()
result = json.loads(lines[0]) if len(lines) == 1 else {}
required = {"test", "api", "elapsed_ms", "checksum", "status"}
valid = required <= result.keys()
valid = valid and result["test"] == sys.argv[3]
valid = valid and result["api"] == sys.argv[2] and result["status"] == "pass"
raise SystemExit(0 if valid else 1)
' "$1" "$API" "$TEST" 2>/dev/null
}

capture_result() {
    python3 -c '
import ast
import json
import pathlib
import re
import sys

for line in pathlib.Path(sys.argv[1]).read_text(errors="replace").splitlines():
    match = re.search(r"OutputDebugStringA (\"(?:[^\"\\]|\\.)*\")", line)
    if match:
        line = ast.literal_eval(match.group(1)).rstrip("\n")
    try:
        result = json.loads(line)
    except json.JSONDecodeError:
        continue
    if result.get("test") == sys.argv[3] and result.get("api") == sys.argv[2]:
        print(line)
        break
' "$STDERR_LOG" "$API" "$TEST" >"$STDOUT_LOG"
}

terminate_run() {
    kill -TERM "$run_pid" 2>/dev/null || true
    for _ in {1..50}; do
        kill -0 "$run_pid" 2>/dev/null || break
        sleep 0.1
    done
    kill -KILL "$run_pid" 2>/dev/null || true
    wait "$run_pid" 2>/dev/null || true
}

terminate_prefix() {
    if [ -d "$STEAM_COMPAT_DATA_PATH/pfx" ]; then
        WINEPREFIX="$STEAM_COMPAT_DATA_PATH/pfx" \
            timeout --signal=TERM --kill-after=2s 5s \
            "${RUNNER[@]}" "$WINESERVER" -k \
            >/dev/null 2>&1 || true
    fi
}

: >"$STDOUT_LOG"
: >"$STDERR_LOG"
: >"$LAUNCH_STDERR_LOG"
timeout "$TIMEOUT" \
    varmint-graphics "$MODE" \
        "${RUNNER[@]}" "$PROTON" run "$EXE" \
        2>"$LAUNCH_STDERR_LOG" &
run_pid=$!

while [ -n "$(jobs -pr)" ]; do
    if [ -s "$STDERR_LOG" ]; then
        capture_result
        if [ -s "$STDOUT_LOG" ] && valid_result "$STDOUT_LOG"; then
            cat "$STDOUT_LOG"
            terminate_prefix
            terminate_run
            exit 0
        fi
    fi
    sleep 0.1
done

if wait "$run_pid"; then
    status=0
else
    status=$?
fi

for _ in {1..50}; do
    if [ -s "$STDERR_LOG" ]; then
        capture_result
        if [ -s "$STDOUT_LOG" ] && valid_result "$STDOUT_LOG"; then
            break
        fi
    fi
    sleep 0.1
done

if [ -s "$STDOUT_LOG" ] && valid_result "$STDOUT_LOG"; then
    cat "$STDOUT_LOG"
    terminate_prefix
    status=0
else
    terminate_prefix
    if [ "$status" -eq 0 ]; then
        status=1
    fi
fi

if [ "$status" -eq 0 ]; then
    exit 0
fi

if [ -s "$STDOUT_LOG" ]; then
    echo "invalid stdout from $API $TEST:" >&2
    cat "$STDOUT_LOG" >&2
fi
if [ -s "$STDERR_LOG" ]; then
    echo "captured stderr from $API $TEST:" >&2
    cat "$STDERR_LOG" >&2
fi
if [ -s "$LAUNCH_STDERR_LOG" ]; then
    echo "captured launcher stderr from $API $TEST:" >&2
    cat "$LAUNCH_STDERR_LOG" >&2
fi

exit "$status"

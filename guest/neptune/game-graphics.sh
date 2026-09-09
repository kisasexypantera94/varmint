#!/usr/bin/env bash
set -euo pipefail

APPID="${1:-}"
MODE="${2:-status}"
EXE_REL="${3:-}"

usage() {
    cat >&2 <<EOF_USAGE
usage: $(basename "$0") <steam-appid> <status|venus|neptune> [relative/path/to/Game.exe]
EOF_USAGE
}

[ -n "$APPID" ] || {
    usage
    exit 2
}

case "$MODE" in
    status|venus|neptune|neptune-dx12) ;;
    *)
        usage
        exit 2
        ;;
esac

REPO="${VARMINT_NEPTUNE_REPO:-$HOME/virtio-win-mesa-neptune}"
BUILD="${VARMINT_NEPTUNE_BUILD:-$REPO/builddir-win64-x86unix/src/virtio/neptune}"
PREBUILT="${VARMINT_NEPTUNE_PREBUILT:-/usr/local/lib/varmint/neptune}"
STEAM="${VARMINT_STEAM_ROOT:-$HOME/.local/share/Steam}"
RUNTIME="${VARMINT_NEPTUNE_RUNTIME:-$HOME/.local/share/varmint/neptune}"
STATE_DIR="${VARMINT_GAME_GRAPHICS_STATE:-$HOME/.local/share/varmint/game-graphics}"
STATE="$STATE_DIR/$APPID"

MANIFEST="$STEAM/steamapps/appmanifest_${APPID}.acf"
test -f "$MANIFEST" || {
    echo "missing Steam manifest: $MANIFEST" >&2
    exit 1
}

INSTALLDIR="$(
    sed -nE 's/^[[:space:]]*"installdir"[[:space:]]+"([^"]+)".*/\1/p' "$MANIFEST" |
    head -1
)"
[ -n "$INSTALLDIR" ] || {
    echo "cannot determine installdir from $MANIFEST" >&2
    exit 1
}

GAME="$STEAM/steamapps/common/$INSTALLDIR"
test -d "$GAME" || {
    echo "missing game directory: $GAME" >&2
    exit 1
}

resolve_target_dir() {
    if [ -z "$EXE_REL" ]; then
        if state_read; then
            case "$STATE_TARGET" in
                "$GAME"|"$GAME"/*) ;;
                *)
                    echo "saved target is outside the current Steam install: $STATE_TARGET" >&2
                    echo "specify the game executable explicitly to choose a new target" >&2
                    exit 1
                    ;;
            esac

            test -d "$STATE_TARGET" || {
                echo "saved target no longer exists: $STATE_TARGET" >&2
                echo "specify the game executable explicitly to choose a new target" >&2
                exit 1
            }

            printf '%s\n' "$STATE_TARGET"
            return
        fi

        printf '%s\n' "$GAME"
        return
    fi

    case "$EXE_REL" in
        /*|../*|*/../*|*/..)
            echo "exe path must be relative to the Steam install root: $EXE_REL" >&2
            exit 2
            ;;
    esac

    local exe="$GAME/$EXE_REL"
    test -f "$exe" || {
        echo "missing exe: $exe" >&2
        exit 1
    }
    dirname "$exe"
}

source_paths() {
    if [ -f "$BUILD/d3d11.dll" ] &&
       [ -f "$BUILD/dxgi.dll" ] &&
       [ -f "$BUILD/nptunix/nptunix.dll" ] &&
       [ -f "$BUILD/nptunix/nptunix.so" ]; then
        D3D11="$BUILD/d3d11.dll"
        D3D12="$BUILD/d3d12.dll"
        DXGI="$BUILD/dxgi.dll"
        NPTDLL="$BUILD/nptunix/nptunix.dll"
        NPTSO="$BUILD/nptunix/nptunix.so"
    else
        D3D11="$PREBUILT/x86_64-windows/d3d11.dll"
        D3D12="$PREBUILT/x86_64-windows/d3d12.dll"
        DXGI="$PREBUILT/x86_64-windows/dxgi.dll"
        NPTDLL="$PREBUILT/x86_64-windows/nptunix.dll"
        NPTSO="$PREBUILT/x86_64-unix/nptunix.so"
    fi
}

state_read() {
    local file hash

    STATE_MODE=""
    STATE_TARGET=""
    STATE_FILES=()

    [ -f "$STATE" ] || return 1

    IFS= read -r STATE_MODE < "$STATE" || return 1
    IFS= read -r STATE_TARGET < <(sed -n '2p' "$STATE") || return 1
    while IFS=$'\t' read -r file hash; do
        [ -n "$file" ] || continue
        STATE_FILES+=("$file" "$hash")
    done < <(tail -n +3 "$STATE")
    return 0
}

remove_managed_files() {
    state_read || return 0

    local i file expected actual path
    for ((i = 0; i < ${#STATE_FILES[@]}; i += 2)); do
        file="${STATE_FILES[i]}"
        expected="${STATE_FILES[i + 1]}"
        path="$STATE_TARGET/$file"
        [ -e "$path" ] || continue
        actual="$(sha256sum "$path" | awk '{print $1}')"
        [ "$actual" = "$expected" ] || {
            echo "refusing to remove modified managed file: $path" >&2
            echo "expected: $expected" >&2
            echo "actual:   $actual" >&2
            exit 1
        }
    done

    for ((i = 0; i < ${#STATE_FILES[@]}; i += 2)); do
        rm -f "$STATE_TARGET/${STATE_FILES[i]}"
    done
    rm -f "$STATE"
}

ensure_target_is_safe() {
    local target="$1"
    shift

    local file path managed=false i
    for file in "$@"; do
        path="$target/$file"
        [ -e "$path" ] || continue

        managed=false
        if state_read && [ "$STATE_TARGET" = "$target" ]; then
            for ((i = 0; i < ${#STATE_FILES[@]}; i += 2)); do
                if [ "${STATE_FILES[i]}" = "$file" ]; then
                    managed=true
                    break
                fi
            done
        fi

        $managed || {
            echo "refusing to overwrite unmanaged file: $path" >&2
            echo "remove or rename it explicitly, then retry" >&2
            exit 1
        }
    done
}

print_launch_options() {
    case "$1" in
        venus)
            echo "%command%"
            ;;
        neptune)
            printf "WINEDLLPATH='%s' WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %%command%%\n" "$RUNTIME"
            ;;
        neptune-dx12)
            printf "WINEDLLPATH='%s' WINEDLLOVERRIDES='dxgi=n,b;nptunix=b;d3d12=n,b;d3d12core=;nvapi,nvapi64=' %%command%%\n" "$RUNTIME"
            ;;
    esac
}

if [ "$MODE" = status ]; then
    echo "game: $GAME"
    if state_read; then
        echo "mode: $STATE_MODE"
        echo "target dir: $STATE_TARGET"
    else
        echo "mode: venus"
    fi
    echo
    echo "Steam launch options:"
    print_launch_options "${STATE_MODE:-venus}"
    exit 0
fi

if [ "$MODE" = venus ]; then
    # Keep the last selected target so switching back to Neptune does not
    # require specifying the executable again.
    VENUS_TARGET="$GAME"
    if state_read; then
        VENUS_TARGET="$STATE_TARGET"
    fi

    remove_managed_files

    install -d "$STATE_DIR"
    {
        printf '%s\n' venus
        printf '%s\n' "$VENUS_TARGET"
    } > "$STATE"

    echo "game: $GAME"
    echo "mode: venus"
    echo "target dir: $VENUS_TARGET"
    echo
    echo "Steam launch options:"
    print_launch_options venus
    exit 0
fi

source_paths
TARGET_DIR="$(resolve_target_dir)"

case "$MODE" in
    neptune)
        REQUIRED=(d3d11.dll dxgi.dll nptunix.dll)
        SOURCES=("$D3D11" "$DXGI" "$NPTDLL")
        ;;
    neptune-dx12)
        REQUIRED=(d3d12.dll dxgi.dll nptunix.dll)
        SOURCES=("$D3D12" "$DXGI" "$NPTDLL")
        ;;
esac

for source in "${SOURCES[@]}" "$NPTSO"; do
    test -f "$source" || {
        echo "missing Neptune artifact: $source" >&2
        exit 1
    }
done

# Validate the destination before removing a previous managed installation.
ensure_target_is_safe "$TARGET_DIR" "${REQUIRED[@]}"
remove_managed_files

install -d "$RUNTIME/x86_64-windows" "$RUNTIME/x86_64-unix" "$STATE_DIR"
install -m 0644 "$NPTDLL" "$RUNTIME/x86_64-windows/nptunix.dll"
install -m 0755 "$NPTSO" "$RUNTIME/x86_64-unix/nptunix.so"

for i in "${!REQUIRED[@]}"; do
    install -m 0644 "${SOURCES[i]}" "$TARGET_DIR/${REQUIRED[i]}"
done

{
    printf '%s\n' "$MODE"
    printf '%s\n' "$TARGET_DIR"
    for file in "${REQUIRED[@]}"; do
        printf '%s\t%s\n' "$file" "$(sha256sum "$TARGET_DIR/$file" | awk '{print $1}')"
    done
} > "$STATE"

echo "game: $GAME"
echo "mode: $MODE"
echo "target dir: $TARGET_DIR"
echo
sha256sum "${REQUIRED[@]/#/$TARGET_DIR/}" "$RUNTIME/x86_64-unix/nptunix.so"
echo
echo "Steam launch options:"
print_launch_options "$MODE"

if [ "$MODE" = neptune-dx12 ]; then
    echo
    echo "host requirements for this experimental mode:"
    echo '  neptune_backend = "d3dmetal"'
    echo '  D3DMETAL_FRAMEWORK_PATH=/path/to/D3DMetal.framework'
fi

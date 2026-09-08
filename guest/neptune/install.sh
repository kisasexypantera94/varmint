#!/usr/bin/env bash
set -euo pipefail

APPID="${1:-}"
EXE_REL="${2:-}"

[ -n "$APPID" ] || {
    echo "usage: $0 <steam-appid> [relative/path/to/Game.exe]" >&2
    exit 2
}

REPO="${VARMINT_NEPTUNE_REPO:-$HOME/virtio-win-mesa-neptune}"
BUILD="${VARMINT_NEPTUNE_BUILD:-$REPO/builddir-win64-x86unix/src/virtio/neptune}"
PREBUILT="${VARMINT_NEPTUNE_PREBUILT:-/usr/local/lib/varmint/neptune}"
STEAM="${VARMINT_STEAM_ROOT:-$HOME/.local/share/Steam}"
RUNTIME="${VARMINT_NEPTUNE_RUNTIME:-$HOME/.local/share/varmint/neptune}"

if [ -f "$BUILD/d3d11.dll" ] && [ -f "$BUILD/dxgi.dll" ] &&
   [ -f "$BUILD/nptunix/nptunix.dll" ] && [ -f "$BUILD/nptunix/nptunix.so" ]; then
    D3D11="$BUILD/d3d11.dll"
    DXGI="$BUILD/dxgi.dll"
    NPTDLL="$BUILD/nptunix/nptunix.dll"
    NPTSO="$BUILD/nptunix/nptunix.so"
else
    D3D11="$PREBUILT/x86_64-windows/d3d11.dll"
    DXGI="$PREBUILT/x86_64-windows/dxgi.dll"
    NPTDLL="$PREBUILT/x86_64-windows/nptunix.dll"
    NPTSO="$PREBUILT/x86_64-unix/nptunix.so"
fi

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

if [ -n "$EXE_REL" ]; then
    EXE="$GAME/$EXE_REL"

    test -f "$EXE" || {
        echo "missing exe: $EXE" >&2
        exit 1
    }

    TARGET_DIR="$(dirname "$EXE")"
else
    TARGET_DIR="$GAME"
fi

for f in "$D3D11" "$DXGI" "$NPTDLL" "$NPTSO"; do
    test -f "$f" || {
        echo "missing build artifact: $f" >&2
        exit 1
    }
done

install -d \
    "$RUNTIME/x86_64-windows" \
    "$RUNTIME/x86_64-unix"

install -m 0644 "$NPTDLL" "$RUNTIME/x86_64-windows/nptunix.dll"
install -m 0755 "$NPTSO"  "$RUNTIME/x86_64-unix/nptunix.so"

install -m 0644 "$D3D11" "$TARGET_DIR/d3d11.dll"
install -m 0644 "$DXGI"  "$TARGET_DIR/dxgi.dll"
install -m 0644 "$NPTDLL" "$TARGET_DIR/nptunix.dll"

cmp -s "$D3D11" "$TARGET_DIR/d3d11.dll" || {
    echo "d3d11.dll copy verification failed" >&2
    exit 1
}
cmp -s "$DXGI" "$TARGET_DIR/dxgi.dll" || {
    echo "dxgi.dll copy verification failed" >&2
    exit 1
}
cmp -s "$NPTDLL" "$TARGET_DIR/nptunix.dll" || {
    echo "nptunix.dll copy verification failed" >&2
    exit 1
}
cmp -s "$NPTSO" "$RUNTIME/x86_64-unix/nptunix.so" || {
    echo "nptunix.so copy verification failed" >&2
    exit 1
}

echo "=== installed ==="
echo "game:       $GAME"
echo "target dir: $TARGET_DIR"

sha256sum \
    "$TARGET_DIR/d3d11.dll" \
    "$TARGET_DIR/dxgi.dll" \
    "$TARGET_DIR/nptunix.dll" \
    "$RUNTIME/x86_64-unix/nptunix.so"

echo
echo "=== Steam launch options ==="
printf '%s\n' \
"WINEDLLPATH='$RUNTIME' PROTON_LOG=1 FEX_X87REDUCEDPRECISION=0 WINEDLLOVERRIDES='d3d11,dxgi=n,b;nptunix=b;d3d12,d3d12core=;nvapi,nvapi64=' %command%"

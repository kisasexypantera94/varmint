#!/usr/bin/env bash
set -euo pipefail

REPO="${VARMINT_NEPTUNE_REPO:-$HOME/virtio-win-mesa-neptune}"
BUILD="${VARMINT_NEPTUNE_BUILD:-$REPO/builddir-win64-x86unix}"

test -d "$REPO/.git" || {
    echo "missing repo: $REPO" >&2
    exit 1
}

if [ ! -d "$BUILD" ]; then
    SHARE="${VARMINT_NEPTUNE_SHARE:-$HOME/.local/share/varmint/neptune}"

    mkdir -p "$SHARE"

    if [ ! -f "$SHARE/build-win64.txt" ]; then
        cat > "$SHARE/build-win64.txt" <<'EOF_CROSS'
[binaries]
c = 'x86_64-w64-mingw32-gcc'
cpp = 'x86_64-w64-mingw32-g++'
ar = 'x86_64-w64-mingw32-ar'
strip = 'x86_64-w64-mingw32-strip'
windres = 'x86_64-w64-mingw32-windres'

[properties]
needs_exe_wrapper = true

[host_machine]
system = 'windows'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF_CROSS
    fi

    if [ ! -f "$SHARE/native-x86_64.ini" ]; then
        cat > "$SHARE/native-x86_64.ini" <<'EOF_NATIVE'
[binaries]
c = 'x86_64-linux-gnu-gcc'
cpp = 'x86_64-linux-gnu-g++'
ar = 'x86_64-linux-gnu-ar'
strip = 'x86_64-linux-gnu-strip'
pkg-config = 'pkg-config'

[properties]
pkg_config_libdir = '/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig'

[host_machine]
system = 'linux'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF_NATIVE
    fi

    echo "=== configure ==="
    meson setup "$BUILD" "$REPO" \
        --cross-file "$SHARE/build-win64.txt" \
        --native-file "$SHARE/native-x86_64.ini" \
        -Dneptune=true \
        -Dnpt_wine=true \
        -Dnpt_umd=off \
        -Dgallium-drivers= \
        -Dvulkan-drivers= \
        -Dplatforms= \
        -Dglx=disabled \
        -Db_lto=false \
        -Dc_args=-fno-omit-frame-pointer \
        -Dcpp_args=-fno-omit-frame-pointer
fi

echo "=== virtio-win-mesa ==="
git -C "$REPO" rev-parse --short HEAD
git -C "$REPO" status --short

echo
echo "=== build ==="
meson compile -C "$BUILD"

echo
echo "=== artifacts ==="
find "$BUILD/src/virtio/neptune" -type f \( \
    -name d3d11.dll -o \
    -name dxgi.dll -o \
    -name d3d12.dll -o \
    -name nptunix.dll -o \
    -name nptunix.so \
\) -print

echo
echo "=== build OK ==="

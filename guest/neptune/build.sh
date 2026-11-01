#!/usr/bin/env bash
set -euo pipefail

REPO="${VARMINT_NEPTUNE_REPO:-$HOME/virtio-win-mesa-neptune}"
BUILD="${VARMINT_NEPTUNE_BUILD:-$REPO/builddir-win64-x86unix}"
SHARE="${VARMINT_NEPTUNE_SHARE:-$HOME/.local/share/varmint/neptune}"

# Both trees build the same AMD64 PE frontend. nptunix.so is a meson
# native target, so the native file selects the Wine unix side:
# builddir-win64-x86unix for x86_64 Proton under FEX, and
# builddir-win64-aarch64unix for ARM64 Proton.
BUILD_AARCH64="$REPO/builddir-win64-aarch64unix"

git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
    echo "missing repo: $REPO" >&2
    exit 1
}

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

if [ ! -f "$SHARE/native-aarch64.ini" ]; then
    cat > "$SHARE/native-aarch64.ini" <<'EOF_NATIVE'
[binaries]
c = 'aarch64-linux-gnu-gcc'
ar = 'aarch64-linux-gnu-ar'
strip = 'aarch64-linux-gnu-strip'
pkg-config = 'pkg-config'

[properties]
pkg_config_libdir = '/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig'

[host_machine]
system = 'linux'
cpu_family = 'aarch64'
cpu = 'aarch64'
endian = 'little'
EOF_NATIVE
fi

configure() {
    local build="$1"
    local native="$2"

    # A failed meson setup may leave a directory without build.ninja.
    # Preserve it for diagnostics rather than deleting it.
    if [ -d "$build" ] && [ ! -f "$build/build.ninja" ]; then
        mv "$build" "${build}.incomplete-$(date +%s)-$$"
    fi

    if [ ! -f "$build/build.ninja" ]; then
        echo "=== configure $(basename "$build") ==="
        meson setup "$build" "$REPO" \
            --cross-file "$SHARE/build-win64.txt" \
            --native-file "$SHARE/$native" \
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
}

configure "$BUILD" native-x86_64.ini
configure "$BUILD_AARCH64" native-aarch64.ini

echo "=== virtio-win-mesa ==="
git -C "$REPO" rev-parse --short HEAD
git -C "$REPO" status --short

for build in "$BUILD" "$BUILD_AARCH64"; do
    echo
    echo "=== build $(basename "$build") ==="
    meson compile -C "$build"
done

echo
echo "=== artifacts ==="

REQUIRED_ARTIFACTS=(
    d3d11.dll
    dxgi.dll
    d3d12.dll
    nptunix/nptunix.dll
    nptunix/nptunix.so
)

for build in "$BUILD" "$BUILD_AARCH64"; do
    for rel in "${REQUIRED_ARTIFACTS[@]}"; do
        path="$build/src/virtio/neptune/$rel"
        test -s "$path" || {
            echo "missing or zero-byte Neptune artifact: $path" >&2
            exit 1
        }
        ls -lh "$path"
    done
done

echo
echo "=== build OK ==="

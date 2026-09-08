#!/bin/sh
set -eu

STATE=/opt/varmint/neptune
SRC=$STATE/src
PATCHES=$STATE/patches
SHARE=/usr/local/share/varmint/neptune
RUNTIME=/usr/local/lib/varmint/neptune

. "$STATE/versions.env"

dpkg --add-architecture amd64
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends $(cat /opt/varmint/neptune/deps.txt)

install -d -m 0755 "$SRC" "$SHARE" \
    "$RUNTIME/x86_64-windows" \
    "$RUNTIME/x86_64-unix"

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

apply_series() {
    repo=$1
    series_dir=$2

    while IFS= read -r patch; do
        [ -n "$patch" ] || continue
        printf 'Applying %s\n' "$patch"
        git -C "$repo" apply --check "$series_dir/$patch"
        git -C "$repo" apply "$series_dir/$patch"
    done < "$series_dir/series"
}

rm -rf "$SRC/neptune-protocol" "$SRC/virtio-win-mesa"

git clone https://github.com/osy/neptune-protocol.git "$SRC/neptune-protocol"
git -C "$SRC/neptune-protocol" checkout --detach "$NEPTUNE_PROTOCOL_COMMIT"
apply_series "$SRC/neptune-protocol" "$PATCHES/neptune-protocol"

git clone https://github.com/osy/virtio-win-mesa.git "$SRC/virtio-win-mesa"
git -C "$SRC/virtio-win-mesa" checkout --detach "$VIRTIO_WIN_MESA_COMMIT"

# Generated protocol is the consumer baseline. Local Mesa patches,
# including temporary patches to generated files, are applied on top.
git -C "$SRC/virtio-win-mesa" apply --check     "$PATCHES/neptune-generated/virtio-win-mesa.patch"
git -C "$SRC/virtio-win-mesa" apply     "$PATCHES/neptune-generated/virtio-win-mesa.patch"

apply_series "$SRC/virtio-win-mesa" "$PATCHES/virtio-win-mesa"

BUILD="$SRC/virtio-win-mesa/builddir-win64-x86unix"

meson setup "$BUILD" "$SRC/virtio-win-mesa" \
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

meson compile -C "$BUILD"

ART="$BUILD/src/virtio/neptune"

install -m 0644 "$ART/d3d11.dll" \
    "$RUNTIME/x86_64-windows/d3d11.dll"
install -m 0644 "$ART/dxgi.dll" \
    "$RUNTIME/x86_64-windows/dxgi.dll"
install -m 0644 "$ART/d3d12.dll" \
    "$RUNTIME/x86_64-windows/d3d12.dll"
install -m 0644 "$ART/nptunix/nptunix.dll" \
    "$RUNTIME/x86_64-windows/nptunix.dll"
install -m 0755 "$ART/nptunix/nptunix.so" \
    "$RUNTIME/x86_64-unix/nptunix.so"

# Builddir contains absolute /opt/varmint/neptune/src paths. The source trees
# are retained, but firstboot recreates the builddir after moving them into
# the user's home.
rm -rf "$BUILD"

test -f "$RUNTIME/x86_64-windows/d3d11.dll"
test -f "$RUNTIME/x86_64-windows/dxgi.dll"
test -f "$RUNTIME/x86_64-windows/nptunix.dll"
test -f "$RUNTIME/x86_64-unix/nptunix.so"

apt-get clean
rm -rf /var/lib/apt/lists/*

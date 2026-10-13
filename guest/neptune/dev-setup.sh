#!/usr/bin/env bash
set -euo pipefail

SHARE=/usr/local/share/varmint/neptune
DEV_SHARE="$HOME/.local/share/varmint/neptune"
VIRTIO_DIR="$HOME/virtio-win-mesa-neptune"
PROTOCOL_DIR="$HOME/neptune-protocol"
BUILD="$VIRTIO_DIR/builddir-win64-x86unix"

source "$SHARE/versions.env"

sudo dpkg --add-architecture amd64
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    $(cat "$SHARE/deps.txt")

# Meson's build-machine compiler produces x86_64 Linux ELF.
# On ARM64, its sanity checks need binfmt/QEMU. Development-only.
if [ "$(dpkg --print-architecture)" = "arm64" ]; then
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
        --no-install-recommends qemu-user-binfmt
    sudo systemctl restart systemd-binfmt
fi

prepare_repo() {
    local url="$1"
    local dir="$2"
    local commit="$3"
    local patch_dir="$4"
    local generated_patch="${5:-}"

    if [ ! -d "$dir/.git" ]; then
        git clone "$url" "$dir"
        git -C "$dir" checkout "$commit"
    else
        [ "$(git -C "$dir" rev-parse HEAD)" = "$commit" ] || {
            echo "error: $dir already exists at a different commit" >&2
            exit 1
        }
    fi

    if [ -n "$generated_patch" ]; then
        if git -C "$dir" apply --check "$generated_patch" 2>/dev/null; then
            git -C "$dir" apply "$generated_patch"
        elif git -C "$dir" apply --reverse --check "$generated_patch" 2>/dev/null; then
            :
        else
            echo "error: cannot apply $generated_patch cleanly" >&2
            exit 1
        fi
    fi

    while IFS= read -r patch; do
        [ -n "$patch" ] || continue
        patch="$patch_dir/$patch"

        if git -C "$dir" apply --check "$patch" 2>/dev/null; then
            git -C "$dir" apply "$patch"
        elif git -C "$dir" apply --reverse --check "$patch" 2>/dev/null; then
            :
        else
            echo "error: cannot apply $patch cleanly" >&2
            exit 1
        fi
    done < "$patch_dir/series"
}

prepare_repo \
    https://github.com/osy/neptune-protocol.git \
    "$PROTOCOL_DIR" \
    "$NEPTUNE_PROTOCOL_COMMIT" \
    "$SHARE/patches/neptune-protocol"

prepare_repo \
    https://github.com/osy/virtio-win-mesa.git \
    "$VIRTIO_DIR" \
    "$VIRTIO_WIN_MESA_COMMIT" \
    "$SHARE/patches/virtio-win-mesa" \
    "$SHARE/patches/neptune-generated/virtio-win-mesa.patch"

mkdir -p "$DEV_SHARE"

cat > "$DEV_SHARE/build-win64.txt" <<'EOF_CROSS'
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

cat > "$DEV_SHARE/native-x86_64.ini" <<'EOF_NATIVE'
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

# A failed meson setup may leave a directory without build.ninja.
# Preserve it for diagnostics rather than deleting it.
if [ -d "$BUILD" ] && [ ! -f "$BUILD/build.ninja" ]; then
    mv "$BUILD" "${BUILD}.incomplete-$(date +%s)-$$"
fi

if [ ! -f "$BUILD/build.ninja" ]; then
    meson setup "$BUILD" "$VIRTIO_DIR" \
        --cross-file "$DEV_SHARE/build-win64.txt" \
        --native-file "$DEV_SHARE/native-x86_64.ini" \
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

echo
echo "Neptune development environment ready."
echo "Build with: varmint-neptune-build"

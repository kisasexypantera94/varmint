#!/usr/bin/env bash
set -euo pipefail

REPO="${VARMINT_NEPTUNE_REPO:-$HOME/virtio-win-mesa-neptune}"
BUILD="${VARMINT_NEPTUNE_BUILD:-$REPO/builddir-win64-x86unix}"

test -d "$REPO/.git" || {
    echo "missing repo: $REPO" >&2
    exit 1
}

test -d "$BUILD" || {
    echo "missing build dir: $BUILD" >&2
    exit 1
}

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

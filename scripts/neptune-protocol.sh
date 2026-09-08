#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
OUT_DIR="$ROOT/patches/neptune-generated"

case "${1:-}" in
    --diff)   MODE=diff ;;
    --update) MODE=update ;;
    *)
        echo "usage: $0 --diff|--update" >&2
        exit 2
        ;;
esac

source "$ROOT/guest/neptune/versions.env"

VIRGL_COMMIT="$(
    python3 - "$ROOT/runtime/manifest.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    print(tomllib.load(f)["dependencies"]["virglrenderer"]["commit"])
PY
)"

HOST_REPO="$ROOT/build/deps/src/virglrenderer"

[ -d "$HOST_REPO/.git" ] || {
    echo "error: missing host virglrenderer checkout: $HOST_REPO" >&2
    exit 1
}

tmp="$(mktemp -d /tmp/varmint-neptune-generated.XXXXXX)"
remote_stage="/tmp/varmint-neptune-generated"
host_wt="$tmp/virglrenderer"

cleanup() {
    git -C "$HOST_REPO" worktree remove --force "$host_wt" >/dev/null 2>&1 || true
    rm -rf "$tmp"
    ssh -n "$TARGET" "rm -rf '$remote_stage'" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "=== stage protocol recipe ==="

COPYFILE_DISABLE=1 tar --no-xattrs -C "$ROOT" -cf - \
    guest/neptune/versions.env \
    guest/neptune/protocol-export.sh \
    patches/neptune-protocol \
| ssh "$TARGET" \
    "rm -rf '$remote_stage' &&
     mkdir -p '$remote_stage' &&
     tar -xf - -C '$remote_stage'"

echo "=== generate guest + host protocol ==="

ssh "$TARGET" \
    "STAGE='$remote_stage' bash -s" > "$tmp/guest-bundle.tar" <<'REMOTE'
set -euo pipefail

source "$STAGE/guest/neptune/versions.env"

PROTO_REPO="$HOME/neptune-protocol"
MESA_REPO="$HOME/virtio-win-mesa-neptune"

for repo in "$PROTO_REPO" "$MESA_REPO"; do
    [ -d "$repo/.git" ] || {
        echo "error: missing repo: $repo" >&2
        exit 1
    }
done

git -C "$PROTO_REPO" cat-file -e "$NEPTUNE_PROTOCOL_COMMIT^{commit}" 2>/dev/null ||
    git -C "$PROTO_REPO" fetch origin "$NEPTUNE_PROTOCOL_COMMIT"

git -C "$MESA_REPO" cat-file -e "$VIRTIO_WIN_MESA_COMMIT^{commit}" 2>/dev/null ||
    git -C "$MESA_REPO" fetch origin "$VIRTIO_WIN_MESA_COMMIT"

tmp="$(mktemp -d /tmp/varmint-neptune-protocol-build.XXXXXX)"
proto_wt="$tmp/protocol"
mesa_wt="$tmp/mesa"

cleanup() {
    git -C "$PROTO_REPO" worktree remove --force "$proto_wt" >/dev/null 2>&1 || true
    git -C "$MESA_REPO" worktree remove --force "$mesa_wt" >/dev/null 2>&1 || true
    rm -rf "$tmp"
}
trap cleanup EXIT

git -C "$PROTO_REPO" worktree add --detach "$proto_wt" \
    "$NEPTUNE_PROTOCOL_COMMIT" >/dev/null

while IFS= read -r patch; do
    [ -n "$patch" ] || continue
    git -C "$proto_wt" apply --check \
        "$STAGE/patches/neptune-protocol/$patch"
    git -C "$proto_wt" apply \
        "$STAGE/patches/neptune-protocol/$patch"
done < "$STAGE/patches/neptune-protocol/series"

NEPTUNE_PROTOCOL_DIR="$proto_wt" \
    bash "$STAGE/guest/neptune/protocol-export.sh" \
    > "$tmp/generated.tar"

mkdir "$tmp/generated"
tar -xf "$tmp/generated.tar" -C "$tmp/generated"

# Build the guest generated baseline patch against pristine Mesa.
git -C "$MESA_REPO" worktree add --detach "$mesa_wt" \
    "$VIRTIO_WIN_MESA_COMMIT" >/dev/null

dst="$mesa_wt/src/virtio/neptune/neptune-protocol"

find "$dst" -maxdepth 1 -type f \( \
    -name "npt_client_families.txt" -o \
    -name "npt_protocol_client_*.c" -o \
    -name "npt_protocol_client_*.h" -o \
    -name "npt_protocol_common_*.h" -o \
    -name "npt_protocol_defs.h" -o \
    -name "npt_protocol_directx_types.h" -o \
    -name "npt_protocol_guest*.h" \
\) -delete

cp "$tmp/generated/guest/"* "$dst/"

git -C "$mesa_wt" diff --binary "$VIRTIO_WIN_MESA_COMMIT" \
    > "$tmp/virtio-win-mesa.patch"

mkdir "$tmp/bundle"
cp "$tmp/virtio-win-mesa.patch" "$tmp/bundle/"
cp -R "$tmp/generated/host" "$tmp/bundle/host"

tar -C "$tmp/bundle" -cf - .
REMOTE

mkdir "$tmp/guest-bundle"
tar -xf "$tmp/guest-bundle.tar" -C "$tmp/guest-bundle"

echo "=== generate host baseline patch ==="

git -C "$HOST_REPO" cat-file -e "$VIRGL_COMMIT^{commit}" 2>/dev/null ||
    git -C "$HOST_REPO" fetch origin "$VIRGL_COMMIT"

git -C "$HOST_REPO" worktree add --detach "$host_wt" \
    "$VIRGL_COMMIT" >/dev/null

host_dst="$host_wt/src/neptune/neptune-protocol"

find "$host_dst" -maxdepth 1 -type f \( \
    -name "npt_protocol_common_*.h" -o \
    -name "npt_protocol_defs.h" -o \
    -name "npt_protocol_directx_types.h" -o \
    -name "npt_protocol_host*.h" \
\) -delete

cp "$tmp/guest-bundle/host/"* "$host_dst/"

git -C "$host_wt" diff --binary "$VIRGL_COMMIT" \
    > "$tmp/virglrenderer.patch"

guest_patch="$tmp/guest-bundle/virtio-win-mesa.patch"
host_patch="$tmp/virglrenderer.patch"

if [ "$MODE" = "update" ]; then
    mkdir -p "$OUT_DIR"
    cp "$guest_patch" "$OUT_DIR/virtio-win-mesa.patch"
    cp "$host_patch" "$OUT_DIR/virglrenderer.patch"

    echo
    echo "=== generated patches updated ==="
    echo "-- virtio-win-mesa --"
    git apply --stat "$OUT_DIR/virtio-win-mesa.patch"
    echo "-- virglrenderer --"
    git apply --stat "$OUT_DIR/virglrenderer.patch"
    exit 0
fi

echo
echo "=== generated patch check ==="

status=0

if [ ! -f "$OUT_DIR/virtio-win-mesa.patch" ] ||
   ! cmp -s "$guest_patch" "$OUT_DIR/virtio-win-mesa.patch"; then
    echo "OUTDATED: patches/neptune-generated/virtio-win-mesa.patch"
    status=1
else
    echo "OK: virtio-win-mesa.patch"
fi

if [ ! -f "$OUT_DIR/virglrenderer.patch" ] ||
   ! cmp -s "$host_patch" "$OUT_DIR/virglrenderer.patch"; then
    echo "OUTDATED: patches/neptune-generated/virglrenderer.patch"
    status=1
else
    echo "OK: virglrenderer.patch"
fi

exit "$status"

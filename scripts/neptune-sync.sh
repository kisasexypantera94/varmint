#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
REMOTE_STAGE="/tmp/varmint-neptune-sync"
FORCE=0

if [ "${1:-}" = "--force" ]; then
    FORCE=1
elif [ "$#" -ne 0 ]; then
    echo "usage: $0 [--force]" >&2
    exit 2
fi

source "$ROOT/guest/neptune/versions.env"

COPYFILE_DISABLE=1 tar --no-xattrs -C "$ROOT" -cf - \
    guest/neptune/versions.env \
    patches/neptune-protocol \
    patches/neptune-generated/virtio-win-mesa.patch \
    patches/virtio-win-mesa \
| ssh "$TARGET" \
    "rm -rf '$REMOTE_STAGE' &&
     mkdir -p '$REMOTE_STAGE' &&
     tar -xf - -C '$REMOTE_STAGE'"

ssh "$TARGET" \
    "NEPTUNE_PROTOCOL_COMMIT='$NEPTUNE_PROTOCOL_COMMIT' \
     VIRTIO_WIN_MESA_COMMIT='$VIRTIO_WIN_MESA_COMMIT' \
     REMOTE_STAGE='$REMOTE_STAGE' \
     GENERATED_MESA_PATCH='$REMOTE_STAGE/patches/neptune-generated/virtio-win-mesa.patch' \
     FORCE='$FORCE' \
     bash -s" <<'REMOTE'
set -euo pipefail

sync_repo() {
    name="$1"
    repo="$2"
    commit="$3"
    patches="$4"
    generated_patch="${5:-}"

    echo "=== $name ==="

    test -d "$repo/.git" || {
        echo "missing repo: $repo" >&2
        exit 1
    }

    git -C "$repo" cat-file -e "$commit^{commit}" 2>/dev/null ||
        git -C "$repo" fetch origin "$commit"

    if [ "$FORCE" = 1 ]; then
        echo "reset -> ${commit:0:12}"

        # Keep upstream/local branch refs untouched: the working tree itself
        # follows the pinned revision in detached HEAD state.
        git -C "$repo" checkout --detach "$commit"
        git -C "$repo" reset --hard "$commit"

        # Deliberately no git clean: build directories and other untracked
        # development files survive a sync.
        if [ -n "$generated_patch" ]; then
            echo "apply $(basename "$generated_patch")"
            git -C "$repo" apply --check "$generated_patch"
            git -C "$repo" apply "$generated_patch"
        fi

        while IFS= read -r patch; do
            [ -n "$patch" ] || continue
            echo "apply $patch"
            git -C "$repo" apply --check "$patches/$patch"
            git -C "$repo" apply "$patches/$patch"
        done < "$patches/series"
    fi

    tmp="$(mktemp -d "/tmp/${name}.XXXXXX")"
    expected="$(mktemp)"
    actual="$(mktemp)"

    cleanup() {
        git -C "$repo" worktree remove --force "$tmp" >/dev/null 2>&1 || true
        rm -f "$expected" "$actual"
    }

    git -C "$repo" worktree add --detach "$tmp" "$commit" >/dev/null

    if [ -n "$generated_patch" ]; then
        git -C "$tmp" apply --check "$generated_patch"
        git -C "$tmp" apply "$generated_patch"
    fi

    while IFS= read -r patch; do
        [ -n "$patch" ] || continue
        git -C "$tmp" apply --check "$patches/$patch"
        git -C "$tmp" apply "$patches/$patch"
    done < "$patches/series"

    git -C "$tmp" diff --binary "$commit" > "$expected"
    git -C "$repo" diff --binary "$commit" > "$actual"

    if ! cmp -s "$expected" "$actual"; then
        echo "$name: guest tree does NOT match varmint pin + patches" >&2
        echo
        echo "--- guest diffstat ---"
        git --no-pager -C "$repo" diff --stat "$commit"
        echo
        echo "--- expected diffstat ---"
        git --no-pager -C "$tmp" diff --stat "$commit"
        cleanup
        exit 1
    fi

    cleanup
    echo "$name: OK"
}

sync_repo \
    neptune-protocol \
    "$HOME/neptune-protocol" \
    "$NEPTUNE_PROTOCOL_COMMIT" \
    "$REMOTE_STAGE/patches/neptune-protocol" \
    ""

sync_repo \
    virtio-win-mesa \
    "$HOME/virtio-win-mesa-neptune" \
    "$VIRTIO_WIN_MESA_COMMIT" \
    "$REMOTE_STAGE/patches/virtio-win-mesa" \
    "$GENERATED_MESA_PATCH"

echo
if [ "$FORCE" = 1 ]; then
    echo "=== guest dependencies synced and verified ==="
else
    echo "=== guest dependency state matches varmint exactly ==="
fi
REMOTE

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
STAMP="$(date +%Y%m%d-%H%M%S)"

source "$ROOT/guest/neptune/versions.env"

capture_repo() {
    name="$1"
    repo_rel="$2"
    commit="$3"

    out="$ROOT/patches/wip/${STAMP}-${name}.patch"
    latest="$ROOT/patches/wip/latest-${name}.patch"
    tmp="${out}.tmp"

    ssh "$TARGET" \
        "git -C \"\$HOME/$repo_rel\" diff --binary '$commit'" > "$tmp"

    if [ ! -s "$tmp" ]; then
        rm -f "$tmp"
        echo "$name: clean"
        return
    fi

    mv "$tmp" "$out"

    (
        cd "$ROOT/patches/wip"
        ln -sfn "$(basename "$out")" "$(basename "$latest")"
    )

    echo "$name:"
    echo "  snapshot: $out"
    echo "  latest:   $latest"
    grep -E '^diff --git ' "$out" | sed 's/^/  /'
}

capture_repo \
    neptune-protocol \
    neptune-protocol \
    "$NEPTUNE_PROTOCOL_COMMIT"

capture_repo \
    virtio-win-mesa \
    virtio-win-mesa-neptune \
    "$VIRTIO_WIN_MESA_COMMIT"

#!/usr/bin/env bash
set -euo pipefail

PROTOCOL="${NEPTUNE_PROTOCOL_DIR:-$HOME/neptune-protocol}"

git -C "$PROTOCOL" rev-parse --is-inside-work-tree >/dev/null 2>&1 || {
    echo "error: missing neptune-protocol checkout: $PROTOCOL" >&2
    exit 1
}

tmp="$(mktemp -d /tmp/varmint-neptune-protocol.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT

build="$tmp/build"
out="$tmp/out"
guest="$out/guest"
host="$out/host"

mkdir -p "$guest" "$host"

meson setup "$build" "$PROTOCOL" >&2
meson compile -C "$build" >&2
meson test -C "$build" --print-errorlogs >&2

copy_pattern() {
    dst="$1"
    pattern="$2"

    while IFS= read -r file; do
        cp "$file" "$dst/"
    done < <(find "$build" -maxdepth 1 -type f -name "$pattern" -print | sort)
}

# Shared output belongs to both consumers.
for pattern in \
    "npt_protocol_common_*.h" \
    "npt_protocol_defs.h" \
    "npt_protocol_directx_types.h"
do
    copy_pattern "$guest" "$pattern"
    copy_pattern "$host" "$pattern"
done

# virtio-win-mesa side.
for pattern in \
    "npt_client_families.txt" \
    "npt_protocol_client_*.c" \
    "npt_protocol_client_*.h" \
    "npt_protocol_guest*.h"
do
    copy_pattern "$guest" "$pattern"
done

# virglrenderer side.
copy_pattern "$host" "npt_protocol_host*.h"

for required in \
    "$guest/npt_protocol_defs.h" \
    "$guest/npt_protocol_guest.h" \
    "$guest/npt_protocol_client_table.c" \
    "$host/npt_protocol_defs.h" \
    "$host/npt_protocol_host.h"
do
    [ -f "$required" ] || {
        echo "error: missing generated output: $(basename "$required")" >&2
        exit 1
    }
done

tar -C "$out" -cf - guest host

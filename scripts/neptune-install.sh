#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
REMOTE="/tmp/varmint-neptune-install.sh"

APPID="${1:-}"
EXE_REL="${2:-}"

[ -n "$APPID" ] || {
    echo "usage: $0 <steam-appid> [relative/path/to/Game.exe]" >&2
    exit 2
}

scp -q "$ROOT/guest/neptune/install.sh" "$TARGET:$REMOTE"

if [ -n "$EXE_REL" ]; then
    ssh "$TARGET" "chmod +x '$REMOTE' && '$REMOTE' '$APPID' '$EXE_REL'"
else
    ssh "$TARGET" "chmod +x '$REMOTE' && '$REMOTE' '$APPID'"
fi

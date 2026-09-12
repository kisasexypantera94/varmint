#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
REMOTE="/tmp/varmint-game-graphics.sh"

APPID="${1:-}"
MODE="${2:-status}"
EXE_REL="${3:-}"

[ -n "$APPID" ] || {
    echo "usage: $0 <steam-appid> <status|venus|neptune> [relative/path/to/Game.exe]" >&2
    exit 2
}

scp -q "$ROOT/guest/neptune/game-graphics.sh" "$TARGET:$REMOTE"

printf -v remote_command 'chmod +x %q && %q %q %q' "$REMOTE" "$REMOTE" "$APPID" "$MODE"
if [ -n "$EXE_REL" ]; then
    printf -v quoted_exe '%q' "$EXE_REL"
    remote_command+=" $quoted_exe"
fi

ssh -n "$TARGET" "$remote_command"

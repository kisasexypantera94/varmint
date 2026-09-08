#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
REMOTE="/tmp/varmint-neptune-build.sh"

scp -q "$ROOT/guest/neptune/build.sh" "$TARGET:$REMOTE"
ssh "$TARGET" "chmod +x '$REMOTE' && '$REMOTE'"

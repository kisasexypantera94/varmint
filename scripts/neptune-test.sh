#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${VARMINT_GUEST_SSH:-varmint-dev}"
REMOTE_STAGE="/tmp/varmint-neptune-test"

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <d3d11|d3d12> <smoke|fullscreen|empty_cs>" >&2
    exit 2
fi

API="$1"
TEST="$2"

case "$API" in
    d3d11 | d3d12) ;;
    *)
        echo "unsupported API: $API" >&2
        exit 2
        ;;
esac

case "$TEST" in
    smoke | fullscreen | empty_cs) ;;
    *)
        echo "unsupported test: $TEST" >&2
        exit 2
        ;;
esac

COPYFILE_DISABLE=1 tar --no-xattrs -C "$ROOT" -cf - tests/neptune |
    ssh "$TARGET" \
        "rm -rf '$REMOTE_STAGE' && mkdir -p '$REMOTE_STAGE' && tar -xf - -C '$REMOTE_STAGE'"

printf -v remote_command 'bash %q %q %q' \
    "$REMOTE_STAGE/tests/neptune/guest-run.sh" "$API" "$TEST"

if [ -n "${VARMINT_NEPTUNE_BUILD:-}" ]; then
    printf -v value '%q' "$VARMINT_NEPTUNE_BUILD"
    remote_command="VARMINT_NEPTUNE_BUILD=$value $remote_command"
fi

if [ -n "${VARMINT_TEST_PROTON:-}" ]; then
    printf -v value '%q' "$VARMINT_TEST_PROTON"
    remote_command="VARMINT_TEST_PROTON=$value $remote_command"
fi

ssh -n "$TARGET" "$remote_command"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMMON_SCRIPT="$SCRIPT_DIR/build-common.sh"
DEPENDENCIES_SCRIPT="$SCRIPT_DIR/build-dependencies.sh"
BUNDLE_SCRIPT="$SCRIPT_DIR/build-bundle.sh"

BUILD_ROOT="$ROOT/build"
PREFIX="$BUILD_ROOT/prefix"
DEPS_SRC="$BUILD_ROOT/deps/src"
DEPS_VENV="$BUILD_ROOT/deps/venv"
APP_BUNDLE="$ROOT/dist/Varmint.app"
MANIFEST="$ROOT/runtime/manifest.toml"
MOLTENVK_PATCHES=(
  "$ROOT/patches/moltenvk/0001-guard-null-multisample-state-in-mesh-pipeline.patch"
  "$ROOT/patches/moltenvk/0002-fix-query-device-available-range.patch"
  "$ROOT/patches/moltenvk/0003-fix-query-copy-availability-offset.patch"
)
VIRGL_PATCHES=(
  "$ROOT/patches/virglrenderer/0001-neptune-renderer-and-d3d11-zerocopy.patch"
)
DXMT_PATCHES=(
  "$ROOT/patches/dxmt/0001-neptune-d3d11-correctness-and-zerocopy.patch"
)
ENTITLEMENTS="$ROOT/runtime/entitlements.plist"
VMNET_HELPER_ENTITLEMENTS="$ROOT/runtime/vmnet-helper-entitlements.plist"
ICON_SOURCE="$ROOT/assets/icon.icon"
ICON="$BUILD_ROOT/runtime/Assets.car"
BINARY="$BUILD_ROOT/release/varmint"
VMNET_HELPER="${VMNET_HELPER:-}"

KERNEL=""
INITRD=""
BASE_IMAGE=""
SKIP_DEPENDENCIES=0
FORCE_REBUILD=0
DEPENDENCIES_ONLY=0
SDK=macosx
ARCH=""
CONFIGURATION=Release

# shellcheck source=build-common.sh
source "$COMMON_SCRIPT"
# shellcheck source=build-dependencies.sh
source "$DEPENDENCIES_SCRIPT"
# shellcheck source=build-bundle.sh
source "$BUNDLE_SCRIPT"
trap cleanup EXIT

usage() {
  cat <<'EOF_USAGE'
usage: ./scripts/build-app.sh [options]

  --kernel PATH           kernel to bundle
  --initrd PATH           initrd to bundle
  --base-image PATH       compressed base disk image to bundle
  --skip-dependencies     reuse the existing build prefix
  --force-dependencies    rebuild pinned dependencies
  --dependencies-only     prepare dependencies and stop
  -h, --help              show this help
EOF_USAGE
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --kernel|--initrd|--base-image)
        [ "$#" -ge 2 ] || die "$1 requires a path"
        case "$1" in
          --kernel) KERNEL="$2" ;;
          --initrd) INITRD="$2" ;;
          --base-image) BASE_IMAGE="$2" ;;
        esac
        shift 2
        ;;
      --skip-dependencies) SKIP_DEPENDENCIES=1; shift ;;
      --force-dependencies) FORCE_REBUILD=1; shift ;;
      --dependencies-only) DEPENDENCIES_ONLY=1; shift ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown option: $1" ;;
    esac
  done
  [ "$DEPENDENCIES_ONLY" = 0 ] || [ "$SKIP_DEPENDENCIES" = 0 ] \
    || die "--dependencies-only cannot be combined with --skip-dependencies"
}

main() {
  parse_args "$@"
  need_file "$MANIFEST"
  load_metadata
  mkdir -p "$BUILD_ROOT" "$(dirname "$APP_BUNDLE")"

  if [ "$SKIP_DEPENDENCIES" = 1 ]; then
    verify_dependency_prefix
  else
    build_dependencies
  fi
  if [ "$DEPENDENCIES_ONLY" = 1 ]; then
    log "dependencies ready: $PREFIX"
    return
  fi

  [ -n "$KERNEL" ] || die "--kernel is required"
  [ -n "$INITRD" ] || die "--initrd is required"
  [ -n "$BASE_IMAGE" ] || die "--base-image is required"
  build_app_bundle
  log "done: $APP_BUNDLE"
}

main "$@"

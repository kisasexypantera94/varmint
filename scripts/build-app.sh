#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_ROOT="$ROOT/build"
PREFIX="$BUILD_ROOT/prefix"
APP_BUNDLE="$ROOT/dist/Varmint.app"
MANIFEST="$ROOT/runtime/manifest.toml"
ENTITLEMENTS="$ROOT/runtime/entitlements.plist"

KERNEL=""
INITRD=""
ICON_SOURCE="$ROOT/assets/icon.icon"
ICON="$BUILD_ROOT/runtime/Assets.car"
BINARY="$BUILD_ROOT/release/varmint"

SKIP_DEPENDENCIES=0
FORCE_REBUILD=0
DEPENDENCIES_ONLY=0

SDK=macosx
ARCH=arm64
CONFIGURATION=Release

PREFIX_UPDATE_ACTIVE=0
PREFIX_BACKUP=""
PREFIX_HAD_EXISTING=0

log() {
  printf '\n== %s ==\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

need_file() {
  [ -f "$1" ] || die "missing file: $1"
}

rollback_dependency_prefix() {
  [ "$PREFIX_UPDATE_ACTIVE" = 1 ] || return 0

  rm -rf "$PREFIX"
  if [ "$PREFIX_HAD_EXISTING" = 1 ] && [ -e "$PREFIX_BACKUP" ]; then
    mv "$PREFIX_BACKUP" "$PREFIX"
    printf 'restored previous dependency prefix: %s\n' "$PREFIX" >&2
  fi

  PREFIX_UPDATE_ACTIVE=0
  PREFIX_BACKUP=""
  PREFIX_HAD_EXISTING=0
}

cleanup() {
  local status=$?
  trap - EXIT
  rollback_dependency_prefix || true
  exit "$status"
}

trap cleanup EXIT

begin_dependency_prefix_update() {
  local parent basename
  parent="$(dirname "$PREFIX")"
  basename="$(basename "$PREFIX")"
  PREFIX_BACKUP="$parent/.${basename}.backup.$$"
  PREFIX_HAD_EXISTING=0

  mkdir -p "$parent"
  rm -rf "$PREFIX_BACKUP"
  if [ -e "$PREFIX" ]; then
    mv "$PREFIX" "$PREFIX_BACKUP"
    PREFIX_HAD_EXISTING=1
  fi

  PREFIX_UPDATE_ACTIVE=1
  mkdir -p "$PREFIX"
}

commit_dependency_prefix_update() {
  PREFIX_UPDATE_ACTIVE=0
  rm -rf "$PREFIX_BACKUP"
  PREFIX_BACKUP=""
  PREFIX_HAD_EXISTING=0
}

manifest_value() {
  local key="$1"
  local manifest="${2:-$MANIFEST}"
  python3 - "$manifest" "$key" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    value = tomllib.load(f)
for part in sys.argv[2].split("."):
    value = value[part]
if isinstance(value, bool):
    print("1" if value else "0")
else:
    print(value)
PY
}

manifest_lines() {
  local key="$1"
  local manifest="${2:-$MANIFEST}"
  python3 - "$manifest" "$key" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as f:
    value = tomllib.load(f)
for part in sys.argv[2].split("."):
    value = value[part]
for item in value:
    print(item)
PY
}

usage() {
  cat <<EOF_USAGE
usage: ./scripts/build-app.sh [options]

options:
  --kernel PATH           kernel to bundle
  --initrd PATH           initrd to bundle
  --skip-dependencies     reuse the existing build prefix
  --force-dependencies    rebuild pinned dependencies even if the stamp matches
  --dependencies-only     prepare dependencies and stop
  -h, --help              show this help

EOF_USAGE
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --kernel)
        [ "$#" -ge 2 ] || die "--kernel requires a path"
        KERNEL="$2"
        shift 2
        ;;
      --initrd)
        [ "$#" -ge 2 ] || die "--initrd requires a path"
        INITRD="$2"
        shift 2
        ;;
      --skip-dependencies)
        SKIP_DEPENDENCIES=1
        shift
        ;;
      --force-dependencies)
        FORCE_REBUILD=1
        shift
        ;;
      --dependencies-only)
        DEPENDENCIES_ONLY=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
  done

  if [ "$DEPENDENCIES_ONLY" = 1 ] && [ "$SKIP_DEPENDENCIES" = 1 ]; then
    die "--dependencies-only cannot be combined with --skip-dependencies"
  fi
}

checkout_repo() {
  local repo="$1"
  local commit="$2"
  local dir="$3"

  mkdir -p "$(dirname "$dir")"
  if [ ! -d "$dir/.git" ]; then
    rm -rf "$dir"
    git clone "$repo" "$dir"
  fi

  git -C "$dir" fetch origin \
    '+refs/heads/*:refs/remotes/origin/*' \
    '+refs/tags/*:refs/tags/*'
  if ! git -C "$dir" cat-file -e "$commit^{tree}" 2>/dev/null; then
    git -C "$dir" fetch origin "$commit"
  fi
  git -C "$dir" cat-file -e "$commit^{tree}" 2>/dev/null || die "cannot resolve $commit in $repo"
  git -C "$dir" checkout --detach "$commit"
}

codesign_file() {
  codesign --force --sign - --timestamp=none "$1"
}

create_angle_framework_wrappers() {
  local stem dylib framework
  for stem in EGL GLESv2; do
    dylib="$PREFIX/lib/lib${stem}.dylib"
    framework="$PREFIX/lib/${stem}.framework"
    need_file "$dylib"
    rm -rf "$framework"
    mkdir -p "$framework/Versions/A"
    ln -s "../../../lib${stem}.dylib" "$framework/Versions/A/$stem"
    ln -s A "$framework/Versions/Current"
    ln -s "Versions/Current/$stem" "$framework/$stem"
  done
}

write_angle_pkgconfig() {
  local angle_commit="$1"
  mkdir -p "$PREFIX/lib/pkgconfig"
  cat > "$PREFIX/lib/pkgconfig/angle.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: angle
Description: UTM WebKit ANGLE
Version: ${angle_commit}
Libs: -L\${libdir} -lEGL -lGLESv2
Cflags: -I\${includedir}
PC
  cat > "$PREFIX/lib/pkgconfig/egl.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: egl
Description: UTM WebKit ANGLE EGL
Version: ${angle_commit}
Libs: -L\${libdir} -lEGL
Cflags: -I\${includedir}
PC
  cat > "$PREFIX/lib/pkgconfig/glesv2.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: glesv2
Description: UTM WebKit ANGLE GLESv2
Version: ${angle_commit}
Libs: -L\${libdir} -lGLESv2
Cflags: -I\${includedir}
PC
}

build_angle() {
  local repository="$1"
  local commit="$2"
  local source="$3"
  local minimum_macos="$4"

  log "ANGLE"
  checkout_repo "$repository" "$commit" "$source"
  local angle_dir="$source/Source/ThirdParty/ANGLE"
  [ -d "$angle_dir" ] || die "missing ANGLE source directory: $angle_dir"
  rm -rf "$angle_dir/ANGLE.xcarchive"

  (
    cd "$angle_dir"
    env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" \
      xcodebuild archive \
      -archivePath ANGLE \
      -scheme ANGLE \
      -sdk "$SDK" \
      -arch "$ARCH" \
      -configuration "$CONFIGURATION" \
      WEBCORE_LIBRARY_DIR=/usr/local/lib \
      NORMAL_UMBRELLA_FRAMEWORKS_DIR= \
      CODE_SIGNING_ALLOWED=NO \
      IPHONEOS_DEPLOYMENT_TARGET=14.0 \
      MACOSX_DEPLOYMENT_TARGET="$minimum_macos" \
      XROS_DEPLOYMENT_TARGET=1.0 \
      'OTHER_CFLAGS=$(inherited) -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall' \
      'OTHER_CPLUSPLUSFLAGS=$(inherited) -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall'
  )

  mkdir -p "$PREFIX/lib" "$PREFIX/include"
  rsync -a "$angle_dir/ANGLE.xcarchive/Products/usr/local/lib/" "$PREFIX/lib/"
  rsync -a "$angle_dir/include/" "$PREFIX/include/"
  need_file "$PREFIX/lib/libEGL.dylib"
  need_file "$PREFIX/lib/libGLESv2.dylib"
  install_name_tool -id "$PREFIX/lib/libEGL.dylib" "$PREFIX/lib/libEGL.dylib"
  install_name_tool -id "$PREFIX/lib/libGLESv2.dylib" "$PREFIX/lib/libGLESv2.dylib"
  codesign_file "$PREFIX/lib/libEGL.dylib"
  codesign_file "$PREFIX/lib/libGLESv2.dylib"
  create_angle_framework_wrappers
  write_angle_pkgconfig "$commit"
}

build_epoxy() {
  local repository="$1"
  local commit="$2"
  local source="$3"

  log "libepoxy"
  checkout_repo "$repository" "$commit" "$source"
  rm -rf "$source/build"

  (
    cd "$source"
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" \
    CFLAGS="-I$PREFIX/include" \
    CPPFLAGS="-I$PREFIX/include" \
    LDFLAGS="-L$PREFIX/lib -Wl,-rpath,$PREFIX/lib" \
      meson setup build \
        --prefix "$PREFIX" \
        -Degl=yes \
        -Dglx=no \
        -Dx11=false \
        -Dtests=false
    meson compile -C build --verbose
    meson install -C build
  )
  need_file "$PREFIX/lib/libepoxy.0.dylib"
  codesign_file "$PREFIX/lib/libepoxy.0.dylib"
  PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" pkg-config --variable=epoxy_has_egl epoxy | grep -qx 1
}

build_moltenvk() {
  local repository="$1"
  local commit="$2"
  local version="$3"
  local source="$4"
  local minimum_macos="$5"

  log "MoltenVK"
  checkout_repo "$repository" "$commit" "$source"
  (
    cd "$source"
    env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" \
      ./fetchDependencies --macos -v
    rm -rf "Package/$CONFIGURATION"
    env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" \
      xcodebuild build \
        -project MoltenVKPackaging.xcodeproj \
        -scheme 'MoltenVK Package (macOS only)' \
        -configuration "$CONFIGURATION" \
        -sdk "$SDK" \
        -arch "$ARCH" \
        MACOSX_DEPLOYMENT_TARGET="$minimum_macos" \
        CODE_SIGNING_ALLOWED=NO
  )

  local package="$source/Package/$CONFIGURATION/MoltenVK"
  local dylib="$package/dynamic/dylib/macOS/libMoltenVK.dylib"
  need_file "$dylib"
  install -m 755 "$dylib" "$PREFIX/lib/libMoltenVK.dylib"
  if [ -d "$package/include" ]; then
    rsync -a "$package/include/" "$PREFIX/include/"
  fi
  install_name_tool -id "$PREFIX/lib/libMoltenVK.dylib" "$PREFIX/lib/libMoltenVK.dylib"
  codesign_file "$PREFIX/lib/libMoltenVK.dylib"

  mkdir -p "$PREFIX/lib/pkgconfig"
  cat > "$PREFIX/lib/pkgconfig/MoltenVK.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: MoltenVK
Description: UTM MoltenVK geometry shader build
Version: ${version}
Libs: -L\${libdir} -lMoltenVK
Cflags: -I\${includedir}
PC
  cat > "$PREFIX/lib/pkgconfig/vulkan.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: Vulkan
Description: Vulkan loader backed by UTM MoltenVK
Version: ${version}
Libs: -L\${libdir} -lMoltenVK
Cflags: -I\${includedir}
PC
}

prepare_python() {
  local venv="$1"
  if [ ! -x "$venv/bin/python3" ]; then
    python3 -m venv "$venv"
  fi
  "$venv/bin/python3" -m pip install --disable-pip-version-check --quiet --upgrade pip pyyaml
  export PATH="$venv/bin:$PATH"
}

build_virglrenderer() {
  local repository="$1"
  local commit="$2"
  local source="$3"
  local venv="$4"

  log "virglrenderer"
  checkout_repo "$repository" "$commit" "$source"
  prepare_python "$venv"
  rm -rf "$source/build"

  (
    cd "$source"
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" \
    CFLAGS="-I$PREFIX/include" \
    CPPFLAGS="-I$PREFIX/include" \
    LDFLAGS="-L$PREFIX/lib -Wl,-rpath,$PREFIX/lib" \
      meson setup build \
        --prefix "$PREFIX" \
        -Dvenus=true \
        -Dvulkan-dload=false \
        -Dplatforms=egl \
        -Drender-server-worker=thread \
        -Ddrm-renderers=[] \
        -Dtests=false \
        -Dcheck-gl-errors=false \
        -Dvideo=false \
        -Dtracing=none
    meson compile -C build --verbose
    meson install -C build
  )
  need_file "$PREFIX/lib/libvirglrenderer.1.dylib"
  codesign_file "$PREFIX/lib/libvirglrenderer.1.dylib"
}

verify_dependency_prefix() {
  need_cmd python3
  need_cmd lipo
  need_cmd pkg-config
  log "verify dependency prefix"
  while IFS= read -r dylib; do
    need_file "$PREFIX/lib/$dylib"
    lipo -archs "$PREFIX/lib/$dylib" | tr ' ' '\n' | grep -qx "$ARCH" || die "$dylib does not contain $ARCH"
  done < <(manifest_lines runtime.dylibs)
  PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" pkg-config --exists epoxy egl glesv2 vulkan
}

dependency_recipe_hash() {
  {
    cat "$MANIFEST"
    declare -f \
      checkout_repo \
      codesign_file \
      create_angle_framework_wrappers \
      write_angle_pkgconfig \
      build_angle \
      build_epoxy \
      build_moltenvk \
      prepare_python \
      build_virglrenderer \
      verify_dependency_prefix
    printf '%s\n' "$SDK" "$ARCH" "$CONFIGURATION"
  } | shasum -a 256 | awk '{print $1}'
}

build_dependencies() {
  need_file "$MANIFEST"
  need_cmd git
  need_cmd python3
  need_cmd xcodebuild
  need_cmd cmake
  need_cmd meson
  need_cmd ninja
  need_cmd pkg-config
  need_cmd rsync
  need_cmd install_name_tool
  need_cmd codesign
  need_cmd lipo
  need_cmd shasum

  local deps_root="$BUILD_ROOT/deps"
  local src_root="$deps_root/src"
  local venv="$deps_root/venv"
  local stamp="$PREFIX/.varmint-dependencies"
  local minimum_macos
  minimum_macos="$(manifest_value runtime.minimum_macos)"

  local angle_repository angle_commit angle_source
  angle_repository="$(manifest_value dependencies.angle.repository)"
  angle_commit="$(manifest_value dependencies.angle.commit)"
  angle_source="$src_root/WebKit"

  local epoxy_repository epoxy_commit epoxy_source
  epoxy_repository="$(manifest_value dependencies.epoxy.repository)"
  epoxy_commit="$(manifest_value dependencies.epoxy.commit)"
  epoxy_source="$src_root/libepoxy"

  local moltenvk_repository moltenvk_commit moltenvk_version moltenvk_source
  moltenvk_repository="$(manifest_value dependencies.moltenvk.repository)"
  moltenvk_commit="$(manifest_value dependencies.moltenvk.commit)"
  moltenvk_version="$(manifest_value dependencies.moltenvk.version)"
  moltenvk_source="$src_root/MoltenVK"

  local virgl_repository virgl_commit virgl_source
  virgl_repository="$(manifest_value dependencies.virglrenderer.repository)"
  virgl_commit="$(manifest_value dependencies.virglrenderer.commit)"
  virgl_source="$src_root/virglrenderer"

  local expected_stamp
  expected_stamp="$(dependency_recipe_hash)"
  if [ "$FORCE_REBUILD" != 1 ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$expected_stamp" ]; then
    if (verify_dependency_prefix); then
      log "dependencies are already current"
      return
    fi
    log "dependency stamp matches, but the prefix is incomplete; rebuilding"
  fi

  mkdir -p "$src_root"
  begin_dependency_prefix_update
  build_angle "$angle_repository" "$angle_commit" "$angle_source" "$minimum_macos"
  build_epoxy "$epoxy_repository" "$epoxy_commit" "$epoxy_source"
  build_moltenvk "$moltenvk_repository" "$moltenvk_commit" "$moltenvk_version" "$moltenvk_source" "$minimum_macos"
  build_virglrenderer "$virgl_repository" "$virgl_commit" "$virgl_source" "$venv"
  verify_dependency_prefix
  printf '%s\n' "$expected_stamp" > "$stamp"
  commit_dependency_prefix_update
  log "dependency prefix ready: $PREFIX"
}

build_varmint() {
  need_cmd cargo
  need_cmd pkg-config
  need_cmd codesign
  need_file "$ENTITLEMENTS"
  while IFS= read -r dylib; do
    need_file "$PREFIX/lib/$dylib"
  done < <(manifest_lines runtime.dylibs)

  local cargo_target_dir="${CARGO_TARGET_DIR:-$BUILD_ROOT/cargo}"
  local output_dir="$BUILD_ROOT/release"
  local minimum_macos
  minimum_macos="$(manifest_value runtime.minimum_macos)"
  mkdir -p "$output_dir"
  local cargo_args=(build --release)
  if [ -f "$ROOT/Cargo.lock" ]; then
    cargo_args+=(--locked)
  fi

  log "build varmint"
  (
    cd "$ROOT"
    MACOSX_DEPLOYMENT_TARGET="$minimum_macos" \
    PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
    RUSTFLAGS="-L native=$PREFIX/lib ${RUSTFLAGS:-}" \
    CARGO_TARGET_DIR="$cargo_target_dir" \
      cargo "${cargo_args[@]}"
  )

  install -m 755 "$cargo_target_dir/release/varmint" "$BINARY"
  log "sign varmint binary"
  codesign --force --sign - --timestamp=none --entitlements "$ENTITLEMENTS" "$BINARY"
  codesign --verify --strict --verbose=2 "$BINARY"
  log "varmint binary ready: $BINARY"
}

build_icon() {
  local composer_stage="$BUILD_ROOT/runtime/Varmint.icon"
  local asset_output_dir="$BUILD_ROOT/runtime/icon-assets"
  local partial_plist="$BUILD_ROOT/runtime/icon-partial-info.plist"
  local minimum_macos

  need_cmd xcrun
  [ -d "$ICON_SOURCE" ] || die "missing Icon Composer asset: $ICON_SOURCE"
  need_file "$ICON_SOURCE/icon.json"
  minimum_macos="$(manifest_value runtime.minimum_macos)"

  rm -rf "$composer_stage" "$asset_output_dir"
  rm -f "$ICON" "$partial_plist"
  mkdir -p "$asset_output_dir" "$(dirname "$ICON")"
  cp -R "$ICON_SOURCE" "$composer_stage"

  xcrun actool "$composer_stage" \
    --compile "$asset_output_dir" \
    --output-format human-readable-text \
    --notices \
    --warnings \
    --errors \
    --output-partial-info-plist "$partial_plist" \
    --app-icon Varmint \
    --include-all-app-icons \
    --enable-on-demand-resources NO \
    --development-region en \
    --target-device mac \
    --minimum-deployment-target "$minimum_macos" \
    --platform macosx

  need_file "$asset_output_dir/Assets.car"
  install -m 644 "$asset_output_dir/Assets.car" "$ICON"
  xcrun --sdk macosx assetutil --info "$ICON" >/dev/null

  rm -rf "$composer_stage" "$asset_output_dir"
  rm -f "$partial_plist"
  log "app icon ready: $ICON"
}

assemble_app() {
  need_cmd python3
  need_cmd plutil
  need_file "$BINARY"
  need_file "$KERNEL"
  need_file "$INITRD"
  need_file "$ICON"
  need_file "$MANIFEST"

  local contents="$APP_BUNDLE/Contents"
  rm -rf "$APP_BUNDLE"
  mkdir -p "$contents/MacOS" "$contents/Frameworks" "$contents/Resources/kernel"
  install -m 755 "$BINARY" "$contents/MacOS/varmint"
  install -m 644 "$KERNEL" "$contents/Resources/kernel/Image"
  install -m 644 "$INITRD" "$contents/Resources/kernel/initrd"
  install -m 644 "$ICON" "$contents/Resources/Assets.car"
  install -m 644 "$MANIFEST" "$contents/Resources/runtime-manifest.toml"

  local version minimum_macos
  version="$(python3 - "$ROOT/Cargo.toml" <<'PY'
import sys
import tomllib
with open(sys.argv[1], "rb") as f:
    print(tomllib.load(f)["package"]["version"])
PY
)"
  minimum_macos="$(manifest_value runtime.minimum_macos)"

  cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Varmint</string>
    <key>CFBundleExecutable</key>
    <string>varmint</string>
    <key>CFBundleIdentifier</key>
    <string>dev.varmint.Varmint</string>
    <key>CFBundleIconName</key>
    <string>Varmint</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Varmint</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSMinimumSystemVersion</key>
    <string>$minimum_macos</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
  printf 'APPL????' > "$contents/PkgInfo"
  plutil -lint "$contents/Info.plist"
  log "app assembled: $APP_BUNDLE"
}

is_system_dependency() {
  case "$1" in
    /usr/lib/*|/System/Library/*) return 0 ;;
    *) return 1 ;;
  esac
}

list_rpaths() {
  local file="$1"
  otool -l "$file" | awk '
    $1 == "cmd" && $2 == "LC_RPATH" { in_rpath = 1; next }
    in_rpath && $1 == "path" {
      line = $0
      sub(/^[[:space:]]*path[[:space:]]+/, "", line)
      sub(/[[:space:]]+\(offset [0-9]+\)$/, "", line)
      print line
      in_rpath = 0
    }
  '
}

has_rpath() {
  local file="$1"
  local rpath="$2"
  list_rpaths "$file" | grep -Fxq "$rpath"
}

add_rpath() {
  local file="$1"
  local rpath="$2"
  has_rpath "$file" "$rpath" || install_name_tool -add_rpath "$rpath" "$file"
}

normalize_rpaths() {
  local file="$1"
  local wanted="$2"
  local rpath
  while IFS= read -r rpath; do
    [ -n "$rpath" ] || continue
    [ "$rpath" = "$wanted" ] || install_name_tool -delete_rpath "$rpath" "$file"
  done < <(list_rpaths "$file")
  add_rpath "$file" "$wanted"
}

resolve_dependency() {
  local dependency="$1"
  local basename="${dependency##*/}"

  case "$dependency" in
    "$PREFIX"/*)
      printf '%s\n' "$dependency"
      ;;
    @rpath/*|@loader_path/*|@executable_path/*)
      printf '%s\n' "$PREFIX/lib/$basename"
      ;;
    /*)
      if [ -f "$PREFIX/lib/$basename" ]; then
        printf '%s\n' "$PREFIX/lib/$basename"
      else
        die "non-system dependency is outside build prefix: $dependency"
      fi
      ;;
    *)
      printf '%s\n' "$PREFIX/lib/$basename"
      ;;
  esac
}

copy_runtime_library() {
  local source="$1"
  local queue="$2"
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local basename="$(basename "$source")"
  local destination="$frameworks/$basename"
  need_file "$source"
  if [ ! -f "$destination" ]; then
    cp -L "$source" "$destination"
    chmod u+w "$destination"
    printf '%s\n' "$destination" >> "$queue"
  fi
}

write_framework_info_plist() {
  local stem="$1"
  local plist="$2"
  cat > "$plist" <<EOF_PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>$stem</string>
  <key>CFBundleIdentifier</key>
  <string>dev.varmint.angle.$stem</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$stem</string>
  <key>CFBundlePackageType</key>
  <string>FMWK</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
EOF_PLIST
}

create_angle_frameworks() {
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local stem loose framework binary resources
  for stem in EGL GLESv2; do
    loose="$frameworks/lib${stem}.dylib"
    framework="$frameworks/${stem}.framework"
    binary="$framework/Versions/A/$stem"
    resources="$framework/Versions/A/Resources"
    need_file "$loose"

    rm -rf "$framework"
    mkdir -p "$resources"
    mv "$loose" "$binary"
    write_framework_info_plist "$stem" "$resources/Info.plist"

    ln -s A "$framework/Versions/Current"
    ln -s "Versions/Current/$stem" "$framework/$stem"
    ln -s Versions/Current/Resources "$framework/Resources"
    ln -s "${stem}.framework/Versions/Current/$stem" "$loose"
  done
}

process_macho() {
  local file="$1"
  local executable="$2"
  local queue="$3"
  local dependency source basename

  if [ "$file" != "$executable" ]; then
    install_name_tool -id "@rpath/$(basename "$file")" "$file"
  fi

  while IFS= read -r dependency; do
    [ -n "$dependency" ] || continue
    is_system_dependency "$dependency" && continue
    source="$(resolve_dependency "$dependency")"
    basename="$(basename "$source")"
    copy_runtime_library "$source" "$queue"
    install_name_tool -change "$dependency" "@rpath/$basename" "$file"
  done < <(otool -L "$file" | tail -n +2 | sed -E 's/^[[:space:]]*//; s/[[:space:]]+\(compatibility version.*$//')

  if [ "$file" = "$executable" ]; then
    normalize_rpaths "$file" '@executable_path/../Frameworks'
  else
    normalize_rpaths "$file" '@loader_path'
  fi
}

fix_rpaths() {
  need_cmd otool
  need_cmd install_name_tool

  local executable="$APP_BUNDLE/Contents/MacOS/varmint"
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local queue="$BUILD_ROOT/tmp/fix-rpaths.queue"
  local seen="$BUILD_ROOT/tmp/fix-rpaths.seen"
  need_file "$executable"
  mkdir -p "$frameworks" "$BUILD_ROOT/tmp"
  : > "$queue"
  : > "$seen"

  printf '%s\n' "$executable" >> "$queue"
  while IFS= read -r dylib; do
    copy_runtime_library "$PREFIX/lib/$dylib" "$queue"
  done < <(manifest_lines runtime.dylibs)

  while [ -s "$queue" ]; do
    local file next_queue
    file="$(sed -n '1p' "$queue")"
    next_queue="$queue.next"
    sed '1d' "$queue" > "$next_queue"
    mv "$next_queue" "$queue"
    grep -Fxq "$file" "$seen" && continue
    printf '%s\n' "$file" >> "$seen"
    process_macho "$file" "$executable" "$queue"
  done

  create_angle_frameworks
  log "rpaths fixed"
  otool -L "$executable"
}

sign_app() {
  need_cmd codesign
  need_file "$ENTITLEMENTS"
  need_file "$APP_BUNDLE/Contents/MacOS/varmint"

  log "sign app"
  find "$APP_BUNDLE/Contents/Frameworks" -maxdepth 1 -type f -name '*.dylib' -print0 \
    | while IFS= read -r -d '' dylib; do
        codesign --force --sign - --timestamp=none "$dylib"
      done

  local stem framework
  for stem in EGL GLESv2; do
    framework="$APP_BUNDLE/Contents/Frameworks/${stem}.framework"
    need_file "$framework/Versions/A/$stem"
    need_file "$framework/Versions/A/Resources/Info.plist"
    codesign --force --sign - --timestamp=none "$framework"
  done
  codesign --force --sign - --timestamp=none \
    --entitlements "$ENTITLEMENTS" \
    "$APP_BUNDLE/Contents/MacOS/varmint"
  codesign --force --sign - --timestamp=none \
    --entitlements "$ENTITLEMENTS" \
    "$APP_BUNDLE"
}

verify_bundle_dependency() {
  local owner="$1"
  local dependency="$2"
  local executable="$3"
  local frameworks="$4"
  local resolved

  is_system_dependency "$dependency" && return 0
  case "$dependency" in
    @rpath/*)
      resolved="$frameworks/${dependency##*/}"
      ;;
    @loader_path/*)
      resolved="$(dirname "$owner")/${dependency#@loader_path/}"
      ;;
    @executable_path/*)
      resolved="$(dirname "$executable")/${dependency#@executable_path/}"
      ;;
    *)
      die "$owner contains forbidden dependency: $dependency"
      ;;
  esac
  [ -f "$resolved" ] || die "$owner references missing dependency: $dependency"
}

verify_bundle_macho() {
  local file="$1"
  local executable="$2"
  local frameworks="$3"
  local dependency rpath id expected_id

  lipo -archs "$file" | tr ' ' '\n' | grep -qx "$ARCH" || die "$file does not contain $ARCH"

  while IFS= read -r dependency; do
    [ -n "$dependency" ] || continue
    verify_bundle_dependency "$file" "$dependency" "$executable" "$frameworks"
  done < <(otool -L "$file" | tail -n +2 | sed -E 's/^[[:space:]]*//; s/[[:space:]]+\(compatibility version.*$//')

  while IFS= read -r rpath; do
    case "$rpath" in
      '@executable_path/../Frameworks'|'@loader_path') ;;
      *) die "$file contains forbidden rpath: $rpath" ;;
    esac
  done < <(list_rpaths "$file")

  if [ "$file" != "$executable" ]; then
    case "$file" in
      "$frameworks/EGL.framework/Versions/A/EGL") expected_id='@rpath/libEGL.dylib' ;;
      "$frameworks/GLESv2.framework/Versions/A/GLESv2") expected_id='@rpath/libGLESv2.dylib' ;;
      *) expected_id="@rpath/$(basename "$file")" ;;
    esac
    id="$(otool -D "$file" | sed -n '2p')"
    [ "$id" = "$expected_id" ] || die "$file has invalid install name: $id"
  fi
}

verify_app() {
  local executable="$APP_BUNDLE/Contents/MacOS/varmint"
  local frameworks="$APP_BUNDLE/Contents/Frameworks"
  local resources="$APP_BUNDLE/Contents/Resources"
  local manifest="$resources/runtime-manifest.toml"
  local signed_entitlements dylib

  need_cmd python3
  need_cmd plutil
  need_cmd otool
  need_cmd lipo
  need_cmd codesign
  need_cmd xcrun
  need_file "$APP_BUNDLE/Contents/Info.plist"
  need_file "$executable"
  need_file "$resources/kernel/Image"
  need_file "$resources/kernel/initrd"
  need_file "$resources/Assets.car"
  need_file "$manifest"
  need_file "$frameworks/EGL.framework/Versions/A/EGL"
  need_file "$frameworks/EGL.framework/Versions/A/Resources/Info.plist"
  need_file "$frameworks/GLESv2.framework/Versions/A/GLESv2"
  need_file "$frameworks/GLESv2.framework/Versions/A/Resources/Info.plist"

  log "verify app"
  plutil -lint "$APP_BUNDLE/Contents/Info.plist" >/dev/null
  if plutil -extract CFBundleIconFile raw -o - "$APP_BUNDLE/Contents/Info.plist" >/dev/null 2>&1; then
    die "Info.plist still contains legacy CFBundleIconFile"
  fi
  [ "$(plutil -extract CFBundleIconName raw -o - "$APP_BUNDLE/Contents/Info.plist")" = "Varmint" ] \
    || die "Info.plist does not reference the Varmint asset icon"
  [ "$(plutil -extract LSMinimumSystemVersion raw -o - "$APP_BUNDLE/Contents/Info.plist")" = "$(manifest_value runtime.minimum_macos "$manifest")" ] \
    || die "Info.plist minimum macOS does not match runtime manifest"
  [ -s "$resources/Assets.car" ] || die "bundled app icon asset catalog is empty"
  xcrun --sdk macosx assetutil --info "$resources/Assets.car" >/dev/null
  plutil -lint "$frameworks/EGL.framework/Versions/A/Resources/Info.plist" >/dev/null
  plutil -lint "$frameworks/GLESv2.framework/Versions/A/Resources/Info.plist" >/dev/null

  [ "$(readlink "$frameworks/libEGL.dylib")" = 'EGL.framework/Versions/Current/EGL' ] \
    || die "libEGL.dylib is not linked to EGL.framework"
  [ "$(readlink "$frameworks/libGLESv2.dylib")" = 'GLESv2.framework/Versions/Current/GLESv2' ] \
    || die "libGLESv2.dylib is not linked to GLESv2.framework"

  [ -s "$resources/kernel/Image" ] || die "bundled kernel is empty"
  [ -s "$resources/kernel/initrd" ] || die "bundled initrd is empty"
  python3 - "$APP_BUNDLE" <<'PY'
from pathlib import Path
import sys

bundle = Path(sys.argv[1]).resolve()
for link in Path(sys.argv[1]).rglob("*"):
    if not link.is_symlink():
        continue
    try:
        target = link.resolve(strict=True)
    except FileNotFoundError:
        raise SystemExit(f"broken bundle symlink: {link}")
    if target != bundle and bundle not in target.parents:
        raise SystemExit(f"bundle symlink escapes app: {link} -> {target}")
PY

  verify_bundle_macho "$executable" "$executable" "$frameworks"
  while IFS= read -r -d '' dylib; do
    verify_bundle_macho "$dylib" "$executable" "$frameworks"
  done < <(find "$frameworks" -maxdepth 1 -type f -name '*.dylib' -print0)
  verify_bundle_macho "$frameworks/EGL.framework/Versions/A/EGL" "$executable" "$frameworks"
  verify_bundle_macho "$frameworks/GLESv2.framework/Versions/A/GLESv2" "$executable" "$frameworks"

  while IFS= read -r dylib; do
    need_file "$frameworks/$dylib"
  done < <(manifest_lines runtime.dylibs "$manifest")

  codesign --verify --strict --verbose=2 "$frameworks/EGL.framework"
  codesign --verify --strict --verbose=2 "$frameworks/GLESv2.framework"
  codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
  signed_entitlements="$(codesign -d --entitlements :- "$executable" 2>&1)"
  printf '%s\n' "$signed_entitlements" | grep -q '<key>com.apple.security.hypervisor</key>' \
    || die "main executable is missing the Hypervisor entitlement"
  if printf '%s\n' "$signed_entitlements" | grep -q '<key>com.apple.vm.networking</key>'; then
    die "ad-hoc bundle contains restricted com.apple.vm.networking entitlement"
  fi
}

main() {
  parse_args "$@"
  need_file "$MANIFEST"
  mkdir -p "$BUILD_ROOT" "$(dirname "$APP_BUNDLE")"

  if [ "$SKIP_DEPENDENCIES" != 1 ]; then
    build_dependencies
  else
    verify_dependency_prefix
  fi

  if [ "$DEPENDENCIES_ONLY" = 1 ]; then
    log "dependencies ready: $PREFIX"
    return
  fi

  [ -n "$KERNEL" ] || die "--kernel is required"
  [ -n "$INITRD" ] || die "--initrd is required"
  need_file "$KERNEL"
  need_file "$INITRD"
  build_varmint
  build_icon
  assemble_app
  fix_rpaths
  sign_app
  verify_app
  log "done: $APP_BUNDLE"
}

main "$@"

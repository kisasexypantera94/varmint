#!/usr/bin/env bash
set -euo pipefail

# Full varmint graphics stack:
#
#   WebKit/ANGLE   -> $HOME/dev/varmint-deps/angle-utm
#   UTM libepoxy   -> $HOME/dev/varmint-deps/libepoxy-utm
#   UTM MoltenVK   -> /opt/homebrew/opt/molten-vk-utm
#   UTM virgl      -> $HOME/dev/varmint-deps/virglrenderer-utm

VARMINT_DEPS="${VARMINT_DEPS:-$HOME/dev/varmint-deps}"
SRC_ROOT="${SRC_ROOT:-$VARMINT_DEPS/src}"
WORKDIR="${WORKDIR:-$HOME/dev/varmint-homebrew-build}"

ANGLE_PREFIX="${ANGLE_PREFIX:-$VARMINT_DEPS/angle-utm}"
EPOXY_PREFIX="${EPOXY_PREFIX:-$VARMINT_DEPS/libepoxy-utm}"
VIRGL_PREFIX="${VIRGL_PREFIX:-$VARMINT_DEPS/virglrenderer-utm}"

WEBKIT_REPO="${WEBKIT_REPO:-https://github.com/utmapp/WebKit.git}"
WEBKIT_COMMIT="${WEBKIT_COMMIT:-ed78ab6e1a37f4f11583a0bd038f22ec91f3ff10}"
WEBKIT_SRC="${WEBKIT_SRC:-$SRC_ROOT/WebKit-utm}"

EPOXY_REPO="${EPOXY_REPO:-https://github.com/utmapp/libepoxy.git}"
EPOXY_COMMIT="${EPOXY_COMMIT:-5014658f79e4d6872a1ad6754da9098ccd9d4fc5}"
EPOXY_SRC="${EPOXY_SRC:-$SRC_ROOT/libepoxy-utm}"

MVK_REPO="${MVK_REPO:-https://github.com/utmapp/MoltenVK.git}"
MVK_COMMIT="${MVK_COMMIT:-111c14f3abf5c00118fc7a5b00c92d7abbf40f62}"
MVK_SRC="${MVK_SRC:-$SRC_ROOT/MoltenVK-utm}"
MVK_VERSION="${MVK_VERSION:-1.4.2-utm-geometry}"
MVK_PACKAGE="$MVK_SRC/Package/Release/MoltenVK"
MVK_DYLIB="$MVK_PACKAGE/dynamic/dylib/macOS/libMoltenVK.dylib"
MVK_PREFIX="${MVK_PREFIX:-/opt/homebrew/opt/molten-vk-utm}"
MVK_TAP="${MVK_TAP:-local/varmint}"
MVK_TARBALL="$WORKDIR/molten-vk-utm-$MVK_VERSION.tar.gz"

VIRGL_REPO="${VIRGL_REPO:-https://github.com/utmapp/virglrenderer.git}"
VIRGL_COMMIT="${VIRGL_COMMIT:-d48a2d0d9a722fffd3f92c83e71d9426a4892a66}"
VIRGL_SRC="${VIRGL_SRC:-$SRC_ROOT/virglrenderer-utm}"

SDK="${SDK:-macosx}"
ARCH="${ARCH:-arm64}"
BUILD_CONFIGURATION="${BUILD_CONFIGURATION:-Release}"

ONLY_MVK="${ONLY_MVK:-0}"
MVK_BUILD_CONFIGURATION="${MVK_BUILD_CONFIGURATION:-Release}"
MVK_XCODE_SCHEME="${MVK_XCODE_SCHEME:-MoltenVK Package (macOS only)}"
MVK_BUILD_WITH_MAKE="${MVK_BUILD_WITH_MAKE:-0}"
MVK_MAKE_ARGS="${MVK_MAKE_ARGS:-}"

CLEAN="${CLEAN:-1}"
SKIP_ANGLE="${SKIP_ANGLE:-0}"
SKIP_EPOXY="${SKIP_EPOXY:-0}"
SKIP_MVK="${SKIP_MVK:-0}"
SKIP_VIRGL="${SKIP_VIRGL:-0}"

log() {
  printf '\n== %s ==\n' "$*"
}

die() {
  echo "error: $*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

need_file() {
  [ -f "$1" ] || die "missing file: $1"
}

codesign_if_exists() {
  local f="$1"
  [ -e "$f" ] || return 0
  codesign --force --sign - --timestamp=none "$f"
  codesign --verify --verbose=2 "$f"
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

  cd "$dir"

  git fetch origin \
    '+refs/heads/*:refs/remotes/origin/*' \
    '+refs/tags/*:refs/tags/*'

  if ! git cat-file -e "$commit^{tree}" 2>/dev/null; then
    git fetch origin "$commit" || true
  fi

  if ! git cat-file -e "$commit^{tree}" 2>/dev/null; then
    echo "Could not fetch commit tree: $commit" >&2
    echo "Repo: $repo" >&2
    echo "Try checking the pin or repo URL." >&2
    exit 1
  fi

  git checkout --detach "$commit"
  git rev-parse HEAD
}

write_angle_pkgconfig() {
  mkdir -p "$ANGLE_PREFIX/lib/pkgconfig"

  cat > "$ANGLE_PREFIX/lib/pkgconfig/angle.pc" <<EOF
prefix=$ANGLE_PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: angle
Description: UTM WebKit ANGLE
Version: utm-webkit-${WEBKIT_COMMIT:0:7}
Libs: -L\${libdir} -lEGL -lGLESv2
Cflags: -I\${includedir}
EOF

  cat > "$ANGLE_PREFIX/lib/pkgconfig/egl.pc" <<EOF
prefix=$ANGLE_PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: egl
Description: UTM ANGLE EGL
Version: utm-webkit-${WEBKIT_COMMIT:0:7}
Libs: -L\${libdir} -lEGL
Cflags: -I\${includedir}
EOF

  cat > "$ANGLE_PREFIX/lib/pkgconfig/glesv2.pc" <<EOF
prefix=$ANGLE_PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: glesv2
Description: UTM ANGLE GLESv2
Version: utm-webkit-${WEBKIT_COMMIT:0:7}
Libs: -L\${libdir} -lGLESv2
Cflags: -I\${includedir}
EOF
}

create_angle_framework_wrappers() {
  echo "== create ANGLE framework wrappers =="

  for stem in EGL GLESv2; do
    dylib="$ANGLE_PREFIX/lib/lib${stem}.dylib"
    fw="$ANGLE_PREFIX/lib/${stem}.framework"

    if [ ! -f "$dylib" ]; then
      echo "missing ANGLE dylib: $dylib" >&2
      exit 1
    fi

    mkdir -p "$fw/Versions/A"

    rm -f "$fw/Versions/A/$stem"
    ln -s "../../../lib${stem}.dylib" "$fw/Versions/A/$stem"

    rm -f "$fw/Versions/Current"
    ln -s "A" "$fw/Versions/Current"

    rm -f "$fw/$stem"
    ln -s "Versions/Current/$stem" "$fw/$stem"

    codesign --force --sign - --timestamp=none "$dylib" || true
    codesign --verify --verbose=2 "$dylib" || true
  done
}

build_angle() {
  log "ANGLE checkout"
  checkout_repo "$WEBKIT_REPO" "$WEBKIT_COMMIT" "$WEBKIT_SRC" >/dev/null

  local angle_dir="$WEBKIT_SRC/Source/ThirdParty/ANGLE"
  [ -d "$angle_dir" ] || die "missing ANGLE dir: $angle_dir"

  log "ANGLE build"
  cd "$angle_dir"

  if [ "$CLEAN" = "1" ]; then
    rm -rf ANGLE.xcarchive
  fi

  env -i PATH="$PATH" xcodebuild archive -archivePath "ANGLE" \
    -scheme "ANGLE" \
    -sdk "$SDK" \
    -arch "$ARCH" \
    -configuration "$BUILD_CONFIGURATION" \
    WEBCORE_LIBRARY_DIR="/usr/local/lib" \
    NORMAL_UMBRELLA_FRAMEWORKS_DIR="" \
    CODE_SIGNING_ALLOWED=NO \
    IPHONEOS_DEPLOYMENT_TARGET="14.0" \
    MACOSX_DEPLOYMENT_TARGET="11.0" \
    XROS_DEPLOYMENT_TARGET="1.0" \
    'OTHER_CFLAGS=$(inherited) -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall' \
    'OTHER_CPLUSPLUSFLAGS=$(inherited) -Wno-unnecessary-virtual-specifier -Wno-nontrivial-memcall'

  log "ANGLE install"
  rm -rf "$ANGLE_PREFIX"
  mkdir -p "$ANGLE_PREFIX/lib" "$ANGLE_PREFIX/include"

  rsync -a "ANGLE.xcarchive/Products/usr/local/lib/" "$ANGLE_PREFIX/lib/"
  rsync -a "include/" "$ANGLE_PREFIX/include/"

  need_file "$ANGLE_PREFIX/lib/libEGL.dylib"
  need_file "$ANGLE_PREFIX/lib/libGLESv2.dylib"

  install_name_tool -id "$ANGLE_PREFIX/lib/libEGL.dylib" "$ANGLE_PREFIX/lib/libEGL.dylib" || true
  install_name_tool -id "$ANGLE_PREFIX/lib/libGLESv2.dylib" "$ANGLE_PREFIX/lib/libGLESv2.dylib" || true

  write_angle_pkgconfig

  find "$ANGLE_PREFIX/lib" -type f \( -name '*.dylib' -o -perm +111 \) -print0 2>/dev/null \
    | while IFS= read -r -d '' f; do codesign_if_exists "$f"; done
  create_angle_framework_wrappers
}

build_epoxy() {
  log "libepoxy checkout"
  checkout_repo "$EPOXY_REPO" "$EPOXY_COMMIT" "$EPOXY_SRC" >/dev/null

  log "libepoxy build"
  cd "$EPOXY_SRC"

  if [ "$CLEAN" = "1" ]; then
    rm -rf build
  fi

  export PKG_CONFIG_PATH="$ANGLE_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export CFLAGS="-I$ANGLE_PREFIX/include ${CFLAGS:-}"
  export CPPFLAGS="-I$ANGLE_PREFIX/include ${CPPFLAGS:-}"
  export LDFLAGS="-L$ANGLE_PREFIX/lib -Wl,-rpath,$ANGLE_PREFIX/lib ${LDFLAGS:-}"

  meson setup build \
    --prefix "$EPOXY_PREFIX" \
    -Degl=yes \
    -Dglx=no \
    -Dx11=false \
    -Dtests=false

  meson compile -C build --verbose

  rm -rf "$EPOXY_PREFIX"
  meson install -C build

  need_file "$EPOXY_PREFIX/lib/libepoxy.0.dylib"

  codesign_if_exists "$EPOXY_PREFIX/lib/libepoxy.0.dylib"

  log "libepoxy verify EGL"
  PKG_CONFIG_PATH="$EPOXY_PREFIX/lib/pkgconfig:$ANGLE_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
    pkg-config --variable=epoxy_has_egl epoxy | grep -qx '1'
}

build_moltenvk_source() {
  log "MoltenVK checkout"
  checkout_repo "$MVK_REPO" "$MVK_COMMIT" "$MVK_SRC" >/dev/null

  log "MoltenVK build"
  cd "$MVK_SRC"

  if [ ! -f fetchDependencies ]; then
    die "MoltenVK checkout has no fetchDependencies script"
  fi

  env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" ./fetchDependencies --macos -v

  if [ "$MVK_BUILD_WITH_MAKE" = "1" ]; then
    # shellcheck disable=SC2086
    env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" \
      make macos $MVK_MAKE_ARGS
  else
    rm -rf "Package/$MVK_BUILD_CONFIGURATION"

    env -i PATH="$PATH" HOME="$HOME" LANG="${LANG:-en_US.UTF-8}" \
      xcodebuild build \
        -project MoltenVKPackaging.xcodeproj \
        -scheme "$MVK_XCODE_SCHEME" \
        -configuration "$MVK_BUILD_CONFIGURATION" \
        -sdk "$SDK" \
        -arch "$ARCH" \
        CODE_SIGNING_ALLOWED=NO
  fi

  MVK_PACKAGE="$MVK_SRC/Package/$MVK_BUILD_CONFIGURATION/MoltenVK"
  MVK_DYLIB="$MVK_PACKAGE/dynamic/dylib/macOS/libMoltenVK.dylib"

  need_file "$MVK_DYLIB"

  log "MoltenVK built"
  echo "MVK_BUILD_CONFIGURATION=$MVK_BUILD_CONFIGURATION"
  echo "MVK_PACKAGE=$MVK_PACKAGE"
  echo "MVK_DYLIB=$MVK_DYLIB"
}

install_moltenvk_brew_keg() {
  log "MoltenVK Homebrew keg"

  unset HOMEBREW_NO_INSTALL_FROM_API
  export HOMEBREW_NO_AUTO_UPDATE=1

  mkdir -p "$WORKDIR"

  rm -f "$MVK_TARBALL"
  tar -C "$(dirname "$MVK_PACKAGE")" -czf "$MVK_TARBALL" "$(basename "$MVK_PACKAGE")"

  local sha
  sha="$(shasum -a 256 "$MVK_TARBALL" | awk '{print $1}')"

  if ! brew --repo "$MVK_TAP" >/dev/null 2>&1; then
    brew tap-new "$MVK_TAP"
  fi

  local tap_repo
  tap_repo="$(brew --repo "$MVK_TAP")"
  mkdir -p "$tap_repo/Formula"

  cat > "$tap_repo/Formula/molten-vk-utm.rb" <<RUBY
class MoltenVkUtm < Formula
  desc "UTM MoltenVK geometry-shader build for varmint"
  homepage "https://github.com/utmapp/MoltenVK"
  url "file://$MVK_TARBALL"
  sha256 "$sha"
  version "$MVK_VERSION"
  license "Apache-2.0"

  keg_only "varmint uses this side-by-side with other Vulkan loaders"

  def install
    candidates = [
      buildpath,
      buildpath/"MoltenVK",
    ]

    mvk = candidates.find do |dir|
      (dir/"dynamic/dylib/macOS/libMoltenVK.dylib").exist?
    end

    odie "Could not find libMoltenVK.dylib under #{buildpath}" if mvk.nil?

    lib.install mvk/"dynamic/dylib/macOS/libMoltenVK.dylib"

    if (mvk/"include").exist?
      include.install (mvk/"include").children
    end

    system "install_name_tool", "-id", "#{opt_lib}/libMoltenVK.dylib", "#{lib}/libMoltenVK.dylib"

    (lib/"pkgconfig").mkpath

    (lib/"pkgconfig/MoltenVK.pc").write <<~EOS
      prefix=#{opt_prefix}
      exec_prefix=\${prefix}
      libdir=\${prefix}/lib
      includedir=\${prefix}/include

      Name: MoltenVK
      Description: UTM MoltenVK geometry-shader build
      Version: #{version}
      Libs: -L\${libdir} -lMoltenVK
      Cflags: -I\${includedir}
    EOS

    (lib/"pkgconfig/vulkan.pc").write <<~EOS
      prefix=#{opt_prefix}
      exec_prefix=\${prefix}
      libdir=\${prefix}/lib
      includedir=\${prefix}/include

      Name: Vulkan
      Description: Vulkan loader alias backed by UTM MoltenVK for varmint
      Version: #{version}
      Libs: -L\${libdir} -lMoltenVK
      Cflags: -I\${includedir}
    EOS

    (share/"vulkan/icd.d").mkpath
    (share/"vulkan/icd.d/MoltenVK_utm_icd.json").write <<~JSON
      {
        "file_format_version": "1.0.0",
        "ICD": {
          "library_path": "#{opt_lib}/libMoltenVK.dylib",
          "api_version": "1.3.0"
        }
      }
    JSON
  end
end
RUBY

  brew trust --formula "$MVK_TAP/molten-vk-utm" || true
  brew uninstall --force "$MVK_TAP/molten-vk-utm" || true
  rm -rf "$(brew --prefix)/Cellar/molten-vk-utm"

  brew install --build-from-source "$MVK_TAP/molten-vk-utm"

  MVK_PREFIX="$(brew --prefix "$MVK_TAP/molten-vk-utm")"
  need_file "$MVK_PREFIX/lib/libMoltenVK.dylib"
  codesign_if_exists "$MVK_PREFIX/lib/libMoltenVK.dylib"
}

build_virglrenderer() {
  log "virglrenderer checkout"
  checkout_repo "$VIRGL_REPO" "$VIRGL_COMMIT" "$VIRGL_SRC" >/dev/null

  log "virglrenderer build"
  cd "$VIRGL_SRC"

  if [ "$CLEAN" = "1" ]; then
    rm -rf build
  fi

  python3 - <<'PY'
import importlib.util
import subprocess
import sys

if importlib.util.find_spec("yaml") is None:
    subprocess.check_call([
        sys.executable,
        "-m",
        "pip",
        "install",
        "--break-system-packages",
        "pyyaml",
    ])
PY

  export PKG_CONFIG_PATH="$EPOXY_PREFIX/lib/pkgconfig:$ANGLE_PREFIX/lib/pkgconfig:$MVK_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export CFLAGS="-I$ANGLE_PREFIX/include -I$EPOXY_PREFIX/include -I$MVK_PREFIX/include ${CFLAGS:-}"
  export CPPFLAGS="-I$ANGLE_PREFIX/include -I$EPOXY_PREFIX/include -I$MVK_PREFIX/include ${CPPFLAGS:-}"
  export LDFLAGS="-L$ANGLE_PREFIX/lib -L$EPOXY_PREFIX/lib -L$MVK_PREFIX/lib -Wl,-rpath,$ANGLE_PREFIX/lib -Wl,-rpath,$EPOXY_PREFIX/lib -Wl,-rpath,$MVK_PREFIX/lib ${LDFLAGS:-}"

  meson setup build \
    --prefix "$VIRGL_PREFIX" \
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

  rm -rf "$VIRGL_PREFIX"
  meson install -C build

  mkdir -p "$VIRGL_PREFIX/share/varmint"
  git rev-parse HEAD > "$VIRGL_PREFIX/share/varmint/virglrenderer-commit.txt"

  local dylib="$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"
  need_file "$dylib"

  for rpath in "$ANGLE_PREFIX/lib" "$EPOXY_PREFIX/lib" "$MVK_PREFIX/lib"; do
    if ! otool -l "$dylib" | grep -Fq "path $rpath"; then
      install_name_tool -add_rpath "$rpath" "$dylib"
    fi
  done

  codesign_if_exists "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib"
  codesign_if_exists "$VIRGL_PREFIX/libexec/virgl_render_server"
  codesign_if_exists "$VIRGL_PREFIX/bin/virgl_test_server"
}

write_env() {
  log "write env helper"

  cat > "$VARMINT_DEPS/env.sh" <<EOF
export VIRGL_UTM="$VIRGL_PREFIX"
export EPOXY_UTM="$EPOXY_PREFIX"
export ANGLE_UTM="$ANGLE_PREFIX"
export MVK_UTM="$MVK_PREFIX"

export PKG_CONFIG_PATH="\$VIRGL_UTM/lib/pkgconfig:\$EPOXY_UTM/lib/pkgconfig:\$ANGLE_UTM/lib/pkgconfig:\$MVK_UTM/lib/pkgconfig:\${PKG_CONFIG_PATH:-}"
export DYLD_LIBRARY_PATH="\$VIRGL_UTM/lib:\$EPOXY_UTM/lib:\$ANGLE_UTM/lib:\$MVK_UTM/lib:\${DYLD_LIBRARY_PATH:-}"
export DYLD_FRAMEWORK_PATH="\$ANGLE_UTM/lib:\${DYLD_FRAMEWORK_PATH:-}"
export RUSTFLAGS="-L \$VIRGL_UTM/lib -L \$EPOXY_UTM/lib -L \$ANGLE_UTM/lib -L \$MVK_UTM/lib -L /opt/homebrew/lib \${RUSTFLAGS:-}"
EOF

  echo "$VARMINT_DEPS/env.sh"
}

verify() {
  log "verify"

  echo "ANGLE_PREFIX=$ANGLE_PREFIX"
  echo "EPOXY_PREFIX=$EPOXY_PREFIX"
  echo "MVK_PREFIX=$MVK_PREFIX"
  echo "VIRGL_PREFIX=$VIRGL_PREFIX"

  echo
  echo "ANGLE dylibs:"
  otool -L "$ANGLE_PREFIX/lib/libEGL.dylib" | head -20
  otool -L "$ANGLE_PREFIX/lib/libGLESv2.dylib" | head -30

  echo
  echo "libepoxy EGL flag:"
  PKG_CONFIG_PATH="$EPOXY_PREFIX/lib/pkgconfig:$ANGLE_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}" \
    pkg-config --variable=epoxy_has_egl epoxy

  echo
  echo "virglrenderer linked dylibs:"
  otool -L "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib" \
    | grep -Ei 'virgl|MoltenVK|epoxy|EGL|GLES|vulkan|System' || true

  echo
  echo "virglrenderer exports:"
  nm -gU "$VIRGL_PREFIX/lib/libvirglrenderer.1.dylib" \
    | grep -E 'virgl_renderer_init|virgl_renderer_submit_cmd|virgl_renderer_context_create_with_flags|virgl_renderer_resource_create_blob|virgl_renderer_resource_map|virgl_renderer_resource_get_info_ext|virgl_renderer_get_cap_set' || true
}

main() {
  need_cmd git
  need_cmd xcodebuild
  need_cmd meson
  need_cmd ninja
  need_cmd pkg-config
  need_cmd brew
  need_cmd codesign
  need_cmd install_name_tool
  need_cmd otool
  need_cmd nm
  need_cmd rsync
  need_cmd python3

  mkdir -p "$SRC_ROOT" "$WORKDIR" "$VARMINT_DEPS"

  if [ "$ONLY_MVK" = "1" ]; then
    SKIP_ANGLE=1
    SKIP_EPOXY=1
    SKIP_MVK=0
    SKIP_VIRGL=1
  fi

  log "inputs"
  echo "VARMINT_DEPS=$VARMINT_DEPS"
  echo "WEBKIT_SRC=$WEBKIT_SRC"
  echo "ANGLE_PREFIX=$ANGLE_PREFIX"
  echo "EPOXY_PREFIX=$EPOXY_PREFIX"
  echo "MVK_SRC=$MVK_SRC"
  echo "MVK_PREFIX=$MVK_PREFIX"
  echo "VIRGL_SRC=$VIRGL_SRC"
  echo "VIRGL_PREFIX=$VIRGL_PREFIX"
  echo "ONLY_MVK=$ONLY_MVK"
  echo "MVK_BUILD_CONFIGURATION=$MVK_BUILD_CONFIGURATION"
  echo "MVK_XCODE_SCHEME=$MVK_XCODE_SCHEME"
  echo "SDK=$SDK ARCH=$ARCH BUILD_CONFIGURATION=$BUILD_CONFIGURATION"

  [ "$SKIP_ANGLE" = "1" ] || build_angle
  [ "$SKIP_EPOXY" = "1" ] || build_epoxy
  if [ "$SKIP_MVK" != "1" ]; then
    build_moltenvk_source
    install_moltenvk_brew_keg
  fi
  [ "$SKIP_VIRGL" = "1" ] || build_virglrenderer

  write_env
  verify

  log "done"
}

main "$@"
